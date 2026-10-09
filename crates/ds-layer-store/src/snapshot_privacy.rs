//! Strict privacy boundary for project snapshot storage.
//!
//! Existing entries are checked, never repaired. Ancestors must be real
//! directories protected against substitution by untrusted users. Checks use
//! native handles; callers must still create files exclusively, avoid following
//! links when opening them, and retain their own storage lock across IO. A
//! successful path check is not a capability for a later, independent open.

use std::path::Path;

/// Verify or create a private directory owned by the current OS user.
pub fn directory(path: &Path) -> Result<(), String> {
    native::directory(path)
}

/// Verify an existing private, singly linked regular file owned by this user.
pub fn file(path: &Path) -> Result<(), String> {
    native::file(path)
}

#[cfg(unix)]
mod native {
    use std::ffi::{CString, OsStr};
    use std::fs::File;
    use std::io;
    use std::os::fd::{AsRawFd, FromRawFd};
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::{Component, Path};

    fn error() -> String {
        format!("snapshot privacy: {}", io::Error::last_os_error())
    }

    fn name(value: &OsStr) -> Result<CString, String> {
        CString::new(value.as_bytes()).map_err(|_| "snapshot privacy: NUL in path".into())
    }

    fn components(path: &Path) -> Result<Vec<CString>, String> {
        if !path.is_absolute() {
            return Err("snapshot privacy: absolute path required".into());
        }
        // components() normalizes '.', so reject it before normalization too.
        if path
            .as_os_str()
            .as_bytes()
            .split(|b| *b == b'/')
            .any(|c| c == b"." || c == b"..")
        {
            return Err("snapshot privacy: non-normal path".into());
        }
        let parts = path
            .components()
            .filter_map(|c| match c {
                Component::RootDir => None,
                Component::Normal(n) => Some(name(n)),
                _ => Some(Err("snapshot privacy: non-normal path".into())),
            })
            .collect::<Result<Vec<_>, _>>()?;
        if parts.is_empty() {
            return Err("snapshot privacy: filesystem root is not DS-owned".into());
        }
        Ok(parts)
    }

    fn root() -> Result<File, String> {
        let fd = unsafe {
            libc::open(
                c"/".as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(error());
        }
        let root = unsafe { File::from_raw_fd(fd) };
        ancestor(&root)?;
        Ok(root)
    }

    fn ancestor(file: &File) -> Result<(), String> {
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        let uid = unsafe { libc::geteuid() };
        if !metadata.is_dir()
            || (metadata.uid() != uid && metadata.uid() != 0)
            || metadata.mode() & 0o022 != 0
        {
            return Err("snapshot privacy: unsafe directory ancestor".into());
        }
        Ok(())
    }

    fn private(file: &File, is_directory: bool) -> Result<(), String> {
        let metadata = file.metadata().map_err(|e| e.to_string())?;
        let expected = if is_directory { 0o700 } else { 0o600 };
        if metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o7777 != expected
            || (is_directory && !metadata.is_dir())
            || (!is_directory && (!metadata.is_file() || metadata.nlink() != 1))
        {
            return Err("snapshot privacy: entry is not private and owned by current user".into());
        }
        Ok(())
    }

    fn child(parent: &File, name: &CString, is_directory: bool) -> Result<File, String> {
        // NONBLOCK prevents a substituted FIFO/device from blocking verification.
        let flags = libc::O_RDONLY
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if is_directory { libc::O_DIRECTORY } else { 0 };
        let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            Err(error())
        } else {
            Ok(unsafe { File::from_raw_fd(fd) })
        }
    }

