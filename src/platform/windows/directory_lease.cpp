#include "../../diagnostics/directory_lease.hpp"

#define NOMINMAX
#include <windows.h>

#include <stdexcept>

namespace kotavel {
namespace {
class WindowsDirectoryLease final : public DirectoryLease {
public:
    explicit WindowsDirectoryLease(const std::filesystem::path& directory) {
        const auto path = directory / ".kotavel.lock";
        handle_ = CreateFileW(path.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                              OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
        if (handle_ == INVALID_HANDLE_VALUE)
            throw std::runtime_error("Log directory is in use or cannot be locked");
    }
    ~WindowsDirectoryLease() override {
        if (handle_ != INVALID_HANDLE_VALUE) CloseHandle(handle_);
    }
private:
    HANDLE handle_ = INVALID_HANDLE_VALUE;
};
}

std::unique_ptr<DirectoryLease> create_directory_lease(
    const std::filesystem::path& directory) {
    return std::make_unique<WindowsDirectoryLease>(directory);
}
} // namespace kotavel
