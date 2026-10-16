#include <kotavel/console/runtime.hpp>
#include <chrono>
#include <fstream>
#include <iostream>

namespace {
void check(bool condition) {
    if (!condition) throw std::runtime_error("Logging configuration contract failed");
}
template<class Action> void invalid(Action action) {
    try { action(); } catch (const std::invalid_argument&) { return; }
    throw std::runtime_error("Invalid logging configuration was accepted");
}
}
int main() {
    try {
        const auto root = std::filesystem::temp_directory_path() /
            ("kotavel-settings-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
        struct Cleanup {
            std::filesystem::path path;
            ~Cleanup() { std::error_code ec; std::filesystem::remove_all(path, ec); }
        } cleanup{root};
        std::filesystem::create_directories(root);
        kotavel::Config config;
        kotavel::ConsoleContext context{root, {}, {}, &config};
        // Los defaults del núcleo conservan archivo, 512 MiB y tres respaldos.
        auto settings = kotavel::console_settings(context);
        check(settings.logs.file && settings.logs.enabled && !settings.logs.console);
        check(settings.logs.directory == root / "storage/logs");
        check(settings.logs.max_bytes == 512 * 1024 * 1024 && settings.logs.backups == 3);
        check(!settings.performance.enabled && settings.performance.console && settings.performance.startup);
        check(!settings.performance.stream);
        check(settings.performance.interval == std::chrono::milliseconds(1000));
        config.set("performance.enabled", true);
        config.set("performance.console", false);
        config.set("performance.stream", true);
        config.set("performance.interval_ms", 250);
        settings = kotavel::console_settings(context);
        check(settings.performance.enabled && !settings.performance.console && settings.performance.stream &&
              settings.performance.interval == std::chrono::milliseconds(250));
        config.set("performance.interval_ms", 99);
        invalid([&] { (void)kotavel::console_settings(context); });
        config.set("performance.interval_ms", 60001);
        invalid([&] { (void)kotavel::console_settings(context); });
        config.set("performance.interval_ms", 60000);
        settings = kotavel::console_settings(context);
        check(settings.performance.interval == std::chrono::milliseconds(60000));
        config.set("performance.enabled", 1);
        invalid([&] { (void)kotavel::console_settings(context); });
        config.set("performance.enabled", true);
        config.set("performance.interval_ms", 1000);
        {
            auto file_options = settings.logs;
            file_options.directory = root / "single/logs";
            kotavel::Logger logger(file_options);
            check(logger.write({kotavel::LogLevel::error, "single", "test", "message"}) == kotavel::ReportStatus::written);
        }
        check(std::filesystem::file_size(root / "single/logs/kotavel.log") > 0);
        config.set("logging.channel", "stderr");
        config.set("logging.level", "error");
        settings = kotavel::console_settings(context);
        check(!settings.logs.file && settings.logs.console);
        check(settings.logs.min_level == kotavel::LogLevel::error);
        {
            kotavel::Logger logger(settings.logs);
            check(logger.write({kotavel::LogLevel::info, "filtered", "test", "message"}) == kotavel::ReportStatus::filtered);
            check(logger.write({kotavel::LogLevel::error, "stderr", "test", "message"}) == kotavel::ReportStatus::written);
        }
        check(!std::filesystem::exists(root / "storage"));
        // null evita E/S; el estado y presentación de errores siguen disponibles.
        config.set("logging.channel", "null");
        settings = kotavel::console_settings(context);
        {
            kotavel::Application app(settings.logs);
            app.boot();
            check(app.execute("recoverable", [] { throw kotavel::RecoverableError("expected"); }) == kotavel::OperationResult::recovered_error);
            check(app.last_error()->report_status == kotavel::ReportStatus::filtered);
            check(app.present_error().has_value());
            check(app.execute("fatal", [] { throw std::runtime_error("fatal"); }) == kotavel::OperationResult::fatal_error);
            check(app.state() == kotavel::ApplicationState::failed);
        }
        check(!std::filesystem::exists(root / "storage"));
        // La ruta relativa parte de root y los valores inválidos nunca se ignoran.
        config.set("logging.directory", "custom/logs");
        check(kotavel::console_settings(context).logs.directory == root / "custom/logs");
        config.set("logging.directory", (root / "absolute").string());
        check(kotavel::console_settings(context).logs.directory == root / "absolute");
        config.set("logging.channel", "unsupported");
        invalid([&] { (void)kotavel::console_settings(context); });
        config.set("logging.channel", "single");
        config.set("logging.level", true);
        invalid([&] { (void)kotavel::console_settings(context); });
        config.set("logging.level", "unsupported");
        invalid([&] { (void)kotavel::console_settings(context); });
        config.set("logging.level", "info");
        config.set("logging.backups", -1);
        invalid([&] { (void)kotavel::console_settings(context); });
        config.set("logging.backups", 3);
        config.set("logging.max_bytes", 0);
        invalid([&] { (void)kotavel::console_settings(context); });
        // El overload habitual carga el mapa antes de construir el logger.
        std::ofstream(root / ".env") << "APP_ENV=production\nAPP_DEBUG=false\nLOG_CHANNEL=\"null\"\n";
        char name[] = "test";
        auto option = "--project-root=" + root.string();
        char* args[]{name, option.data()};
        const auto result = kotavel::run_console(2, args,
            [](kotavel::Application& app, const kotavel::ConsoleContext&) {
                check(app.state() == kotavel::ApplicationState::ready);
                return 0;
            }, {}, [] {
                kotavel::Config config;
                config.set("logging.channel", kotavel::env("LOG_CHANNEL", "single"));
                return config;
            });
        check(result == 0 && !std::filesystem::exists(root / "storage"));
        std::cout << "Logging configuration tests passed\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
