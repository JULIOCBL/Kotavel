#include <kotavel/console/runtime.hpp>
#include <kotavel/system/os.hpp>
#include <iostream>
#include <sstream>
#include <fstream>
#include <chrono>
#include <stdexcept>
int main() {
    const auto root = std::filesystem::temp_directory_path() /
        ("kotavel-console-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
    struct Cleanup { std::filesystem::path path; ~Cleanup() { std::error_code ec; std::filesystem::remove_all(path, ec); } } cleanup{root};
    std::filesystem::create_directories(root);
    std::ofstream(root / ".env") << "APP_ENV=production\nAPP_DEBUG=false\n";
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
    if (kotavel::run_console(2, args, configure, execute) != 1 || !configured) return 1;
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
    std::ostringstream diagnostic;
    struct RestoreStream {
        std::streambuf* previous;
        ~RestoreStream() { std::cerr.rdbuf(previous); }
    } restore{std::cerr.rdbuf(diagnostic.rdbuf())};
    configured = false;
    const auto missing_code = kotavel::run_console(2, missing_args, configure, execute);
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
    if (missing_code != 1 || configured || diagnostic.str() != expected) return 1;
    std::ifstream bootstrap_log(missing_root / "storage" / "logs" / "kotavel.log");
    const std::string logged((std::istreambuf_iterator<char>(bootstrap_log)),
                             std::istreambuf_iterator<char>());
    if (logged.find("bootstrap.ERROR: No se pudo abrir el archivo de entorno `.env`") == std::string::npos ||
        logged.find("environment.file_open_failed") == std::string::npos ||
        logged.find("\"example\":\".env.example\"") == std::string::npos)
        return 1;
    return 0;
}
