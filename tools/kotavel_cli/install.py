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
import shutil
import subprocess
import sys
import sysconfig
import venv

try:
    from .project_paths import conan_home
    from .conan_remote import configure_remote
    from .platforms import get_platform
except ImportError:  # install.py also runs directly as a preparation subprocess.
    from project_paths import conan_home
    from conan_remote import configure_remote
    from platforms import get_platform

# Load by file path as tests and other callers can import this script directly.
_posix_spec = importlib.util.spec_from_file_location('kotavel_posix', Path(__file__).with_name('posix_environment.py'))
_posix = importlib.util.module_from_spec(_posix_spec)
_posix_spec.loader.exec_module(_posix)

ROOT = Path.cwd()
PLATFORM = Path(__file__).resolve().parent / "platform"
DEFAULT_PROFILES = Path(__file__).resolve().parent / "templates" / "application" / "profiles"
TOOLS = ROOT / '.kotavel' / 'tools'
CONAN_HOME = conan_home(ROOT, get_platform().name)
SYSTEMS = {'Windows': ('windows', 'Windows'), 'Linux': ('linux', 'Linux'), 'Darwin': ('macos', 'Macos')}


def read_profile(system):
    return get_platform(system).read_profile(ROOT, DEFAULT_PROFILES)


def architecture(machine):
    return get_platform().architecture(machine)


def run(args, env):
    command = [str(arg) for arg in args]
    command[0] = shutil.which(command[0], path=env.get('PATH')) or command[0]
    # Bypass our own pip launchers for native executables when possible.
    command = get_platform().resolve_native_command(command, TOOLS)
    subprocess.run(command, cwd=ROOT, env=env, check=True)


def write_if_changed(path, content, encoding='utf-8'):
    if not path.is_file() or path.read_text(encoding=encoding) != content:
        path.write_text(content, encoding=encoding)


def prepend_tools_path(env, directory, system):
    """Keep activation repeatable without changing other PATH entries."""
    get_platform(system).prepend_tools_path(env, directory)


def installed_windows_platform(config, env):
    """Reuse an installed LLVM-MinGW; leave installation to PowerShell."""
    return get_platform('Windows').installed_toolchain(config, env, TOOLS)


def prepare_platform(system, config, env, configure_only):
    """Delegate operating-system preparation and read its environment manifest."""
    return get_platform(system).prepare(
        config, env, configure_only, tools=TOOLS, scripts=PLATFORM,
        run_command=run, write_if_changed=write_if_changed)


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
    adapter = get_platform('Windows' if prepared['activation_kind'] == 'powershell' else None)
    adapter.write_activation(prepared, env, TOOLS, CONAN_HOME, write_if_changed)


def success_banner(system, action):
    use_color = sys.stdout.isatty()
    if use_color:
        use_color = get_platform(system).configure_console()
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


def project_executable(build_type):
    """Lee el nombre que CMake resolvió desde la configuración del entorno."""
    manifest = ROOT / 'build' / build_type / 'kotavel-app-name.txt'
    if not manifest.is_file():
        raise RuntimeError(f'No se encontró el nombre de aplicación generado: {manifest}')
    name = manifest.read_text(encoding='utf-8').strip()
    if (not isinstance(name, str) or
            not re.fullmatch(r'[a-zA-Z0-9][a-zA-Z0-9_. -]*', name) or name.endswith(' ')):
        raise RuntimeError(f'{manifest}: APP_NAME debe ser un nombre de ejecutable, sin rutas.')
    return name


def project_executable_path(build_type, name, system):
    """Resuelve ejecutables en la ubicación estándar o en app/ del consumidor."""
    return get_platform(system).executable_path(ROOT, build_type, name)


def runtime_environment(env, build_type):
    """Add Conan's generated runtime DLL paths on Windows, leaving POSIX untouched."""
    return get_platform().runtime_environment(env, ROOT, build_type)


def run_application(executable, environment, deep_link, env, build_type='Debug'):
    """Ejecuta el consumidor con el entorno elegido y, opcionalmente, una URI desktop."""
    executable = Path(executable)
    executable = get_platform().executable_for_run(executable)
    command = [executable, '--env=' + environment, '--project-root=' + str(ROOT)]
    if deep_link:
        # DesktopKernel valida el esquema configurado y resuelve la ruta registrada.
        command.append(deep_link)
    run(command, runtime_environment(env, build_type))


def profile_label(path):
    try:
        return path.relative_to(ROOT).as_posix()
    except ValueError:
        return 'perfil predeterminado: ' + path.name


