#include "../../platform_services.hpp"

#define NOMINMAX
#include <windows.h>
#include <psapi.h>

namespace kotavel::platform::detail {
namespace {
class WindowsProcessMetricsProvider final : public contracts::ProcessMetricsProvider {
public:
    std::optional<contracts::ProcessMetricsSnapshot> read() const noexcept override {
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
        constexpr std::uint64_t nanoseconds_per_100ns = 100;
        const auto ticks = kernel_ticks.QuadPart + user_ticks.QuadPart;
        return contracts::ProcessMetricsSnapshot{
            std::chrono::nanoseconds(ticks * nanoseconds_per_100ns),
            static_cast<std::uint64_t>(memory.WorkingSetSize)};
    }
};
}

std::unique_ptr<contracts::ProcessMetricsProvider> make_process_metrics_provider() {
    return std::make_unique<WindowsProcessMetricsProvider>();
}
} // namespace kotavel::platform::detail
