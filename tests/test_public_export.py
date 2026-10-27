import importlib.util
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('public_export_under_test', ROOT/'tools/export_public.py')
export = importlib.util.module_from_spec(spec)
spec.loader.exec_module(export)


class ExportPolicyTests(unittest.TestCase):
    def test_required_assets_and_docs_are_selected(self):
        for name in ['LICENSE', 'README.md', 'service/Cargo.lock', 'service/src/word.rs', 'service/assets/word-card.png', 'service/assets/NOTO-COPYRIGHT', 'firmware/diagnostics/backlight-polarity/main.c', 'firmware/profiles/pico_w_lcd_160x80_4.h', 'firmware/devices/sample-a/config.sample.h', 'docs/guides/plugin-author.en.md', 'docs/media/picomado-devices.jpg', 'tools/public_gate.py']:
            self.assertTrue(export.selected(name), name)

    def test_internal_records_and_device_settings_are_excluded(self):
        for name in ['.git/config', 'docs/changes/private.json', 'docs/checkpoints/private.json', 'firmware/devices/mine/config.h', 'firmware/devices/mine/board-id.txt', 'firmware/devices/mine/profile.txt', 'firmware/devices/sample-a/config.h', 'firmware/tools/flash.sh', 'service/history/private.md']:
            self.assertFalse(export.selected(name), name)

    def test_selected_inputs_are_scrubbed_and_history_is_absent(self):
        with tempfile.TemporaryDirectory() as tmp, patch('sys.argv', ['export_public.py', '--allow-dirty', '--out', str(Path(tmp)/'candidate')]):
            export.main()
            out = Path(tmp)/'candidate'
            self.assertFalse((out/'.git').exists())
            self.assertFalse((out/'docs/changes').exists())
            self.assertFalse((out/'firmware/devices/mine').exists())
            fixture = (ROOT/'firmware/tests/heartbeat_owner_runner_test.py').read_text()
            import re
            identifier = re.search(r"expected_flash_id=['\"]([A-F0-9]{16})['\"]", fixture).group(1).encode()
            for path in out.rglob('*'):
                if path.is_file():
                    data = path.read_bytes()
                    self.assertNotIn(identifier, data, str(path.relative_to(out)))
                    self.assertNotIn(b'/home/', data, str(path.relative_to(out)))


if __name__ == '__main__': unittest.main()
