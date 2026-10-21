#include <kotavel/system/os.hpp>

namespace kotavel {

const OSType OS::type = OSType::LINUX;

std::string_view OS::name() noexcept { return "Linux"; }
std::string_view OS::key() noexcept { return "linux"; }
bool OS::isWindows() noexcept { return false; }
bool OS::isMac() noexcept { return false; }
bool OS::isLinux() noexcept { return true; }

} // namespace kotavel
