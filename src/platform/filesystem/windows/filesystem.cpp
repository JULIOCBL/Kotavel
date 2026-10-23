#include "../../../filesystem/filesystem_platform.hpp"

#include <windows.h>
#include <aclapi.h>

#include <algorithm>
#include <stdexcept>
#include <system_error>
#include <vector>

namespace kotavel::filesystem::detail {
namespace {
[[noreturn]] void throw_windows_error(const char* message, DWORD error = ::GetLastError()) {
    throw std::system_error(static_cast<int>(error), std::system_category(), message);
}

class WindowsFileLock final : public contracts::FileLock {
public:
    explicit WindowsFileLock(const std::filesystem::path& path) {
        handle_ = ::CreateFileW(path.c_str(), GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE, nullptr, OPEN_ALWAYS,
            FILE_ATTRIBUTE_NORMAL, nullptr);
        if (handle_ == INVALID_HANDLE_VALUE) throw_windows_error("Cannot open lock file");
        if (!::LockFileEx(handle_, LOCKFILE_EXCLUSIVE_LOCK, 0, MAXDWORD, MAXDWORD, &overlapped_)) {
            const auto error = ::GetLastError();
            ::CloseHandle(handle_);
            handle_ = INVALID_HANDLE_VALUE;
            throw_windows_error("Cannot lock file", error);
        }
    }
    ~WindowsFileLock() override {
        if (handle_ != INVALID_HANDLE_VALUE) {
            ::UnlockFileEx(handle_, 0, MAXDWORD, MAXDWORD, &overlapped_);
            ::CloseHandle(handle_);
        }
    }
private:
    HANDLE handle_ = INVALID_HANDLE_VALUE;
    OVERLAPPED overlapped_{};
};

PACL private_user_acl() {
    HANDLE token = nullptr;
    if (!::OpenProcessToken(::GetCurrentProcess(), TOKEN_QUERY, &token))
        throw_windows_error("Cannot read current Windows user");
    DWORD required = 0;
    ::GetTokenInformation(token, TokenUser, nullptr, 0, &required);
    if (::GetLastError() != ERROR_INSUFFICIENT_BUFFER) {
        const auto error = ::GetLastError();
        ::CloseHandle(token);
        throw_windows_error("Cannot read current Windows user", error);
    }
    std::vector<unsigned char> token_user_bytes(required);
    if (!::GetTokenInformation(token, TokenUser, token_user_bytes.data(), required, &required)) {
        const auto error = ::GetLastError();
        ::CloseHandle(token);
        throw_windows_error("Cannot read current Windows user", error);
    }
    ::CloseHandle(token);
    const auto* token_user = reinterpret_cast<const TOKEN_USER*>(token_user_bytes.data());

    EXPLICIT_ACCESSW entry{};
    entry.grfAccessPermissions = FILE_ALL_ACCESS;
    entry.grfAccessMode = SET_ACCESS;
    entry.grfInheritance = NO_INHERITANCE;
    entry.Trustee.TrusteeForm = TRUSTEE_IS_SID;
    entry.Trustee.TrusteeType = TRUSTEE_IS_USER;
    entry.Trustee.ptstrName = static_cast<LPWSTR>(token_user->User.Sid);
    PACL acl = nullptr;
    const auto error = ::SetEntriesInAclW(1, &entry, nullptr, &acl);
    if (error != ERROR_SUCCESS) throw_windows_error("Cannot create private file ACL", error);
    return acl;
}
}

void write_temp_file(const std::filesystem::path& path, std::string_view bytes,
                     bool restrict_permissions) {
    PACL acl = restrict_permissions ? private_user_acl() : nullptr;
    SECURITY_DESCRIPTOR security_descriptor{};
    SECURITY_ATTRIBUTES attributes{sizeof(SECURITY_ATTRIBUTES), nullptr, FALSE};
    if (restrict_permissions) {
        if (!::InitializeSecurityDescriptor(&security_descriptor, SECURITY_DESCRIPTOR_REVISION)) {
            const auto error = ::GetLastError();
            ::LocalFree(acl);
            throw_windows_error("Cannot initialize private file security", error);
        }
        if (!::SetSecurityDescriptorDacl(&security_descriptor, TRUE, acl, FALSE)) {
            const auto error = ::GetLastError();
            ::LocalFree(acl);
            throw_windows_error("Cannot set private file security", error);
        }
        attributes.lpSecurityDescriptor = &security_descriptor;
    }
    const HANDLE handle = ::CreateFileW(path.c_str(), GENERIC_WRITE, 0,
        restrict_permissions ? &attributes : nullptr, CREATE_NEW, FILE_ATTRIBUTE_TEMPORARY, nullptr);
    const auto open_error = handle == INVALID_HANDLE_VALUE ? ::GetLastError() : ERROR_SUCCESS;
    if (acl) ::LocalFree(acl);
    if (handle == INVALID_HANDLE_VALUE) throw_windows_error("Cannot create private file", open_error);
    std::size_t offset = 0;
    while (offset < bytes.size()) {
        const auto requested = static_cast<DWORD>(std::min<std::size_t>(
            bytes.size() - offset, static_cast<std::size_t>(MAXDWORD)));
        DWORD written = 0;
        if (!::WriteFile(handle, bytes.data() + offset, requested, &written, nullptr) || written == 0) {
            const auto error = ::GetLastError();
            ::CloseHandle(handle);
            throw_windows_error("Cannot write private file", error);
        }
        offset += written;
    }
    if (!::FlushFileBuffers(handle)) {
        const auto error = ::GetLastError();
        ::CloseHandle(handle);
        throw_windows_error("Cannot sync private file", error);
    }
    if (!::CloseHandle(handle)) throw_windows_error("Cannot close private file");
}

void replace_file_atomically(const std::filesystem::path& temporary,
                             const std::filesystem::path& destination) {
    if (!::MoveFileExW(temporary.c_str(), destination.c_str(),
                       MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH))
        throw_windows_error("Cannot atomically replace file");
}

void restrict_directory_permissions(const std::filesystem::path& path) {
    HANDLE token = nullptr;
    if (!::OpenProcessToken(::GetCurrentProcess(), TOKEN_QUERY, &token))
        throw_windows_error("Cannot read current Windows user");
    DWORD required = 0;
    ::GetTokenInformation(token, TokenUser, nullptr, 0, &required);
    if (::GetLastError() != ERROR_INSUFFICIENT_BUFFER) {
        const auto error = ::GetLastError();
        ::CloseHandle(token);
        throw_windows_error("Cannot read current Windows user", error);
    }
    std::vector<unsigned char> token_user_bytes(required);
    if (!::GetTokenInformation(token, TokenUser, token_user_bytes.data(), required, &required)) {
        const auto error = ::GetLastError();
        ::CloseHandle(token);
        throw_windows_error("Cannot read current Windows user", error);
    }
    ::CloseHandle(token);
    const auto* token_user = reinterpret_cast<const TOKEN_USER*>(token_user_bytes.data());

    EXPLICIT_ACCESSW entry{};
    entry.grfAccessPermissions = FILE_ALL_ACCESS;
    entry.grfAccessMode = SET_ACCESS;
    entry.grfInheritance = SUB_CONTAINERS_AND_OBJECTS_INHERIT;
    entry.Trustee.TrusteeForm = TRUSTEE_IS_SID;
    entry.Trustee.TrusteeType = TRUSTEE_IS_USER;
    entry.Trustee.ptstrName = static_cast<LPWSTR>(token_user->User.Sid);
    PACL acl = nullptr;
    const auto acl_error = ::SetEntriesInAclW(1, &entry, nullptr, &acl);
    if (acl_error != ERROR_SUCCESS) throw_windows_error("Cannot create private directory ACL", acl_error);
    const auto security_error = ::SetNamedSecurityInfoW(
        const_cast<LPWSTR>(path.c_str()), SE_FILE_OBJECT,
        DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
        nullptr, nullptr, acl, nullptr);
    ::LocalFree(acl);
    if (security_error != ERROR_SUCCESS)
        throw_windows_error("Cannot restrict directory permissions", security_error);
}

std::unique_ptr<contracts::FileLock> lock_file(const std::filesystem::path& path) {
    return std::make_unique<WindowsFileLock>(path);
}
} // namespace kotavel::filesystem::detail
