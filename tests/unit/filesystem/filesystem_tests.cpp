#include <kotavel/filesystem/filesystem.hpp>

#include <chrono>
#include <filesystem>
#include <iostream>
#include <stdexcept>
#include <string>

namespace {
int failures = 0;

void expect(bool condition, const char* message) {
    if (!condition) {
        ++failures;
        std::cerr << "FAIL: " << message << '\n';
    }
}

std::filesystem::path test_root() {
    const auto stamp = std::chrono::steady_clock::now().time_since_epoch().count();
    return std::filesystem::temp_directory_path() /
        ("kotavel-filesystem-tests-" + std::to_string(stamp));
}

void test_local_filesystem_operations() {
    kotavel::filesystem::LocalFilesystem filesystem;
    const auto root = test_root();
    const auto nested = root / "storage" / "sessions";
    const auto target = nested / "record.json";

    try {
        filesystem.create_directories(nested);
        expect(std::filesystem::is_directory(nested),
               "creates nested directory paths");
        expect(!filesystem.read_file(target, 1024).has_value(),
               "returns an empty optional for a missing file");

        filesystem.write_file_atomically(target, "first value");
        expect(filesystem.read_file(target, 1024) == "first value",
               "writes and reads file contents");
        filesystem.write_file_atomically(target, "replacement");
        expect(filesystem.read_file(target, 1024) == "replacement",
               "atomically replaces existing content");

        bool size_limit_enforced = false;
        try { (void)filesystem.read_file(target, 3); }
        catch (const std::length_error&) { size_limit_enforced = true; }
        expect(size_limit_enforced, "rejects reads larger than the caller's limit");

        const auto files = filesystem.list_files(nested);
        expect(files.size() == 1 && files.front() == target,
               "lists regular files in a directory");
        const auto second = nested / "second.json";
        filesystem.write_file_atomically(second, "second value");
        filesystem.create_directories(nested / "child");
        auto cursor = filesystem.scan_files(nested);
        const auto first_entry = cursor->next();
        expect(first_entry.has_value(), "streaming enumeration returns its first file");
        if (first_entry) (void)filesystem.remove_file(*first_entry);
        const auto next_entry = cursor->next();
        expect(next_entry && next_entry != first_entry,
               "cursor resumes after deleting a returned file");
        expect(!cursor->next(), "cursor skips directories and terminates");
        expect(!filesystem.scan_files(root / "missing")->next(),
               "missing directory has an empty cursor");
        filesystem.remove_files({target, second});
        expect(!std::filesystem::exists(target) && !std::filesystem::exists(second),
               "removes a batch of files through the filesystem contract");
        expect(!filesystem.remove_file(target), "missing file removal is idempotent");

        const auto lock_path = nested / ".locks" / "record.lock";
        filesystem.create_directories(lock_path.parent_path(), true);
        {
            auto lock = filesystem.lock_file(lock_path);
            expect(lock != nullptr && std::filesystem::exists(lock_path),
                   "creates and acquires a native file lock");
        }

        std::error_code ignored;
        std::filesystem::remove_all(root, ignored);
        filesystem.create_directories(nested);
        expect(std::filesystem::is_directory(nested),
               "recreates a directory removed while the filesystem object is alive");
    } catch (const std::exception& error) {
        ++failures;
        std::cerr << "FAIL: unexpected filesystem error: " << error.what() << '\n';
        std::error_code ignored;
        std::filesystem::remove_all(root, ignored);
    }
}
}

int main() {
    test_local_filesystem_operations();
    if (failures != 0) std::cerr << failures << " filesystem test(s) failed\n";
    return failures == 0 ? 0 : 1;
}
