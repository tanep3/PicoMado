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
import subprocess
import struct

sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
import event_stream as ev
import event_causality as causal


def packet(sequence, component, phase, *, operation=0, parent=0, frame=0, us=None, a=0, b=0, dropped=0):
    body=ev.BODY.pack(component,1,phase,1,sequence,sequence*1000 if us is None else us,operation,parent,1,frame,dropped,a,b)
    return b'ev1 '+body.hex().encode()+b' '+('%08x'%zlib.crc32(body)).encode()+b'\n'


class Tests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.build=tempfile.TemporaryDirectory();cls.exe=Path(cls.build.name)/'causal'
        firmware=Path(__file__).resolve().parents[1]
        args=['cc','-std=gnu11','-Wall','-Wextra','-Werror','-DPM_USE_SAMPLE_CONFIG=1','-DPM_NORMAL_BUILD=1','-DPM_EVENT_STREAM=1','-DPM_TRACE_ENABLE=1','-DPM_EXPECTED_BOARD_ID="0000000000000001"','-DPM_BUILD_DEVICE="sample-a"','-Isrc','-Itests/app_stubs','-Idevices/sample-a','tests/causal_fixture.c','src/core.c','src/video.c','src/trace.c','src/rx_stats.c','src/usb_control.c','-o',str(cls.exe)]
        subprocess.run(args,cwd=firmware,check=True,capture_output=True)
    @classmethod
    def tearDownClass(cls):cls.build.cleanup()
    def actual_c(self, metadata, outside=False):
        token=dict(device_id='sample-a',incarnation=1,session_epoch=1,display_epoch=2,screen_owner_epoch=42,frame_id=2)
        data=dict(token=token,mode='normal',width=160,height=80,format='rgb565be',byte_len=25600,crc32=zlib.crc32(bytes(25600)))
        if outside:data.update(version=1,boot=4660,operation=2)
        body=json.dumps(dict(type='frame_start',data=data),separators=(',',':'))
        if metadata is not None:body=body[:-1]+',"observation":'+metadata+'}'
        payload=body.encode();packet=(struct.pack('>I',len(payload)+2)+bytes([1,12])+payload).hex()
        commands='USB EV1 ON abc\nBUTTON 3\nPACKET '+packet+'\nBUTTON 3\nUSB EV1 OFF abd\n'
        process=subprocess.run([self.exe],input=commands,text=True,capture_output=True,timeout=5)
        self.assertEqual(process.returncode,0,process.stderr)
        with tempfile.TemporaryDirectory() as t:
            path=Path(t)/'device';path.write_text(''.join(s+'\n' for s in process.stdout.splitlines() if s.startswith('ev1 ')))
            return causal.device_source(path)

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

    def test_catalog3_coverage_and_pre_read_limit_are_explicit(self):
        raw,rows=self.evidence()
        for row in (rows[0],rows[-1]):row.update(catalog=3,coverage=0xef)
        device,server=self.parse(raw,rows);self.assertTrue(server[1])
        limits=causal.analyze(device,server)['boundary_limits']
        self.assertEqual(limits['pre_first_observation'],'unknown')
        self.assertIn('not kernel arrival',limits['raw_read'])
        for row in (rows[0],rows[-1]):row['coverage']=0xeb
        self.assertFalse(self.parse(raw,rows)[1][1])
        # Historical catalog2 remains readable without gaining Network coverage.
        for row in (rows[0],rows[-1]):row['catalog']=2
        self.assertTrue(self.parse(raw,rows)[1][1])

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

    def test_old_and_post_capture_inputs_never_enter_current_interval(self):
        raw,rows=self.evidence();facts=[ev.decode(line) for line in raw if not line.startswith(b'ev1 s ')]
        n=facts[-1]['sequence'];us=facts[-1]['us']
        raw=[line for line in raw if not line.startswith(b'ev1 s ')]+[packet(n+1,8,1,us=us+1000,a=2,b=0x9f),('ev1 s a 1 %x 0 0 0 1 0\n'%(n+1)).encode(),packet(n+2,8,4,us=us+5000000,a=2,b=0x9f),('ev1 s b 1 %x 0 0 0 0 0\n'%(n+2)).encode(),packet(n+3,1,1,operation=n+3,us=us+6000000)]
        d,s=self.parse(raw,rows);self.assertTrue(d[1],d);self.assertEqual(causal.analyze(d,s)['results'],[])

    def test_frame_without_resolved_cause_is_unknown_even_after_deadline(self):
        raw,rows=self.evidence(draw=False);raw[4]=packet(4,4,1,parent=0,frame=8)
        result=causal.analyze(*self.parse(raw,rows),require_frame=True)['results'][0]
        self.assertEqual(result['status'],'unknown');self.assertEqual(result['reasons'],['frame_cause_unresolved'])

    def test_actual_c_supersession_is_cancellation_not_failure(self):
        d=self.actual_c('{"version":1,"boot":4660,"operation":2}')
        self.assertTrue(d[1],d);self.assertTrue(any(f['component']==4 and f['phase']==7 for f in d[0]))
        cause=dict(version=1,boot=4660,operation=2)
        s=([dict(component=c,phase=p,cause=cause,frame=f) for c,p,f in [('Api','Dispatched',0),('State','Committed',0),('Frame','Pending',2)]],True,[])
        self.assertEqual(causal.analyze(d,s)['results'][0]['status'],'cancelled')

    def test_actual_c_duplicate_or_outside_metadata_preserves_domain_without_correlation(self):
        for metadata,outside in [('{"version":1,"version":99,"boot":4660,"operation":2}',False),('{"version":1,"boot":4660,"\\u0062oot":4660,"operation":2}',False),(None,True)]:
            with self.subTest(metadata=metadata,outside=outside):
                d=self.actual_c(metadata,outside)
                self.assertTrue(d[1],d)
                start=next(f for f in d[0] if f['component']==4 and f['phase']==1)
                self.assertEqual(start['parent'],0)
                self.assertFalse(any(f['component']==2 and f['event']==12 and f['phase'] in [5,6] for f in d[0]),'domain FrameStart must still be accepted')
                cause=dict(version=1,boot=4660,operation=2)
                s=([dict(component=c,phase=p,cause=cause,frame=f) for c,p,f in [('Api','Dispatched',0),('State','Committed',0),('Frame','Pending',2)]],True,[])
                self.assertEqual(causal.analyze(d,s)['results'][0]['status'],'unknown')


if __name__=='__main__':unittest.main()
