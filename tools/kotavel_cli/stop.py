"""Stop Kotavel applications started for the selected project."""
import argparse
from pathlib import Path
import re
import time

from .platforms import get_platform


_ENDPOINT_NAME = re.compile(r'^kotavel-(\d+)\.endpoint$')


def _process_id(endpoint: Path):
    match = _ENDPOINT_NAME.fullmatch(endpoint.name)
    if not match:
        return None
    process_id = int(match.group(1))
    return process_id if process_id > 0 else None


def _belongs_to_project(command_line, project: Path) -> bool:
    if not command_line:
        return False
    try:
        executable = Path(command_line[0].strip('"')).resolve()
        executable.relative_to((project / 'build').resolve())
    except (OSError, ValueError):
        return False

    expected_root = str(project.resolve())
    for argument in command_line[1:]:
        option, separator, value = argument.partition('=')
        if separator and option.strip('"') == '--project-root':
            try:
                return Path(value.strip('"')).resolve() == Path(expected_root)
            except (OSError, ValueError):
                return False
    return False


def _find_instances(project: Path, requested_pid, platform):
    directory = project / 'storage' / 'framework' / 'metrics'
    if not directory.is_dir():
        raise RuntimeError(f'No hay una instancia activa de Kotavel para {project}.')

    candidates = []
    for endpoint in directory.glob('kotavel-*.endpoint'):
        process_id = _process_id(endpoint)
        if process_id is None or (requested_pid is not None and process_id != requested_pid):
            continue
        try:
            port = int(endpoint.read_text(encoding='ascii').splitlines()[0])
        except (OSError, UnicodeError, ValueError, IndexError):
            continue
        if not 0 < port < 65536:
            continue
        command_line = platform.process_command_line(process_id)
        if _belongs_to_project(command_line, project):
            candidates.append((endpoint, process_id))

    if not candidates:
        if requested_pid is not None:
            raise RuntimeError(f'No se encontró una instancia activa de Kotavel con PID {requested_pid} en este proyecto.')
        raise RuntimeError('No se encontró una instancia activa de Kotavel en este proyecto. '
                           'Comprueba que `metrics.stream` esté habilitado en la configuración.')
    return sorted(candidates, key=lambda candidate: candidate[1])


def _stop_instance(endpoint, process_id, project, platform):
    platform.request_process_stop(process_id)
    deadline = time.monotonic() + 10
    while time.monotonic() < deadline and platform.process_is_running(process_id):
        time.sleep(0.1)
    forced = False
    if platform.process_is_running(process_id):
        current_command = platform.process_command_line(process_id)
        if not _belongs_to_project(current_command, project):
            raise RuntimeError('El PID ya no corresponde a esta aplicación; no se cerró otro proceso.')
        platform.force_process_stop(process_id)
        forced = True
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline and platform.process_is_running(process_id):
            time.sleep(0.1)
        if platform.process_is_running(process_id):
            raise RuntimeError(f'No se pudo detener el proceso Kotavel {process_id}.')

    # Una salida abrupta puede impedir que el destructor nativo retire el endpoint.
    endpoint.unlink(missing_ok=True)
    return forced


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project', type=Path, default=Path.cwd(), help='Raíz del consumidor')
    parser.add_argument('--pid', type=int, help='PID de la instancia que se debe detener')
    args = parser.parse_args(argv)
    project = args.project.resolve()
    platform = get_platform()
    instances = _find_instances(project, args.pid, platform)
    stopped = []
    failures = []
    for endpoint, process_id in instances:
        try:
            forced = _stop_instance(endpoint, process_id, project, platform)
            stopped.append((process_id, forced))
        except (OSError, RuntimeError) as error:
            failures.append(f'{process_id}: {error}')

    for process_id, forced in stopped:
        suffix = ' (cierre forzado tras esperar 10 segundos)' if forced else ''
        print(f'Aplicación Kotavel detenida (PID {process_id}){suffix}.')
    if failures:
        raise RuntimeError('No se pudieron detener todas las instancias: ' + '; '.join(failures))
    return 0
