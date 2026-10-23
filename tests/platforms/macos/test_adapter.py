"""Tests for macOS-specific command adapter behavior."""
import os
import sys
import tempfile
import unittest
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[3]))
from tools.kotavel_cli.platforms import get_platform


@unittest.skipUnless(sys.platform == 'darwin', 'macOS adapter tests run on the macOS CI job')
class MacOSAdapterTests(unittest.TestCase):
    def setUp(self):
        self.adapter = get_platform('Darwin')

    def test_profile_and_conan_home_follow_macos_defaults(self):
        project = Path('/tmp/kotavel-consumer')
        defaults = Path(__file__).parents[3] / 'tools' / 'kotavel_cli' / 'templates' / 'application' / 'profiles'
        path, config = self.adapter.read_profile(project, defaults)
        self.assertEqual(path.name, 'macos')
        self.assertEqual(config['settings']['os'], 'Macos')
        self.assertEqual(self.adapter.conan_home(project, Path('/tmp/user'), None),
                         Path('/tmp/user/.kotavel/conan'))
        self.assertEqual(self.adapter.conan_home(project, Path('/tmp/user'), '/tmp/shared-conan'),
                         Path('/tmp/shared-conan').resolve())

    def test_app_bundle_resolution_and_cli_executable(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            build = root / 'build' / 'Release' / 'app'
            bundle = build / 'Kotavel Laboratory.app'
            executable = bundle / 'Contents' / 'MacOS' / 'Kotavel Laboratory'
            executable.parent.mkdir(parents=True)
            executable.touch()
            self.assertEqual(self.adapter.executable_path(root, 'Release', 'Kotavel Laboratory'), bundle)
            self.assertEqual(self.adapter.executable_for_run(bundle), executable)

    def test_macos_packaging_is_supported_and_prefers_conan_macdeployqt(self):
        self.assertTrue(self.adapter.package_supported())
        self.assertIsNone(self.adapter.require_packager())
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            tool = cache / 'p' / 'b' / 'qt123' / 'p' / 'bin' / 'macdeployqt'
            tool.parent.mkdir(parents=True)
            tool.touch()
            self.assertEqual(self.adapter.macdeployqt(Path('/tmp/project'), cache), tool)

    @unittest.skipIf(os.name == 'nt', 'macOS PATH semantics are validated on macOS runners')
    def test_macos_activation_is_repeatable_and_does_not_duplicate_path(self):
        with tempfile.TemporaryDirectory() as directory:
            tools = Path(directory)
            scripts = tools / 'python' / 'bin'
            env = {'PATH': f'{scripts}:/usr/bin:{scripts}', 'CONAN_HOME': '/tmp/conan'}
            self.adapter.prepend_tools_path(env, scripts)
            self.adapter.prepend_tools_path(env, scripts)
            self.assertEqual(env['PATH'].split(':'), [str(scripts), '/usr/bin'])
            self.adapter.write_activation(
                {'activation_kind': 'posix'}, env, tools, Path('/tmp/conan'),
                self.adapter.write_if_changed)
            activation = tools / 'activate.sh'
            self.assertIn('export CONAN_HOME=/tmp/conan', activation.read_text())
            self.assertIn('export PATH=', activation.read_text())
