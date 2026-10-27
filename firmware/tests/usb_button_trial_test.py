import contextlib,io,json,tempfile,unittest,sys,types,hashlib,copy,os,socket
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
import usb_button_trial as m
class Tests(unittest.TestCase):
 def test_pure_scope_rule_separates_cached_mapping_from_reachability(self):
  # Trial-09's actual MAC/route/pre-ARM socket and after-ARM STALE row.
  # An after-ARM socket fact below is a counterfactual isolated input because
  # the old resolver rejected NUD before it queried that socket.
  args=types.SimpleNamespace(interface='eth0',known_mac='28:cd:c1:09:78:41',
      local_ip='192.0.2.120',port=16153,host_pid=1928084,device='PicoMado-lcd1')
  row=dict(dst='192.168.0.173',lladdr=args.known_mac,state=['STALE'])
  route=[dict(dst=row['dst'],dev='eth0',prefsrc=args.local_ip)]
  before=dict(interface='eth0',local_ip=args.local_ip,local_port=16153,peer_ip=row['dst'],
      peer_port=61823,inode=13769022,pid=args.host_pid,fd=17,process_start_ticks=60873347,
      cookie=[4217,0],known_device_mac=args.known_mac,socket_state=1,
      control_socket_owned=True,owned_socket_count=1,
      binding=dict(boot='18e0859a3ed1bab9-1d6b94',run='0',phase='idle',
                   connection=69,session=2330,target=args.device))
  after=copy.deepcopy(before);after['binding'].update(phase='armed',run='one-run')
  decide=lambda rows=[row],via=route,target=after,prior=before:m.target_scope_decision(
      args,rows,via,target,expected_phase='armed',prior=prior)
  self.assertEqual(decide()['neighbor_state'],'STALE')
  self.assertTrue(decide()['scope_bound']);self.assertFalse(decide()['recent_kernel_reachability'])
  self.assertTrue(decide([row|{'state':['DELAY']}])['scope_bound'])
  reachable=decide([row|{'state':['REACHABLE']}]);self.assertTrue(reachable['scope_bound'])
  self.assertTrue(reachable['recent_kernel_reachability'])
  for state in ('NONE','INCOMPLETE','PROBE','FAILED'):
   with self.subTest(state=state):self.assertFalse(decide([row|{'state':[state]}])['scope_bound'])
  counterexamples=[
      dict(rows=[]),dict(rows=[row,row]),
      dict(rows=[row,row|{'lladdr':'00:00:00:00:00:01'}]),
      dict(rows=[row|{'dev':'wlan0'}]),
      dict(rows=[row|{'dst':'192.168.0.174'}]),
      dict(via=[route[0]|{'dev':'wlan0'}]),dict(via=[route[0]|{'gateway':'192.168.0.1'}]),
      dict(target=after|{'peer_port':61824}),dict(target=after|{'socket_state':7}),
      dict(target=after|{'pid':1928085}),dict(target=after|{'process_start_ticks':60873348}),
      dict(target=after|{'control_socket_owned':False}),
      dict(target=after|{'owned_socket_count':2}),
      dict(target=after|{'cookie':[4218,0]}),
      dict(target=after|{'inode':13769023}),
      dict(target=after|{'binding':after['binding']|{'boot':'wrong-1d6b94'}}),
      dict(target=after|{'binding':after['binding']|{'session':2331}}),
  ]
  for i,change in enumerate(counterexamples):
   with self.subTest(counterexample=i):self.assertFalse(decide(**change)['scope_bound'])
 def test_known_neighbor_saved_identity_and_reachability_are_separate(self):
  # The MAC/interface are from the retained trial-02 config; DELAY was the
  # later read-only observation, not a guard-instant state from that trial.
  args=types.SimpleNamespace(interface='eth0',known_mac='28:cd:c1:09:78:41')
  # Actual scoped `ip -j neigh show dev eth0` rows omit `dev` on this host.
  row=dict(dst='192.168.0.173',lladdr=args.known_mac,state=['DELAY'])
  with tempfile.TemporaryDirectory()as d:
   path=Path(d)/'neighbor.json'
   with patch.object(m.subprocess,'check_output',return_value=json.dumps([row])):
    with self.assertRaisesRegex(ValueError,'not REACHABLE'):m.known_neighbor(args,path)
   saved=json.loads(path.read_text())
   self.assertEqual(saved['match_count'],1)
   self.assertEqual(saved['matches'],[row])
   self.assertEqual(saved['identity_decision'],'matched')
   self.assertEqual(saved['reachability_decision'],'not_REACHABLE')
   self.assertEqual(saved['refusal'],'known MAC is not REACHABLE')
   row['state']=['REACHABLE']
   with patch.object(m.subprocess,'check_output',return_value=json.dumps([row])):
    self.assertEqual(m.known_neighbor(args,Path(d)/'reachable.json'),'192.168.0.173')
   with patch.object(m.subprocess,'check_output',return_value=json.dumps([row,row])):
    with self.assertRaisesRegex(ValueError,'match count'):m.known_neighbor(args,Path(d)/'duplicate.json')
   with patch.object(m.subprocess,'check_output',return_value=json.dumps([row|{'dev':'wlan0'}])):
    with self.assertRaisesRegex(ValueError,'interface/IP mismatch'):m.known_neighbor(args,Path(d)/'wrong-dev.json')
   with patch.object(m.subprocess,'check_output',side_effect=OSError('netlink blocked')):
    with self.assertRaisesRegex(OSError,'netlink blocked'):m.known_neighbor(args,Path(d)/'read-error.json')
   self.assertIn('neighbor read failed',json.loads((Path(d)/'read-error.json').read_text())['refusal'])
 def test_cached_candidate_adapter_preserves_stale_without_claiming_reachability(self):
  args=types.SimpleNamespace(interface='eth0',known_mac='28:cd:c1:09:78:41')
  row=dict(dst='192.168.0.173',lladdr=args.known_mac,state=['STALE'])
  with tempfile.TemporaryDirectory()as d,patch.object(m.subprocess,'check_output',return_value=json.dumps([row])):
   peer,rows=m.scoped_neighbor_candidate(args,Path(d)/'neighbor.json')
   self.assertEqual(peer,row['dst']);self.assertEqual(rows,[row])
   saved=json.loads((Path(d)/'neighbor.json').read_text())
   self.assertEqual(saved['identity_decision'],'matched')
   self.assertEqual(saved['reachability_decision'],'cached_not_REACHABLE')
 def test_cached_scope_adapter_binds_one_real_local_socket_across_arm(self):
  try:listener=socket.socket(socket.AF_INET,socket.SOCK_STREAM)
  except PermissionError:self.skipTest('loopback socket unavailable in this sandbox')
  try:client=socket.socket(socket.AF_INET,socket.SOCK_STREAM)
  except PermissionError:
   listener.close();self.skipTest('loopback socket unavailable in this sandbox')
  with tempfile.TemporaryDirectory()as d,\
       socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)as control,\
       listener,client:
   path=str(Path(d)/'control.sock');control.bind(path);control.listen()
   listener.bind(('127.0.0.1',0));listener.listen();port=listener.getsockname()[1]
   client.connect(('127.0.0.1',port))
   accepted,_=listener.accept()
   try:
    args=types.SimpleNamespace(interface='lo',known_mac='28:cd:c1:09:78:41',
        local_ip='127.0.0.1',port=port,host_pid=os.getpid(),device='lcd',socket=Path(path))
    state=dict(boot=f'test-{os.getpid():x}',run='0',phase='idle',connection=1,session=2,target='lcd')
    row=dict(dst='127.0.0.1',lladdr=args.known_mac,state=['STALE'])
    cookie_box=[[1234,0]]
    changed_on_second_query=[False]
    class Diag:
     def query(self,target,cookie=None):
      value=[1235,0]if cookie is not None and changed_on_second_query[0]else cookie_box[0]
      return dict(inode=target['inode'],cookie=value,state=1)
     def close(self):pass
    observer=types.SimpleNamespace(Diag=Diag,stamp=lambda:{},validate_target=lambda target:None)
    def output(command,**kwargs):
     if 'neigh'in command:return json.dumps([row])
     if 'route'in command:return json.dumps([dict(dst='127.0.0.1',dev='lo',prefsrc='127.0.0.1')])
     raise AssertionError(command)
    with patch.object(m.subprocess,'check_output',side_effect=output):
     before=m.refresh_target(args,lambda _:state,observer,expected_phase='idle',
                             bound_cached_scope=True)
     self.assertTrue(before['scope_decision']['scope_bound'])
     self.assertFalse(before['scope_decision']['recent_kernel_reachability'])
     state.update(run='one',phase='armed');row['state']=['DELAY']
     after=m.refresh_target(args,lambda _:state,observer,expected_phase='armed',
                            bound_cached_scope=True,prior=before)
     self.assertTrue(after['scope_decision']['scope_bound'])
     self.assertEqual(after['scope_decision']['neighbor_state'],'DELAY')
     changed_on_second_query[0]=True
     with self.assertRaisesRegex(ValueError,'socket cookie changed'):
      m.refresh_target(args,lambda _:state,observer,expected_phase='armed',
                       bound_cached_scope=True,prior=before)
     changed_on_second_query[0]=False
     cookie_box[0]=[1235,0]
     with self.assertRaisesRegex(ValueError,'pre/post ARM target/socket binding changed'):
      m.refresh_target(args,lambda _:state,observer,expected_phase='armed',
                       bound_cached_scope=True,prior=before)
   finally:accepted.close()
 def test_preflight_socket_cookie_and_binding_must_survive_arm(self):
  target=dict(interface='eth0',local_ip='192.0.2.120',local_port=16153,
              peer_ip='192.168.0.173',peer_port=55555,inode=123,pid=456,
              process_start_ticks=789,cookie=[1,2],known_device_mac='28:cd:c1:09:78:41',
              binding=dict(boot='boot',connection=12,session=2318,target='PicoMado-lcd1',
                           phase='idle',run='0'))
  after=json.loads(json.dumps(target));after['binding'].update(phase='armed',run='new-run')
  m.require_same_target(target,after)
  after['cookie']=[1,3]
  with self.assertRaisesRegex(ValueError,'changed after ARM'):m.require_same_target(target,after)
  after['cookie']=[1,2];after['binding']['connection']=13
  with self.assertRaisesRegex(ValueError,'changed after ARM'):m.require_same_target(target,after)
 def test_start_before_commit_and_lost_ack_no_retry(self):
  events=[]
  class Session:
   def until(self,name,timeout):events.append(name);return {'event':name}
   def start(self):events.append('START')
  class USB:
   def request(self,command,end):events.append(command);raise TimeoutError('lost ACK')
  with tempfile.TemporaryDirectory()as d,self.assertRaises(TimeoutError):m.observe_and_commit(Session(),USB(),'a',1,2,Path(d))
  self.assertEqual(events[:3],['prepared','START','started']);self.assertEqual(len([x for x in events if 'COMMIT'in x]),1)
 def test_cleanup_stops_and_reaps_despite_serial_error(self):
  events=[]
  class USB:
   def request(self,*_):raise EOFError('gone')
  class Session:
   def stop(self):events.append('stop')
   def close_control(self):events.append('close')
   def finish(self,n):events.append(('finish',n));return {'exit':0}
  with tempfile.TemporaryDirectory()as d:r=m.cleanup(Session(),USB(),'a',3,Path(d))
  self.assertIn('sim_stop_uncertain',r);self.assertEqual(events,['stop','close',('finish',3)])
 def test_default_plan_does_not_open_serial_or_observer(self):
  a=self.args(Path('/tmp/not-created'),execute=False)
  with patch.object(m,'Serial',side_effect=AssertionError('USB')),patch.object(m.subprocess,'check_output',side_effect=AssertionError('network')),contextlib.redirect_stdout(io.StringIO())as out:m.run(a)
  self.assertTrue(json.loads(out.getvalue())['plan_only'])
 def test_inner_feature_failure_before_H_release_or_ARM(self):
  log=self.scenario('CY1 CAPS',require_inner=True);self.assertNotIn('host arm',log);self.assertNotIn('host release',log);self.assertFalse(any('SIM COMMIT'in x for x in log))
 def test_inner_prepare_failure_before_button_job(self):
  log=self.scenario('CY1 STATUS',require_inner=True);self.assertEqual(log.count('host arm'),1);self.assertFalse(any('SIM BEGIN'in x or'SIM COMMIT'in x for x in log));self.assertIn('host freeze',log)
 def test_tx_digest_explicit_signed_return(self):
  row=dict(t=1,s=2,e=3,a=4,dt=5,ip=6,len=7,rc=-1,nr=8,f=9,ref=10)
  self.assertEqual(m.tx_digest([row]),m.tx_digest([row|{'rc':255}]))
  self.assertNotEqual(m.tx_digest([row]),m.tx_digest([row|{'s':3}]))
 @staticmethod
 def args(out,execute=True):return types.SimpleNamespace(execute=execute,key=1,hold_ms=100,delay_ms=1000,device='unit',serial='/dev/fake',socket=Path('/tmp/unit.sock'),host_pid=1,interface='eth0',known_mac='00:00:00:00:00:01',local_ip='192.0.2.120',port=16153,observer_base=Path('/tmp'),out=out,release_saved=None)
 def scenario(self,fail,require_inner=False):
  log=[];owner=self;hs=dict(boot='a-1',run='0',last_run='0',phase='idle',target='unit',connection=2,session=3);open_usb=False
  class USB:
   on_error=None
   def __init__(self,path):pass
   def __enter__(self):nonlocal open_usb;open_usb=True;log.append('open');return self
   def __exit__(self,k,v,t):
    nonlocal open_usb
    if k and self.on_error:self.on_error()
    open_usb=False;log.append('close')
   def request(self,command,end):
    log.append(command)
    if fail and fail in command:raise TimeoutError('injected '+fail)
    if command=='AP2 ID':line='ap2 identity boot=b device=unit last=0'
    elif command=='AP2 STATUS':line='ap2 status boot=b state=0'
    elif command=='SIM CAPS':line='sim caps b=b keys=4 s=3 state=3'
    elif command.startswith('SIM BIND'):line=f'sim bind b=b rb=b r={hs["run"]} s=3 j={owner.job:x} state=3'
    elif command=='CY1 CAPS':line='cy1 caps v=2 rows=f crc=100 off=e depth=4 row=98 src=cyw43-inner-v2:7f4c9375'
    elif command.startswith('AP2 ARM'):line='ap2 status boot=b run='+hs['run']+' state=4'
    elif command=='AP2 LIVE':line='ap2 live boot=b session=3 active=1 gen=1 state=4 trigger=0 buttons=0 begin=0'
    elif command.startswith('AP2 FREEZE'):line='ap2 status boot=b run='+hs['run']+' state=3'
    elif command.startswith('SIM '):
     w=command.split();
     if w[1]=='BEGIN':owner.job=int(w[4],16)
     result={'BEGIN':'staging','STEP':'staged','COMMIT':'accepted','STOP':'ok'}[w[1]];line=f'sim ack b=b cid={w[3]} job={w[4]if len(w)>4 else "0"} result={result}'
    else:raise AssertionError(command)
    return [line]
  def host(path,q):
   log.append('host '+q['op'])
   if q['op']=='arm':hs.update(phase='armed',run=q['run'])
   elif q['op']=='freeze':hs['phase']='frozen'
   return hs.copy()
  class Session:
   def __init__(self,*_,**kw):owner.assertEqual(kw['seconds'],60);log.append('observer create')
   def until(self,event,timeout):log.append(event);return {'event':event}
   def start(self):log.append('observer START')
   def stop(self):log.append('observer STOP')
   def close_control(self):log.append('observer close')
   def finish(self,timeout):log.append('observer reap');return {'exit':0}
  def observe(session,out):
   self.assertFalse(open_usb);log.append('observe finite');return {'quality_complete':True}
  def target(args,host,observer):return {'binding':{k:hs[k]for k in ['run','connection','session']}}
  client=types.SimpleNamespace(Session=Session,observer=object())
  with tempfile.TemporaryDirectory()as d:
   a=self.args(Path(d)/'trial');a.require_inner=require_inner
   with patch.object(m,'Serial',USB),patch.object(m.ap,'host_request',host),patch.object(m,'refresh_target',target),patch.object(m,'finish_observer',observe),patch.object(m.ap,'wait_for_window'),patch.object(m.ap,'read_pico',side_effect=lambda *_:{'status':{'digest':'1'},'complete':True}),patch.object(m.ap,'read_host',return_value={'status':{'digest':2},'usable_without_overwrite':True}),patch.object(m,'collect_extra',return_value={'quality':{}}),patch.dict(sys.modules,{'observer_client':client}),contextlib.redirect_stdout(io.StringIO()):
    if fail:
     with self.assertRaises(TimeoutError):m.run(a)
     self.assertTrue((a.out/'failed.json').exists());self.assertFalse((a.out/'saved.json').exists())
    else:m.run(a);self.assertTrue((a.out/'saved.json').exists())
   self.assertFalse(open_usb)
  return log
 def test_one_trial_closes_before_operation_observation(self):
  log=self.scenario(None);self.assertEqual(sum('SIM COMMIT' in x for x in log),1);self.assertEqual(log.count('host arm'),1);self.assertLess(log.index('close'),log.index('observe finite'));self.assertNotIn('host release',log)
 def test_each_serial_failure_is_finite_no_rearm(self):
  for stage in ['AP2 ARM','SIM BEGIN','SIM STEP','SIM COMMIT','AP2 FREEZE']:
   with self.subTest(stage=stage):
    log=self.scenario(stage);self.assertEqual(log.count('host arm'),1);self.assertLessEqual(sum('SIM COMMIT'in x for x in log),1);self.assertNotIn('host release',log)
    if stage!='AP2 FREEZE':self.assertTrue(any('SIM STOP'in x for x in log))
    if stage=='SIM COMMIT':self.assertIn('observer STOP',log);self.assertIn('observer reap',log)
 def test_saved_release_rejects_unknown_or_same_boot(self):
  class USB:
   def request(self,*_):return ['ap2 status boot=b state=0']
  host=lambda q:{'phase':'frozen'}
  with tempfile.TemporaryDirectory()as d,self.assertRaisesRegex(ValueError,'explicit'):m.release_prior(host,USB(),'unit',None,Path(d))
 def test_sim_error_exit_attempts_stop_before_dtr_close(self):
  events=[];obj=m.Serial('/dev/fake');obj.on_error=lambda:events.append('STOP')
  with patch.object(m.ap.Serial,'__exit__',side_effect=lambda *_:events.append('DTRclose')):obj.__exit__(TimeoutError,TimeoutError('lost'),None)
  self.assertEqual(events,['STOP','DTRclose'])
