#pragma once

#include <filesystem>
#include <stdexcept>
#if defined(_WIN32)
#ifndef NOMINMAX
#define NOMINMAX
#endif
#include <windows.h>
#else
#include <fcntl.h>
#include <sys/file.h>
#include <unistd.h>
#endif

namespace kotavel {
// Un único propietario por carpeta. El fichero se conserva: eliminarlo mientras
// se tiene el lock permitiría que otro proceso bloqueara un inode diferente.
class DirectoryLease final {
public:
    explicit DirectoryLease(const std::filesystem::path& directory) {
        const auto path = directory / ".kotavel.lock";
#if defined(_WIN32)
        handle_ = CreateFileW(path.c_str(), GENERIC_READ | GENERIC_WRITE, 0, nullptr,
                              OPEN_ALWAYS, FILE_ATTRIBUTE_NORMAL, nullptr);
        if (handle_ == INVALID_HANDLE_VALUE)
            throw std::runtime_error("Log directory is in use or cannot be locked");
#else
        fd_ = ::open(path.c_str(), O_CREAT | O_RDWR | O_CLOEXEC, 0600);
        if (fd_ < 0) throw std::runtime_error("Cannot open log directory lock");
        if (::flock(fd_, LOCK_EX | LOCK_NB) != 0) {
            ::close(fd_); fd_ = -1;
            throw std::runtime_error("Log directory is already in use");
        }
#endif
    }
    ~DirectoryLease() {
#if defined(_WIN32)
        if (handle_ != INVALID_HANDLE_VALUE) CloseHandle(handle_);
#else
        if (fd_ >= 0) ::close(fd_);
#endif
    }
    DirectoryLease(const DirectoryLease&) = delete;
    DirectoryLease& operator=(const DirectoryLease&) = delete;
private:
#if defined(_WIN32)
    HANDLE handle_ = INVALID_HANDLE_VALUE;
#else
    int fd_ = -1;
#endif
};
} // namespace kotavel
