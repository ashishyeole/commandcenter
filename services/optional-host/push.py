"""Opt-in native push. Fixed generic text; encrypted tokens; tenant/subject ownership.
No provider call occurs on import or registration. No retries after ambiguous sends.
"""
import base64, json, os, re, secrets, time
TITLE = 'Personal Command Center'
BODY = 'A task needs your attention. Open Personal Command Center to review your due tasks.'
ID = re.compile(r'^[A-Za-z0-9_-]{1,100}$')

def crypt(token, context, decrypt=False):
    from cryptography.hazmat.primitives.ciphers.aead import AESGCM
    key=base64.b64decode(os.environ['PCC_PUSH_STORAGE_KEY_BASE64'],validate=True)
    if len(key)!=32: raise ValueError('Invalid push storage key')
    if decrypt:
        raw=base64.b64decode(token,validate=True)
        return AESGCM(key).decrypt(raw[:12],raw[12:],context.encode()).decode()
    nonce=secrets.token_bytes(12)
    return base64.b64encode(nonce+AESGCM(key).encrypt(nonce,token.encode(),context.encode())).decode()

def deliver(device, token, event):
    import httpx
    with httpx.Client(http2=True,timeout=20,follow_redirects=False,trust_env=False) as client:
        if device['platform']=='apns':
            import jwt
            keyfile=os.environ['PCC_APNS_KEY_FILE']
            with open(keyfile,'rb') as file: key=file.read(16384)
            auth=jwt.encode({'iss':os.environ['PCC_APNS_TEAM_ID'],'iat':int(time.time())},key,algorithm='ES256',headers={'kid':os.environ['PCC_APNS_KEY_ID']})
            topic=os.environ['PCC_APNS_TOPIC']
            host='api.sandbox.push.apple.com' if device['environment']=='sandbox' else 'api.push.apple.com'
            response=client.post(f'https://{host}/3/device/{token}',headers={'authorization':'bearer '+auth,'apns-topic':topic,'apns-push-type':'alert','apns-priority':'10','apns-expiration':str(int(time.time())+3600),'apns-collapse-id':'pcc-task-review'},json={'aps':{'alert':{'title':TITLE,'body':BODY},'sound':'default'},'pcc_event':event})
            return {'status':'provider_accepted' if response.status_code==200 else 'provider_rejected','http_status':response.status_code,'invalid_token':response.status_code==410}
        import google.auth
        from google.auth.transport.requests import Request
        credentials,project=google.auth.default(scopes=['https://www.googleapis.com/auth/firebase.messaging'])
        credentials.refresh(Request())
        project=os.environ.get('PCC_FCM_PROJECT',project or '')
        if not re.fullmatch(r'[a-z][a-z0-9-]{4,62}',project): raise ValueError('Invalid Firebase project')
        # Data-only message lets the companion enforce its local consent, quiet hours and dedupe.
        response=client.post(f'https://fcm.googleapis.com/v1/projects/{project}/messages:send',headers={'authorization':'Bearer '+credentials.token},json={'message':{'token':token,'data':{'pcc_event':event,'kind':'task_review'},'android':{'priority':'HIGH','ttl':'3600s','collapse_key':'pcc-task-review'}}})
        value=response.json() if len(response.content)<65536 else {}
        invalid=any(x.get('errorCode')=='UNREGISTERED' for x in value.get('error',{}).get('details',[]) if isinstance(x,dict))
        return {'status':'provider_accepted' if response.status_code==200 else 'provider_rejected','http_status':response.status_code,'invalid_token':invalid}

