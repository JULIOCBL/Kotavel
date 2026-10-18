#include "arguments.hpp"
#include <stdexcept>

namespace kotavel::detail {
// Interpretar solo opciones comunes; el consumidor valida los argumentos restantes.
ConsoleArguments parse_console_arguments(int argc, char** argv) {
    ConsoleArguments options;
    for (int i = 1; i < argc; ++i) {
        if (!argv[i]) throw std::invalid_argument("Null console argument");
        const std::string_view argument(argv[i]);
        if (argument == "--help" || argument == "-h") options.help = true;
        else if (argument == "--development") options.debug = true;
        else if (argument.starts_with("--env=")) {
            options.environment = argument.substr(6);
            options.environment_explicit = true;
        }
        else if (argument.starts_with("--project-root=")) {
            if (argument.size() == 15) throw std::invalid_argument("Empty project root");
            options.root = argument.substr(15);
        } else options.arguments.push_back(argument);
    }
    return options;
}
}
