"""Contract tests for the per-platform development command adapters."""
import json
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, call, patch

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


class WindowsAdapterTests(unittest.TestCase):
    def setUp(self):
        self.adapter = get_platform('Windows')

    @patch('tools.kotavel_cli.platforms.windows.subprocess.run')
    @patch('tools.kotavel_cli.platforms.windows.WindowsPlatform._post_close_to_process_windows', return_value=True)
    @patch('tools.kotavel_cli.platforms.windows.WindowsPlatform._send_console_break', return_value=False)
    def test_process_stop_falls_back_to_closing_windows(self, send_break, post_close, run):
        run.return_value = SimpleNamespace(returncode=3)
        self.adapter.request_process_stop(123)
        send_break.assert_called_once_with(123)
        post_close.assert_called_once_with(123)

    @patch('tools.kotavel_cli.platforms.windows.subprocess.run')
    @patch('tools.kotavel_cli.platforms.windows.WindowsPlatform._post_close_to_process_windows', return_value=False)
    @patch('tools.kotavel_cli.platforms.windows.WindowsPlatform._send_console_break', return_value=False)
    def test_process_stop_reports_failure_when_no_window_can_be_closed(self, send_break, post_close, run):
        run.return_value = SimpleNamespace(returncode=3)
        with self.assertRaisesRegex(RuntimeError, 'cierre ordenado'):
            self.adapter.request_process_stop(123)
        send_break.assert_called_once_with(123)
        post_close.assert_called_once_with(123)

    @patch('tools.kotavel_cli.platforms.windows.subprocess.run')
    @patch('tools.kotavel_cli.platforms.windows.WindowsPlatform._send_console_break', return_value=True)
    def test_process_stop_sends_break_to_child_group_from_its_terminal(self, send_break, run):
        self.adapter.request_process_stop(123)
        send_break.assert_called_once_with(123)
        run.assert_not_called()

    def test_window_close_targets_only_windows_owned_by_the_process(self):
        owners = {0x1001: 123, 0x1002: 456}
        user32 = SimpleNamespace()

        def enumerate_windows(callback, _):
            callback(0x1001, 0)
            callback(0x1002, 0)
            return True

        def get_owner(window, owner):
            owner._obj.value = owners[window]
            return 1

        user32.EnumWindows = Mock(side_effect=enumerate_windows)
        user32.GetWindowThreadProcessId = Mock(side_effect=get_owner)
        user32.PostMessageW = Mock(return_value=1)
        with patch('tools.kotavel_cli.platforms.windows.ctypes.windll',
                   SimpleNamespace(user32=user32)):
            self.assertTrue(self.adapter._post_close_to_process_windows(123))
        user32.PostMessageW.assert_called_once_with(0x1001, 0x0010, 0, 0)

    @patch('tools.kotavel_cli.platforms.windows.subprocess.Popen')
    def test_run_application_force_stops_api_when_break_times_out(self, popen):
        process = Mock()
        process.pid = 123
        process.wait.side_effect = [KeyboardInterrupt(),
                                    subprocess.TimeoutExpired('api.exe', 3), None]
        popen.return_value = process
        with patch('tools.kotavel_cli.platforms.windows.subprocess.CREATE_NEW_PROCESS_GROUP',
                   512, create=True), \
             patch.object(self.adapter, 'request_process_stop') as stop, \
             patch.object(self.adapter, 'force_process_stop') as force:
            with self.assertRaises(KeyboardInterrupt):
                self.adapter.run_application(['api.exe'], cwd='.', env={})
        stop.assert_called_once_with(123)
        force.assert_called_once_with(123)
        self.assertEqual(process.wait.call_count, 3)

    @patch('tools.kotavel_cli.platforms.windows.subprocess.Popen')
    def test_run_application_waits_after_graceful_interrupt(self, popen):
        process = Mock()
        process.pid = 123
        process.wait.side_effect = [KeyboardInterrupt(), None]
        popen.return_value = process
        with patch('tools.kotavel_cli.platforms.windows.subprocess.CREATE_NEW_PROCESS_GROUP',
                   512, create=True), \
             patch.object(self.adapter, 'request_process_stop') as stop, \
             patch.object(self.adapter, 'force_process_stop') as force:
            with self.assertRaises(KeyboardInterrupt):
                self.adapter.run_application(['api.exe'], cwd='.', env={})
        stop.assert_called_once_with(123)
        force.assert_not_called()
        self.assertEqual(process.wait.call_count, 2)

    def test_windows_process_wait_checks_for_interrupts_between_short_waits(self):
        process = Mock()
        process.wait.side_effect = [
            subprocess.TimeoutExpired('api.exe', 0.2),
            subprocess.TimeoutExpired('api.exe', 0.2),
            0,
        ]
        self.assertEqual(self.adapter._wait_for_process(process), 0)
        self.assertEqual(process.wait.call_args_list,
                         [call(timeout=0.2)] * 3)


if __name__ == '__main__':
    unittest.main()
