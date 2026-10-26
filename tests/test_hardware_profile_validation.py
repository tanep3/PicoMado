"""Compiler-visible profile checks, including legacy and indirect overrides."""
import importlib.util
from pathlib import Path
import shutil
import tempfile
import unittest

SOURCE = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('hardware_profile_validation', SOURCE/'firmware/tools/validate_hardware_profile.py')
validator = importlib.util.module_from_spec(spec)
spec.loader.exec_module(validator)


class HardwareProfileValidation(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.name = 'pico_w_lcd_160x80_4'
        self.profile = self.root/(self.name+'.h')
        shutil.copy2(SOURCE/'firmware/profiles'/(self.name+'.h'), self.profile)
        self.config = self.root/'config.h'
        self.cc = shutil.which('cc')
        self.assertIsNotNone(self.cc)

    def check(self, text):
        self.config.write_text(text)
        validator.validate(self.config, self.profile, self.cc)

    def test_verified_samples(self):
        for device in ('sample-a', 'sample-b', 'E-ink_0', 'E-ink_1'):
            with self.subTest(device=device):
                name = (SOURCE/'firmware/devices'/device/'profile.txt').read_text().strip()
                validator.validate(SOURCE/'firmware/devices'/device/'config.sample.h',
                                   SOURCE/'firmware/profiles'/(name+'.h'), self.cc)

    def test_include_then_gpio_redefinition_and_undef_rejected(self):
        base = f'#include "{self.name}.h"\n#define PM_LCD_BLK_CONNECTED 0\n'
        self.check(base)
        for change in ('#undef PM_LCD_PIN_CS\n',
                       '#undef PM_LCD_PIN_CS\n#define PM_LCD_PIN_CS 5\n',
                       '#define PM_BUTTON_UP_PIN 6\n'):
            with self.subTest(change=change):
                with self.assertRaises(ValueError):
                    self.check(base+change)

    def test_indirect_override_and_duplicate_legacy_rejected(self):
        base = f'#include "{self.name}.h"\n'
        (self.root/'override.h').write_text('#undef PM_LCD_PIN_CS\n#define PM_LCD_PIN_CS 17\n')
        with self.assertRaises(ValueError):
            self.check(base+'#include "override.h"\n')
        legacy = self.profile.read_text().replace('#pragma once\n', '')
        self.check(legacy)
        with self.assertRaises(ValueError):
            self.check(legacy+'#define PM_LCD_PIN_CS 17\n')
        with self.assertRaises(ValueError):
            self.check(legacy+'#undef PM_LCD_PIN_CS\n#define PM_LCD_PIN_CS 17\n')

    def test_only_documented_optional_hardware_can_vary(self):
        self.check(f'#include "{self.name}.h"\n#define PM_LCD_BLK_CONNECTED 1\n#define PM_LCD_BLK_ACTIVE_HIGH 0\n')
        with self.assertRaises(ValueError):
            self.check(f'#include "{self.name}.h"\n#define PM_EINK_PANEL_CONNECTED 1\n')
        with self.assertRaises(ValueError):
            self.check(f'#include "{self.name}.h"\n#define PM_LCD_BLK_CONNECTED 2\n')


if __name__ == '__main__':
    unittest.main()
