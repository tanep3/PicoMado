import copy
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from gate_fixture import GateFixture,SOURCE
sys.path.insert(0,str(SOURCE/'firmware/tools'))
import observation_domain as domain
from observation_serialization import report_value
spec=importlib.util.spec_from_file_location('standard',SOURCE/'development/standard/v1/check.py');standard=importlib.util.module_from_spec(spec);spec.loader.exec_module(standard)


class ReviewCorrections(unittest.TestCase):
    def test_exact_report_union_counterexample_and_unverified_run(self):
        report=domain.incomplete('raw not saved')
        with self.assertRaises(TypeError):report|={'complete':True}
        for mutate in [lambda:report.update(complete=True),lambda:report.setdefault('complete',True),lambda:dict.__setitem__(report,'complete',True)]:
            with self.assertRaises((TypeError,AttributeError)):mutate()
        with self.assertRaises((TypeError,AttributeError)):report['errors'].append('changed')
        result=domain.run_outcome(report,errors=[],pico_idle=True,host_idle=True,owner_closed=True)
        self.assertFalse(result.complete);self.assertFalse(result.evidence_confirmed)
        with self.assertRaises(TypeError):domain.run_outcome(report_value(report),errors=[],pico_idle=True,host_idle=True,owner_closed=True)
        forged=report_value(report)|{'complete':True}
        with self.assertRaises(TypeError):domain.run_outcome(forged,errors=[],pico_idle=True,host_idle=True,owner_closed=True)

    def test_exact_static_union_counterexample(self):
        config=json.loads((SOURCE/'contracts/development-boundaries.json').read_text())
        with tempfile.TemporaryDirectory()as d:
            root=Path(d)
            for item in config['python']+config['writes']:
                target=root/item['path'];target.parent.mkdir(parents=True,exist_ok=True);target.write_bytes((SOURCE/item['path']).read_bytes())
            p=root/'firmware/tools/heartbeat_capture.py';p.write_text(p.read_text()+'\ndef bad(report,incoming):\n    report |= incoming\n    return report\n')
            self.assertTrue(any('|='in x for x in standard.check(root,config)))

    def test_contract_digest_and_stage_evidence_are_bound_without_cycle(self):
        with tempfile.TemporaryDirectory()as d:
            f=GateFixture(d);f.check_fixture();before=f.gate.input_snapshot()
            p=f.root/'contracts/design.md';old=p.read_bytes();p.write_bytes(old+b'new invariant\n')
            changed=f.gate.input_snapshot();self.assertNotEqual(before['subject_digest'],changed['subject_digest']);self.assertNotEqual(before['source_digest'],changed['source_digest'])
            self.assertNotEqual(f.cli('verify-check','--report',f.report).returncode,0);p.write_bytes(old)
            f.case['stages']['implementation']=dict(state='verified',evidence=[]);f.save_case()
            changed=f.gate.input_snapshot();self.assertEqual(before['subject_digest'],changed['subject_digest']);self.assertNotEqual(before['source_digest'],changed['source_digest'])
            p=f.cli('verify-check','--report',f.report);self.assertNotEqual(p.returncode,0);self.assertIn('needs evidence',p.stderr)
            f.implementation_verified();self.assertEqual(f.gate.input_snapshot()['subject_digest'],before['subject_digest'])
            p=f.cli('verify-check','--report',f.report);self.assertEqual(p.returncode,0,p.stderr)
            report=json.loads(f.report.read_text());report['stage']='independent_verification';f.report.write_text(json.dumps(report))
            self.assertNotEqual(f.cli('verify-check','--report',f.report).returncode,0)


if __name__=='__main__':unittest.main()
