import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'tools'))
spec = importlib.util.spec_from_file_location('public_gate_under_test', ROOT/'tools/public_gate.py')
gate = importlib.util.module_from_spec(spec)
spec.loader.exec_module(gate)


class PublicGateTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)/'source'; self.root.mkdir()
        (self.root/'source.txt').write_text('source')
        self.patch = patch.object(gate, 'ROOT', self.root); self.patch.start()
        self.out = Path(self.temp.name)/'check'; self.out.mkdir()
        commands = [[sys.executable, 'tools/generate_observation.py', '--check'], [sys.executable, 'development/standard/v1/check.py', '--root', str(self.root), '--config', str(self.root/'contracts/development-boundaries.json')], [sys.executable, '-m', 'unittest', 'discover', '-s', 'tests', '-p', 'test_hardware_profile_validation.py'], [sys.executable, '-m', 'unittest', 'discover', '-s', 'tests', '-p', 'test_public_gate.py'], ['bash', 'firmware/tests/run_host_tests.sh'], ['bash', 'firmware/tests/run_usb_tx_tests.sh'], ['cargo', 'test', '--offline', '--locked', '--all-features', '--manifest-path', 'service/Cargo.toml']]
        checks = []
        for name, command in zip(['generated.log', 'boundaries.log', 'hardware-profile.log', 'public-gate.log', 'firmware-host.log', 'firmware-usb.log', 'rust.log'], commands):
            path = self.out/name; path.write_text('passed\n')
            checks.append({'command': command, 'log': str(path), 'sha256': gate.sha(path), 'exit': 0})
        self.report = self.out/'check.json'
        gate.write(self.report, {'schema': 1, 'type': 'picomado-public-check', 'source': gate.source_digest(), 'checks': checks})

    def tearDown(self):
        self.patch.stop(); self.temp.cleanup()

    def test_check_rejects_changed_source_log_and_command(self):
        gate.verify_check(self.report)
        (self.root/'source.txt').write_text('changed')
        with self.assertRaisesRegex(ValueError, 'source changed'): gate.verify_check(self.report)
        (self.root/'source.txt').write_text('source')
        (self.out/'rust.log').write_text('changed')
        with self.assertRaisesRegex(ValueError, 'logs or commands'): gate.verify_check(self.report)
        (self.out/'rust.log').write_text('passed\n')
        report = json.loads(self.report.read_text()); report['checks'][0]['command'] = ['true']
        self.report.write_text(json.dumps(report))
        with self.assertRaisesRegex(ValueError, 'logs or commands'): gate.verify_check(self.report)

    def test_device_selection_is_local_and_source_changes_are_bound(self):
        before = gate.source_digest()
        device = self.root/'firmware/devices/new-device'; device.mkdir(parents=True)
        (device/'profile.txt').write_text('profile\n'); (device/'config.h').write_text('secret\n')
        self.assertEqual(before, gate.source_digest())
        (self.root/'source.txt').write_text('different')
        self.assertNotEqual(before, gate.source_digest())
        (self.root/'source.txt').write_text('source')
        nested = self.root/'service/src/target'; nested.mkdir(parents=True)
        (nested/'plugin.rs').write_text('plugin source')
        self.assertNotEqual(before, gate.source_digest())

    def test_artifact_rejects_bad_elf_and_bad_uf2(self):
        image = Path(self.temp.name)/'image'; image.write_bytes(b'not an executable')
        with self.assertRaises(ValueError): gate.artifact(image, 'server')
        image.write_bytes(bytes(512))
        with self.assertRaises(ValueError): gate.artifact(image, 'firmware')

    def test_preflight_binds_artifact_configuration_and_check(self):
        build_dir = Path(self.temp.name)/'build'; (build_dir/'release').mkdir(parents=True)
        image = build_dir/'release/picomado-server'; image.write_bytes(b'\x7fELFfixture')
        log = Path(self.temp.name)/'build-build.log'; log.write_text('build succeeded')
        b = {'build_dir': str(build_dir), 'builder': {'path': '/usr/bin/cargo'}, 'features': [], 'no_default': False}
        command = ['/usr/bin/cargo', 'build', '--offline', '--locked', '--release', '--manifest-path', str(self.root/'service/Cargo.toml'), '--target-dir', str(build_dir), '--bin', 'picomado-server', '--verbose']
        receipt = Path(self.temp.name)/'receipt.json'
        gate.write(receipt, {'schema': 1, 'type': 'picomado-public-build', 'kind': 'server', 'source': gate.source_digest(), 'check': str(self.report), 'check_sha256': gate.sha(self.report), 'build': b, 'command': {'exit': 0, 'command': command, 'log': str(log), 'sha256': gate.sha(log)}, 'artifact': str(image), 'artifact_sha256': gate.sha(image)})
        with patch.object(gate.lineage, 'snapshot', return_value=b), patch.object(gate.lineage, 'fingerprint'):
            gate.preflight(receipt)
            image.write_bytes(b'\x7fELFchanged')
            with self.assertRaisesRegex(ValueError, 'artifact changed'): gate.preflight(receipt)
        image.write_bytes(b'\x7fELFfixture')
        with patch.object(gate.lineage, 'snapshot', return_value=b | {'features': ['changed']}):
            with self.assertRaisesRegex(ValueError, 'configuration'): gate.preflight(receipt)
        (self.out/'rust.log').write_text('tampered')
        with self.assertRaisesRegex(ValueError, 'logs or commands'): gate.preflight(receipt)

    def test_verify_source_detects_changed_file_and_traversal(self):
        manifest = self.root/'PUBLIC-SOURCE-MANIFEST.json'
        manifest.write_text(json.dumps({'schema': 1, 'files': {'source.txt': gate.sha(self.root/'source.txt')}}))
        gate.verify_source()
        (self.root/'unexpected').write_text('extra source')
        with self.assertRaisesRegex(ValueError, 'unexpected file'): gate.verify_source()
        (self.root/'unexpected').unlink()
        (self.root/'source.txt').write_text('tampered')
        with self.assertRaisesRegex(ValueError, 'missing or changed'): gate.verify_source()
        manifest.write_text(json.dumps({'schema': 1, 'files': {'../outside': 'x'}}))
        with self.assertRaisesRegex(ValueError, 'manifest path'): gate.verify_source()

    def test_firmware_target_follows_selected_profile(self):
        profiles = self.root/'firmware/profiles'; profiles.mkdir(parents=True)
        device = self.root/'firmware/devices/test-device'; device.mkdir(parents=True)
        (device/'profile.txt').write_text('chosen\n')
        (profiles/'chosen.board').write_text('pico_w\n')
        header = profiles/'chosen.h'
        header.write_text('#define PM_DISPLAY_TYPE PM_DISPLAY_LCD\n')
        self.assertEqual(gate.firmware_target('test-device'), 'picomado_lcd')
        header.write_text('#define PM_DISPLAY_TYPE PM_DISPLAY_EINK\n')
        self.assertEqual(gate.firmware_target('test-device'), 'picomado_eink')
        (profiles/'chosen.board').write_text('pico2_w\n')
        with self.assertRaisesRegex(ValueError, 'MCU board'): gate.firmware_target('test-device')

    def test_firmware_preflight_binds_configure_and_artifact(self):
        import struct
        profiles = self.root/'firmware/profiles'; profiles.mkdir(parents=True)
        device = self.root/'firmware/devices/test-device'; device.mkdir(parents=True)
        (device/'profile.txt').write_text('chosen\n'); (profiles/'chosen.board').write_text('pico_w\n')
        (profiles/'chosen.h').write_text('#define PM_DISPLAY_TYPE PM_DISPLAY_LCD\n')
        (device/'config.h').write_text('dummy configuration\n'); (device/'board-id.txt').write_text('0123456789ABCDEF\n')
        out = self.root/'firmware/build/test-device'; out.mkdir(parents=True)
        image = out/'test-device.uf2'; block = bytearray(512)
        struct.pack_into('<8I', block, 0, 0x0a324655, 0x9e5d5157, 0x2000, 0x10000000, 4, 0, 1, 0xe48bff56)
        struct.pack_into('<I', block, 508, 0x0ab16f30); image.write_bytes(block)
        configlog = out.parent/'test-device-configure.log'; configlog.write_text('configured')
        buildlog = out.parent/'test-device-build.log'; buildlog.write_text('built')
        b = {'mode': 'normal', 'device': 'test-device', 'board': 'pico_w', 'build_dir': str(out), 'builder': {'path': '/usr/bin/cmake'}, 'flags': {flag:'OFF' for flag in gate.lineage.FIRMWARE_FLAGS}, 'configuration': {str(device/'config.h'):gate.sha(device/'config.h')}}
        configure = ['/usr/bin/cmake', '-S', str(self.root/'firmware'), '-B', str(out), '-DPM_DEVICE=test-device', '-DPM_USE_SAMPLE=OFF', '-DPM_PREPARE_UNBOUND=OFF', '-DPM_PREPARE_UNBOUND_PANEL=OFF', '-DPICO_BOARD=pico_w', '-DCMAKE_BUILD_TYPE=Release', '-DCMAKE_EXPORT_COMPILE_COMMANDS=ON', '-DPICO_SDK_FETCH_FROM_GIT=OFF', '-DPICO_SDK_SUBMODULE_AUTO_UPDATE=OFF'] + ['-D'+flag+'=OFF' for flag in gate.lineage.FIRMWARE_FLAGS]
        command = ['/usr/bin/cmake', '--build', str(out), '--target', 'picomado_lcd', '-j4', '--verbose']
        receipt = Path(self.temp.name)/'firmware.json'; source = gate.source_digest()
        gate.write(receipt, {'schema':1, 'type':'picomado-public-build', 'kind':'firmware', 'source':source, 'check':str(self.report), 'check_sha256':gate.sha(self.report), 'build':b, 'configure':{'exit':0, 'command':configure, 'log':str(configlog), 'sha256':gate.sha(configlog)}, 'command':{'exit':0, 'command':command, 'log':str(buildlog), 'sha256':gate.sha(buildlog)}, 'artifact':str(image), 'artifact_sha256':gate.sha(image)})
        with patch.object(gate, 'verify_check', return_value={'source':source}), patch.object(gate.lineage, 'snapshot', return_value=b):
            gate.preflight(receipt)
            configlog.write_text('changed')
            with self.assertRaisesRegex(ValueError, 'configure command or log changed'): gate.preflight(receipt)

    def test_preflight_rejects_sample_firmware_before_io(self):
        receipt = Path(self.temp.name)/'sample.json'
        gate.write(receipt, {'schema': 1, 'type': 'picomado-public-build', 'kind': 'firmware', 'source': gate.source_digest(), 'check': str(self.report), 'check_sha256': gate.sha(self.report), 'build': {'mode': 'sample', 'build_dir': str(self.root/'firmware/build/example'), 'device': 'example'}})
        with patch.object(gate.lineage, 'snapshot') as snapshot:
            with self.assertRaisesRegex(ValueError, 'mode or directory'): gate.preflight(receipt)
            snapshot.assert_not_called()

    def test_flash_refuses_wrong_board_before_write(self):
        from types import SimpleNamespace
        device = self.root/'firmware/devices/test-device'; device.mkdir(parents=True)
        (device/'board-id.txt').write_text('0123456789ABCDEF\n')
        receipt = {'kind': 'firmware', 'build': {'device': 'test-device'}, 'artifact': '/tmp/dummy.uf2'}
        answers = ['pico_board: pico_w\n', 'Device Information\nflash id: 0x0000000000000001\n']
        with patch.object(gate, 'preflight', return_value=receipt), patch.object(gate.subprocess, 'check_output', side_effect=answers), patch.object(gate.subprocess, 'run') as write:
            with self.assertRaisesRegex(ValueError, 'board ID mismatch'):
                gate.flash(SimpleNamespace(receipt='receipt', device='test-device', check_only=False))
            write.assert_not_called()

    def test_rejects_symlink_in_source(self):
        (self.root/'link').symlink_to(self.root/'source.txt')
        with self.assertRaisesRegex(ValueError, 'symlink'): gate.source_digest()


if __name__ == '__main__': unittest.main()
