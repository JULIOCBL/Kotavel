"""Prepare the local environment using the matching versioned Conan profile."""
import argparse
import configparser
import importlib.util
import io
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import subprocess
import sys
import sysconfig
import venv

# Load by file path as tests and other callers can import this script directly.
_posix_spec = importlib.util.spec_from_file_location('kotavel_posix', Path(__file__).with_name('posix_environment.py'))
_posix = importlib.util.module_from_spec(_posix_spec)
_posix_spec.loader.exec_module(_posix)

ROOT = Path.cwd()
PLATFORM = Path(__file__).resolve().parent / "platform"
DEFAULT_PROFILES = Path(__file__).resolve().parent / "templates" / "application" / "profiles"
TOOLS = ROOT / '.kotavel' / 'tools'
CONAN_HOME = ROOT / '.kotavel' / 'conan'
SYSTEMS = {'Windows': ('windows', 'Windows'), 'Linux': ('linux', 'Linux'), 'Darwin': ('macos', 'Macos')}


def read_profile(system):
    if system not in SYSTEMS:
        raise RuntimeError(f'Sistema no soportado: {system}')
    name, conan_os = SYSTEMS[system]
    path = ROOT / 'profiles' / name
    if not path.is_file():
        path = DEFAULT_PROFILES / name
    config = configparser.ConfigParser(interpolation=None, delimiters=('=',))
    config.optionxform = str
    with path.open(encoding='utf-8-sig') as source:
        config.read_file(source)
    if config['settings']['os'] != conan_os:
        raise RuntimeError(f'El sistema de {path} no coincide con {system}')
    return path, config


def architecture(machine):
    aliases = {'amd64': 'x86_64', 'x86_64': 'x86_64', 'arm64': 'armv8', 'aarch64': 'armv8'}
    if machine.lower() not in aliases:
        raise RuntimeError(f'Arquitectura no soportada: {machine}')
    return aliases[machine.lower()]


def run(args, env):
    command = [str(arg) for arg in args]
    command[0] = shutil.which(command[0], path=env.get('PATH')) or command[0]
    # The pip launchers start Python just to invoke these native executables.
    # Only bypass our own launchers, preserving external tool selection.
    if platform.system() == 'Windows':
        executable = Path(command[0])
        scripts = TOOLS / 'python' / 'Scripts'
        if executable.parent == scripts and executable.name.lower() in ('cmake.exe', 'ctest.exe'):
            native = TOOLS / 'python' / 'Lib' / 'site-packages' / 'cmake' / 'data' / 'bin' / executable.name
            if native.is_file():
                command[0] = str(native)
    elif platform.system() in ('Linux', 'Darwin'):
        executable = Path(command[0])
        if executable.parent == TOOLS / 'python' / 'bin' and executable.name in ('cmake', 'ctest'):
            candidates = list((TOOLS / 'python' / 'lib').glob(
                f'python*/site-packages/cmake/data/bin/{executable.name}'))
            # An ambiguous or incomplete installation keeps the original launcher.
            if len(candidates) == 1 and candidates[0].is_file() and os.access(candidates[0], os.X_OK):
                command[0] = str(candidates[0])
    subprocess.run(command, cwd=ROOT, env=env, check=True)


def write_if_changed(path, content, encoding='utf-8'):
    if not path.is_file() or path.read_text(encoding=encoding) != content:
        path.write_text(content, encoding=encoding)


def installed_windows_platform(config, env):
    """Reuse an installed LLVM-MinGW; leave installation to PowerShell."""
    if config['settings']['compiler'] != 'clang':
        return None
    if config['settings']['arch'] != 'x86_64':
        raise RuntimeError('LLVM-MinGW portable requiere Python x64.')
    candidates = sorted(TOOLS.glob('llvm-mingw-*-ucrt-x86_64'), key=lambda path: path.name.lower(), reverse=True)
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
    }


def prepare_platform(system, config, env, configure_only):
    """Delegate operating-system preparation and read its environment manifest."""
    manifest = TOOLS / 'platform.json'
    if system in ('Linux', 'Darwin'):
        prepared = _posix.prepare(config['settings']['compiler'], env)
        write_if_changed(manifest, json.dumps(prepared))
        return prepared
    if system == 'Windows':
        prepared = installed_windows_platform(config, env)
        if prepared is not None:
            write_if_changed(manifest, json.dumps(prepared), encoding='utf-8-sig')
            env['PATH'] = prepared['compiler_bin'] + os.pathsep + env.get('PATH', '')
            return prepared
    manifest.unlink(missing_ok=True)
    if system == 'Windows':
        shell = shutil.which('powershell', path=env.get('PATH')) or shutil.which('pwsh', path=env.get('PATH'))
        if not shell:
            raise RuntimeError('Se necesita PowerShell para preparar Windows.')
        command = [shell, '-NoLogo', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
                   PLATFORM / 'install-windows.ps1', '-Tools', TOOLS,
                   '-Compiler', config['settings']['compiler'],
                   '-Architecture', config['settings']['arch'], '-Manifest', manifest]
        if configure_only:
            command.append('-ConfigureOnly')
    else:
        name = SYSTEMS[system][0]
        command = ['sh', PLATFORM / f'install-{name}.sh',
                   sys.executable, manifest, config['settings']['compiler']]
    run(command, env)
    with manifest.open(encoding='utf-8-sig') as source:
        prepared = json.load(source)
    if prepared['compiler_bin']:
        env['PATH'] = prepared['compiler_bin'] + os.pathsep + env.get('PATH', '')
    return prepared


