"""Tests for Linux-specific command adapter configuration."""
import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[3]))
from tools.kotavel_cli.platforms import get_platform


@unittest.skipUnless(sys.platform.startswith('linux'), 'Linux adapter tests run on the Linux CI job')
class LinuxAdapterTests(unittest.TestCase):
    def setUp(self):
        self.adapter = get_platform('Linux')

    def test_linux_profile_and_posix_tool_layout(self):
        repository = Path(__file__).resolve().parents[3]
        defaults = repository / 'tools' / 'kotavel_cli' / 'templates' / 'application' / 'profiles'
        path, profile = self.adapter.read_profile(repository, defaults)
        self.assertEqual(path.name, 'linux')
        self.assertEqual(profile['settings']['os'], 'Linux')
        self.assertEqual(self.adapter.python_scripts('/project/.kotavel/tools'),
                         Path('/project/.kotavel/tools/python/bin'))
        self.assertEqual(self.adapter.python_path('/project/.kotavel/tools'),
                         Path('/project/.kotavel/tools/python/bin/python'))
        self.assertEqual(self.adapter.conan_path('/project/.kotavel/tools'),
                         Path('/project/.kotavel/tools/python/bin/conan'))
