"""Actual private-root CLI probes; parent approvals/check payloads are TEST ONLY."""
import copy
import json
from pathlib import Path
import subprocess
import tempfile
import unittest
from gate_fixture import GateFixture


def prepared(base):
    f=GateFixture(Path(base)/'source');f.check_fixture();f.implementation_verified();f.parent_reviewed()
    active=Path(base)/'active'
    subprocess.run(['git','clone','--quiet','--local',str(f.root),str(active)],check=True)
    result=f.cli('probe-route','--active-root',active,'--report',f.report,'--out',f.root/'.gate/probe')
    if result.returncode:raise AssertionError(result.stderr)
    probe=json.loads(result.stdout)['probe'];subject=f.gate.input_snapshot()['subject_digest']
    common=dict(schema=1,change='FIXTURE',subject_digest=subject,owner='parent',source_root=str(f.root),active_root=str(active),probe=probe)
    app=f.put('.gate/application.json',dict(common,stage='operational_application',applied=True))
    app_ref=dict(kind='application',path=str(app),SHA256=f.gate.sha(app))
    f.case['stages']['operational_application']=dict(state='verified',evidence=[app_ref])
    effect=f.put('.gate/effectiveness.json',dict(common,stage='effectiveness_confirmation',confirmed=True,application=app_ref))
    f.case['stages']['effectiveness_confirmation']=dict(state='verified',evidence=[dict(kind='effectiveness',path=str(effect),SHA256=f.gate.sha(effect))])
    f.case['status']='complete';f.save_case();f.commit()
    return f,active,app,effect


