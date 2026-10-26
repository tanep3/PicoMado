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
        (self.root/'tracked').mkdir()
        (self.root/'device').mkdir()
        self.profile = self.root/'tracked'/(self.name+'.h')
        shutil.copy2(SOURCE/'firmware/profiles'/(self.name+'.h'), self.profile)
        self.config = self.root/'device/config.h'
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

    def test_device_gpio_redefinition_and_undef_rejected(self):
        base = '#define PM_LCD_BLK_CONNECTED 0\n'
        self.check(base)
        for change in ('#undef PM_LCD_PIN_CS\n',
                       '#undef PM_LCD_PIN_CS\n#define PM_LCD_PIN_CS 5\n',
                       '#define PM_BUTTON_UP_PIN 6\n'):
            with self.subTest(change=change):
                with self.assertRaises(ValueError):
                    self.check(base+change)

    def test_indirect_override_and_duplicate_legacy_rejected(self):
        base = ''
        (self.root/'override.h').write_text('#undef PM_LCD_PIN_CS\n#define PM_LCD_PIN_CS 17\n')
        with self.assertRaises(ValueError):
            self.check(base+'#include "override.h"\n')
        legacy = self.profile.read_text().replace('#pragma once\n', '')
        self.check(legacy)
        with self.assertRaises(ValueError):
            self.check(legacy+'#define PM_LCD_PIN_CS 17\n')
        with self.assertRaises(ValueError):
            self.check(legacy+'#undef PM_LCD_PIN_CS\n#define PM_LCD_PIN_CS 17\n')

    def test_conditional_indirect_include_rejected_independent_of_build_flags(self):
        (self.root/'override.h').write_text('#undef PM_LCD_PIN_CS\n#define PM_LCD_PIN_CS 5\n')
        base = ''
        for flag in ('PM_NORMAL_BUILD', 'PM_TRACE_ENABLE', 'PM_USB_BUTTON_SIM'):
            with self.subTest(flag=flag), self.assertRaisesRegex(ValueError, 'conditional'):
                self.check(base+f'#if {flag}\n#include "override.h"\n#endif\n')
        legacy = self.profile.read_text().replace('#pragma once\n', '')
        with self.assertRaisesRegex(ValueError, 'conditional'):
            self.check(legacy+'#if PM_NORMAL_BUILD\n#define PM_LCD_PIN_CS 5\n#endif\n')
        self.profile.write_text(self.profile.read_text()+'#if PM_NORMAL_BUILD\n#define PM_LCD_PIN_CS 5\n#endif\n')
        with self.assertRaisesRegex(ValueError, 'conditional'):
            self.check(base)

    def test_spliced_or_commented_extra_include_is_rejected(self):
        base = ''
        for extra in ('#\\\ninclude "override.h"\n', '#include/*comment*/"override.h"\n'):
            with self.subTest(extra=extra), self.assertRaisesRegex(ValueError, 'include'):
                self.check(base+extra)
        self.check(base+'#define PM_HOME_SSID "https://example.invalid/*literal*/"\n')

    def test_same_name_device_shadow_is_not_read(self):
        shadow = self.config.parent/(self.name+'.h')
        shadow.write_text('#if PM_NORMAL_BUILD\n#define PM_LCD_PIN_CS 5\n#endif\n')
        self.assertEqual(validator.validate(self.config_after(''), self.profile, self.cc), 'profile')
        raw = validator.compiler_output(self.cc, self.config, self.profile.parent, '-dM', self.profile)
        self.assertEqual(validator.effective(raw)['PM_LCD_PIN_CS'], '17')
        # The production form has the absolute tracked -include even when a
        # build condition is defined and a same-name device header exists.
        import subprocess
        result = subprocess.run([self.cc, '-E', '-dM', '-DPM_NORMAL_BUILD=1', '-x', 'c',
                                 '-I', str(self.config.parent), '-include', str(self.profile),
                                 '-include', str(self.config),
                                 '/dev/null'], capture_output=True, text=True, check=True)
        self.assertEqual(validator.effective(result.stdout)['PM_LCD_PIN_CS'], '17')

    def config_after(self, text):
        self.config.write_text(text)
        return self.config

    def test_only_documented_optional_hardware_can_vary(self):
        self.check('#define PM_LCD_BLK_CONNECTED 1\n#define PM_LCD_BLK_ACTIVE_HIGH 0\n')
        with self.assertRaises(ValueError):
            self.check('#define PM_EINK_PANEL_CONNECTED 1\n')
        with self.assertRaises(ValueError):
            self.check('#define PM_LCD_BLK_CONNECTED 2\n')


if __name__ == '__main__':
    unittest.main()
