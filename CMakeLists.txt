cmake_minimum_required(VERSION 3.25)
project(Kotavel VERSION 0.1.0 LANGUAGES CXX)

# Target mínimo sin implementación; la arquitectura está por definir.
add_library(kotavel_core INTERFACE)
# El consumidor usa el mismo nombre en modo local y como paquete Conan.
add_library(Kotavel::Core ALIAS kotavel_core)
target_compile_features(kotavel_core INTERFACE cxx_std_23)
set_target_properties(kotavel_core PROPERTIES CXX_EXTENSIONS OFF EXPORT_NAME Core)
# Las cabeceras se buscan en el árbol fuente o en el paquete instalado.
target_include_directories(kotavel_core INTERFACE
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
    $<INSTALL_INTERFACE:include>)

# Conan utiliza estas reglas al construir el paquete distribuible.
include(GNUInstallDirs)
install(TARGETS kotavel_core EXPORT KotavelTargets
    ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR}
    LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR}
    RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
install(DIRECTORY include/ DESTINATION ${CMAKE_INSTALL_INCLUDEDIR})
install(EXPORT KotavelTargets NAMESPACE Kotavel:: DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/Kotavel)
