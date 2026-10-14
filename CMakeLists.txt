cmake_minimum_required(VERSION 3.25)
project(Kotavel VERSION 0.1.0 LANGUAGES CXX)

# Núcleo compilado: ciclo de vida y diagnóstico sin dependencias externas.
add_library(kotavel_core STATIC
    # Configuración: mapas y consultas en memoria; entorno separado en src/env/.
    src/config/config.cpp
    src/config/config_operations.cpp
    src/config/config_map.cpp
    src/config/config_file.cpp
    src/config/config_helpers.cpp
    src/env/env.cpp
    src/env/process_environment.cpp
    src/env/file_environment.cpp
    src/env/environment_value.cpp
    src/env/env_helpers.cpp
    src/env/app_environment.cpp
    src/console/runtime.cpp
    src/console/settings.cpp
    src/console/arguments.cpp
    src/console/operation.cpp
    src/foundation/application.cpp
    src/foundation/application_errors.cpp
    src/diagnostics/logger.cpp
    src/diagnostics/log_formatter.cpp
    src/diagnostics/error.cpp
    src/diagnostics/stack_trace.cpp
    src/diagnostics/error_renderer.cpp
    src/diagnostics/pending.cpp)

# Comprobar enlace, no solo cabecera: algunas libc no ofrecen backtrace.
include(CheckCXXSymbolExists)
if(NOT WIN32)
    check_cxx_symbol_exists(backtrace "execinfo.h" KOTAVEL_HAS_BACKTRACE)
    if(KOTAVEL_HAS_BACKTRACE)
        target_compile_definitions(kotavel_core PRIVATE KOTAVEL_HAS_BACKTRACE=1)
    endif()
endif()
find_package(Threads REQUIRED)
target_link_libraries(kotavel_core PUBLIC Threads::Threads)
# El consumidor usa el mismo nombre en modo local y como paquete Conan.
add_library(Kotavel::Core ALIAS kotavel_core)
target_compile_features(kotavel_core PUBLIC cxx_std_23)
set_target_properties(kotavel_core PROPERTIES CXX_EXTENSIONS OFF EXPORT_NAME Core)
# Las cabeceras se buscan en el árbol fuente o en el paquete instalado.
target_include_directories(kotavel_core PUBLIC
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

# Las pruebas del framework no se añaden por defecto al proyecto consumidor.
option(KOTAVEL_BUILD_TESTS "Build Kotavel runtime tests" ${PROJECT_IS_TOP_LEVEL})
if(KOTAVEL_BUILD_TESTS)
    enable_testing()
    # Las fuentes unitarias se agrupan por módulo bajo tests/unit/.
    add_executable(kotavel_config_tests tests/unit/config/config_tests.cpp)
    target_link_libraries(kotavel_config_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.config COMMAND kotavel_config_tests)
    add_executable(kotavel_config_extended_tests tests/unit/config/config_extended_tests.cpp)
    target_link_libraries(kotavel_config_extended_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.config.extended COMMAND kotavel_config_extended_tests)
    add_executable(kotavel_config_edge_tests tests/unit/config/config_edge_tests.cpp)
    target_link_libraries(kotavel_config_edge_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.config.edge COMMAND kotavel_config_edge_tests)
    add_executable(kotavel_console_tests tests/unit/console/console_tests.cpp)
    target_link_libraries(kotavel_console_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.console COMMAND kotavel_console_tests)
    add_executable(kotavel_logging_settings_tests tests/unit/console/logging_settings_tests.cpp)
    target_link_libraries(kotavel_logging_settings_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.console.logging COMMAND kotavel_logging_settings_tests)
    set_tests_properties(kotavel.console.logging PROPERTIES ENVIRONMENT "APP_ENV=production;APP_DEBUG=false;LOG_CHANNEL=\"null\"")
    set_tests_properties(kotavel.console PROPERTIES ENVIRONMENT "APP_ENV=production;APP_DEBUG=false")
    add_executable(kotavel_env_tests tests/unit/env/env_tests.cpp)
    target_link_libraries(kotavel_env_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.env COMMAND kotavel_env_tests)
    set_tests_properties(kotavel.env PROPERTIES ENVIRONMENT "KOTAVEL_TEST_PROCESS_VALUE=copied;KOTAVEL_TEST_BOOL=true")
    add_executable(kotavel_core_tests tests/unit/core/core_tests.cpp)
    target_link_libraries(kotavel_core_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.core COMMAND kotavel_core_tests)
    add_executable(kotavel_diagnostics_tests tests/unit/diagnostics/diagnostics_tests.cpp)
    target_link_libraries(kotavel_diagnostics_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.diagnostics COMMAND kotavel_diagnostics_tests)
    add_executable(kotavel_logging_tests tests/unit/logging/logging_tests.cpp)
    target_link_libraries(kotavel_logging_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.logging COMMAND kotavel_logging_tests)
    find_package(Python3 REQUIRED COMPONENTS Interpreter)
    if(Python3_Interpreter_FOUND)
        add_test(NAME kotavel.logging.processes COMMAND ${Python3_EXECUTABLE}
            ${CMAKE_CURRENT_SOURCE_DIR}/tests/integration/logging_processes.py
            $<TARGET_FILE:kotavel_logging_tests>)
        set_tests_properties(kotavel.logging.processes PROPERTIES TIMEOUT 30)
    endif()
endif()

include(CMakePackageConfigHelpers)
configure_package_config_file(cmake/KotavelConfig.cmake.in
    ${CMAKE_CURRENT_BINARY_DIR}/KotavelConfig.cmake
    INSTALL_DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/Kotavel)
write_basic_package_version_file(${CMAKE_CURRENT_BINARY_DIR}/KotavelConfigVersion.cmake
    VERSION ${PROJECT_VERSION} COMPATIBILITY SameMajorVersion)
install(FILES ${CMAKE_CURRENT_BINARY_DIR}/KotavelConfig.cmake
    ${CMAKE_CURRENT_BINARY_DIR}/KotavelConfigVersion.cmake
    DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/Kotavel)

option(KOTAVEL_BUILD_BENCHMARKS "Build reproducible core microbenchmarks" OFF)
if(KOTAVEL_BUILD_BENCHMARKS)
    add_executable(kotavel_core_benchmark benchmarks/core_benchmark.cpp)
    target_link_libraries(kotavel_core_benchmark PRIVATE Kotavel::Core)
endif()

if(MSVC)
    target_compile_options(kotavel_core PUBLIC /utf-8)
endif()
