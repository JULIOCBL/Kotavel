"""Tests for Windows-specific command and process adapter behavior."""
import ctypes
import os
import subprocess
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import Mock, call, patch
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[3]))
from tools.kotavel_cli.platforms import get_platform


@unittest.skipUnless(os.name == 'nt', 'Windows adapter tests run on the Windows CI job')
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