    fn walk(path: &Path, create_directory: bool) -> Result<(), String> {
        let parts = components(path)?;
        let mut parent = root()?;
        for (index, part) in parts.iter().enumerate() {
            let last = index + 1 == parts.len();
            let is_directory = !last || create_directory;
            let mut created = false;
            // fstatat distinguishes absence from every other open failure.
            if create_directory {
                let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
                let exists = unsafe {
                    libc::fstatat(
                        parent.as_raw_fd(),
                        part.as_ptr(),
                        stat.as_mut_ptr(),
                        libc::AT_SYMLINK_NOFOLLOW,
                    )
                };
                if exists != 0 {
                    if io::Error::last_os_error().raw_os_error() != Some(libc::ENOENT) {
                        return Err(error());
                    }
                    if unsafe { libc::mkdirat(parent.as_raw_fd(), part.as_ptr(), 0o700) } != 0 {
                        // A racing creator must pass all the same checks.
                        if io::Error::last_os_error().raw_os_error() != Some(libc::EEXIST) {
                            return Err(error());
                        }
                    } else {
                        created = true;
                    }
                }
            }
            let opened = child(&parent, part, is_directory)?;
            if created {
                // Creation is already restrictive even under an unusual umask.
                // fchmod is handle based and its failure is never ignored.
                let metadata = opened.metadata().map_err(|e| e.to_string())?;
                if metadata.uid() != unsafe { libc::geteuid() } || !metadata.is_dir() {
                    return Err("snapshot privacy: created directory owner changed".into());
                }
                if unsafe { libc::fchmod(opened.as_raw_fd(), 0o700) } != 0 {
                    return Err(error());
                }
                private(&opened, true)?;
            }
            if last {
                private(&opened, is_directory)?;
            } else {
                ancestor(&opened)?;
            }
            parent = opened;
        }
        Ok(())
    }

    pub(super) fn directory(path: &Path) -> Result<(), String> {
        walk(path, true)
    }
    pub(super) fn file(path: &Path) -> Result<(), String> {
        walk(path, false)
    }
}

#[cfg(windows)]
mod native {
    use std::fs::{File, OpenOptions};
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::path::{Component, Path, PathBuf, Prefix};
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{
        CloseHandle, LocalFree, ERROR_ALREADY_EXISTS, GENERIC_ALL, HANDLE,
    };
    use windows_sys::Win32::Security::Authorization::{GetSecurityInfo, SE_FILE_OBJECT};
    use windows_sys::Win32::Security::*;
    use windows_sys::Win32::Storage::FileSystem::*;
    use windows_sys::Win32::System::SystemServices::{
        ACCESS_ALLOWED_ACE_TYPE, ACCESS_DENIED_ACE_TYPE, FILE_PERSISTENT_ACLS,
        SECURITY_DESCRIPTOR_REVISION,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    fn error() -> String {
        format!("snapshot privacy: {}", io::Error::last_os_error())
    }

    fn paths(path: &Path) -> Result<Vec<PathBuf>, String> {
        if !path.is_absolute() {
            return Err("snapshot privacy: absolute path required".into());
        }
        let mut parts = path.components();
        let prefix = match parts.next() {
            Some(Component::Prefix(p)) if matches!(p.kind(), Prefix::Disk(_)) => p.as_os_str(),
            _ => return Err("snapshot privacy: local drive path required".into()),
        };
        if !matches!(parts.next(), Some(Component::RootDir)) {
            return Err("snapshot privacy: drive root required".into());
        }
        // Reject ADS, normalization aliases and Win32 device names.
        let raw = path.as_os_str().encode_wide().collect::<Vec<_>>();
        if raw.contains(&0)
            || raw
                .split(|c| *c == 92 || *c == 47)
                .skip(1)
                .any(|c| c == [46] || c == [46, 46])
        {
            return Err("snapshot privacy: non-normal path".into());
        }
        let mut current = PathBuf::from(prefix);
        current.push(Path::new("\\"));
        let mut result = vec![current.clone()];
        for part in parts {
            let Component::Normal(value) = part else {
                return Err("snapshot privacy: non-normal path".into());
            };
            let units = value.encode_wide().collect::<Vec<_>>();
            let stem = value
                .to_string_lossy()
                .split('.')
                .next()
                .unwrap_or("")
                .to_ascii_uppercase();
            if units
                .iter()
                .any(|u| *u < 32 || [58, 42, 63, 34, 60, 62, 124].contains(u))
                || matches!(units.last(), Some(32 | 46))
                || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.len() == 4
                    && matches!(stem.as_bytes()[3], b'1'..=b'9'))
            {
                return Err("snapshot privacy: unsafe Windows path component".into());
            }
            current.push(value);
            result.push(current.clone());
        }
        if result.len() == 1 {
            return Err("snapshot privacy: drive root is not DS-owned".into());
        }
        Ok(result)
    }

