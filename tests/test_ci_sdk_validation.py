"""CI SDK preflight fixtures never fetch, edit a shared SDK, or touch hardware."""
import importlib.util
from pathlib import Path
import subprocess
import tempfile
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('ci_sdk', ROOT / 'tools/check_ci_sdk.py')
ci_sdk = importlib.util.module_from_spec(spec)
spec.loader.exec_module(ci_sdk)
REVISION = 'a' * 40
MODULES = f" {'b' * 40} lib/lwip (pinned)\n {'c' * 40} lib/cyw43-driver (pinned)\n"


class CiSdkValidationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.sdk = Path(self.temp.name) / 'sdk'
        for name in ci_sdk.REQUIRED_FILES:
            path = self.sdk / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text('fixture\n')
        self.results = {
            ('rev-parse', 'HEAD'): REVISION + '\n',
            ('submodule', 'status', '--recursive'): MODULES,
            ('status', '--porcelain', '--untracked-files=no'): '',
        }
        patcher = patch.object(ci_sdk, 'git_output', side_effect=self.git_output)
        self.git = patcher.start()
        self.addCleanup(patcher.stop)

    def git_output(self, sdk, *args):
        self.assertEqual(sdk, self.sdk)
        return self.results[args]

    def test_complete_pinned_sdk_passes_without_changing_files(self):
        before = {name: (self.sdk / name).read_bytes() for name in ci_sdk.REQUIRED_FILES}
        self.assertEqual(ci_sdk.validate(self.sdk, REVISION), REVISION)
        self.assertEqual(before, {name: (self.sdk / name).read_bytes() for name in before})
        self.assertEqual(self.git.call_count, 3)

    def test_absent_sdk_is_rejected_before_git_or_host_tests(self):
        with self.assertRaisesRegex(ValueError, 'Pico SDK directory is missing'):
            ci_sdk.validate(self.sdk / 'absent', REVISION)
        self.git.assert_not_called()

    def test_each_required_source_is_checked(self):
        for name in ci_sdk.REQUIRED_FILES:
            with self.subTest(name=name):
                path = self.sdk / name
                path.unlink()
                with self.assertRaisesRegex(ValueError, 'test dependencies are missing') as error:
                    ci_sdk.validate(self.sdk, REVISION)
                self.assertIn(name, str(error.exception))
                path.write_text('fixture\n')

    def test_wrong_revision_is_rejected(self):
        self.results[('rev-parse', 'HEAD')] = 'd' * 40 + '\n'
        with self.assertRaisesRegex(ValueError, 'revision mismatch'):
            ci_sdk.validate(self.sdk, REVISION)

    def test_revision_must_be_immutable_commit(self):
        for revision in ('main', '2.3.1', 'abc1234', ''):
            with self.subTest(revision=revision), self.assertRaisesRegex(ValueError, 'full 40-character'):
                ci_sdk.validate(self.sdk, revision)
        self.git.assert_not_called()

    def test_uninitialized_mismatched_conflicted_or_absent_submodules_fail(self):
        for modules in ('', '-' + MODULES[1:], '+' + MODULES[1:], 'U' + MODULES[1:]):
            with self.subTest(modules=modules):
                self.results[('submodule', 'status', '--recursive')] = modules
                with self.assertRaisesRegex(ValueError, 'submodules are missing or differ'):
                    ci_sdk.validate(self.sdk, REVISION)

    def test_modified_sdk_or_submodule_fails(self):
        for status in (' M src/rp2_common/pico_flash/flash.c\n', ' m lib/cyw43-driver\n'):
            with self.subTest(status=status):
                self.results[('status', '--porcelain', '--untracked-files=no')] = status
                with self.assertRaisesRegex(ValueError, 'tracked changes'):
                    ci_sdk.validate(self.sdk, REVISION)


class GitFailureTests(unittest.TestCase):
    def test_git_failure_and_timeout_are_reported_as_validation_failures(self):
        for error in (subprocess.CalledProcessError(128, ['git']),
                      subprocess.TimeoutExpired(['git'], 30), FileNotFoundError('git')):
            with self.subTest(error=error), patch.object(ci_sdk.subprocess, 'run', side_effect=error):
                with self.assertRaisesRegex(ValueError, 'Cannot inspect Pico SDK'):
                    ci_sdk.git_output(Path('.'), 'rev-parse', 'HEAD')


if __name__ == '__main__':
    unittest.main()