def compiler_environment(config, env, executables=None):
    compiler = config['settings']['compiler']
    names = {'clang': ('clang', 'clang++'), 'apple-clang': ('clang', 'clang++'), 'gcc': ('gcc', 'g++'), 'msvc': ('cl', 'cl')}
    if compiler not in names:
        raise RuntimeError(f'Compilador no soportado por el instalador: {compiler}')
    c_name, cpp_name = names[compiler]
    c = executables['c'] if executables else shutil.which(c_name, path=env['PATH'])
    cpp = executables['cpp'] if executables else shutil.which(cpp_name, path=env['PATH'])
    if not c or not cpp:
        raise RuntimeError(f'El perfil requiere {compiler}. Instala su compilador y actívalo en la terminal antes de continuar.')
    if compiler == 'msvc':
        result = subprocess.run([cpp], env=env, capture_output=True, text=True)
        output = result.stdout + result.stderr
        match = re.search(r'(?:Version|Versión)\s+(\d+)\.(\d+)', output, re.IGNORECASE)
        version = match.group(1) + match.group(2)[0] if match else None
    elif compiler == 'gcc':
        output = subprocess.check_output([cpp, '-dumpfullversion'], env=env, text=True)
        version = output.strip().split('.')[0]
    else:
        output = subprocess.run([cpp, '--version'], env=env, capture_output=True, text=True).stdout
        match = re.search(r'(?:Apple clang|clang) version (\d+)', output)
        version = match.group(1) if match else None
        if compiler == 'apple-clang' and 'Apple clang' not in output:
            raise RuntimeError('El perfil apple-clang requiere el compilador de Apple.')
    if not version:
        raise RuntimeError(f'No se pudo detectar la versión de {compiler}.')
    config['settings']['compiler.version'] = version
    if not config.has_section('conf'):
        config.add_section('conf')
    config['conf']['tools.build:compiler_executables'] = json.dumps({'c': c.replace('\\', '/'), 'cpp': cpp.replace('\\', '/')})


def write_activation(prepared, env):
    # Python cannot change its parent shell: emit a file to load in that shell.
    if prepared['activation_kind'] == 'powershell':
        quote = lambda value: "'" + value.replace("'", "''") + "'"
        script = '$env:PATH = ' + quote(env['PATH']) + '\n$env:CONAN_HOME = ' + quote(str(CONAN_HOME)) + '\n'
        write_if_changed(TOOLS / 'activate.ps1', script, encoding='utf-8-sig')
    else:
        script = 'export PATH=' + shlex.quote(env['PATH']) + '\nexport CONAN_HOME=' + shlex.quote(str(CONAN_HOME)) + '\n'
        if platform.system() in ('Linux', 'Darwin'):
            write_if_changed(TOOLS / 'activate.sh', script)
        else:
            (TOOLS / 'activate.sh').write_text(script, encoding='utf-8')


def success_banner(system, action):
    use_color = sys.stdout.isatty()
    if use_color and system == 'Windows':
        import ctypes
        handle = ctypes.windll.kernel32.GetStdHandle(-11)
        mode = ctypes.c_ulong()
        use_color = bool(ctypes.windll.kernel32.GetConsoleMode(handle, ctypes.byref(mode)) and ctypes.windll.kernel32.SetConsoleMode(handle, mode.value | 4))
    def line(text, color):
        print(f'\033[{color}m{text}\033[0m' if use_color else text, flush=True)
    print('', flush=True)
    line('  ====================================================', '36')
    line('   KOTAVEL', '36')
    line('   Preparación completada correctamente', '32')
    line('  ====================================================', '36')
    if action == 'run':
        line('  Aplicación ejecutada correctamente', '32')
    elif action == 'build':
        line('  Compilación y pruebas completadas correctamente', '32')
    elif action == 'test':
        line('  Pruebas completadas correctamente', '32')
    else:
        line('  Siguiente comando: conan install . --build=missing', '33')
    print('', flush=True)

def enter_shell(prepared, env):
    if not sys.stdin.isatty():
        raise RuntimeError('--shell requiere una terminal interactiva; en CI utiliza --build.')
    print('Entorno activo. Ejecuta: conan install . --build=missing', flush=True)
    print('Escribe exit para volver a la terminal anterior.', flush=True)
    run(prepared['shell'], env)


def project_executable():
    config = json.loads((ROOT / 'kotavel.json').read_text(encoding='utf-8-sig'))
    name = config.get('executable', ROOT.name)
    if not isinstance(name, str) or not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9_.-]*', name):
        raise RuntimeError('kotavel.json: executable debe ser un nombre de ejecutable, sin rutas.')
    return name


