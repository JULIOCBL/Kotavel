#pragma once

#include <kotavel/desktop/desktop.hpp>

namespace kotavel::desktop {

// Crea el adaptador Qt Quick opcional; solo existe al habilitar Kotavel::DesktopQt.
[[nodiscard]] std::unique_ptr<DesktopPlatform> make_qt_platform();

} // namespace kotavel::desktop
