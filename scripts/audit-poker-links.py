"""Read-only live link verification. Never authenticate or rotate on refusals."""
import concurrent.futures
import datetime
import html
import ipaddress
import json
from pathlib import Path
import re
import socket
import time
from urllib.error import HTTPError
from urllib.parse import urlparse
from urllib.request import HTTPRedirectHandler, Request, build_opener

ROOT = Path(__file__).resolve().parents[1]
DATA = ROOT / 'target/poker-live'
hub = json.loads((DATA / 'native-schedules.json').read_text())
items = json.loads((DATA / 'visible-tournaments.json').read_text())


def public(url):
    u = urlparse(url)
    if u.scheme != 'https' or u.username or u.password or not u.hostname:
        raise ValueError('Not a public HTTPS schedule')
    if any(not ipaddress.ip_address(a[4][0]).is_global for a in socket.getaddrinfo(u.hostname, 443)):
        raise ValueError('Non-public address')


class Redirects(HTTPRedirectHandler):
    max_redirections = 4
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        public(newurl)
        return super().redirect_request(req, fp, code, msg, headers, newurl)


def check_group(rows):
    results = []
    paused = False
    for row in rows:
        url = row['url']
        if paused:
            results.append({**row, 'verified': False, 'error': 'Not retried after provider access/rate refusal'})
            continue
        for attempt in range(2):
            try:
                public(url)
                with build_opener(Redirects()).open(Request(url, headers={'User-Agent': 'PersonalCommandCenter/0.5 (user-authorized feed reader)'}), timeout=12) as response:
                    body = response.read(128 * 1024).decode('utf-8', errors='replace')
                    title = re.search(r'<title[^>]*>(.*?)</title>', body, re.I | re.S)
                    title = html.unescape(re.sub(r'\s+', ' ', title.group(1))).strip() if title else ''
                    final = response.url
                    blocked = bool(re.search(r'just a moment|access denied|page not found|404 not found', title, re.I))
                    results.append({**row, 'status': response.status, 'final_url': final, 'page_title': title[:300], 'verified': not blocked,
                                    'reader_origin_compatible': urlparse(final).hostname.removeprefix('www.') == urlparse(url).hostname.removeprefix('www.')})
                break
            except HTTPError as error:
                paused = error.code in (401, 403, 429) or error.headers.get('Retry-After') is not None
                if not paused and error.code in (500, 502, 503, 504) and attempt == 0:
                    time.sleep(1)
                    continue
                results.append({**row, 'status': error.code, 'verified': False, 'error': str(error)})
                break
            except Exception as error:
                results.append({**row, 'verified': False, 'error': str(error)})
                break
        time.sleep(0.15)
    return results


verified = []
pending = {}
seen = set()
for item in items:
    url = item['url']
    if url in seen:
        continue
    seen.add(url)
    evidence = item.get('location_evidence') or {}
    if evidence.get('http_status') == 200 and not evidence.get('detail_error'):
        verified.append({'url': url, 'kind': 'event', 'verified': True, 'status': 200,
                         'evidence': 'Native public-client detail GET during this live refresh', 'reader_origin_compatible': True})
    else:
        pending.setdefault(urlparse(url).hostname, []).append({'url': url, 'kind': 'event'})
for source in hub['streams']:
    url = source['stream']['query']
    if url not in seen:
        seen.add(url)
        pending.setdefault(urlparse(url).hostname, []).append({'url': url, 'kind': 'provider'})
with concurrent.futures.ThreadPoolExecutor(max_workers=3) as pool:
    for result in pool.map(check_group, pending.values()):
        verified.extend(result)
report = {'checked_at': datetime.datetime.now(datetime.timezone.utc).isoformat(), 'accounts_used': False,
          'api_keys_used': False, 'links': verified, 'checked': len(verified),
          'verified': sum(r['verified'] for r in verified),
          'failed': [r for r in verified if not r['verified']],
          'reader_origin_changes': [r for r in verified if r.get('reader_origin_compatible') is False],
          'interactive_ui_tested': False}
(DATA / 'link-results.json').write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps({k: v for k, v in report.items() if k != 'links'}, indent=2))
