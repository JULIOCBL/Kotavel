"""Platform adapters used by the development commands."""
import platform as host_platform

from .linux import LinuxPlatform
from .macos import MacOSPlatform
from .windows import WindowsPlatform


_PLATFORMS = {
    'Windows': WindowsPlatform,
    'Linux': LinuxPlatform,
    'Darwin': MacOSPlatform,
}


def get_platform(system=None):
    """Compose one platform adapter at the command boundary."""
    name = system or host_platform.system()
    try:
        return _PLATFORMS[name]()
    except KeyError as error:
        raise RuntimeError(f'Sistema no soportado: {name}') from error
