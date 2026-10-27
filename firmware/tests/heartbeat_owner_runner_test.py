"""Runner integration specification. Fresh PTYs only; DTR bits are modeled."""
import copy
import errno
import hashlib
import json
import os
import select
import struct
import sys
import tempfile
import termios
import threading
import time
import unittest
from pathlib import Path
from unittest.mock import patch
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
from heartbeat_capture_test import fixture,seal,mixed_fixture
import heartbeat_capture_runner as run


class Clock:
    def __init__(self,device):self.value=0.;self.device=device
    def now(self):return self.value
    def now_ns(self):return int(round(self.value*1e9))
    def sleep(self,n):self.value+=n;self.device.finish_window()


class Model:
    def __init__(self,scenario='normal'):
        self.scenario=scenario;self.data=fixture();seal(self.data)
        self.pstate='0';self.prun='0';self.pboot='1';self.triggered=False;self.bits=True
        self.hstate='idle';self.hrun='0';self.last=7;self.expected=8;self.htrigger=False
        self.sim_phase='0';self.sim_job='0'
        self.commands=[];self.host_calls=[];self.stop=threading.Event();self.errors=[]
        self.master,slave=os.openpty();self.path=os.ttyname(slave);os.close(slave)
        assert self.path.startswith('/dev/pts/');os.set_blocking(self.master,False)
        self.thread=threading.Thread(target=self.peer,daemon=True);self.thread.start()
        self.low_calls=self.high_calls=0;self.failed_get=False
    def send(self,data):
        for at in range(0,len(data),7):
            part=data[at:at+7]
            while part and not self.stop.is_set():
                try:n=os.write(self.master,part);part=part[n:]
                except BlockingIOError:time.sleep(.001)
    def peer(self):
        pending=b''
        while not self.stop.is_set():
            if not select.select([self.master],[],[],.02)[0]:continue
            try:part=os.read(self.master,4096)
            except OSError:time.sleep(.001);continue
            pending+=part
            while b'\n'in pending:
                line,pending=pending.split(b'\n',1);command=line.decode().strip()
                if not command:continue
                self.commands.append(command)
                try:reply=self.response(command)
                except BaseException as e:self.errors.append(e);return
                if reply is not None:self.send(('\n'.join(reply)+'\n').encode())
    def line(self,prefix,fields):return prefix+' '+' '.join(k+'='+str(v)for k,v in fields.items())
    def response(self,command):
        p,h,e,ready,headers=self.data;words=command.split()
        if self.scenario=='timeout'and command=='CY1 CAPS':return None
        if command=='AP2 CAPS':return ['ap2 caps v=3 hb=1 event=19']
        if command=='EV1 STATUS':return ['ev1 status b=1 on=0 seq=0 sat=0']
        if command=='CY1 CAPS':return [self.line('cy1 caps',run.hc.cy.SCHEMA)]
        if command=='SIM CAPS':return ['sim caps b=1 keys=4 steps=4 job_ms=3a98 hold_ms=1388 gate_ms=1f4 s=9 state=3']
        if words[:2]==['SIM','BEGIN']:
            self.sim_job=words[4];self.sim_phase='1'
            return [f'sim ack b=1 cid={words[3]} job={self.sim_job} result=staging']
        if words[:2]==['SIM','STEP']:
            return [f'sim ack b=1 cid={words[3]} job={self.sim_job} result=staged']
        if words[:2]==['SIM','COMMIT']:
            self.sim_phase='2'
            return [f'sim ack b=1 cid={words[3]} job={self.sim_job} result=accepted']
        if words[:2]==['SIM','STOP']:
            self.sim_phase='5'
            return [f'sim ack b=1 cid={words[3]} job={self.sim_job} result=ok']
        if words[:2]==['SIM','STATUS']:
            return [self.line('sim status',dict(b='1',j=self.sim_job,phase=self.sim_phase,n='1',
                    a='1'if self.sim_phase in ['2','3','4','5']else'0',
                    d='1'if self.sim_phase=='4'else'0',r='1'if self.sim_phase=='4'else'0',
                    c='1'if self.sim_phase=='5'else'0',why='1'if self.sim_phase=='5'else'0',drops='0'))]
        if words[:2]==['SIM','BIND']:
            return [f'sim bind b=1 rb=1 r=a j={self.sim_job} s=9 state=3']
        if words[:2]==['SIM','GET']:
            return [f'sim step j={self.sim_job} i=0 a=1 d={int(self.sim_phase=="4")} '
                    f'r={int(self.sim_phase=="4")} c={int(self.sim_phase=="5")} key=1 '
                    f'logical={1 if self.sim_phase=="4" else -1} dt=0 rt=0']
        if command=='AP2 SYNTH':
            syn=e.get('SIM',{}).get('synth',dict(boot='1',run='a',n='0',job='0',step='0')).copy()
            if not self.triggered:syn.update(n='0',job='0')
            return [self.line('ap2 synth',syn)]
        if command=='AP2 STATUS':
            status=p['status']|dict(boot=self.pboot,run=self.prun,state=self.pstate)
            if not self.triggered:status|=dict(n='0',digest='811c9dc5',over='0',gen='1')
            else:status|=dict(n=str(len(p['records'])))
            return [self.line('ap2 status',status)]
        if words[:2]==['AP2','ARM_HB']:
            self.prun=words[3];self.pstate='4'
            if self.scenario=='late_timeout':return None
            return self.response('AP2 STATUS')
        if command=='AP2 TRIGGER':
            if self.scenario=='prepared_race'and not self.triggered:self.last=8
            tr=e['trigger'].copy()
            if not self.triggered:tr|=dict(m='0',g='7',flags='0')
            return [self.line('ap2 trigger',tr)]
        if command=='AP2 LIVE':return ['ap2 live boot=1 session=9 active=1 gen=1 buttons=0 begin=0 state=4 trigger=0']
        if words[:2]==['AP2','FREEZE']:
            assert words[2]==self.pboot;self.pstate='3';return self.response('AP2 STATUS')
        if command=='AP2 WINDOW':
            w=dict(boot='1',run='a',trigger='0',freeze='2000000',begin='0',end='0')
            if self.triggered:w|=p['window']
            return [self.line('ap2 window',w)]
        if command=='AP2 QUALITY':return ['ap2 quality boot=1 run=a gate=1 buttons=0 overlap=0']
        if words[:2]==['AP2','READ']:
            if self.scenario=='read_timeout':return None
            start=int(words[4],16);rows=p['records'][start:start+8]if self.triggered else[]
            total=len(p['records'])if self.triggered else 0
            digest=p['status']['digest']if self.triggered else'811c9dc5'
            result=[f'ap2 page boot=1 run=a from={start:x} count={len(rows):x} total={total:x} digest={digest}']
            for i,r in enumerate(rows,start):
                result.append('ap2 i=%x t=%x e=%x k=%x g=%x m=%x s=%x f=%x'%(i,*[r[k]for k in ['us','event','kind','connection','message','session','frame']]))
                result.append('ap2 i=%x flags=%x a=%x b=%x c=%x d=%x'%(i,*[r[k]for k in ['flags','a','b','c','d']]))
            return result+[f'ap2 end from={start:x} count={len(rows):x} digest={digest}']
        if words[:2]==['AP2','RELEASE']:
            assert words[2]==self.pboot and self.pstate=='3';self.pstate='0';self.prun='0';return self.response('AP2 STATUS')
        if command=='TX1 STATUS':
            status=e['tx_status'].copy()
            if not self.triggered:status|=dict(n='0',h=format(run.hc.txlib.tx_digest([]),'x'))
            return [self.line('tx1 status',status)]
        if words[:2]==['TX1','READ']:
            row=e['tx_rows'][int(words[4],16)]
            return ['tx1 row '+' '.join(k+'='+(str(v)if k=='rc'else format(v,'x'))for k,v in row.items())]
        if command=='CY1 STATUS':
            status=e['SDK_inner']['status'].copy()
            if not self.triggered:status|=dict(n='0',h=format(run.hc.cy.digest([]),'x'))
            return [self.line('cy1 status',status)]
        if command=='CY1 QUALITY':return [self.line('cy1 quality',e['SDK_inner']['quality'])]
        if words[:2]==['CY1','READ']:
            r=e['SDK_inner']['rows'][int(words[4],16)][int(words[5],16)]
            return ['cy1 row '+' '.join(k+'='+(str(v)if k in ['rxrc','brc']else format(v,'x'))for k,v in r.items())]
        raise AssertionError(command)
    def host(self,q):
        self.host_calls.append(q.copy());h=self.data[1];op=q['op']
        if op=='arm':
            if q['baseline']!=self.last:raise ValueError('baseline')
            self.hstate='armed';self.hrun=q['run'];self.expected=q['expected_message']
            if self.scenario=='host_arm_timeout':raise TimeoutError('host arm response uncertain')
        if op=='status'and self.scenario=='host_down_after_arm'and self.hstate=='armed':raise TimeoutError('host control unavailable')
        if op=='status'and self.scenario=='mixed_interrupt'and self.sim_phase=='1':raise TimeoutError('pre-gate interrupted')
        if op=='freeze':assert q['boot']=='host'and q['run']==self.hrun;self.hstate='frozen'
        if op=='release':assert q['run']==self.hrun and self.hstate=='frozen';self.hstate='idle';self.hrun='0'
        s=h['status']|dict(phase=self.hstate,run=self.hrun,target='lcd',heartbeat_trigger=1,
                 connection=431,session=9,last_message=self.last,count=len(h['records'])if self.htrigger else 0,last_run='a')
        if not self.htrigger:s|=dict(trigger_us=0,freeze_us=21000000,digest=0)
        if op=='read':
            rows=h['records']if self.htrigger else[];at=q['start'];chunk=rows[at:at+16]
            return dict(status=s,start=at,count=len(chunk),records=chunk)
        return s
    def low(self):
        if self.scenario=='no_hb':return
        if self.scenario in ['mixed_after','mixed_before']:
            self.data=mixed_fixture(sim_first=self.scenario=='mixed_before')
            self.sim_phase='4'
        self.triggered=True;self.pstate='2';self.htrigger=True
        if self.scenario=='gate_race':self.data[2]['trigger'].update(g='8',m='9',flags='2')
        if self.scenario=='pending_hb':self.data[0]['records'][4]['flags']=13
        if self.scenario=='overflow':self.data[2]['SDK_inner']['status']['over']='1';self.data[2]['SDK_inner']['no_diagnostic_loss']=False
        if self.scenario=='ap_overflow':self.data[0]['status']['over']='1'
        if self.scenario=='tx_overflow':self.data[2]['tx_status']['over']='1'
        if self.scenario=='host_overflow':self.data[1]['status']['overwritten']=1
        if self.scenario=='copy_failure':
            self.data[0]['records'][5]['a']=0xffffffff;self.data[0]['records'][8]['a']=0
            self.data[2]['tx_rows']=[];self.data[2]['tx_status']['n']='0'
            self.data[2]['SDK_inner']['rows']=[];self.data[2]['SDK_inner']['status']['n']='0'
            self.htrigger=False
        seal(self.data)
    def finish_window(self):
        if self.triggered:self.pstate='3';self.hstate='frozen'if self.htrigger else'armed'
        if self.scenario=='reconnect':self.pboot='2';self.pstate='0';self.prun='0'
    def ioctl(self,fd,request,*args):
        if request==termios.TIOCMGET:
            if self.failed_get:raise OSError(errno.EIO,'DTR state unknown')
            return struct.pack('I',termios.TIOCM_DTR if self.bits else 0)
        if request in [termios.TIOCMBIS,termios.TIOCMBIC]:
            high=request==termios.TIOCMBIS
            if high:self.high_calls+=1
            else:self.low_calls+=1
            fail=(not high and self.scenario.startswith('low_fail'))or(high and self.scenario.startswith('high_fail'))
            changed=not fail or self.scenario.endswith('changed')
            if changed:
                self.bits=high
                if not high:self.low()
            if self.scenario=='get_fail'and not high:self.failed_get=True
            if fail:raise OSError(errno.EIO,'injected DTR failure')
            return 0
        raise AssertionError(request)
    def close(self):self.stop.set();self.thread.join(2);os.close(self.master)


