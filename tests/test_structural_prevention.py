"""Contract-derived structural refusal tests; expected rules do not come from adapters."""
import importlib.util
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'firmware/tools'))
import observation_domain as domain
spec=importlib.util.spec_from_file_location('standard',ROOT/'development/standard/v1/check.py')
standard=importlib.util.module_from_spec(spec);spec.loader.exec_module(standard)
CONFIG=json.loads((ROOT/'contracts/development-boundaries.json').read_text())


class Structure(unittest.TestCase):
    def test_unverified_success_and_direct_state_are_not_constructible(self):
        for ctor,args in [(domain.CaptureReport,({'complete':True},)),(domain.VerifiedEvidence,({},))]:
            with self.assertRaises(TypeError):ctor(*args)
        capture=domain.CaptureWindow({'run':'a'})
        with self.assertRaises(AttributeError):capture.state='Confirmed'
        with self.assertRaises(TypeError):capture.confirm({'verified':True})
        capture.assess(domain.pending_assessment('missing evidence'))
        with self.assertRaises(TypeError):capture.confirm({'verified':True})
        with self.assertRaises(ValueError):capture.assess(domain.pending_assessment('again'))
        with self.assertRaises(TypeError):domain.run_outcome({'complete':True},errors=[],pico_idle=True,host_idle=True,owner_closed=True)

    def copy_governed(self,root):
        for item in CONFIG['python']+CONFIG['writes']:
            p=root/item['path'];p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(ROOT/item['path'],p)

    def test_static_forbidden_operations_are_rejected(self):
        cases=[('firmware/tools/heartbeat_capture.py','\nresult=dict(complete=True)\n','completion'),
               ('firmware/tools/heartbeat_capture.py','\nx["complete"]=True\n','completion'),
               ('firmware/tools/heartbeat_capture.py','\nx._CaptureWindow__state="Confirmed"\n','state'),
               ('firmware/tools/heartbeat_capture.py','\nx=domain._verified_evidence({})\n','restricted'),
               ('firmware/tools/observation_domain.py','\nimport os\n','dependency'),
               ('firmware/tools/observation_domain.py','\nimport heartbeat_capture\n','dependency'),
               ('firmware/src/ack_probe.c','\nvoid forbidden(void){probe.summary.state=PM_AP_RECORDING;}\n','state'),
               ('service/src/ack_probe.rs','\nfn forbidden(&mut self){self.phase=Phase::Recording;}\n','state')]
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);self.copy_governed(root)
            self.assertEqual(standard.check(root,CONFIG),[])
            for name,text,reason in cases:
                with self.subTest(name=name,text=text):
                    p=root/name;old=p.read_text();p.write_text(old+text)
                    errors=standard.check(root,CONFIG);self.assertTrue(any(reason in e for e in errors),errors)
                    p.write_text(old)

    def test_generated_drift_is_rejected(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d)
            for name in ['contracts/observation-domain.json','firmware/src/observation_values.h','service/src/observation_values.rs','firmware/tools/observation_values.py']:
                p=root/name;p.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(ROOT/name,p)
            p=root/'firmware/src/observation_values.h';p.write_text(p.read_text()+'\n/* unauthorized */\n')
            command=[sys.executable,str(ROOT/'development/standard/v1/generate.py'),'--root',str(root),'--check']
            result=subprocess.run(command,capture_output=True,text=True)
            self.assertNotEqual(result.returncode,0);self.assertIn('generated drift',result.stderr)

    def test_actual_C_semantic_values_reject_substitution(self):
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);source=root/'case.c'
            source.write_text('#include "observation_values.h"\nint main(void){obs_PicoSendAttemptOrdinal a=obs_PicoSendAttemptOrdinal_from_wire(46);obs_HostProcessedOrdinal b=a;(void)b;}\n')
            p=subprocess.run(['cc','-std=c11','-I'+str(ROOT/'firmware/src'),str(source),'-o',str(root/'case')],capture_output=True,text=True)
            self.assertNotEqual(p.returncode,0);self.assertIn('invalid initializer',p.stderr)

    def test_actual_Rust_values_and_private_state_reject_substitution(self):
        # Remove only serde rendering attributes, preserving the actual generated API.
        text=(ROOT/'service/src/observation_values.rs').read_text().replace(', serde::Serialize','')
        text=re.sub(r'#\[serde\([^\n]*\)\]\n','',text)
        cases=[('let a=values::PicoSendAttemptOrdinal::from_wire(46);let _:values::HostProcessedOrdinal=a;','mismatched types'),
               ('let mut w=values::HostWindow::new();w.state=values::HostState::Recording;','private'),
               ('let a=values::PicoMicros::from_wire(1);let b=values::HostProbeMicros::from_wire(1);let _=a==b;','mismatched types')]
        with tempfile.TemporaryDirectory()as d:
            root=Path(d);(root/'values.rs').write_text(text)
            positive=root/'positive.rs';positive.write_text('mod values;fn main(){let mut w=values::HostWindow::new();assert!(w.apply(values::HostEvent::Arm).is_ok());assert!(w.apply(values::HostEvent::Arm).is_err());}')
            p=subprocess.run(['rustc','--edition=2021',str(positive),'-o',str(root/'positive')],capture_output=True,text=True);self.assertEqual(p.returncode,0,p.stderr)
            subprocess.run([str(root/'positive')],check=True)
            for code,error in cases:
                source=root/'negative.rs';source.write_text('mod values;fn main(){'+code+'}')
                p=subprocess.run(['rustc','--edition=2021',str(source),'-o',str(root/'negative')],capture_output=True,text=True)
                self.assertNotEqual(p.returncode,0);self.assertIn(error,p.stderr)


if __name__=='__main__':unittest.main()