def has_framework_cpp_tests(framework_source):
    """Detecta la estructura agrupada mediante la suite base del núcleo."""
    return (framework_source / 'tests' / 'unit' / 'core' / 'core_tests.cpp').is_file()


def test_framework(framework_source, build_type, config, python, env):
    """Validate tools, consumer, runtime and integration; stop on first failure."""
    suites = (framework_source / 'tests', ROOT / 'tests')
    for suite in suites:
        if not any(suite.glob('test_*.py')):
            raise RuntimeError(f'No se encontraron pruebas Python en {suite}')
    for index, suite in enumerate(suites, 1):
        print(f'[{index}/4] Pruebas Python: {suite}', flush=True)
        run([python, '-m', 'unittest', 'discover', '-s', suite, '-p', 'test_*.py'], env)

    # Las dependencias del consumidor incluyen las del framework local. Resuélvelas
    # antes de configurar el núcleo para que CMake encuentre sus paquetes CMake.
    run(['conan', 'install', '.', '--build=missing'], env)

    print('[3/4] Compilación y pruebas del núcleo de Kotavel', flush=True)
    destination = ROOT / 'build' / 'framework-tests' / build_type
    generators = ROOT / 'build' / build_type / 'generators'
    compilers = json.loads(config['conf']['tools.build:compiler_executables'])
    run(['cmake', '-S', framework_source, '-B', destination,
         '-G', 'Ninja', '-DCMAKE_BUILD_TYPE=' + build_type,
         '-DCMAKE_CXX_COMPILER=' + compilers['cpp'],
         '-DCMAKE_PREFIX_PATH=' + str(generators),
         '-DKOTAVEL_BUILD_TESTS=ON', '-DPython3_EXECUTABLE=' + str(python)], env)
    run(['cmake', '--build', destination, '--config', build_type], env)
    run(['ctest', '--test-dir', destination, '-C', build_type,
         '--output-on-failure', '--no-tests=error'], env)

    print('[4/4] Compilación y pruebas de integración del consumidor', flush=True)
    preset = 'conan-' + build_type.lower()
    for command in (['cmake', '--preset', preset, '-DBUILD_TESTING=ON'],
                    ['cmake', '--build', '--preset', preset],
                    ['ctest', '--preset', preset, '--output-on-failure', '--no-tests=error']):
        run(command, env)


