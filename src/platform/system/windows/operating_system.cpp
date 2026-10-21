#include <kotavel/system/os.hpp>

namespace kotavel {

const OSType OS::type = OSType::WINDOWS;

std::string_view OS::name() noexcept { return "Windows"; }
std::string_view OS::key() noexcept { return "windows"; }
bool OS::isWindows() noexcept { return true; }
bool OS::isMac() noexcept { return false; }
bool OS::isLinux() noexcept { return false; }

} // namespace kotavel
