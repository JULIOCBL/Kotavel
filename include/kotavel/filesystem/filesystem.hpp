#pragma once

#include <kotavel/contracts/filesystem.hpp>

namespace kotavel::filesystem {

// Local disk implementation used by the framework's file-backed facilities.
// Platform-sensitive locking, atomic writes, and permissions are supplied by
// the native adapter selected by CMake.
class LocalFilesystem final : public contracts::Filesystem {
public:
    void create_directories(const std::filesystem::path& path,
                            bool restrict_permissions = false) override;
    [[nodiscard]] std::optional<std::string>
    read_file(const std::filesystem::path& path, std::size_t maximum_bytes) const override;
    void write_file_atomically(const std::filesystem::path& path,
                               std::string_view bytes,
                               bool restrict_permissions = false) override;
    [[nodiscard]] bool remove_file(const std::filesystem::path& path) override;
    void remove_files(const std::vector<std::filesystem::path>& paths) override;
    [[nodiscard]] std::vector<std::filesystem::path>
    list_files(const std::filesystem::path& directory) const override;
    [[nodiscard]] std::unique_ptr<contracts::FileCursor>
    scan_files(const std::filesystem::path& directory) const override;
    [[nodiscard]] std::unique_ptr<contracts::FileLock>
    lock_file(const std::filesystem::path& path) override;
};

} // namespace kotavel::filesystem
