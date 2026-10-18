#pragma once
#include <filesystem>
#include <iosfwd>
#include <map>
#include <optional>
#include <string>
#include <string_view>
#include <variant>

namespace kotavel {
// Una fuente entrega copias: ningún consumidor retiene el puntero de getenv().
class EnvironmentSource {
public:
    virtual ~EnvironmentSource() = default;
    virtual std::optional<std::string> read(std::string_view key) const = 0;
};
class ProcessEnvironment final : public EnvironmentSource {
public:
    std::optional<std::string> read(std::string_view key) const override;
};
// Configuración inmutable del archivo; el proceso tiene prioridad al consultar.
class FileEnvironment final : public EnvironmentSource {
public:
    FileEnvironment(const std::filesystem::path& project_root, std::string_view environment);
    std::optional<std::string> read(std::string_view key) const override;
private:
    ProcessEnvironment process_;
    std::map<std::string, std::string, std::less<>> values_;
};
using EnvValue = std::variant<std::monostate, bool, std::string>;
// Resultado del helper sin valor predeterminado: conserva el tipo y la ausencia,
// y permite imprimirlo sin conversiones manuales ni conversiones implícitas.
class EnvironmentValue final {
public:
    explicit EnvironmentValue(EnvValue value);
    [[nodiscard]] bool is_null() const noexcept;
    [[nodiscard]] const std::string& as_string() const;
    [[nodiscard]] bool as_bool() const;
    friend std::ostream& operator<<(std::ostream& output, const EnvironmentValue& value);
private:
    EnvValue value_;
};
class Env final {
public:
    // La fuente prestada debe vivir más que Env. Leer durante el arranque,
    // antes de iniciar hilos; no modificar el entorno concurrentemente.
    explicit Env(const EnvironmentSource& source) : source_(source) {}
    Env(EnvironmentSource&&) = delete;
    EnvValue get(std::string_view key, EnvValue fallback = {}) const;
    EnvValue require(std::string_view key) const;
    std::string string(std::string_view key, std::string fallback) const;
    bool boolean(std::string_view key, bool fallback) const;
private:
    const EnvironmentSource& source_;
};
struct AppEnvironment {
    std::string name = "Kotavel";
    std::string environment = "production";
    bool debug = false;
    static AppEnvironment load(const Env& env);
    static AppEnvironment load();
};

// Activa una fuente para los helpers del hilo actual y restaura la anterior.
// La fuente debe permanecer viva durante todo el ámbito; no se propaga a otros hilos.
class EnvScope final {
public:
    explicit EnvScope(const Env& reader) noexcept;
    ~EnvScope();
    EnvScope(const EnvScope&) = delete;
    EnvScope& operator=(const EnvScope&) = delete;
    EnvScope(Env&&) = delete;
private:
    const Env* previous_;
};

// Helpers tipados: la sobrecarga de texto evita convertir un literal a bool.
std::string env(std::string_view key, std::string_view fallback);
std::string env(std::string_view key, const char* fallback);
std::string env(std::string_view key, const std::string& fallback);
bool env(std::string_view key, bool fallback);
EnvironmentValue env(std::string_view key);
// Evitar conversiones implícitas engañosas de números a bool.
template<class T> void env(std::string_view, T) = delete;
}
