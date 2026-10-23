"""Windows implementation of the development command platform contract."""
import ctypes
import json
import ntpath
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import sysconfig
from contextlib import contextmanager

from .base import PlatformAdapter


class WindowsPlatform(PlatformAdapter):
    name = 'Windows'
    profile_name = 'windows'
    conan_os = 'Windows'
    scripts_directory = 'Scripts'
    python_executable = 'python.exe'
    conan_executable = 'conan.exe'
    activation_kind = 'powershell'

    def process_command_line(self, process_id):
        shell = shutil.which('powershell') or shutil.which('pwsh')
        if not shell:
            raise RuntimeError('Se necesita PowerShell para identificar la instancia de Kotavel.')
        command = ("$p = Get-CimInstance Win32_Process -Filter 'ProcessId = " +
                   str(process_id) + "'; if ($p) { $p.CommandLine }")
        result = subprocess.run([shell, '-NoProfile', '-NonInteractive', '-Command', command],
                                capture_output=True, text=True, check=False)
        if result.returncode != 0 or not result.stdout.strip():
            return None
        import shlex
        return shlex.split(result.stdout.strip(), posix=False)

    def request_process_stop(self, process_id):
        helper = r'''import ctypes, sys, time
k = ctypes.windll.kernel32
pid = int(sys.argv[1])
k.SetConsoleCtrlHandler(None, True)
if not k.AttachConsole(pid): raise SystemExit(2)
if not k.GenerateConsoleCtrlEvent(1, pid): raise SystemExit(3)
time.sleep(0.2)
k.FreeConsole()
'''
        result = subprocess.run([sys.executable, '-c', helper, str(process_id)],
                                creationflags=subprocess.CREATE_NO_WINDOW,
                                capture_output=True, text=True, check=False)
        if result.returncode != 0:
            raise RuntimeError('No se pudo enviar el cierre ordenado a la consola de Kotavel.')

    def force_process_stop(self, process_id):
        kernel = ctypes.windll.kernel32
        kernel.OpenProcess.argtypes = [ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
        kernel.OpenProcess.restype = ctypes.c_void_p
        kernel.TerminateProcess.argtypes = [ctypes.c_void_p, ctypes.c_uint]
        kernel.TerminateProcess.restype = ctypes.c_int
        kernel.CloseHandle.argtypes = [ctypes.c_void_p]
        handle = kernel.OpenProcess(0x0001, False, process_id)  # PROCESS_TERMINATE
        if not handle:
            if kernel.GetLastError() == 87:
                return
            raise RuntimeError('No se pudo abrir el proceso de Kotavel para cerrarlo.')
        try:
            if not kernel.TerminateProcess(handle, 1):
                raise RuntimeError('Windows no pudo terminar el proceso de Kotavel.')
        finally:
            kernel.CloseHandle(handle)

    def process_is_running(self, process_id):
        kernel = ctypes.windll.kernel32
        kernel.OpenProcess.argtypes = [ctypes.c_ulong, ctypes.c_int, ctypes.c_ulong]
        kernel.OpenProcess.restype = ctypes.c_void_p
        kernel.WaitForSingleObject.argtypes = [ctypes.c_void_p, ctypes.c_ulong]
        kernel.WaitForSingleObject.restype = ctypes.c_ulong
        kernel.CloseHandle.argtypes = [ctypes.c_void_p]
        handle = kernel.OpenProcess(0x00100000, False, process_id)  # SYNCHRONIZE
        if not handle:
            return kernel.GetLastError() != 87  # ERROR_INVALID_PARAMETER means exited.
        try:
            return kernel.WaitForSingleObject(handle, 0) != 0
        finally:
            kernel.CloseHandle(handle)

    def run_application(self, command, *, cwd, env):
        process = subprocess.Popen(command, cwd=cwd, env=env,
                                   creationflags=subprocess.CREATE_NEW_PROCESS_GROUP)
        try:
            result = process.wait()
        except KeyboardInterrupt:
            self.request_process_stop(process.pid)
            process.wait(timeout=10)
            raise
        if result != 0:
            raise subprocess.CalledProcessError(result, command)

    def compiler_machine(self):
        return sysconfig.get_platform().removeprefix('win-')

    def architecture(self, machine=None):
        machine = machine or self.compiler_machine()
        return super().architecture(machine)

    def conan_home(self, project_root, user_home, override):
        if override:
            return Path(override).expanduser().resolve()
        return Path(user_home) / '.k'

    def prepend_tools_path(self, env, directory):
        directory = str(directory)
        key = ntpath.normcase(ntpath.normpath(directory))
        entries = [entry for entry in env.get('PATH', '').split(';')
                   if ntpath.normcase(ntpath.normpath(entry.strip('"'))) != key]
        env['PATH'] = ';'.join([directory, *entries])

    def prepare(self, config, env, configure_only, *, tools, scripts, run_command,
                write_if_changed):
        manifest = Path(tools) / 'platform.json'
        prepared = self.installed_toolchain(config, env, tools)
        if prepared is not None:
            write_if_changed(manifest, json.dumps(prepared), encoding='utf-8-sig')
            self.prepend_tools_path(env, prepared['compiler_bin'])
            return prepared

        manifest.unlink(missing_ok=True)
        shell = shutil.which('powershell', path=env.get('PATH')) or shutil.which('pwsh', path=env.get('PATH'))
        if not shell:
            raise RuntimeError('Se necesita PowerShell para preparar Windows.')
        command = [shell, '-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
                   Path(scripts) / 'install-windows.ps1', '-Tools', tools,
                   '-Compiler', config['settings']['compiler'],
                   '-Architecture', config['settings']['arch'], '-Manifest', manifest]
        if configure_only:
            command.append('-ConfigureOnly')
        run_command(command, env)
        with manifest.open(encoding='utf-8-sig') as source:
            prepared = json.load(source)
        if prepared['compiler_bin']:
            self.prepend_tools_path(env, prepared['compiler_bin'])
        return prepared

    @staticmethod
    def installed_toolchain(config, env, tools):
        if config['settings']['compiler'] != 'clang':
            return None
        if config['settings']['arch'] != 'x86_64':
            raise RuntimeError('LLVM-MinGW portable requiere Python x64.')
        candidates = sorted(Path(tools).glob('llvm-mingw-*-ucrt-x86_64'),
                            key=lambda path: path.name.lower(), reverse=True)
        toolchain = next((path for path in candidates if path.is_dir() and
                          (path / 'bin' / 'clang++.exe').is_file()), None)
        if toolchain is None or not (toolchain / 'bin' / 'clang.exe').is_file():
            return None
        shell = shutil.which('pwsh', path=env.get('PATH')) or shutil.which('powershell', path=env.get('PATH'))
        if not shell:
            raise RuntimeError('Se necesita PowerShell para preparar Windows.')
        return {
            'scripts_directory': 'Scripts', 'python_executable': 'python.exe',
            'compiler_bin': str(toolchain / 'bin'),
            'shell': [shell, '-NoLogo', '-NoProfile'], 'activation_kind': 'powershell',
            'build_environment': {'CONAN_OPENSSL_CONFIGURATION': 'mingw64'},
        }

    def write_activation(self, prepared, env, tools, conan_home, write_if_changed):
        quote = lambda value: "'" + value.replace("'", "''") + "'"
        script = '$env:PATH = ' + quote(env['PATH']) + '\n'
        script += '$env:CONAN_HOME = ' + quote(str(conan_home)) + '\n'
        for name, value in prepared.get('build_environment', {}).items():
            script += '$env:' + name + ' = ' + quote(value) + '\n'
        write_if_changed(Path(tools) / 'activate.ps1', script, encoding='utf-8-sig')

    def resolve_native_command(self, command, tools):
        executable = Path(command[0])
        scripts = self.python_scripts(tools)
        if executable.parent != scripts or executable.name.lower() not in ('cmake.exe', 'ctest.exe'):
            return command
        native = (Path(tools) / 'python' / 'Lib' / 'site-packages' / 'cmake' /
                  'data' / 'bin' / executable.name)
        if native.is_file():
            return [str(native), *command[1:]]
        return command

    def configure_console(self):
        handle = ctypes.windll.kernel32.GetStdHandle(-11)
        mode = ctypes.c_ulong()
        return bool(ctypes.windll.kernel32.GetConsoleMode(handle, ctypes.byref(mode)) and
                    ctypes.windll.kernel32.SetConsoleMode(handle, mode.value | 4))

    def activation_command(self):
        return '. .\\.kotavel\\tools\\activate.ps1'

    def executable_path(self, root, build_type, name):
        return super().executable_path(root, build_type, name + '.exe')

    def runtime_environment(self, env, root, build_type):
        runtime_env = dict(env)
        generators = Path(root) / 'build' / build_type / 'generators'
        scripts = sorted(generators.glob('conanrunenv-*.bat'))
        if not scripts:
            return runtime_env
        path_updated = False
        for line in scripts[0].read_text(encoding='utf-8-sig').splitlines():
            match = re.fullmatch(r'\s*set\s+"(QT_PLUGIN_PATH|OPENSSL_MODULES|PATH)=(.*)"\s*', line, re.IGNORECASE)
            if not match:
                continue
            name, value = match.group(1).upper(), match.group(2)
            if name == 'PATH':
                if path_updated:
                    continue
                value = re.sub(r'%PATH%', lambda _: runtime_env.get('PATH', ''), value, flags=re.IGNORECASE)
                runtime_env['PATH'] = value
                path_updated = True
            else:
                runtime_env[name] = value
        return runtime_env

    @contextmanager
    def lock_project(self, stream):
        import msvcrt

        if stream.seek(0, os.SEEK_END) == 0:
            stream.write(b'\0')
            stream.flush()
        stream.seek(0)
        msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
        try:
            yield
        finally:
            stream.seek(0)
            msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)

    def require_packager(self):
        raise RuntimeError('El empaquetado .app/.dmg/.pkg solo está disponible en macOS.')
