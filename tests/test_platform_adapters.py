"""Contract tests for the per-platform development command adapters."""
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

from tools.kotavel_cli.platforms import get_platform


class PlatformAdapterTests(unittest.TestCase):
    def test_platform_registry_selects_a_complete_adapter(self):
        cases = {
            'Windows': ('windows', 'Scripts', 'python.exe', 'conan.exe'),
            'Linux': ('linux', 'bin', 'python', 'conan'),
            'Darwin': ('macos', 'bin', 'python', 'conan'),
        }
        for name, expected in cases.items():
            with self.subTest(platform=name):
                adapter = get_platform(name)
                self.assertEqual((adapter.profile_name, adapter.scripts_directory,
                                  adapter.python_executable, adapter.conan_executable), expected)

    def test_unsupported_platform_fails_at_adapter_selection(self):
        with self.assertRaisesRegex(RuntimeError, 'Sistema no soportado: Plan9'):
            get_platform('Plan9')

    def test_direct_script_imports_keep_relative_package_dependencies_resolvable(self):
        repository = Path(__file__).parents[1]
        cli = repository / 'tools' / 'kotavel_cli'
        with tempfile.TemporaryDirectory() as directory:
            code = r'''
import json, sys, types
from pathlib import Path
sys.path.insert(0, sys.argv[1])
import project_paths
sys.modules['posix_environment'] = types.SimpleNamespace(
    prepare=lambda compiler, env: {'compiler_bin': '', 'compiler': compiler})
from platforms import get_platform
root = Path(sys.argv[2])
adapter = get_platform('Darwin')
prepared = adapter.prepare(
    {'settings': {'compiler': 'apple-clang'}}, {}, True,
    tools=root, scripts=root, run_command=lambda *args: None,
    write_if_changed=lambda path, content: Path(path).write_text(content))
assert prepared['compiler'] == 'apple-clang'
assert json.loads((root / 'platform.json').read_text()) == prepared
assert project_paths.conan_home(root, 'Darwin').name == 'conan'
'''
            result = subprocess.run(
                [sys.executable, '-c', code, str(cli), directory],
                cwd=repository, env=os.environ.copy(), capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)


class MacOSAdapterTests(unittest.TestCase):
    def setUp(self):
        self.adapter = get_platform('Darwin')

    def test_profile_and_conan_home_follow_macos_defaults(self):
        project = Path('/tmp/kotavel-consumer')
        defaults = Path(__file__).parents[1] / 'tools' / 'kotavel_cli' / 'templates' / 'application' / 'profiles'
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


if __name__ == '__main__':
    unittest.main()