def main(argv=None):
    global ROOT, TOOLS, CONAN_HOME
    if sys.version_info < (3, 9):
        raise RuntimeError('Se necesita Python 3.9 o posterior.')
    parser = argparse.ArgumentParser(prog='python3 kotavel.py', description=__doc__)
    parser.add_argument('--project', type=Path, help='Raíz del proyecto consumidor')
    parser.add_argument('--test', action='store_true', help='Ejecuta CTest sobre una compilación existente')
    parser.add_argument('--framework', action='store_true', help='Comprueba herramientas Python, núcleo e integración del consumidor local (con --test)')
    parser.add_argument('--build-type', choices=('Debug', 'Release'))
    parser.add_argument('--build', action='store_true')
    parser.add_argument('--env', choices=('local', 'development', 'production'), help='Archivo de entorno al ejecutar run')
    parser.add_argument('--deep-link', help='Abre una ruta desktop desde una URI, por ejemplo kotavel://pos/home')
    parser.add_argument('--run', action='store_true', help='Compila, prueba y ejecuta el consumidor')
    parser.add_argument('--shell', action='store_true', help='Entra en una terminal con las herramientas activas')
    parser.add_argument('--configure-only', action='store_true', help='Reutiliza las herramientas existentes sin descargar nada')
    args = parser.parse_args(argv)
    if args.env and not (args.run or args.build):
        parser.error('--env solo se utiliza con run y build')
    if args.deep_link and not args.run:
        parser.error('--deep-link solo se utiliza con run')
    if args.framework and (not args.test or args.build or args.run or args.shell):
        parser.error('--framework solo se utiliza con el comando test')
    if args.project is not None:
        ROOT = args.project.resolve()
        TOOLS = ROOT / '.kotavel' / 'tools'
        CONAN_HOME = conan_home(ROOT, get_platform().name)
    if not (ROOT / 'conanfile.py').is_file():
        raise RuntimeError(f'No se encontró conanfile.py en {ROOT}')
    framework_source = None
    if args.framework:
        settings = json.loads((ROOT / 'kotavel.json').read_text(encoding='utf-8-sig'))
        if settings.get('mode') != 'local':
            raise RuntimeError('Las pruebas del framework requieren mode local en kotavel.json.')
        source_dir = settings.get('source_dir')
        if not isinstance(source_dir, str) or not source_dir.strip():
            raise RuntimeError('kotavel.json requiere source_dir para probar el framework local.')
        framework_source = (ROOT / source_dir).resolve()
        if not has_framework_cpp_tests(framework_source):
            raise RuntimeError(f'No se encontraron las pruebas de Kotavel en {framework_source}')
    if args.shell and not sys.stdin.isatty():
        raise RuntimeError('--shell requiere una terminal interactiva; en CI utiliza --build.')
    adapter = get_platform()
    system = adapter.name
    profile_path, config = read_profile(system)
    config['settings']['arch'] = architecture(adapter.compiler_machine())
    if args.build_type:
        config['settings']['build_type'] = args.build_type
    if config['settings']['compiler'] == 'msvc':
        config['settings']['compiler.runtime_type'] = config['settings']['build_type']
    build_type = config['settings']['build_type']
    TOOLS.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env['CONAN_HOME'] = str(CONAN_HOME)
    prepared = prepare_platform(system, config, env, args.configure_only)
    env.update(prepared.get('build_environment', {}))
    scripts = TOOLS / 'python' / prepared['scripts_directory']
    python = scripts / prepared['python_executable']
    prepend_tools_path(env, scripts, system)
    if not args.configure_only:
        if not python.is_file():
            venv.EnvBuilder(with_pip=True).create(TOOLS / 'python')
        if not all(shutil.which(tool, path=str(scripts)) for tool in ('conan', 'cmake', 'ninja')):
            run([python, '-m', 'pip', 'install', '--disable-pip-version-check', 'conan>=2,<3', 'cmake>=3.25', 'ninja'], env)
    if not all(shutil.which(tool, path=str(scripts)) for tool in ('conan', 'cmake', 'ninja')):
        raise RuntimeError('Faltan las herramientas locales; ejecuta la preparación sin --configure-only.')
    if not args.configure_only:
        configure_remote(shutil.which('conan', path=str(scripts)), env, run)
    compiler_environment(config, env, prepared.get('compiler_executables'))
    destination = CONAN_HOME / 'profiles' / 'default'
    destination.parent.mkdir(parents=True, exist_ok=True)
    target = io.StringIO()
    target.write(f'# Generado desde {profile_label(profile_path)}\n')
    config.write(target, space_around_delimiters=False)
    write_if_changed(destination, target.getvalue())
    write_activation(prepared, env)
    print(f'Perfil leído: {profile_label(profile_path)}', flush=True)
    print(f'Entorno preparado: {system} / {config["settings"]["arch"]} / {config["settings"]["compiler"]} {config["settings"]["compiler.version"]} / C++{config["settings"]["compiler.cppstd"]} / {build_type}', flush=True)
    if not args.shell and not args.build and not args.run and not args.test:
        print('Activa esta terminal: ' + get_platform(system).activation_command(), flush=True)
        print('Después: conan install . --build=missing', flush=True)
    if args.build or args.run:
        preset = 'conan-' + build_type.lower()
        environment = args.env or 'production'
        for command in ([ 'conan', 'install', '.', '--build=missing'],
                        ['cmake', '--preset', preset, '-DKOTAVEL_APP_ENV=' + environment],
                        ['cmake', '--build', '--preset', preset],
                        ['ctest', '--preset', preset, '--output-on-failure']):
            run(command, env)
        if args.run:
            name = project_executable(build_type)
            executable = project_executable_path(build_type, name, system)
            if not executable.exists():
                raise RuntimeError(f'No se encontró el ejecutable generado: {executable}')
            run_application(executable, environment, args.deep_link, env, build_type)
    if args.test and not args.build and not args.run:
        if framework_source is not None:
            test_framework(framework_source, build_type, config, python, env)
        else:
            run(['ctest', '--preset', 'conan-' + build_type.lower(), '--output-on-failure'], env)
    success_banner(system, 'run' if args.run else 'build' if args.build else 'test' if args.test else 'prepare')
    if args.shell:
        enter_shell(prepared, env)


if __name__ == '__main__':
    try:
        main()
    except subprocess.CalledProcessError as error:
        print(f'ERROR: El proceso terminó con código {error.returncode}. Revisa el diagnóstico anterior.', file=sys.stderr)
        sys.exit(1)
    except (RuntimeError, OSError, configparser.Error) as error:
        print(f'ERROR: {error}', file=sys.stderr)
        sys.exit(1)
