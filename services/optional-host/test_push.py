"""Isolated native-push authorization tests. All provider calls are mocked."""
import base64, os, tempfile, unittest
from unittest.mock import patch
import push
from storage import Storage
class ApiError(Exception):
    def __init__(self,status,message): self.status,self.message=status,message
class Handler:
    def __init__(self,path,method='POST',body=None):self.path,self.command,self.value=path,method,body or {}
    def body(self):return self.value
class PushTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory();self.env=patch.dict(os.environ,{'PCC_STATE_DIRECTORY':self.tmp.name,'PCC_STATE_BUCKET':'','PCC_PUSH_STORAGE_KEY_BASE64':base64.b64encode(os.urandom(32)).decode()});self.env.start();self.store=Storage();self.user={'tenant':'test','subject':'alice','role':'member'}
        self.patcher=patch.object(push,'deliver',return_value={'status':'provider_accepted','http_status':200,'invalid_token':False});self.provider=self.patcher.start()
    def tearDown(self):self.patcher.stop();self.env.stop();self.tmp.cleanup()
    def call(self,path,method='POST',body=None,user=None):return push.route(Handler(path,method,body),user or self.user,self.store,ApiError)
    def register(self):return self.call('/v1/push/devices',body={'id':'phone','platform':'apns','environment':'sandbox','token':'ab'*32,'consent':True})
    def send(self,event='event1'):return self.call('/v1/push/send',body={'device':'phone','event':event,'consent':True,'confirmation':'SEND NATIVE PUSH'})
    def test_registration_encrypted_and_listing_redacted(self):
        self.register();state,_=self.store.read('tenant/test/push-v1');self.assertNotIn('ab'*32,str(state));self.assertNotIn('token',self.call('/v1/push/devices','GET')['devices'][0]);self.provider.assert_not_called()
    def test_token_bound_to_tenant_subject_device(self):
        self.register();state,_=self.store.read('tenant/test/push-v1');token=state['devices']['phone']['token'];self.assertEqual(push.crypt(token,'test/alice/phone',True),'ab'*32)
        with self.assertRaises(Exception):push.crypt(token,'test/bob/phone',True)
    def test_other_subject_cannot_replace_delete_or_send(self):
        self.register();bob=dict(self.user,subject='bob')
        with self.assertRaises(ApiError):self.call('/v1/push/devices/phone','DELETE',user=bob)
        with self.assertRaises(ApiError):self.call('/v1/push/send',body={'device':'phone','event':'x','consent':True,'confirmation':'SEND NATIVE PUSH'},user=bob)
        self.assertEqual(self.call('/v1/push/devices','GET',user=bob)['devices'],[]);self.provider.assert_not_called()
    def test_reader_cannot_register(self):
        with self.assertRaises(ApiError):self.call('/v1/push/devices',user=dict(self.user,role='reader'))
        self.provider.assert_not_called()
    def test_explicit_send_approval(self):
        self.register()
        with self.assertRaises(ApiError):self.call('/v1/push/send',body={'device':'phone','event':'x'})
        self.provider.assert_not_called()
    def test_duplicate_event_and_rate_limit_do_not_resend(self):
        self.register();self.assertFalse(self.send()['delivery_confirmed'])
        for event in ['event1','event2']:
            with self.assertRaises(ApiError):self.send(event)
        self.assertEqual(self.provider.call_count,1)
    def test_unknown_outcome_reserved_without_retry(self):
        self.register();self.provider.side_effect=TimeoutError();self.assertEqual(self.send()['status'],'outcome_unknown')
        with self.assertRaises(ApiError):self.send()
        self.assertEqual(self.provider.call_count,1)
    def test_invalid_token_disables_current_registration(self):
        self.register();self.provider.return_value={'status':'provider_rejected','http_status':410,'invalid_token':True};self.send();self.assertFalse(self.call('/v1/push/devices','GET')['devices'][0]['active'])
    def test_delayed_invalid_result_cannot_disable_rotated_registration(self):
        self.register()
        def rotate(*args):self.register();return {'status':'provider_rejected','invalid_token':True}
        self.provider.side_effect=rotate;self.send();self.assertTrue(self.call('/v1/push/devices','GET')['devices'][0]['active'])
    def test_remove_registration_blocks_future_send(self):
        self.register();self.call('/v1/push/devices/phone','DELETE')
        with self.assertRaises(ApiError):self.send()
        self.provider.assert_not_called()
if __name__=='__main__':unittest.main()
