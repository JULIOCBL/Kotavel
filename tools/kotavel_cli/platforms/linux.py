"""Linux implementation of the development command platform contract."""
from .posix import PosixPlatform


class LinuxPlatform(PosixPlatform):
    name = 'Linux'
    profile_name = 'linux'
    conan_os = 'Linux'