    fn with_sid<T>(f: impl FnOnce(PSID) -> Result<T, String>) -> Result<T, String> {
        let mut token: HANDLE = null_mut();
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(error());
        }
        let mut needed = 0;
        unsafe {
            GetTokenInformation(token, TokenUser, null_mut(), 0, &mut needed);
        }
        if needed < std::mem::size_of::<TOKEN_USER>() as u32 || needed > 65536 {
            unsafe {
                CloseHandle(token);
            }
            return Err("snapshot privacy: invalid token information size".into());
        }
        let mut words = vec![0usize; (needed as usize).div_ceil(std::mem::size_of::<usize>())];
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                words.as_mut_ptr().cast(),
                needed,
                &mut needed,
            )
        };
        unsafe {
            CloseHandle(token);
        }
        if ok == 0 {
            return Err(error());
        }
        let sid = unsafe { (*(words.as_ptr().cast::<TOKEN_USER>())).User.Sid };
        if sid.is_null() || unsafe { IsValidSid(sid) } == 0 {
            return Err("snapshot privacy: invalid current user SID".into());
        }
        f(sid)
    }

    fn trusted_sid(sid: PSID, kind: WELL_KNOWN_SID_TYPE) -> Result<bool, String> {
        let mut storage = [0usize; 16];
        let mut length = std::mem::size_of_val(&storage) as u32;
        if unsafe { CreateWellKnownSid(kind, null_mut(), storage.as_mut_ptr().cast(), &mut length) }
            == 0
        {
            return Err(error());
        }
        Ok(unsafe { EqualSid(sid, storage.as_mut_ptr().cast()) } != 0)
    }

    fn trusted_installer_owner(owner: PSID) -> Result<bool, String> {
        // Resolve one fully qualified local service identity. Neither arbitrary
        // service identities nor CREATOR OWNER trustees acquire this exception.
        // A null system name starts resolution locally; verify the returned
        // authority as well so domain fallback cannot substitute an account.
        let account = "NT SERVICE\\TrustedInstaller\0"
            .encode_utf16()
            .collect::<Vec<_>>();
        let mut storage = [0usize; 16];
        let mut sid_length = std::mem::size_of_val(&storage) as u32;
        let mut domain = [0u16; 256];
        let mut domain_length = domain.len() as u32;
        let mut kind = SidTypeUnknown;
        if unsafe {
            LookupAccountNameW(
                std::ptr::null(),
                account.as_ptr(),
                storage.as_mut_ptr().cast(),
                &mut sid_length,
                domain.as_mut_ptr(),
                &mut domain_length,
                &mut kind,
            )
        } == 0
        {
            return Err(error());
        }
        let sid: PSID = storage.as_mut_ptr().cast();
        let domain_end = domain
            .iter()
            .position(|unit| *unit == 0)
            .ok_or_else(|| "snapshot privacy: invalid service account domain".to_string())?;
        if sid_length < 12
            || sid_length as usize > std::mem::size_of_val(&storage)
            || unsafe { IsValidSid(sid) } == 0
            || unsafe { GetLengthSid(sid) } != sid_length
            || !String::from_utf16(&domain[..domain_end])
                .map_err(|_| "snapshot privacy: invalid service account domain")?
                .eq_ignore_ascii_case("NT SERVICE")
            || unsafe { (*GetSidIdentifierAuthority(sid)).Value } != [0, 0, 0, 0, 0, 5]
            || unsafe { *GetSidSubAuthorityCount(sid) } == 0
            || unsafe { *GetSidSubAuthority(sid, 0) } != 80
        {
            return Err("snapshot privacy: invalid TrustedInstaller service identity".into());
        }
        Ok(unsafe { EqualSid(owner, sid) } != 0)
    }

    fn check_acl(file: &File, user: PSID, private: bool, protected: bool) -> Result<(), String> {
        let mut owner = null_mut();
        let mut acl: *mut ACL = null_mut();
        let mut descriptor = null_mut();
        let code = unsafe {
            GetSecurityInfo(
                file.as_raw_handle().cast(),
                SE_FILE_OBJECT,
                OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
                &mut owner,
                null_mut(),
                &mut acl,
                null_mut(),
                &mut descriptor,
            )
        };
        let checked = (|| {
            if code != 0
                || descriptor.is_null()
                || owner.is_null()
                || acl.is_null()
                || unsafe { IsValidAcl(acl) } == 0
            {
                return Err("snapshot privacy: security descriptor unavailable".into());
            }
            let mut control = 0;
            let mut revision = 0;
            if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0
            {
                return Err(error());
            }
            if private
                && (unsafe { EqualSid(owner, user) } == 0
                    || (protected && control & SE_DACL_PROTECTED == 0))
            {
                return Err("snapshot privacy: current owner and protected DACL required".into());
            }
            if !private
                && unsafe { EqualSid(owner, user) } == 0
                && !trusted_sid(owner, WinLocalSystemSid)?
                && !trusted_sid(owner, WinBuiltinAdministratorsSid)?
                && !trusted_installer_owner(owner)?
            {
                // Ownership itself can confer the right to change a DACL;
                // examining the allow entries alone would miss that authority.
                return Err("snapshot privacy: untrusted directory ancestor owner".into());
            }
            let count = unsafe { (*acl).AceCount };
            if private && count != 1 {
                return Err("snapshot privacy: owner-only DACL required".into());
            }
            for index in 0..count {
                let mut raw = null_mut();
                if unsafe { GetAce(acl, index as u32, &mut raw) } == 0 || raw.is_null() {
                    return Err(error());
                }
                let header = unsafe { &*(raw.cast::<ACE_HEADER>()) };
                if header.AceType as u32 == ACCESS_DENIED_ACE_TYPE && !private {
                    continue;
                }
                if header.AceType as u32 != ACCESS_ALLOWED_ACE_TYPE
                    || (header.AceSize as usize) < std::mem::size_of::<ACCESS_ALLOWED_ACE>()
                {
                    return Err("snapshot privacy: unsupported access control entry".into());
                }
                let ace = unsafe { &*(raw.cast::<ACCESS_ALLOWED_ACE>()) };
                let sid: PSID = std::ptr::addr_of!(ace.SidStart).cast_mut().cast();
                // SID's fixed header is eight bytes; validate its declared
                // length before asking native SID routines to inspect it.
                let available = header.AceSize as usize - 8;
                if available < 8
                    || 8 + 4 * unsafe { *sid.cast::<u8>().add(1) } as usize > available
                    || unsafe { IsValidSid(sid) } == 0
                {
                    return Err("snapshot privacy: invalid ACL SID".into());
                }
                if private {
                    let forbidden = INHERIT_ONLY_ACE | if protected { INHERITED_ACE } else { 0 };
                    if header.AceFlags as u32 & forbidden != 0
                        || unsafe { EqualSid(sid, user) } == 0
                        || ace.Mask & FILE_ALL_ACCESS != FILE_ALL_ACCESS
                    {
                        return Err("snapshot privacy: non-private ACL".into());
                    }
                } else if header.AceFlags as u32 & INHERIT_ONLY_ACE == 0 {
                    // Directory ADD_FILE/ADD_SUBDIRECTORY rights create new
                    // names; they do not replace a protected existing child.
                    // Reject grants that can delete names or change authority.
                    let writes = GENERIC_ALL | DELETE | WRITE_DAC | WRITE_OWNER | FILE_DELETE_CHILD;
                    if ace.Mask & writes != 0
                        && unsafe { EqualSid(sid, user) } == 0
                        // The ancestor owner was admitted above, including an
                        // exactly resolved TrustedInstaller service account.
                        && unsafe { EqualSid(sid, owner) } == 0
                        && !trusted_sid(sid, WinLocalSystemSid)?
                        && !trusted_sid(sid, WinBuiltinAdministratorsSid)?
                    {
                        return Err("snapshot privacy: writable directory ancestor".into());
                    }
                }
            }
            Ok(())
        })();
        if !descriptor.is_null() {
            unsafe {
                LocalFree(descriptor);
            }
        }
        checked
    }

    fn open(path: &Path, directory: bool) -> Result<File, String> {
        let file = OpenOptions::new()
            .access_mode(READ_CONTROL | FILE_READ_ATTRIBUTES)
            // Deny delete sharing: retain every ancestor handle during traversal.
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
            .map_err(|e| format!("snapshot privacy: {e}"))?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut info) } == 0 {
            return Err(error());
        }
        let mut filesystem_flags = 0;
        if unsafe { GetFileType(file.as_raw_handle().cast()) } != FILE_TYPE_DISK
            || unsafe {
                GetVolumeInformationByHandleW(
                    file.as_raw_handle().cast(),
                    null_mut(),
                    0,
                    null_mut(),
                    null_mut(),
                    &mut filesystem_flags,
                    null_mut(),
                    0,
                )
            } == 0
            || filesystem_flags & FILE_PERSISTENT_ACLS == 0
        {
            return Err("snapshot privacy: persistent native filesystem ACLs required".into());
        }
        if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || (info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0) != directory
            || (!directory && info.nNumberOfLinks != 1)
        {
            return Err("snapshot privacy: linked or non-regular entry".into());
        }
        Ok(file)
    }

    fn create(path: &Path, user: PSID) -> Result<(), String> {
        // Pass the protected descriptor at creation, with no public window.
        let mut storage = [0usize; 32];
        let acl = storage.as_mut_ptr().cast::<ACL>();
        let mut descriptor = SECURITY_DESCRIPTOR::default();
        let sd = (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast();
        if unsafe { InitializeAcl(acl, std::mem::size_of_val(&storage) as u32, ACL_REVISION) } == 0
            || unsafe {
                AddAccessAllowedAceEx(
                    acl,
                    ACL_REVISION,
                    OBJECT_INHERIT_ACE | CONTAINER_INHERIT_ACE,
                    FILE_ALL_ACCESS,
                    user,
                )
            } == 0
            || unsafe { InitializeSecurityDescriptor(sd, SECURITY_DESCRIPTOR_REVISION) } == 0
            || unsafe { SetSecurityDescriptorOwner(sd, user, 0) } == 0
            || unsafe { SetSecurityDescriptorDacl(sd, 1, acl, 0) } == 0
            || unsafe { SetSecurityDescriptorControl(sd, SE_DACL_PROTECTED, SE_DACL_PROTECTED) }
                == 0
        {
            return Err(error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };
        let wide = path
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        if unsafe { CreateDirectoryW(wide.as_ptr(), &attributes) } == 0
            && io::Error::last_os_error().raw_os_error() != Some(ERROR_ALREADY_EXISTS as i32)
        {
            return Err(error());
        }
        Ok(())
    }

    fn walk(path: &Path, directory: bool) -> Result<(), String> {
        let paths = paths(path)?;
        with_sid(|user| {
            let mut retained = Vec::new();
            for (index, part) in paths.iter().enumerate() {
                let last = index + 1 == paths.len();
                let is_directory = !last || directory;
                let mut created = false;
                if directory && index != 0 {
                    match std::fs::symlink_metadata(part) {
                        Ok(_) => {}
                        Err(e) if e.kind() == io::ErrorKind::NotFound => {
                            create(part, user)?;
                            created = true;
                        }
                        Err(e) => return Err(format!("snapshot privacy: {e}")),
                    }
                }
                let opened = open(part, is_directory)?;
                // Files may inherit the parent's owner-only ACE. Directory
                // roots and newly created ancestors must block inheritance.
                check_acl(
                    &opened,
                    user,
                    last || created,
                    is_directory && (last || created),
                )?;
                retained.push(opened);
            }
            Ok(())
        })
    }

    pub(super) fn directory(path: &Path) -> Result<(), String> {
        walk(path, true)
    }
    pub(super) fn file(path: &Path) -> Result<(), String> {
        walk(path, false)
    }
    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn refuses_remote_device_alias_and_stream_paths() {
            for value in [
                r"relative\snapshot",
                r"\\server\share\snapshot",
                r"\\?\C:\snapshot",
                r"C:\snapshot:stream",
                r"C:\private\..\snapshot",
                r"C:\private\.\snapshot",
                r"C:\private\snapshot.",
                r"C:\private\CON",
                r"C:\private\COM1.txt",
            ] {
                assert!(paths(Path::new(value)).is_err(), "{value}");
            }
            assert!(paths(Path::new(r"C:\private\snapshot.json")).is_ok());
        }
    }
}

