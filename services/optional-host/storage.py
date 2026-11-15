"""Versioned state storage. Local durable volume or optional Google Cloud Storage.
No desktop database, decrypted vault data or encryption key belongs in this store.
"""
import base64
import hashlib
import json
import os
import pathlib
import tempfile
import urllib.error
import urllib.parse
import urllib.request
import time
import threading
from contextlib import contextmanager

class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl): return None
HTTP = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())

class Conflict(Exception): pass
class NotFound(Exception): pass

class Storage:
    def __init__(self):
        self.bucket = os.environ.get('PCC_STATE_BUCKET', '')
        self.root = pathlib.Path(os.environ.get('PCC_STATE_DIRECTORY', '/data'))
        self.token = None
        self.token_until = 0
        self.token_lock = threading.Lock()
        if not self.bucket:
            self.root.mkdir(parents=True, exist_ok=True, mode=0o700)

    def _access_token(self):
        with self.token_lock:
            if self.token and time.monotonic() < self.token_until:
                return self.token
            # Fixed Google metadata endpoint, never caller-controlled.
            request = urllib.request.Request('http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token', headers={'Metadata-Flavor': 'Google'})
            with HTTP.open(request, timeout=5) as response:
                result = json.loads(response.read(32768))
            self.token = result['access_token']
            self.token_until = time.monotonic() + max(1, int(result['expires_in']) - 60)
            return self.token

    def _url(self, key, upload=False):
        bucket = urllib.parse.quote(self.bucket, safe='')
        name = urllib.parse.quote(key, safe='')
        if upload:
            return f'https://storage.googleapis.com/upload/storage/v1/b/{bucket}/o?uploadType=media&name={name}'
        return f'https://storage.googleapis.com/storage/v1/b/{bucket}/o/{name}'

    def _cloud(self, url, method='GET', data=None):
        req = urllib.request.Request(url, data=data, method=method, headers={'Authorization': 'Bearer '+self._access_token(), 'Content-Type':'application/json'})
        try:
            with HTTP.open(req, timeout=20) as response:
                body = response.read(12 * 1024 * 1024 + 1)
                if len(body) > 12 * 1024 * 1024: raise ValueError('Stored object exceeds limit')
                return body, response.headers
        except urllib.error.HTTPError as error:
            if error.code == 404: raise NotFound() from None
            if error.code == 412: raise Conflict() from None
            raise RuntimeError('State storage request failed') from None

    def _path(self, key):
        # Logical keys never become user-selected paths.
        return self.root / (hashlib.sha256(key.encode()).hexdigest()+'.json')

    @contextmanager
    def _lock(self, key):
        import fcntl
        path = self._path(key).with_suffix('.lock')
        with open(path, 'a+b') as lock:
            os.chmod(path, 0o600)
            fcntl.flock(lock.fileno(), fcntl.LOCK_EX)
            try: yield
            finally: fcntl.flock(lock.fileno(), fcntl.LOCK_UN)

    def read(self, key):
        if self.bucket:
            # Metadata gives a generation; pin the media read to that immutable generation.
            raw, _ = self._cloud(self._url(key))
            meta = json.loads(raw)
            version = str(meta['generation'])
            raw, _ = self._cloud(self._url(key)+'?alt=media&generation='+version)
            return json.loads(raw), version
        try:
            envelope = json.loads(self._path(key).read_bytes())
            return envelope['data'], str(envelope['version'])
        except FileNotFoundError: raise NotFound() from None

    def write(self, key, data, expected):
        raw = json.dumps(data, separators=(',', ':'), ensure_ascii=False).encode()
        if len(raw) > 10 * 1024 * 1024: raise ValueError('State exceeds 10 MiB')
        if self.bucket:
            response, _ = self._cloud(self._url(key, True)+'&ifGenerationMatch='+urllib.parse.quote(str(expected or 0),safe=''), 'POST', raw)
            return str(json.loads(response)['generation'])
        with self._lock(key):
            try: _, current = self.read(key)
            except NotFound: current = None
            if current != expected: raise Conflict()
            version = str(int(current or 0)+1)
            envelope = json.dumps({'version':version,'data':data},separators=(',',':')).encode()
            fd, temporary = tempfile.mkstemp(dir=self.root, prefix='.pending-')
            try:
                with os.fdopen(fd, 'wb') as out:
                    out.write(envelope); out.flush(); os.fsync(out.fileno())
                os.replace(temporary, self._path(key))
            finally:
                if os.path.exists(temporary): os.unlink(temporary)
            return version

    def modify(self, key, default, change):
        for _ in range(8):
            try: data, version = self.read(key)
            except NotFound: data, version = default(), None
            result = change(data)
            try:
                self.write(key, data, version)
                return result
            except Conflict: continue
        raise Conflict('Concurrent updates; retry later')
