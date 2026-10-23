cmake_minimum_required(VERSION 3.25)
project(Kotavel VERSION 0.1.0 LANGUAGES CXX)

option(KOTAVEL_BUILD_HTTP "Build optional HTTP adapters" OFF)
set(KOTAVEL_WITH_HTTP ${KOTAVEL_BUILD_HTTP})
option(KOTAVEL_BUILD_DESKTOP_QT "Build optional Qt Quick desktop windows" OFF)

include(cmake/Messages.cmake)
include(cmake/KotavelPlatform.cmake)

# Núcleo compilado: ciclo de vida y diagnóstico sin dependencias externas.
add_library(kotavel_core STATIC
    src/text/messages.cpp
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
    # Contenedor explícito de servicios y su ciclo de vida.
    src/container/container.cpp
    src/platform/platform_service_provider.cpp
    src/console/runtime.cpp
    src/console/settings.cpp
    src/console/arguments.cpp
    src/console/operation.cpp
    # Métricas de proceso: API propia con adaptador del sistema operativo.
    src/metrics/metrics.cpp
    src/metrics/metrics_stream.cpp
    src/foundation/app.cpp
    src/foundation/application.cpp
    src/foundation/application_errors.cpp
    src/diagnostics/logger.cpp
    src/diagnostics/bootstrap_logger.cpp
    src/diagnostics/log_formatter.cpp
    src/diagnostics/error.cpp
    src/diagnostics/stack_trace.cpp
    src/diagnostics/error_renderer.cpp
    src/diagnostics/pending.cpp
    ${KOTAVEL_PLATFORM_SOURCES})
target_link_libraries(kotavel_core PRIVATE ${KOTAVEL_PLATFORM_LIBRARIES})

# Comprobar enlace, no solo cabecera: algunas libc no ofrecen backtrace.
if(KOTAVEL_HAS_BACKTRACE)
    target_compile_definitions(kotavel_core PRIVATE KOTAVEL_HAS_BACKTRACE=1)
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
target_include_directories(kotavel_core PRIVATE "${CMAKE_CURRENT_BINARY_DIR}/generated/text")

# General file operations are a reusable module; native details stay in the
# platform adapters selected by cmake/KotavelPlatform.cmake.
add_library(kotavel_filesystem STATIC
    src/filesystem/filesystem.cpp
    ${KOTAVEL_FILESYSTEM_PLATFORM_SOURCES})
add_library(Kotavel::Filesystem ALIAS kotavel_filesystem)
target_compile_features(kotavel_filesystem PUBLIC cxx_std_23)
set_target_properties(kotavel_filesystem PROPERTIES CXX_EXTENSIONS OFF EXPORT_NAME Filesystem)
target_include_directories(kotavel_filesystem PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
    $<INSTALL_INTERFACE:include>)
target_link_libraries(kotavel_filesystem PRIVATE ${KOTAVEL_FILESYSTEM_PLATFORM_LIBRARIES})
target_link_libraries(kotavel_core PUBLIC Kotavel::Filesystem)

# API y web son opcionales; solo este adaptador incorpora Drogon al ejecutable.
find_package(RapidJSON CONFIG REQUIRED)
add_library(kotavel_http_contract STATIC
    src/http/router.cpp
    src/http/kernel.cpp
    src/http/api_response.cpp
    src/http/request.cpp
    src/http/form_request.cpp
    src/http/session_middleware.cpp
    src/session/session.cpp
    src/session/file_session_store.cpp
    ${KOTAVEL_HTTP_PLATFORM_SOURCES})
add_library(Kotavel::HttpContract ALIAS kotavel_http_contract)
target_compile_features(kotavel_http_contract PUBLIC cxx_std_23)
set_target_properties(kotavel_http_contract PROPERTIES CXX_EXTENSIONS OFF EXPORT_NAME HttpContract)
target_include_directories(kotavel_http_contract PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
    $<INSTALL_INTERFACE:include>)
target_link_libraries(kotavel_http_contract PUBLIC Kotavel::Core Kotavel::Filesystem
    PRIVATE $<BUILD_INTERFACE:rapidjson> ${KOTAVEL_HTTP_PLATFORM_LIBRARIES})

add_library(kotavel_desktop STATIC
    src/desktop/desktop.cpp)
add_library(Kotavel::Desktop ALIAS kotavel_desktop)
target_compile_features(kotavel_desktop PUBLIC cxx_std_23)
set_target_properties(kotavel_desktop PROPERTIES CXX_EXTENSIONS OFF EXPORT_NAME Desktop)
target_include_directories(kotavel_desktop PUBLIC
    $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
    $<INSTALL_INTERFACE:include>)

