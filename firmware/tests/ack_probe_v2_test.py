import importlib.util,json,tempfile,unittest,copy,os
from pathlib import Path
from unittest.mock import patch
import sys,contextlib,io
S=Path(__file__).resolve().parents[1]/'tools/ack_probe_v2.py';spec=importlib.util.spec_from_file_location('probe',S);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m)
class ActualExport:
 def __init__(self,text):
  self.lines=text.splitlines();self.pages={};self.status=next(x for x in reversed(self.lines) if x.startswith('ap2 status ') and 'run=77 ' in x and 'state=3 ' in x)
  page=[]
  for line in self.lines:
   if line.startswith('ap2 page '):page=[line]
   elif page:
    page.append(line)
    if line.startswith('ap2 end '):self.pages[int(m.pico_fields(page[0])['from'],16)]=page;page=[]
 def request(self,command,end):
  if command=='AP2 STATUS':return [self.status]
  if command=='AP2 WINDOW':return [next(x for x in self.lines if x.startswith('ap2 window '))]
  if command=='AP2 LIVE':return [next(x for x in self.lines if x.startswith('ap2 live '))]
  if command=='AP2 QUALITY':return [next(x for x in self.lines if x.startswith('ap2 quality '))]
  return self.pages[int(command.split()[4],16)].copy()
class Tests(unittest.TestCase):
 @classmethod
 def setUpClass(cls):cls.raw=Path(os.environ['PM_ACK_EXPORT_FIXTURE']).read_text()
 def test_actual_native_export_roundtrip(self):
  result=m.read_pico(ActualExport(self.raw),'1234','77');self.assertEqual(len(result['records']),9);self.assertTrue(result['complete']);self.assertEqual(result['records'][4]['message'],3)
 def test_post_deadline_buttons_mark_last_report_unusable(self):
  result=m.read_pico(ActualExport(self.raw),'1234','77');self.assertTrue(result['post_freeze_buttons_detected']);self.assertFalse(result['usable_for_last_reported_button'])
 def test_wrong_boot(self):
  with self.assertRaisesRegex(ValueError,'mismatch'):m.read_pico(ActualExport(self.raw),'9999','77')
 def test_wrong_run(self):
  with self.assertRaisesRegex(ValueError,'mismatch'):m.read_pico(ActualExport(self.raw),'1234','88')
 def test_missing_part(self):
  t=ActualExport(self.raw);t.pages[0]=t.pages[0][:-2]+t.pages[0][-1:]
  with self.assertRaisesRegex(ValueError,'incomplete'):m.read_pico(t,'1234','77')
 def test_duplicate_part(self):
  t=ActualExport(self.raw);t.pages[0].insert(2,t.pages[0][1])
  with self.assertRaisesRegex(ValueError,'duplicate'):m.read_pico(t,'1234','77')
 def test_wrong_page_metadata(self):
  t=ActualExport(self.raw);t.pages[0][0]=t.pages[0][0].replace('run=77','run=99')
  with self.assertRaisesRegex(ValueError,'identity'):m.read_pico(t,'1234','77')
 def test_corrupt_payload_checksum(self):
  t=ActualExport(self.raw);t.pages[0]=[x.replace('a=1 ','a=ff ') for x in t.pages[0]]
  with self.assertRaisesRegex(ValueError,'digest'):m.read_pico(t,'1234','77')
 def test_durable_persist_idempotent_and_refuses_overwrite(self):
  with tempfile.TemporaryDirectory() as d:
   p=Path(d)/'data.json';a=m.persist(p,{'a':1});self.assertEqual(a,m.persist(p,{'a':1}))
   with self.assertRaisesRegex(ValueError,'differs'):m.persist(p,{'a':2})
   self.assertEqual(json.loads(p.read_text()),{'a':1})
 def test_host_pages_validate_digest(self):
  import zlib
  row={'us':1,'event':'decoded','connection':2,'message':3,'session':4,'frame':0,'kind':15,'a':123,'b':4};digest=zlib.crc32(json.dumps(row,separators=(',',':')).encode());status={'boot':'host','run':'1','phase':'frozen','count':1,'digest':digest,'overwritten':0,'contended':0}
  def request(q):return status if q['op']=='status' else {'status':status,'start':0,'count':1,'records':[dict(sorted(row.items()))]}
  self.assertTrue(m.read_host(request,'host','1')['complete']);row['a']+=1
  with self.assertRaisesRegex(ValueError,'digest'):m.read_host(request,'host','1')

