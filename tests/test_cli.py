import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'tools'))
from kotavel_cli import cli, deps, install


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

    def test_framework_tests_reject_package_mode_before_preparation(self):
        (self.root / 'conanfile.py').write_text('')
        (self.root / 'kotavel.json').write_text(json.dumps({'mode': 'package'}))
        with patch.object(install, 'ROOT', self.root), patch.object(install, 'prepare_platform') as prepare, patch('sys.stderr'):
            self.assertEqual(cli.main(['--project', str(self.root), 'test', '--framework']), 1)
            prepare.assert_not_called()

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
