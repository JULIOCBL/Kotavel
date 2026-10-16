import importlib.util
from contextlib import redirect_stdout
import io
import json
from pathlib import Path
import sys
import subprocess
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from kotavel_cli import cli, deps, install, monitor


class ConsumerToolsTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory(prefix='kotavel-consumer-')
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)

    def test_install_receives_explicit_consumer_and_arguments(self):
        with patch.object(deps, 'main') as command:
            self.assertEqual(cli.main(['--project', str(self.root), 'install', 'add', 'fmt/11.2.0', '--offline']), 0)
        command.assert_called_once_with(['add', 'fmt/11.2.0', '--offline', '--project', str(self.root.resolve())])

    def test_build_test_and_run_dispatch_to_consumer(self):
        for name, flag in (('build', '--build'), ('test', '--test'), ('run', '--run')):
            with self.subTest(name=name), patch.object(install, 'main') as command:
                self.assertEqual(cli.main(['--project', str(self.root), name, '--build-type', 'Release']), 0)
            command.assert_called_once_with([flag, '--build-type', 'Release', '--project', str(self.root.resolve())])

    def test_invalid_project_returns_error_without_starting_tools(self):
        with patch.object(install, 'ROOT', self.root), patch.object(install, 'TOOLS', self.root / '.kotavel/tools'), patch.object(install, 'CONAN_HOME', self.root / '.kotavel/conan'), patch.object(install, 'prepare_platform') as prepare, patch('sys.stderr'):
            self.assertEqual(cli.main(['--project', str(self.root), 'prepare']), 1)
            prepare.assert_not_called()

    def test_framework_tests_forward_scope_to_installer(self):
        with patch.object(install, 'main') as command:
            self.assertEqual(cli.main(['--project', str(self.root), 'test', '--framework']), 0)
        command.assert_called_once_with(['--test', '--framework', '--project', str(self.root.resolve())])

    def test_monitor_command_dispatches_with_selected_project(self):
        with patch.object(monitor, 'main', return_value=0) as command:
            self.assertEqual(cli.main(['--project', str(self.root), 'monitor', '--pid', '123']), 0)
        command.assert_called_once_with(['--pid', '123', '--project', str(self.root.resolve())])

    def test_monitor_formats_startup_and_sample_events(self):
        output = io.StringIO()
        with redirect_stdout(output):
            monitor._show({
                'type': 'startup', 'environment_ms': 1.0, 'configuration_ms': 2.0,
                'settings_ms': 3.0, 'application_boot_ms': 4.0, 'total_ms': 10.0,
            })
            monitor._show({'type': 'sample', 'cpu_percent': None,
                           'resident_memory_bytes': 1024 * 1024})
        self.assertIn('Arranque:', output.getvalue())
        self.assertIn('CPU: calculando | RAM residente: 1.0 MiB', output.getvalue())

    def test_monitor_rejects_unknown_event(self):
        with self.assertRaisesRegex(ValueError, 'evento de métricas desconocido'):
            monitor._show({'type': 'future-event'})

    def test_framework_tests_reject_package_mode_before_preparation(self):
        (self.root / 'conanfile.py').write_text('')
        (self.root / 'kotavel.json').write_text(json.dumps({'mode': 'package'}))
        with patch.object(install, 'ROOT', self.root), patch.object(install, 'prepare_platform') as prepare, patch('sys.stderr'):
            self.assertEqual(cli.main(['--project', str(self.root), 'test', '--framework']), 1)
            prepare.assert_not_called()

    def test_framework_validation_runs_all_suites_and_stops_on_each_failure(self):
        framework = self.root / 'framework with spaces'
        for source in (framework, self.root):
            (source / 'tests').mkdir(parents=True)
            (source / 'tests' / 'test_example.py').touch()
        config = {'conf': {'tools.build:compiler_executables': json.dumps({'cpp': '/selected compiler/clang++'})}}
        python = '/selected python/python'
        for build_type in ('Debug', 'Release'):
            for failure in (None, *range(9)):
                commands = []
                def execute(command, env):
                    commands.append([str(arg) for arg in command])
                    if len(commands) - 1 == failure:
                        raise subprocess.CalledProcessError(1, command)
                with self.subTest(build_type=build_type, failure=failure), patch.object(install, 'ROOT', self.root), patch.object(install, 'run', side_effect=execute), patch('builtins.print'):
                    if failure is None:
                        install.test_framework(framework, build_type, config, python, {})
                    else:
                        with self.assertRaises(subprocess.CalledProcessError):
                            install.test_framework(framework, build_type, config, python, {})
                self.assertEqual(len(commands), 9 if failure is None else failure + 1)
                if failure is None:
                    self.assertEqual(commands[0], [python, '-m', 'unittest', 'discover', '-s', str(framework / 'tests'), '-p', 'test_*.py'])
                    self.assertEqual(commands[1][5], str(self.root / 'tests'))
                    self.assertIn('-DCMAKE_CXX_COMPILER=/selected compiler/clang++', commands[2])
                    self.assertIn('-DCMAKE_BUILD_TYPE=' + build_type, commands[2])
                    self.assertEqual(commands[5], ['conan', 'install', '.', '--build=missing'])
                    self.assertIn('-DBUILD_TESTING=ON', commands[6])
                    self.assertIn('conan-' + build_type.lower(), commands[8])
                    for index in (4, 8):
                        self.assertIn('--no-tests=error', commands[index])

    def test_framework_validation_rejects_missing_python_suites(self):
        with patch.object(install, 'ROOT', self.root), patch.object(install, 'run') as run:
            with self.assertRaisesRegex(RuntimeError, 'pruebas Python'):
                install.test_framework(self.root / 'missing', 'Debug', {}, sys.executable, {})
            run.assert_not_called()

    def test_framework_detection_uses_grouped_core_test_directory(self):
        framework = self.root / 'framework'
        grouped_core_test = framework / 'tests' / 'unit' / 'core' / 'core_tests.cpp'
        grouped_core_test.parent.mkdir(parents=True)
        grouped_core_test.touch()
        self.assertTrue(install.has_framework_cpp_tests(framework))

        old_layout_test = framework / 'tests' / 'unit' / 'core_tests.cpp'
        old_layout_test.touch()
        grouped_core_test.unlink()
        self.assertFalse(install.has_framework_cpp_tests(framework))

    def test_consumer_executable_is_configurable_and_rejects_paths(self):
        with patch.object(install, 'ROOT', self.root):
            (self.root / 'kotavel.json').write_text(json.dumps({'executable': 'another-app'}))
            self.assertEqual(install.project_executable(), 'another-app')
            (self.root / 'kotavel.json').write_text(json.dumps({'executable': '../outside'}))
            with self.assertRaises(RuntimeError):
                install.project_executable()

    def test_missing_consumer_profiles_use_packaged_defaults(self):
        with patch.object(install, 'ROOT', self.root):
            path, config = install.read_profile('Darwin')
            self.assertEqual(path, install.DEFAULT_PROFILES / 'macos')
            self.assertEqual(config['settings']['compiler'], 'apple-clang')

    def test_consumer_profile_overrides_defaults(self):
        profiles = self.root / 'profiles'
        profiles.mkdir()
        (profiles / 'linux').write_text('[settings]\nos=Linux\ncompiler=gcc\ncompiler.cppstd=23\nbuild_type=Release\n')
        with patch.object(install, 'ROOT', self.root):
            path, config = install.read_profile('Linux')
            self.assertEqual(path, profiles / 'linux')
            self.assertEqual(config['settings']['build_type'], 'Release')
