import json
from pathlib import Path
import re
import sys
import tempfile
import time
import unittest
import zlib
import os

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
import event_stream as ev
import event_causality as causal
from capture_boundary_cases import CASES,B,E,D
from event_collector_test import Device

def encoded(fact):
    body=ev.BODY.pack(*(fact[k] for k in ev.FIELDS))
    return b'ev1 '+body.hex().encode()+b' '+('%08x'%zlib.crc32(body)).encode()+b'\n'

def status(sequence,on,boot=1,cid=1):
    return dict(cid=cid,boot=boot,sequence=sequence,dropped=0,first_lost=0,last_lost=0,on=int(on),saturated=0)

def status_line(value):
    return ('ev1 s %x %x %x 0 0 0 %u 0\n'%(value['cid'],value['boot'],value['sequence'],value['on'])).encode()

class StateTests(unittest.TestCase):
    def test_table_fact_ack_and_byte_matrix(self):
        checked=0
        with tempfile.TemporaryDirectory(prefix='pm-state-') as temp:
            path=Path(temp)/'raw'
            for case in CASES:
                for ack_at in range(len(case['facts'])+1):
                    for chunk_size in (1,65536):
                        with self.subTest(case=case['id'],ack_at=ack_at,chunk=chunk_size):
                            start=status(case.get('start',1),True)
                            end=None if case['end'] is None else status(case['end'],case.get('end_on',False),case.get('end_boot',1),2)
                            lines=[encoded(f) for f in case['facts']];lines.insert(ack_at,status_line(start))
                            if end:lines.append(status_line(end))
                            raw=b''.join(lines)+(b'partial' if case.get('partial') else b'')
                            window=ev.Window(1);framing=ev.Lines();host=0
                            for at in range(0,len(raw),chunk_size):
                                for line in framing.feed(raw[at:at+chunk_size]):
                                    host+=1;f=ev.decode(line)
                                    if f:window.accept(f,host)
                                    elif ev.parse_status(line)['on']:window.begin(ev.parse_status(line))
                            result=window.finish(end,framing.partial())
                            self.assertEqual(result['causal_complete'],case['complete'],result)
                            if case.get('error'):self.assertIn(case['error'],result['reasons'])
                            path.write_bytes(raw)
                            self.assertEqual(causal.device_source(path)[1],case['complete'],causal.device_source(path))
                            if case['complete']:
                                self.assertEqual(window.boundary_state,'ended')
                                self.assertEqual(window.begin_fact['sequence'],start['sequence'])
                            checked+=1
        print('PASS transition-table fact/ACK/byte matrix',checked,'cases')

    def test_terminal_states_and_anchor_are_not_reset(self):
        start=status(1,True);window=ev.Window(1);window.begin(start);window.accept(B(1),10)
        original=window.begin_fact;window.begin(status(3,True));window.accept(E(2))
        self.assertEqual(window.start,start);self.assertIs(window.begin_fact,original)
        self.assertIn('unexpected_start',window.reasons)
        self.assertFalse(window.finish(status(2,False))['causal_complete'])
        aborted=ev.Window(1);aborted.begin(start);aborted.accept(B(1));aborted.finish(None)
        aborted.accept(B(3));aborted.begin(status(3,True))
        self.assertEqual(aborted.boundary_state,'aborted')
        self.assertIn('event_after_abort',aborted.reasons);self.assertIn('unexpected_start',aborted.reasons)
        # A raw protocol with two ON replies also fails canonical anchoring.
        with tempfile.TemporaryDirectory(prefix='pm-state-') as temp:
            p=Path(temp)/'raw';p.write_bytes(status_line(start)+encoded(B(1))+status_line(status(3,True))+encoded(E(2))+status_line(status(2,False)))
            self.assertFalse(causal.device_source(p)[1])
        # Table T01 overflow remains bounded and cannot restore quality.
        overflow=ev.Window(1)
        for _ in range(33):overflow.accept(D(0))
        self.assertEqual(len(overflow.prelude),32);self.assertIn('prelude_overflow',overflow.reasons)
        overflow.begin(start);overflow.accept(B(1));overflow.accept(E(2))
        self.assertFalse(overflow.finish(status(2,False))['causal_complete'])

    def test_every_documented_transition_has_a_case(self):
        doc=Path(__file__).resolve().parents[2]/'docs/capture-boundary-state-contract.md'
        documented=set(re.findall(r'\| (T\d\d) \|',doc.read_text()))
        covered={row for case in CASES for row in case['rows']}|{'T15','T16'}
        self.assertEqual(documented,covered)

    def test_fresh_pty_from_transition_table(self):
        count=0
        selected=['normal','old-prefix','distinct-begin','lower-begin','catalog-changed-begin','foreign-begin']
        for case in [c for c in CASES if c['id'] in selected]:
            prefix=case['facts'][:-1];ending=encoded(case['facts'][-1])
            for ack_at in range(len(prefix)+1):
                for split in (False,True):
                    with self.subTest(case=case['id'],ack_at=ack_at,split=split):
                        class TableDevice(Device):
                            activated=False
                            def wire(self,data):
                                if split:
                                    for byte in data:Device.send(self,bytes([byte]))
                                else:
                                    while data:
                                        try:n=os.write(self.master,data);data=data[n:]
                                        except BlockingIOError:time.sleep(.001)
                            def send(self,data):
                                reply=ev.parse_status(data) if data.startswith(b'ev1 s ') else None
                                if reply and reply['on'] and not self.activated:
                                    self.activated=True
                                    self.wire(b''.join(encoded(f) for f in prefix[:ack_at])+data+b''.join(encoded(f) for f in prefix[ack_at:]))
                                elif reply and not reply['on'] and self.activated:
                                    self.seq=case['end'];words=data.split();words[4]=format(self.seq,'x').encode()
                                    self.wire(ending+b' '.join(words)+b'\n')
                                else:self.wire(data)
                        device=TableDevice(events=[],initial_sequence=case.get('start',1))
                        try:
                            with tempfile.TemporaryDirectory(prefix='pm-state-') as temp:
                                p=Path(temp);announced=[]
                                result=ev.collect('new-PTY-only',p/'raw',p/'owner',.12,port_factory=device.open,
                                    journal_factory=lambda p:ev.Journal(p,sync_seconds=.03),causal_ready=announced.append)
                                self.assertEqual(result['causal_complete'],case['complete'],result)
                                self.assertEqual(causal.device_source(p/'raw')[1],case['complete'])
                                self.assertEqual(len(announced),1 if case['complete'] else 0)
                                if case.get('error'):self.assertIn(case['error'],result['segments'][0]['reasons'])
                        finally:device.close()
                        count+=1
        print('PASS transition-table fresh PTY',count,'cases')

    def test_fresh_pty_abort_and_late_begin(self):
        for disconnected in (True,False):
            with self.subTest(disconnected=disconnected):
                class BoundaryDevice(Device):
                    activated=False
                    def send(self,data):
                        reply=ev.parse_status(data) if data.startswith(b'ev1 s ') else None
                        if reply and reply['on'] and not self.activated:
                            self.activated=True;Device.send(self,encoded(B(1))+data)
                        elif reply and not reply['on'] and self.activated:
                            self.seq=3;words=data.split();words[4]=b'3'
                            Device.send(self,encoded(E(3))+b' '.join(words)+b'\n')
                        else:Device.send(self,data)
                device=BoundaryDevice(events=[],initial_sequence=1,disconnect=disconnected)
                try:
                    with tempfile.TemporaryDirectory(prefix='pm-state-') as temp:
                        p=Path(temp);announced=[]
                        def ready(status):
                            announced.append(status)
                            if not disconnected:
                                device.send(encoded(B(2)))
                                owner.stop_requested=True
                        owner=ev.Collector('new-PTY-only',p/'raw',p/'owner',.3,port_factory=device.open,
                            journal_factory=lambda p:ev.Journal(p,sync_seconds=.01),causal_ready=ready)
                        result=owner.run()
                        self.assertFalse(result['causal_complete'],result)
                        self.assertFalse(causal.device_source(p/'raw')[1])
                        if disconnected:
                            self.assertEqual(result['segments'][0]['boundary_state'],'aborted')
                            self.assertIn('disconnected',result['segments'][0]['reasons'])
                        else:
                            self.assertEqual(len(announced),1) # valid before the later boundary existed
                            self.assertIn('unexpected_begin',result['segments'][0]['reasons'])
                            self.assertEqual(owner.window.begin_fact['sequence'],1)
                            self.assertFalse(owner.status()['causal_ready'])
                finally:device.close()

if __name__=='__main__':unittest.main()
