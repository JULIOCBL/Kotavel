#include <kotavel/config/config_map.hpp>
#include <kotavel/config/config_helpers.hpp>
#include <sstream>
#include <iostream>

namespace {
void check(bool value) { if (!value) throw std::runtime_error("Extended configuration contract failed"); }
}
int main() {
    try {
        kotavel::Config settings;
        settings.merge("app", {
            {"name", "Kotavel"}, {"service", nullptr}, {"empty", kotavel::ConfigMap{}},
            {"locales", kotavel::ConfigList{"es", "en"}},
            {"servers", kotavel::ConfigList{kotavel::ConfigMap{{"host", "localhost"}, {"port", 8080}}}},
            {"routing", {{"api", {{"port", 8080}}}}},
        });
        settings.merge("empty_group", {});
        check(settings.contains("empty_group"));
        check(settings.group("empty_group")->empty());
        const kotavel::ConfigScope scope(settings);
        check(settings.contains("app.service"));
        check(!settings.contains("missing"));
        check(kotavel::config("app.service").is_null());
        check(std::holds_alternative<std::monostate>(settings.value("app.service")));
        const auto app = kotavel::config<kotavel::ConfigMap>("app");
        check(app.at("name").get<std::string>() == "Kotavel");
        check(app.at("service").is_null());
        check(app.at("routing").get<kotavel::ConfigMap>().at("api").get<kotavel::ConfigMap>().at("port").get<int>() == 8080);
        auto snapshot = kotavel::config("app");
        const auto& list = kotavel::config<kotavel::ConfigList>("app.locales");
        check(list.size() == 2 && list[0].get<std::string>() == "es");
        check(kotavel::config<kotavel::ConfigList>("app.servers")[0].get<kotavel::ConfigMap>().at("port").get<int>() == 8080);
        check(kotavel::config<kotavel::ConfigMap>("app.empty").empty());
        auto many = settings.get_many({"app.name", "app.service", "app.routing", "missing"});
        check(many.at("app.name").get<std::string>() == "Kotavel" && many.at("missing").is_null());
        settings.set_many({{"app.name", std::string("Changed")}, {"app.routing.api.port", 9000}});
        check(settings.get<std::string>("app.name") == "Changed");
        check(kotavel::config<kotavel::ConfigMap>("app").at("routing").get<kotavel::ConfigMap>().at("api").get<kotavel::ConfigMap>().at("port").get<int>() == 9000);
        // Un grupo obtenido antes es una copia estable, independiente del lote.
        check(snapshot.get<kotavel::ConfigMap>().at("name").get<std::string>() == "Kotavel");
        try {
            settings.set_many({{"app.name", std::string("must not persist")}, {"", true}});
            return 1;
        } catch (const std::invalid_argument&) {}
        check(settings.get<std::string>("app.name") == "Changed");
        kotavel::config({{"app.name", std::string("Helper")}, {"app.debug", false}});
        check(kotavel::config<std::string>("app.name") == "Helper");
        settings.set("app.service", nullptr);
        std::ostringstream printed;
        printed << kotavel::config("app.service");
        check(printed.str() == "null");
        // Listas y grupos no vuelcan datos potencialmente privados en consola.
        std::ostringstream container;
        container << kotavel::config("app.servers");
        check(container.str().find("localhost") == std::string::npos);
        // Sustituir un grupo actualiza sus hojas y elimina las anteriores.
        settings.set("app.routing", kotavel::ConfigMap{{"enabled", true}});
        check(settings.get<bool>("app.routing.enabled"));
        check(!settings.contains("app.routing.api.port"));
        settings.set("app.routing", kotavel::ConfigMap{});
        check(kotavel::config<kotavel::ConfigMap>("app.routing").empty());
        check(!settings.contains("app.routing.enabled"));
        std::cout << "Extended configuration tests passed\n";
    } catch (const std::exception& error) { std::cerr << error.what() << '\n'; return 1; }
}
