#include <kotavel/metrics/metrics.hpp>

#include <atomic>
#include <chrono>
#include <cmath>
#include <condition_variable>
#include <iostream>
#include <mutex>
#include <stdexcept>
#include <thread>
#include <utility>

namespace {
using namespace std::chrono_literals;
using kotavel::metrics::MetricsMonitor;
using kotavel::metrics::ProcessMetricsCollector;

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
}

int main() {
    try {
        check_process_sample_contract();
        check_monitor_validation();
        check_monitor_callback_and_shutdown();
        check_stop_from_callback();
        std::cout << "Metrics tests passed\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
