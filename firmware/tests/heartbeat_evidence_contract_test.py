import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'tools'))
import heartbeat_capture as hc
from heartbeat_capture_test import fixture,seal


def interval(f):
    headers=f[4]
    headers['controller']=dict(clock='CLOCK_MONOTONIC',start_request_before_ns=100000000000,
       start_confirm_after_ns=100200000000,DTR_low_before_ns=101000000000,
       DTR_low_after_ns=101001000000,required_end_ns=137100000000,
       stop_request_before_ns=137100000000,finished_after_ns=137200000000,
       wait_budget_ns=16000000000,errors=[])
    headers['metadata_events']=[e for e in headers['metadata_events']if e.get('event')!='started']
    headers['metadata_events'].insert(0,dict(event='started',clock=dict(mono_before_ns=100100000000,mono_after_ns=100100001000)))
    worker=next(e for e in headers['metadata_events']if e['event']=='worker_finished')
    worker.update(reason='stop_command',capture_started=True,capture_seconds=37.05,
                  max_queue_sample_gap_ns=100000000,packet_stats=dict(kernel_drops=0,kernel_packets=1))
    return f


def saved(root):
    f=interval(fixture());seal(f)
    p,h,e,r,headers=f
    for name,value in [('pico',p),('host',h),('extra',e),('headers',headers),('quality',hc.validate(*f))]:
        hc.ap.persist(root/(name+'.json'),value)
    (root/'headers.jsonl').write_bytes(b'{"scope_valid":true}\n');(root/'recvq.jsonl').write_bytes(b'')
    return f,hc.confirm_saved(root,r)


class Tests(unittest.TestCase):
    def test_short_or_unconfirmed_observer_never_complete(self):
        for fault in ['short','time_limit','missing_start','missing_bracket','late_start','early_stop','wrong_clock','duration']:
            with self.subTest(fault=fault):
                f=interval(fixture());seal(f);headers=f[4];c=headers['controller'];worker=next(e for e in headers['metadata_events']if e['event']=='worker_finished')
                if fault=='short':worker['capture_seconds']=1.
                if fault=='time_limit':worker['reason']='time_limit'
                if fault=='missing_start':headers['metadata_events']=headers['metadata_events'][1:]
                if fault=='missing_bracket':headers.pop('controller')
                if fault=='late_start':c['start_confirm_after_ns']=102000000000
                if fault=='early_stop':c['stop_request_before_ns']=137000000000
                if fault=='wrong_clock':c['clock']='host_probe_relative'
                if fault=='duration':worker['capture_seconds']=100.
                self.assertFalse(hc.validate(*f)['complete'])
    def test_cli_rejects_five_JSON_without_manifest_and_raw(self):
        f=interval(fixture());seal(f)
        with tempfile.TemporaryDirectory()as d:
            root=Path(d)
            for name,value in zip(['pico','host','extra','ready','headers'],f):hc.ap.persist(root/(name+'.json'),value)
            p=subprocess.run([sys.executable,str(Path(hc.__file__)),str(root)],capture_output=True,text=True)
            self.assertNotEqual(p.returncode,0);self.assertFalse(json.loads(p.stdout)['complete'])
    def test_shared_manifest_valid_then_each_required_file_missing(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);_,result=saved(root);self.assertTrue(result['complete'],result)
            for name in ['saved.json','headers.jsonl','recvq.jsonl','ready.json','quality.json']:
                path=root/name;data=path.read_bytes();path.unlink()
                result=hc.validate_directory(root);self.assertFalse(result['evidence_confirmed']);self.assertFalse(result['complete'])
                path.write_bytes(data)
    def test_tamper_hash_set_identity_and_size(self):
        for fault in ['raw','hash_set','identity','digest','large']:
            with self.subTest(fault=fault),tempfile.TemporaryDirectory()as d:
                root=Path(d);saved(root)
                if fault=='raw':(root/'headers.jsonl').write_bytes(b'changed')
                elif fault=='large':(root/'recvq.jsonl').write_bytes(b'x'*524289)
                else:
                    manifest=json.loads((root/'saved.json').read_text())
                    if fault=='digest':manifest['pico_digest']+=1
                    elif fault=='hash_set':manifest['hashes'].pop('headers.jsonl')
                    else:manifest['ready']['run']='bad'
                    (root/'saved.json').write_text(json.dumps(manifest))
                self.assertFalse(hc.validate_directory(root)['evidence_confirmed'])


if __name__=='__main__':unittest.main()
