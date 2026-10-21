#include <kotavel/system/os.hpp>

#include <iostream>
#include <stdexcept>

namespace {
void check(bool condition, const char* message) {
    if (!condition) throw std::runtime_error(message);
}
}

int main() {
    try {
        using kotavel::OS;
        using kotavel::OSType;

        switch (OS::type) {
        case OSType::WINDOWS:
            check(OS::name() == "Windows" && OS::key() == "windows" &&
                      OS::isWindows() && !OS::isMac() && !OS::isLinux(),
                  "Windows adapter identity must be internally consistent");
            break;
        case OSType::MACOS:
            check(OS::name() == "macOS" && OS::key() == "macos" &&
                      !OS::isWindows() && OS::isMac() && !OS::isLinux(),
                  "macOS adapter identity must be internally consistent");
            break;
        case OSType::LINUX:
            check(OS::name() == "Linux" && OS::key() == "linux" &&
                      !OS::isWindows() && !OS::isMac() && OS::isLinux(),
                  "Linux adapter identity must be internally consistent");
            break;
        case OSType::UNKNOWN:
            throw std::runtime_error("A supported build must not select UNKNOWN");
        }

        const int enabled_flags = static_cast<int>(OS::isWindows()) +
            static_cast<int>(OS::isMac()) + static_cast<int>(OS::isLinux());
        check(enabled_flags == 1, "Exactly one platform predicate must be true");
        std::cout << "Platform adapter identity: OK\n";
        return 0;
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
}
