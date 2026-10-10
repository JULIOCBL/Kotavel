#include <kotavel/foundation/application.hpp>
#include <algorithm>
#include <chrono>
#include <filesystem>
#include <iostream>
#include <vector>

using Clock = std::chrono::steady_clock;
struct Directory {
    std::filesystem::path path = std::filesystem::temp_directory_path() /
        ("kotavel-benchmark-" + std::to_string(Clock::now().time_since_epoch().count()));
    ~Directory() { std::error_code ec; std::filesystem::remove_all(path, ec); }
};
template<class Action> void measure(const char* name, std::size_t count, Action action, std::size_t batch = 1) {
    for (int i = 0; i < 100; ++i) action(); // Calentamiento excluido.
    std::vector<double> samples; samples.reserve(count);
    const auto start = Clock::now();
    for (std::size_t i = 0; i < count; ++i) {
        const auto begin = Clock::now();
        for (std::size_t operation = 0; operation < batch; ++operation) action();
        samples.push_back(std::chrono::duration<double, std::nano>(Clock::now() - begin).count() / batch);
    }
    const auto seconds = std::chrono::duration<double>(Clock::now() - start).count();
    std::sort(samples.begin(), samples.end());
    std::cout << name << ',' << count << ',' << batch << ',' << samples[count / 2] << ','
              << samples[count * 95 / 100] << ',' << samples[count * 99 / 100] << ','
              << count * batch / seconds << '\n';
}
int main() {
    using namespace kotavel;
    try {
        Directory root;
        LogOptions options{root.path}; options.console = false;
        options.min_level = LogLevel::emergency;
        ErrorHandlingOptions errors; errors.capture_traces = false;
        {
            Application app(options, errors); app.boot();
            std::cout << "scenario,samples,operations_per_sample,p50_ns,p95_ns,p99_ns,operations_per_second\n";
            measure("successful_operation", 1000, [&] {
                if (app.execute("bench", [] {}) != OperationResult::success) throw std::runtime_error("failure");
            }, 100);
            measure("error_without_trace_filtered", 5000, [&] {
                if (app.execute("bench", [] { throw RecoverableError("benchmark"); }) !=
                    OperationResult::recovered_error) throw std::runtime_error("failure");
            });
        }
        {
            errors.capture_traces = true;
            Application app(options, errors); app.boot();
            measure("error_with_trace_filtered", 5000, [&] {
                if (app.execute("bench", [] { throw RecoverableError("benchmark"); }) !=
                    OperationResult::recovered_error) throw std::runtime_error("failure");
            });
        }
        {
            options.min_level = LogLevel::info;
            Logger logger(options);
            measure("file_report", 2000, [&] {
                if (!logger.report("benchmark", "bench", "message")) throw std::runtime_error("failure");
            });
            logger.configure_report_throttling({1, std::chrono::hours{1}, 256});
            measure("throttled_report", 10000, [&] {
                if (!logger.report("benchmark", "bench", "message")) throw std::runtime_error("failure");
            }, 10);
        }
        return 0;
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
