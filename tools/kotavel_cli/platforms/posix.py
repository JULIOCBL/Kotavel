"""Shared implementation for POSIX development hosts."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import fcntl
from contextlib import contextmanager

from .base import PlatformAdapter


class PosixPlatform(PlatformAdapter):
    scripts_directory = 'bin'
    python_executable = 'python'
    conan_executable = 'conan'
    activation_kind = 'posix'

    def prepare(self, config, env, configure_only, *, tools, scripts, run_command,
                write_if_changed):
        if '.' in __package__:
            from ..posix_environment import prepare as prepare_posix_environment
        else:
            from posix_environment import prepare as prepare_posix_environment

        manifest = Path(tools) / 'platform.json'
        prepared = prepare_posix_environment(config['settings']['compiler'], env)
        write_if_changed(manifest, json.dumps(prepared))
        return prepared

    def prepend_tools_path(self, env, directory):
        directory = os.path.normpath(str(directory))
        entries = [entry for entry in env.get('PATH', '').split(os.pathsep)
                   if os.path.normpath(entry) != directory]
        env['PATH'] = os.pathsep.join([directory, *entries])

    def write_activation(self, prepared, env, tools, conan_home, write_if_changed):
        import shlex

        script = 'export PATH=' + shlex.quote(env['PATH']) + '\n'
        script += 'export CONAN_HOME=' + shlex.quote(str(conan_home)) + '\n'
        write_if_changed(Path(tools) / 'activate.sh', script)

    def resolve_native_command(self, command, tools):
        executable = Path(command[0])
        scripts = self.python_scripts(tools)
        if executable.parent != scripts or executable.name not in ('cmake', 'ctest'):
            return command
        candidates = list(dict.fromkeys(
            path.resolve()
            for library in ('lib', 'lib64')
            for path in (Path(tools) / 'python' / library).glob(
                f'python*/site-packages/cmake/data/bin/{executable.name}')))
        if len(candidates) == 1 and candidates[0].is_file() and os.access(candidates[0], os.X_OK):
            return [str(candidates[0]), *command[1:]]
        return command

    @contextmanager
    def lock_project(self, stream):
        fcntl.flock(stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        try:
            yield
        finally:
            fcntl.flock(stream.fileno(), fcntl.LOCK_UN)
