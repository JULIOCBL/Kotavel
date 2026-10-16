#include <kotavel/metrics/performance.hpp>
#include "process_metrics_internal.hpp"

#include <sys/resource.h>
#include <unistd.h>

#if defined(__APPLE__)
#include <mach/mach.h>
#else
#include <fstream>
#endif

namespace kotavel::metrics::detail {
namespace {
std::chrono::nanoseconds cpu_time(const rusage& usage) noexcept {
    const auto user = std::chrono::seconds(usage.ru_utime.tv_sec) +
                      std::chrono::microseconds(usage.ru_utime.tv_usec);
    const auto system = std::chrono::seconds(usage.ru_stime.tv_sec) +
                        std::chrono::microseconds(usage.ru_stime.tv_usec);
    return std::chrono::duration_cast<std::chrono::nanoseconds>(user + system);
}
}

std::optional<ProcessSnapshot> read_process_snapshot() {
    rusage usage{};
    if (getrusage(RUSAGE_SELF, &usage) != 0) return std::nullopt;
    std::uint64_t resident = 0;
#if defined(__APPLE__)
    mach_task_basic_info_data_t info{};
    mach_msg_type_number_t count = MACH_TASK_BASIC_INFO_COUNT;
    if (task_info(mach_task_self(), MACH_TASK_BASIC_INFO,
                  reinterpret_cast<task_info_t>(&info), &count) != KERN_SUCCESS)
        return std::nullopt;
    resident = static_cast<std::uint64_t>(info.resident_size);
#else
    // /proc/self/statm informa páginas residentes actuales; ru_maxrss sería solo el pico.
    std::ifstream statm("/proc/self/statm");
    std::uint64_t total_pages = 0;
    std::uint64_t resident_pages = 0;
    if (!(statm >> total_pages >> resident_pages)) return std::nullopt;
    const long page_size = sysconf(_SC_PAGESIZE);
    if (page_size <= 0) return std::nullopt;
    resident = resident_pages * static_cast<std::uint64_t>(page_size);
#endif
    return ProcessSnapshot{cpu_time(usage), resident};
}
} // namespace kotavel::metrics::detail
