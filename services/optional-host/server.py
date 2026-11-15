"""Opt-in hosted control plane. Never imports or opens a desktop workspace.
Run behind a TLS reverse proxy. No cookies, wildcard CORS, automatic enrollment or
plaintext personal records. Every request is bound to one configured principal.
"""
import base64
import hashlib
import hmac
import http.client
import ipaddress
import json
import os
import re
import secrets
import socket
import ssl
import threading
import time
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from storage import Storage, Conflict, NotFound

STORE = Storage()
ID = re.compile(r'^[A-Za-z0-9_-]{1,100}$')
SOURCE_HOSTS = frozenset(h.lower() for h in os.environ.get('PCC_SOURCE_HOSTS','export.arxiv.org,hnrss.org').split(',') if h)
# Store token SHA-256 values, never raw access tokens, in configuration/Secret Manager.
PRINCIPALS = json.loads(os.environ.get('PCC_PRINCIPALS_JSON','[]'))
if not isinstance(PRINCIPALS,list): raise RuntimeError('Access keys must be an array')
if len(PRINCIPALS)>100: raise RuntimeError('Maximum 100 configured access keys per deployment')
if not PRINCIPALS: raise RuntimeError('Configure at least one strong access-token digest; anonymous service is forbidden')
for principal in PRINCIPALS:
    if not isinstance(principal,dict) or any(not isinstance(principal.get(k,''),str) for k in ('tenant','subject','role','token_sha256')) or type(principal.get('expires_at',2**53)) is not int or type(principal.get('catalog_moderator',False)) is not bool or not ID.fullmatch(principal.get('tenant','')) or not ID.fullmatch(principal.get('subject','')) or principal.get('role') not in ('owner','member','reader','scheduler') or not re.fullmatch(r'[0-9a-f]{64}',principal.get('token_sha256','')):
        raise RuntimeError('Invalid principal configuration')
if len({p['token_sha256'] for p in PRINCIPALS}) != len(PRINCIPALS): raise RuntimeError('Each access token must identify exactly one principal')
TENANTS = sorted({p['tenant'] for p in PRINCIPALS if p['role']!='scheduler'})
LICENSES = json.loads(os.environ.get('PCC_LICENSES_JSON','{}'))
FEATURES = frozenset(('collectors','relay','catalog_publish'))
if not isinstance(LICENSES,dict): raise RuntimeError('Hosted license policies must be an object')
for tenant,license in LICENSES.items():
    if not ID.fullmatch(tenant) or not isinstance(license,dict) or not isinstance(license.get('features',[]),list) or any(not isinstance(f,str) for f in license.get('features',[])) or type(license.get('expires_at',2**53)) is not int or not isinstance(license.get('plan',''),str) or len(license.get('plan',''))>100 or not set(license.get('features',[]))<=FEATURES or type(license.get('max_members',1)) is not int or license.get('max_members',1)<1:
        raise RuntimeError('Invalid hosted license policy')
    seats=len({p['subject'] for p in PRINCIPALS if p['tenant']==tenant and p['role']!='scheduler'})
    if seats>license.get('max_members',1): raise RuntimeError('Configured tenant exceeds its licensed member count')
def entitlement(p,feature):
    license=LICENSES.get(p['tenant'])
    # Self-hosted deployments have no paywall unless the operator configures a policy.
    if license is not None and (license.get('expires_at',2**53)<=time.time() or feature not in license.get('features',[])):
        raise ApiError(403,'Optional hosted feature entitlement unavailable; local data, export and downloads remain accessible')
STARTED = time.time()
RATE_LOCK = threading.Lock()
RATES = {}

class ApiError(Exception):
    def __init__(self, status, message): self.status,self.message=status,message

def identifier(value):
    if not isinstance(value,str) or not ID.fullmatch(value): raise ApiError(400,'Invalid identifier')
    return value

def text(value, limit=200):
    if not isinstance(value,str) or not value.strip() or len(value)>limit: raise ApiError(400,'Invalid text or size')
    return value.strip()