if(KOTAVEL_BUILD_DESKTOP_QT)
    find_package(Qt6 REQUIRED COMPONENTS Gui Qml Quick)
    add_library(kotavel_desktop_qt STATIC
        src/desktop/qt_platform.cpp
        src/desktop/kotavel_branding.qrc)
    set_target_properties(kotavel_desktop_qt PROPERTIES AUTORCC ON)
    add_library(Kotavel::DesktopQt ALIAS kotavel_desktop_qt)
    target_compile_features(kotavel_desktop_qt PUBLIC cxx_std_23)
    set_target_properties(kotavel_desktop_qt PROPERTIES CXX_EXTENSIONS OFF EXPORT_NAME DesktopQt)
    target_include_directories(kotavel_desktop_qt PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
        $<INSTALL_INTERFACE:include>)
    target_link_libraries(kotavel_desktop_qt PUBLIC Kotavel::Desktop Qt6::Gui Qt6::Qml Qt6::Quick)
endif()

if(KOTAVEL_BUILD_HTTP)
    find_package(Drogon CONFIG REQUIRED)
    add_library(kotavel_http_drogon STATIC src/http/drogon_server.cpp)
    add_library(Kotavel::HttpDrogon ALIAS kotavel_http_drogon)
    target_compile_features(kotavel_http_drogon PUBLIC cxx_std_23)
    set_target_properties(kotavel_http_drogon PROPERTIES CXX_EXTENSIONS OFF EXPORT_NAME HttpDrogon)
    target_include_directories(kotavel_http_drogon PUBLIC
        $<BUILD_INTERFACE:${CMAKE_CURRENT_SOURCE_DIR}/include>
        $<INSTALL_INTERFACE:include>)
    target_link_libraries(kotavel_http_drogon PUBLIC kotavel_http_contract Drogon::Drogon)
    add_library(kotavel_http INTERFACE)
    add_library(Kotavel::Http ALIAS kotavel_http)
    set_target_properties(kotavel_http PROPERTIES EXPORT_NAME Http)
    target_link_libraries(kotavel_http INTERFACE kotavel_http_drogon)
endif()

# Conan utiliza estas reglas al construir el paquete distribuible.
include(GNUInstallDirs)
install(TARGETS kotavel_core kotavel_filesystem kotavel_http_contract kotavel_desktop EXPORT KotavelTargets
    ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR}
    LIBRARY DESTINATION ${CMAKE_INSTALL_LIBDIR}
    RUNTIME DESTINATION ${CMAKE_INSTALL_BINDIR})
if(KOTAVEL_BUILD_DESKTOP_QT)
    install(TARGETS kotavel_desktop_qt EXPORT KotavelTargets
        ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR})
endif()
if(KOTAVEL_BUILD_HTTP)
    install(TARGETS kotavel_http_drogon EXPORT KotavelTargets
        ARCHIVE DESTINATION ${CMAKE_INSTALL_LIBDIR})
    install(TARGETS kotavel_http EXPORT KotavelTargets)
endif()
install(DIRECTORY include/ DESTINATION ${CMAKE_INSTALL_INCLUDEDIR})
install(EXPORT KotavelTargets NAMESPACE Kotavel:: DESTINATION ${CMAKE_INSTALL_LIBDIR}/cmake/Kotavel)

