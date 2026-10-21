import copy
import hashlib
import json
import os
import pty
import select
import sys
import tempfile
import threading
import unittest
import zlib
from unittest.mock import patch
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
import heartbeat_capture as hc


def fixture():
    ready = dict(pico_boot='1', host_boot='host', run='a', device='lcd',
                 host_connection=431, session=9, baseline=7, expected=8, generation=1,ev1_sequence='0')
    records = []
    for i, event in enumerate([25, 12, 13, 14, 15, 16, 23, 17, 18]):
        records.append(dict(us=100+i, event=event, kind=5, connection=1, message=8,
                            session=9, frame=0, flags=0, a=0, b=0, c=0, d=0))
    records[0].update(a=7, b=7, d=1)
    records[1].update(a=123, b=87)
    records[3].update(a=123, b=87)
    records[4].update(flags=9, a=100, b=100)
    records[6].update(a=100, b=199, c=100, d=100)
    records[8]['a'] = 1
    pico = dict(status=dict(boot='1', run='a', state='3', over='0', gen='1', digest='0'),
                window=dict(trigger='64', freeze=format(20000100, 'x')),
                prepared_input_detected=False, observation_usb_overlap_detected=False,
                post_freeze_buttons_detected=False, records=records)
    decoded = dict(us=200, event='decoded', connection=431, message=8, kind=5,
                   session=9, frame=0, a=123, b=87)
    host = dict(status=dict(boot='host', run='a', phase='frozen', trigger_mode='heartbeat',
                baseline_message=7, expected_message=8, trigger_message=8, hb_connection=431,
                hb_session=9, overwritten=0, pre_overwritten=0, contended=0,
                digest=0, trigger_us=200, freeze_us=20000200), records=[decoded])
    tx = dict(i=0, t=200, s=100, e=199, a=450, len=99, ip=46, dt=10, rc=0, nr=0, f=24, ref=1)
    key = dict(i=0, p=1, s=100, e=199, a=450, ip=46, len=99, et=2048, f=24, v=1)
    parts = [dict(i=0, p=0, t=201, bus=205, end=209, id=17, flags=25, dep=1, nest=0),
             key, key | dict(p=2),
             dict(i=0, p=3, c0=45, cr=0, cb=45, b0=0, bb=0, off=14, cov=157, domain=157),
             dict(i=0, p=4, wait=0, polls=0, d=0, c=0, a=0, bad=0, rxrc=0, n=0),
             dict(i=0, p=5, calls=0, req=0, errs=0, f0=0, f1=0, c0=1, c1=1, s0=1, s1=2, brc=0)]
    inner = dict(caps=hc.cy.SCHEMA.copy(), status=dict(b='1', r='a', gen='1', active='0',
                 n='1', over='0', inc='0', changed='0', h='0'),
                 quality=dict(sat='0', depth='0', reject='0'), rows=[parts], no_diagnostic_loss=True)
    extra = dict(trigger=dict(b='1', r='a', mode='1', o='7', g='7', m='8', flags='0'),
                 EV1=dict(b='1',on='0',sat='0',seq='0'),tx_status=dict(b='1', r='a', gen='1', active='0', n='1', ok='1',
                                over='0', bad='0', changed='0', h='0'), tx_rows=[tx], SDK_inner=inner)
    worker = dict(event='worker_finished', packets=1, reason='stop_command',capture_started=True,capture_seconds=37.05,max_queue_sample_gap_ns=100000000,packet_stats=dict(kernel_drops=0,kernel_packets=1),
                  sockets_closed=True, payload_copied=False, payload_persisted=False)
    headers = dict(controller=dict(clock='CLOCK_MONOTONIC',start_request_before_ns=100000000000,start_confirm_after_ns=100200000000,DTR_low_before_ns=101000000000,DTR_low_after_ns=101001000000,required_end_ns=137100000000,stop_request_before_ns=137100000000,finished_after_ns=137200000000,wait_budget_ns=16000000000,errors=[]),complete_interval=True, payload_copied_or_persisted=False, scope_valid=True,
                   target_binding={k:ready[k] for k in ['pico_boot','host_boot','run','device','host_connection','session']},
                   metadata_events=[dict(event='started',clock=dict(mono_before_ns=100100000000,mono_after_ns=100100001000)),worker],SHA256={n:hashlib.sha256(raw).hexdigest() for n,raw in
                       [('headers.jsonl',b'{"scope_valid":true}\n'),('recvq.jsonl',b'')]})
    return [pico, host, extra, ready, headers]


