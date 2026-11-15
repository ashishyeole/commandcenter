"""Moderated manifest catalog. Packages are inert data, never installed by the host."""
import hashlib
import json
import time
import re

KEY='marketplace/catalog-v1'
ID=re.compile(r'^[A-Za-z0-9_.-]{1,100}$')
def empty(): return {'schema':1,'published':{},'pending':{}}

def route(handler,principal,store,ApiError):
    path,method=handler.path,handler.command
    if path=='/v1/catalog' and method=='GET':
        from storage import NotFound
        try: state,_=store.read(KEY)
        except NotFound: state=empty()
        return {'packages':list(state['published'].values()),'trust_notice':'Moderator approval is not a security endorsement. Verify publisher keys and exact permissions before installing.'}
    if principal is None: raise ApiError(401,'Authentication required')
    if path=='/v1/catalog/pending' and method=='GET':
        from storage import NotFound
        try: state,_=store.read(KEY)
        except NotFound: state=empty()
        rows=state['pending'].values()
        if not principal.get('catalog_moderator',False): rows=[r for r in rows if r['tenant']==principal['tenant'] and (principal['role']=='owner' or r['subject']==principal['subject'])]
        return {'submissions':list(rows)}
    if path=='/v1/catalog/submit' and method=='POST':
        if principal['role'] not in ('owner','member'): raise ApiError(403,'Publisher write permission required')
        body=handler.body();raw=body.get('manifest_json');public_key=body.get('public_key','');signature=body.get('signature','')
        if not isinstance(raw,str) or len(raw.encode())>16000 or not isinstance(public_key,str) or len(public_key)>4096 or not isinstance(signature,str) or len(signature)>8192: raise ApiError(400,'Invalid package size')
        try: manifest=json.loads(raw)
        except (ValueError,TypeError): raise ApiError(400,'Invalid manifest JSON') from None
        if not isinstance(manifest,dict) or manifest.get('schema')!=1 or manifest.get('kind') not in ('rest','container','mcp-container') or not ID.fullmatch(str(manifest.get('id',''))) or not isinstance(manifest.get('name'),str) or not 1<=len(manifest['name'])<=100 or not isinstance(manifest.get('version'),str) or not 1<=len(manifest['version'])<=80: raise ApiError(400,'Invalid declarative extension manifest')
        namespace=principal['tenant']+'.'+manifest['id']
        def change(state):
            existing=state['pending'].get(namespace) or state['published'].get(namespace)
            if existing and existing['subject']!=principal['subject'] and principal['role']!='owner': raise ApiError(403,'Package belongs to another publisher')
            revision=(existing or {}).get('revision',0)
            if body.get('expected_revision',0)!=revision: raise ApiError(409,'Package changed; refresh before submitting')
            if len(state['pending'])>=100 and namespace not in state['pending']: raise ApiError(409,'Moderation queue full')
            if sum(r['tenant']==principal['tenant'] for r in state['pending'].values())>=10 and namespace not in state['pending']: raise ApiError(409,'Tenant moderation quota reached')
            row={'id':namespace,'manifest_json':raw,'public_key':public_key,'signature':signature,'sha256':hashlib.sha256(raw.encode()).hexdigest(),'name':manifest['name'],'version':manifest['version'],'tenant':principal['tenant'],'subject':principal['subject'],'submitted_at':int(time.time()),'revision':revision+1,'publisher_verification':'unverified; client must verify supplied signature and trusted key'}
            state['pending'][namespace]=row
            return {'id':namespace,'revision':row['revision'],'status':'pending_moderation','public':False}
        return store.modify(KEY,empty,change)
    if path=='/v1/catalog/withdraw' and method=='POST':
        if principal['role'] not in ('owner','member'): raise ApiError(403,'Publisher permission required')
        body=handler.body();package=body.get('id')
        def change(state):
            row=state['pending'].get(package) or state['published'].get(package)
            if not row: raise ApiError(404,'Package not found')
            if row['tenant']!=principal['tenant'] or (row['subject']!=principal['subject'] and principal['role']!='owner'): raise ApiError(403,'Package belongs to another publisher')
            if row['revision']!=body.get('expected_revision') or row['sha256']!=body.get('reviewed_sha256'): raise ApiError(409,'Package changed; review before withdrawal')
            state['pending'].pop(package,None);state['published'].pop(package,None)
            return {'id':package,'withdrawn':True,'installed_copies_affected':False}
        return store.modify(KEY,empty,change)
    if path=='/v1/catalog/moderate' and method=='POST':
        if not principal.get('catalog_moderator',False) or principal['role']!='owner': raise ApiError(403,'Catalog moderator permission required')
        body=handler.body();package=body.get('id');decision=body.get('decision')
        if not isinstance(package,str) or decision not in ('approve','reject','unpublish'): raise ApiError(400,'Invalid moderation decision')
        def change(state):
            table='published' if decision=='unpublish' else 'pending'
            row=state[table].get(package)
            if not row: raise ApiError(404,'Package not found')
            if row['sha256']!=body.get('reviewed_sha256') or row['revision']!=body.get('expected_revision'): raise ApiError(409,'Review the exact current submission before moderating')
            if decision=='approve':
                if len(state['published'])>=100 and package not in state['published']: raise ApiError(409,'Catalog capacity reached')
                row.update(moderated_at=int(time.time()),moderated_by=principal['subject'],moderation='approved_for_listing_not_execution')
                state['published'][package]=row
            del state[table][package]
            return {'id':package,'decision':decision}
        return store.modify(KEY,empty,change)
    raise ApiError(404,'Unknown catalog endpoint')
