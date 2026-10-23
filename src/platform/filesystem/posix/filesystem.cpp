#include "../../../filesystem/filesystem_platform.hpp"

#include <cerrno>
#include <fcntl.h>
#include <stdexcept>
#include <system_error>
#include <sys/file.h>
#include <sys/stat.h>
#include <unistd.h>

namespace kotavel::filesystem::detail {
namespace {
class PosixFileLock final : public contracts::FileLock {
public:
    explicit PosixFileLock(const std::filesystem::path& path) {
        descriptor_ = ::open(path.c_str(), O_CREAT | O_RDWR, S_IRUSR | S_IWUSR);
        if (descriptor_ < 0)
            throw std::system_error(errno, std::generic_category(), "Cannot open lock file");
        while (::flock(descriptor_, LOCK_EX) != 0) {
            if (errno == EINTR) continue;
            const int error = errno;
            ::close(descriptor_);
            descriptor_ = -1;
            throw std::system_error(error, std::generic_category(), "Cannot lock file");
        }
    }
    ~PosixFileLock() override {
        if (descriptor_ >= 0) {
            ::flock(descriptor_, LOCK_UN);
            ::close(descriptor_);
        }
    }
private:
    int descriptor_ = -1;
};
}

void write_temp_file(const std::filesystem::path& path, std::string_view bytes,
                     bool restrict_permissions) {
    const mode_t permissions = restrict_permissions
        ? static_cast<mode_t>(S_IRUSR | S_IWUSR)
        : static_cast<mode_t>(S_IRUSR | S_IWUSR | S_IRGRP | S_IWGRP | S_IROTH | S_IWOTH);
    const int descriptor = ::open(path.c_str(), O_WRONLY | O_CREAT | O_EXCL, permissions);
    if (descriptor < 0)
        throw std::system_error(errno, std::generic_category(), "Cannot create private file");
    std::size_t offset = 0;
    while (offset < bytes.size()) {
        const auto count = ::write(descriptor, bytes.data() + offset, bytes.size() - offset);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) {
            const int error = count < 0 ? errno : EIO;
            ::close(descriptor);
            throw std::system_error(error, std::generic_category(), "Cannot write private file");
        }
        offset += static_cast<std::size_t>(count);
    }
    if (::fsync(descriptor) != 0) {
        const int error = errno;
        ::close(descriptor);
        throw std::system_error(error, std::generic_category(), "Cannot sync private file");
    }
    if (::close(descriptor) != 0)
        throw std::system_error(errno, std::generic_category(), "Cannot close private file");
}

void replace_file_atomically(const std::filesystem::path& temporary,
                             const std::filesystem::path& destination) {
    if (::rename(temporary.c_str(), destination.c_str()) != 0)
        throw std::system_error(errno, std::generic_category(), "Cannot atomically replace file");
}

void restrict_directory_permissions(const std::filesystem::path& path) {
    if (::chmod(path.c_str(), S_IRWXU) != 0)
        throw std::system_error(errno, std::generic_category(), "Cannot restrict directory permissions");
}

std::unique_ptr<contracts::FileLock> lock_file(const std::filesystem::path& path) {
    return std::make_unique<PosixFileLock>(path);
}
} // namespace kotavel::filesystem::detail
