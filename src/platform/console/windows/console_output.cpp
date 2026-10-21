#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>

#include "../../platform_services.hpp"

namespace kotavel::platform::detail {

void configure_console_output() noexcept {
    ::SetConsoleOutputCP(CP_UTF8);
}

} // namespace kotavel::platform::detail