def profile_label(path):
    try:
        return path.relative_to(ROOT).as_posix()
    except ValueError:
        return 'perfil predeterminado: ' + path.name


def main(argv=None):
    global ROOT, TOOLS, CONAN_HOME
    if sys.version_info < (3, 9):
        raise RuntimeError('Se necesita Python 3.9 o posterior.')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project', type=Path, help='Raíz del proyecto consumidor')
    parser.add_argument('--test', action='store_true', help='Ejecuta CTest sobre una compilación existente')
    parser.add_argument('--build-type', choices=('Debug', 'Release'))
    parser.add_argument('--build', action='store_true')
    parser.add_argument('--run', action='store_true', help='Compila, prueba y ejecuta el consumidor')
    parser.add_argument('--shell', action='store_true', help='Entra en una terminal con las herramientas activas')
    parser.add_argument('--configure-only', action='store_true', help='Reutiliza las herramientas existentes sin descargar nada')
    args = parser.parse_args(argv)
    if args.project is not None:
        ROOT = args.project.resolve()
        TOOLS = ROOT / '.kotavel' / 'tools'
        CONAN_HOME = ROOT / '.kotavel' / 'conan'
    if not (ROOT / 'conanfile.py').is_file():
        raise RuntimeError(f'No se encontró conanfile.py en {ROOT}')
    if args.shell and not sys.stdin.isatty():
        raise RuntimeError('--shell requiere una terminal interactiva; en CI utiliza --build.')
    system = platform.system()
    profile_path, config = read_profile(system)
    machine = sysconfig.get_platform().removeprefix('win-') if system == 'Windows' else platform.machine()
    config['settings']['arch'] = architecture(machine)
    if args.build_type:
        config['settings']['build_type'] = args.build_type
    if config['settings']['compiler'] == 'msvc':
        config['settings']['compiler.runtime_type'] = config['settings']['build_type']
    build_type = config['settings']['build_type']
    TOOLS.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env['CONAN_HOME'] = str(CONAN_HOME)
    prepared = prepare_platform(system, config, env, args.configure_only)
    scripts = TOOLS / 'python' / prepared['scripts_directory']
    python = scripts / prepared['python_executable']
    env['PATH'] = str(scripts) + os.pathsep + env.get('PATH', '')
    if not args.configure_only:
        if not python.is_file():
            venv.EnvBuilder(with_pip=True).create(TOOLS / 'python')
        if not all(shutil.which(tool, path=str(scripts)) for tool in ('conan', 'cmake', 'ninja')):
            run([python, '-m', 'pip', 'install', '--disable-pip-version-check', 'conan>=2,<3', 'cmake>=3.25', 'ninja'], env)
    if not all(shutil.which(tool, path=str(scripts)) for tool in ('conan', 'cmake', 'ninja')):
        raise RuntimeError('Faltan las herramientas locales; ejecuta la preparación sin --configure-only.')
    compiler_environment(config, env, prepared.get('compiler_executables'))
    destination = CONAN_HOME / 'profiles' / 'default'
    destination.parent.mkdir(parents=True, exist_ok=True)
    target = io.StringIO()
    target.write(f'# Generado desde {profile_label(profile_path)}\n')
    config.write(target, space_around_delimiters=False)
    if system in ('Windows', 'Linux', 'Darwin'):
        write_if_changed(destination, target.getvalue())
    else:
        destination.write_text(target.getvalue(), encoding='utf-8')
    write_activation(prepared, env)
    print(f'Perfil leído: {profile_label(profile_path)}', flush=True)
    print(f'Entorno preparado: {system} / {config["settings"]["arch"]} / {config["settings"]["compiler"]} {config["settings"]["compiler.version"]} / C++{config["settings"]["compiler.cppstd"]} / {build_type}', flush=True)
    if not args.shell and not args.build and not args.run and not args.test:
        if system == 'Windows':
            print('Activa esta terminal: . .\\.kotavel\\tools\\activate.ps1', flush=True)
        else:
            print('Activa esta terminal: source .kotavel/tools/activate.sh', flush=True)
        print('Después: conan install . --build=missing', flush=True)
    if args.build or args.run:
        preset = 'conan-' + build_type.lower()
        for command in ([ 'conan', 'install', '.', '--build=missing'], ['cmake', '--preset', preset], ['cmake', '--build', '--preset', preset], ['ctest', '--preset', preset, '--output-on-failure']):
            run(command, env)
        if args.run:
            name = project_executable()
            executable = ROOT / 'build' / build_type / (name + '.exe' if system == 'Windows' else name)
            if not executable.is_file():
                raise RuntimeError(f'No se encontró el ejecutable generado: {executable}')
            run([executable], env)
    if args.test and not args.build and not args.run:
        run(['ctest', '--preset', 'conan-' + build_type.lower(), '--output-on-failure'], env)
    success_banner(system, 'run' if args.run else 'build' if args.build else 'test' if args.test else 'prepare')
    if args.shell:
        enter_shell(prepared, env)


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, OSError, subprocess.CalledProcessError, configparser.Error) as error:
        print(f'ERROR: {error}', file=sys.stderr)
        sys.exit(1)
