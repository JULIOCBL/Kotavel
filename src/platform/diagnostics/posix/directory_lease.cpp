#include "../../../diagnostics/directory_lease.hpp"

#include <fcntl.h>
#include <sys/file.h>
#include <unistd.h>

#include <stdexcept>

namespace kotavel {
namespace {
class PosixDirectoryLease final : public DirectoryLease {
public:
    explicit PosixDirectoryLease(const std::filesystem::path& directory) {
        const auto path = directory / ".kotavel.lock";
        fd_ = ::open(path.c_str(), O_CREAT | O_RDWR | O_CLOEXEC, 0600);
        if (fd_ < 0) throw std::runtime_error("Cannot open log directory lock");
        if (::flock(fd_, LOCK_EX | LOCK_NB) != 0) {
            ::close(fd_);
            fd_ = -1;
            throw std::runtime_error("Log directory is already in use");
        }
    }
    ~PosixDirectoryLease() override {
        if (fd_ >= 0) ::close(fd_);
    }
private:
    int fd_ = -1;
};
}

std::unique_ptr<DirectoryLease> create_directory_lease(
    const std::filesystem::path& directory) {
    return std::make_unique<PosixDirectoryLease>(directory);
}
} // namespace kotavel
