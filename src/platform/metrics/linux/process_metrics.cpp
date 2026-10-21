#include "../../platform_services.hpp"

#include <fstream>
#include <sys/resource.h>
#include <unistd.h>

namespace kotavel::platform::detail {
namespace {
class LinuxProcessMetricsProvider final : public contracts::ProcessMetricsProvider {
public:
    std::optional<contracts::ProcessMetricsSnapshot> read() const noexcept override {
        rusage usage{};
        if (getrusage(RUSAGE_SELF, &usage) != 0) return std::nullopt;

        std::ifstream statm("/proc/self/statm");
        std::uint64_t total_pages = 0;
        std::uint64_t resident_pages = 0;
        if (!(statm >> total_pages >> resident_pages)) return std::nullopt;
        const long page_size = sysconf(_SC_PAGESIZE);
        if (page_size <= 0) return std::nullopt;

        const auto user = std::chrono::seconds(usage.ru_utime.tv_sec) +
                          std::chrono::microseconds(usage.ru_utime.tv_usec);
        const auto system = std::chrono::seconds(usage.ru_stime.tv_sec) +
                            std::chrono::microseconds(usage.ru_stime.tv_usec);
        return contracts::ProcessMetricsSnapshot{
            std::chrono::duration_cast<std::chrono::nanoseconds>(user + system),
            resident_pages * static_cast<std::uint64_t>(page_size)};
    }
};
}

std::unique_ptr<contracts::ProcessMetricsProvider> make_process_metrics_provider() {
    return std::make_unique<LinuxProcessMetricsProvider>();
}
} // namespace kotavel::platform::detail
