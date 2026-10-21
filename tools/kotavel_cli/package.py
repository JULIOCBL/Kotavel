"""Build and package a macOS Kotavel application bundle."""
import argparse
from pathlib import Path
import plistlib
import re
import shutil
import subprocess
import sys
import tempfile
import time
import uuid

from .project_paths import conan_home
from .platforms import get_platform


def environment_name(env_file):
    """Map supported environment filenames to the runtime's environment key."""
    name = Path(env_file).name
    names = {'.env': 'production', '.env.local': 'local',
             '.env.development': 'development'}
    if name not in names:
        raise ValueError('Usa .env, .env.local o .env.development para --env-file.')
    return names[name]


def run(command):
    subprocess.run([str(part) for part in command], check=True)


def reset_output_directory(output, source):
    """Remove previous deliverables before creating a fresh set."""
    output = Path(output).absolute()
    if output.is_symlink():
        raise ValueError(f'El directorio de salida no puede ser un enlace simbólico: {output}')
    output = output.resolve()
    source = Path(source).resolve()
    if (output == Path(output.anchor) or output == source
            or output in source.parents or source in output.parents):
        raise ValueError(f'El directorio de salida no se puede borrar de forma segura: {output}')
    if output.exists():
        previous_output = output.with_name(
            f'.{output.name}.old-{uuid.uuid4().hex}')
        output.rename(previous_output)
        output.mkdir(parents=True, exist_ok=True)
        for attempt in range(3):
            try:
                shutil.rmtree(previous_output)
                break
            except OSError:
                if attempt == 2:
                    raise
                time.sleep(0.1)
    output.mkdir(parents=True, exist_ok=True)


def macdeployqt(project_root):
    adapter = get_platform()
    adapter.require_packager()
    return adapter.macdeployqt(project_root, conan_home(project_root, adapter.name))


def package_app(app_bundle, output_directory, formats=('app', 'dmg', 'pkg'),
                title='Kotavel', version=None, *, project_root=None,
                env_file=None, environment='production'):
    """Create macOS deliverables for one built .app bundle."""
    output = Path(output_directory).resolve()
    formats = tuple(dict.fromkeys(formats))
    get_platform().require_packager()
    source = Path(app_bundle).resolve()
    if not source.is_dir() or source.suffix != '.app' or not (source / 'Contents' / 'Info.plist').is_file():
        raise ValueError(f'No es un bundle .app válido: {source}')
    if not formats or any(item not in {'app', 'dmg', 'pkg'} for item in formats):
        raise ValueError('Los formatos disponibles son app, dmg y pkg.')
    if not title.strip():
        raise ValueError('El título no puede estar vacío.')
    with (source / 'Contents' / 'Info.plist').open('rb') as plist_file:
        metadata = plistlib.load(plist_file)
    version = version or metadata.get('CFBundleShortVersionString') or metadata.get('CFBundleVersion') or '1.0.0'
    if not isinstance(version, str) or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._+-]*', version):
        raise ValueError('La versión debe ser válida.')

    reset_output_directory(output, source)
    app_copy = output / source.name
    shutil.copytree(source, app_copy, symlinks=True)

    if project_root is not None:
        project_root = Path(project_root).resolve()
        resources = app_copy / 'Contents' / 'Resources'
        resources.mkdir(parents=True, exist_ok=True)
        views = project_root / 'resources' / 'views'
        if not views.is_dir():
            raise RuntimeError(f'No se encontraron las vistas del consumidor: {views}')
        bundled_views = resources / 'resources' / 'views'
        if bundled_views.exists():
            shutil.rmtree(bundled_views)
        shutil.copytree(views, bundled_views)
        if env_file is None or not Path(env_file).is_file():
            raise RuntimeError('Se necesita un archivo .env válido para empaquetar la aplicación.')
        shutil.copy2(env_file, resources / Path(env_file).name)
        (resources / 'kotavel-environment').write_text(environment + '\n', encoding='utf-8')

        deploy_tool = macdeployqt(project_root)
        run([deploy_tool, app_copy, f'-qmldir={views}', '-always-overwrite'])
        run(['codesign', '--force', '--deep', '--sign', '-', app_copy])
        run(['codesign', '--verify', '--deep', '--strict', app_copy])

    artifacts = []
    if 'app' in formats:
        artifacts.append(app_copy)

    safe_title = ''.join(char if char.isalnum() or char in '-_.' else '-' for char in title).strip('-.') or 'Kotavel'
    if 'dmg' in formats:
        image = output / f'{safe_title}-{version}.dmg'
        with tempfile.TemporaryDirectory(prefix='kotavel-dmg-') as directory:
            stage = Path(directory)
            shutil.copytree(app_copy, stage / app_copy.name, symlinks=True)
            (stage / 'Applications').symlink_to('/Applications')
            image.unlink(missing_ok=True)
            run(['hdiutil', 'create', '-volname', title, '-srcfolder', stage,
                 '-ov', '-format', 'UDZO', image])
        artifacts.append(image)

    if 'pkg' in formats:
        package = output / f'{safe_title}-{version}.pkg'
        package.unlink(missing_ok=True)
        run(['productbuild', '--component', app_copy, '/Applications', package])
        artifacts.append(package)
    return artifacts