# Las pruebas del framework no se añaden por defecto al proyecto consumidor.
option(KOTAVEL_BUILD_TESTS "Build Kotavel runtime tests" ${PROJECT_IS_TOP_LEVEL})
if(KOTAVEL_BUILD_TESTS)
    enable_testing()
    find_package(Python3 REQUIRED COMPONENTS Interpreter)
    add_executable(kotavel_filesystem_tests tests/unit/filesystem/filesystem_tests.cpp)
    target_link_libraries(kotavel_filesystem_tests PRIVATE Kotavel::Filesystem)
    add_test(NAME kotavel.filesystem COMMAND kotavel_filesystem_tests)
    add_executable(kotavel_messages_tests tests/unit/text/messages_tests.cpp)
    target_link_libraries(kotavel_messages_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.messages COMMAND kotavel_messages_tests)
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
    add_executable(kotavel_metrics_tests tests/unit/metrics/metrics_tests.cpp)
    target_link_libraries(kotavel_metrics_tests PRIVATE Kotavel::Core)
    target_include_directories(kotavel_metrics_tests PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/src)
    add_test(NAME kotavel.metrics COMMAND kotavel_metrics_tests)
    add_executable(kotavel_os_tests tests/unit/system/os_tests.cpp)
    target_link_libraries(kotavel_os_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.os COMMAND kotavel_os_tests)
    add_executable(kotavel_platform_services_integration_tests
        tests/integration/platform_services.cpp)
    target_link_libraries(kotavel_platform_services_integration_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.platform.services
        COMMAND kotavel_platform_services_integration_tests)
    set_tests_properties(kotavel.platform.services PROPERTIES TIMEOUT 15)
    add_executable(kotavel_metrics_stream_fixture tests/unit/metrics/metrics_stream_fixture.cpp)
    target_link_libraries(kotavel_metrics_stream_fixture PRIVATE Kotavel::Core)
    add_test(NAME kotavel.metrics.stream COMMAND ${Python3_EXECUTABLE}
        ${CMAKE_CURRENT_SOURCE_DIR}/tests/integration/metrics_stream.py
        $<TARGET_FILE:kotavel_metrics_stream_fixture>)
    set_tests_properties(kotavel.metrics.stream PROPERTIES TIMEOUT 10 SKIP_RETURN_CODE 77)
    add_executable(kotavel_env_tests tests/unit/env/env_tests.cpp)
    target_link_libraries(kotavel_env_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.env COMMAND kotavel_env_tests)
    set_tests_properties(kotavel.env PROPERTIES ENVIRONMENT "KOTAVEL_TEST_PROCESS_VALUE=copied;KOTAVEL_TEST_BOOL=true")
    add_executable(kotavel_core_tests tests/unit/core/core_tests.cpp)
    target_link_libraries(kotavel_core_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.core COMMAND kotavel_core_tests)
    add_executable(kotavel_container_tests tests/unit/container/container_tests.cpp)
    target_link_libraries(kotavel_container_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.container COMMAND kotavel_container_tests)
    add_executable(kotavel_routes_tests tests/unit/http/routes_tests.cpp)
    target_link_libraries(kotavel_routes_tests PRIVATE Kotavel::HttpContract Kotavel::Desktop)
    add_test(NAME kotavel.routes COMMAND kotavel_routes_tests)
    add_executable(kotavel_desktop_tests tests/unit/desktop/desktop_tests.cpp)
    target_link_libraries(kotavel_desktop_tests PRIVATE Kotavel::Desktop)
    add_test(NAME kotavel.desktop COMMAND kotavel_desktop_tests)
    add_executable(kotavel_http_middleware_tests tests/unit/http/middleware_tests.cpp)
    target_link_libraries(kotavel_http_middleware_tests PRIVATE Kotavel::HttpContract)
    target_include_directories(kotavel_http_middleware_tests PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/src)
    add_test(NAME kotavel.http.middleware COMMAND kotavel_http_middleware_tests)
    add_executable(kotavel_api_response_tests tests/unit/http/api_response_tests.cpp)
    target_link_libraries(kotavel_api_response_tests PRIVATE Kotavel::HttpContract)
    add_test(NAME kotavel.http.api_response COMMAND kotavel_api_response_tests)
    add_executable(kotavel_http_request_tests tests/unit/http/request_tests.cpp)
    target_link_libraries(kotavel_http_request_tests PRIVATE Kotavel::HttpContract)
    add_test(NAME kotavel.http.request COMMAND kotavel_http_request_tests)
    add_executable(kotavel_http_content_negotiation_tests
        tests/unit/http/content_negotiation_tests.cpp)
    target_link_libraries(kotavel_http_content_negotiation_tests PRIVATE Kotavel::HttpContract)
    add_test(NAME kotavel.http.content_negotiation COMMAND kotavel_http_content_negotiation_tests)
    add_executable(kotavel_http_session_tests tests/unit/session/session_tests.cpp)
    target_link_libraries(kotavel_http_session_tests PRIVATE Kotavel::HttpContract)
    add_test(NAME kotavel.session COMMAND kotavel_http_session_tests)
    set_tests_properties(kotavel.session PROPERTIES TIMEOUT 30)
    add_executable(kotavel_http_session_integration_tests
        tests/integration/session/session_integration.cpp)
    target_link_libraries(kotavel_http_session_integration_tests PRIVATE Kotavel::HttpContract)
    add_test(NAME kotavel.session.integration
        COMMAND kotavel_http_session_integration_tests)
    set_tests_properties(kotavel.session.integration PROPERTIES TIMEOUT 15)
    if(KOTAVEL_BUILD_HTTP)
        add_executable(kotavel_http_transport_fixture tests/integration/http_transport_fixture.cpp)
        target_link_libraries(kotavel_http_transport_fixture PRIVATE Kotavel::Http)
        add_test(NAME kotavel.http.transport COMMAND ${Python3_EXECUTABLE}
            ${CMAKE_CURRENT_SOURCE_DIR}/tests/integration/http_transport.py
            $<TARGET_FILE:kotavel_http_transport_fixture>)
        set_tests_properties(kotavel.http.transport PROPERTIES TIMEOUT 30)
    endif()
    add_executable(kotavel_diagnostics_tests tests/unit/diagnostics/diagnostics_tests.cpp)
    target_link_libraries(kotavel_diagnostics_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.diagnostics COMMAND kotavel_diagnostics_tests)
    add_executable(kotavel_logging_tests tests/unit/logging/logging_tests.cpp)
    target_link_libraries(kotavel_logging_tests PRIVATE Kotavel::Core)
    add_test(NAME kotavel.logging COMMAND kotavel_logging_tests)
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
    add_executable(kotavel_http_benchmark benchmarks/http_benchmark.cpp)
    target_link_libraries(kotavel_http_benchmark PRIVATE Kotavel::HttpContract)
endif()

if(MSVC)
    target_compile_options(kotavel_core PUBLIC /utf-8)
endif()
