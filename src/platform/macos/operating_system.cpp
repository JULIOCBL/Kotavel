#include <kotavel/system/os.hpp>

namespace kotavel {

const OSType OS::type = OSType::MACOS;

std::string_view OS::name() noexcept { return "macOS"; }
std::string_view OS::key() noexcept { return "macos"; }
bool OS::isWindows() noexcept { return false; }
bool OS::isMac() noexcept { return true; }
bool OS::isLinux() noexcept { return false; }

} // namespace kotavel
