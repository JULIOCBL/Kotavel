"""Verify profile selection and preservation without installing tools."""
import importlib.util
import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('kotavel_setup', Path(__file__).resolve().parents[1] / 'tools' / 'kotavel_cli' / 'install.py')
setup = importlib.util.module_from_spec(spec)
spec.loader.exec_module(setup)


class ProfileTests(unittest.TestCase):
    def test_selects_each_system_profile(self):
        for system, (name, expected_os) in setup.SYSTEMS.items():
            with self.subTest(system=system):
                path, config = setup.read_profile(system)
                self.assertEqual(path.name, name)
                self.assertEqual(config['settings']['os'], expected_os)
                self.assertNotIn('compiler.version', config['settings'])
                self.assertNotIn('arch', config['settings'])

    def test_profile_is_the_source_of_build_configuration(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'profiles').mkdir()
            (root / 'profiles' / 'linux').write_text('[settings]\nos=Linux\ncompiler=gcc\ncompiler.cppstd=20\nbuild_type=Release\n[conf]\ntools.build:jobs=2\n', encoding='utf-8')
            with patch.object(setup, 'ROOT', root):
                _, config = setup.read_profile('Linux')
            self.assertEqual(config['settings']['compiler.cppstd'], '20')
            self.assertEqual(config['settings']['build_type'], 'Release')
            self.assertEqual(config['conf']['tools.build:jobs'], '2')

    def test_rejects_wrong_operating_system_in_profile(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'profiles').mkdir()
            (root / 'profiles' / 'windows').write_text('[settings]\nos=Linux\n', encoding='utf-8')
            with patch.object(setup, 'ROOT', root):
                with self.assertRaises(RuntimeError):
                    setup.read_profile('Windows')

    def test_rejects_unsupported_platform(self):
        with self.assertRaises(RuntimeError):
            setup.read_profile('Other')
        with self.assertRaises(RuntimeError):
            setup.architecture('unknown')

    def test_normalizes_architecture_names(self):
        self.assertEqual(setup.architecture('AMD64'), 'x86_64')
        self.assertEqual(setup.architecture('arm64'), 'armv8')
        self.assertEqual(setup.architecture('aarch64'), 'armv8')


    def test_windows_shell_receives_the_prepared_environment(self):
        env = {'PATH': 'local-tools', 'CONAN_HOME': 'local-conan'}
        with patch.object(setup.sys.stdin, 'isatty', return_value=True), patch.object(setup, 'run') as run:
            setup.enter_shell({'shell': ['powershell.exe', '-NoLogo', '-NoProfile']}, env)
        run.assert_called_once_with(['powershell.exe', '-NoLogo', '-NoProfile'], env)

    def test_shell_is_not_started_in_ci(self):
        with patch.object(setup.sys.stdin, 'isatty', return_value=False), patch.object(setup, 'run') as run:
            with self.assertRaises(RuntimeError):
                setup.enter_shell({'shell': ['powershell.exe']}, {'PATH': ''})
        run.assert_not_called()

    def test_dispatches_to_each_platform_installer(self):
        import json
        for system, filename in [('Windows', 'install-windows.ps1')]:
            with self.subTest(system=system), tempfile.TemporaryDirectory() as directory:
                tools = Path(directory)
                _, config = setup.read_profile(system)
                config['settings']['arch'] = 'x86_64'
                prepared = {'compiler_bin': 'compiler-path', 'shell': ['shell'], 'scripts_directory': 'bin', 'python_executable': 'python', 'activation_kind': 'posix'}
                def execute(command, env):
                    self.assertTrue(any(str(arg).endswith(filename) for arg in command))
                    (tools / 'platform.json').write_text(json.dumps(prepared), encoding='utf-8')
                env = {'PATH': 'existing-path'}
                with patch.object(setup, 'TOOLS', tools), patch.object(setup.shutil, 'which', return_value='powershell.exe'), patch.object(setup, 'run', side_effect=execute):
                    actual = setup.prepare_platform(system, config, env, True)
                self.assertEqual(actual, prepared)
                self.assertTrue(env['PATH'].startswith('compiler-path'))

    def test_platform_failure_is_not_reported_as_success(self):
        import subprocess
        _, config = setup.read_profile('Windows')
        config['settings']['arch'] = 'x86_64'
        with tempfile.TemporaryDirectory() as directory, patch.object(setup, 'TOOLS', Path(directory)), patch.object(setup.shutil, 'which', return_value='powershell.exe'), patch.object(setup, 'run', side_effect=subprocess.CalledProcessError(1, 'installer')):
            with self.assertRaises(subprocess.CalledProcessError):
                setup.prepare_platform('Windows', config, {'PATH': ''}, False)

