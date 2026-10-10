from conan import ConanFile
from conan.tools.cmake import CMake, CMakeDeps, CMakeToolchain, cmake_layout
from conan.tools.files import copy
import os

# La receta es la fuente de las dependencias del framework en ambos modos.
# Mantén estas constantes literales: el consumidor local las lee sin ejecutar la receta.
FRAMEWORK_REQUIRES = ()
FRAMEWORK_OPTIONS = {}


class KotavelConan(ConanFile):
    name = "kotavel"
    version = "0.1.0"
    license = "MIT"
    author = "Julio Cesar Borges Lopez"
    package_type = "static-library"
    settings = "os", "compiler", "build_type", "arch"
    exports_sources = "CMakeLists.txt", "include/*", "src/*", "cmake/*", "LICENSE"
    default_options = FRAMEWORK_OPTIONS

    def requirements(self):
        for reference in FRAMEWORK_REQUIRES:
            self.requires(reference)

    def layout(self):
        cmake_layout(self)

    def generate(self):
        CMakeDeps(self).generate()
        toolchain = CMakeToolchain(self)
        toolchain.variables["KOTAVEL_BUILD_TESTS"] = False
        toolchain.generate()

    def build(self):
        cmake = CMake(self)
        cmake.configure()
        cmake.build()

    def package(self):
        CMake(self).install()
        copy(self, "LICENSE", src=self.source_folder,
             dst=os.path.join(self.package_folder, "licenses"))

    def package_info(self):
        # CMakeDeps expone el mismo target público que la compilación desde carpeta.
        self.cpp_info.set_property("cmake_file_name", "Kotavel")
        self.cpp_info.set_property("cmake_target_name", "Kotavel::Core")

        self.cpp_info.libs = ["kotavel_core"]
        if self.settings.os == "Linux":
            self.cpp_info.system_libs = ["pthread"]