def main(argv=None):
    parser = argparse.ArgumentParser(prog='python3 kotavel.py package',
                                     description=__doc__)
    parser.add_argument('--format', choices=('app', 'dmg', 'pkg', 'all'), default='all',
                        help='Formatos que se generan (predeterminado: all).')
    parser.add_argument('--env-file', type=Path, default=Path('.env'),
                        help='Archivo de entorno usado al compilar (predeterminado: .env).')
    parser.add_argument('--output', type=Path, default=Path('dist'),
                        help='Directorio de salida (predeterminado: dist).')
    parser.add_argument('--build-type', choices=('Debug', 'Release'))
    parser.add_argument('--title', help='Nombre visible en el instalador.')
    parser.add_argument('--version', help='Versión de los artefactos (por omisión se lee del Info.plist).')
    parser.add_argument('--project', type=Path, help=argparse.SUPPRESS)
    args = parser.parse_args(argv)

    adapter = get_platform()
    adapter.require_packager()
    root = (args.project or Path.cwd()).resolve()
    env_file = args.env_file if args.env_file.is_absolute() else root / args.env_file
    env_file = env_file.resolve()
    if env_file.parent != root:
        raise RuntimeError('--env-file debe estar en la raíz del proyecto para que CMake y la aplicación usen la misma configuración.')
    if not env_file.is_file():
        raise RuntimeError(f'No se encontró el archivo de entorno: {env_file}')
    selected_environment = environment_name(env_file)
    # Reutiliza la preparación, dependencias, compilación y pruebas del consumidor.
    from . import install
    build_args = ['--build', '--env', selected_environment]
    if args.build_type:
        build_args.extend(['--build-type', args.build_type])
    if args.project:
        build_args.extend(['--project', str(args.project.resolve())])
    install.main(build_args)

    build_type = args.build_type
    if build_type is None:
        _, config = install.read_profile(adapter.name)
        build_type = config['settings']['build_type']
    name = install.project_executable(build_type)
    app_bundle = install.project_executable_path(build_type, name, adapter.name)
    if not app_bundle.is_dir() or app_bundle.suffix != '.app':
        raise RuntimeError(f'No se encontró el bundle .app compilado: {app_bundle}')
    formats = ('app', 'dmg', 'pkg') if args.format == 'all' else (args.format,)
    title = args.title or name
    output = args.output if args.output.is_absolute() else root / args.output
    artifacts = package_app(app_bundle, output, formats, title, args.version,
                            project_root=root, env_file=env_file,
                            environment=selected_environment)
    for artifact in artifacts:
        print(f'Generado: {artifact}', flush=True)
    return 0


if __name__ == '__main__':
    try:
        raise SystemExit(main())
    except (RuntimeError, OSError, ValueError, subprocess.CalledProcessError) as error:
        print(f'ERROR: {error}', file=sys.stderr)
        raise SystemExit(1)
