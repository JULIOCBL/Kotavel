"""Instala dependencias y sincroniza conan.lock; add añade una biblioteca sin duplicarla."""
import argparse
import ast
from contextlib import contextmanager
import json
import os
from pathlib import Path
import re
import runpy
import subprocess
import sys
import tempfile

ROOT = Path.cwd()
TOOLS = ROOT / '.kotavel' / 'tools'
NAME = r'[a-z0-9][a-z0-9_.+\-]*'
VERSION = r'[a-zA-Z0-9][a-zA-Z0-9_.+\-]*'


def dependency_name(reference):
    name = reference.split('/', 1)[0]
    if not re.fullmatch(NAME, name):
        raise RuntimeError('Utiliza un nombre Conan válido, por ejemplo fmt/11.2.0.')
    return name


def validate_reference(reference):
    if not re.fullmatch(rf'{NAME}(?:/{VERSION})?', reference):
        raise RuntimeError('add acepta nombre o nombre/versión; configura rangos, usuarios y canales en conanfile.py.')
    return dependency_name(reference)


def declared_dependencies(source):
    """Inspect literal declarations in the recipe, including conditional calls."""
    tree = ast.parse(source)
    base_names = {'ConanFile'}
    for node in tree.body:
        if isinstance(node, ast.ImportFrom) and node.module == 'conan':
            base_names.update(alias.asname or alias.name for alias in node.names if alias.name == 'ConanFile')
    recipes = [node for node in tree.body if isinstance(node, ast.ClassDef) and any(
        (isinstance(base, ast.Name) and base.id in base_names) or
        (isinstance(base, ast.Attribute) and base.attr == 'ConanFile') for base in node.bases)]
    if len(recipes) != 1:
        raise RuntimeError('add necesita una única clase derivada directamente de ConanFile; revisa la declaración manualmente.')
    names = set()
    for node in ast.walk(recipes[0]):
        values = None
        if isinstance(node, (ast.Assign, ast.AnnAssign)):
            targets = node.targets if isinstance(node, ast.Assign) else [node.target]
            if any(isinstance(target, ast.Name) and target.id == 'requires' for target in targets):
                try:
                    values = ast.literal_eval(node.value)
                except (ValueError, TypeError):
                    raise RuntimeError('Hay dependencias calculadas dinámicamente; decláralas manualmente y ejecuta python3 kotavel.py install.')
                if isinstance(values, str):
                    values = [values]
                if not isinstance(values, (tuple, list)) or not all(isinstance(value, str) for value in values):
                    raise RuntimeError('No se pudo comprobar la declaración requires.')
        elif isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute):
            if isinstance(node.func.value, ast.Name) and node.func.value.id == 'self' and node.func.attr == 'requires':
                if not node.args or not isinstance(node.args[0], ast.Constant) or not isinstance(node.args[0].value, str):
                    raise RuntimeError('Hay dependencias calculadas dinámicamente; decláralas manualmente y ejecuta python3 kotavel.py install.')
                values = [node.args[0].value]
        if values is not None:
            names.update(dependency_name(value) for value in values)
    return names


def project_dependencies(source):
    # Incluye los nombres heredados del framework para evitar duplicarlos en la aplicación.
    names = declared_dependencies(source)
    if (ROOT / 'kotavel.json').is_file():
        helpers = runpy.run_path(str(ROOT / 'kotavel_config.py'))
        config = helpers['load_config'](ROOT)
        if config['mode'] == 'package':
            names.add('kotavel')
        else:
            framework = helpers['framework_metadata'](config)
            names.update(dependency_name(ref) for ref in framework['FRAMEWORK_REQUIRES'])
    return names


