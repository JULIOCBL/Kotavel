from conan import ConanFile
from conan.tools.cmake import CMake, CMakeDeps, CMakeToolchain, cmake_layout
from conan.tools.files import copy
import os

# La receta es la fuente de las dependencias del framework en ambos modos.
# Mantén estas constantes literales: el consumidor local las lee sin ejecutar la receta.
FRAMEWORK_REQUIRES = ("rapidjson/cci.20250205", "sqlite3/3.54.0")
# The CCI snapshot fixes RapidJSON 1.1.0's Clang 23 assignment error and its
# lazy-stack pointer arithmetic reported by UBSan. Use one header version on
# every platform so the public build and sanitizer matrix stay consistent.
FRAMEWORK_PLATFORM_REQUIRES = {}
FRAMEWORK_OPTIONAL_REQUIRES = {
    "http": ("drogon/1.9.13",),
    "desktop": ("qt/6.11.1",),
}
FRAMEWORK_OPTIONS = {}
FRAMEWORK_FEATURE_OPTIONS = {
    "http": {
        "drogon/*:with_boost": False,
        "drogon/*:with_orm": False,
        "drogon/*:with_ctl": False,
        "drogon/*:with_brotli": False,
    },
    # Qt se enlaza dinámicamente para respetar su alternativa de distribución LGPL.
    "desktop": {
        "qt/*:shared": True,
        "qt/*:qtdeclarative": True,
        "qt/*:qtshadertools": True,
        "qt/*:qtquickcontrols2": False,
        "qt/*:widgets": False,
        "qt/*:with_dbus": False,
        # No activar drivers multimedia o SQL que la capa de ventanas no utiliza.
        "qt/*:with_sqlite3": False,
        "qt/*:with_pq": False,
        "qt/*:with_odbc": False,
        "qt/*:with_openal": False,
        "qt/*:with_brotli": False,
        "qt/*:with_md4c": False,
    },
}


class KotavelConan(ConanFile):
    name = "kotavel"
    version = "0.1.0"
    license = "MIT"
    author = "Julio Cesar Borges Lopez"
    package_type = "static-library"
    settings = "os", "compiler", "build_type", "arch"
    options = {"with_http": [True, False], "with_desktop": [True, False]}
    exports_sources = "CMakeLists.txt", "include/*", "src/*", "cmake/*", "resources/*", "LICENSE"
    default_options = {"with_http": False, "with_desktop": False}

    def requirements(self):
        platform_requires = FRAMEWORK_PLATFORM_REQUIRES.get(str(self.settings.os), ())
        replaced = {reference.split("/", 1)[0] for reference in platform_requires}
        for reference in FRAMEWORK_REQUIRES:
            if reference.split("/", 1)[0] not in replaced:
                self.requires(reference)
        for reference in platform_requires:
            self.requires(reference)
        if self.options.with_http:
            self.requires("drogon/1.9.13", options={
                "with_boost": False,
                "with_orm": False,
                "with_ctl": False,
                "with_brotli": False,
            })
        if self.options.with_desktop:
            self.requires("qt/6.11.1", options={
                "shared": True,
                "qtdeclarative": True,
                "qtshadertools": True,
                "qtquickcontrols2": False,
                "widgets": False,
                "with_dbus": False,
                "with_sqlite3": False,
                "with_pq": False,
                "with_odbc": False,
                "with_openal": False,
                "with_brotli": False,
                "with_md4c": False,
            })

    def layout(self):
        cmake_layout(self)

    def generate(self):
        CMakeDeps(self).generate()
        toolchain = CMakeToolchain(self)
        toolchain.variables["KOTAVEL_BUILD_TESTS"] = False
        toolchain.variables["KOTAVEL_BUILD_HTTP"] = bool(self.options.with_http)
        toolchain.variables["KOTAVEL_BUILD_DESKTOP_QT"] = bool(self.options.with_desktop)
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
        # RapidJSON es privado; SQLite forma parte del enlace del contrato de sesiones.
        self.cpp_info.ignored_requires = ["rapidjson"]
        core = self.cpp_info.components["core"]
        core.set_property("cmake_target_name", "Kotavel::Core")
        core.libs = ["kotavel_core"]
        contract = self.cpp_info.components["http_contract"]
        contract.set_property("cmake_target_name", "Kotavel::HttpContract")
        contract.libs = ["kotavel_http_contract"]
        contract.requires = ["core", "sqlite3::sqlite"]
        if self.settings.os == "Windows":
            # Session IDs use BCryptGenRandom.
            contract.system_libs = ["bcrypt", "advapi32"]
        desktop = self.cpp_info.components["desktop"]
        desktop.set_property("cmake_target_name", "Kotavel::Desktop")
        desktop.libs = ["kotavel_desktop"]
        desktop.requires = ["http_contract"]
        if self.options.with_desktop:
            desktop_qt = self.cpp_info.components["desktop_qt"]
            desktop_qt.set_property("cmake_target_name", "Kotavel::DesktopQt")
            desktop_qt.libs = ["kotavel_desktop_qt"]
            desktop_qt.requires = ["desktop", "qt::qtQuick", "qt::qtQml", "qt::qtGui"]
        if self.options.with_http:
            adapter = self.cpp_info.components["http_drogon"]
            adapter.set_property("cmake_target_name", "Kotavel::HttpDrogon")
            adapter.libs = ["kotavel_http_drogon"]
            adapter.requires = ["http_contract", "drogon::drogon"]

            http = self.cpp_info.components["http"]
            http.set_property("cmake_target_name", "Kotavel::Http")
            http.requires = ["http_drogon"]
        if self.settings.os == "Linux":
            core.system_libs = ["pthread"]
