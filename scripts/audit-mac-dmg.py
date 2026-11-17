#!/usr/bin/env python3
"""Exercise a delivered development DMG in its own profile, never the owner profile.

This audits the real packaged process and its public caches; it does not click UI
controls or infer all-feature acceptance from startup. Uses no account or key.
"""
import datetime
import hashlib
import json
import plistlib
from pathlib import Path
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
from urllib.parse import urlparse
import uuid

ROOT = Path(__file__).resolve().parents[1]
DMG = ROOT / 'target/debug/bundle/dmg/Personal Command Center_0.5.0_arm64.dmg'
OUT = ROOT / 'target/final-dmg-audit'


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def snapshot(path):
    with sqlite3.connect(path.as_uri() + '?mode=ro', uri=True, timeout=3) as db:
        settings = dict(db.execute('SELECT key,value FROM settings'))
        sources = json.loads(settings.get('discovery_hub_streams', '[]'))
        caches = {source: json.loads(raw) for source, raw in db.execute(
            "SELECT source,payload FROM feed_cache WHERE source LIKE 'hub:%'")}
        return {
            'batch': json.loads(settings.get('discovery_hub_last_refresh', 'null')),
            'configured_sources': len(sources),
            'auto_refresh_minutes': settings.get('discovery_hub_auto_refresh'),
            'sources': [{'stream': s, 'health': json.loads(settings.get('hub_health:' + s['id'], 'null')),
                         'items': caches.get('hub:' + s['id'], [])} for s in sources],
            'live_item_count': sum(map(len, caches.values())),
            'ordinary_item_count': db.execute('SELECT COUNT(*) FROM items').fetchone()[0],
            'workspace_record_count': db.execute('SELECT COUNT(*) FROM records').fetchone()[0],
            'enabled_account_flags': [k for k, v in settings.items() if k.endswith('_enabled') and v == 'true'],
        }


