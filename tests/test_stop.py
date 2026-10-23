"""Unit coverage for selecting and stopping project-owned Kotavel processes."""
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import sys
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from tools.kotavel_cli import stop
from tools.kotavel_cli.platforms.posix import PosixPlatform


class FakePlatform:
    def __init__(self, command_lines=None, running=None):
        self.command_lines = command_lines or {}
        self.running = list(running or [])
        self.stop_requests = []
        self.force_requests = []

    def process_command_line(self, process_id):
        value = self.command_lines.get(process_id)
        return value() if callable(value) else value

    def request_process_stop(self, process_id):
        self.stop_requests.append(process_id)

    def force_process_stop(self, process_id):
        self.force_requests.append(process_id)

    def process_is_running(self, process_id):
        if self.running:
            return self.running.pop(0)
        return False


class StopCommandTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='kotavel-stop-')
        self.addCleanup(self.temp.cleanup)
        self.project = Path(self.temp.name).resolve()
        self.build = self.project / 'build' / 'Debug'
        self.build.mkdir(parents=True)
        self.executable = self.build / 'Kotavel'
        self.executable.touch()
        self.metrics = self.project / 'storage' / 'framework' / 'metrics'
        self.metrics.mkdir(parents=True)

    def test_posix_process_check_does_not_treat_zombie_as_running(self):
        with patch('tools.kotavel_cli.platforms.posix.os.kill'), \
             patch('tools.kotavel_cli.platforms.posix.subprocess.run',
                   return_value=SimpleNamespace(returncode=0, stdout='Z+')):
            self.assertFalse(PosixPlatform().process_is_running(123))

    def test_posix_process_check_keeps_live_process_running(self):
        with patch('tools.kotavel_cli.platforms.posix.os.kill'), \
             patch('tools.kotavel_cli.platforms.posix.subprocess.run',
                   return_value=SimpleNamespace(returncode=0, stdout='S+')):
            self.assertTrue(PosixPlatform().process_is_running(123))

    def command_line(self, project=None, executable=None):
        root = project or self.project
        binary = executable or self.executable
        return [str(binary), '--env=production', f'--project-root={root}']

    def endpoint(self, process_id, port=49152):
        path = self.metrics / f'kotavel-{process_id}.endpoint'
        path.write_text(f'{port}\n', encoding='ascii')
        return path

    def test_process_id_accepts_only_positive_numeric_endpoint_names(self):
        for name, expected in (
            ('kotavel-42.endpoint', 42), ('kotavel-0.endpoint', None),
            ('kotavel--2.endpoint', None), ('kotavel-abc.endpoint', None),
            ('other-42.endpoint', None), ('kotavel-42.endpoint.bak', None),
        ):
            with self.subTest(name=name):
                self.assertEqual(stop._process_id(Path(name)), expected)

    def test_process_identity_requires_build_binary_and_exact_project_root(self):
        self.assertTrue(stop._belongs_to_project(self.command_line(), self.project))
        self.assertFalse(stop._belongs_to_project(
            self.command_line(project=self.project.parent / 'another'), self.project))
        external = Path(self.temp.name).parent / 'Kotavel'
        self.assertFalse(stop._belongs_to_project(
            self.command_line(executable=external), self.project))
        self.assertFalse(stop._belongs_to_project([], self.project))

    def test_finds_all_live_instances_and_skips_stale_or_malformed_descriptors(self):
        endpoint_a = self.endpoint(41, 40001)
        endpoint_b = self.endpoint(42, 40002)
        self.endpoint(43, 0)
        self.endpoint(44, 40004).write_text('not-a-port', encoding='ascii')
        stale = self.endpoint(45)
        outside = self.endpoint(46)
        command_lines = {
            41: self.command_line(), 42: self.command_line(),
            43: self.command_line(), 44: self.command_line(), 45: None,
            46: self.command_line(project=self.project.parent / 'other'),
        }
        found = stop._find_instances(self.project, None, FakePlatform(command_lines))
        self.assertEqual(found, [(endpoint_a, 41), (endpoint_b, 42)])
        self.assertTrue(stale.exists())
        self.assertTrue(outside.exists())

    def test_pid_option_selects_only_that_project_instance(self):
        first = self.endpoint(51)
        second = self.endpoint(52)
        platform = FakePlatform({51: self.command_line(), 52: self.command_line()})
        self.assertEqual(stop._find_instances(self.project, 52, platform), [(second, 52)])
        self.assertTrue(first.exists())

    def test_graceful_stop_removes_only_the_selected_endpoint(self):
        endpoint = self.endpoint(61)
        platform = FakePlatform({61: self.command_line()})
        with patch.object(stop.time, 'monotonic', return_value=0), \
             patch.object(stop.time, 'sleep'):
            forced = stop._stop_instance(endpoint, 61, self.project, platform)
        self.assertFalse(forced)
        self.assertEqual(platform.stop_requests, [61])
        self.assertEqual(platform.force_requests, [])
        self.assertFalse(endpoint.exists())

    def test_unresponsive_process_is_force_stopped_only_after_identity_recheck(self):
        endpoint = self.endpoint(71)
        platform = FakePlatform({71: self.command_line()})
        platform.process_is_running = lambda process_id: not platform.force_requests

        with patch.object(stop.time, 'monotonic', side_effect=[0, 11, 12, 13]), \
             patch.object(stop.time, 'sleep'):
            forced = stop._stop_instance(endpoint, 71, self.project, platform)
        self.assertTrue(forced)
        self.assertEqual(platform.stop_requests, [71])
        self.assertEqual(platform.force_requests, [71])
        self.assertFalse(endpoint.exists())

    def test_pid_reuse_during_grace_period_never_kills_another_process(self):
        endpoint = self.endpoint(81)
        other_binary = self.project.parent / 'Other'
        platform = FakePlatform({81: self.command_line(executable=other_binary)},
                                running=[True])

        with patch.object(stop.time, 'monotonic', side_effect=[0, 11]), \
             patch.object(stop.time, 'sleep'), \
             self.assertRaisesRegex(RuntimeError, 'no se cerró otro proceso'):
            stop._stop_instance(endpoint, 81, self.project, platform)
        self.assertEqual(platform.force_requests, [])
        self.assertTrue(endpoint.exists())

    def test_stop_without_pid_attempts_every_instance(self):
        self.endpoint(91)
        self.endpoint(92)
        platform = FakePlatform({91: self.command_line(), 92: self.command_line()})
        with patch.object(stop, 'get_platform', return_value=platform), \
             patch('sys.stdout'):
            self.assertEqual(stop.main(['--project', str(self.project)]), 0)
        self.assertEqual(platform.stop_requests, [91, 92])

    def test_stop_with_pid_does_not_touch_other_instances(self):
        self.endpoint(101)
        self.endpoint(102)
        platform = FakePlatform({101: self.command_line(), 102: self.command_line()})
        with patch.object(stop, 'get_platform', return_value=platform), \
             patch('sys.stdout'):
            self.assertEqual(stop.main(['--project', str(self.project), '--pid', '102']), 0)
        self.assertEqual(platform.stop_requests, [102])
        self.assertTrue((self.metrics / 'kotavel-101.endpoint').exists())


if __name__ == '__main__':
    unittest.main()
