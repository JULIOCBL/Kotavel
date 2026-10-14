#pragma once

#include <kotavel/config/config_value.hpp>
#include <optional>
#include <span>
#include <functional>
#include <map>
#include <stdexcept>
#include <string>
#include <string_view>
#include <type_traits>
#include <unordered_map>
#include <variant>

namespace kotavel {
// Instancia explícita: claves completas con puntos, sin árbol ni estado global.
// Lecturas concurrentes solo si ningún hilo modifica el objeto. Para escribir,
// el llamador debe excluir también lectores y referencias prestadas activas.
class Config final {
public:
    using Value = ConfigNode::Value;
    struct Update { std::string_view key; Value value; };

    // set modifica exclusivamente memoria; nunca registra ni persiste valores.
    void set(std::string_view key, Value value);
    void set(std::string_view key, std::nullptr_t);
    // Lote atómico: cualquier fallo deja los valores originales intactos.
    // Un lote exitoso invalida todas las referencias/vistas previas de esta instancia.
    void set_many(std::span<const Update> updates);
    void set_many(std::initializer_list<Update> updates) {
        set_many(std::span<const Update>(updates.begin(), updates.size()));
    }
    // Copias propias para resultados múltiples; las ausencias se representan con null.
    [[nodiscard]] ConfigMap get_many(std::span<const std::string_view> keys) const;
    [[nodiscard]] ConfigMap get_many(std::initializer_list<std::string_view> keys) const {
        return get_many(std::span<const std::string_view>(keys.begin(), keys.size()));
    }
    // Reconstrucción bajo demanda; no añade coste a las consultas de hojas.
    [[nodiscard]] std::optional<ConfigMap> group(std::string_view prefix) const;
    // Sobrecargas exactas evitan conversiones ambiguas de 0, double y bool.
    void set(std::string_view key, int value);
    void set(std::string_view key, double value);
    void set(std::string_view key, bool value);
    void set(std::string_view key, std::string value);
    void set(std::string_view key, const char* value);
    void set(std::string_view key, std::string_view value);
    [[nodiscard]] bool contains(std::string_view key) const;
    // Consulta opcional sin excepciones por ausencia ni creación de cadenas.
    [[nodiscard]] const Value* find(std::string_view key) const &;
    const Value* find(std::string_view) const && = delete;
    // Cargar un grupo anidado una vez; rechazar colisiones de claves completas.
    void merge(std::string_view group, const ConfigMap& map);
    [[nodiscard]] const Value& value(std::string_view key) const & { return find_value(key); }
    const Value& value(std::string_view) const && = delete;

    // Sin conversiones implícitas. La referencia vive hasta actualizar esa clave,
    // asignar/mover/destruir Config, reemplazar un grupo o ejecutar set_many.
    // Insertar otras hojas escalares no la invalida.
    template<class T>
    [[nodiscard]] const T& get(std::string_view key) const & {
        static_assert(std::is_same_v<T, std::string> || std::is_same_v<T, int> ||
                      std::is_same_v<T, double> || std::is_same_v<T, bool> ||
                      std::is_same_v<T, ConfigList> || std::is_same_v<T, ConfigMap> ||
                      std::is_same_v<T, std::monostate>,
                      "Config supports scalar values, null, ConfigList and ConfigMap");
        const auto& value = find_value(key);
        if (const auto result = std::get_if<T>(&value)) return *result;
        throw std::invalid_argument("Configuration type mismatch for key: " + std::string(key));
    }
    // Evitar devolver una referencia prestada desde un objeto temporal.
    template<class T> const T& get(std::string_view) const && = delete;

private:
    struct KeyHash {
        using is_transparent = void;
        std::size_t operator()(std::string_view key) const noexcept {
            return std::hash<std::string_view>{}(key);
        }
    };
    [[nodiscard]] const Value& find_value(std::string_view key) const;
    std::unordered_map<std::string, Value, KeyHash, std::equal_to<>> values_;
};
}
