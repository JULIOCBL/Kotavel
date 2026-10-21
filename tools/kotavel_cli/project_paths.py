"""Paths shared by local preparation and dependency commands."""
from pathlib import Path


def conan_home(project_root, system):
    """Give Windows packages a short, stable cache path to avoid MAX_PATH."""
    project_root = Path(project_root).resolve()
    if system == 'Windows':
        # Qt MOC still hits MAX_PATH in deeply nested generated include paths;
        # keep one shared Conan cache root exceptionally short on Windows.
        return Path.home() / '.k'
    return project_root / '.kotavel' / 'conan'