def state_key(principal): return 'tenant/'+principal['tenant']+'/state-v1'
def empty(): return {'schema':1,'jobs':{},'inboxes':{},'audit':[],'devices':{}}
def audit(state, principal, action, target):
    state['audit'].append({'at':int(time.time()),'subject':principal['subject'],'action':action,'target':target})
    state['audit']=state['audit'][-300:]

def require_write(p):
    if p['role'] not in ('owner','member'): raise ApiError(403,'Write permission required')

def public_url(value):
    value=text(value,2000)
    parsed=urllib.parse.urlsplit(value)
    if parsed.scheme!='https' or parsed.username or parsed.password or parsed.fragment or parsed.port not in (None,443) or parsed.hostname not in SOURCE_HOSTS:
        raise ApiError(400,'Source must use HTTPS on an operator-allowlisted host')
    return parsed

class PinnedHTTPS(http.client.HTTPSConnection):
    def connect(self):
        addresses=socket.getaddrinfo(self.host,443,type=socket.SOCK_STREAM)
        if not addresses: raise ApiError(502,'Source did not resolve')
        for _,_,_,_,address in addresses:
            ip=ipaddress.ip_address(address[0])
            if not ip.is_global or getattr(ip,'ipv4_mapped',None): raise ApiError(400,'Source resolved to a non-public address')
        last=None
        for family,kind,protocol,_,address in addresses:
            sock=socket.socket(family,kind,protocol);sock.settimeout(12)
            try:
                sock.connect(address)
                self.sock=self._context.wrap_socket(sock,server_hostname=self.host)
                return
            except (OSError,ssl.SSLError) as error: sock.close();last=error
        raise OSError('Source connection failed') from last

def fetch_source(url):
    parsed=public_url(url)
    conn=PinnedHTTPS(parsed.hostname,timeout=12,context=ssl.create_default_context())
    try:
        path=parsed.path or '/'
        if parsed.query: path+='?'+parsed.query
        conn.request('GET',path,headers={'Accept':'application/rss+xml, application/atom+xml, application/json, text/plain','Accept-Encoding':'identity','User-Agent':'PersonalCommandCenter-OptionalCollector/0.5'})
        response=conn.getresponse()
        if response.status!=200: raise ApiError(502,'Source returned HTTP '+str(response.status))
        if response.getheader('Content-Encoding','identity')!='identity': raise ApiError(502,'Compressed source responses are unsupported')
        body=response.read(262145)
        if len(body)>262144: raise ApiError(502,'Source response exceeds 256 KiB')
        kind=response.getheader('Content-Type','').split(';')[0].lower()
        if kind not in ('application/rss+xml','application/atom+xml','application/xml','text/xml','application/json','text/plain'): raise ApiError(502,'Unsupported source content type')
        return {'source_url':url,'fetched_at':int(time.time()),'content_type':kind,'body':body.decode('utf-8'),'sha256':hashlib.sha256(body).hexdigest(),'trust':'untrusted_external_content'}
    except (UnicodeError,OSError,http.client.HTTPException): raise ApiError(502,'Source request failed') from None
    finally: conn.close()

def scheduler_tick():
    results=[]
    # Invoked by an external scheduler. No persistent background process is required.
    for tenant in TENANTS:
        p={'tenant':tenant,'subject':'hosted-scheduler','role':'scheduler'}
        def expire(state):
            for who in list(state['inboxes']): state['inboxes'][who]=[m for m in state['inboxes'][who] if m['expires_at']>time.time()]
        STORE.modify(state_key(p),empty,expire)
        try: entitlement(p,'collectors')
        except ApiError: continue
        now=int(time.time());lease=secrets.token_hex(16)
        def claim(state):
            due=[]
            for jid,job in state['jobs'].items():
                if job['enabled'] and job.get('next_run',0)<=now and job.get('lease_until',0)<=now:
                    job.update(lease=lease,lease_until=now+120)
                    due.append((jid,dict(job)))
                    if len(due)>=2: break
            return due
        due=STORE.modify(state_key(p),empty,claim)
        for jid,job in due:
            try: result,error=fetch_source(job['url']),None
            except ApiError as failure: result,error=None,failure.message
            def finish(state):
                current=state['jobs'].get(jid)
                if not current or current.get('lease')!=lease: return False
                failures=0 if result else min(8,current.get('failures',0)+1)
                delay=job['interval_minutes']*60 if result else max(job['interval_minutes']*60,min(86400,60*(2**failures)))
                current.update(lease_until=0,lease='',next_run=int(time.time())+delay,last_attempt=int(time.time()),last_error=error,failures=failures)
                if result: current['result']=result
                audit(state,p,'collector_succeeded' if result else 'collector_failed',jid)
                return True
            STORE.modify(state_key(p),empty,finish)
            results.append({'tenant':tenant,'job':jid,'ok':result is not None})
    return {'processed':results}

