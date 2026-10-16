#include <kotavel/metrics/performance.hpp>
#include "process_metrics_internal.hpp"

#define NOMINMAX
#include <windows.h>
#include <psapi.h>

namespace kotavel::metrics::detail {
std::optional<ProcessSnapshot> read_process_snapshot() {
    // Windows entrega el tiempo acumulado en unidades de 100 ns y working set en bytes.
    FILETIME creation{}, exit{}, kernel{}, user{};
    if (!GetProcessTimes(GetCurrentProcess(), &creation, &exit, &kernel, &user))
        return std::nullopt;
    PROCESS_MEMORY_COUNTERS_EX memory{};
    memory.cb = sizeof(memory);
    if (!GetProcessMemoryInfo(GetCurrentProcess(),
            reinterpret_cast<PROCESS_MEMORY_COUNTERS*>(&memory), sizeof(memory)))
        return std::nullopt;

    ULARGE_INTEGER kernel_ticks{};
    ULARGE_INTEGER user_ticks{};
    kernel_ticks.LowPart = kernel.dwLowDateTime;
    kernel_ticks.HighPart = kernel.dwHighDateTime;
    user_ticks.LowPart = user.dwLowDateTime;
    user_ticks.HighPart = user.dwHighDateTime;
    constexpr std::uint64_t hundred_ns_per_second = 10'000'000;
    const auto ticks = kernel_ticks.QuadPart + user_ticks.QuadPart;
    const auto cpu_ns = ticks * (1'000'000'000ULL / hundred_ns_per_second);
    return ProcessSnapshot{std::chrono::nanoseconds(cpu_ns),
                           static_cast<std::uint64_t>(memory.WorkingSetSize)};
}
} // namespace kotavel::metrics::detail
