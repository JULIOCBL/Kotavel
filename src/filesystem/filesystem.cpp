#include <kotavel/filesystem/filesystem.hpp>

#include "filesystem_platform.hpp"

#include <atomic>
#include <fstream>
#include <random>
#include <stdexcept>
#include <system_error>

namespace kotavel::filesystem {
namespace {
class LocalFileCursor final : public contracts::FileCursor {
    std::filesystem::directory_iterator current_;
public:
    explicit LocalFileCursor(const std::filesystem::path& directory) {
        std::error_code error;
        current_ = std::filesystem::directory_iterator(directory, error);
        if (error && error != std::errc::no_such_file_or_directory)
            throw std::system_error(error, "Cannot scan directory");
    }
    std::optional<std::filesystem::path> next() override {
        while (current_ != std::filesystem::directory_iterator{}) {
            const auto entry = *current_;
            std::error_code error;
            current_.increment(error);
            if (error) throw std::system_error(error, "Cannot advance directory scan");
            const bool regular = entry.is_regular_file(error);
            if (error == std::errc::no_such_file_or_directory) continue;
            if (error) throw std::system_error(error, "Cannot inspect directory entry");
            if (regular) return entry.path();
        }
        return std::nullopt;
    }
};

std::filesystem::path temporary_path_for(const std::filesystem::path& destination) {
    static std::atomic_uint64_t counter{0};
    static thread_local std::mt19937_64 generator(std::random_device{}());
    auto temporary = destination;
    temporary += ".kotavel-tmp-" + std::to_string(generator()) + "-" +
                 std::to_string(counter.fetch_add(1, std::memory_order_relaxed));
    return temporary;
}
} // namespace

void LocalFilesystem::create_directories(const std::filesystem::path& path,
                                         bool restrict_permissions) {
    if (path.empty()) throw std::invalid_argument("Directory path cannot be empty");
    std::error_code error;
    const bool created = std::filesystem::create_directories(path, error);
    if (error) throw std::system_error(error, "Cannot create directory");
    if (created || restrict_permissions) detail::restrict_directory_permissions(path);
}

std::optional<std::string> LocalFilesystem::read_file(
    const std::filesystem::path& path, std::size_t maximum_bytes) const {
    std::error_code error;
    const auto size = std::filesystem::file_size(path, error);
    if (error == std::errc::no_such_file_or_directory) return std::nullopt;
    if (error) throw std::system_error(error, "Cannot inspect file");
    if (size > maximum_bytes) throw std::length_error("File exceeds the configured read limit");

    std::ifstream input(path, std::ios::binary);
    if (!input) throw std::runtime_error("Cannot open file");
    std::string bytes(static_cast<std::size_t>(size), '\0');
    if (size && !input.read(bytes.data(), static_cast<std::streamsize>(size)))
        throw std::runtime_error("Cannot read file");
    return bytes;
}

void LocalFilesystem::write_file_atomically(const std::filesystem::path& path,
                                            std::string_view bytes,
                                            bool restrict_permissions) {
    if (path.empty()) throw std::invalid_argument("File path cannot be empty");
    const auto temporary = temporary_path_for(path);
    try {
        detail::write_temp_file(temporary, bytes, restrict_permissions);
        detail::replace_file_atomically(temporary, path);
    } catch (...) {
        std::error_code ignored;
        std::filesystem::remove(temporary, ignored);
        throw;
    }
}

bool LocalFilesystem::remove_file(const std::filesystem::path& path) {
    std::error_code error;
    const bool removed = std::filesystem::remove(path, error);
    if (error == std::errc::no_such_file_or_directory) return false;
    if (error) throw std::system_error(error, "Cannot remove file");
    return removed;
}

void LocalFilesystem::remove_files(const std::vector<std::filesystem::path>& paths) {
    // Local filesystems do not expose a portable multi-unlink primitive. Keep
    // deletion behind one contract call so other backends can optimize it.
    for (const auto& path : paths) (void)remove_file(path);
}

std::vector<std::filesystem::path> LocalFilesystem::list_files(
    const std::filesystem::path& directory) const {
    std::error_code error;
    std::filesystem::directory_iterator current(directory, error), end;
    if (error) throw std::system_error(error, "Cannot list directory");
    std::vector<std::filesystem::path> files;
    for (; current != end; current.increment(error)) {
        if (error) throw std::system_error(error, "Cannot list directory");
        if (current->is_regular_file(error)) files.push_back(current->path());
        if (error) throw std::system_error(error, "Cannot inspect directory entry");
    }
    return files;
}

std::unique_ptr<contracts::FileLock> LocalFilesystem::lock_file(
    const std::filesystem::path& path) {
    return detail::lock_file(path);
}

std::unique_ptr<contracts::FileCursor> LocalFilesystem::scan_files(
    const std::filesystem::path& directory) const {
    return std::make_unique<LocalFileCursor>(directory);
}

} // namespace kotavel::filesystem
