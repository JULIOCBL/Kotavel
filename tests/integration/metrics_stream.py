#!/usr/bin/env python3
"""Exercise the C++ metrics stream through a loopback client on every OS."""

import json
import errno
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time


def main() -> int:
    executable = sys.argv[1]
    # Some CI sandboxes prohibit loopback sockets; report a CTest skip only when
    # the environment itself blocks the transport before the fixture is started.
    try:
        with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as probe:
            probe.bind(('127.0.0.1', 0))
    except OSError as error:
        if error.errno in (errno.EPERM, errno.EACCES, 10013):
            print(f'Loopback sockets are unavailable in this environment: {error}', file=sys.stderr)
            return 77
        raise

    with tempfile.TemporaryDirectory(prefix='kotavel-metrics-stream-') as temporary:
        root = Path(temporary)
        process = subprocess.Popen(
            [executable, str(root)], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True
        )
        connection = None
        endpoint = None
        try:
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline and connection is None:
                endpoints = list((root / 'storage/framework/metrics').glob('kotavel-*.endpoint'))
                if endpoints:
                    endpoint = endpoints[0]
                    try:
                        port = int(endpoint.read_text(encoding='ascii').strip())
                        connection = socket.create_connection(('127.0.0.1', port), timeout=2)
                    except (OSError, ValueError):
                        connection = None
                if connection is None:
                    if process.poll() is not None:
                        raise AssertionError(f"Stream fixture exited early: {process.stderr.read()}")
                    time.sleep(0.02)
            if connection is None or endpoint is None:
                raise AssertionError('Metrics stream did not publish a connectable endpoint.')

            connection.settimeout(3)
            with connection, connection.makefile('r', encoding='utf-8') as stream:
                startup = json.loads(stream.readline())
                sample = json.loads(stream.readline())
            if startup.get('type') != 'startup' or startup.get('total_ms') != 10.0:
                raise AssertionError(f'Invalid startup event: {startup}')
            if sample.get('type') != 'sample' or sample.get('cpu_percent') != 12.5:
                raise AssertionError(f'Invalid process sample: {sample}')
            if sample.get('resident_memory_bytes') != 4 * 1024 * 1024:
                raise AssertionError(f'Invalid resident memory value: {sample}')

            process.wait(timeout=4)
            if process.returncode != 0:
                raise AssertionError(f'Stream fixture exited with {process.returncode}.')
            if endpoint.exists():
                raise AssertionError('The endpoint descriptor remained after normal shutdown.')
        finally:
            if connection is not None:
                connection.close()
            if process.poll() is None:
                process.kill()
                process.wait(timeout=2)
            if process.stderr is not None:
                process.stderr.close()
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
