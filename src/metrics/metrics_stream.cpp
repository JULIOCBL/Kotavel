#include <kotavel/metrics/metrics.hpp>

#include "../platform/metrics_stream_transport.hpp"

#include <atomic>
#include <cmath>
#include <fstream>
#include <iomanip>
#include <locale>
#include <mutex>
#include <sstream>
#include <stdexcept>
#include <string>
#include <thread>
#include <utility>

namespace kotavel::metrics {

struct MetricsStream::Impl {
    std::filesystem::path endpoint;
    std::unique_ptr<platform::detail::LocalMetricsServer> server =
        platform::detail::make_local_metrics_server();
    std::thread worker;
    std::atomic<bool> stopping{true};
    std::atomic<bool> active{false};
    std::mutex sample_mutex;
    std::string startup_message;
    std::string latest_sample;
    std::uint64_t generation = 0;

    void serve() noexcept {
        std::unique_ptr<platform::detail::LocalMetricsConnection> client;
        std::uint64_t sent_generation = 0;
        bool sent_startup = false;
        try {
            while (!stopping.load(std::memory_order_acquire)) {
                if (auto accepted = server->accept_for(std::chrono::milliseconds(100))) {
                    client = std::move(accepted);
                    sent_generation = 0;
                    sent_startup = false;
                }
                if (!client) continue;

                std::string payload;
                std::uint64_t current_generation = 0;
                {
                    std::lock_guard lock(sample_mutex);
                    current_generation = generation;
                    if (!sent_startup) payload = startup_message;
                }
                if (!payload.empty()) {
                    if (!client->send(payload)) {
                        client.reset();
                        continue;
                    }
                    sent_startup = true;
                }

                {
                    std::lock_guard lock(sample_mutex);
                    current_generation = generation;
                    if (current_generation != sent_generation) payload = latest_sample;
                    else payload.clear();
                }
                if (!payload.empty()) {
                    if (!client->send(payload)) client.reset();
                    else sent_generation = current_generation;
                }
            }
        } catch (...) {
            // Allocation or transport failures only stop the optional metrics stream.
        }
        client.reset();
        active.store(false, std::memory_order_release);
    }
};

MetricsStream::MetricsStream() : impl_(std::make_unique<Impl>()) {}
MetricsStream::~MetricsStream() { stop(); }

void MetricsStream::start(const std::filesystem::path& project_root) {
    if (impl_->active.load(std::memory_order_acquire) || impl_->worker.joinable())
        throw std::logic_error("Metrics stream is already running");

    const auto directory = project_root / "storage" / "framework" / "metrics";
    std::filesystem::create_directories(directory);
    const auto process_id = platform::detail::current_process_id();
    impl_->endpoint = directory / ("kotavel-" + std::to_string(process_id) + ".endpoint");
    const auto port = impl_->server->start_loopback();
    {
        std::ofstream descriptor(impl_->endpoint, std::ios::trunc);
        if (!descriptor || !(descriptor << port << '\n')) {
            stop();
            throw std::runtime_error("Could not publish local metrics endpoint");
        }
    }
    {
        std::lock_guard lock(impl_->sample_mutex);
        impl_->startup_message.clear();
        impl_->latest_sample.clear();
        impl_->generation = 0;
    }
    impl_->stopping.store(false, std::memory_order_release);
    try {
        impl_->worker = std::thread([state = impl_.get()] { state->serve(); });
    } catch (...) {
        stop();
        throw;
    }
    impl_->active.store(true, std::memory_order_release);
}

void MetricsStream::publish(const MetricsSample& sample) noexcept {
    try {
        std::ostringstream message;
        message.imbue(std::locale::classic());
        message << std::fixed << std::setprecision(1)
                << "{\"type\":\"sample\",\"cpu_percent\":";
        if (sample.process_cpu_percent && std::isfinite(*sample.process_cpu_percent) &&
            *sample.process_cpu_percent >= 0.0)
            message << *sample.process_cpu_percent;
        else message << "null";
        message << ",\"resident_memory_bytes\":" << sample.resident_memory_bytes << "}\n";
        std::lock_guard lock(impl_->sample_mutex);
        impl_->latest_sample = std::move(message).str();
        ++impl_->generation;
    } catch (...) {
        // A metrics transport failure must never interrupt the application.
    }
}

void MetricsStream::publish_startup(const StartupMetrics& metrics) noexcept {
    try {
        const auto milliseconds = [](std::chrono::nanoseconds value) {
            return std::chrono::duration<double, std::milli>(value).count();
        };
        std::ostringstream message;
        message.imbue(std::locale::classic());
        message << std::fixed << std::setprecision(2)
                << "{\"type\":\"startup\",\"environment_ms\":" << milliseconds(metrics.environment)
                << ",\"configuration_ms\":" << milliseconds(metrics.configuration)
                << ",\"settings_ms\":" << milliseconds(metrics.settings)
                << ",\"application_boot_ms\":" << milliseconds(metrics.application_boot)
                << ",\"total_ms\":" << milliseconds(metrics.total) << "}\n";
        std::lock_guard lock(impl_->sample_mutex);
        impl_->startup_message = std::move(message).str();
    } catch (...) {
        // A metrics transport failure must never interrupt the application.
    }
}

void MetricsStream::stop() noexcept {
    if (!impl_) return;
    impl_->stopping.store(true, std::memory_order_release);
    if (impl_->worker.joinable()) impl_->worker.join();
    impl_->server->stop();
    if (!impl_->endpoint.empty()) {
        std::error_code error;
        std::filesystem::remove(impl_->endpoint, error);
        impl_->endpoint.clear();
    }
    impl_->active.store(false, std::memory_order_release);
}

bool MetricsStream::running() const noexcept {
    return impl_ && impl_->active.load(std::memory_order_acquire);
}

} // namespace kotavel::metrics
