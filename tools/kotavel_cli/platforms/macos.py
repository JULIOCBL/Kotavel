"""macOS implementation of the development command platform contract."""
from pathlib import Path

from .posix import PosixPlatform


class MacOSPlatform(PosixPlatform):
    name = 'Darwin'
    profile_name = 'macos'
    conan_os = 'Macos'

    def executable_path(self, root, build_type, name):
        build = Path(root) / 'build' / build_type
        bundle = build / 'app' / (name + '.app')
        if bundle.is_dir():
            return bundle
        return super().executable_path(root, build_type, name)

    def executable_for_run(self, executable):
        executable = Path(executable)
        if executable.suffix == '.app':
            return executable / 'Contents' / 'MacOS' / executable.stem
        return executable

    def require_packager(self):
        return None

    def package_supported(self):
        return True

    def macdeployqt(self, project_root, conan_home):
        cache = Path(conan_home)
        tools = sorted((cache / 'p' / 'b').glob('qt*/p/bin/macdeployqt'))
        tool = next((candidate for candidate in tools if candidate.is_file()), None)
        if tool is None:
            import shutil
            from_path = shutil.which('macdeployqt')
            tool = Path(from_path) if from_path else None
        if tool is None:
            raise RuntimeError('No se encontró macdeployqt. Instala Qt con Conan antes de empaquetar.')
        return tool
