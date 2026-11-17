"""Read only the fresh app profile on a disposable Windows CI runner."""
import datetime
import json
import os
from pathlib import Path
import sqlite3
import time

if os.name != 'nt' or os.environ.get('GITHUB_ACTIONS') != 'true':
    raise SystemExit('This audit runs only on an isolated Windows Actions runner')
paths = [Path(os.environ[k]) / 'com.personal.commandcenter/command-center.db'
         for k in ('APPDATA', 'LOCALAPPDATA') if os.environ.get(k)]
out = Path('target/release/bundle/windows-public-feed-smoke.json')
report = {'checked_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
          'isolated_runner': True, 'app_login_used': False, 'api_keys_used': False,
          'full_feature_acceptance': False, 'bootstrap_passed': False}
deadline = time.monotonic() + 90
while time.monotonic() < deadline:
    path = next((p for p in paths if p.exists()), None)
    if path:
        try:
            with sqlite3.connect(path.as_uri() + '?mode=ro', uri=True, timeout=2) as db:
                def setting(key):
                    row = db.execute('SELECT value FROM settings WHERE key=?', (key,)).fetchone()
                    return row[0] if row else None
                streams = json.loads(setting('discovery_hub_streams') or '[]')
                summary = json.loads(setting('discovery_hub_last_refresh') or 'null')
                health = [{'id': source['id'], 'provider': source['provider'],
                           'health': json.loads(setting('hub_health:' + source['id']) or 'null')}
                          for source in streams if source.get('enabled')]
                caches = db.execute("SELECT source,payload FROM feed_cache WHERE source LIKE 'hub:%'").fetchall()
                by_source = {source: json.loads(raw) for source, raw in caches}
                charts = []
                for chart_id, region in [('app-charts-us', 'US'), ('app-charts-india', 'IN')]:
                    rows = by_source.get('hub:' + chart_id, [])
                    valid = len(rows) == 50 and all(
                        r.get('metric') == f'#{index + 1} · top free · {region}'
                        and r.get('metadata', {}).get('storefront') == region
                        and r.get('metadata', {}).get('feed_url', '').startswith('https://')
                        for index, r in enumerate(rows))
                    charts.append({'id': chart_id, 'count': len(rows), 'passed': valid,
                                   'metadata': rows[0].get('metadata') if rows else None,
                                   'samples': rows[:3]})
                ordinary = db.execute('SELECT COUNT(*) FROM items').fetchone()[0]
                records = db.execute('SELECT COUNT(*) FROM records').fetchone()[0]
                enabled = [s for s in streams if s.get('enabled')]
                flags = db.execute("SELECT key FROM settings WHERE key LIKE '%_enabled' AND value='true'").fetchall()
                report.update(configured_sources=len(streams), enabled_sources=len(enabled),
                              auto_refresh_minutes=setting('discovery_hub_auto_refresh'),
                              initialization_marker=setting('public_discovery_initialized'),
                              live_cache_count=len(caches), live_item_count=sum(len(json.loads(raw)) for _, raw in caches),
                              ordinary_item_count=ordinary, workspace_record_count=records,
                              enabled_account_flags=[r[0] for r in flags], batch=summary, sources=health,
                              apple_charts=charts, apple_charts_passed=all(c['passed'] for c in charts),
                              failed_sources=[r for r in health if r['health'] and r['health'].get('status') == 'error'])
                report['bootstrap_passed'] = bool(len(enabled) == 35 and report['live_item_count'] > 0
                    and ordinary == 0 and records == 0 and not flags
                    and report['auto_refresh_minutes'] == '30' and report['initialization_marker'])
                if summary:
                    report['all_public_sources_succeeded'] = summary.get('refreshed') == 35 and summary.get('failed') == 0
                    break
        except (sqlite3.Error, ValueError) as error:
            report['last_read_error'] = str(error)
    time.sleep(2)
out.parent.mkdir(parents=True, exist_ok=True)
out.write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(report, indent=2))
if not report['bootstrap_passed']:
    raise SystemExit('No verified no-account live bootstrap within 90 seconds')
if not report.get('apple_charts_passed'):
    raise SystemExit('Installed app did not load 50 correctly ranked US and India Apple chart entries')
