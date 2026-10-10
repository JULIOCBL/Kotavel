"""Comandos compartidos; cada ejecución opera sobre un consumidor explícito."""
import argparse
import configparser
from pathlib import Path
import subprocess
import sys


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--project', type=Path, default=Path.cwd(), help='Raíz del consumidor')
    parser.add_argument('command', choices=('prepare', 'install', 'build', 'test', 'run', 'shell'))
    args, remaining = parser.parse_known_args(argv)
    root = args.project.resolve()
    try:
        if args.command == 'install':
            from . import deps
            deps.main([*remaining, '--project', str(root)])
        else:
            from . import install
            flags = {'prepare': [], 'build': ['--build'], 'test': ['--test'],
                     'run': ['--run'], 'shell': ['--shell']}
            install.main([*flags[args.command], *remaining, '--project', str(root)])
        return 0
    except KeyboardInterrupt:
        print('Operación cancelada.', file=sys.stderr)
        return 130
    except (RuntimeError, OSError, ValueError, SyntaxError, configparser.Error,
            subprocess.CalledProcessError) as error:
        print(f'ERROR: {error}', file=sys.stderr)
        return 1
