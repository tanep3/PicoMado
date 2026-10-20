#!/usr/bin/env python3
import copy
import importlib.util
import json
import hashlib
from pathlib import Path
import sys
import tempfile
import unittest
import zlib

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
import event_stream as ev
import event_causality as causal


def packet(sequence, component, phase, *, operation=0, parent=0, frame=0, us=None, a=0, b=0, dropped=0):
    body=ev.BODY.pack(component,1,phase,1,sequence,sequence*1000 if us is None else us,operation,parent,1,frame,dropped,a,b)
    return b'ev1 '+body.hex().encode()+b' '+('%08x'%zlib.crc32(body)).encode()+b'\n'


class Tests(unittest.TestCase):
    def evidence(self, *, draw=True, end_us=5000000):
        cause=dict(version=1,boot=1,operation=2)
        raw=[packet(1,8,1,a=2,b=0x9f),packet(2,1,1,operation=2),
             packet(3,3,3,parent=2),packet(4,4,1,parent=2,frame=8),packet(5,4,4,frame=8)]
        if draw:
            raw.append(packet(6,5,4,frame=8,us=600000))
        end=len(raw)+1
        raw.append(packet(end,8,4,us=end_us,a=2,b=0x9f))
        raw.insert(1,b'ev1 s a 1 1 0 0 0 1 0\n')
        raw.append(('ev1 s b 1 %x 0 0 0 0 0\n'%end).encode())
        status=dict(boot=10,sequence=0,dropped=0,enabled=False,saturated=False)
        rows=[dict(kind='begin',catalog=2,coverage=0xef,status=status,us=1000000000000)]
        for seq,(component,phase,frame) in enumerate([('Api','Dispatched',0),('State','Committed',0),('Frame','Pending',8)],1):
            rows.append(dict(kind='event',event=dict(boot=10,sequence=seq,us=1000000000000+seq,component=component,phase=phase,frame=frame,cause=cause)))
        rows.append(dict(kind='end',catalog=2,coverage=0xef,status=dict(status,sequence=3),us=1000000001000,complete=True))
        return raw,rows

    def parse(self, raw, rows):
        with tempfile.TemporaryDirectory() as t:
            root=Path(t);(root/'device').write_bytes(b''.join(raw));(root/'server').write_text(''.join(json.dumps(r)+'\n' for r in rows))
            (root/'server.summary.json').write_text(json.dumps(dict(complete=True,storage_durable=True,source_sha256=hashlib.sha256((root/'server').read_bytes()).hexdigest())))
            return causal.device_source(root/'device'),causal.server_source(root/'server')

    def test_same_source_elapsed_and_exact_cause_frame(self):
        d,s=self.parse(*self.evidence())
        result=causal.analyze(d,s)['results'][0]
        self.assertEqual(result['status'],'slow');self.assertEqual(result['elapsed_device_us'],598000)
        # Host clock is deliberately 10^12us away and is never subtracted.
        raw,rows=self.evidence();rows[3]['event']['cause']['operation']=99
        self.assertEqual(causal.analyze(*self.parse(raw,rows))['results'][0]['status'],'unknown')

    def test_complete_missing_after_deadline_and_finite_end_before_deadline(self):
        d,s=self.parse(*self.evidence(draw=False))
        result=causal.analyze(d,s)['results'][0]
        self.assertEqual(result['status'],'missing_after_deadline');self.assertEqual(result['boundary'],'display_complete')
        d,s=self.parse(*self.evidence(draw=False,end_us=100000))
        self.assertEqual(causal.analyze(d,s)['results'][0]['reasons'],['capture_ended_before_deadline'])

    def test_crc_gap_drop_old_catalog_missing_metadata_and_host_failure_are_unknown(self):
        for fault in ['crc','gap','drop','catalog','cause','host_footer']:
            raw,rows=self.evidence(draw=False)
            if fault=='crc':raw[3]=raw[3][:-5]+b'ffff\n'
            if fault=='gap':del raw[3]
            if fault=='drop':raw[3]=packet(3,3,3,parent=2,dropped=1)
            if fault=='catalog':raw[0]=packet(1,8,1,a=1,b=0x9f)
            if fault=='cause':
                for row in rows:
                    if 'event' in row:row['event']['cause']=None
            if fault=='host_footer':rows.pop()
            with self.subTest(fault=fault):
                result=causal.analyze(*self.parse(raw,rows))['results'][0]
                self.assertIn(result['status'],['unknown','incomplete'])

    def test_later_input_can_supersede_and_is_not_a_missing_frame_failure(self):
        raw,rows=self.evidence(draw=False);raw.insert(-2,packet(6,1,1,operation=6))
        raw[-2]=packet(7,8,4,us=5000000,a=2,b=0x9f);raw[-1]=b'ev1 s b 1 7 0 0 0 0 0\n'
        result=causal.analyze(*self.parse(raw,rows))['results'][0]
        self.assertEqual(result['status'],'unknown');self.assertEqual(result['reasons'],['later_input_may_supersede'])

    def test_explicit_draw_failure_keeps_frame_and_phase(self):
        raw,rows=self.evidence();raw[-3]=packet(6,5,6,frame=8,us=600000)
        result=causal.analyze(*self.parse(raw,rows))['results'][0]
        self.assertEqual(result['status'],'explicit_failure');self.assertEqual(result['frame'],8)


if __name__=='__main__':unittest.main()