class Handler(BaseHTTPRequestHandler):
    server_version='PCC'
    sys_version=''
    protocol_version='HTTP/1.1'
    def setup(self):
        super().setup();self.connection.settimeout(20)
    def log_message(self,*_): pass # Never log Authorization, query strings or request bodies.
    def response(self,status,value):
        raw=json.dumps(value,separators=(',',':'),ensure_ascii=False).encode()
        self.send_response(status)
        for k,v in [('Content-Type','application/json; charset=utf-8'),('Content-Length',str(len(raw))),('Cache-Control','no-store'),('X-Content-Type-Options','nosniff'),('Content-Security-Policy',"default-src 'none'; frame-ancestors 'none'"),('Connection','close')]: self.send_header(k,v)
        self.end_headers();self.wfile.write(raw);self.close_connection=True
    def principal(self):
        if len(self.headers.get_all('Authorization',[]))!=1: raise ApiError(401,'Exactly one authorization header required')
        auth=self.headers.get('Authorization','')
        if not auth.startswith('Bearer ') or len(auth)>1024 or len(auth[7:])<32 or any(c.isspace() for c in auth[7:]): raise ApiError(401,'Bearer authentication required')
        digest=hashlib.sha256(auth[7:].encode()).hexdigest()
        matched=None
        for p in PRINCIPALS:
            if hmac.compare_digest(digest,p['token_sha256']): matched=p
        if matched is None: raise ApiError(401,'Invalid credentials')
        if matched.get('expires_at',2**53)<=int(time.time()): raise ApiError(401,'Credential expired')
        with RATE_LOCK:
            slot=int(time.time()/60);key=digest
            previous,count=RATES.get(key,(slot,0))
            count=count+1 if previous==slot else 1
            RATES[key]=(slot,count)
            if count>120: raise ApiError(429,'Rate limit exceeded')
        return matched
    def body(self):
        if self.headers.get('Content-Encoding','identity')!='identity': raise ApiError(415,'Compressed request bodies unsupported')
        if len(self.headers.get_all('Content-Length',[]))!=1: raise ApiError(400,'Exactly one content length required')
        if self.headers.get('Transfer-Encoding'): raise ApiError(400,'Chunked request bodies are unsupported')
        if self.headers.get('Content-Type','').split(';')[0]!='application/json': raise ApiError(415,'JSON required')
        try: size=int(self.headers.get('Content-Length','-1'))
        except ValueError: raise ApiError(400,'Invalid content length') from None
        if size<0 or size>524288: raise ApiError(413,'Request exceeds 512 KiB')
        raw=self.rfile.read(size)
        if len(raw)!=size: raise ApiError(400,'Incomplete request body')
        try: value=json.loads(raw)
        except (ValueError,UnicodeError): raise ApiError(400,'Invalid JSON') from None
        if not isinstance(value,dict): raise ApiError(400,'Expected an object')
        return value
    def dispatch(self):
        if self.path=='/health' and self.command=='GET': return {'status':'ready','schema':1,'version':'0.5.0'}
        if self.headers.get('Origin'): raise ApiError(403,'Browser-origin requests are disabled; use the native client')
        if self.path=='/v1/catalog' and self.command=='GET':
            from marketplace import route
            return route(self,None,STORE,ApiError)
        p=self.principal()
        if self.path=='/v1/scheduler/tick' and self.command=='POST':
            if p['role']!='scheduler': raise ApiError(403,'Scheduler credential required')
            return scheduler_tick()
        if p['role']=='scheduler': raise ApiError(403,'Scheduler cannot read tenant data')
        if self.path.startswith('/v1/push/'):
            from push import route
            return route(self,p,STORE,ApiError)
        if self.path.startswith('/v1/catalog/'):
            if self.path=='/v1/catalog/submit': entitlement(p,'catalog_publish')
            from marketplace import route
            return route(self,p,STORE,ApiError)
        if self.path=='/v1/me' and self.command=='GET':
            return {'tenant':p['tenant'],'subject':p['subject'],'role':p['role'],'source_hosts':sorted(SOURCE_HOSTS),'relay':'opaque-envelopes-only','mandatory_for_local_use':False,'license':LICENSES.get(p['tenant'],{'plan':'self_hosted','features':sorted(FEATURES),'expires_at':None,'max_members':None})}
        key=state_key(p)
        try: state,_=STORE.read(key)
        except NotFound: state=empty()
        if self.path=='/v1/jobs' and self.command=='GET': return {'jobs':[dict(id=k,**v) for k,v in state['jobs'].items()]}
        if self.path=='/v1/jobs' and self.command=='POST':
            require_write(p);body=self.body();jid=identifier(body.get('id'));
            if body.get('enabled'): entitlement(p,'collectors')
            url=text(body.get('url'),2000);public_url(url)
            name=text(body.get('name'));interval=body.get('interval_minutes');enabled=body.get('enabled')
            if type(interval) is not int or interval<15 or interval>10080 or type(enabled) is not bool: raise ApiError(400,'Use a 15-minute to seven-day interval and explicit enabled flag')
            def change(s):
                previous=s['jobs'].get(jid)
                if previous and previous['owner']!=p['subject'] and p['role']!='owner': raise ApiError(403,'Only the job owner can edit it')
                if previous and body.get('expected_revision')!=previous['revision']: raise ApiError(409,'Collector changed; reload before editing')
                if not previous and body.get('expected_revision') is not None: raise ApiError(409,'Collector was deleted; reload before saving')
                if not previous and len(s['jobs'])>=8: raise ApiError(409,'Maximum eight collectors per tenant')
                # Editing invalidates in-flight results and explicitly reauthorizes the URL.
                s['jobs'][jid]={'name':name,'url':url,'interval_minutes':interval,'enabled':enabled,'owner':p['subject'],'next_run':int(time.time()),'lease_until':0,'revision':(previous or {}).get('revision',0)+1}
                audit(s,p,'collector_configured',jid)
                return {'id':jid}
            return STORE.modify(key,empty,change)
        if self.path.startswith('/v1/jobs/') and self.command=='DELETE':
            require_write(p);jid=identifier(self.path[len('/v1/jobs/'):])
            def change(s):
                job=s['jobs'].get(jid)
                if job and job['owner']!=p['subject'] and p['role']!='owner': raise ApiError(403,'Only the job owner can delete it')
                s['jobs'].pop(jid,None);audit(s,p,'collector_deleted',jid);return {'deleted':jid}
            return STORE.modify(key,empty,change)
        if self.path=='/v1/relay' and self.command=='POST':
            require_write(p);entitlement(p,'relay');body=self.body();recipient=identifier(body.get('recipient'));message=identifier(body.get('id'))
            if recipient not in {x['subject'] for x in PRINCIPALS if x['tenant']==p['tenant'] and x['role']!='scheduler'}: raise ApiError(404,'Recipient not in this tenant')
            if body.get('format')!='pcc-encrypted-archive-v1': raise ApiError(400,'Only explicitly encrypted portable archive envelopes are accepted')
            cipher=text(body.get('ciphertext_base64'),180000)
            try: raw=base64.b64decode(cipher,validate=True)
            except ValueError: raise ApiError(400,'Invalid base64') from None
            if not raw.startswith(b'PACC01') or len(raw)<64 or len(raw)>131072: raise ApiError(400,'Encrypted envelope must be 64 bytes to 128 KiB')
            ttl=body.get('ttl_seconds',86400)
            if type(ttl) is not int or ttl<60 or ttl>604800: raise ApiError(400,'TTL must be one minute to seven days')
            digest=hashlib.sha256(raw).hexdigest();now=int(time.time())
            def change(s):
                inboxes=s['inboxes']
                for who in list(inboxes): inboxes[who]=[m for m in inboxes[who] if m['expires_at']>now]
                inbox=inboxes.setdefault(recipient,[])
                existing=next((m for m in inbox if m['id']==message and m['sender']==p['subject']),None)
                if existing:
                    if existing['sha256']!=digest: raise ApiError(409,'Message ID already has different content')
                    return {'id':message,'duplicate':True}
                if sum(len(x) for x in inboxes.values())>=32: raise ApiError(409,'Tenant relay queue full; acknowledge deliveries first')
                inbox.append({'id':message,'sender':p['subject'],'recipient':recipient,'ciphertext_base64':cipher,'format':body['format'],'sha256':digest,'created_at':now,'expires_at':now+ttl})
                audit(s,p,'encrypted_envelope_queued',message);return {'id':message,'duplicate':False}
            return STORE.modify(key,empty,change)
        if self.path=='/v1/relay' and self.command=='GET':
            return {'messages':[m for m in state['inboxes'].get(p['subject'],[]) if m['expires_at']>time.time()]}
        if self.path.startswith('/v1/relay/') and self.command=='DELETE':
            # A recipient may acknowledge only its own queue, including read-only principals.
            message=identifier(self.path[len('/v1/relay/'):])
            def change(s):
                s['inboxes'][p['subject']]=[m for m in s['inboxes'].get(p['subject'],[]) if m['id']!=message]
                audit(s,p,'encrypted_envelope_acknowledged',message);return {'acknowledged':message}
            return STORE.modify(key,empty,change)
        if self.path=='/v1/audit' and self.command=='GET':
            if p['role']!='owner': raise ApiError(403,'Tenant owner required')
            return {'events':state['audit']}
        if self.path=='/v1/export' and self.command=='GET':
            if p['role']!='owner': raise ApiError(403,'Tenant owner required')
            try: push,_=STORE.read('tenant/'+p['tenant']+'/push-v1')
            except NotFound: push={'devices':{},'attempts':{}}
            push['devices']={i:{k:v for k,v in d.items() if k not in ('token','generation')} for i,d in push['devices'].items()}
            return {'tenant':p['tenant'],'exported_at':int(time.time()),'state':state,'push_metadata':push,'credentials_included':False}
        if self.path=='/v1/reset' and self.command=='POST':
            if p['role']!='owner' or self.body().get('confirmation')!='DELETE HOSTED TENANT DATA': raise ApiError(403,'Owner and exact confirmation required')
            def clear_push(s): s.clear();s.update({'devices':{},'attempts':{}})
            STORE.modify('tenant/'+p['tenant']+'/push-v1',lambda:{'devices':{},'attempts':{}},clear_push)
            def change(s): s.clear();s.update(empty());audit(s,p,'tenant_data_reset',p['tenant']);return {'deleted':True,'local_device_data_affected':False}
            return STORE.modify(key,empty,change)
        raise ApiError(404,'Unknown endpoint')
    def handle_request(self):
        try: self.response(200,self.dispatch())
        except ApiError as e: self.response(e.status,{'error':e.message})
        except Conflict: self.response(409,{'error':'Concurrent change; retry'})
        except (ValueError,KeyError,TypeError): self.response(400,{'error':'Invalid request'})
        except (BrokenPipeError,ConnectionResetError,socket.timeout): self.close_connection=True
        except Exception: self.response(503,{'error':'Service temporarily unavailable'})
    do_GET=handle_request
    do_POST=handle_request
    do_DELETE=handle_request

class BoundedServer(ThreadingHTTPServer):
    daemon_threads = True
    def __init__(self,*args,**kwargs):
        self.slots=threading.BoundedSemaphore(32)
        super().__init__(*args,**kwargs)
    def process_request(self,request,address):
        if not self.slots.acquire(blocking=False):
            request.close();return
        try: super().process_request(request,address)
        except Exception: self.slots.release();raise
    def process_request_thread(self,request,address):
        try: super().process_request_thread(request,address)
        finally: self.slots.release()

if __name__=='__main__':
    # The process is stateless with GCS; Cloud Run / a TLS ingress terminates HTTPS.
    BoundedServer((os.environ.get('PCC_BIND','127.0.0.1'),int(os.environ.get('PORT','8080'))),Handler).serve_forever()
