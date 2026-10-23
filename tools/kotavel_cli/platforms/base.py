"""Shared contract for command-line platform adapters."""
from pathlib import Path


class PlatformAdapter:
    name = ''
    profile_name = ''
    conan_os = ''
    scripts_directory = ''
    python_executable = ''
    conan_executable = ''
    activation_kind = ''

    def profile_path(self, root, defaults):
        project = Path(root) / 'profiles' / self.profile_name
        return project if project.is_file() else Path(defaults) / self.profile_name

    def python_scripts(self, tools):
        return Path(tools) / 'python' / self.scripts_directory

    def python_path(self, tools):
        return self.python_scripts(tools) / self.python_executable

    def conan_path(self, tools):
        return self.python_scripts(tools) / self.conan_executable

    def architecture(self, machine):
        aliases = {
            'amd64': 'x86_64', 'x86_64': 'x86_64',
            'arm64': 'armv8', 'aarch64': 'armv8',
        }
        try:
            return aliases[machine.lower()]
        except KeyError as error:
            raise RuntimeError(f'Arquitectura no soportada: {machine}') from error

    def host_machine(self):
        import platform
        return platform.machine()

    def compiler_machine(self):
        return self.host_machine()

    def read_profile(self, root, defaults):
        import configparser
        path = self.profile_path(root, defaults)
        config = configparser.ConfigParser(interpolation=None, delimiters=('=',))
        config.optionxform = str
        with path.open(encoding='utf-8-sig') as source:
            config.read_file(source)
        if config['settings']['os'] != self.conan_os:
            raise RuntimeError(f'El sistema de {path} no coincide con {self.name}')
        return path, config

    def conan_home(self, project_root, user_home, override):
        if override:
            return Path(override).expanduser().resolve()
        legacy = Path(project_root).resolve() / '.kotavel' / 'conan'
        if legacy.is_dir():
            return legacy
        return Path(user_home) / '.kotavel' / 'conan'

    def prepare(self, config, env, configure_only, *, tools, scripts, run_command,
                write_if_changed):
        raise NotImplementedError

    def prepend_tools_path(self, env, directory):
        raise NotImplementedError

    def write_activation(self, prepared, env, tools, conan_home, write_if_changed):
        raise NotImplementedError

    def resolve_native_command(self, command, tools):
        return command

    def configure_console(self):
        return False

    def executable_path(self, root, build_type, name):
        build = Path(root) / 'build' / build_type
        candidates = (build / 'app' / name, build / name)
        return next((path for path in candidates if path.is_file()), candidates[0])

    def runtime_environment(self, env, root, build_type):
        return dict(env)

    def executable_for_run(self, executable):
        return Path(executable)

    def process_command_line(self, process_id):
        raise NotImplementedError

    def request_process_stop(self, process_id):
        raise NotImplementedError

    def force_process_stop(self, process_id):
        raise NotImplementedError

    def process_is_running(self, process_id):
        raise NotImplementedError

    def run_application(self, command, *, cwd, env):
        import subprocess
        subprocess.run(command, cwd=cwd, env=env, check=True)

    def lock_project(self, stream):
        raise NotImplementedError

    def write_if_changed(self, path, content, encoding='utf-8'):
        path = Path(path)
        if not path.is_file() or path.read_text(encoding=encoding) != content:
            path.write_text(content, encoding=encoding)

    def activation_command(self):
        return 'source .kotavel/tools/activate.sh'

    def configure_console(self):
        return True

    def package_supported(self):
        return False

    def require_packager(self):
        raise RuntimeError('El empaquetado de aplicaciones no está disponible en este sistema.')
