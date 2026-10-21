import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tools.kotavel_cli.conan_remote import REMOTE_NAME, configure_remote
from tools.kotavel_cli.project_paths import conan_home


class ConanRemoteTests(unittest.TestCase):
    def test_remote_is_optional(self):
        commands = []
        self.assertFalse(configure_remote('conan', {}, lambda command, env: commands.append(command)))
        self.assertEqual(commands, [])

    def test_configures_shared_binary_remote_and_login(self):
        commands = []
        env = {
            'KOTAVEL_CONAN_REMOTE_URL': 'https://packages.example.test/artifactory/api/conan/kotavel',
            'KOTAVEL_CONAN_REMOTE_USER': 'builder',
            'KOTAVEL_CONAN_REMOTE_PASSWORD': 'secret',
        }
        self.assertTrue(configure_remote('conan', env, lambda command, _: commands.append(command)))
        self.assertEqual(commands, [
            ['conan', 'remote', 'add', REMOTE_NAME, env['KOTAVEL_CONAN_REMOTE_URL'], '--index', '0', '--force'],
            ['conan', 'remote', 'login', REMOTE_NAME, 'builder', '-p', 'secret'],
        ])

    def test_rejects_invalid_url_or_partial_credentials(self):
        for env in (
            {'KOTAVEL_CONAN_REMOTE_URL': 'file:///tmp/cache'},
            {'KOTAVEL_CONAN_REMOTE_URL': 'https://packages.example.test/cache',
             'KOTAVEL_CONAN_REMOTE_USER': 'builder'},
        ):
            with self.subTest(env=env), self.assertRaises(RuntimeError):
                configure_remote('conan', env, lambda *_: self.fail('Invalid settings reached Conan'))

    def test_new_projects_share_cache_and_existing_project_cache_is_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            home = root / 'user-home'
            project_a = root / 'project-a'
            project_b = root / 'project-b'
            with patch.object(Path, 'home', return_value=home):
                self.assertEqual(conan_home(project_a, 'Linux'), conan_home(project_b, 'Linux'))
                self.assertEqual(conan_home(project_a, 'Linux'), home / '.kotavel' / 'conan')
                legacy = project_a / '.kotavel' / 'conan'
                legacy.mkdir(parents=True)
                self.assertEqual(conan_home(project_a, 'Linux'), legacy.resolve())
                with patch.dict(os.environ, {'KOTAVEL_CONAN_HOME': str(root / 'override')}):
                    self.assertEqual(conan_home(project_b, 'Linux'), (root / 'override').resolve())


if __name__ == '__main__':
    unittest.main()
