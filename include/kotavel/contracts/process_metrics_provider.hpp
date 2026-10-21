#pragma once

#include <chrono>
#include <cstdint>
#include <optional>

namespace kotavel::contracts {

// Datos nativos del proceso. El núcleo deriva porcentajes y tiempos muestreados.
struct ProcessMetricsSnapshot {
    std::chrono::nanoseconds cpu_time{};
    std::uint64_t resident_memory_bytes = 0;
};

// Puerto para leer métricas del proceso actual sin exponer APIs del sistema operativo.
class ProcessMetricsProvider {
public:
    virtual ~ProcessMetricsProvider() = default;
    [[nodiscard]] virtual std::optional<ProcessMetricsSnapshot> read() const noexcept = 0;
};

} // namespace kotavel::contracts