class ApplicationEvidence(unittest.TestCase):
    def test_original_counterexample_and_missing_verifier_fail_closed(self):
        with tempfile.TemporaryDirectory()as d:
            f=GateFixture(d);f.check_fixture();f.implementation_verified();f.parent_reviewed()
            subject=f.gate.input_snapshot()['subject_digest']
            for stage,kind,extra in [('operational_application','application',dict(applied=True,active_root='/nonexistent/route')),
                    ('effectiveness_confirmation','effectiveness',dict(confirmed=True,refusal_evidence='checked manually'))]:
                proof=f.put('.gate/'+kind+'.json',dict(change='FIXTURE',stage=stage,subject_digest=subject,owner='parent',**extra))
                f.case['stages'][stage]=dict(state='verified',evidence=[dict(kind=kind,path=str(proof),SHA256=f.gate.sha(proof))])
            f.case['status']='complete';f.save_case()
            with self.assertRaisesRegex(ValueError,'schema'):f.gate.workflow_admission('close')
            errors=f.gate.workflow_module().admit(f.case,'close',subject,f.gate.load_proof,lambda *a:True,lambda *a:True)
            self.assertTrue(any('verified route execution required'in e for e in errors))

    def test_real_distinct_root_probe_and_exact_application_link(self):
        with tempfile.TemporaryDirectory()as d:
            f,active,app,effect=prepared(d)
            f.gate.workflow_admission('close')
            proof=json.loads(effect.read_text());probe=f.gate.load_proof(proof['probe'])
            self.assertNotEqual(active,f.root)
            for op,code in [('current-check',0),('stale-check',1)]:
                result=f.gate.load_proof(probe['executions'][op]);self.assertEqual(result['exit'],code)
                self.assertEqual(result['cwd'],str(active));self.assertEqual(result['command'][3],str(active/'tools/project_gate.py'))
                self.assertEqual(result['stdout'],f.gate.load_proof(result['log'])['stdout'])
            self.assertEqual(f.cli('verify-check','--report',f.report).returncode,0)
            self.assertNotEqual(f.cli('probe-route','--active-root',active,'--report',f.report,'--out',f.root/'.gate/probe').returncode,0)

    def test_current_root_source_and_saved_evidence_tampering_refused(self):
        with tempfile.TemporaryDirectory()as d:
            f,active,app,effect=prepared(d);f.gate.workflow_admission('close')
            def refused():
                with self.assertRaises((ValueError,OSError,KeyError)):f.gate.workflow_admission('close')
            for path in [active/'AGENTS.md',active/'tools/project_gate.py',f.root/'development/standard/v1/standard.md']:
                with self.subTest(changed_entry=str(path)):
                    original=path.read_bytes();path.write_bytes(original+b'\nchanged\n');refused();path.write_bytes(original)
            moved=active.with_name('removed');active.rename(moved);refused();moved.rename(active)
            application=json.loads(app.read_text());probe_path=Path(application['probe']['path']);probe=json.loads(probe_path.read_text())
            positive=f.gate.load_proof(probe['executions']['current-check'])
            for ref in [application['probe'],probe['executions']['current-check'],positive['log'],positive['input']]:
                path=Path(ref['path']);original=path.read_bytes()
                with self.subTest(changed_saved_file=str(path)):
                    path.write_bytes(original+b' ');refused();path.write_bytes(original)
                    path.unlink();refused();path.write_bytes(original)
            f.gate.workflow_admission('close')

    def test_rehashed_contradictory_executions_and_linkage_refused(self):
        with tempfile.TemporaryDirectory()as d:
            f,active,app,effect=prepared(d);baseline=copy.deepcopy(f.case)
            # Recompute the entire declaration hash chain: rejection must be semantic.
            originals={p:p.read_bytes()for p in (f.root/'.gate').rglob('*.json')}
            def refresh():
                application=json.loads(app.read_text());probe_path=Path(application['probe']['path']);probe=json.loads(probe_path.read_text())
                for ref in probe['executions'].values():
                    path=Path(ref['path']);receipt=json.loads(path.read_text())
                    for key in ['log','input']:receipt[key]['SHA256']=f.gate.sha(receipt[key]['path'])
                    f.gate.write(path,receipt);ref['SHA256']=f.gate.sha(path)
                f.gate.write(probe_path,probe);application['probe']['SHA256']=f.gate.sha(probe_path);f.gate.write(app,application)
                app_ref=f.case['stages']['operational_application']['evidence'][0];app_ref['SHA256']=f.gate.sha(app)
                proof=json.loads(effect.read_text());proof['probe']=application['probe'];proof['application']=copy.deepcopy(app_ref);f.gate.write(effect,proof)
                f.case['stages']['effectiveness_confirmation']['evidence'][0]['SHA256']=f.gate.sha(effect);f.save_case()
            def alter_receipt(op,key,value):
                proof=json.loads(app.read_text());probe=f.gate.load_proof(proof['probe']);path=Path(probe['executions'][op]['path']);receipt=json.loads(path.read_text());receipt[key]=value
                log={k:receipt[k]for k in ['operation','command','cwd','start_ns','end_ns','exit','stdout','stderr']};f.gate.write(receipt['log']['path'],log);f.gate.write(path,receipt)
            def alter_probe(key,value):
                path=Path(json.loads(app.read_text())['probe']['path']);probe=json.loads(path.read_text());probe['scope'][key]=value;f.gate.write(path,probe)
            def alter_input():
                probe=json.loads(Path(json.loads(app.read_text())['probe']['path']).read_text())
                receipt=f.gate.load_proof(probe['executions']['stale-check']);path=Path(receipt['input']['path']);body=json.loads(path.read_text());body['complete']=False;f.gate.write(path,body)
            alterations=[
                lambda:alter_receipt('current-check','command',['echo','PASS']),
                lambda:alter_receipt('current-check','cwd',str(f.root)),
                lambda:alter_receipt('current-check','exit',1),
                lambda:alter_receipt('stale-check','stderr','Traceback: unrelated failure\n'),
                lambda:alter_receipt('stale-check','exit',0),
                lambda:alter_receipt('stale-check','end_ns',0),
                lambda:alter_probe('source_root',str(active)),
                lambda:alter_probe('active_commit','0'*40),
                alter_input,
            ]
            for i,alter in enumerate(alterations):
                with self.subTest(contradiction=i):
                    alter();refresh()
                    with self.assertRaises(ValueError):f.gate.workflow_admission('close')
                for path,body in originals.items():path.write_bytes(body)
                f.case=copy.deepcopy(baseline);f.save_case()
            proof=json.loads(effect.read_text());proof['application']['path']=str(f.root/'.gate/undeclared-application.json')
            f.gate.write(effect,proof);f.case['stages']['effectiveness_confirmation']['evidence'][0]['SHA256']=f.gate.sha(effect);f.save_case()
            with self.assertRaisesRegex(ValueError,'not the declared application'):f.gate.workflow_admission('close')


if __name__=='__main__':unittest.main()
