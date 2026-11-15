//! Native Windows file protection shared by wallet keys and consumer credentials.
//! A protected DACL grants only the current token user; no shell commands or ACL inheritance.
use std::{
    ffi::c_void,
    fs::File,
    mem::size_of,
    os::windows::{
        ffi::OsStrExt,
        fs::MetadataExt,
        io::{AsRawHandle, FromRawHandle},
    },
    path::Path,
    ptr::{null, null_mut},
};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE},
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        unsafe {
            LocalFree(self.0);
        }
    }
}
struct Token(windows_sys::Win32::Foundation::HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct User {
    storage: Vec<u64>,
}
impl User {
    fn new() -> Result<Self, String> {
        unsafe {
            let mut token = null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return Err("cannot inspect Windows user token".into());
            }
            let token = Token(token);
            let mut length = 0;
            GetTokenInformation(token.0, TokenUser, null_mut(), 0, &mut length);
            if length == 0 || length > 64 * 1024 {
                return Err("invalid Windows user token size".into());
            }
            let mut storage = vec![0u64; (length as usize + 7) / 8];
            if GetTokenInformation(
                token.0,
                TokenUser,
                storage.as_mut_ptr().cast(),
                length,
                &mut length,
            ) == 0
            {
                return Err("cannot read Windows user token".into());
            }
            Ok(Self { storage })
        }
    }
    fn sid(&self) -> PSID {
        unsafe { (*(self.storage.as_ptr().cast::<TOKEN_USER>())).User.Sid }
    }
    fn descriptor(&self, directory: bool) -> Result<Local, String> {
        unsafe {
            let mut sid_text = null_mut();
            if ConvertSidToStringSidW(self.sid(), &mut sid_text) == 0 {
                return Err("cannot encode Windows user identity".into());
            }
            let allocation = Local(sid_text.cast());
            let mut len = 0;
            while len < 256 && *sid_text.add(len) != 0 {
                len += 1;
            }
            if len == 256 {
                return Err("invalid Windows user identity".into());
            }
            let sid = String::from_utf16(std::slice::from_raw_parts(sid_text, len))
                .map_err(|_| "invalid Windows user identity")?;
            drop(allocation);
            let inheritance = if directory { "OICI" } else { "" };
            let sddl: Vec<u16> = format!("O:{sid}D:P(A;{inheritance};FA;;;{sid})")
                .encode_utf16()
                .chain(Some(0))
                .collect();
            let mut descriptor = null_mut();
            if ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                1,
                &mut descriptor,
                null_mut(),
            ) == 0
            {
                return Err("cannot construct owner-only Windows security descriptor".into());
            }
            Ok(Local(descriptor))
        }
    }
}
fn wide(path: &Path) -> Result<Vec<u16>, String> {
    let mut chars: Vec<u16> = path.as_os_str().encode_wide().collect();
    if chars.contains(&0) {
        return Err("invalid secret path".into());
    }
    chars.push(0);
    Ok(chars)
}
pub(super) fn validate(file: &File) -> Result<(), String> {
    unsafe {
        if file
            .metadata()
            .map_err(|_| "cannot inspect secret file")?
            .file_attributes()
            & FILE_ATTRIBUTE_REPARSE_POINT
            != 0
        {
            return Err("secret paths cannot be Windows reparse points".into());
        }
        let user = User::new()?;
        let mut owner = null_mut();
        let mut dacl = null_mut();
        let mut descriptor = null_mut();
        if GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        ) != 0
        {
            return Err("cannot inspect secret DACL".into());
        }
        let allocation = Local(descriptor);
        if owner.is_null()
            || EqualSid(owner, user.sid()) == 0
            || dacl.is_null()
            || (*dacl).AceCount != 1
        {
            return Err("secret must have a current-user-only Windows DACL".into());
        }
        let mut control = 0;
        let mut revision = 0;
        if GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) == 0
            || control & SE_DACL_PROTECTED as u16 == 0
        {
            return Err("secret DACL must disable inherited permissions".into());
        }
        let mut ace = null_mut();
        if GetAce(dacl, 0, &mut ace) == 0 || ace.is_null() {
            return Err("cannot read secret access rule".into());
        }
        let ace = &*ace.cast::<ACCESS_ALLOWED_ACE>();
        // ACCESS_ALLOWED_ACE_TYPE is the canonical allow ACE (0); no conditional/object grants.
        if ace.Header.AceType != 0
            || ace.Header.AceFlags & 0x08 != 0
            || EqualSid((&ace.SidStart as *const u32).cast_mut().cast(), user.sid()) == 0
        {
            return Err("secret access rule grants another principal".into());
        }
        drop(allocation);
        Ok(())
    }
}
pub(super) fn create(path: &Path) -> Result<File, String> {
    unsafe {
        let path = wide(path)?;
        let descriptor = User::new()?.descriptor(false)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        let handle = CreateFileW(
            path.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            &attributes,
            CREATE_NEW,
            FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        );
        if handle == INVALID_HANDLE_VALUE {
            return Err("cannot create protected secret file (must not exist)".into());
        }
        let file = File::from_raw_handle(handle);
        validate(&file)?;
        Ok(file)
    }
}
pub(super) fn directory(path: &Path) -> Result<(), String> {
    if !path.exists() {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent).map_err(|_| "cannot create secret directory parent")?;
        }
        let name = wide(path)?;
        let descriptor = User::new()?.descriptor(true)?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            bInheritHandle: 0,
        };
        if unsafe { CreateDirectoryW(name.as_ptr(), &attributes) } == 0 && !path.exists() {
            return Err("cannot create protected secret directory".into());
        }
    }
    let name = wide(path)?;
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            READ_CONTROL,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err("cannot open protected secret directory".into());
    }
    let file = unsafe { File::from_raw_handle(handle) };
    if !file
        .metadata()
        .map_err(|_| "cannot inspect secret directory")?
        .is_dir()
    {
        return Err("secret directory is not a directory".into());
    }
    validate(&file)
}
