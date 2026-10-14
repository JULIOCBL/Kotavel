#include <kotavel/console/runtime.hpp>
#include "arguments.hpp"
#include <iostream>
#include <utility>

namespace kotavel {
int run_console(int argc, char** argv, const ExecuteConsole& execute,
                std::string_view application_help, const LoadConsoleConfig& load_config) {
    return run_console(argc, argv, console_settings, execute, application_help, load_config);
}
int run_console(int argc, char** argv, const ConfigureConsole& configure,
                const ExecuteConsole& execute, std::string_view application_help,
                const LoadConsoleConfig& load_config) {
    bool debug = false;
    try {
        if (argc < 1 || !argv || !configure || !execute)
            throw std::invalid_argument("Console requires arguments and callbacks");
        auto options = detail::parse_console_arguments(argc, argv);
        debug = options.debug;
        if (options.help) {
            std::cout << "Uso: aplicación [--env=local|development|production] [--project-root=RUTA] [--development]\n";
            if (!application_help.empty()) std::cout << application_help << '\n';
            return 0;
        }
        auto root = std::move(options.root);
        if (root.empty()) root = std::filesystem::current_path();
        root = std::filesystem::canonical(root);
        if (!std::filesystem::is_directory(root)) throw std::invalid_argument("Project root must be a directory");
        // Etapa 1: cargar y validar el entorno antes de cualquier configuración.
        FileEnvironment source(root, options.environment);
        const Env reader(source);
        // Los helpers del consumidor consultan el mismo archivo y precedencia
        // durante este arranque; RAII restaura la fuente incluso si hay un fallo.
        const EnvScope environment_scope(reader);
        auto identity = AppEnvironment::load(reader);
        debug = debug || identity.debug;
        // Etapa 2: cargar mapas de configuración usando el entorno ya disponible.
        // Su ámbito incluye configure, execute y cierre; nunca se propaga a otros hilos.
        Config settings_map = load_config ? load_config() : Config{};
        const ConfigScope config_scope(settings_map);
        ConsoleContext context{std::move(root), std::move(identity), options.arguments, &settings_map};
        auto settings = configure(context);
        // La presentación detallada depende del entorno resuelto, no de cada comando.
        settings.errors.environment = debug ? Environment::development : Environment::production;
        Application application(std::move(settings.logs), std::move(settings.errors));
        application.boot();
        const auto code = execute(application, context);
        // No convertir un fallo fatal ignorado por el consumidor en una salida exitosa.
        const bool failed = application.state() == ApplicationState::failed ||
            (application.last_error() && !application.last_error()->recoverable);
        application.shutdown();
        return failed && code == 0 ? 1 : code;
    } catch (const std::exception& error) {
        // Application libera sus recursos por RAII antes de informar un fallo externo.
        std::cerr << "No se pudo iniciar o ejecutar Kotavel.\n";
        if (debug) std::cerr << error.what() << '\n';
        return 1;
    } catch (...) {
        std::cerr << "Kotavel terminó por un error inesperado.\n";
        return 1;
    }
}
}
