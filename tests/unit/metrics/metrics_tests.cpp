#include <kotavel/metrics/metrics.hpp>
#include "metrics/metrics_endpoint.hpp"

#include <atomic>
#include <chrono>
#include <cmath>
#include <condition_variable>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <mutex>
#include <set>
#include <stdexcept>
#include <thread>
#include <utility>

namespace {
using namespace std::chrono_literals;
using kotavel::metrics::MetricsMonitor;
using kotavel::metrics::ProcessMetricsCollector;

const std::set<std::uint64_t> live_processes{202};
bool fake_process_is_running(std::uint64_t process_id) noexcept {
    return live_processes.contains(process_id);
}

class FixedMetricsProvider final : public kotavel::contracts::ProcessMetricsProvider {
public:
    mutable std::size_t reads = 0;
    std::optional<kotavel::contracts::ProcessMetricsSnapshot> read() const noexcept override {
        ++reads;
        return kotavel::contracts::ProcessMetricsSnapshot{
            std::chrono::milliseconds(25 * static_cast<int>(reads)), 1024 * reads};
    }
};

void check(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

template<class Exception, class Action>
void expect_exception(Action&& action, const char* message) {
    try {
        std::forward<Action>(action)();
    } catch (const Exception&) {
        return;
    }
    throw std::runtime_error(message);
}

void check_process_sample_contract() {
    ProcessMetricsCollector collector;
    const auto first = collector.sample();
    check(first.has_value(), "First process sample was unavailable");
    check(first->resident_memory_bytes > 0, "Resident memory must be positive");
    check(!first->process_cpu_percent, "The first CPU sample must be empty");

    // CPU is calculated from two readings; no exact operating-system value is assumed.
    std::this_thread::sleep_for(30ms);
    const auto second = collector.sample();
    check(second.has_value(), "Second process sample was unavailable");
    check(second->resident_memory_bytes > 0, "Second resident memory must be positive");
    check(second->captured_at > first->captured_at, "Sample timestamps must increase");
    check(second->process_cpu_percent.has_value(), "Second CPU sample must be present");
    check(std::isfinite(*second->process_cpu_percent) && *second->process_cpu_percent >= 0.0,
          "CPU percentage must be finite and non-negative");
}

void check_process_metrics_provider_contract() {
    auto provider = std::make_shared<FixedMetricsProvider>();
    ProcessMetricsCollector collector(provider);
    const auto first = collector.sample();
    // The collector uses a real steady clock. Keep this test independent of how
    // quickly an optimized runner can execute two consecutive samples.
    std::this_thread::sleep_for(10ms);
    const auto second = collector.sample();
    check(first && second, "Injected process metrics provider did not produce samples");
    check(provider->reads == 2, "Collector did not delegate both reads to its provider");
    check(first->resident_memory_bytes == 1024 && second->resident_memory_bytes == 2048,
          "Collector did not preserve provider memory readings");
    check(second->process_cpu_percent.has_value(),
          "Collector did not derive CPU percentage from provider snapshots");
    expect_exception<std::invalid_argument>(
        [] { ProcessMetricsCollector invalid(nullptr); },
        "Collector accepted a null platform provider");
}

void check_monitor_validation() {
    MetricsMonitor monitor;
    const auto noop = [](const auto&) {};
    expect_exception<std::invalid_argument>(
        [&] { monitor.start(99ms, noop); }, "Monitor accepted an interval below 100 ms");
    expect_exception<std::invalid_argument>(
        [&] { monitor.start(60001ms, noop); }, "Monitor accepted an interval above 60 s");
    expect_exception<std::invalid_argument>(
        [&] { monitor.start(100ms, {}); }, "Monitor accepted an empty callback");

    // Both documented interval endpoints are valid and stop must wake the worker.
    monitor.start(100ms, noop);
    check(monitor.running(), "Monitor did not enter the running state");
    monitor.stop();
    check(!monitor.running(), "Monitor stayed active after stop");
    monitor.stop(); // Stop is safe to repeat.
    monitor.start(60s, noop);
    monitor.stop();
    check(!monitor.running(), "Monitor did not stop at the maximum interval");
}

void check_monitor_callback_and_shutdown() {
    std::mutex mutex;
    std::condition_variable ready;
    std::atomic<int> callbacks{0};
    std::atomic<bool> invalid_sample{false};
    ProcessMetricsCollector collector;
    check(collector.sample().has_value(), "Could not seed the monitor collector");

    MetricsMonitor monitor;
    monitor.start(100ms, [&](const auto& sample) {
        if (sample.resident_memory_bytes == 0 || !sample.process_cpu_percent ||
            !std::isfinite(*sample.process_cpu_percent) || *sample.process_cpu_percent < 0.0)
            invalid_sample.store(true, std::memory_order_relaxed);

        const auto count = callbacks.fetch_add(1, std::memory_order_relaxed) + 1;
        // Pair notification with the wait mutex so the test cannot miss a wakeup.
        {
            std::lock_guard lock(mutex);
        }
        ready.notify_one();
        if (count == 1) throw std::runtime_error("Callback failure must be contained");
    }, std::move(collector));

    expect_exception<std::logic_error>(
        [&] { monitor.start(100ms, [](const auto&) {}); },
        "Monitor accepted a second start while already active");
    {
        std::unique_lock lock(mutex);
        check(ready.wait_for(lock, 5s, [&] {
            return callbacks.load(std::memory_order_relaxed) >= 2;
        }), "Monitor did not continue after a callback threw");
    }
    check(monitor.running(), "Monitor stopped after a callback exception");
    check(!invalid_sample.load(std::memory_order_relaxed), "Monitor delivered an invalid sample");

    monitor.stop();
    check(!monitor.running(), "Monitor stayed active after joining its worker");
    const auto callbacks_after_stop = callbacks.load(std::memory_order_relaxed);
    std::this_thread::sleep_for(150ms);
    check(callbacks.load(std::memory_order_relaxed) == callbacks_after_stop,
          "Callback ran after stop returned");
    monitor.stop();

    // A fully stopped monitor can be started again with a fresh collector.
    monitor.start(100ms, [&](const auto&) {
        callbacks.fetch_add(1, std::memory_order_relaxed);
        {
            std::lock_guard lock(mutex);
        }
        ready.notify_one();
    });
    {
        std::unique_lock lock(mutex);
        check(ready.wait_for(lock, 5s, [&] {
            return callbacks.load(std::memory_order_relaxed) > callbacks_after_stop;
        }), "Monitor could not restart after stop");
    }
    monitor.stop();
}

void check_stop_from_callback() {
    std::mutex mutex;
    std::condition_variable ready;
    bool callback_returned = false;
    MetricsMonitor monitor;
    monitor.start(100ms, [&](const auto&) {
        // Self-stop requests cancellation; the owner thread performs the join later.
        monitor.stop();
        {
            std::lock_guard lock(mutex);
            callback_returned = true;
        }
        ready.notify_one();
    });

    {
        std::unique_lock lock(mutex);
        check(ready.wait_for(lock, 5s, [&] { return callback_returned; }),
              "Callback could not request monitor shutdown");
    }
    check(!monitor.running(), "Self-stop did not change the running state");
    monitor.stop(); // Join from the owner thread after the callback returns.
}

void check_metrics_endpoint_cleanup() {
    using kotavel::metrics::detail::endpoint_process_id;
    using kotavel::metrics::detail::remove_stale_endpoints;

    check(endpoint_process_id("kotavel-202.endpoint") == 202,
          "Endpoint parser did not read a valid process identifier");
    check(!endpoint_process_id("other-202.endpoint"),
          "Endpoint parser accepted an unexpected prefix");
    check(!endpoint_process_id("kotavel-0.endpoint"),
          "Endpoint parser accepted PID zero");
    check(!endpoint_process_id("kotavel-202.extra.endpoint"),
          "Endpoint parser accepted a nonnumeric PID");
    check(!endpoint_process_id("kotavel-202.txt"),
          "Endpoint parser accepted an unexpected suffix");

    const auto root = std::filesystem::temp_directory_path() /
        ("kotavel-metrics-endpoint-tests-" + std::to_string(
            std::chrono::steady_clock::now().time_since_epoch().count()));
    const auto write_file = [](const std::filesystem::path& path) {
        std::ofstream output(path);
        if (!output) throw std::runtime_error("Could not create metrics endpoint test file");
        output << "12345\n";
    };

    try {
        std::filesystem::create_directories(root);
        const auto stale = root / "kotavel-101.endpoint";
        const auto live = root / "kotavel-202.endpoint";
        const auto current = root / "kotavel-303.endpoint";
        const auto malformed = root / "kotavel-not-a-pid.endpoint";
        const auto unrelated = root / "keep.txt";
        write_file(stale);
        write_file(live);
        write_file(current);
        write_file(malformed);
        write_file(unrelated);

        remove_stale_endpoints(root, 303, fake_process_is_running);
        check(!std::filesystem::exists(stale),
              "Cleanup did not remove an endpoint owned by a dead process");
        check(std::filesystem::exists(live),
              "Cleanup removed an endpoint owned by a live process");
        check(std::filesystem::exists(current),
              "Cleanup removed the current process endpoint");
        check(std::filesystem::exists(malformed),
              "Cleanup removed a file that is not a valid PID endpoint");
        check(std::filesystem::exists(unrelated),
              "Cleanup removed an unrelated file in the metrics directory");

        remove_stale_endpoints(root / "missing", 303, fake_process_is_running);
    } catch (...) {
        std::error_code ignored;
        std::filesystem::remove_all(root, ignored);
        throw;
    }
    std::error_code ignored;
    std::filesystem::remove_all(root, ignored);
}
}

int main() {
    try {
        check_process_sample_contract();
        check_process_metrics_provider_contract();
        check_monitor_validation();
        check_monitor_callback_and_shutdown();
        check_stop_from_callback();
        check_metrics_endpoint_cleanup();
        std::cout << "Metrics tests passed\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