def seal(f):
    p, h, e, _, _ = f
    p['status']['digest'] = format(hc.ap.fnv(p['records']), 'x')
    digest = 0
    for r in h['records']:
        digest = zlib.crc32(json.dumps({k:r[k] for k in ['us','event','connection','message','session','frame','kind','a','b']},separators=(',',':')).encode(),digest)
    h['status']['digest'] = digest
    e['tx_status']['h'] = format(hc.txlib.tx_digest(e['tx_rows']), 'x')
    e['SDK_inner']['status']['h'] = format(hc.cy.digest(e['SDK_inner']['rows']), 'x')


class Validation(unittest.TestCase):
    def result(self, f):
        seal(f)
        return hc.validate(*f).record()

    def test_exact_target(self):
        self.assertTrue(self.result(fixture())['measurement_complete'])

    def test_origin_and_copy_fault_matrix(self):
        faults = [lambda f:f[2]['trigger'].update(g='8'),
                  lambda f:f[2]['trigger'].update(flags='1'),
                  lambda f:f[2]['trigger'].update(m='9'),
                  lambda f:f[0]['records'][0].update(c=2),
                  lambda f:f[0]['records'][4].update(flags=13),
                  lambda f:f[0]['records'][6].update(d=99),
                  lambda f:f[0]['records'][5].update(a=0xffffffff),
                  lambda f:f[0]['records'][8].update(a=0),
                  lambda f:f[0]['records'][6].update(b=198),
                  lambda f:f[0]['records'][1].update(session=10),
                  lambda f:f[0]['records'].append(copy.deepcopy(f[0]['records'][0]))]
        for fault in faults:
            with self.subTest(fault=fault):
                f=fixture();fault(f);self.assertFalse(self.result(f)['measurement_complete'])

    def test_old_prepared_late_identical_crc_cannot_alias(self):
        for ordinal in [7,9]:
            f=fixture();f[1]['records'][0]['message']=ordinal
            self.assertFalse(self.result(f)['measurement_complete'])
        f=fixture();f[0]['records'][0]['message']=9;f[2]['trigger'].update(g='8',m='9')
        self.assertFalse(self.result(f)['measurement_complete'])
        f=fixture();f[1]['records'].append(copy.deepcopy(f[1]['records'][0]))
        self.assertFalse(self.result(f)['measurement_complete'])

    def test_all_loss_and_binding_conditions_remain_fatal(self):
        faults=[lambda f:f[0]['status'].update(over='1'),
                lambda f:f[0]['status'].update(gen='2'),
                lambda f:f[0].update(observation_usb_overlap_detected=True),
                lambda f:f[0].update(prepared_input_detected=True),
                lambda f:f[1]['status'].update(pre_overwritten=1),
                lambda f:f[1]['status'].update(contended=1),
                lambda f:f[1]['status'].update(overwritten=1),
                lambda f:f[1]['status'].update(hb_connection=432),
                lambda f:f[1]['records'][0].update(connection=432),
                lambda f:f[1]['records'][0].update(kind=15),
                lambda f:f[2]['tx_status'].update(over='1'),
                lambda f:f[2]['tx_status'].update(changed='1'),
                lambda f:f[2]['tx_status'].update(bad='1'),
                lambda f:f[2]['SDK_inner']['status'].update(over='1'),
                lambda f:f[2]['SDK_inner']['quality'].update(depth='1'),
                lambda f:f[2]['SDK_inner'].update(no_diagnostic_loss=False),
                lambda f:f[4].update(scope_valid=False),
                lambda f:next(e for e in f[4]['metadata_events']if e['event']=='worker_finished')['packet_stats'].update(kernel_drops=1)]
        for fault in faults:
            with self.subTest(fault=fault):
                f=fixture();fault(f);self.assertFalse(self.result(f)['measurement_complete'])

    def test_missing_CY_is_not_waived(self):
        f=fixture();f[2]['SDK_inner']['rows']=[];f[2]['SDK_inner']['status']['n']='0'
        self.assertFalse(self.result(f)['measurement_complete'])

    def test_fifteen_rows_and_sixteenth_drop(self):
        f=fixture();e=f[2]
        for i in range(1,15):
            tx=copy.deepcopy(e['tx_rows'][0]);tx.update(i=i,t=200+i*20,ip=46+i)
            cy=copy.deepcopy(e['SDK_inner']['rows'][0])
            for p in cy:p['i']=i
            cy[0].update(t=tx['t']+1,bus=tx['t']+5,end=tx['t']+9,id=(i+1)*16+i+1)
            cy[1]['ip']=cy[2]['ip']=46+i
            e['tx_rows'].append(tx);e['SDK_inner']['rows'].append(cy)
        e['tx_status']['n']=e['SDK_inner']['status']['n']='f'
        self.assertTrue(self.result(f)['measurement_complete'])
        e['SDK_inner']['status']['over']='1';e['SDK_inner']['no_diagnostic_loss']=False
        self.assertFalse(self.result(f)['measurement_complete'])

    def test_late_or_missing_trigger_and_short_window(self):
        for who in [0,1]:
            f=fixture()
            if who==0:f[0]['window']['freeze']='65'
            else:f[1]['status']['trigger_us']=0
            self.assertFalse(self.result(f)['measurement_complete'])

    def test_input_is_reported_as_input(self):
        f=fixture();f[0]['records'].append(f[0]['records'][0] | dict(event=24))
        result=self.result(f);self.assertFalse(result['measurement_complete']);self.assertEqual(result['input_count'],1)

    def test_saved_digest_tamper(self):
        f=fixture();seal(f);f[0]['records'][1]['a']+=1
        self.assertIn('Pico digest',hc.validate(*f).errors)