def route(handler,p,store,Error):
    if p['role'] not in ('owner','member'): raise Error(403,'Native push requires member access')
    def ident(v):
        if not isinstance(v,str) or not ID.fullmatch(v): raise Error(400,'Invalid push identifier')
        return v
    key='tenant/'+p['tenant']+'/push-v1'
    def empty(): return {'devices':{},'attempts':{}}
    def context(d): return p['tenant']+'/'+p['subject']+'/'+d
    if handler.path=='/v1/push/devices' and handler.command=='GET':
        def listing(s): return {'devices':[{'id':i,'platform':d['platform'],'environment':d['environment'],'updated_at':d['updated_at'],'active':bool(d.get('token')),'last_attempt':d.get('last_attempt')} for i,d in s['devices'].items() if d['subject']==p['subject']]}
        return store.modify(key,empty,listing)
    if handler.path=='/v1/push/devices' and handler.command=='POST':
        b=handler.body();id=ident(b.get('id'));platform=b.get('platform');token=b.get('token');env=b.get('environment','production')
        if b.get('consent') is not True or platform not in ('apns','fcm') or env not in ('sandbox','production') or not isinstance(token,str): raise Error(400,'Explicit registration consent and valid platform required')
        if not ((platform=='apns' and re.fullmatch(r'[a-f0-9]{32,256}',token)) or (platform=='fcm' and 20<=len(token)<=4096 and all(33<=ord(c)<=126 for c in token))): raise Error(400,'Invalid provider token')
        try: encrypted=crypt(token,context(id))
        except Exception: raise Error(503,'Push encryption/dependencies must be configured by the host owner') from None
        def register(s):
            old=s['devices'].get(id)
            if old and old['subject']!=p['subject']: raise Error(403,'Device belongs to another subject')
            if not old and len(s['devices'])>=32: raise Error(409,'Maximum 32 push devices per tenant')
            s['devices'][id]={'subject':p['subject'],'platform':platform,'environment':env,'token':encrypted,'updated_at':int(time.time()),'generation':secrets.token_hex(16),'last_attempt':old.get('last_attempt') if old else None}
            return {'registered':True,'id':id}
        return store.modify(key,empty,register)
    if handler.path.startswith('/v1/push/devices/') and handler.command=='DELETE':
        id=ident(handler.path.removeprefix('/v1/push/devices/'))
        def remove(s):
            d=s['devices'].get(id)
            if d and d['subject']!=p['subject']: raise Error(403,'Device belongs to another subject')
            s['devices'].pop(id,None);return {'removed':True}
        return store.modify(key,empty,remove)
    if handler.path=='/v1/push/send' and handler.command=='POST':
        b=handler.body();id=ident(b.get('device'));event=ident(b.get('event'))
        if b.get('confirmation')!='SEND NATIVE PUSH' or b.get('consent') is not True: raise Error(400,'Explicit send approval required')
        now=int(time.time());attempt=p['subject']+':'+event
        def reserve(s):
            s['attempts']={k:v for k,v in s['attempts'].items() if v['at']>now-86400}
            d=s['devices'].get(id)
            if not d or d['subject']!=p['subject'] or not d.get('token'): raise Error(404,'Active own device not found')
            if attempt in s['attempts']: raise Error(409,'Attempt already reserved; inspect receipt before retrying')
            recent=[v for k,v in s['attempts'].items() if v['subject']==p['subject']]
            if len(recent)>=20 or any(v['at']>now-60 for v in recent): raise Error(429,'Push limit: one attempt per minute and twenty per day')
            s['attempts'][attempt]={'at':now,'subject':p['subject'],'device':id,'status':'outcome_unknown'}
            d['last_attempt']={'at':now,'status':'outcome_unknown'}
            return dict(d)
        d=store.modify(key,empty,reserve)
        try: outcome=deliver(d,crypt(d['token'],context(id),True),event)
        except Exception: outcome={'status':'outcome_unknown','notice':'Provider setup or transport failed; no automatic retry. Inspect provider before retrying.'}
        receipt=dict(outcome,at=now,delivery_confirmed=False)
        def finish(s):
            if attempt in s['attempts']: s['attempts'][attempt].update(receipt)
            current=s['devices'].get(id)
            if current and current['generation']==d['generation']:
                current['last_attempt']=receipt
                if outcome.get('invalid_token'): current['token']=''
            return receipt
        return store.modify(key,empty,finish)
    raise Error(404,'Unknown native push operation')
