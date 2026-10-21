#include "../../platform_services.hpp"

#include <sys/resource.h>
#include <mach/mach.h>

namespace kotavel::platform::detail {
namespace {
class MacOSProcessMetricsProvider final : public contracts::ProcessMetricsProvider {
public:
    std::optional<contracts::ProcessMetricsSnapshot> read() const noexcept override {
        rusage usage{};
        if (getrusage(RUSAGE_SELF, &usage) != 0) return std::nullopt;
        mach_task_basic_info_data_t info{};
        mach_msg_type_number_t count = MACH_TASK_BASIC_INFO_COUNT;
        if (task_info(mach_task_self(), MACH_TASK_BASIC_INFO,
                      reinterpret_cast<task_info_t>(&info), &count) != KERN_SUCCESS)
            return std::nullopt;
        const auto user = std::chrono::seconds(usage.ru_utime.tv_sec) +
                          std::chrono::microseconds(usage.ru_utime.tv_usec);
        const auto system = std::chrono::seconds(usage.ru_stime.tv_sec) +
                            std::chrono::microseconds(usage.ru_stime.tv_usec);
        return contracts::ProcessMetricsSnapshot{
            std::chrono::duration_cast<std::chrono::nanoseconds>(user + system),
            static_cast<std::uint64_t>(info.resident_size)};
    }
};
}

std::unique_ptr<contracts::ProcessMetricsProvider> make_process_metrics_provider() {
    return std::make_unique<MacOSProcessMetricsProvider>();
}
} // namespace kotavel::platform::detail
