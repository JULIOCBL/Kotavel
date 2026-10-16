#include <kotavel/metrics/metrics.hpp>
#include "process_metrics_internal.hpp"

#include <stdexcept>
#include <utility>

namespace kotavel::metrics {

std::optional<MetricsSample> ProcessMetricsCollector::sample() noexcept {
    std::optional<detail::ProcessSnapshot> current;
    try { current = detail::read_process_snapshot(); }
    catch (...) { return std::nullopt; }
    if (!current) return std::nullopt;

    const auto now = std::chrono::steady_clock::now();
    MetricsSample result{now, std::nullopt, current->resident_memory_bytes};
    if (previous_cpu_time_ && previous_wall_time_) {
        const auto wall_delta = now - *previous_wall_time_;
        const auto cpu_delta = current->process_cpu_time - *previous_cpu_time_;
        if (wall_delta > std::chrono::steady_clock::duration::zero() &&
            cpu_delta >= std::chrono::nanoseconds::zero()) {
            const auto wall_ns = std::chrono::duration<double, std::nano>(wall_delta).count();
            const auto cpu_ns = static_cast<double>(cpu_delta.count());
            result.process_cpu_percent = cpu_ns * 100.0 / wall_ns;
        }
    }
    previous_cpu_time_ = current->process_cpu_time;
    previous_wall_time_ = now;
    return result;
}

MetricsMonitor::~MetricsMonitor() { stop(); }

void MetricsMonitor::start(std::chrono::milliseconds interval,
                               MetricsCallback callback) {
    start(interval, std::move(callback), ProcessMetricsCollector{});
}

void MetricsMonitor::start(std::chrono::milliseconds interval,
                               MetricsCallback callback,
                               ProcessMetricsCollector collector) {
    if (interval < std::chrono::milliseconds(100) || interval > std::chrono::minutes(1))
        throw std::invalid_argument("Metrics sampling interval must be between 100 ms and 60 s");
    if (!callback) throw std::invalid_argument("Metrics monitor requires a callback");

    std::lock_guard lock(state_mutex_);
    if (worker_.joinable()) throw std::logic_error("Metrics monitor is already running");
    stopping_.store(false, std::memory_order_release);
    worker_ = std::thread([this, interval, callback = std::move(callback),
                           collector = std::move(collector)]() mutable {
        run(interval, std::move(callback), std::move(collector));
    });
}

void MetricsMonitor::stop() noexcept {
    {
        std::lock_guard lock(state_mutex_);
        if (!worker_.joinable()) return;
        stopping_.store(true, std::memory_order_release);
        wake_.notify_all();
        // Una parada solicitada desde el callback se completa al volver al ciclo.
        if (worker_.get_id() == std::this_thread::get_id()) return;
    }
    worker_.join();
}

bool MetricsMonitor::running() const noexcept {
    std::lock_guard lock(state_mutex_);
    return worker_.joinable() && !stopping_.load(std::memory_order_acquire);
}

void MetricsMonitor::run(std::chrono::milliseconds interval,
                             MetricsCallback callback,
                             ProcessMetricsCollector collector) noexcept {
    try {
        std::unique_lock wait_lock(wait_mutex_);
        while (!stopping_.load(std::memory_order_acquire)) {
            // Esperar el intervalo evita una segunda lectura duplicada al iniciar.
            if (wake_.wait_for(wait_lock, interval, [this] {
                    return stopping_.load(std::memory_order_acquire);
                })) break;
            if (const auto value = collector.sample()) {
                try { callback(*value); }
                catch (...) { /* Un callback externo nunca debe terminar la aplicación. */ }
            }
        }
    } catch (...) { /* Fallos internos de muestreo tampoco salen del hilo de trabajo. */ }
}

} // namespace kotavel::metrics
