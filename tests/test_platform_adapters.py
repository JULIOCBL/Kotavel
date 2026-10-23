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


if __name__ == '__main__':
    unittest.main()
