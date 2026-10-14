#include <kotavel/config/config_file.hpp>
#include <kotavel/config/config_helpers.hpp>

#include <iostream>
#include <stdexcept>
#include <string>
#include <string_view>
#include <thread>
#include <utility>

namespace {
void check(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}

template<class Exception, class Action>
void expect_failure(Action&& action, std::string_view message_part) {
    try {
        action();
    } catch (const Exception& error) {
        const std::string_view message(error.what());
        check(message.find(message_part) != std::string_view::npos,
              "Configuration error did not identify the expected problem");
        check(message.find("edge-test-secret") == std::string_view::npos,
              "Configuration error exposed a stored secret");
        return;
    }
    throw std::runtime_error("Expected configuration failure containing: " +
                             std::string(message_part));
}

kotavel::ConfigMap map_at_depth(unsigned nested_maps) {
    kotavel::ConfigMap result{{"leaf", 7}};
    for (unsigned depth = 0; depth < nested_maps; ++depth)
        result = kotavel::ConfigMap{{"next", std::move(result)}};
    return result;
}

kotavel::ConfigMap load_app_name() { return {{"name", "registered"}}; }
}

int main() {
    try {
        // null, valores falsy y texto parecido a un booleano siguen siendo distintos.
        kotavel::Config values;
        values.set("flags.false", false);
        values.set("flags.zero", 0);
        values.set("flags.empty", std::string{});
        values.set("flags.text", "false");
        values.set("flags.null", nullptr);
        check(values.contains("flags.null"), "Explicit null must still exist");
        check(std::holds_alternative<std::monostate>(values.value("flags.null")),
              "Explicit null must use monostate");
        check(!values.get<bool>("flags.false"), "Stored false changed");
        check(values.get<int>("flags.zero") == 0, "Stored zero changed");
        check(values.get<std::string>("flags.empty").empty(), "Stored empty text changed");
        check(values.get<std::string>("flags.text") == "false", "Text was converted to bool");
        check(values.find("absent") == nullptr && !values.contains("absent"),
              "Missing keys must stay distinct from explicit null");

        // Los nombres con prefijos similares no deben contaminar los grupos.
        values.merge("app", {{"name", "Kotavel"}, {"debug", true}});
        values.merge("application", {{"name", "Other"}});
        const auto app = values.group("app");
        check(app.has_value() && app->size() == 2, "Group prefix matched an unrelated group");
        check(app->at("name").get<std::string>() == "Kotavel", "Wrong group value");
        check(!values.group("missing").has_value(), "Missing group was reported as present");

        // Los errores de merge y de registro no deben dejar datos parciales.
        expect_failure<std::invalid_argument>([&] {
            values.merge("app", {{"name", "edge-test-secret"}});
        }, "app.name");
        check(values.get<std::string>("app.name") == "Kotavel",
              "Failed merge changed the existing configuration");
        expect_failure<std::invalid_argument>([&] { values.merge("bad.group", {}); }, "single name");
        const kotavel::ConfigFile duplicate_groups[] = {
            {"app", load_app_name}, {"app", load_app_name},
        };
        expect_failure<std::invalid_argument>([&] {
            (void)kotavel::load_config_files(duplicate_groups);
        }, "app.name");
        const kotavel::ConfigFile invalid_group[] = {{"App", load_app_name}};
        expect_failure<std::invalid_argument>([&] {
            (void)kotavel::load_config_files(invalid_group);
        }, "lowercase identifier");

        // Los límites aceptan exactamente 32 niveles y 1024 bytes de clave.
        kotavel::Config boundary;
        boundary.merge("depth", map_at_depth(32));
        std::string deepest_key = "depth";
        for (unsigned depth = 0; depth < 32; ++depth) deepest_key += ".next";
        deepest_key += ".leaf";
        check(boundary.get<int>(deepest_key) == 7,
              "Valid depth boundary was not loaded");
        expect_failure<std::invalid_argument>([&] {
            boundary.merge("too_deep", map_at_depth(33));
        }, "32 levels");
        check(!boundary.contains("too_deep.next.leaf"), "Rejected deep map left partial keys");

        const std::string max_key(1022, 'k');
        boundary.merge("g", {{max_key, 11}});
        check(boundary.get<int>("g." + max_key) == 11, "1024-byte key should be accepted");
        const std::string over_key(1023, 'k');
        expect_failure<std::invalid_argument>([&] {
            boundary.merge("g2", {{over_key, 12}});
        }, "1024 bytes");
        check(!boundary.contains("g2." + over_key), "Rejected long key left partial data");

        // La cuenta de nodos permite 10 000 y rechaza el primero que excede el límite.
        kotavel::ConfigMap ten_thousand;
        for (int index = 0; index < 10000; ++index)
            ten_thousand.emplace("key_" + std::to_string(index), index);
        kotavel::Config node_limit;
        node_limit.merge("large", ten_thousand);
        check(node_limit.get<int>("large.key_9999") == 9999,
              "Configuration map at the node limit was truncated");
        ten_thousand.emplace("key_10000", 10000);
        expect_failure<std::invalid_argument>([&] {
            node_limit.merge("too_large", ten_thousand);
        }, "10000 nodes");
        check(!node_limit.contains("too_large.key_0"), "Rejected large map left partial data");

        // Una operación fallida dentro del lote revierte también cambios previos del lote.
        kotavel::Config transactional;
        transactional.merge("app", {{"name", "original"}, {"enabled", true}});
        const auto original = transactional.group("app");
        expect_failure<std::invalid_argument>([&] {
            transactional.set_many({
                {"app.name", std::string("edge-test-secret")},
                {"app", kotavel::Config::Value(map_at_depth(33))},
            });
        }, "32 levels");
        check(transactional.get<std::string>("app.name") == "original",
              "Failed batch published an earlier update");
        check(transactional.group("app")->at("enabled").get<bool>(),
              "Failed group replacement changed existing values");

        // Repetir una clave en un lote sigue el orden y gana la última escritura.
        transactional.set_many({
            {"app.name", std::string("first")},
            {"app.name", std::string("last")},
        });
        check(transactional.get<std::string>("app.name") == "last",
              "Last update did not win in a batch");
        const auto* name_before_empty_batch = &transactional.get<std::string>("app.name");
        transactional.set_many({});
        check(name_before_empty_batch == &transactional.get<std::string>("app.name"),
              "Empty batch unexpectedly replaced stored values");

        // get_many preserva los nombres, agrupa prefijos y compacta claves repetidas.
        const auto selected = transactional.get_many({
            "app.name", "app", "app.name", "flags.null", "never.present",
        });
        check(selected.size() == 4, "Repeated get_many keys were not collapsed");
        check(selected.at("app.name").get<std::string>() == "last", "get_many lost a scalar");
        check(selected.at("app").get<kotavel::ConfigMap>().at("enabled").get<bool>(),
              "get_many failed to reconstruct a group");
        check(selected.at("flags.null").is_null() && selected.at("never.present").is_null(),
              "get_many did not represent null and absence");

        // El helper mantiene defaults estrictos y restaura ámbitos anidados.
        {
            const kotavel::ConfigScope scope(transactional);
            check(kotavel::config("not.present", 15) == 15, "Missing key ignored its default");
            expect_failure<std::invalid_argument>([&] {
                (void)kotavel::config("app.enabled", std::string("wrong type"));
            }, "app.enabled");
            kotavel::Config nested;
            nested.set("app.name", "nested");
            {
                const kotavel::ConfigScope nested_scope(nested);
                check(kotavel::config<std::string>("app.name") == "nested",
                      "Nested ConfigScope did not activate its instance");
            }
            check(kotavel::config<std::string>("app.name") == "last",
                  "Nested ConfigScope did not restore the previous instance");
        }
        expect_failure<std::logic_error>([] { (void)kotavel::config(); }, "No active");

        std::cout << "Config edge tests passed\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
