"""Platform adapters used by the development commands."""
from importlib import import_module
import platform as host_platform

_PLATFORMS = {
    'Windows': ('.windows', 'WindowsPlatform'),
    'Linux': ('.linux', 'LinuxPlatform'),
    'Darwin': ('.macos', 'MacOSPlatform'),
}


def get_platform(system=None):
    """Compose one platform adapter at the command boundary."""
    name = system or host_platform.system()
    try:
        module_name, class_name = _PLATFORMS[name]
    except KeyError as error:
        raise RuntimeError(f'Sistema no soportado: {name}') from error
    adapter = getattr(import_module(module_name, __package__), class_name)
    return adapter()