class Prepare(unittest.TestCase):
    class USB:
        fd=7
        def __init__(self):self.calls=[];self.old_caps=False;self.old_live=False;self.state='0';self.generation=1
        def request(self,command,prefix):
            self.calls.append(command)
            if command=='AP2 CAPS':return ['ap2 caps v=3 hb=1 event=19' if not self.old_caps else 'ap2 caps v=2']
            if command=='CY1 CAPS':return ['cy1 caps '+ ' '.join(k+'='+v for k,v in hc.cy.SCHEMA.items())]
            if command=='EV1 STATUS':return ['ev1 status b=1 on=0 sat=0 seq=0']
            if command.startswith('AP2 ARM_HB'):self.state='4'
            if prefix=='ap2 status ':return [f'ap2 status boot=1 run={"0" if self.state=="0" else "a"} state={self.state} gen={self.generation}']
            if command=='AP2 TRIGGER':return ['ap2 trigger b=1 r=a mode=1 o=7 g=7 m=0 flags=0']
            if command=='AP2 LIVE':return [f'ap2 live boot=1 session=9 active=1 gen={self.generation:x} buttons=0 begin=0 state=4 trigger=0'+('' if self.old_live else ' message=7')]
            raise AssertionError(command)
    def host(self,log,bad=False):
        state=dict(phase='idle',boot='host',run='0',target='lcd',heartbeat_trigger=1,connection=431,session=9,last_message=7)
        def call(q):
            log.append(q.copy())
            if q['op']=='arm':
                if bad:raise ValueError('baseline')
                state.update(phase='armed',run='a',baseline_message=7,expected_message=8)
            return state.copy()
        return call
    def test_prepare_strict_origin_no_SIM_and_mutator_once(self):
        usb=self.USB();log=[]
        with tempfile.TemporaryDirectory()as d:
            result=hc.prepare(usb,self.host(log),'lcd','a',Path(d))
            self.assertEqual(result['expected'],8)
            self.assertEqual(sum(c.startswith('AP2 ARM_HB')for c in usb.calls),1)
            self.assertFalse(any('SIM'in c for c in usb.calls))
            self.assertTrue((Path(d)/'ready.json').exists())
    def test_caps_refusal_before_arm(self):
        usb=self.USB();usb.old_caps=True;log=[]
        with tempfile.TemporaryDirectory()as d, self.assertRaises(ValueError):hc.prepare(usb,self.host(log),'lcd','a',Path(d))
        self.assertFalse(any('ARM'in c for c in usb.calls));self.assertFalse(log)
    def test_baseline_refusal_retains_partial_arm_no_retry(self):
        usb=self.USB();log=[]
        with tempfile.TemporaryDirectory()as d:
            with self.assertRaises(ValueError):hc.prepare(usb,self.host(log,True),'lcd','a',Path(d))
            self.assertTrue((Path(d)/'identity.json').exists());self.assertFalse((Path(d)/'ready.json').exists())
            before=json.loads((Path(d)/'baseline-before-arm.json').read_text())
            refused=json.loads((Path(d)/'baseline-arm-refusal.json').read_text())
            self.assertEqual(before['host_before']['value']['last_message'],7)
            self.assertEqual(refused['host_after']['value']['last_message'],7)
            self.assertEqual(before['pico_before']['value']['message'],'7')
            self.assertEqual(refused['pico_after']['value']['message'],'7')
            self.assertLess(before['host_before']['response_after_ns'],refused['arm_request_before_ns'])
            self.assertLess(refused['arm_response_after_ns'],refused['host_after']['request_before_ns'])
        self.assertEqual(sum(c.startswith('AP2 ARM_HB')for c in usb.calls),1)
        self.assertEqual(sum(q['op']=='arm'for q in log),1)
    def test_refusal_bracket_keeps_moving_host_watermark_bounded(self):
        usb=self.USB();status=self.host([],True);reads=0
        def moving_host(query):
            nonlocal reads
            if query['op']=='status':
                reads+=1
                value=status(query)
                if reads==3:value['last_message']=8
                return value
            return status(query)
        with tempfile.TemporaryDirectory()as d:
            with self.assertRaisesRegex(ValueError,'baseline'):
                hc.prepare(usb,moving_host,'lcd','a',Path(d))
            before=json.loads((Path(d)/'baseline-before-arm.json').read_text())
            after=json.loads((Path(d)/'baseline-arm-refusal.json').read_text())
            self.assertEqual(before['host_before']['value']['last_message'],7)
            self.assertEqual(after['host_after']['value']['last_message'],8)
            self.assertEqual(sum(c.startswith('AP2 ARM_HB')for c in usb.calls),1)
    def test_old_firmware_still_records_host_refusal_bracket(self):
        usb=self.USB();usb.old_live=True
        with tempfile.TemporaryDirectory()as d:
            with self.assertRaisesRegex(ValueError,'baseline'):
                hc.prepare(usb,self.host([],True),'lcd','a',Path(d))
            before=json.loads((Path(d)/'baseline-before-arm.json').read_text())
            after=json.loads((Path(d)/'baseline-arm-refusal.json').read_text())
            self.assertNotIn('message',before['pico_before']['value'])
            self.assertIsNotNone(after['host_after'])
    def test_status_decimal_live_hex_generation_is_one_binding(self):
        usb=self.USB();usb.generation=15
        with tempfile.TemporaryDirectory()as d:
            ready=hc.prepare(usb,self.host([]),'lcd','a',Path(d))
            self.assertEqual(ready['generation'],15)
            bracket=json.loads((Path(d)/'baseline-before-arm.json').read_text())
            self.assertEqual(bracket['pico_before']['value']['gen'],'f')
    def test_observation_finite_and_finally_restores_DTR(self):
        for fail in [False,True]:
            log=[]
            def wait(n):
                log.append(('wait',n))
                if fail:raise RuntimeError('stop')
            args=(lambda x:log.append(('dtr',x)),lambda:(log.append(('headers','start'))or dict(event='started')),lambda:log.append(('headers','stop')))
            if fail:
                with self.assertRaises(RuntimeError):hc.observe(*args,wait=wait)
            else:hc.observe(*args,wait=wait)
            self.assertEqual(log,[('headers','start'),('dtr',False),('wait',36.1),('headers','stop'),('dtr',True)])
    def test_observation_bound_rejects_before_IO(self):
        with self.assertRaises(ValueError):hc.observe(lambda _:self.fail(),self.fail,self.fail,max_wait=17)

    def test_real_PTY_split_commands_and_exclusive_owner(self):
        import fcntl
        import termios
        master,slave=pty.openpty();path=os.ttyname(slave);os.close(slave)
        self.assertTrue(path.startswith('/dev/pts/'))
        backend=self.USB();stop=threading.Event();fault=[]
        def peer():
            pending=b''
            while not stop.is_set():
                if not select.select([master],[],[],.02)[0]:continue
                try:data=os.read(master,4096)
                except OSError:continue
                pending+=data
                while b'\n'in pending:
                    command,pending=pending.split(b'\n',1)
                    if not command:continue
                    command=command.decode()
                    prefix='ap2 status ' if command=='AP2 STATUS'or command.startswith('AP2 ARM_HB')else ''
                    try:lines=backend.request(command,prefix)
                    except BaseException as error:fault.append(error);return
                    wire=('\n'.join(lines)+'\n').encode()
                    for i in range(0,len(wire),3):os.write(master,wire[i:i+3])
        worker=threading.Thread(target=peer,daemon=True);worker.start();original=fcntl.ioctl
        def ioctl(fd,request,*args):
            # PTYs have no physical DTR. Every other ioctl is the real kernel call.
            if request in [termios.TIOCMBIS,termios.TIOCMBIC]:return 0
            return original(fd,request,*args)
        try:
            with patch.object(hc.ap.fcntl,'ioctl',side_effect=ioctl),hc.txlib.Serial(path)as usb,tempfile.TemporaryDirectory()as d:
                with self.assertRaises(OSError):os.open(path,os.O_RDWR|os.O_NOCTTY)
                ready=hc.prepare(usb,self.host([]),'lcd','a',Path(d))
                self.assertEqual(ready['expected'],8)
            self.assertIsNone(usb.fd);self.assertFalse(fault)
        finally:
            stop.set();worker.join(2);os.close(master)