#[cfg(not(any(unix, windows)))]
mod native {
    use std::path::Path;
    pub(super) fn directory(_: &Path) -> Result<(), String> {
        Err("snapshot privacy: unsupported OS".into())
    }
    pub(super) fn file(_: &Path) -> Result<(), String> {
        Err("snapshot privacy: unsupported OS".into())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::{symlink, OpenOptionsExt, PermissionsExt};

    fn fixture() -> tempfile::TempDir {
        // Deliberately avoid temp_dir(): the host's /tmp is RAM-backed and is
        // also an unsafe shared ancestor for this contract.
        let base = dirs::home_dir().expect("private disk-backed home for tests");
        tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .prefix("snapshot-privacy-")
            .tempdir_in(base)
            .unwrap()
    }

    #[test]
    fn creates_private_tree_and_verifies_regular_files() {
        let fixture = fixture();
        let root = fixture.path().join("a/b");
        directory(&root).unwrap();
        assert_eq!(
            fs::metadata(&root).unwrap().permissions().mode() & 0o7777,
            0o700
        );
        let path = root.join("snapshot");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        file(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        assert!(file(&path).is_err());
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o640
        );
    }

    #[test]
    fn refuses_links_public_directories_and_non_normal_paths() {
        let fixture = fixture();
        let root = fixture.path().join("private");
        directory(&root).unwrap();
        symlink(&root, fixture.path().join("alias")).unwrap();
        assert!(directory(&fixture.path().join("alias/child")).is_err());
        assert!(!root.join("child").exists());
        let path = root.join("file");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .unwrap();
        fs::hard_link(&path, root.join("hardlink")).unwrap();
        assert!(file(&path).is_err());
        symlink(&path, root.join("symlink")).unwrap();
        assert!(file(&root.join("symlink")).is_err());
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(directory(&root).is_err());
        fs::set_permissions(&root, fs::Permissions::from_mode(0o777)).unwrap();
        assert!(directory(&root.join("child")).is_err());
        assert!(!root.join("child").exists());
        assert!(directory(Path::new("relative")).is_err());
        assert!(directory(&fixture.path().join("private/../escape")).is_err());
        assert!(directory(&fixture.path().join("private/./escape")).is_err());
    }
}
