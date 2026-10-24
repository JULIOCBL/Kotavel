# Selección única del adaptador nativo para cada sistema soportado.
include(CheckCXXSymbolExists)
set(KOTAVEL_PLATFORM_LIBRARIES)
set(KOTAVEL_HTTP_PLATFORM_LIBRARIES)
set(KOTAVEL_FILESYSTEM_PLATFORM_LIBRARIES)

if(WIN32)
    set(KOTAVEL_PLATFORM_NAME "windows")
    set(KOTAVEL_PLATFORM_SOURCES
        src/platform/system/windows/operating_system.cpp
        src/platform/console/windows/console_output.cpp
        src/platform/metrics/windows/process_metrics.cpp
        src/platform/application/windows/application_bundle.cpp
        src/platform/paths/windows/system_paths.cpp
        src/platform/paths/windows/runtime_paths.cpp
        src/platform/diagnostics/windows/directory_lease.cpp
        src/platform/diagnostics/windows/stack_trace.cpp
        src/platform/time/windows/utc_time.cpp
        src/platform/networking/windows/host_resolver.cpp
        src/platform/networking/windows/network_addresses.cpp
        src/platform/metrics/windows/metrics_stream_transport.cpp)
    set(KOTAVEL_PLATFORM_LIBRARIES Psapi Ws2_32 Iphlpapi)
    set(KOTAVEL_HTTP_PLATFORM_SOURCES src/platform/security/windows/session_random.cpp)
    set(KOTAVEL_HTTP_PLATFORM_LIBRARIES Bcrypt)
    set(KOTAVEL_FILESYSTEM_PLATFORM_SOURCES src/platform/filesystem/windows/filesystem.cpp)
    set(KOTAVEL_FILESYSTEM_PLATFORM_LIBRARIES Advapi32)
elseif(APPLE)
    set(KOTAVEL_PLATFORM_NAME "macos")
    set(KOTAVEL_PLATFORM_SOURCES
        src/platform/system/macos/operating_system.cpp
        src/platform/console/posix/console_output.cpp
        src/platform/metrics/macos/process_metrics.cpp
        src/platform/application/macos/application_bundle.cpp
        src/platform/paths/macos/system_paths.cpp
        src/platform/paths/macos/runtime_paths.cpp
        src/platform/diagnostics/posix/directory_lease.cpp
        src/platform/diagnostics/posix/stack_trace.cpp
        src/platform/time/posix/utc_time.cpp
        src/platform/networking/posix/host_resolver.cpp
        src/platform/networking/posix/network_addresses.cpp
        src/platform/networking/macos/socket_signal.cpp
        src/platform/metrics/posix/metrics_stream_transport.cpp)
    set(KOTAVEL_HTTP_PLATFORM_SOURCES src/platform/security/posix/session_random.cpp)
    set(KOTAVEL_FILESYSTEM_PLATFORM_SOURCES src/platform/filesystem/posix/filesystem.cpp)
elseif(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    set(KOTAVEL_PLATFORM_NAME "linux")
    set(KOTAVEL_PLATFORM_SOURCES
        src/platform/system/linux/operating_system.cpp
        src/platform/console/posix/console_output.cpp
        src/platform/metrics/linux/process_metrics.cpp
        src/platform/application/linux/application_bundle.cpp
        src/platform/paths/linux/system_paths.cpp
        src/platform/paths/linux/runtime_paths.cpp
        src/platform/diagnostics/posix/directory_lease.cpp
        src/platform/diagnostics/posix/stack_trace.cpp
        src/platform/time/posix/utc_time.cpp
        src/platform/networking/posix/host_resolver.cpp
        src/platform/networking/posix/network_addresses.cpp
        src/platform/networking/linux/socket_signal.cpp
        src/platform/metrics/posix/metrics_stream_transport.cpp)
    set(KOTAVEL_HTTP_PLATFORM_SOURCES src/platform/security/posix/session_random.cpp)
    set(KOTAVEL_FILESYSTEM_PLATFORM_SOURCES src/platform/filesystem/posix/filesystem.cpp)
else()
    message(FATAL_ERROR "Unsupported Kotavel platform: ${CMAKE_SYSTEM_NAME}")
endif()

check_cxx_symbol_exists(backtrace "execinfo.h" KOTAVEL_HAS_BACKTRACE)
