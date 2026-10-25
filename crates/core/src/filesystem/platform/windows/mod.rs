use std::{
    fs::{File, OpenOptions},
    io,
    os::windows::{ffi::OsStrExt, io::AsRawHandle},
    path::Path,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, HANDLE, LocalFree},
    Security::Authorization::{
        EXPLICIT_ACCESS_W, SE_FILE_OBJECT, SET_ACCESS, SetEntriesInAclW, SetNamedSecurityInfoW,
        SetSecurityInfo, TRUSTEE_IS_SID, TRUSTEE_IS_USER, TRUSTEE_W,
    },
    Security::{
        DACL_SECURITY_INFORMATION, GetTokenInformation, NO_INHERITANCE,
        PROTECTED_DACL_SECURITY_INFORMATION, SUB_CONTAINERS_AND_OBJECTS_INHERIT, TOKEN_QUERY,
        TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::FILE_ALL_ACCESS,
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct Acl(*mut windows_sys::Win32::Security::ACL);
impl Drop for Acl {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0.cast());
        }
    }
}

fn private_user_acl(inheritance: u32) -> io::Result<Acl> {
    unsafe {
        let mut token = null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let token = Token(token);
        let mut required = 0;
        let _ = GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut required);
        if required == 0 {
            return Err(io::Error::last_os_error());
        }
        let word_count = (required as usize).div_ceil(std::mem::size_of::<usize>());
        let mut buffer = vec![0usize; word_count];
        if GetTokenInformation(
            token.0,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        ) == 0
        {
            return Err(io::Error::last_os_error());
        }
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let entry = EXPLICIT_ACCESS_W {
            grfAccessPermissions: FILE_ALL_ACCESS,
            grfAccessMode: SET_ACCESS,
            grfInheritance: inheritance,
            Trustee: TRUSTEE_W {
                TrusteeForm: TRUSTEE_IS_SID,
                TrusteeType: TRUSTEE_IS_USER,
                ptstrName: user.User.Sid.cast(),
                ..Default::default()
            },
        };
        let mut acl = null_mut();
        let result = SetEntriesInAclW(1, &entry, null(), &mut acl);
        if result != 0 {
            return Err(io::Error::from_raw_os_error(result as i32));
        }
        Ok(Acl(acl))
    }
}

pub fn restrict_directory(path: &Path) -> io::Result<()> {
    apply_acl_to_path(path, SUB_CONTAINERS_AND_OBJECTS_INHERIT)
}

pub fn restrict_file_path(path: &Path) -> io::Result<()> {
    apply_acl_to_path(path, NO_INHERITANCE)
}

fn apply_acl_to_path(path: &Path, inheritance: u32) -> io::Result<()> {
    let acl = private_user_acl(inheritance)?;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let flags = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
    let result = unsafe {
        SetNamedSecurityInfoW(
            wide.as_ptr(),
            SE_FILE_OBJECT,
            flags,
            null_mut(),
            null_mut(),
            acl.0,
            null_mut(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32))
    }
}

pub fn restrict_file(file: &File) -> io::Result<()> {
    let acl = private_user_acl(NO_INHERITANCE)?;
    let flags = DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION;
    let handle = file.as_raw_handle() as HANDLE;
    let result = unsafe {
        SetSecurityInfo(
            handle,
            SE_FILE_OBJECT,
            flags,
            null_mut(),
            null_mut(),
            acl.0,
            null_mut(),
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result as i32))
    }
}

pub fn sync_directory(_path: &Path) -> io::Result<()> {
    Ok(())
}

pub fn replace_file_atomically(temporary: &Path, destination: &Path) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let temporary: Vec<u16> = temporary.as_os_str().encode_wide().chain(Some(0)).collect();
    let destination: Vec<u16> = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let flags = MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH;
    if unsafe { MoveFileExW(temporary.as_ptr(), destination.as_ptr(), flags) } != 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(
            unsafe { GetLastError() } as i32
        ))
    }
}

pub fn open_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .read(true)
        .write(true)
        .truncate(false)
        .open(path)
}
