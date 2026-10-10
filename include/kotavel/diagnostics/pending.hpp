#pragma once

#include <chrono>
#include <cstddef>
#include <string_view>

namespace kotavel {

// Contratos reservados: no configuran ni activan servicios. El resultado
// explícito evita que el consumidor suponga que una capacidad ya funciona.
enum class FeatureStatus { not_implemented };
[[nodiscard]] FeatureStatus configure_remote_logging(std::string_view endpoint) noexcept;
[[nodiscard]] FeatureStatus enable_fatal_system_capture() noexcept;

} // namespace kotavel
