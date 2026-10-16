#include <kotavel/foundation/application.hpp>
#include <atomic>
#include <chrono>
#include <fstream>
#include <iostream>
#include <thread>
#include <vector>

namespace {
void check(bool value, const char* message) { if (!value) throw std::runtime_error(message); }
struct Directory {
    std::filesystem::path path = std::filesystem::temp_directory_path() /
        ("kotavel-logging-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
    ~Directory() { std::error_code ec; std::filesystem::remove_all(path, ec); }
};
std::string read(const std::filesystem::path& path) {
    std::ifstream stream(path);
    return {std::istreambuf_iterator<char>(stream), {}};
}
void nested_failure(int depth) {
    if (!depth) throw std::runtime_error("root cause");
    try { nested_failure(depth - 1); }
    catch (...) { std::throw_with_nested(kotavel::RecoverableError("outer", "outer cause")); }
}
}

int main(int argc, char** argv) {
    using namespace kotavel;
    // Usado por la prueba real de exclusión entre procesos.
    if (argc == 3) {
        try {
            LogOptions options{argv[2]}; options.console = false;
            Logger owner(options);
            if (std::string_view(argv[1]) == "--hold") {
                std::cout << "ready" << std::endl;
                std::string release; std::getline(std::cin, release);
            }
            return 0;
        } catch (...) { return 42; }
    }
    try {
        Directory root;
        LogOptions options{root.path / "app"}; options.console = false;
        ErrorHandlingOptions handling; handling.capture_traces = false;
        Application app(options, handling); app.boot();
        const std::vector<ContextField> fields{{"order_id", "123"}, {"tenant", "test"}};
        check(app.execute(OperationContext{"incident", "orders", fields}, [] {
            nested_failure(1);
        }) == OperationResult::recovered_error, "recover nested error");
        const auto& error = *app.last_error();
        check(error.module == "orders" && error.fields.size() == 2, "retain context");
        check(error.chain.causes.size() == 1 && error.chain.causes[0].message == "root cause",
              "retain nested cause");
        check(!error.context.stack.enabled && error.context.stack.size == 0, "disable capture at source");
        check(error.report_status == ReportStatus::written, "report status");
        check(app.present_error()->detail.empty(), "production hides context and causes");
        const auto log = read(options.directory / "kotavel.log");
        check(log.find(" orders.ERROR: ") != std::string::npos &&
              log.find("\"order_id\":\"123\"") != std::string::npos &&
              log.find("root cause") != std::string::npos &&
              log.find("[stacktrace]") != std::string::npos &&
              log.find("\"exception\":\"[object]") != std::string::npos,
              "Laravel-style message, context and exception trace");
        const std::vector<ContextField> duplicate{{"id", "1"}, {"id", "2"}};
        bool invalid_context = false;
        try { (void)app.execute(OperationContext{"bad", "orders", duplicate}, [] {}); }
        catch (const std::invalid_argument&) { invalid_context = true; }
        check(invalid_context && app.state() == ApplicationState::ready, "reject ambiguous context before action");
        check(app.execute("deep", [] { nested_failure(12); }) == OperationResult::recovered_error,
              "deep nested error");
        check(app.last_error()->chain.causes.size() == 8 && app.last_error()->chain.truncated,
              "bounded chain");
        std::filesystem::remove(options.directory / "kotavel.log");
        std::filesystem::create_directory(options.directory / "kotavel.log");
        check(app.execute("disk-failure", [] { throw RecoverableError("controlled"); }) ==
              OperationResult::recovered_error, "logging failure does not change recovery");
        check(app.last_error()->report_status == ReportStatus::failed, "logging failure visible");
        check(app.execute("next", [] {}) == OperationResult::success && !app.last_error(),
              "next operation clears context");
        app.shutdown();
        check(StackTrace::capture().enabled, "trace policy restored outside operation");

        LogOptions throttle{root.path / "throttle"}; throttle.console = false;
        throttle.throttle = {2, std::chrono::milliseconds{50}, 2};
        Logger limited(throttle);
        LogRecord record; record.code = "repeat"; record.module = "orders";
        check(limited.write(record) == ReportStatus::written, "first report");
        check(limited.write(record) == ReportStatus::written, "second report");
        check(limited.write(record) == ReportStatus::throttled, "third report suppressed");
        record.module = "payments";
        check(limited.write(record) == ReportStatus::written, "separate module budget");
        record.code = "new-key";
        check(limited.write(record) == ReportStatus::throttled, "bounded cardinality");
        std::this_thread::sleep_for(std::chrono::milliseconds{70});
        check(limited.write(record) == ReportStatus::written, "expired keys reclaimed");
        limited.configure_report_throttling({});
        for (int i = 0; i < 5; ++i) check(limited.write(record) == ReportStatus::written, "disable limit");
        check(limited.stats().throttled == 2 && limited.stats().failed == 0, "observable counters");

        LogOptions concurrent{root.path / "concurrent"}; concurrent.console = false;
        Logger shared(concurrent);
        bool denied = false;
        try { Logger second(concurrent); } catch (const std::runtime_error&) { denied = true; }
        check(denied, "second owner rejected");
        std::atomic<int> failures{0};
        std::vector<std::thread> writers;
        for (int thread = 0; thread < 4; ++thread) writers.emplace_back([&, thread] {
            const std::string id = std::to_string(thread);
            for (int i = 0; i < 100; ++i)
                if (!shared.report(LogLevel::info, "concurrent", id, "message")) ++failures;
            TraceCaptureScope scope(false);
            if (StackTrace::capture().enabled) ++failures;
        });
        for (auto& thread : writers) thread.join();
        check(failures == 0 && shared.stats().written == 400, "concurrent writes");
        check(StackTrace::capture().enabled, "thread local trace isolation");
        std::ifstream entries(concurrent.directory / "kotavel.log");
        std::string line; int count = 0;
        while (std::getline(entries, line)) {
            check(line.starts_with('[') && line.find(".INFO: ") != std::string::npos &&
                  line.ends_with('}'), "complete concurrent records");
            ++count;
        }
        check(count == 400, "no missing or interleaved lines");
        LogOptions bounded{root.path / "bounded"}; bounded.console = false; bounded.max_bytes = 16384;
        Logger bound_logger(bounded);
        std::vector<ContextField> large;
        for (int i = 0; i < 16; ++i) large.push_back({std::to_string(i), std::string(256, '\n')});
        std::vector<ErrorCause> causes(8);
        for (auto& cause : causes) cause.message = std::string(512, '\n');
        LogRecord huge; huge.code = std::string_view("bounded"); huge.fields = large; huge.causes = causes;
        const auto context = DiagnosticContext{SourceLocation::current(), TraceOrigin::exception_construction,
                                                StackTrace::capture()};
        huge.diagnostic = &context;
        check(bound_logger.write(huge) == ReportStatus::written, "bounded structured report");
        check(std::filesystem::file_size(bounded.directory / "kotavel.log") <= 16384, "bounded payload file");
        check(read(bounded.directory / "kotavel.log").find("\"payload_truncated\":true") != std::string::npos,
              "make payload truncation visible");
        std::cout << "Kotavel logging checks passed\n";
        return 0;
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
