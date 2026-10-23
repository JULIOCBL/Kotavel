#pragma once

#include <cstddef>
#include <filesystem>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <vector>
#include <utility>

namespace kotavel::contracts {

class FileLock {
public:
    virtual ~FileLock() = default;
};

// A resumable, non-recursive enumeration. Concurrent directory changes may
// become visible on the next traversal. Owners serialize calls to next().
class FileCursor {
public:
    virtual ~FileCursor() = default;
    virtual std::optional<std::filesystem::path> next() = 0;
};

// Storage boundary shared by framework facilities such as sessions and cache.
// Implementations throw std::system_error or std::runtime_error on I/O errors.
class Filesystem {
public:
    virtual ~Filesystem() = default;

    virtual void create_directories(const std::filesystem::path& path,
                                    bool restrict_permissions = false) = 0;
    [[nodiscard]] virtual std::optional<std::string>
    read_file(const std::filesystem::path& path, std::size_t maximum_bytes) const = 0;
    virtual void write_file_atomically(const std::filesystem::path& path,
                                       std::string_view bytes,
                                       bool restrict_permissions = false) = 0;
    [[nodiscard]] virtual bool remove_file(const std::filesystem::path& path) = 0;
    // Batch boundary for stores that need to remove many records. Backends may
    // override this with a native multi-delete; the default preserves existing
    // filesystem implementations.
    virtual void remove_files(const std::vector<std::filesystem::path>& paths) {
        for (const auto& path : paths) (void)remove_file(path);
    }
    [[nodiscard]] virtual std::vector<std::filesystem::path>
    list_files(const std::filesystem::path& directory) const = 0;
    // Compatibility fallback for custom backends; local disk streams entries.
    virtual std::unique_ptr<FileCursor> scan_files(const std::filesystem::path& directory) const {
        class Snapshot final : public FileCursor {
            std::vector<std::filesystem::path> paths_;
            std::size_t position_ = 0;
        public:
            explicit Snapshot(std::vector<std::filesystem::path> paths) : paths_(std::move(paths)) {}
            std::optional<std::filesystem::path> next() override {
                if (position_ == paths_.size()) return std::nullopt;
                return std::move(paths_[position_++]);
            }
        };
        return std::make_unique<Snapshot>(list_files(directory));
    }
    [[nodiscard]] virtual std::unique_ptr<FileLock>
    lock_file(const std::filesystem::path& path) = 0;
};

} // namespace kotavel::contracts
