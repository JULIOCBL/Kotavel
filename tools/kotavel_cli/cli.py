"""Comandos compartidos; cada ejecución opera sobre un consumidor explícito."""
import argparse
import configparser
import os
from pathlib import Path
import subprocess
import sys


class CommandHelpParser(argparse.ArgumentParser):
    def format_help(self):
        # Usar color solo en terminales; los archivos y las tuberías reciben texto limpio.
        colored = sys.stdout.isatty() and 'NO_COLOR' not in os.environ and os.environ.get('TERM') != 'dumb'

        def paint(text, color):
            return f'\033[{color}m{text}\033[0m' if colored else text

        def row(command, description, width=27):
            return '  ' + paint(command.ljust(width), '33') + description

        lines = [paint('KOTAVEL · Herramientas de desarrollo', '1;36'),
                 paint('Uso: ', '1;36') + 'python3 kotavel.py <comando> [opciones]', '',
                 paint('Comandos:', '1;36')]
        for command, description in [
            ('prepare', 'Prepara herramientas y perfil.'),
            ('install', 'Instala dependencias y actualiza conan.lock.'),
            ('build', 'Instala dependencias, compila y prueba.'),
            ('test', 'Ejecuta las pruebas sin recompilar.'),
            ('run', 'Compila, prueba y ejecuta la aplicación.'),
            ('monitor', 'Muestra métricas de una aplicación en ejecución.'),
            ('shell', 'Abre la terminal con las herramientas activas.'),
        ]:
            lines.append(row(command, description))
        lines.extend(['', paint('Variantes:', '1;36')])
        for command, description in [
            ('install add nombre/version', 'Añade una dependencia sin duplicarla.'),
            ('run --env=local', 'Carga .env.local y ejecuta la aplicación.'),
            ('run --env=development', 'Carga .env.development y ejecuta.'),
            ('run --env=production', 'Carga .env (opción predeterminada).'),
            ('run --deep-link URI', 'Abre una ruta desktop desde su URI.'),
            ('install --offline', 'Usa solo la caché local.'),
            ('test --framework', 'Comprueba Python, núcleo e integración local.'),
            ('monitor --pid PID', 'Selecciona una instancia concreta.'),
        ]:
            lines.append(row(command, description))
        lines.extend(['', paint('Opciones:', '1;36')])
        for option, description in [
            ('--build-type Debug|Release', 'Selecciona el tipo de compilación.'),
            ('--configure-only', 'Reutiliza herramientas sin descargarlas.'),
            ('--project RUTA', 'Selecciona otro proyecto; va antes del comando.'),
            ('-h, --help', 'Muestra la ayuda.'),
        ]:
            lines.append(row(option, description))
        lines.extend(['', paint('Ejemplos:', '1;36'),
                      '  ' + paint('python3 kotavel.py test --framework', '33'),
                      '  ' + paint('python3 kotavel.py install add nombre/version', '33'),
                      '  ' + paint('python3 kotavel.py <comando> --help', '33'), ''])
        return '\n'.join(lines)


def main(argv=None):
    parser = CommandHelpParser(prog='python3 kotavel.py',
                               description='Herramientas de desarrollo de Kotavel.')
    parser.add_argument('--project', type=Path, default=Path.cwd(), help='Raíz del consumidor')
    commands = parser.add_subparsers(dest='command', required=True, title='Comandos disponibles', parser_class=argparse.ArgumentParser)
    descriptions = {
        'prepare': 'Prepara las herramientas, el compilador y el perfil del proyecto.',
        'install': 'Instala dependencias con Conan y actualiza conan.lock.',
        'build': 'Instala dependencias, compila el proyecto y ejecuta sus pruebas.',
        'test': 'Ejecuta las pruebas del proyecto; --framework comprueba Python, núcleo e integración local.',
        'run': 'Compila, prueba y ejecuta la aplicación.',
        'monitor': 'Muestra CPU y RAM de una aplicación Kotavel activa.',
        'shell': 'Abre una terminal con las herramientas del proyecto activas.',
    }
    for name, description in descriptions.items():
        # La ayuda de cada comando se delega al módulo que conoce sus opciones.
        commands.add_parser(name, help=description, add_help=False)
    args, remaining = parser.parse_known_args(argv)
    root = args.project.resolve()
    try:
        if args.command == 'monitor':
            from . import monitor
            return monitor.main([*remaining, '--project', str(root)])
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
    except subprocess.CalledProcessError as error:
        print(f'ERROR: El proceso terminó con código {error.returncode}. Revisa el diagnóstico anterior.', file=sys.stderr)
        return 1
    except (RuntimeError, OSError, ValueError, SyntaxError, configparser.Error) as error:
        print(f'ERROR: {error}', file=sys.stderr)
        return 1