class WindowsOptimizationTests(unittest.TestCase):
    def test_repeated_activation_keeps_path_stable_and_preserves_other_entries(self):
        env = {'PATH': 'C:\\Other;;"c:/TOOLS/bin/";C:\\Other;C:\\Tools\\bin'}
        for _ in range(3):
            setup.prepend_tools_path(env, 'C:\\Tools\\bin', 'Windows')
            self.assertEqual(env['PATH'], 'C:\\Tools\\bin;C:\\Other;;C:\\Other')

    def test_posix_path_removes_repeated_tools_directory(self):
        for system in ('Darwin', 'Linux'):
            with self.subTest(system=system):
                env = {'PATH': os.pathsep.join(['/usr/bin', '/tools'])}
                setup.prepend_tools_path(env, '/tools', system)
                self.assertEqual(env['PATH'], os.pathsep.join(['/tools', '/usr/bin']))

    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.tools = self.root / '.kotavel' / 'tools'
        self.tools.mkdir(parents=True)
        self.patch_tools = patch.object(setup, 'TOOLS', self.tools)
        self.patch_tools.start()
        self.addCleanup(self.patch_tools.stop)
        _, self.config = setup.read_profile('Windows')
        self.config['settings']['arch'] = 'x86_64'

    def toolchain(self, name, complete=True):
        binary = self.tools / name / 'bin'
        binary.mkdir(parents=True)
        (binary / 'clang++.exe').touch()
        if complete:
            (binary / 'clang.exe').touch()
        return binary

    def test_installed_compiler_avoids_powershell_and_preserves_environment(self):
        self.toolchain('llvm-mingw-20260101-ucrt-x86_64')
        binary = self.toolchain('llvm-mingw-20261006-ucrt-x86_64')
        env = {'PATH': 'existing-path'}
        with patch.object(setup.shutil, 'which', return_value='pwsh.exe'), patch.object(setup, 'run') as run:
            prepared = setup.prepare_platform('Windows', self.config, env, False)
        run.assert_not_called()
        self.assertEqual(prepared['compiler_bin'], str(binary))
        self.assertEqual(env['PATH'], str(binary) + ';existing-path')
        self.assertEqual(prepared['shell'], ['pwsh.exe', '-NoLogo', '-NoProfile'])
        self.assertEqual(json.loads((self.tools / 'platform.json').read_text(encoding='utf-8-sig')), prepared)

    def test_missing_or_partial_compiler_requires_normal_preparation(self):
        self.assertIsNone(setup.installed_windows_platform(self.config, {'PATH': ''}))
        self.toolchain('llvm-mingw-20260101-ucrt-x86_64')
        self.toolchain('llvm-mingw-20261006-ucrt-x86_64', complete=False)
        self.assertIsNone(setup.installed_windows_platform(self.config, {'PATH': ''}))

    def test_architecture_guard_is_preserved(self):
        self.config['settings']['arch'] = 'armv8'
        with self.assertRaisesRegex(RuntimeError, 'Python x64'):
            setup.installed_windows_platform(self.config, {'PATH': ''})

    def test_other_compilers_keep_normal_preparation(self):
        self.config['settings']['compiler'] = 'msvc'
        self.assertIsNone(setup.installed_windows_platform(self.config, {'PATH': ''}))

    def test_local_cmake_and_ctest_use_native_binaries(self):
        for name in ('cmake.exe', 'ctest.exe'):
            launcher = self.tools / 'python' / 'Scripts' / name
            native = self.tools / 'python' / 'Lib' / 'site-packages' / 'cmake' / 'data' / 'bin' / name
            native.parent.mkdir(parents=True, exist_ok=True)
            native.touch()
            env = {'PATH': 'local-tools'}
            with self.subTest(name=name), patch.object(setup.platform, 'system', return_value='Windows'), patch.object(setup.shutil, 'which', return_value=str(launcher)), patch.object(setup.subprocess, 'run') as run:
                setup.run([name, '--version'], env)
            run.assert_called_once_with([str(native), '--version'], cwd=setup.ROOT, env=env, check=True)

    def test_external_or_missing_native_cmake_keeps_original_command(self):
        for launcher in (self.root / 'external' / 'cmake.exe', self.tools / 'python' / 'Scripts' / 'cmake.exe'):
            with self.subTest(launcher=launcher), patch.object(setup.platform, 'system', return_value='Windows'), patch.object(setup.shutil, 'which', return_value=str(launcher)), patch.object(setup.subprocess, 'run') as run:
                setup.run(['cmake', '--version'], {})
            self.assertEqual(run.call_args.args[0], [str(launcher), '--version'])

    def test_unchanged_files_keep_timestamp_and_changed_files_are_updated(self):
        for encoding in ('utf-8', 'utf-8-sig'):
            path = self.root / encoding
            setup.write_if_changed(path, 'perfil\n', encoding)
            os.utime(path, (1000000000, 1000000000))
            previous = path.stat().st_mtime_ns
            setup.write_if_changed(path, 'perfil\n', encoding)
            self.assertEqual(path.stat().st_mtime_ns, previous)
            setup.write_if_changed(path, 'nuevo perfil\n', encoding)
            self.assertEqual(path.read_text(encoding=encoding), 'nuevo perfil\n')

    def test_msvc_version_detection_runs_compiler_once(self):
        self.config['settings']['compiler'] = 'msvc'
        result = subprocess.CompletedProcess(['cl'], 2, stdout='', stderr='Microsoft C/C++ Compiler Version 19.44.35207')
        with patch.object(setup.subprocess, 'run', return_value=result) as run:
            setup.compiler_environment(self.config, {'PATH': ''}, {'c': 'cl', 'cpp': 'cl'})
        run.assert_called_once()
        self.assertEqual(self.config['settings']['compiler.version'], '194')

    def test_full_run_keeps_dependency_checks_and_stops_on_test_failure(self):
        for fail_tests in (False, True):
            commands = []
            config = setup.configparser.ConfigParser(interpolation=None, delimiters=('=',))
            config.read_dict({section: dict(self.config[section]) for section in self.config.sections()})
            config['settings']['compiler.version'] = '23'
            executable = self.root / 'build' / 'Debug' / 'kotavel-lab.exe'
            (self.root / 'conanfile.py').touch()
            (self.root / 'kotavel.json').write_text(json.dumps({'executable': 'kotavel-lab'}))
            executable.parent.mkdir(parents=True, exist_ok=True)
            executable.touch()
            prepared = {'scripts_directory': 'Scripts', 'python_executable': 'python.exe', 'activation_kind': 'powershell'}
            def execute(args, env):
                commands.append([str(arg) for arg in args])
                if fail_tests and args[0] == 'ctest':
                    raise subprocess.CalledProcessError(1, args)
            with self.subTest(fail_tests=fail_tests), patch.object(setup, 'ROOT', self.root), patch.object(setup, 'CONAN_HOME', self.root / 'conan'), patch.object(setup, 'read_profile', return_value=(self.root / 'profiles' / 'windows', config)), patch.object(setup.platform, 'system', return_value='Windows'), patch.object(setup.sysconfig, 'get_platform', return_value='win-amd64'), patch.object(setup, 'prepare_platform', return_value=prepared), patch.object(setup, 'compiler_environment'), patch.object(setup.shutil, 'which', return_value='tool'), patch.object(setup, 'run', side_effect=execute), patch.object(setup, 'project_executable', return_value='kotavel-lab'), patch.object(setup.sys, 'argv', ['install.py', '--configure-only', '--run']), patch.object(setup, 'success_banner') as banner:
                if fail_tests:
                    with self.assertRaises(subprocess.CalledProcessError):
                        setup.main()
                    banner.assert_not_called()
                else:
                    setup.main()
                    banner.assert_called_once_with('Windows', 'run')
            self.assertEqual(commands[:4], [
                ['conan', 'install', '.', '--build=missing'],
                ['cmake', '--preset', 'conan-debug', '-DKOTAVEL_APP_ENV=production'],
                ['cmake', '--build', '--preset', 'conan-debug'],
                ['ctest', '--preset', 'conan-debug', '--output-on-failure'],
            ])
            self.assertEqual(commands[4:], [] if fail_tests else [[str(executable), '--env=production', '--project-root=' + str(executable.parent.parent.parent)]])


