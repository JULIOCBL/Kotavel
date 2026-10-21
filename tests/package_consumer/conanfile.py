from conan import ConanFile
from conan.tools.cmake import CMakeDeps, CMakeToolchain


class KotavelBinaryConsumer(ConanFile):
    name = "kotavel-binary-consumer"
    version = "0.1"
    settings = "os", "arch", "compiler", "build_type"
    requires = "kotavel/0.1.0"

    def generate(self):
        CMakeDeps(self).generate()
        CMakeToolchain(self).generate()
