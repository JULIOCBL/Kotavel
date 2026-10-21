#pragma once

#include <filesystem>
#include <memory>

namespace kotavel {
class DirectoryLease {
public:
    virtual ~DirectoryLease() = default;
    DirectoryLease(const DirectoryLease&) = delete;
    DirectoryLease& operator=(const DirectoryLease&) = delete;
protected:
    DirectoryLease() = default;
};

// Un único propietario por carpeta. El fichero se conserva para mantener estable el lock.
[[nodiscard]] std::unique_ptr<DirectoryLease>
create_directory_lease(const std::filesystem::path& directory);
} // namespace kotavel
