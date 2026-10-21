"""Actual CLI refusal and completed-build lineage in private test repositories."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from gate_fixture import GateFixture,SOURCE


class Routes(unittest.TestCase):
    def test_actual_server_build_and_lineage_refusals(self):
        with tempfile.TemporaryDirectory()as d:
            f=GateFixture(d);f.native_server();f.check_fixture();f.implementation_verified();f.parent_reviewed()
            f.build('server');f.record('server');p=f.preflight('server');self.assertEqual(p.returncode,0,p.stderr)
            old=f.image.read_bytes();f.image.write_bytes(old+b'bad')
            self.assertNotEqual(f.preflight('server').returncode,0);f.image.write_bytes(old)
            receipt=json.loads(f.build_receipt.read_text());original=f.build_receipt.read_bytes()
            receipt['build']['features']=['unbuilt_feature'];f.build_receipt.write_text(json.dumps(receipt))
            self.assertNotEqual(f.preflight('server').returncode,0);f.build_receipt.write_bytes(original)
            f.approval.unlink();self.assertNotEqual(f.preflight('server').returncode,0)
            p=f.cli('record','--report',f.report,'--artifact',f.image,'--kind','server','--out',Path(d)/'.gate/arbitrary.json')
            self.assertNotEqual(p.returncode,0);self.assertIn('--build-receipt',p.stderr)

    def test_actual_firmware_build_sdk_flags_and_offline_flash(self):
        with tempfile.TemporaryDirectory()as d:
            f=GateFixture(d);f.native_firmware_container();f.check_fixture();f.implementation_verified();f.parent_reviewed()
            f.build('firmware');f.record('firmware');p=f.preflight('firmware');self.assertEqual(p.returncode,0,p.stderr)
            sdk=f.root/'sdk/fixture.h';old=sdk.read_bytes();sdk.write_bytes(old+b'// dirty SDK\n')
            p=f.preflight('firmware');self.assertNotEqual(p.returncode,0);sdk.write_bytes(old)
            cache=f.root/'.gate/build/CMakeCache.txt';old=cache.read_bytes();cache.write_text(cache.read_text().replace('PM_USE_SAMPLE:UNINITIALIZED=OFF','PM_USE_SAMPLE:UNINITIALIZED=ON'))
            self.assertNotEqual(f.preflight('firmware').returncode,0);cache.write_bytes(old)
            config=f.root/'firmware/devices/fixture/config.h';old=config.read_bytes();config.write_bytes(old+b'// changed config\n')
            self.assertNotEqual(f.preflight('firmware').returncode,0);config.write_bytes(old)
            image=f.image;build=image.parent;(build/'manifest.txt').write_text('OFF fixture 0000000000000001 '+f.gate.sha(image)+'\n')
            fake=f.root/'.gate/fake';fake.mkdir();called=f.root/'.gate/PICOTOOL_MUST_NOT_RUN'
            (fake/'picotool').write_text('#!/bin/sh\ntouch "'+str(called)+'"\nexit 99\n');(fake/'picotool').chmod(0o755)
            env=dict(os.environ,PM_BUILD_DIR=str(build),PATH=str(fake)+os.pathsep+os.environ['PATH'])
            cmd=['bash',str(f.root/'firmware/tools/flash.sh'),'--device','fixture','--verify-only']
            p=subprocess.run(cmd,capture_output=True,text=True,env=env);self.assertEqual(p.returncode,0,p.stderr);self.assertFalse(called.exists())
            f.approval.unlink();p=subprocess.run(cmd,capture_output=True,text=True,env=env);self.assertNotEqual(p.returncode,0);self.assertFalse(called.exists())

    def test_previous_partial_records_refused_by_actual_CLI(self):
        sys.path.insert(0,str(SOURCE/'firmware/tests'));sys.path.insert(0,str(SOURCE/'firmware/tools'))
        from heartbeat_capture_test import fixture,seal
        f=fixture();seal(f)
        with tempfile.TemporaryDirectory()as d:
            root=Path(d)
            for name,value in zip(['pico','host','extra','ready','headers'],f):(root/(name+'.json')).write_text(json.dumps(value))
            p=subprocess.run([sys.executable,str(SOURCE/'firmware/tools/heartbeat_capture.py'),str(root)],capture_output=True,text=True)
            self.assertNotEqual(p.returncode,0);self.assertFalse(json.loads(p.stdout)['complete'])


if __name__=='__main__':unittest.main()
