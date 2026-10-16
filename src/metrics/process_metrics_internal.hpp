#pragma once

#include <chrono>
#include <cstdint>
#include <optional>

namespace kotavel::metrics::detail {
struct ProcessSnapshot {
    std::chrono::nanoseconds process_cpu_time{};
    std::uint64_t resident_memory_bytes = 0;
};

// Cada plataforma implementa esta lectura privada del proceso actual.
[[nodiscard]] std::optional<ProcessSnapshot> read_process_snapshot();
}
