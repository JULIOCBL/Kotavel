#include <kotavel/foundation/application.hpp>
#include <kotavel/diagnostics/pending.hpp>
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
        ("kotavel-diagnostics-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
    ~Directory() { std::error_code ec; std::filesystem::remove_all(path, ec); }
};
std::string read(const std::filesystem::path& path) {
    std::ifstream stream(path);
    return {std::istreambuf_iterator<char>(stream), {}};
}
}

int main() {
    try {
        Directory root;
        kotavel::Application app({root.path / "production", 16384, 2, false});
        app.boot();
        std::uint_least32_t expected_line = 0;
        auto result = app.execute("incident-42", [&] {
            expected_line = __LINE__ + 1;
            throw kotavel::RecoverableError("database.unavailable", "private-host password=secret");
        });
        check(result == kotavel::OperationResult::recovered_error, "recoverability unchanged");
        check(app.last_error().has_value(), "retain incident");
        const auto& details = *app.last_error();
        check(details.context.location.line() == expected_line, "capture caller line");
        check(std::string(details.context.location.file_name()).find("diagnostics_tests.cpp") !=
              std::string::npos, "capture caller file");
        check(details.context.origin == kotavel::TraceOrigin::exception_construction, "trace origin");
        check(!details.context.stack.format().empty(), "trace or explicit unavailable status");
        const auto public_error = app.present_error();
        check(public_error && public_error->reference == "incident-42", "public reference");
        check(public_error->detail.empty(), "production hides internal details");
        const auto production = public_error->console_text();
        check(production.find("secret") == std::string::npos &&
              production.find(".cpp") == std::string::npos, "production does not expose internals");
        const auto development = kotavel::render_error(details, kotavel::Environment::development).console_text();
        check(development.find("private-host") != std::string::npos &&
              development.find("diagnostics_tests.cpp") != std::string::npos, "development details");
        const auto log = read(root.path / "production/kotavel.log");
        check(log.find(".ERROR: ") != std::string::npos &&
              log.find("\"exception\":\"[object]") != std::string::npos &&
              log.find("diagnostics_tests.cpp") != std::string::npos &&
              log.find("\\n[stacktrace]\\n") != std::string::npos,
              "Laravel-style log includes exception source and stack trace");
        check(app.execute("next", [] {}) == kotavel::OperationResult::success, "next operation works");
        check(!app.last_error() && !app.present_error(), "do not leak previous incident");
        check(app.execute("external", [] { throw std::runtime_error("external-secret"); }) ==
              kotavel::OperationResult::fatal_error, "external exceptions stop execution");
        check(app.last_error()->context.origin == kotavel::TraceOrigin::handler &&
              app.last_error()->context.location.line() == 0, "do not invent external origin");
        app.shutdown();

        kotavel::ErrorHandlingOptions handling;
        handling.rules["ignored"] = {false, {}, "Intenta más tarde."};
        handling.rules["custom"] = {true, kotavel::LogLevel::notice, "Mensaje seguro."};
        kotavel::Application configured({root.path / "rules", 16384, 2, false}, handling);
        configured.boot();
        check(configured.execute("ignored-1", [] {
            throw kotavel::RecoverableError("ignored", "secret");
        }) == kotavel::OperationResult::recovered_error, "ignored still returns error");
        check(read(root.path / "rules/kotavel.log").empty(), "skip report by code");
        check(configured.present_error()->message == "Intenta más tarde.", "custom public message");
        check(configured.execute("custom-1", [] {
            throw kotavel::DiagnosticError("custom", "private");
        }) == kotavel::OperationResult::fatal_error, "severity cannot change recovery");
        check(read(root.path / "rules/kotavel.log").find(".NOTICE: ") !=
              std::string::npos, "custom report level");
        configured.shutdown();

        kotavel::ErrorHandlingOptions debug;
        debug.environment = kotavel::Environment::development;
        kotavel::Application unknown({root.path / "unknown", 16384, 2, false}, debug);
        unknown.boot();
        check(unknown.execute("unknown-1", [] { throw 1; }) == kotavel::OperationResult::fatal_error,
              "unknown exception classified");
        check(unknown.present_error()->detail.find("manejador") != std::string::npos,
              "handler trace labeled in presentation");
        unknown.shutdown();

        check(kotavel::configure_remote_logging("https://example.invalid") ==
              kotavel::FeatureStatus::not_implemented, "remote placeholder");
        check(kotavel::enable_fatal_system_capture() == kotavel::FeatureStatus::not_implemented,
              "fatal capture placeholder");
        std::cout << "Kotavel diagnostic checks passed\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
