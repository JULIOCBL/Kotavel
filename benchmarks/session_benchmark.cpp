#include <kotavel/http/request.hpp>
#include <kotavel/http/session_middleware.hpp>
#include <kotavel/metrics/metrics.hpp>
#include <kotavel/session/session.hpp>

#include <algorithm>
#include <atomic>
#include <chrono>
#include <cstdint>
#include <filesystem>
#include <iostream>
#include <latch>
#include <memory>
#include <mutex>
#include <exception>
#include <stdexcept>
#include <string>
#include <string_view>
#include <thread>
#include <vector>

using namespace kotavel::http;
using Clock = std::chrono::steady_clock;

namespace {
struct Options final {
    std::size_t clients = 500;
    std::size_t duration_seconds = 5;
    std::size_t batch_window_us = 0;
    enum class Workload { independent, shared, shared_keys } workload = Workload::independent;
};

Options parse_options(int argc, char** argv) {
    Options options;
    if (argc > 5)
        throw std::invalid_argument(
            "usage: kotavel_session_benchmark [clients] [seconds] "
            "[independent|shared|shared-keys] [batch-window-us]");
    if (argc > 1) options.clients = std::stoull(argv[1]);
    if (argc > 2) options.duration_seconds = std::stoull(argv[2]);
    if (argc > 3) {
        const std::string_view workload(argv[3]);
        if (workload == "independent") options.workload = Options::Workload::independent;
        else if (workload == "shared") options.workload = Options::Workload::shared;
        else if (workload == "shared-keys") options.workload = Options::Workload::shared_keys;
        else throw std::invalid_argument("workload must be independent, shared or shared-keys");
    }
    if (argc > 4) options.batch_window_us = std::stoull(argv[4]);
    if (options.clients == 0 || options.duration_seconds == 0 || options.clients > 20000 ||
        options.duration_seconds > 3600 || options.batch_window_us > 10000)
        throw std::invalid_argument(
            "clients must be 1..20000, seconds 1..3600 and batch window 0..10000 microseconds");
    return options;
}

Request request_with_cookie(std::string cookie) {
    RequestData data;
    data.kind = RouteKind::web;
    data.method = Method::get;
    data.path = "/session-benchmark";
    data.raw_method = "GET";
    data.headers.emplace("Cookie", std::move(cookie));
    return Request(std::move(data));
}

std::string cookie_id(const Response& response) {
    const auto found = response.headers.find("Set-Cookie");
    if (found == response.headers.end()) throw std::runtime_error("session response has no cookie");
    const auto begin = found->second.find('=') + 1;
    const auto end = found->second.find(';', begin);
    if (begin == 0 || end == std::string::npos) throw std::runtime_error("invalid session cookie");
    return found->second.substr(begin, end - begin);
}

double percentile(const std::vector<double>& sorted, double fraction) {
    if (sorted.empty()) return 0;
    const auto index = static_cast<std::size_t>(fraction * static_cast<double>(sorted.size() - 1));
    return sorted[index];
}

int run(const Options& options) {
    const auto root = std::filesystem::temp_directory_path() /
        ("kotavel-session-benchmark-" + std::to_string(
            Clock::now().time_since_epoch().count()));
    std::filesystem::create_directories(root);

    SessionConfig config;
    config.driver = "sqlite";
    config.database = root / "sessions.sqlite3";
    config.cookie = "kotavel_session";
    config.cache_max_sessions = std::max<std::size_t>(2048, options.clients + 64);
    config.cache_ttl = std::chrono::seconds(options.duration_seconds + 60);
    auto backing = std::make_shared<SQLiteSessionStore>(config.database, 8, 1024, 5000,
        std::chrono::microseconds(options.batch_window_us));
    auto cache = std::make_shared<CachedSessionStore>(backing, config.cache_max_sessions,
        config.cache_max_bytes, config.cache_ttl);
    auto middleware = start_session(config, cache);

    const bool shared_session = options.workload != Options::Workload::independent;
    const auto session_count = shared_session ? std::size_t{1} : options.clients;
    std::vector<std::string> ids;
    ids.reserve(session_count);
    for (std::size_t index = 0; index < session_count; ++index) {
        const auto response = middleware(request_with_cookie(""), [] {
            return Response{200, "text/plain", "ready"};
        });
        if (response.status != 200) throw std::runtime_error("could not create benchmark session");
        ids.push_back(cookie_id(response));
    }

    const auto rounds = options.duration_seconds * 2;
    std::latch ready(static_cast<std::ptrdiff_t>(options.clients));
    std::latch start_gate(1);
    std::vector<std::vector<double>> per_client_latency(options.clients);
    std::vector<std::vector<double>> per_client_write_latency(options.clients);
    std::vector<std::thread> workers;
    workers.reserve(options.clients);
    std::vector<std::size_t> failed_responses(options.clients, 0);
    std::vector<std::size_t> conflicts(options.clients, 0);
    std::vector<std::size_t> successful_writes(options.clients, 0);
    std::vector<std::exception_ptr> errors(options.clients);
    Clock::time_point start;

    for (std::size_t client = 0; client < options.clients; ++client) {
        workers.emplace_back([&, client] {
            ready.count_down();
            start_gate.wait();
            auto& samples = per_client_latency[client];
            samples.reserve(rounds);
            auto& write_samples = per_client_write_latency[client];
            write_samples.reserve(rounds / 5 + 1);
            try {
                const auto client_phase = std::chrono::milliseconds(
                    static_cast<std::int64_t>(500 * client / options.clients));
                for (std::size_t round = 0; round < rounds; ++round) {
                    std::this_thread::sleep_until(start + std::chrono::milliseconds(500 * round) +
                                                  client_phase);
                    const auto begin = Clock::now();
                    // Offset write rounds per client to model a steady offered
                    // mix instead of synchronizing every client's writes.
                    const bool write_request = (client + round) % 5 == 4;
                    const auto session_index = shared_session ? std::size_t{0} : client;
                    const auto write_key = options.workload == Options::Workload::shared_keys
                        ? "writes." + std::to_string(client) : std::string("writes");
                    const auto response = middleware(
                        request_with_cookie("kotavel_session=" + ids[session_index]),
                        [write_request, write_key] {
                            if (write_request) {
                                (void)Session::increment(write_key);
                            } else {
                                (void)Session::get<std::uint64_t>(write_key, 0);
                            }
                            return Response{200, "text/plain", "ok"};
                        });
                    samples.push_back(std::chrono::duration<double, std::milli>(
                        Clock::now() - begin).count());
                    if (write_request)
                        write_samples.push_back(std::chrono::duration<double, std::milli>(
                            Clock::now() - begin).count());
                    if (response.status == 409) ++conflicts[client];
                    else if (response.status != 200) ++failed_responses[client];
                    else if (write_request) ++successful_writes[client];
                }
            } catch (...) {
                errors[client] = std::current_exception();
            }
        });
    }

    ready.wait();
    kotavel::metrics::ProcessMetricsCollector process_metrics;
    const auto initial_process_sample = process_metrics.sample();
    std::atomic<std::uint64_t> peak_rss_bytes{
        initial_process_sample ? initial_process_sample->resident_memory_bytes : 0};
    std::mutex process_metrics_mutex;
    double cpu_percent_total = 0.0;
    double cpu_percent_peak = 0.0;
    std::size_t cpu_samples = 0;
    kotavel::metrics::MetricsMonitor process_metrics_monitor;
    process_metrics_monitor.start(std::chrono::milliseconds(100),
        [&](const kotavel::metrics::MetricsSample& sample) {
            auto peak = peak_rss_bytes.load(std::memory_order_relaxed);
            while (peak < sample.resident_memory_bytes &&
                   !peak_rss_bytes.compare_exchange_weak(peak, sample.resident_memory_bytes,
                                                        std::memory_order_relaxed)) {}
            if (!sample.process_cpu_percent) return;
            std::lock_guard guard(process_metrics_mutex);
            cpu_percent_total += *sample.process_cpu_percent;
            cpu_percent_peak = std::max(cpu_percent_peak, *sample.process_cpu_percent);
            ++cpu_samples;
        }, std::move(process_metrics));
    start = Clock::now();
    start_gate.count_down();
    for (auto& worker : workers) worker.join();
    const auto elapsed_seconds = std::chrono::duration<double>(Clock::now() - start).count();
    process_metrics_monitor.stop();
    const double cpu_percent_mean = cpu_samples == 0 ? 0.0 : cpu_percent_total / cpu_samples;

    for (const auto& error : errors) if (error) std::rethrow_exception(error);
    std::vector<double> latency;
    std::vector<double> write_latency;
    latency.reserve(options.clients * rounds);
    write_latency.reserve(options.clients * rounds / 5);
    std::size_t failures = 0;
    std::size_t total_conflicts = 0;
    std::size_t committed_writes = 0;
    for (std::size_t client = 0; client < options.clients; ++client) {
        failures += failed_responses[client];
        total_conflicts += conflicts[client];
        committed_writes += successful_writes[client];
        latency.insert(latency.end(), per_client_latency[client].begin(), per_client_latency[client].end());
        write_latency.insert(write_latency.end(), per_client_write_latency[client].begin(),
                             per_client_write_latency[client].end());
    }
    std::sort(latency.begin(), latency.end());
    std::sort(write_latency.begin(), write_latency.end());

    const auto cache_stats = cache->statistics();
    const auto writer_stats = backing->statistics();
    const auto total_requests = options.clients * rounds;
    std::size_t durable_session_payload_bytes = 0;
    for (std::size_t session_index = 0; session_index < ids.size(); ++session_index) {
        const auto loaded = backing->load(ids[session_index]);
        if (!loaded) throw std::runtime_error("persisted benchmark session disappeared");
        durable_session_payload_bytes += loaded->estimated_values_bytes;
        std::uint64_t writes = 0;
        std::uint64_t expected_writes = 0;
        if (options.workload == Options::Workload::shared_keys) {
            for (const auto& [key, value] : loaded->values) {
                if (!key.starts_with("writes.")) continue;
                const auto& stored = value.value();
                if (const auto* number = std::get_if<std::uint64_t>(&stored)) writes += *number;
                else if (const auto* signed_number = std::get_if<std::int64_t>(&stored);
                         signed_number && *signed_number >= 0)
                    writes += static_cast<std::uint64_t>(*signed_number);
            }
            expected_writes = committed_writes;
        } else {
            const auto value = loaded->values.find("writes");
            if (value != loaded->values.end()) {
                const auto& stored = value->second.value();
                if (const auto* number = std::get_if<std::uint64_t>(&stored)) writes = *number;
                else if (const auto* signed_number = std::get_if<std::int64_t>(&stored);
                         signed_number && *signed_number >= 0)
                    writes = static_cast<std::uint64_t>(*signed_number);
            }
        }
        if (options.workload == Options::Workload::shared) {
            expected_writes = committed_writes;
        } else if (options.workload == Options::Workload::independent) {
            for (std::size_t round = 0; round < rounds; ++round)
                expected_writes += (session_index + round) % 5 == 4;
        }
        if (writes != expected_writes)
            throw std::runtime_error("durable session write count differs from accepted writes");
    }

    const auto cache_requests = cache_stats.hits + cache_stats.misses;
    const auto hit_rate = cache_requests == 0 ? 0.0 :
        100.0 * static_cast<double>(cache_stats.hits) / static_cast<double>(cache_requests);
    std::cout << "backend,workload,clients,sessions,offered_rps,requests,writes_committed,conflicts,failures,elapsed_s,"
                 "p50_ms,p95_ms,p99_ms,write_p50_ms,write_p95_ms,write_p99_ms,"
                 "throughput_rps,cache_hits,cache_misses,cache_hit_rate_pct,cache_lock_contended,"
                 "cache_lock_wait_mean_ms,cache_lock_wait_max_ms,resident_bytes,"
                 "writer_jobs,queue_admission_mean_ms,writer_queue_mean_ms,writer_queue_max_ms,"
                 "writer_service_mean_ms,writer_service_max_ms,writer_peak_queue,"
                 "write_batches,batched_write_jobs,mean_batch_size,max_batch_size,"
                 "process_cpu_mean_pct,process_cpu_peak_pct,process_metric_samples,"
                 "process_peak_rss_bytes,process_metrics_interval_ms,"
                 "durable_session_payload_bytes,durable_payload_bytes_per_session\n";
    const auto workload_name = options.workload == Options::Workload::independent ? "independent" :
        options.workload == Options::Workload::shared ? "shared" : "shared-keys";
    std::cout << "sqlite," << workload_name << ','
              << options.clients << ',' << session_count << ',' << options.clients * 2 << ',' << total_requests << ','
              << committed_writes << ',' << total_conflicts << ',' << failures << ',' << elapsed_seconds << ','
              << percentile(latency, 0.50) << ',' << percentile(latency, 0.95) << ','
              << percentile(latency, 0.99) << ',' << percentile(write_latency, 0.50) << ','
              << percentile(write_latency, 0.95) << ',' << percentile(write_latency, 0.99) << ','
              << total_requests / elapsed_seconds << ','
              << cache_stats.hits << ',' << cache_stats.misses << ',' << hit_rate << ','
              << cache_stats.contended_lock_acquisitions << ','
              << (cache_stats.contended_lock_acquisitions == 0 ? 0.0 :
                  static_cast<double>(cache_stats.lock_wait_ns) /
                      cache_stats.contended_lock_acquisitions / 1e6) << ','
              << static_cast<double>(cache_stats.max_lock_wait_ns) / 1e6 << ','
              << cache_stats.resident_bytes << ',' << writer_stats.write_jobs << ','
              << (writer_stats.write_jobs == 0 ? 0.0 :
                  static_cast<double>(writer_stats.admission_wait_ns) / writer_stats.write_jobs / 1e6) << ','
              << (writer_stats.write_jobs == 0 ? 0.0 :
                  static_cast<double>(writer_stats.queue_wait_ns) / writer_stats.write_jobs / 1e6) << ','
              << static_cast<double>(writer_stats.max_queue_wait_ns) / 1e6 << ','
              << (writer_stats.write_jobs == 0 ? 0.0 :
                  static_cast<double>(writer_stats.service_ns) / writer_stats.write_jobs / 1e6) << ','
              << static_cast<double>(writer_stats.max_service_ns) / 1e6 << ','
              << writer_stats.peak_queue_depth << ',' << writer_stats.write_batches << ','
              << writer_stats.batched_write_jobs << ','
              << (writer_stats.write_batches == 0 ? 0.0 :
                  static_cast<double>(writer_stats.batched_write_jobs) / writer_stats.write_batches) << ','
              << writer_stats.max_batch_size << ',' << cpu_percent_mean << ','
              << cpu_percent_peak << ',' << cpu_samples << ','
              << peak_rss_bytes.load(std::memory_order_relaxed) << ",100,"
              << durable_session_payload_bytes << ','
              << (session_count == 0 ? 0.0 :
                  static_cast<double>(durable_session_payload_bytes) / session_count) << '\n';

    std::error_code ignored;
    std::filesystem::remove_all(root, ignored);
    return failures == 0 && total_conflicts == 0 ? 0 : 1;
}
}

int main(int argc, char** argv) {
    try {
        return run(parse_options(argc, argv));
    } catch (const std::exception& error) {
        std::cerr << "session benchmark failed: " << error.what() << '\n';
        return 1;
    }
}
