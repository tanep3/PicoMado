"""The established build entry carries only the seven known firmware options.
Native CMake fixtures exercise routing/receipts; real ARM artifacts are separate.
"""
import importlib.util
import json
import os
import tempfile
import unittest
from unittest.mock import patch
from gate_fixture import GateFixture,SOURCE

spec=importlib.util.spec_from_file_location('firmware_lineage',SOURCE/'tools/build_lineage.py')
lineage=importlib.util.module_from_spec(spec);spec.loader.exec_module(lineage)
FLAGS=('PM_ACK_PATH_PROBE','PM_USB_BUTTON_SIM','PM_TX_LINK_PROBE','PM_CYW43_INNER_PROBE',
       'PM_CYW43_SPI_REAL_SIZE','PM_EVENT_STREAM','PM_STREAM_DIAGNOSTICS')

class FirmwareBuildFlags(unittest.TestCase):
    def test_defaults_are_off_and_unrelated_environment_is_not_a_cmake_option(self):
        with patch.dict(os.environ,{'PM_UNSUPPORTED':'ON'},clear=True):
            self.assertEqual(lineage.firmware_flags(),dict.fromkeys(FLAGS,'OFF'))

    def test_only_exact_on_off_values_are_accepted(self):
        with patch.dict(os.environ,dict.fromkeys(FLAGS,'ON'),clear=True):
            self.assertEqual(lineage.firmware_flags(),dict.fromkeys(FLAGS,'ON'))
        for bad in ['1','0','on','OFF; true','ON -DPICO_BOARD=other','']:
            with self.subTest(value=bad),patch.dict(os.environ,{FLAGS[0]:bad},clear=True):
                with self.assertRaisesRegex(ValueError,'ON or OFF'):lineage.firmware_flags()

    def test_existing_root_build_configure_and_receipt_retain_all_requested_options(self):
        with tempfile.TemporaryDirectory()as d:
            f=GateFixture(d);f.native_firmware_container();f.check_fixture()
            with patch.dict(os.environ,dict.fromkeys(FLAGS,'ON')):f.build('firmware')
            receipt=json.loads(f.build_receipt.read_text())
            for name in FLAGS:
                self.assertIn('-D'+name+'=ON',receipt['configure_command'])
                self.assertEqual(receipt['build']['flags'][name],'ON')
            self.assertEqual(receipt['exit'],0)

    def test_configure_cannot_silently_change_a_requested_option(self):
        with tempfile.TemporaryDirectory()as d:
            f=GateFixture(d);f.native_firmware_container()
            cmake=f.root/'firmware/CMakeLists.txt'
            cmake.write_text(cmake.read_text()+'\nset(PM_EVENT_STREAM OFF CACHE BOOL "fixture conflict" FORCE)\n')
            f.commit();f.check_fixture()
            with patch.dict(os.environ,dict.fromkeys(FLAGS,'ON')):
                result=f.cli('build','--kind','firmware','--device','fixture','--build-dir',f.root/'.gate/conflict','--report',f.report)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('configured firmware flags differ',result.stderr)
            self.assertFalse((f.root/'.gate/conflict/build-receipt.json').exists())
            self.assertFalse((f.root/'.gate/conflict-build.log').exists())

if __name__=='__main__':unittest.main()
