#include <kotavel/system/os.hpp>

using kotavel::OS;
using kotavel::OSType;

#if defined(_WIN32)
static_assert(OS::type == OSType::WINDOWS);
static_assert(OS::name() == "Windows");
static_assert(OS::key() == "windows");
static_assert(OS::isWindows() && !OS::isMac() && !OS::isLinux());
#elif defined(__APPLE__) && defined(__MACH__)
static_assert(OS::type == OSType::MACOS);
static_assert(OS::name() == "macOS");
static_assert(OS::key() == "macos");
static_assert(!OS::isWindows() && OS::isMac() && !OS::isLinux());
#elif defined(__linux__)
static_assert(OS::type == OSType::LINUX);
static_assert(OS::name() == "Linux");
static_assert(OS::key() == "linux");
static_assert(!OS::isWindows() && !OS::isMac() && OS::isLinux());
#else
static_assert(OS::type == OSType::UNKNOWN);
static_assert(OS::name() == "Unknown");
static_assert(OS::key() == "unknown");
static_assert(!OS::isWindows() && !OS::isMac() && !OS::isLinux());
#endif

int main() { return 0; }
