#pragma once

#include <chrono>
#include <condition_variable>
#include <cstdint>
#include <functional>
#include <atomic>
#include <filesystem>
#include <memory>
#include <mutex>
#include <optional>
#include <thread>

namespace kotavel::metrics {

// CPU se expresa como porcentaje de un núcleo; puede superar 100 % en varios núcleos.
struct PerformanceSample {
    std::chrono::steady_clock::time_point captured_at;
    std::optional<double> process_cpu_percent;
    std::uint64_t resident_memory_bytes = 0;
};

struct StartupMetrics;

// Publica la última muestra a un cliente local sin bloquear al recolector.
class PerformanceStream final {
public:
    PerformanceStream();
    ~PerformanceStream();
    PerformanceStream(const PerformanceStream&) = delete;
    PerformanceStream& operator=(const PerformanceStream&) = delete;

    // Crea un listener solo en loopback y anuncia su puerto bajo storage/framework/metrics/.
    void start(const std::filesystem::path& project_root);
    void publish_startup(const StartupMetrics& metrics) noexcept;
    void publish(const PerformanceSample& sample) noexcept;
    void stop() noexcept;
    [[nodiscard]] bool running() const noexcept;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};

// Una instancia mide el proceso completo y solo debe consultarse desde un hilo.
class ProcessMetricsCollector final {
public:
    [[nodiscard]] std::optional<PerformanceSample> sample() noexcept;
private:
    std::optional<std::chrono::nanoseconds> previous_cpu_time_;
    std::optional<std::chrono::steady_clock::time_point> previous_wall_time_;
};

using PerformanceCallback = std::function<void(const PerformanceSample&)>;

// El callback corre en el hilo del monitor; el propietario gestiona start/stop.
class PerformanceMonitor final {
public:
    PerformanceMonitor() = default;
    ~PerformanceMonitor();
    PerformanceMonitor(const PerformanceMonitor&) = delete;
    PerformanceMonitor& operator=(const PerformanceMonitor&) = delete;

    void start(std::chrono::milliseconds interval, PerformanceCallback callback);
    // Permite continuar el cálculo desde una muestra inicial del mismo proceso.
    void start(std::chrono::milliseconds interval, PerformanceCallback callback,
               ProcessMetricsCollector collector);
    void stop() noexcept;
    [[nodiscard]] bool running() const noexcept;
private:
    void run(std::chrono::milliseconds interval, PerformanceCallback callback,
             ProcessMetricsCollector collector) noexcept;

    mutable std::mutex state_mutex_;
    std::mutex wait_mutex_;
    std::condition_variable wake_;
    std::atomic<bool> stopping_{true};
    std::thread worker_;
};

struct StartupMetrics {
    std::chrono::nanoseconds environment{};
    std::chrono::nanoseconds configuration{};
    std::chrono::nanoseconds settings{};
    std::chrono::nanoseconds application_boot{};
    std::chrono::nanoseconds total{};
};

struct PerformanceOptions {
    bool enabled = false;
    std::chrono::milliseconds interval{1000};
    bool console = true;
    bool startup = true;
    bool stream = false;
};

} // namespace kotavel::metrics