@contextmanager
def project_guard():
    """OS locks are released even if the process exits unexpectedly."""
    TOOLS.mkdir(parents=True, exist_ok=True)
    with (TOOLS / 'deps.guard').open('a+b') as stream:
        if os.name == 'nt':
            import msvcrt
            if stream.seek(0, os.SEEK_END) == 0:
                stream.write(b'\0')
                stream.flush()
            stream.seek(0)
            acquire = lambda: msvcrt.locking(stream.fileno(), msvcrt.LK_NBLCK, 1)
            release = lambda: msvcrt.locking(stream.fileno(), msvcrt.LK_UNLCK, 1)
        else:
            import fcntl
            acquire = lambda: fcntl.flock(stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
            release = lambda: fcntl.flock(stream.fileno(), fcntl.LOCK_UN)
        try:
            acquire()
        except OSError as error:
            raise RuntimeError('Otro comando install está trabajando en este proyecto; espera a que termine.') from error
        try:
            yield
        finally:
            stream.seek(0)
            release()


def execute(command, env):
    command = [str(arg) for arg in command]
    with subprocess.Popen(command, cwd=ROOT, env=env) as process:
        try:
            code = process.wait()
        except BaseException:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
            raise
        if code:
            raise subprocess.CalledProcessError(code, command)


def read_optional(path):
    return path.read_bytes() if path.exists() else None


def atomic_write(path, content):
    """Replace a file on the same filesystem without exposing a partial write."""
    descriptor, name = tempfile.mkstemp(prefix=f'.{path.name}.', dir=path.parent)
    temporary = Path(name)
    try:
        with os.fdopen(descriptor, 'wb') as stream:
            stream.write(content)
        temporary.replace(path)
    finally:
        temporary.unlink(missing_ok=True)


def add_dependency(reference, conan, env, offline=False):
    recipe = ROOT / 'conanfile.py'
    original = recipe.read_bytes()
    names = project_dependencies(original.decode('utf-8-sig')) if reference else set()
    name = validate_reference(reference) if reference else None
    lock = ROOT / 'conan.lock'
    previous_lock = read_optional(lock)
    edited = original
    remote_args = ['--no-remote'] if offline else []
    with tempfile.TemporaryDirectory(prefix='deps-', dir=TOOLS) as directory:
        temporary = Path(directory) / 'conan.lock'
        try:
            if name in names:
                print(f'{name} ya está declarada; se conserva su versión y sus opciones.', flush=True)
            elif name is not None:
                try:
                    execute([conan, 'require', 'add', reference, *remote_args], env)
                finally:
                    edited = read_optional(recipe)
                if edited is None or name not in declared_dependencies(edited.decode('utf-8-sig')):
                    raise RuntimeError(f'Conan no añadió la dependencia {name}.')
            command = [conan, 'lock', 'create', '.', '--lockfile-partial',
                       '--lockfile-out', temporary, '--build=missing', *remote_args]
            if previous_lock is not None:
                command.extend(['--lockfile', lock])
            execute(command, env)
            candidate = temporary.read_bytes()
            data = json.loads(candidate)
            if not isinstance(data, dict) or not isinstance(data.get('requires'), list) or not data.get('version'):
                raise RuntimeError('Conan generó un lockfile inválido.')
            execute([conan, 'install', '.', '--lockfile', temporary, '--build=missing', *remote_args], env)
            if read_optional(recipe) != edited or read_optional(lock) != previous_lock:
                raise RuntimeError('La receta o el lockfile cambiaron durante la operación; se conserva la edición externa. Ejecuta python3 kotavel.py install de nuevo.')
            if candidate != previous_lock:
                temporary.replace(lock)
        except BaseException:
            if edited != original and read_optional(recipe) == edited:
                atomic_write(recipe, original)
            raise
    print('Dependencias instaladas y conan.lock actualizado.', flush=True)


def prepare_environment(offline):
    scripts = TOOLS / 'python' / ('Scripts' if os.name == 'nt' else 'bin')
    conan = scripts / ('conan.exe' if os.name == 'nt' else 'conan')
    prepare = [sys.executable, Path(__file__).with_name('install.py'), '--project', ROOT]
    if offline:
        prepare.append('--configure-only')
    execute(prepare, os.environ.copy())
    if not conan.is_file():
        raise RuntimeError('No se encontró Conan en el entorno local del proyecto.')
    manifest = json.loads((TOOLS / 'platform.json').read_text(encoding='utf-8-sig'))
    env = os.environ.copy()
    paths = [str(scripts)]
    if manifest.get('compiler_bin'):
        paths.append(manifest['compiler_bin'])
    paths.append(env.get('PATH', ''))
    env['PATH'] = os.pathsep.join(paths)
    env['CONAN_HOME'] = str(ROOT / '.kotavel' / 'conan')
    return conan, env


def main(argv=None):
    global ROOT, TOOLS
    parser = argparse.ArgumentParser(prog='python3 kotavel.py install', description=__doc__)
    parser.add_argument('--project', type=Path, help='Raíz del proyecto consumidor')
    parser.add_argument('action', nargs='?', default='install', choices=['install', 'add'])
    parser.add_argument('dependency', nargs='?', help='Nombre o nombre/versión de Conan')
    parser.add_argument('--offline', action='store_true', help='Utiliza solo herramientas y paquetes de la caché local')
    args = parser.parse_args(argv)
    if args.project is not None:
        ROOT = args.project.resolve()
        TOOLS = ROOT / '.kotavel' / 'tools'
    if args.action == 'add':
        if not args.dependency:
            parser.error('add necesita una dependencia, por ejemplo: add fmt/11.2.0')
        validate_reference(args.dependency)
    elif args.dependency:
        parser.error('install no recibe una dependencia; instala todas las declaradas.')
    with project_guard():
        if args.action == 'add':
            declared_dependencies((ROOT / 'conanfile.py').read_text(encoding='utf-8-sig'))
        conan, env = prepare_environment(args.offline)
        add_dependency(args.dependency, conan, env, args.offline)


if __name__ == '__main__':
    try:
        main()
    except KeyboardInterrupt:
        print('Operación cancelada.', file=sys.stderr)
        sys.exit(130)
    except (RuntimeError, OSError, ValueError, SyntaxError, subprocess.CalledProcessError) as error:
        print(f'ERROR: {error}', file=sys.stderr)
        sys.exit(1)
