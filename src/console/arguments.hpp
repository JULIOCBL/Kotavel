#pragma once
#include <filesystem>
#include <string>
#include <string_view>
#include <vector>

namespace kotavel::detail {
// Opciones comunes propias del runtime; las vistas prestan memoria de argv.
struct ConsoleArguments {
    std::string environment = "production";
    std::filesystem::path root;
    std::vector<std::string_view> arguments;
    bool help = false;
    bool debug = false;
};
ConsoleArguments parse_console_arguments(int argc, char** argv);
}