@unittest.skipIf(os.name == 'nt', 'Requires a POSIX shell')
class LinuxOptimizationTests(unittest.TestCase):
    def test_path_is_repeatable_and_preserves_other_entries(self):
        directory = '/tools with spaces/bin'
        env = {'PATH': '/usr/bin:/tools with spaces/bin/:/Tools/bin::/usr/bin'}
        setup.prepend_tools_path(env, directory, 'Linux')
        expected = directory + ':/usr/bin:/Tools/bin::/usr/bin'
        self.assertEqual(env['PATH'], expected)
        setup.prepend_tools_path(env, directory, 'Linux')
        self.assertEqual(env['PATH'], expected)

    def test_native_cmake_supports_lib64_and_linked_library_directories(self):
        launcher = self.root / 'python/bin/cmake'
        native = self.executable(self.root / 'python/lib64/python3.14/site-packages/cmake/data/bin/cmake')
        for linked in (False, True):
            if linked:
                (self.root / 'python/lib').symlink_to('lib64', target_is_directory=True)
            with self.subTest(linked=linked), patch.object(setup.platform, 'system', return_value='Linux'), patch.object(setup.shutil, 'which', return_value=str(launcher)), patch.object(setup.subprocess, 'run') as run:
                setup.run(['cmake', '--version'], {})
                self.assertEqual(run.call_args.args[0], [str(native.resolve()), '--version'])

    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name).resolve()
        patcher = patch.object(setup, 'TOOLS', self.root)
        patcher.start()
        self.addCleanup(patcher.stop)

    def executable(self, path):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text('#!/bin/sh\nexit 0\n')
        path.chmod(0o755)
        return path

    def test_preparation_is_in_process_and_rechecks_path(self):
        _, config = setup.read_profile('Linux')
        for version in ('14', '15'):
            directory = self.root / version
            for name in ('gcc', 'g++'):
                self.executable(directory / f'{name}-{version}')
            env = {'PATH': str(directory), 'SHELL': '/bin/bash'}
            with patch.object(setup, 'run') as run:
                prepared = setup.prepare_platform('Linux', config, env, True)
            run.assert_not_called()
            self.assertEqual(prepared['compiler_executables']['cpp'], str(directory / f'g++-{version}'))
            manifest = self.root / 'platform.json'
            os.utime(manifest, (1000000000, 1000000000))
            setup.prepare_platform('Linux', config, env, False)
            self.assertEqual(manifest.stat().st_mtime_ns, 1000000000000000000)
        with self.assertRaisesRegex(RuntimeError, 'El perfil requiere gcc'):
            setup.prepare_platform('Linux', config, {'PATH': '', 'SHELL': '/bin/bash'}, False)

    def test_native_tools_and_safe_fallbacks(self):
        for name in ('cmake', 'ctest'):
            launcher = self.root / 'python' / 'bin' / name
            native = self.executable(self.root / 'python/lib/python3.14/site-packages/cmake/data/bin' / name)
            for system, selected, expected in [('Linux', launcher, native),
                                               ('Linux', self.root / 'external' / name, self.root / 'external' / name),
                                               ('Darwin', launcher, native)]:
                with self.subTest(name=name, system=system, selected=selected), patch.object(setup.platform, 'system', return_value=system), patch.object(setup.shutil, 'which', return_value=str(selected)), patch.object(setup.subprocess, 'run') as run:
                    setup.run([name, '--version'], {})
                self.assertEqual(run.call_args.args[0], [str(expected), '--version'])
            native.chmod(0o644)
            with patch.object(setup.platform, 'system', return_value='Linux'), patch.object(setup.shutil, 'which', return_value=str(launcher)), patch.object(setup.subprocess, 'run') as run:
                setup.run([name, '--version'], {})
            self.assertEqual(run.call_args.args[0][0], str(launcher))
            native.chmod(0o755)
            self.executable(self.root / 'python/lib/python3.13/site-packages/cmake/data/bin' / name)
            with patch.object(setup.platform, 'system', return_value='Linux'), patch.object(setup.shutil, 'which', return_value=str(launcher)), patch.object(setup.subprocess, 'run') as run:
                setup.run([name, '--version'], {})
            self.assertEqual(run.call_args.args[0][0], str(launcher))

    def test_activation_is_only_written_when_changed(self):
        prepared = {'activation_kind': 'posix'}
        env = {'PATH': '/some path/bin'}
        with patch.object(setup.platform, 'system', return_value='Linux'):
            setup.write_activation(prepared, env)
            activation = self.root / 'activate.sh'
            os.utime(activation, (1000000000, 1000000000))
            setup.write_activation(prepared, env)
            self.assertEqual(activation.stat().st_mtime_ns, 1000000000000000000)
            env['PATH'] = '/another/bin'
            setup.write_activation(prepared, env)
            self.assertIn('/another/bin', activation.read_text())

    def test_apple_clang_manifest_and_version_validation(self):
        for name in ('clang', 'clang++'):
            self.executable(self.root / name)
        env = {'PATH': str(self.root), 'SHELL': '/bin/zsh'}
        prepared = setup._posix.prepare('apple-clang', env)
        self.assertEqual(prepared['shell'], ['/bin/zsh', '-i'])
        _, config = setup.read_profile('Darwin')
        for output, valid in [('Apple clang version 17.0.0', True), ('clang version 17.0.0', False)]:
            result = subprocess.CompletedProcess([], 0, stdout=output)
            with patch.object(setup.subprocess, 'run', return_value=result):
                if valid:
                    setup.compiler_environment(config, env, prepared['compiler_executables'])
                    self.assertEqual(config['settings']['compiler.version'], '17')
                else:
                    with self.assertRaisesRegex(RuntimeError, 'compilador de Apple'):
                        setup.compiler_environment(config, env, prepared['compiler_executables'])

    def test_linux_run_preserves_profile_and_stops_on_test_failure(self):
        _, config = setup.read_profile('Linux')
        config['settings']['compiler.version'] = '15'
        prepared = {'scripts_directory': 'bin', 'python_executable': 'python', 'activation_kind': 'posix'}
        executable = self.executable(self.root / 'build/Debug/kotavel-lab')
        (self.root / 'conanfile.py').touch()
        (self.root / 'kotavel.json').write_text(json.dumps({'executable': 'kotavel-lab'}))
        profile = self.root / 'conan/profiles/default'
        for fail_tests in (False, True):
            commands = []
            def execute(args, env):
                commands.append([str(arg) for arg in args])
                if fail_tests and args[0] == 'ctest':
                    raise subprocess.CalledProcessError(1, args)
            with patch.object(setup, 'ROOT', self.root), patch.object(setup, 'CONAN_HOME', self.root / 'conan'), patch.object(setup, 'read_profile', return_value=(self.root / 'profiles/linux', config)), patch.object(setup.platform, 'system', return_value='Linux'), patch.object(setup.platform, 'machine', return_value='aarch64'), patch.object(setup, 'prepare_platform', return_value=prepared), patch.object(setup, 'compiler_environment'), patch.object(setup.shutil, 'which', return_value='tool'), patch.object(setup, 'run', side_effect=execute), patch.object(setup, 'project_executable', return_value='kotavel-lab'), patch.object(setup.sys, 'argv', ['install.py', '--configure-only', '--run']), patch.object(setup, 'success_banner') as banner:
                if fail_tests:
                    with self.assertRaises(subprocess.CalledProcessError):
                        setup.main()
                    banner.assert_not_called()
                    self.assertEqual(profile.stat().st_mtime_ns, 1000000000000000000)
                else:
                    setup.main()
                    banner.assert_called_once_with('Linux', 'run')
                    os.utime(profile, (1000000000, 1000000000))
            self.assertEqual(commands[:4], [
                ['conan', 'install', '.', '--build=missing'],
                ['cmake', '--preset', 'conan-debug', '-DKOTAVEL_APP_ENV=production'],
                ['cmake', '--build', '--preset', 'conan-debug'],
                ['ctest', '--preset', 'conan-debug', '--output-on-failure'],
            ])
            self.assertEqual(commands[4:], [] if fail_tests else [[str(executable), '--env=production', '--project-root=' + str(executable.parent.parent.parent)]])


