"""Detect POSIX tools without starting another shell or Python interpreter."""
import json
import os
from pathlib import Path
import shutil
import sys


def prepare(compiler, env):
    names = {'gcc': ('gcc', 'g++'), 'clang': ('clang', 'clang++'),
             'apple-clang': ('clang', 'clang++')}
    if compiler not in names:
        raise RuntimeError(f'Compilador no soportado en este sistema: {compiler}')
    cc, cpp = (shutil.which(name, path=env.get('PATH', '')) for name in names[compiler])
    if compiler == 'gcc' and not (cc and cpp):
        candidates = []
        directories = dict.fromkeys(env.get('PATH', '').split(os.pathsep))
        for directory in directories:
            if not directory:
                continue
            for executable in Path(directory).glob('g++-*'):
                suffix = executable.name.removeprefix('g++-')
                if suffix.isdigit():
                    candidates.append((int(suffix), suffix, executable))
        for _, suffix, executable in sorted(candidates, key=lambda item: item[0], reverse=True):
            c = executable.with_name(f'gcc-{suffix}')
            if all(path.is_file() and os.access(path, os.X_OK) for path in (c, executable)):
                cc, cpp = str(c.absolute()), str(executable.absolute())
                break
    if not (cc and cpp):
        hint = ('En Ubuntu/Debian: sudo apt install build-essential. '
                'Con Homebrew: brew install gcc; activa brew en el PATH.') if compiler == 'gcc' else (
                'Instala Clang y actívalo en el PATH; en macOS: xcode-select --install.')
        raise RuntimeError(f'El perfil requiere {compiler}. {hint}')
    shell = env.get('SHELL') or shutil.which('sh', path=env.get('PATH', ''))
    if not shell:
        raise RuntimeError('No se encontró una terminal compatible.')
    arguments = [shell, '--noprofile', '--norc', '-i'] if Path(shell).name == 'bash' else [shell, '-i']
    return {
        'scripts_directory': 'bin', 'python_executable': 'python',
        'compiler_bin': '', 'shell': arguments, 'activation_kind': 'posix',
        'compiler_executables': {'c': cc, 'cpp': cpp},
    }


if __name__ == '__main__':
    try:
        prepared = prepare(sys.argv[2], os.environ)
        Path(sys.argv[1]).write_text(json.dumps(prepared), encoding='utf-8')
    except (RuntimeError, OSError) as error:
        sys.exit(str(error))
