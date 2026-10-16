"""Cliente local para mostrar las métricas publicadas por Kotavel."""
import argparse
import json
from pathlib import Path
import socket
from typing import Optional


def _connect(project: Path, pid: Optional[int]) -> socket.socket:
    """Conecta al endpoint activo más reciente o al PID solicitado."""
    directory = project / 'storage' / 'framework' / 'metrics'
    candidates = list(directory.glob('kotavel-*.endpoint'))
    if pid is not None:
        candidates = [path for path in candidates if path.name == f'kotavel-{pid}.endpoint']
    def modified_time(path: Path) -> int:
        try:
            return path.stat().st_mtime_ns
        except OSError:
            return 0

    candidates.sort(key=modified_time, reverse=True)

    for endpoint in candidates:
        try:
            port = int(endpoint.read_text(encoding='ascii').strip())
            if not 0 < port < 65536:
                continue
            return socket.create_connection(('127.0.0.1', port), timeout=2)
        except (OSError, ValueError):
            # Ignorar descriptores que quedaron de un proceso terminado abruptamente.
            continue
    if pid is not None:
        raise RuntimeError(f'No hay métricas activas para el proceso {pid} en {project}.')
    raise RuntimeError(f'No hay una aplicación Kotavel activa en {project}. Inicia primero `python3 kotavel.py run`.')


def _show(message: dict) -> None:
    """Presenta cada tipo de evento del protocolo local en formato legible."""
    if message.get('type') == 'startup':
        print(
            '[Kotavel] Arranque: '
            f"entorno {message['environment_ms']:.2f} ms | "
            f"configuración {message['configuration_ms']:.2f} ms | "
            f"opciones {message['settings_ms']:.2f} ms | "
            f"aplicación {message['application_boot_ms']:.2f} ms | "
            f"total {message['total_ms']:.2f} ms",
            flush=True,
        )
    elif message.get('type') == 'sample':
        cpu = message.get('cpu_percent')
        cpu_text = 'calculando' if cpu is None else f'{cpu:.1f} %'
        memory_mib = message['resident_memory_bytes'] / (1024 * 1024)
        print(f'[Kotavel] CPU: {cpu_text} | RAM residente: {memory_mib:.1f} MiB', flush=True)
    else:
        raise ValueError('El proceso Kotavel envió un evento de métricas desconocido.')


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project', type=Path, default=Path.cwd(), help='Raíz del consumidor')
    parser.add_argument('--pid', type=int, help='PID de la instancia a observar')
    args = parser.parse_args(argv)
    project = args.project.resolve()
    connection = _connect(project, args.pid)
    # Configured intervals may be as long as 60 seconds; do not time out while idle.
    connection.settimeout(None)
    print(f'Monitor conectado a {project}. Presiona Ctrl+C para desconectarte.', flush=True)
    with connection:
        stream = connection.makefile('r', encoding='utf-8')
        with stream:
            for line in stream:
                _show(json.loads(line))
    print('La aplicación Kotavel se desconectó.', flush=True)
    return 0
