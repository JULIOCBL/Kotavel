#include <kotavel/console/runtime.hpp>
#include <kotavel/system/os.hpp>
#include <iostream>
#include <sstream>
#include <fstream>
#include <chrono>
#include <stdexcept>
#include <utility>

namespace {
class ThrowingBootProvider final : public kotavel::ServiceProvider {
public:
    void register_services(kotavel::Container&) override {}
    void boot(kotavel::Container&) override {
        throw std::invalid_argument("Route group callback cannot be empty");
    }
};
}

int main() {
    const auto root = std::filesystem::current_path() / "storage" /
        ("kotavel-console-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
    struct Cleanup { std::filesystem::path path; ~Cleanup() { std::error_code ec; std::filesystem::remove_all(path, ec); } } cleanup{root};
    std::filesystem::create_directories(root);
    std::ofstream(root / ".env") << "APP_ENV=production\nAPP_DEBUG=false\n";
    const auto capture_run = [](auto&& run) {
        std::ostringstream diagnostic;
        auto* previous = std::cerr.rdbuf(diagnostic.rdbuf());
        int code = 1;
        try { code = run(); }
        catch (...) { std::cerr.rdbuf(previous); throw; }
        std::cerr.rdbuf(previous);
        return std::pair{code, diagnostic.str()};
    };
    const auto read_file = [](const std::filesystem::path& path) {
        std::ifstream input(path, std::ios::binary);
        return std::string((std::istreambuf_iterator<char>(input)),
                           std::istreambuf_iterator<char>());
    };
    const auto make_root = [&](std::string_view name) {
        const auto path = root / name;
        std::filesystem::create_directories(path);
        std::ofstream(path / ".env") << "APP_ENV=production\nAPP_DEBUG=false\n";
        return path;
    };
    char name[] = "consumer", help[] = "--help";
    char* help_args[] = {name, help};
    bool configured = false;
    auto configure = [&](const kotavel::ConsoleContext& context) {
        configured = true;
        if (context.root != std::filesystem::canonical(root)) throw std::runtime_error("Wrong root");
        kotavel::ConsoleSettings settings;
        settings.logs.directory = root / "logs";
        settings.logs.console = false;
        return settings;
    };
    auto execute = [](kotavel::Application& application, const kotavel::ConsoleContext&) {
        if (application.state() != kotavel::ApplicationState::ready) throw std::runtime_error("Not ready");
        throw std::runtime_error("External callback failure");
        return 0;
    };
    if (kotavel::run_console(2, help_args, configure, execute) != 0 || configured) return 1;
    auto option = "--project-root=" + root.string();
    char* args[] = {name, option.data()};
    const auto first_failure_code = kotavel::run_console(2, args, configure, execute);
    if (first_failure_code != 1 || !configured) return 1;
    // El fallo del callback también debe liberar el bloqueo del logger.
    { kotavel::Logger logger({root / "logs", 16384, 0, false}); }
    // Un callback que ignore un resultado fatal nunca debe producir éxito,
    // incluso si ya solicitó shutdown antes de devolver su código.
    for (bool shutdown : {false, true}) {
        auto ignored_failure = [shutdown](kotavel::Application& application, const kotavel::ConsoleContext&) {
            (void)application.execute("fatal", [] { throw std::runtime_error("fatal operation"); });
            if (shutdown) application.shutdown();
            return 0;
        };
        if (kotavel::run_console(2, args, configure, ignored_failure) != 1) return 1;
    }
    auto recovered = [](kotavel::Application& application, const kotavel::ConsoleContext&) {
        (void)application.execute("recoverable", [] { throw kotavel::RecoverableError("expected"); });
        return 0;
    };
    if (kotavel::run_console(2, args, configure, recovered) != 0) return 1;
    // El entorno existe antes de cargar mapas; Config se activa después y
    // permanece disponible tanto al configurar como al ejecutar la aplicación.
    bool loaded = false;
    auto load = [&] {
        if (kotavel::env("APP_ENV", "missing") != "production")
            throw std::runtime_error("Environment must load before configuration");
        kotavel::Config config;
        config.set("app.env", kotavel::env("APP_ENV", "missing"));
        loaded = true;
        return config;
    };
    auto configure_after_load = [&](const kotavel::ConsoleContext& context) {
        if (!loaded || context.configuration != &kotavel::config())
            throw std::runtime_error("Configuration must load before configure");
        return configure(context);
    };
    auto execute_after_load = [](kotavel::Application&, const kotavel::ConsoleContext&) {
        return kotavel::config<std::string>("app.env") == "production" ? 0 : 1;
    };
    if (kotavel::run_console(2, args, configure_after_load, execute_after_load, {}, load) != 0) return 1;
    // El diagnóstico de entorno conserva las tres líneas y el comando de la plataforma.
    const auto missing_root = root / "missing-env";
    std::filesystem::create_directories(missing_root);
    auto missing_option = "--project-root=" + missing_root.string();
    char* missing_args[] = {name, missing_option.data()};
    configured = false;
    auto [missing_code, diagnostic] = capture_run([&] {
        return kotavel::run_console(2, missing_args, configure, execute);
    });
    const std::string description =
        "No se pudo abrir el archivo de entorno `.env` en la raíz del proyecto.\n";
    std::string expected = description;
    switch (kotavel::OS::type) {
        case kotavel::OSType::WINDOWS:
            expected += "Verifica que `.env` exista y tenga permisos de lectura. Si falta, créalo desde `.env.example`.\n"
                        "En PowerShell: Copy-Item -LiteralPath '.env.example' -Destination '.env'\n";
            break;
        case kotavel::OSType::MACOS:
            expected += "Verifica que `.env` exista y tengas permisos de lectura. Si falta, créalo desde `.env.example`.\n"
                        "En Terminal: cp '.env.example' '.env'\n";
            break;
        case kotavel::OSType::LINUX:
            expected += "Verifica que `.env` exista y tengas permisos de lectura. Si falta, créalo desde `.env.example`.\n"
                        "En la terminal: cp '.env.example' '.env'\n";
            break;
        default:
            expected += "Verifica que `.env` exista y que Kotavel tenga permisos para leerlo. Si falta, créalo desde `.env.example`.\n"
                        "Copia manualmente `.env.example` como `.env`.\n";
            break;
    }
    if (missing_code != 1 || configured || diagnostic != expected) return 1;
    std::ifstream bootstrap_log(missing_root / "storage" / "logs" / "kotavel.log");
    const std::string logged((std::istreambuf_iterator<char>(bootstrap_log)),
                             std::istreambuf_iterator<char>());
    if (logged.find("bootstrap.ERROR: No se pudo abrir el archivo de entorno `.env`") == std::string::npos ||
        logged.find("environment.file_open_failed") == std::string::npos ||
        logged.find("\"example\":\".env.example\"") == std::string::npos)
        return 1;

    // Los fallos siguientes se ejecutan dentro del mismo proceso de prueba:
    // si un error termina el proceso, no se alcanzan las comprobaciones finales.
    const auto invalid_root = make_root("invalid-env");
    std::ofstream(invalid_root / ".env", std::ios::trunc)
        << "APP_ENV=production\nMALFORMED_LINE\n";
    auto invalid_option = "--project-root=" + invalid_root.string();
    char* invalid_args[] = {name, invalid_option.data()};
    bool continued_after_invalid_env = false;
    auto [invalid_code, invalid_diagnostic] = capture_run([&] {
        const auto result = kotavel::run_console(2, invalid_args, configure, execute);
        continued_after_invalid_env = true;
        return result;
    });
    const auto logged_invalid = read_file(invalid_root / "storage" / "logs" / "kotavel.log");
    if (invalid_code != 1 || !continued_after_invalid_env ||
        invalid_diagnostic.find("línea 2") == std::string::npos ||
        logged_invalid.find("environment.file_invalid") == std::string::npos ||
        logged_invalid.find("\"line\":\"2\"") == std::string::npos)
        return 1;

    const auto config_root = make_root("invalid-config");
    auto config_option = "--project-root=" + config_root.string();
    char* config_args[] = {name, config_option.data()};
    auto invalid_config = [] {
        kotavel::Config config;
        config.set("logging.channel", "not-a-channel");
        return config;
    };
    bool continued_after_invalid_config = false;
    auto [config_code, config_diagnostic] = capture_run([&] {
        const auto result = kotavel::run_console(2, config_args, execute, {}, invalid_config);
        continued_after_invalid_config = true;
        return result;
    });
    const auto logged_config = read_file(config_root / "storage" / "logs" / "kotavel.log");
    if (config_code != 1 || !continued_after_invalid_config ||
        config_diagnostic.find("logging.channel") == std::string::npos ||
        logged_config.find("configuration.invalid") == std::string::npos ||
        logged_config.find("logging.channel") == std::string::npos)
        return 1;

    const auto startup_root = make_root("startup-exception");
    auto startup_option = "--project-root=" + startup_root.string();
    char* startup_args[] = {name, startup_option.data()};
    auto throwing_config = []() -> kotavel::Config {
        throw std::runtime_error("simulated configuration loader failure");
    };
    bool continued_after_exception = false;
    auto [startup_code, startup_diagnostic] = capture_run([&] {
        const auto result = kotavel::run_console(2, startup_args, execute, {}, throwing_config);
        continued_after_exception = true;
        return result;
    });
    const auto logged_startup = read_file(startup_root / "storage" / "logs" / "kotavel.log");
    if (startup_code != 1 || !continued_after_exception ||
        startup_diagnostic.find("--debug") == std::string::npos ||
        logged_startup.find("runtime.startup_failed") == std::string::npos ||
        logged_startup.find("load_configuration") == std::string::npos ||
        logged_startup.find("simulated configuration loader failure") == std::string::npos)
        return 1;

    const auto unknown_root = make_root("unknown-exception");
    auto unknown_option = "--project-root=" + unknown_root.string();
    char* unknown_args[] = {name, unknown_option.data()};
    auto throwing_unknown = []() -> kotavel::Config { throw 42; };
    bool continued_after_unknown = false;
    auto [unknown_code, unknown_diagnostic] = capture_run([&] {
        const auto result = kotavel::run_console(2, unknown_args, execute, {}, throwing_unknown);
        continued_after_unknown = true;
        return result;
    });
    const auto logged_unknown = read_file(unknown_root / "storage" / "logs" / "kotavel.log");
    if (unknown_code != 1 || !continued_after_unknown ||
        unknown_diagnostic.find("no pudo iniciar") == std::string::npos ||
        logged_unknown.find("Unknown non-standard exception") == std::string::npos)
        return 1;

    // Un error de validación en el boot de un provider (por ejemplo, rutas)
    // debe llegar al log del proyecto aunque ocurra antes del callback principal.
    const auto provider_root = make_root("provider-boot-exception");
    auto provider_option = "--project-root=" + provider_root.string();
    char* provider_args[] = {name, provider_option.data()};
    auto create_throwing_provider = [] {
        kotavel::ServiceProviderList providers;
        providers.push_back(std::make_unique<ThrowingBootProvider>());
        return providers;
    };
    auto execute_without_work = [](kotavel::Application&, const kotavel::ConsoleContext&) {
        return 0;
    };
    auto [provider_code, provider_diagnostic] = capture_run([&] {
        return kotavel::run_console(2, provider_args, execute_without_work, {}, {},
                                    create_throwing_provider);
    });
    const auto logged_provider = read_file(provider_root / "storage" / "logs" / "kotavel.log");
    if (provider_code != 1 ||
        provider_diagnostic.find("Kotavel no pudo iniciar o completar") == std::string::npos ||
        logged_provider.find("runtime.startup_failed") == std::string::npos ||
        logged_provider.find("boot_services") == std::string::npos ||
        logged_provider.find("Route group callback cannot be empty") == std::string::npos)
        return 1;

    // Un archivo donde debería ir el directorio hace fallar Logger y también
    // el intento de diagnóstico. Ambos fallos deben quedar contenidos.
    const auto blocked_root = make_root("blocked-log-destination");
    const auto blocked_logs = blocked_root / "logs-is-a-file";
    std::ofstream(blocked_logs) << "block directory creation";
    auto blocked_option = "--project-root=" + blocked_root.string();
    char* blocked_args[] = {name, blocked_option.data()};
    auto blocked_configure = [&](const kotavel::ConsoleContext&) {
        kotavel::ConsoleSettings settings;
        settings.logs.directory = blocked_logs;
        settings.logs.console = false;
        return settings;
    };
    bool continued_after_log_failure = false;
    auto [log_code, log_diagnostic] = capture_run([&] {
        const auto result = kotavel::run_console(2, blocked_args, blocked_configure, execute);
        continued_after_log_failure = true;
        return result;
    });
    if (log_code != 1 || !continued_after_log_failure ||
        log_diagnostic.find("no pudo iniciar") == std::string::npos ||
        log_diagnostic.find("No se pudo guardar el registro") == std::string::npos)
        return 1;
    return 0;
}