class Retention(unittest.TestCase):
    def backend(self, f, calls):
        class USB:
            def request(self, command, prefix):
                calls.append(command)
                if command.startswith('AP2 RELEASE'):return ['ap2 status state=0']
                if command=='AP2 STATUS':return ['ap2 status boot=1 run=a state=3']
                if command=='EV1 STATUS':return ['ev1 status b=1 on=0 sat=0 seq=0']
                if command=='AP2 TRIGGER':return ['ap2 trigger '+' '.join(k+'='+v for k,v in f[2]['trigger'].items())]
                if command=='TX1 STATUS':return ['tx1 status '+' '.join(k+'='+v for k,v in f[2]['tx_status'].items())]
                if command.startswith('TX1 READ'):
                    return ['tx1 row '+' '.join(k+'='+(str(v) if k=='rc' else format(v,'x'))for k,v in f[2]['tx_rows'][0].items())]
                raise AssertionError(command)
        def host(q):
            calls.append(q['op'])
            return dict(phase='idle' if q['op']=='release' else 'frozen')
        return USB(),host

    def collect(self,f,root,calls):
        seal(f);usb,host=self.backend(f,calls)
        (root/'headers.jsonl').write_bytes(b'{"scope_valid":true}\n');(root/'recvq.jsonl').write_bytes(b'')
        with patch.object(hc.ap,'read_pico',return_value=f[0]),patch.object(hc.ap,'read_host',return_value=f[1]),patch.object(hc.cy,'collect',return_value=f[2]['SDK_inner']):
            result=hc.collect(usb,host,f[3],root,f[4])
        return result,usb,host

    def test_save_before_release_and_no_automatic_rearm(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);f=fixture();calls=[];result,usb,host=self.collect(f,root,calls)
            self.assertTrue(result['complete']);self.assertNotIn('release',calls)
            saved=json.loads((root/'saved.json').read_text());hc.release(usb,host,f[3],saved,root)
            self.assertTrue((root/'released.json').exists());self.assertIn('release',calls)
            self.assertFalse(any('ARM'in c for c in calls))

    def test_tampered_saved_file_prevents_release(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);f=fixture();calls=[];_,usb,host=self.collect(f,root,calls)
            saved=json.loads((root/'saved.json').read_text());(root/'pico.json').write_text('{}')
            before=len(calls)
            with self.assertRaisesRegex(ValueError,'evidence changed'):hc.release(usb,host,f[3],saved,root)
            self.assertEqual(len(calls),before)

    def test_raw_headers_are_preserved_and_checked_before_release(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);f=fixture();calls=[];_,usb,host=self.collect(f,root,calls)
            saved=json.loads((root/'saved.json').read_text());self.assertIn('headers.jsonl',saved['hashes'])
            (root/'headers.jsonl').unlink();before=len(calls)
            with self.assertRaises(ValueError):hc.release(usb,host,f[3],saved,root)
            self.assertEqual(len(calls),before)

    def test_quality_failure_saved_without_discard_or_retry(self):
        with tempfile.TemporaryDirectory()as d:
            f=fixture();f[2]['SDK_inner']['status']['over']='1';calls=[]
            result,_,_=self.collect(f,Path(d),calls)
            self.assertFalse(result['complete']);self.assertIn('CY loss',result['errors'])
            self.assertTrue((Path(d)/'saved.json').exists());self.assertNotIn('release',calls)

    def test_partial_read_keeps_capture_and_does_not_release(self):
        with tempfile.TemporaryDirectory()as d:
            f=fixture();seal(f);calls=[];usb,host=self.backend(f,calls)
            with patch.object(hc.ap,'read_pico',side_effect=EOFError('partial page')):
                with self.assertRaises(EOFError):hc.collect(usb,host,f[3],Path(d),f[4])
            self.assertEqual(calls,['freeze','AP2 STATUS']);self.assertFalse((Path(d)/'saved.json').exists())

    def test_save_failure_retains_both_captures(self):
        with tempfile.TemporaryDirectory()as d:
            f=fixture();seal(f);calls=[];usb,host=self.backend(f,calls)
            with patch.object(hc.ap,'read_pico',return_value=f[0]),patch.object(hc.ap,'read_host',return_value=f[1]),patch.object(hc.cy,'collect',return_value=f[2]['SDK_inner']),patch.object(hc.ap,'persist',side_effect=OSError('disk full')):
                with self.assertRaises(OSError):hc.collect(usb,host,f[3],Path(d),f[4])
            self.assertNotIn('release',calls);self.assertFalse((Path(d)/'saved.json').exists())

    def test_header_binding_required(self):
        f=fixture();target=dict(binding=dict(boot='host',run='a',connection=431,session=9,target='lcd'))
        result=hc.header_evidence(f[4],[dict(scope_valid=True)],target,f[3])
        self.assertTrue(result['scope_valid']);target['binding']['connection']=432
        with self.assertRaises(ValueError):hc.header_evidence(f[4],[],target,f[3])


if __name__=='__main__':unittest.main()
