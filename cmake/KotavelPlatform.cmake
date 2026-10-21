# Selección única del adaptador nativo para cada sistema soportado.
include(CheckCXXSymbolExists)
set(KOTAVEL_PLATFORM_LIBRARIES)
set(KOTAVEL_HTTP_PLATFORM_LIBRARIES)

if(WIN32)
    set(KOTAVEL_PLATFORM_NAME "windows")
    set(KOTAVEL_PLATFORM_SOURCES
        src/platform/windows/operating_system.cpp
        src/platform/windows/process_metrics.cpp
        src/platform/windows/application_bundle.cpp
        src/platform/windows/system_paths.cpp
        src/platform/windows/runtime_paths.cpp
        src/platform/windows/directory_lease.cpp
        src/platform/windows/stack_trace.cpp
        src/platform/windows/utc_time.cpp
        src/platform/windows/host_resolver.cpp
        src/platform/windows/metrics_stream_transport.cpp)
    set(KOTAVEL_PLATFORM_LIBRARIES Psapi Ws2_32)
    set(KOTAVEL_HTTP_PLATFORM_LIBRARIES Ws2_32)
elseif(APPLE)
    set(KOTAVEL_PLATFORM_NAME "macos")
    set(KOTAVEL_PLATFORM_SOURCES
        src/platform/macos/operating_system.cpp
        src/platform/macos/process_metrics.cpp
        src/platform/macos/application_bundle.cpp
        src/platform/macos/system_paths.cpp
        src/platform/macos/runtime_paths.cpp
        src/platform/posix/directory_lease.cpp
        src/platform/posix/stack_trace.cpp
        src/platform/posix/utc_time.cpp
        src/platform/posix/host_resolver.cpp
        src/platform/macos/socket_signal.cpp
        src/platform/posix/metrics_stream_transport.cpp)
    set(KOTAVEL_HTTP_PLATFORM_LIBRARIES)
elseif(CMAKE_SYSTEM_NAME STREQUAL "Linux")
    set(KOTAVEL_PLATFORM_NAME "linux")
    set(KOTAVEL_PLATFORM_SOURCES
        src/platform/linux/operating_system.cpp
        src/platform/linux/process_metrics.cpp
        src/platform/linux/application_bundle.cpp
        src/platform/linux/system_paths.cpp
        src/platform/linux/runtime_paths.cpp
        src/platform/posix/directory_lease.cpp
        src/platform/posix/stack_trace.cpp
        src/platform/posix/utc_time.cpp
        src/platform/posix/host_resolver.cpp
        src/platform/linux/socket_signal.cpp
        src/platform/posix/metrics_stream_transport.cpp)
    set(KOTAVEL_HTTP_PLATFORM_LIBRARIES)
else()
    message(FATAL_ERROR "Unsupported Kotavel platform: ${CMAKE_SYSTEM_NAME}")
endif()

check_cxx_symbol_exists(backtrace "execinfo.h" KOTAVEL_HAS_BACKTRACE)
