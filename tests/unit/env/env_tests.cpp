#include <kotavel/env/env.hpp>
#include <map>
#include <fstream>
#include <chrono>
#include <type_traits>
#include <iostream>
#include <sstream>
#include <stdexcept>
struct Source : kotavel::EnvironmentSource {
    std::map<std::string, std::string, std::less<>> values;
    std::optional<std::string> read(std::string_view key) const override {
        auto it = values.find(key);
        if (it == values.end()) return std::nullopt;
        return it->second;
    }
};
void check(bool condition) { if (!condition) throw std::runtime_error("Env contract failed"); }
template<class F> void invalid(F f) {
    try { f(); } catch (const std::invalid_argument&) { return; }
    throw std::runtime_error("Expected invalid configuration");
}
int main() {
    try {
        Source source;
        kotavel::Env env(source);
        auto defaults = kotavel::AppEnvironment::load(env);
        check(defaults.name == "Kotavel" && defaults.environment == "production" && !defaults.debug);
        source.values = {{"APP_NAME", "Kotavel"}, {"APP_ENV", "development"}, {"APP_DEBUG", "true"}};
        auto settings = kotavel::AppEnvironment::load(env);
        check(settings.debug && settings.environment == "development");
        source.values["APP_ENV"] = "production";
        check(kotavel::AppEnvironment::load(env).debug); // Entorno y debug son independientes.
        for (const auto value : {"true", "TRUE", "(true)"}) {
            source.values["X"] = value; check(env.boolean("X", false));
        }
        for (const auto value : {"false", "FALSE", "(false)"}) {
            source.values["X"] = value; check(!env.boolean("X", true));
        }
        source.values["X"] = "null";
        check(std::holds_alternative<std::monostate>(env.get("X", true)));
        invalid([&] { env.require("X"); });
        source.values["X"] = "empty"; check(env.string("X", "fallback").empty());
        source.values["X"] = "\"true\""; check(env.string("X", "") == "true");
        invalid([&] { env.boolean("X", false); });
        source.values["APP_DEBUG"] = "yes"; invalid([&] { kotavel::AppEnvironment::load(env); });
        source.values["APP_DEBUG"] = "false";
        source.values["APP_ENV"] = "typo"; invalid([&] { kotavel::AppEnvironment::load(env); });
        source.values["APP_ENV"] = "testing"; source.values["APP_NAME"] = "";
        invalid([&] { kotavel::AppEnvironment::load(env); });
        invalid([&] { env.get(std::string("A\0B", 3)); });
        invalid([&] { env.get("A=B"); });
        source.values["X"] = std::string(4097, 'a'); invalid([&] { env.get("X"); });
        kotavel::ProcessEnvironment process;
        check(process.read("KOTAVEL_TEST_PROCESS_VALUE") == "copied");
        const std::string sliced = "KOTAVEL_TEST_PROCESS_VALUE_suffix";
        check(process.read(std::string_view(sliced).substr(0, 26)) == "copied");
        invalid([&] { process.read(std::string(257, 'A')); });
        // Verificar ambos límites sin depender del entorno real de la máquina.
        source.values["X"] = std::string(4096, 'a');
        check(env.string("X", "").size() == 4096);
        source.values["X"] = "'Texto UTF-8: á'";
        check(env.string("X", "") == "Texto UTF-8: á");
        source.values["X"] = "(NuLl)";
        check(std::holds_alternative<std::monostate>(env.get("X")));
        {
            using kotavel::env;
            static_assert(std::is_same_v<decltype(env("APP_NAME", "Kotavel")), std::string>);
            static_assert(std::is_same_v<decltype(env("APP_DEBUG", false)), bool>);
            check(env("KOTAVEL_TEST_PROCESS_VALUE", "fallback") == "copied");
            check(env("KOTAVEL_TEST_MISSING", "Kotavel") == "Kotavel");
            check(!env("KOTAVEL_TEST_MISSING", false));
            check(env("KOTAVEL_TEST_BOOL", false));
            invalid([&] { env("KOTAVEL_TEST_PROCESS_VALUE", false); });
        }
        const auto folder = std::filesystem::temp_directory_path() /
            ("kotavel-env-" + std::to_string(std::chrono::steady_clock::now().time_since_epoch().count()));
        struct Cleanup { std::filesystem::path path; ~Cleanup() { std::error_code ec; std::filesystem::remove_all(path, ec); } } cleanup{folder};
        std::filesystem::create_directories(folder);
        auto write = [&](const std::string& content) { std::ofstream(folder / ".env.local") << content; };
        write("# settings\r\nAPP_ENV=local\r\nKOTAVEL_FILE_TEXT=\"hello # world\" # comment\nKOTAVEL_TEST_PROCESS_VALUE=file\n");
        kotavel::FileEnvironment files(folder, "local");
        kotavel::Env reader(files);
        check(reader.string("KOTAVEL_FILE_TEXT", "") == "hello # world");
        check(reader.string("KOTAVEL_TEST_PROCESS_VALUE", "") == "copied");
        {
            const kotavel::EnvScope scope(reader);
            check(kotavel::env("APP_ENV").as_string() == "local");
            std::ostringstream output;
            output << kotavel::env("APP_ENV") << ':' << kotavel::env("KOTAVEL_TEST_BOOL")
                   << ':' << kotavel::env("KOTAVEL_TEST_MISSING");
            check(output.str() == "local:true:null");
            check(kotavel::env("KOTAVEL_TEST_MISSING").is_null());
            check(kotavel::env("KOTAVEL_TEST_BOOL").as_bool());
            invalid([&] { (void)kotavel::env("APP_ENV").as_bool(); });
            std::ostringstream false_output;
            false_output << kotavel::EnvironmentValue(false);
            check(false_output.str() == "false");
            check(kotavel::env("KOTAVEL_FILE_TEXT", "") == "hello # world");
            {
                const kotavel::EnvScope nested(env);
                check(kotavel::env("KOTAVEL_FILE_TEXT", "fallback") == "fallback");
            }
            check(kotavel::env("KOTAVEL_FILE_TEXT", "") == "hello # world");
        }
        check(kotavel::env("KOTAVEL_FILE_TEXT", "fallback") == "fallback");
        invalid([&] { kotavel::FileEnvironment bad(folder, "../local"); });
        write("APP_ENV=production\n"); invalid([&] { kotavel::FileEnvironment bad(folder, "local"); });
        write("X=1\nX=2\n"); invalid([&] { kotavel::FileEnvironment bad(folder, "local"); });
        write("X=\"unclosed\n"); invalid([&] { kotavel::FileEnvironment bad(folder, "local"); });
        std::cout << "Environment tests passed\n";
    } catch (const std::exception& e) { std::cerr << e.what() << '\n'; return 1; }
}
