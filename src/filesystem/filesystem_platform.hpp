#pragma once

#include <kotavel/contracts/filesystem.hpp>

namespace kotavel::filesystem::detail {

void write_temp_file(const std::filesystem::path& path, std::string_view bytes,
                     bool restrict_permissions);
void replace_file_atomically(const std::filesystem::path& temporary,
                             const std::filesystem::path& destination);
void restrict_directory_permissions(const std::filesystem::path& path);
[[nodiscard]] std::unique_ptr<contracts::FileLock>
lock_file(const std::filesystem::path& path);

} // namespace kotavel::filesystem::detail