class Workflow(unittest.TestCase):
 def setUp(self):
  self.tmp=tempfile.TemporaryDirectory();self.root=Path(self.tmp.name)/'trial';self.hs={'boot':'host','run':'0','last_run':'0','phase':'idle','connection':11,'session':1,'target':'sample-a','count':0,'digest':0,'overwritten':0,'contended':0};self.ps={'boot':'1234','run':'0','state':'0','n':'0','over':'0','gen':'1','digest':format(2166136261,'x'),'reason':'0'};self.last=0;self.buttons=0;self.arm_buttons=0;self.gate_buttons=0;self.waits=[];self.closed=False;self.fail_arm=False;self.fail_release=False;self.calls=[];owner=self
  class USB:
   def __init__(self,path):pass
   def __enter__(self):
    owner.closed=False
    if owner.ps['state']=='1':owner.ps['state']='4'
    return self
   def __exit__(self,*_):
    owner.closed=True
    if owner.ps['state']=='4':owner.ps['state']='1';owner.gate_buttons=owner.buttons
   def request(self,command,end):return owner.usb_request(command)
  self.usb_class=USB;self.real_wait=m.wait_for_window
 def tearDown(self):self.tmp.cleanup()
 def host(self,q):
  self.calls.append(('host',q.copy()));op=q['op']
  if op=='arm':
   self.hs.update(phase='armed',run=q['run'],last_run=q['run'])
  elif op=='freeze':self.hs['phase']='frozen'
  elif op=='release':
   if self.fail_release:self.fail_release=False;raise TimeoutError('host release reply lost')
   self.hs.update(phase='idle',run='0')
  elif op=='read':return {'status':self.hs.copy(),'start':0,'count':0,'records':[]}
  return self.hs.copy()
 def usb_request(self,command):
  self.calls.append(('pico',command));parts=command.split();op=parts[1]
  if op=='ID':return [f'ap2 identity boot=1234 device=sample-a last={self.last:x}']
  if op=='LIVE':return [f"ap2 live boot=1234 session=1 active=1 gen=1 buttons={self.buttons:x} state={self.ps['state']} trigger={1 if self.ps['state']=='2' else 0:x} begin={self.arm_buttons:x}"]
  if op=='QUALITY':return [f"ap2 quality boot=1234 run={self.ps['run']} gate=1 buttons={self.gate_buttons:x} overlap=0"]
  if op=='WINDOW':return [f"ap2 window boot=1234 run={self.ps['run']} trigger=0 freeze=0 begin={self.arm_buttons:x} end={self.buttons:x}"]
  if op=='ARM':
   if self.fail_arm:self.fail_arm=False;raise TimeoutError('Pico arm reply lost')
   self.ps.update(run=parts[3],state='4');self.last=int(parts[3],16);self.arm_buttons=self.buttons
  elif op=='FREEZE':self.ps['state']='3'
  elif op=='RELEASE':self.ps.update(state='0',run='0')
  elif op=='READ':return [f"ap2 page boot=1234 run={self.ps['run']} from=0 count=0 total=0 digest={self.ps['digest']}",f"ap2 end from=0 count=0 digest={self.ps['digest']}"]
  return ['ap2 status '+' '.join(f'{k}={v}' for k,v in self.ps.items())]
 def wait_window(self,since):
  now=[since];sleeps=[]
  def sleep(dt):
   sleeps.append(dt);now[0]+=dt
   if now[0]>=since+m.WINDOW_SECONDS and self.ps['state']=='2':self.ps['state']='3'
  self.real_wait(since,clock=lambda:now[0],sleeper=sleep)
  self.waits.append((now[0]-since,sleeps))
 def invoke(self,op,execute=True):
  args=['probe',op,'--serial','FAKE','--socket','FAKE','--device','sample-a','--dir',str(self.root)]+(['--execute'] if execute else [])
  with patch.object(m,'Serial',self.usb_class),patch.object(m,'host_request',side_effect=lambda _,q:self.host(q)),patch.object(sys,'argv',args),patch.object(m,'wait_for_window',side_effect=self.wait_window),contextlib.redirect_stdout(io.StringIO()):m.main()
 def test_plan_never_opens_or_arms(self):
  self.invoke('prepare',False);self.assertFalse(self.calls);self.assertFalse(self.root.exists())
 def test_ready_requires_both_arms_and_USB_closed(self):
  self.invoke('prepare');ready=json.loads((self.root/'ready.json').read_text());self.assertTrue(self.closed);self.assertTrue(ready['USB_closed']);self.assertEqual(self.hs['run'],self.ps['run'])
 def test_prepare_resume_after_lost_arm_ack(self):
  self.fail_arm=True
  with self.assertRaises(TimeoutError):self.invoke('prepare')
  self.assertFalse((self.root/'ready.json').exists());self.assertTrue(self.closed);saved=json.loads((self.root/'identity.json').read_text());self.invoke('prepare');self.assertEqual(saved['run'],json.loads((self.root/'ready.json').read_text())['run'])
 def test_collect_persist_before_release_and_retry_partial_release(self):
  self.invoke('prepare');self.invoke('collect');self.assertFalse(any(x[0]=='host' and x[1].get('op')=='release' for x in self.calls));self.assertTrue((self.root/'saved.json').exists());self.fail_release=True
  with self.assertRaises(TimeoutError):self.invoke('release')
  self.assertEqual(self.ps['state'],'0');self.assertEqual(self.hs['phase'],'frozen');self.invoke('release');self.assertEqual(self.hs['phase'],'idle');self.assertTrue((self.root/'released.json').exists())
 def test_changed_file_blocks_release(self):
  self.invoke('prepare');self.invoke('collect');(self.root/'pico.json').write_text('{}')
  before=len(self.calls)
  with self.assertRaisesRegex(ValueError,'changed'):self.invoke('release')
  self.assertEqual(before,len(self.calls));self.assertEqual(self.ps['state'],'3')

