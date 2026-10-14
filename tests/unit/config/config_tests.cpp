#include <kotavel/config/config.hpp>
#include <kotavel/config/config_map.hpp>
#include <kotavel/config/config_helpers.hpp>
#include <kotavel/config/config_file.hpp>
#include <iostream>
#include <sstream>
#include <thread>
#include <type_traits>

namespace {
void check(bool condition) { if (!condition) throw std::runtime_error("Config contract failed"); }
template<class E, class F> void fails(F action, std::string_view key) {
    try { action(); } catch (const E& error) {
        check(std::string_view(error.what()).find(key) != std::string_view::npos);
        check(std::string_view(error.what()).find("test-secret-value") == std::string_view::npos);
        return;
    }
    throw std::runtime_error("Expected configuration error");
}
}
int main() {
    try {
        kotavel::Config config;
        config.set("database.connections.mysql.username", "usuario");
        check(config.get<std::string>("database.connections.mysql.username") == "usuario");
        config.set("count", 12); config.set("ratio", 1.5); config.set("enabled", true);
        check(config.get<int>("count") == 12 && config.get<double>("ratio") == 1.5 && config.get<bool>("enabled"));
        check(config.contains("count") && !config.contains("missing"));
        // La lectura presta el mismo objeto; no devuelve una copia.
        static_assert(std::is_same_v<decltype(config.get<std::string>("x")), const std::string&>);
        const auto* text = &config.get<std::string>("database.connections.mysql.username");
        for (int i = 0; i < 1000; ++i) config.set("other." + std::to_string(i), i);
        check(text == &config.get<std::string>("database.connections.mysql.username"));
        config.set("count", 20); check(config.get<int>("count") == 20);
        config.set("count", "updated"); check(config.get<std::string>("count") == "updated");
        config.set("database", "flat");
        check(config.get<std::string>("database.connections.mysql.username") == "usuario");
        const std::string buffer = "count_suffix";
        check(config.get<std::string>(std::string_view(buffer).substr(0, 5)) == "updated");
        config.set("password", "test-secret-value");
        fails<std::invalid_argument>([&] { (void)config.get<int>("password"); }, "password");
        fails<std::out_of_range>([&] { (void)config.get<bool>("missing"); }, "missing");
        fails<std::invalid_argument>([&] { config.set("", true); }, "empty");
        // Los grupos anidados se aplanan una sola vez, también junto a claves con puntos.
        config.merge("app", {{"name", "Kotavel"}, {"debug", false},
            {"routing", {{"api", {{"port", 8080}, {"timeout", 2.5}}}}}});
        check(config.get<int>("app.routing.api.port") == 8080);
        check(config.get<double>("app.routing.api.timeout") == 2.5);
        fails<std::invalid_argument>([&] {
            config.merge("conflict", {{"a.b", 1}, {"a", {{"b", 2}}}});
        }, "conflict.a.b");
        check(!config.contains("conflict.a.b"));
        fails<std::invalid_argument>([&] { config.merge("app", {{"name", "duplicate"}}); }, "app.name");
        {
            const kotavel::ConfigScope scope(config);
            check(&kotavel::config() == &config);
            check(&kotavel::config<std::string>("app.name") == &config.get<std::string>("app.name"));
            std::ostringstream text;
            text << kotavel::config("app.name") << ':' << kotavel::config("app.debug");
            check(text.str() == "Kotavel:false");
            std::ostringstream missing_text;
            missing_text << kotavel::config("app.parametro_inexistente");
            check(missing_text.str() == "null");
            check(kotavel::config("app.parametro_inexistente").is_null());
            check(!kotavel::config("app.parametro_inexistente", false));
            check(kotavel::config("missing", "fallback") == "fallback");
            check(kotavel::config("missing", 42) == 42);
            check(kotavel::config("app.name", "fallback") == "Kotavel");
            fails<std::invalid_argument>([] { (void)kotavel::config("app.name", false); }, "app.name");
            kotavel::Config other;
            other.set("app.name", "nested");
            {
                const kotavel::ConfigScope nested(other);
                check(kotavel::config<std::string>("app.name") == "nested");
            }
            check(kotavel::config<std::string>("app.name") == "Kotavel");
            bool isolated = false;
            std::thread thread([&] {
                try { (void)kotavel::config(); }
                catch (const std::logic_error&) { isolated = true; }
            });
            thread.join();
            check(isolated);
        }
        fails<std::logic_error>([] { (void)kotavel::config(); }, "No active");
        // El registro explícito usa el prefijo declarado y carga su mapa.
        const kotavel::ConfigFile files[] = {
            {"app", []() -> kotavel::ConfigMap { return {{"name", "Kotavel"}}; }},
        };
        const auto registered = kotavel::load_config_files(files);
        check(registered.get<std::string>("app.name") == "Kotavel");
        const kotavel::ConfigFile missing[] = {{"app", nullptr}};
        fails<std::invalid_argument>([&] { (void)kotavel::load_config_files(missing); }, "loader");
        std::cout << "Config tests passed\n";
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
