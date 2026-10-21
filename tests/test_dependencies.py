"""Exercise dependency transactions without downloading packages."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tools.kotavel_cli import deps
BASE = 'from conan import ConanFile\nclass Project(ConanFile):\n    requires = ("spdlog/1.15.3",)\n'
FMT = '\n    def requirements(self):\n        self.requires("fmt/[>=11.2.0 <12]")\n'
NEW_LOCK = json.dumps({'version': '0.5', 'requires': ['fmt/11.2.0']}).encode()
OLD_LOCK = json.dumps({'version': '0.5', 'requires': []}).encode()


class DependencyTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name)
        for attr in ('ROOT', 'TOOLS'):
            patcher = patch.object(deps, attr, self.root)
            patcher.start()
            self.addCleanup(patcher.stop)
        self.recipe = self.root / 'conanfile.py'
        self.recipe.write_text(BASE)
        self.lock = self.root / 'conan.lock'
        self.lock.write_bytes(OLD_LOCK)
        self.commands = []
        self.failure = None
        self.external_edit = None

    def execute(self, command, env):
        self.commands.append(command)
        if command[1] == 'require':
            self.recipe.write_text(BASE + FMT)
        if command[1] == self.failure:
            raise subprocess.CalledProcessError(1, command)
        if command[1] == 'lock':
            Path(command[command.index('--lockfile-out') + 1]).write_bytes(NEW_LOCK)
        if command[1] == 'install' and self.external_edit:
            self.external_edit()

    def run_add(self, reference='fmt/11.2.0', offline=False):
        with patch.object(deps, 'execute', side_effect=self.execute):
            deps.add_dependency(reference, 'conan', {}, offline)

    def test_checks_declarations_calls_and_ignores_other_classes(self):
        source = BASE + FMT + '\nclass Other:\n    requires = get_dependencies()\n'
        self.assertEqual(deps.declared_dependencies(source), {'spdlog', 'fmt'})
        self.assertEqual(deps.declared_dependencies(source.replace('requires = ("spdlog/1.15.3",)', 'requires = "spdlog/1.15.3"')), {'spdlog', 'fmt'})
        source = source.replace('import ConanFile', 'import ConanFile as Recipe').replace('Project(ConanFile)', 'Project(Recipe)')
        self.assertEqual(deps.declared_dependencies(source), {'spdlog', 'fmt'})

    def test_dynamic_declarations_are_not_blindly_duplicated(self):
        for source in (BASE.replace('("spdlog/1.15.3",)', 'get_dependencies()'), BASE + FMT.replace('"fmt/[>=11.2.0 <12]"', 'reference')):
            with self.assertRaises(RuntimeError):
                deps.declared_dependencies(source)

    def test_invalid_references_are_rejected(self):
        for reference in ('--help', '', '../fmt', 'fmt/', 'fmt/1/extra', 'fmt/1@user/channel', 'fmt/[>=1 <12]', 'fmt/1\n'):
            with self.subTest(reference=reference), self.assertRaises(RuntimeError):
                deps.validate_reference(reference)
        for reference in ('fmt', 'fmt/11.2.0', 'msys2/cci.latest'):
            self.assertTrue(deps.validate_reference(reference))

    def test_existing_range_and_install_only_never_add(self):
        self.recipe.write_text(BASE + FMT)
        for reference in ('fmt/11.2.0', 'fmt/11.1.0', 'fmt', None):
            self.commands.clear()
            self.run_add(reference, offline=True)
            self.assertEqual([command[1] for command in self.commands], ['lock', 'install'])
            self.assertTrue(all('--no-remote' in command for command in self.commands))
            self.assertEqual(self.recipe.read_text(), BASE + FMT)
            self.assertEqual(self.lock.read_bytes(), NEW_LOCK)

    def test_success_adds_once_and_publishes_lock(self):
        self.lock.unlink()
        self.run_add()
        self.assertEqual(self.recipe.read_text(), BASE + FMT)
        self.assertEqual(self.lock.read_bytes(), NEW_LOCK)
        self.assertNotIn('--lockfile', self.commands[1])
        self.assertFalse(list(self.root.glob('deps-*')))

    def test_failures_restore_recipe_and_preserve_lock(self):
        for failing_step in ('require', 'lock', 'install'):
            self.failure = failing_step
            with self.subTest(step=failing_step), self.assertRaises(subprocess.CalledProcessError):
                self.run_add()
            self.assertEqual(self.recipe.read_text(), BASE)
            self.assertEqual(self.lock.read_bytes(), OLD_LOCK)
            self.assertFalse(list(self.root.glob('deps-*')))

    def test_success_exit_without_adding_dependency_is_an_error(self):
        with patch.object(deps, 'execute'), self.assertRaisesRegex(RuntimeError, 'no añadió'):
            deps.add_dependency('fmt', 'conan', {})
        self.assertEqual(self.recipe.read_text(), BASE)
        self.assertEqual(self.lock.read_bytes(), OLD_LOCK)

    def test_bad_generated_lock_never_reaches_install(self):
        def execute(command, env):
            self.execute(command, env)
            if command[1] == 'lock':
                Path(command[command.index('--lockfile-out') + 1]).write_text('{}')
        with patch.object(deps, 'execute', side_effect=execute), self.assertRaisesRegex(RuntimeError, 'lockfile inválido'):
            deps.add_dependency('fmt', 'conan', {})
        self.assertEqual([command[1] for command in self.commands], ['require', 'lock'])
        self.assertEqual(self.recipe.read_text(), BASE)
        self.assertEqual(self.lock.read_bytes(), OLD_LOCK)

    def test_interrupt_restores_recipe(self):
        def execute(command, env):
            self.execute(command, env)
            if command[1] == 'install':
                raise KeyboardInterrupt()
        with patch.object(deps, 'execute', side_effect=execute), self.assertRaises(KeyboardInterrupt):
            deps.add_dependency('fmt', 'conan', {})
        self.assertEqual(self.recipe.read_text(), BASE)
        self.assertEqual(self.lock.read_bytes(), OLD_LOCK)

    def test_external_edits_are_preserved(self):
        for target in (self.recipe, self.lock):
            self.recipe.write_text(BASE)
            self.lock.write_bytes(OLD_LOCK)
            self.external_edit = lambda: target.write_text('external edit')
            with self.assertRaisesRegex(RuntimeError, 'edición externa'):
                self.run_add()
            self.assertEqual(target.read_text(), 'external edit')

    def test_unchanged_lock_keeps_timestamp(self):
        self.recipe.write_text(BASE + FMT)
        self.lock.write_bytes(NEW_LOCK)
        os.utime(self.lock, (1000000000, 1000000000))
        self.run_add()
        self.assertEqual(self.lock.stat().st_mtime_ns, 1000000000000000000)

    def test_guard_excludes_second_run_and_releases_after_failure(self):
        with self.assertRaisesRegex(RuntimeError, 'otro error'):
            with deps.project_guard():
                with self.assertRaisesRegex(RuntimeError, 'Otro comando'):
                    with deps.project_guard():
                        self.fail('A second run acquired the same project')
                raise RuntimeError('otro error')
        with deps.project_guard():
            pass

    def test_platform_environment_and_offline_preparation(self):
        for system, directory, executable, separator in (
                ('nt', 'Scripts', 'conan.exe', ';'), ('posix', 'bin', 'conan', ':')):
            binary = self.root / 'python' / directory / executable
            binary.parent.mkdir(parents=True, exist_ok=True)
            binary.touch()
            build_environment = {'CONAN_OPENSSL_CONFIGURATION': 'mingw64'} if system == 'nt' else {}
            (self.root / 'platform.json').write_text(json.dumps({
                'compiler_bin': 'compiler', 'build_environment': build_environment,
            }), encoding='utf-8-sig')
            platform_os = SimpleNamespace(name=system, environ={'PATH': 'original'}, pathsep=separator)
            platform_name = 'Windows' if system == 'nt' else 'Linux'
            with patch.object(deps, 'os', platform_os), patch.object(deps.platform, 'system', return_value=platform_name), patch.object(deps, 'execute') as run:
                conan, env = deps.prepare_environment(True)
            self.assertEqual(conan, binary)
            self.assertEqual(env['PATH'], separator.join((str(binary.parent), 'compiler', 'original')))
            expected_home = Path.home() / '.k' if system == 'nt' else self.root / '.kotavel/conan'
            self.assertEqual(env['CONAN_HOME'], str(expected_home))
            if system == 'nt':
                self.assertEqual(env['CONAN_OPENSSL_CONFIGURATION'], 'mingw64')
            else:
                self.assertNotIn('CONAN_OPENSSL_CONFIGURATION', env)
            self.assertIn('--configure-only', run.call_args.args[0])
