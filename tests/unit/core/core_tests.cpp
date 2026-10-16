#include <kotavel/foundation/application.hpp>
#include <chrono>
#include <fstream>
#include <iostream>
#include <iterator>

namespace {
void check(bool value, const char* message) {
    if (!value) throw std::runtime_error(message);
}
struct Directory {
    std::filesystem::path path = std::filesystem::temp_directory_path() /
        ("kotavel-test-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
    ~Directory() { std::error_code ec; std::filesystem::remove_all(path, ec); }
};
struct Resource {
    int& active;
    explicit Resource(int& count) : active(count) { ++active; }
    ~Resource() { --active; }
};
}

int main() {
    try {
        Directory root;
        using kotavel::LogLevel;
        const LogLevel levels[] = {LogLevel::trace, LogLevel::debug, LogLevel::info,
            LogLevel::notice, LogLevel::warning, LogLevel::error, LogLevel::critical, LogLevel::alert, LogLevel::emergency};
        const char* names[] = {"trace", "debug", "info", "notice", "warning", "error", "critical", "alert", "emergency"};
        const char* formatted_names[] = {"TRACE", "DEBUG", "INFO", "NOTICE", "WARNING",
            "ERROR", "CRITICAL", "ALERT", "EMERGENCY"};
        // Comprobar cada umbral, incluyendo exactamente su nivel y los superiores.
        for (int threshold = 0; threshold < 9; ++threshold) {
            kotavel::Logger filtered({root.path / names[threshold], 16384, 2, false,
                                      levels[threshold]});
            for (int level = 0; level < 9; ++level)
                check(filtered.report(levels[level], "level-test", "test", "message"), "level report");
            std::ifstream logs(root.path / names[threshold] / "kotavel.log");
            std::string record;
            for (int level = threshold; level < 9; ++level) {
                check(bool(std::getline(logs, record)), "missing accepted level");
                check(record.find(std::string(".") + formatted_names[level] + ": ") !=
                      std::string::npos, "incorrect serialized level");
            }
            check(!std::getline(logs, record), "filtered levels must not be written");
        }
        // Un mensaje filtrado no accede siquiera a un destino que ha fallado.
        const auto unavailable = root.path / "unavailable";
        kotavel::Logger filtered({unavailable, 16384, 2, false, LogLevel::warning});
        std::filesystem::remove(unavailable / "kotavel.log");
        std::filesystem::create_directory(unavailable / "kotavel.log");
        check(filtered.report(LogLevel::info, "ignored", "test", "message"), "filter before IO");
        check(std::filesystem::is_directory(unavailable / "kotavel.log"), "filter must not touch destination");
        bool bad_level = false;
        try {
            kotavel::Logger invalid({root.path / "invalid", 16384, 2, false,
                                     static_cast<LogLevel>(99)});
        } catch (const std::invalid_argument&) { bad_level = true; }
        check(bad_level, "reject invalid threshold");
        check(!filtered.report(static_cast<LogLevel>(99), "invalid", "test", "message"),
              "reject invalid report level");
        kotavel::LogOptions options{root.path, 16384, 2, false};
        kotavel::Application app(options);
        bool rejected = false;
        try { (void)app.execute("before-boot", [] {}); }
        catch (const std::logic_error&) { rejected = true; }
        check(rejected, "execution before boot must fail");
        app.boot();
        check(app.execute("first", [] {}) == kotavel::OperationResult::success, "initial operation");
        int resources = 0;
        check(app.execute("failure-1", [&] {
            Resource resource(resources);
            throw kotavel::RecoverableError("controlled\nerror");
        }) == kotavel::OperationResult::recovered_error, "recoverable outcome");
        check(resources == 0, "resources must unwind on error");
        check(app.execute("after-failure", [] {}) == kotavel::OperationResult::success, "recovery");
        std::ifstream input(root.path / "kotavel.log");
        std::string line; std::getline(input, line);
        check(line.find("failure-1") != std::string::npos, "diagnostic context");
        check(line.find(".ERROR: ") != std::string::npos, "default error level");
        check(line.find("controlled\\\\nerror") != std::string::npos, "escape multiline error");
        check(!std::getline(input, line), "one line per incident");
        input.close();
        check(app.execute("unexpected", [] { throw std::runtime_error("unknown state"); }) ==
              kotavel::OperationResult::fatal_error, "unexpected error outcome");
        check(app.state() == kotavel::ApplicationState::failed, "failed state");
        std::ifstream fatal_log(root.path / "kotavel.log");
        std::getline(fatal_log, line);
        std::getline(fatal_log, line);
        check(line.find(".CRITICAL: ") != std::string::npos, "fatal level");
        fatal_log.close();
        rejected = false;
        try { (void)app.execute("unsafe", [] {}); }
        catch (const std::logic_error&) { rejected = true; }
        check(rejected, "do not continue after unexpected error");
        app.shutdown(); app.shutdown();
        check(app.state() == kotavel::ApplicationState::stopped, "idempotent shutdown");

        kotavel::Logger logger(options);
        for (int i = 0; i < 100; ++i)
            check(logger.report("rotation", "test", std::string(600, 'x')), "rotation report");
        check(std::filesystem::exists(root.path / "kotavel.log.2"), "rotation backups");
        check(!std::filesystem::exists(root.path / "kotavel.log.3"), "retention bound");
        for (const auto& entry : std::filesystem::directory_iterator(root.path))
            if (entry.is_regular_file())
                check(entry.file_size() <= options.max_bytes, "file size bound");
        // Sustituir el archivo por un directorio fuerza un fallo de escritura portable.
        std::filesystem::remove(root.path / "kotavel.log");
        std::filesystem::create_directory(root.path / "kotavel.log");
        check(!logger.report("fallback", "test", "disk unavailable"), "emergency fallback");

        auto invalid = options; invalid.directory.clear();
        kotavel::Application broken(invalid);
        rejected = false;
        try { broken.boot(); } catch (const std::invalid_argument&) { rejected = true; }
        check(rejected && broken.state() == kotavel::ApplicationState::failed, "invalid startup");
        std::cout << "Kotavel core checks passed\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n'; return 1;
    }
}