class CaptureTests(unittest.TestCase):
 def sample(self,bind_run='a',bad_digest=False,drop='0'):
  row=dict(i=0,t=100,s=1,e=11,a=3,len=10,ip=9,dt=2,rc=-1,nr=0,f=24,ref=1);digest=m.tx_digest([row])+(1 if bad_digest else 0)
  pico={'status':{'boot':'b','run':'a'},'records':[dict(event=24,a=100,b=0,c=0,d=1)],'window':{'begin':'0','end':'0'},'quality':{'buttons':'0'},'usable_without_overwrite':True,'observation_usb_overlap_detected':False}
  lines={'SIM BIND b':f'sim bind b=b rb=b r={bind_run} s=3 j=64 state=3','AP2 SYNTH':'ap2 synth boot=b run=a n=1 job=64 step=0','SIM STATUS b':'sim status b=b j=64 phase=4 n=1 a=1 d=1 r=1 c=0 why=0 drops=0','SIM GET b 64 0':'sim step j=64 i=0 a=1 d=1 r=1 c=0 key=1 logical=1 dt=10 rt=20','TX1 STATUS':f'tx1 status b=b r=a n=1 over={drop} gen=1 active=0 ok=1 changed=0 bad=0 h={digest:x}','TX1 READ b a 0':'tx1 row '+' '.join(f'{k}={v if k=="rc" else format(v,"x")}'for k,v in row.items())}
  usb=types.SimpleNamespace(request=lambda command,end:[lines[command]])
  return usb,pico
 def test_tx_sim_freeze_collection_persist_roundtrip(self):
  usb,pico=self.sample();extra=m.collect_extra(usb,pico,100)
  self.assertTrue(extra['quality']['synthetic_origin_validated']);self.assertTrue(extra['quality']['operation_dispatched_and_normally_released']);self.assertFalse(extra['quality']['dispatch_is_render_success'])
  with tempfile.TemporaryDirectory()as d:
   p=Path(d)/'extra.json';sha=m.ap.persist(p,extra);self.assertEqual(sha,hashlib.sha256(p.read_bytes()).hexdigest());self.assertEqual(json.loads(p.read_text())['tx_rows'][0]['rc'],-1)
 def test_binding_mismatch_rejected(self):
  usb,pico=self.sample(bind_run='b')
  with self.assertRaisesRegex(ValueError,'binding mismatch'):m.collect_extra(usb,pico,100)
 def test_tx_corruption_rejected(self):
  usb,pico=self.sample(bad_digest=True)
  with self.assertRaisesRegex(ValueError,'digest mismatch'):m.collect_extra(usb,pico,100)
 def test_tx_drop_is_incomplete_quality(self):
  usb,pico=self.sample(drop='1');self.assertFalse(m.collect_extra(usb,pico,100)['quality']['TX_no_drop'])
 def test_prior_release_preserves_files_and_checks_boot_and_sha(self):
  for fault in ['none','same_boot','SHA']:
   with self.subTest(fault=fault),tempfile.TemporaryDirectory()as d:
    root=Path(d);saved_dir=root/'prior';saved_dir.mkdir();out=root/'out';out.mkdir()
    for name in ['pico','host']:(saved_dir/(name+'.json')).write_text('{}')
    sha=hashlib.sha256(b'{}').hexdigest();saved={'device':'unit','pico_boot':'old','host_boot':'c-1','run':'a','host_digest':2,'pico_sha256':sha,'host_sha256':sha};(saved_dir/'saved.json').write_text(json.dumps(saved))
    if fault=='SHA':(saved_dir/'host.json').write_text('changed')
    prior={p.name:p.read_bytes()for p in saved_dir.iterdir()};calls=[]
    def host(q):calls.append(q);return {'phase':'idle'}if q['op']=='release'else {'phase':'frozen','boot':'c-1','run':'a','digest':2}
    usb=types.SimpleNamespace(request=lambda *_:['ap2 status boot='+('old'if fault=='same_boot'else'new')+' state=0'])
    with patch.object(m.ap,'read_host',return_value={'complete':True}):
     if fault=='none':self.assertEqual(m.release_prior(host,usb,'unit',saved_dir,out)['phase'],'idle');self.assertTrue((out/'prior-preserved.json').exists())
     else:
      with self.assertRaises(ValueError):m.release_prior(host,usb,'unit',saved_dir,out)
      self.assertFalse(any(q['op']=='release'for q in calls))
    self.assertEqual(prior,{p.name:p.read_bytes()for p in saved_dir.iterdir()})
 def test_uncertain_cleanup_keeps_exact_pid_metadata(self):
  class Session:
   process=types.SimpleNamespace(pid=123)
   events=[{'event':'prepared','pid':124}]
   def stop(self):pass
   def close_control(self):pass
   def finish(self,_):raise TimeoutError('guard termination uncertain')
  with tempfile.TemporaryDirectory()as d:
   result=m.cleanup(Session(),None,None,1,Path(d));self.assertEqual(result['observer_controller_pid'],123);self.assertEqual(result['observer_last_metadata'][0]['pid'],124);self.assertIn('uncertain',result['observer_cleanup_uncertain'])
if __name__=='__main__':unittest.main()