class ReviewRegressions(Workflow):
 def test_input_during_last_live_with_delayed_host_has_no_READY(self):
  original=self.usb_request
  def request(command):
   if command=='AP2 LIVE':self.buttons+=1;self.ps['state']='2'
   return original(command)
  self.usb_request=request
  with self.assertRaisesRegex(ValueError,'no cue'):self.invoke('prepare')
  self.assertEqual(self.hs['phase'],'armed');self.assertFalse((self.root/'ready.json').exists())
 def test_prepared_input_does_not_trigger_but_rejects_READY(self):
  original=self.usb_request
  def request(command):
   if command=='AP2 LIVE':self.buttons+=1;self.assertEqual(self.ps['state'],'4')
   return original(command)
  self.usb_request=request
  with self.assertRaisesRegex(ValueError,'no cue'):self.invoke('prepare')
  self.assertNotEqual(self.ps['state'],'2');self.assertFalse((self.root/'ready.json').exists())
 def test_early_collect_opens_USB_only_after_full_settle(self):
  self.invoke('prepare');self.ps['state']='2';self.hs['phase']='recording';observed=[];original=self.usb_class
  owner=self
  class USB(original):
   def __enter__(self):observed.append(owner.ps['state']);return super().__enter__()
  self.usb_class=USB;self.invoke('collect');self.assertEqual(observed,['3']);self.assertGreaterEqual(self.waits[-1][0],m.WINDOW_SECONDS);self.assertAlmostEqual(self.waits[-1][0],m.SETTLE_SECONDS,places=8);self.assertLessEqual(max(self.waits[-1][1]),.25)
 def test_ready_publish_failure_resumes_without_expiring_capture(self):
  original=m._write_staging
  def interrupted(fd,data):
   if Path(os.readlink('/proc/self/fd/'+str(fd))).name.startswith('.ready.json.'):
    os.write(fd,data[:8]);raise OSError('ready staging fault')
   return original(fd,data)
  with patch.object(m,'_write_staging',side_effect=interrupted):
   with self.assertRaises(OSError):self.invoke('prepare')
  self.assertFalse((self.root/'ready.json').exists());self.assertEqual(self.ps['state'],'1');self.invoke('prepare');self.assertTrue((self.root/'ready.json').exists())
 def test_interrupted_collect_stage_can_resume(self):
  self.invoke('prepare');original=m._write_staging
  def interrupted(fd,data):
   if Path(os.readlink('/proc/self/fd/'+str(fd))).name.startswith('.host.json.'):
    os.write(fd,data[:11]);raise OSError('interrupted staging write')
   return original(fd,data)
  with patch.object(m,'_write_staging',side_effect=interrupted):
   with self.assertRaises(OSError):self.invoke('collect')
  self.assertFalse((self.root/'host.json').exists());self.assertEqual(self.ps['state'],'3');self.assertEqual(self.hs['phase'],'frozen');self.invoke('collect');self.assertTrue((self.root/'saved.json').exists())