def run():
    if sys.platform != 'darwin':
        raise SystemExit('The DMG audit runs on macOS only')
    OUT.mkdir(parents=True, exist_ok=True)
    suffix = uuid.uuid4().hex[:10]
    profile = 'dmg-' + suffix
    identifier = 'com.personal.commandcenter.qa.' + profile
    profile_dir = Path.home() / 'Library/Application Support' / identifier
    if profile_dir.exists():
        raise SystemExit('Expected a fresh isolated QA profile')
    db_path = profile_dir / 'command-center.db'
    mount = Path(tempfile.mkdtemp(prefix='pcc-dmg-audit.', dir='/private/tmp'))
    copied = OUT / suffix / 'Personal Command Center.app'
    report = {'checked_at': datetime.datetime.now(datetime.timezone.utc).isoformat(),
              'source_revision': subprocess.check_output(['git', 'log', '-1', '--format=%H', '--', 'apps/desktop', 'crates/core'], cwd=ROOT, text=True).strip(),
              'dmg_sha256': sha(DMG), 'profile': identifier, 'normal_profile_accessed': False,
              'app_login_used': False, 'api_keys_used': False, 'interactive_ui_automation_available': False,
              'all_use_cases_passed': False, 'mounted': False}
    process = None
    logfile = (OUT / 'packaged-process.log').open('w')
    try:
        subprocess.run(['hdiutil', 'verify', str(DMG)], check=True, stdout=logfile, stderr=subprocess.STDOUT)
        subprocess.run(['hdiutil', 'attach', '-readonly', '-nobrowse', '-mountpoint', str(mount), str(DMG)], check=True, stdout=logfile, stderr=subprocess.STDOUT)
        report['mounted'] = True
        bundle = mount / 'Personal Command Center.app'
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(bundle)], check=True, stdout=logfile, stderr=subprocess.STDOUT)
        with (bundle / 'Contents/Info.plist').open('rb') as stream:
            info = plistlib.load(stream)
        report['version'] = info['CFBundleShortVersionString']
        executable = Path('Contents/MacOS') / info['CFBundleExecutable']
        original_hash = sha(bundle / executable)
        subprocess.run(['ditto', str(bundle), str(copied)], check=True)
        subprocess.run(['codesign', '--verify', '--deep', '--strict', str(copied)], check=True, stdout=logfile, stderr=subprocess.STDOUT)
        assert original_hash == sha(copied / executable)
        assert (mount / 'Applications').readlink() == Path('/Applications')
        report.update(executable_sha256=original_hash, copied_executable_matches_dmg=True,
                      signature_verified=True, applications_shortcut_verified=True)
        process = subprocess.Popen([str(copied / executable), '--qa-profile', profile], stdout=logfile, stderr=subprocess.STDOUT)
        deadline = time.monotonic() + 180
        while time.monotonic() < deadline:
            if process.poll() is not None:
                raise RuntimeError(f'Packaged app exited: {process.returncode}')
            if db_path.exists():
                current = snapshot(db_path)
                if current['batch']:
                    break
            time.sleep(2)
        else:
            raise RuntimeError('No completed public batch within 180 seconds')
        enabled = [s for s in current['sources'] if s['stream']['enabled']]
        report['bootstrap'] = {k: v for k, v in current.items() if k != 'sources'}
        report['sources'] = [{**{k: v for k, v in s.items() if k != 'items'}, 'count': len(s['items']), 'samples': s['items'][:2]} for s in current['sources']]
        (OUT / 'public-caches.json').write_text(json.dumps(current, indent=2) + '\n')
        assert len(enabled) == 35
        assert current['batch']['refreshed'] == 35 and current['batch']['failed'] == 0
        assert not current['ordinary_item_count'] and not current['workspace_record_count'] and not current['enabled_account_flags']
        problems = []
        for source in enabled:
            assert source['health']['status'] == 'ok'
            for row in source['items']:
                url = urlparse(row.get('url', ''))
                if not row.get('id') or not row.get('title') or url.scheme not in ('https', 'http') or not url.hostname or url.username or url.password:
                    problems.append({'source': source['stream']['id'], 'id': row.get('id'), 'issue': 'missing identity/title or invalid public link'})
        report['data_quality_problems'] = problems
        assert not problems
        for source_id, region in [('app-charts-us', 'US'), ('app-charts-india', 'IN')]:
            rows = next(s['items'] for s in enabled if s['stream']['id'] == source_id)
            assert len(rows) == 50
            assert all(r['id'].startswith('apple:' + region.lower() + ':') and r['metric'] == f'#{i + 1} · top free · {region}' for i, r in enumerate(rows))
        process.terminate(); process.wait(timeout=15); process = None
        # Explicit fixture policy change while the test process is stopped. This
        # tests restored/off startup policy and exact cache persistence, not UI clicks.
        with sqlite3.connect(db_path) as db:
            db.execute("UPDATE settings SET value='0' WHERE key='discovery_hub_auto_refresh'")
        process = subprocess.Popen([str(copied / executable), '--qa-profile', profile], stdout=logfile, stderr=subprocess.STDOUT)
        time.sleep(8)
        assert process.poll() is None
        after = snapshot(db_path)
        assert after['auto_refresh_minutes'] == '0'
        assert after['sources'] == current['sources'] and after['batch'] == current['batch']
        report.update(restart_passed=True, cached_payloads_unchanged=True, explicit_off_policy_preserved=True,
                      native_launch_and_local_workflows_passed=True)
        delivery = ROOT / 'dist/installers' / DMG.name
        shutil.copy2(DMG, delivery)
        assert sha(delivery) == report['dmg_sha256']
        report['delivery_sha256'] = sha(delivery)
    finally:
        if process is not None and process.poll() is None:
            process.terminate(); process.wait(timeout=15)
        if report['mounted']:
            subprocess.run(['hdiutil', 'detach', str(mount)], check=True, stdout=logfile, stderr=subprocess.STDOUT)
        mount.rmdir(); logfile.close()
        (OUT / 'results.json').write_text(json.dumps(report, indent=2) + '\n')
    print(json.dumps({k: v for k, v in report.items() if k != 'sources'}, indent=2))


if __name__ == '__main__':
    run()
