#pragma once
#include <initializer_list>
#include <map>
#include <stdexcept>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

namespace kotavel {
struct ConfigNode;
using ConfigMap = std::map<std::string, ConfigNode, std::less<>>;
using ConfigList = std::vector<ConfigNode>;

// Valor recursivo de entrada y de listas. Los grupos del almacén se aplanan.
struct ConfigNode {
    using Value = std::variant<std::monostate, std::string, int, double, bool, ConfigMap, ConfigList>;
    Value value;
    ConfigNode() = default;
    ConfigNode(std::nullptr_t) : value(std::monostate{}) {}
    ConfigNode(std::monostate) : value(std::monostate{}) {}
    ConfigNode(const char* text) : value(std::string(text ? text : "")) {
        if (!text) throw std::invalid_argument("Configuration string cannot be null; use nullptr explicitly");
    }
    ConfigNode(std::string text) : value(std::move(text)) {}
    ConfigNode(std::string_view text) : ConfigNode(std::string(text)) {}
    ConfigNode(int number) : value(number) {}
    ConfigNode(double number) : value(number) {}
    ConfigNode(bool boolean) : value(boolean) {}
    ConfigNode(ConfigMap map) : value(std::move(map)) {}
    ConfigNode(ConfigList list) : value(std::move(list)) {}
    ConfigNode(Value data) : value(std::move(data)) {}
    // Las llaves sin tipo representan un grupo. Para listas usa ConfigList{...}.
    ConfigNode(std::initializer_list<ConfigMap::value_type> entries) : value(ConfigMap(entries)) {}
    [[nodiscard]] bool is_null() const noexcept { return std::holds_alternative<std::monostate>(value); }
    template<class T> const T& get() const & {
        if (const auto item = std::get_if<T>(&value)) return *item;
        throw std::invalid_argument("Configuration node type mismatch");
    }
    template<class T> const T& get() const && = delete;
};
}