class Observer:
    def __init__(self,target,model):self.target=target;self.model=model;self.stopped=False
    def until(self,event,timeout):
        if event=='started'and self.model.scenario=='unconfirmed_start':return dict(event=event)
        return dict(event=event,clock=dict(mono_before_ns=self.model.clock.now_ns(),mono_after_ns=self.model.clock.now_ns()))
    def start(self):self.started=self.model.clock.now_ns()
    def pump(self,timeout=0):pass
    def stop(self):self.stopped=True
    def finish(self,timeout):
        if self.model.scenario=='observer_timeout':raise TimeoutError('guard exit pending')
        summary=copy.deepcopy(self.model.data[4]);meta=[e for e in summary['metadata_events']if e.get('event')!='started']
        worker=next(e for e in meta if e['event']=='worker_finished')
        worker.update(capture_seconds=(self.model.clock.now_ns()-self.started)/1e9,capture_started=True,reason='stop_command',max_queue_sample_gap_ns=100000000,packet_stats=dict(kernel_drops=0,kernel_packets=1))
        if self.model.scenario=='header_overflow':worker['packet_stats']['kernel_drops']=1
        meta.insert(0,dict(event='started',clock=dict(mono_before_ns=self.started,mono_after_ns=self.started)))
        return dict(exit=0,events=[dict(event='packet',scope_valid=True),*meta,dict(event='guard_reaped',forced=False,child_exit=0)],stderr='')