@unittest.skipIf(os.name == 'nt', 'Requires POSIX executables')
class MacOptimizationTests(unittest.TestCase):
    def test_preparation_rechecks_path_and_preserves_unchanged_files(self):
        _, config = setup.read_profile('Darwin')
        with tempfile.TemporaryDirectory() as directory, patch.object(setup, 'TOOLS', Path(directory)), patch.object(setup, 'run') as run:
            root = Path(directory)
            for name in ('xcode', 'command-line-tools'):
                binary = root / name
                binary.mkdir()
                for compiler in ('clang', 'clang++'):
                    path = binary / compiler
                    path.write_text('#!/bin/sh\necho "Apple clang version 15.0.0"\n')
                    path.chmod(0o755)
                env = {'PATH': str(binary), 'SHELL': '/bin/zsh'}
                prepared = setup.prepare_platform('Darwin', config, env, True)
                self.assertEqual(prepared['compiler_executables']['cpp'], str(binary / 'clang++'))
                self.assertEqual(prepared['shell'], ['/bin/zsh', '-i'])
                setup.compiler_environment(config, env, prepared['compiler_executables'])
                self.assertEqual(config['settings']['compiler.version'], '15')
                with patch.object(setup.platform, 'system', return_value='Darwin'):
                    setup.write_activation(prepared, env)
                    paths = (root / 'platform.json', root / 'activate.sh')
                    for path in paths:
                        os.utime(path, (1000000000, 1000000000))
                    setup.prepare_platform('Darwin', config, env, False)
                    setup.write_activation(prepared, env)
                    for path in paths:
                        self.assertEqual(path.stat().st_mtime_ns, 1000000000000000000)
                    self.assertIn(str(binary), paths[1].read_text())
            with self.assertRaisesRegex(RuntimeError, 'xcode-select --install'):
                setup.prepare_platform('Darwin', config, {'PATH': '', 'SHELL': '/bin/zsh'}, True)
            run.assert_not_called()

    def test_native_tools_preserve_fallbacks_and_external_selection(self):
        with tempfile.TemporaryDirectory() as directory, patch.object(setup, 'TOOLS', Path(directory).resolve()), patch.object(setup.platform, 'system', return_value='Darwin'):
            root = Path(directory).resolve()
            for name in ('cmake', 'ctest'):
                launcher = root / 'python/bin' / name
                native = root / 'python/lib/python3.14/site-packages/cmake/data/bin' / name
                alternate = root / 'python/lib/python3.13/site-packages/cmake/data/bin' / name
                for state in ('missing', 'not-executable', 'executable', 'ambiguous', 'external'):
                    if state == 'not-executable':
                        native.parent.mkdir(parents=True, exist_ok=True)
                        native.touch()
                        native.chmod(0o644)
                    elif state == 'executable':
                        native.chmod(0o755)
                    elif state == 'ambiguous':
                        alternate.parent.mkdir(parents=True, exist_ok=True)
                        alternate.touch()
                        alternate.chmod(0o755)
                    elif state == 'external':
                        alternate.unlink()
                    selected = root / 'external' / name if state == 'external' else launcher
                    expected = native if state == 'executable' else selected
                    with self.subTest(name=name, state=state), patch.object(setup.shutil, 'which', return_value=str(selected)), patch.object(setup.subprocess, 'run') as run:
                        setup.run([name, '--version'], {})
                        self.assertEqual(run.call_args.args[0], [str(expected), '--version'])

    def test_run_preserves_profile_and_stops_on_test_failure(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            tools = root / 'tools'
            _, config = setup.read_profile('Darwin')
            config['settings']['compiler.version'] = '15'
            prepared = {'scripts_directory': 'bin', 'python_executable': 'python', 'activation_kind': 'posix'}
            executable = root / 'build/Release/kotavel-lab'
            executable.parent.mkdir(parents=True)
            executable.touch()
            (root / 'conanfile.py').touch()
            (root / 'kotavel.json').write_text(json.dumps({'executable': 'kotavel-lab'}))
            profile = root / 'conan/profiles/default'
            for fail_tests in (False, True):
                commands = []
                def execute(args, env):
                    commands.append([str(arg) for arg in args])
                    if fail_tests and args[0] == 'ctest':
                        raise subprocess.CalledProcessError(1, args)
                with self.subTest(fail_tests=fail_tests), patch.object(setup, 'ROOT', root), patch.object(setup, 'TOOLS', tools), patch.object(setup, 'CONAN_HOME', root / 'conan'), patch.object(setup, 'read_profile', return_value=(root / 'profiles/macos', config)), patch.object(setup.platform, 'system', return_value='Darwin'), patch.object(setup.platform, 'machine', return_value='arm64'), patch.object(setup, 'prepare_platform', return_value=prepared), patch.object(setup, 'compiler_environment'), patch.object(setup.shutil, 'which', return_value='tool'), patch.object(setup, 'run', side_effect=execute), patch.object(setup, 'project_executable', return_value='kotavel-lab'), patch.object(setup.sys, 'argv', ['install.py', '--configure-only', '--run', '--build-type', 'Release']), patch.object(setup, 'success_banner') as banner:
                    if fail_tests:
                        with self.assertRaises(subprocess.CalledProcessError):
                            setup.main()
                        banner.assert_not_called()
                        self.assertEqual(profile.stat().st_mtime_ns, 1000000000000000000)
                    else:
                        setup.main()
                        banner.assert_called_once_with('Darwin', 'run')
                        os.utime(profile, (1000000000, 1000000000))
                self.assertEqual(commands[:4], [
                    ['conan', 'install', '.', '--build=missing'],
                    ['cmake', '--preset', 'conan-release', '-DKOTAVEL_APP_ENV=production'],
                    ['cmake', '--build', '--preset', 'conan-release'],
                    ['ctest', '--preset', 'conan-release', '--output-on-failure'],
                ])
                self.assertEqual(commands[4:], [] if fail_tests else [[str(executable), '--env=production', '--project-root=' + str(executable.parent.parent.parent)]])
                content = profile.read_text()
                for setting in ('arch=armv8', 'compiler=apple-clang', 'compiler.libcxx=libc++', 'compiler.cppstd=23', 'build_type=Release'):
                    self.assertIn(setting, content)


@unittest.skipIf(os.name == 'nt', 'Requires a POSIX shell')
class PosixInstallerTests(unittest.TestCase):
    def test_versioned_gcc_is_used_by_conan_profile(self):
        shell = shutil.which('sh')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ('gcc-15', 'g++-15'):
                executable = root / name
                executable.write_text('#!/bin/sh\necho 15.2.0\n', encoding='utf-8')
                executable.chmod(0o755)
            manifest = root / 'platform.json'
            env = dict(os.environ, PATH=directory, SHELL=shell)
            subprocess.run([shell, str(setup.PLATFORM / 'install-posix.sh'),
                            sys.executable, str(manifest), 'gcc'], env=env, check=True)
            prepared = json.loads(manifest.read_text())
            _, config = setup.read_profile('Linux')
            setup.compiler_environment(config, env, prepared['compiler_executables'])
            self.assertEqual(config['settings']['compiler.version'], '15')
            self.assertEqual(json.loads(config['conf']['tools.build:compiler_executables']),
                             {'c': str(root / 'gcc-15'), 'cpp': str(root / 'g++-15')})

    def test_missing_gcc_reports_installation_command(self):
        with tempfile.TemporaryDirectory() as directory:
            manifest = Path(directory) / 'platform.json'
            result = subprocess.run([shutil.which('sh'), str(setup.PLATFORM / 'install-posix.sh'),
                                     sys.executable, str(manifest), 'gcc'],
                                    env=dict(os.environ, PATH=directory), capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn('sudo apt install build-essential', result.stderr)
            self.assertFalse(manifest.exists())

if __name__ == '__main__':
    unittest.main()
