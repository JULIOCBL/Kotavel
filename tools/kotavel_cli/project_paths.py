"""Paths shared by local preparation and dependency commands."""
from pathlib import Path
import os

try:
    from .platforms import get_platform
except ImportError:  # install.py can import this module as a direct script.
    from platforms import get_platform


def conan_home(project_root, system):
    """Share the Conan cache between projects while retaining legacy caches."""
    project_root = Path(project_root).resolve()
    return get_platform(system).conan_home(
        project_root, Path.home(), os.environ.get('KOTAVEL_CONAN_HOME'))