class Tests(unittest.TestCase):
    def test_preflight_refusal_is_saved_before_usb_owner(self):
        with tempfile.TemporaryDirectory() as base:
            root=Path(base);observer=root/'observer';observer.mkdir()
            digests={}
            for name in ('header_observer.py','observer_client.py'):
                data=b'# isolated observer stub\n';(observer/name).write_bytes(data)
                digests[name]=hashlib.sha256(data).hexdigest()
            cfg=dict(device='PicoMado-lcd1',expected_flash_id='E661640843239D21',
                serial_by_id='/dev/serial/by-id/E661640843239D21',PID=os.getpid(),
                server_sha=hashlib.sha256(Path(f'/proc/{os.getpid()}/exe').read_bytes()).hexdigest(),
                observer=str(observer),observer_SHA256=digests,host_socket=str(root/'host.sock'),
                interface='eth0',known_mac='02:00:00:00:00:01',local_ip='192.0.2.120',port=16153)
            config=root/'config.json';config.write_text(json.dumps(cfg));out=root/'trial'
            with patch.object(sys,'argv',['heartbeat_capture_runner.py','--config',str(config),'--dir',str(out),'--execute']),\
                 patch.object(run.hc.txlib,'refresh_target',side_effect=ValueError('neighbor not REACHABLE')) as resolver,\
                 patch.object(run,'Owner',side_effect=AssertionError('USB opened')):
                with self.assertRaisesRegex(ValueError,'neighbor not REACHABLE'):run.main()
            self.assertTrue(resolver.call_args.kwargs['bound_cached_scope'])
            self.assertEqual(json.loads((out/'target-preflight-error.json').read_text())['USB_opened'],False)
            self.assertFalse((out/'owner.private.jsonl').exists())
    def scenario(self,name='normal',disk_failure=False,result_failure=False,sim=None,initial_clock=0):
        m=Model(name);ports=[]
        def port(path):
            self.assertEqual(path,m.path);p=run.ev.SerialPort(path,dtr=False);ports.append(p);return p
        try:
            with tempfile.TemporaryDirectory()as directory:
                root=Path(directory);owner=run.Owner(m.path,root/'owner.private.jsonl',port_factory=port,ioctl=m.ioctl,quiet_seconds=0,request_seconds=.2)
                clock=Clock(m);clock.value=initial_clock;m.clock=clock
                def target(ready):return dict(binding=dict(boot='host',run='a',connection=431,session=9,target='lcd'))
                runner=run.Runner(owner,m.host,target,lambda t:Observer(t,m),root,clock=clock.now,wait=clock.sleep,clock_ns=clock.now_ns,sim=sim)
                if disk_failure or result_failure:
                    original=runner.persist
                    def persist(path,value):
                        if path.name==('result.json'if result_failure else'pre-release.json'):raise OSError('disk full')
                        original(path,value)
                    runner.persist=persist
                result=runner.run('lcd','a')
                self.assertTrue(result['owner_closed']);self.assertIsNone(owner.fd);self.assertEqual(len(ports),1)
                self.assertFalse(m.errors,m.errors)
                if sim is None:self.assertFalse(any(c.startswith('SIM')for c in m.commands))
                self.assertEqual(result['observation_USB_io'],dict(reads=0,writes=0))
                return result,m.commands[:],m.host_calls[:],m.low_calls,m.high_calls
        finally:m.close()
    def test_normal_save_release_both_IDLE_no_rearm(self):
        r,commands,_,lo,hi=self.scenario();self.assertTrue(r['complete'],r)
        self.assertTrue(r['pico_idle']and r['host_idle']);self.assertEqual((lo,hi),(1,1))
        self.assertEqual(sum(c.startswith('AP2 ARM_HB')for c in commands),1)
        self.assertEqual(sum(c.startswith('AP2 RELEASE')for c in commands),1)
    def test_one_sim_after_heartbeat_is_complete_and_sim_first_is_rejected_but_released(self):
        plan=dict(key=1,delay_ms=14000,hold_ms=150)
        good,commands,_,lo,hi=self.scenario('mixed_after',sim=plan)
        self.assertTrue(good['complete'],good)
        self.assertEqual((lo,hi),(1,1));self.assertEqual(sum(c.startswith('SIM COMMIT')for c in commands),1)
        bad,commands,_,_,_=self.scenario('mixed_before',sim=plan)
        self.assertFalse(bad['complete']);self.assertTrue(bad['evidence_confirmed'])
        self.assertTrue(bad['pico_idle']and bad['host_idle'])
        self.assertEqual(sum(c.startswith('AP2 RELEASE')for c in commands),1)
    def test_mixed_save_failure_retains_ram_and_pre_gate_stop_cancels_sim(self):
        plan=dict(key=1,delay_ms=14000,hold_ms=150)
        bad,commands,_,_,_=self.scenario('mixed_after',disk_failure=True,sim=plan)
        self.assertFalse(bad['complete']);self.assertFalse(any(c.startswith('AP2 RELEASE')for c in commands))
        stopped,commands,_,lo,hi=self.scenario('mixed_interrupt',sim=plan)
        self.assertFalse(stopped['complete']);self.assertEqual((lo,hi),(0,0))
        self.assertEqual(sum(c.startswith('SIM STOP')for c in commands),1)
        self.assertTrue(stopped['pico_idle']and stopped['host_idle'])
    def test_same_session_monotonic_plan_is_bounded_and_saved_before_step(self):
        plan=dict(key=1,hold_ms=150,anchor_ns=1000000000,receipt_interval_ns=15000000000,anchor_session=9,
                  anchor_boot=Path('/proc/sys/kernel/random/boot_id').read_text().strip().replace('-',''),
                  gate_guard_ms=500,aim_gap_ms=200)
        self.assertEqual(run.next_heartbeat_sim_delay(plan,1800000000)['chosen_delay_ms'],13900)
        observed=plan|{'receipt_interval_ns':14993355000}
        self.assertEqual(run.next_heartbeat_sim_delay(observed,1800000000)['predicted_next_HB_ns'],15993355000)
        self.assertEqual(run.next_heartbeat_sim_delay(observed,1800000000)['chosen_delay_ms'],13893)
        with self.assertRaisesRegex(ValueError,'outside one finite job'):
            run.next_heartbeat_sim_delay(plan,900000000)
        good,commands,_,lo,hi=self.scenario('mixed_after',sim=plan,initial_clock=1.8)
        self.assertTrue(good['complete'],good)
        step=next(c for c in commands if c.startswith('SIM STEP'))
        self.assertEqual(int(step.split()[6],16),13900)
        self.assertEqual((lo,hi),(1,1))
        bad,commands,_,lo,hi=self.scenario('mixed_after',sim=plan,initial_clock=.9)
        self.assertFalse(bad['complete']);self.assertEqual((lo,hi),(0,0))
        self.assertTrue(any(c.startswith('SIM STOP') for c in commands))
        self.assertFalse(any(c.startswith('SIM COMMIT') for c in commands))
    def test_origin_race_and_pre_gate_change_are_quality_stops(self):
        for name in ['prepared_race','gate_race','pending_hb','copy_failure','no_hb']:
            with self.subTest(name=name):
                r,_,_,_,_=self.scenario(name);self.assertFalse(r['complete']);self.assertTrue(r['pico_idle']and r['host_idle'],r)
    def test_overflow_saved_but_not_complete(self):
        for name in ['overflow','ap_overflow','tx_overflow','host_overflow','header_overflow']:
            with self.subTest(name=name):
                r,_,_,_,_=self.scenario(name);self.assertFalse(r['complete']);self.assertTrue(r['pico_idle']and r['host_idle'],r)
    def test_unfinished_observer_prevents_release(self):
        r,commands,host,_,_=self.scenario('observer_timeout');self.assertFalse(r['complete'])
        self.assertFalse(any(c.startswith('AP2 RELEASE')for c in commands));self.assertFalse(any(q['op']=='release'for q in host))
    def test_START_confirmation_precedes_DTR(self):
        r,_,_,lo,hi=self.scenario('unconfirmed_start');self.assertFalse(r['complete'])
        self.assertEqual((lo,hi),(0,0));self.assertTrue(r['pico_idle']and r['host_idle'])
    def test_DTR_failures_not_retried_or_hidden(self):
        for name in ['low_fail','low_fail_changed','high_fail','high_fail_changed','get_fail']:
            with self.subTest(name=name):
                r,_,_,lo,hi=self.scenario(name);self.assertFalse(r['complete']);self.assertLessEqual(lo,1);self.assertLessEqual(hi,1)
    def test_uncertain_commands_do_not_reopen_or_retry(self):
        for name in ['timeout','late_timeout','read_timeout']:
            with self.subTest(name=name):
                r,commands,_,_,_=self.scenario(name);self.assertFalse(r['complete']);self.assertFalse(r['pico_idle'])
                if name=='late_timeout':self.assertEqual(sum(c.startswith('AP2 ARM_HB')for c in commands),1)
    def test_host_arm_timeout_resolved_without_arm_retry(self):
        r,_,host,_,_=self.scenario('host_arm_timeout');self.assertFalse(r['complete'])
        self.assertTrue(r['pico_idle']and r['host_idle'],r);self.assertEqual(sum(q['op']=='arm'for q in host),1)
    def test_host_failure_still_freezes_owned_Pico(self):
        r,commands,_,_,_=self.scenario('host_down_after_arm');self.assertFalse(r['complete'])
        self.assertTrue(any(c.startswith('AP2 FREEZE 1 a')for c in commands));self.assertFalse(any(c.startswith('AP2 RELEASE')for c in commands))
    def test_reconnect_does_not_mutate_new_boot(self):
        r,commands,_,_,_=self.scenario('reconnect');self.assertFalse(r['complete']);self.assertFalse(r['pico_idle'])
        self.assertFalse(any(c.startswith(('AP2 FREEZE 2','AP2 RELEASE 2'))for c in commands))
    def test_save_failure_retains_RAM(self):
        r,commands,host,_,_=self.scenario(disk_failure=True);self.assertFalse(r['complete'])
        self.assertFalse(any(c.startswith('AP2 RELEASE')for c in commands));self.assertFalse(any(q['op']=='release'for q in host))
    def test_final_report_failure_is_not_success(self):
        r,_,_,_,_=self.scenario(result_failure=True);self.assertFalse(r['complete'])
        self.assertTrue(r['pico_idle']and r['host_idle'])


if __name__=='__main__':unittest.main()