class AtomicPersistence(unittest.TestCase):
 def test_all_output_names_recover_at_every_persistence_boundary(self):
  names=['identity.json','ready.json','pico.json','host.json','saved.json','released.json']
  for name in names:
   for stage in ['write','file_fsync','publish','dir_fsync']:
    with self.subTest(name=name,stage=stage),tempfile.TemporaryDirectory() as d:
     path=Path(d)/name;value={'status':{'run':'123'},'rows':[1,2,3]};original_write=m._write_staging;original_fsync=m.os.fsync
     def write(fd,data):
      os.write(fd,data[:7]);raise OSError('partial staging')
     sync_calls=[0]
     def sync(fd):
      sync_calls[0]+=1
      if sync_calls[0]==(1 if stage=='file_fsync' else 2):raise OSError('fsync fault')
      return original_fsync(fd)
     with contextlib.ExitStack() as stack:
      if stage=='write':stack.enter_context(patch.object(m,'_write_staging',side_effect=write))
      elif stage in ['file_fsync','dir_fsync']:stack.enter_context(patch.object(m.os,'fsync',side_effect=sync))
      elif stage=='publish':stack.enter_context(patch.object(m.os,'link',side_effect=OSError('publish fault')))
      with self.assertRaises(OSError):m.persist(path,value)
     if path.exists():self.assertEqual(json.loads(path.read_text()),value)
     m.persist(path,value);self.assertEqual(json.loads(path.read_text()),value)
 def test_orphan_staging_file_never_blocks_retry_or_is_overwritten(self):
  with tempfile.TemporaryDirectory() as d:
   path=Path(d)/'host.json';orphan=Path(d)/'.host.json.abandoned.tmp';orphan.write_bytes(b'{"status":')
   m.persist(path,{'ok':True});self.assertEqual(json.loads(path.read_text()),{'ok':True});self.assertEqual(orphan.read_bytes(),b'{"status":')
 def test_conflicting_atomic_publish_preserves_existing_file(self):
  with tempfile.TemporaryDirectory() as d:
   path=Path(d)/'host.json';original=m.os.link
   def race(src,dst):Path(dst).write_text('{"other":1}');return original(src,dst)
   with patch.object(m.os,'link',side_effect=race):
    with self.assertRaisesRegex(ValueError,'differs'):m.persist(path,{'ours':1})
   self.assertEqual(json.loads(path.read_text()),{'other':1})
if __name__=='__main__':unittest.main()
