//! Windows host primitives shared with the protected credential/state owner.
//! This is native implementation plumbing, not a public transport or SDK.
use std::ffi::c_void;
use std::fs::{self, File, OpenOptions};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::Path;
use std::ptr::{addr_of, null_mut};
use windows_sys::Win32::Foundation::{
    CloseHandle, GENERIC_ALL, GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo,
    SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    ACCESS_ALLOWED_ACE, ACL, DACL_SECURITY_INFORMATION, EqualSid, GetAce, GetLengthSid, IsValidSid,
    OWNER_SECURITY_INFORMATION, PSID, SECURITY_ATTRIBUTES,
};
use windows_sys::Win32::Storage::FileSystem::{
    CREATE_NEW, CreateDirectoryW, CreateFileW, FILE_ALL_ACCESS, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ, FILE_SHARE_WRITE,
};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

/// Complete binary SID, zero padded; never a truncated hash or login name.
pub type Account = [u8; 68];
fn error() -> String {
    "Windows protected host state or account could not be verified".into()
}
fn copy_sid(sid: PSID) -> Result<Account, String> {
    let length = unsafe { GetLengthSid(sid) } as usize;
    if length == 0 || length > 68 {
        return Err(error());
    }
    let mut account = [0u8; 68];
    account[..length].copy_from_slice(unsafe { std::slice::from_raw_parts(sid.cast(), length) });
    Ok(account)
}
/// Resolve this account's native known folder without accepting an environment
/// substitute, creating identity/state, or reading credential contents.
pub fn local_application_data() -> Result<std::path::PathBuf, String> {
    let root = crate::state_windows::local_app_data().map_err(|_| error())?;
    crate::state_windows::validate_directory(&root).map_err(|_| error())?;
    Ok(root)
}
pub fn current_account() -> Result<Account, String> {
    crate::state_windows::with_current_user_sid(copy_sid).map_err(|_| error())?
}
/// Query a held process handle, rather than trusting a caller's account claim.
pub fn process_account(pid: u32) -> Result<Account, String> {
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(error());
    }
    let result =
        crate::state_windows::with_process_user_sid(process, copy_sid).map_err(|_| error());
    unsafe { CloseHandle(process) };
    result?
}

/// A descriptor exists only for the synchronous creation call. Nothing is
/// inherited by child processes; the sole allow ACE names the current SID.
pub fn with_owner_security<T>(
    create: impl FnOnce(*mut c_void) -> Result<T, String>,
) -> Result<T, String> {
    let sddl = crate::state_windows::with_current_user_sid(|sid| {
        let mut text = null_mut();
        if unsafe { ConvertSidToStringSidW(sid, &mut text) } == 0 {
            return Err(error());
        }
        let mut length = 0;
        while unsafe { *text.add(length) } != 0 {
            length += 1;
            if length > 256 {
                unsafe { LocalFree(text.cast()) };
                return Err(error());
            }
        }
        let sid = String::from_utf16(unsafe { std::slice::from_raw_parts(text, length) });
        unsafe { LocalFree(text.cast()) };
        let sid = sid.map_err(|_| error())?;
        Ok(format!("O:{sid}D:P(A;OICI;GA;;;{sid})"))
    })
    .map_err(|_| error())??;
    let wide: Vec<u16> = sddl.encode_utf16().chain(Some(0)).collect();
    let mut descriptor = null_mut();
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide.as_ptr(),
            1,
            &mut descriptor,
            null_mut(),
        )
    } == 0
    {
        return Err(error());
    }
    let mut attributes = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: descriptor,
        bInheritHandle: 0,
    };
    let result = create((&mut attributes as *mut SECURITY_ATTRIBUTES).cast());
    unsafe { LocalFree(descriptor) };
    result
}

fn no_reparse_ancestors(path: &Path) -> Result<(), String> {
    crate::state_windows::validate_absolute_local_path(path).map_err(|_| error())?;
    for parent in path.ancestors() {
        match fs::symlink_metadata(parent) {
            Ok(meta) if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 => {
                return Err(error());
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(error()),
        }
    }
    Ok(())
}
fn validate_private(file: &File) -> Result<(), String> {
    let mut owner: PSID = null_mut();
    let mut acl: *mut ACL = null_mut();
    let mut descriptor = null_mut();
    if unsafe {
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
    } != 0
    {
        return Err(error());
    }
    let checked = (|| {
        if owner.is_null()
            || acl.is_null()
            || descriptor.is_null()
            || unsafe { (*acl).AceCount } != 1
        {
            return Err(error());
        }
        let mut ace: *mut c_void = null_mut();
        if unsafe { GetAce(acl, 0, &mut ace) } == 0 || ace.is_null() {
            return Err(error());
        }
        let allow = ace.cast::<ACCESS_ALLOWED_ACE>();
        // ACCESS_ALLOWED_ACE_TYPE is zero. Bounds come from the OS ACL getter.
        if (unsafe { (*allow).Header.AceSize } as usize)
            < std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart) + 8
            || unsafe { (*allow).Header.AceType } != 0
            || (unsafe { (*allow).Mask } & GENERIC_ALL == 0
                && unsafe { (*allow).Mask } & FILE_ALL_ACCESS != FILE_ALL_ACCESS)
        {
            return Err(error());
        }
        let allowed: PSID = unsafe { addr_of!((*allow).SidStart).cast_mut().cast() };
        if unsafe { IsValidSid(allowed) } == 0
            || unsafe { GetLengthSid(allowed) } as usize
                + std::mem::offset_of!(ACCESS_ALLOWED_ACE, SidStart)
                > unsafe { (*allow).Header.AceSize } as usize
        {
            return Err(error());
        }
        let same = crate::state_windows::with_current_user_sid(|sid| unsafe {
            EqualSid(owner, sid) != 0 && EqualSid(allowed, sid) != 0
        })
        .map_err(|_| error())?;
        if same { Ok(()) } else { Err(error()) }
    })();
    if !descriptor.is_null() {
        unsafe { LocalFree(descriptor) };
    }
    checked
}

pub fn validate_directory(path: &Path) -> Result<(), String> {
    no_reparse_ancestors(path)?;
    crate::state_windows::validate_directory(path).map_err(|_| error())?;
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| error())?;
    validate_private(&file)
}
/// Create missing descendants privately; existing ancestors are validated by
/// the existing state adapter and are never rewritten or granted new access.
pub fn prepare_directory(path: &Path) -> Result<(), String> {
    no_reparse_ancestors(path)?;
    if path.exists() {
        return validate_directory(path);
    }
    let parent = path.parent().ok_or_else(error)?;
    if !parent.exists() {
        prepare_directory(parent)?;
    }
    crate::state_windows::validate_directory(parent).map_err(|_| error())?;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    with_owner_security(|attributes| {
        if unsafe { CreateDirectoryW(wide.as_ptr(), attributes.cast()) } == 0 && !path.exists() {
            return Err(error());
        }
        Ok(())
    })?;
    validate_directory(path)
}
pub fn open_lock(path: &Path) -> Result<File, String> {
    validate_directory(path.parent().ok_or_else(error)?)?;
    let file = crate::state_windows::open_lock(path).map_err(|_| error())?;
    validate_private(&file)?;
    Ok(file)
}
pub fn read_file(path: &Path) -> Result<File, String> {
    validate_directory(path.parent().ok_or_else(error)?)?;
    let file = crate::state_windows::open_read(path).map_err(|_| error())?;
    validate_private(&file)?;
    Ok(file)
}
/// Admit an existing private retained store while its SQLite owner is live.
/// Sharing permits the owner's writer; this handle itself grants only reads.
pub fn read_live_store_file(path: &Path) -> Result<File, String> {
    validate_directory(path.parent().ok_or_else(error)?)?;
    let file = crate::state_windows::open_lock_metadata(path).map_err(|_| error())?;
    validate_private(&file)?;
    Ok(file)
}
pub fn create_file(path: &Path) -> Result<File, String> {
    validate_directory(path.parent().ok_or_else(error)?)?;
    let file = crate::state_windows::create_new(path).map_err(|_| error())?;
    validate_private(&file)?;
    Ok(file)
}
/// Create a new owner-only output in an ordinary safe per-user directory.
/// The parent is validated without changing its ACL; the protected descriptor
/// applies at atomic creation, before a caller can write any plaintext.
pub fn create_private_output_file(path: &Path) -> Result<File, String> {
    no_reparse_ancestors(path)?;
    crate::state_windows::validate_directory(path.parent().ok_or_else(error)?)
        .map_err(|_| error())?;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let file = with_owner_security(|attributes| {
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                0,
                attributes.cast(),
                CREATE_NEW,
                FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        if handle == INVALID_HANDLE_VALUE {
            return Err(error());
        }
        // The successful native handle is now owned and closed by File.
        Ok(unsafe { File::from_raw_handle(handle.cast()) })
    })?;
    crate::state_windows::validate_file(&file).map_err(|_| error())?;
    validate_private(&file)?;
    Ok(file)
}
pub fn replace_file(staging: &Path, destination: &Path) -> Result<(), String> {
    if staging.parent() != destination.parent() {
        return Err(error());
    }
    drop(read_file(staging)?);
    if destination.exists() {
        drop(read_file(destination)?);
    }
    crate::state_windows::replace(staging, destination).map_err(|_| error())?;
    drop(read_file(destination)?);
    Ok(())
}

/// Retain a private file without replacing an existing destination. The native
/// move is atomic and write-through; a racing destination makes it refuse.
pub fn move_file_no_replace(source: &Path, destination: &Path) -> Result<(), String> {
    if source.parent() != destination.parent() {
        return Err(error());
    }
    drop(read_file(source)?);
    crate::state_windows::move_no_replace(source, destination).map_err(|_| error())?;
    drop(read_file(destination)?);
    Ok(())
}

/// The validated single-owner handle stays live through audit and serving.
/// Its private path is the only file the audit may skip reopening: an active
/// read/write lock deliberately permits contenders to try the same OS lock.
pub struct RuntimeLock {
    _file: File,
    path: std::path::PathBuf,
}
impl RuntimeLock {
    pub fn acquire(path: &Path) -> Result<Self, String> {
        let file = open_lock(path)?;
        file.try_lock().map_err(|_| {
            "a Core already owns this state directory, or its lock cannot be verified".to_string()
        })?;
        Ok(Self {
            _file: file,
            path: path.to_path_buf(),
        })
    }
}
/// Windows traversal privileges mean a private parent does not hide a known
/// child filename with an old permissive DACL. Audit retained contents once,
/// under the owner lock, before opening the queue or publishing the pipe.
/// Nothing is rewritten or deleted. New children inherit the private root ACL.
pub fn audit_runtime_tree(root: &Path, lock: &RuntimeLock) -> Result<(), String> {
    if lock.path.parent() != Some(root) {
        return Err(error());
    }
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut count = 0usize;
    let mut path_bytes = 0usize;
    while let Some((directory, depth)) = pending.pop() {
        validate_directory(&directory)?;
        for entry in fs::read_dir(&directory).map_err(|_| error())? {
            let path = entry.map_err(|_| error())?.path();
            count += 1;
            path_bytes =
                path_bytes.saturating_add(path.as_os_str().encode_wide().count().saturating_mul(2));
            if count > 100_000 || depth >= 64 || path_bytes > 16 * 1024 * 1024 {
                return Err("Windows Core state ownership audit exceeds its bounded inventory; retained state was preserved".into());
            }
            if path == lock.path {
                continue;
            }
            let meta = fs::symlink_metadata(&path).map_err(|_| error())?;
            if meta.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                return Err(error());
            }
            if meta.is_dir() {
                pending.push((path, depth + 1));
            } else if meta.is_file() {
                drop(read_file(&path)?);
            } else {
                return Err(error());
            }
        }
    }
    Ok(())
}

/// Bind the existing account/SID and sole-owner DACL checks to opaque UI IO.
/// Registration creates no credentials, identity, queue, or filesystem state.
pub fn register_ui_store_owner() -> Result<(), String> {
    use ds_update_runtime::modules::ui::{WindowsUiStoreOwner, register_windows_store_owner};
    fn validate_file(path: &Path) -> Result<(), String> {
        if path.file_name().is_some_and(|name| name == "update.lock") {
            validate_directory(path.parent().ok_or_else(error)?)?;
            let file = crate::state_windows::open_lock_metadata(path).map_err(|_| error())?;
            // Same SID, DACL, hardlink and reparse checks, without excluding
            // the updater's own held writer before its generation fence.
            return validate_private(&file);
        }
        drop(read_file(path)?);
        Ok(())
    }
    static OWNER: WindowsUiStoreOwner = WindowsUiStoreOwner {
        prepare_directory,
        validate_directory,
        validate_file,
    };
    register_windows_store_owner(&OWNER)
}

#[cfg(test)]
mod ui_store_tests {
    use super::*;
    use ds_command_kernel::module_install::ActivationGuard;
    use ds_command_kernel::module_release::ui::UiComponent;
    use ds_update_runtime::modules::ui::{MAX_MEMBER_CHUNK, UiModuleStore};

    struct FixtureDirectory(std::path::PathBuf);
    impl FixtureDirectory {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for FixtureDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn private_fixture() -> (FixtureDirectory, UiModuleStore) {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        register_ui_store_owner().unwrap();
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // The managed sandbox grants foreign SIDs writes to TEMP; the native
        // owner correctly refuses it. Use its safe current-user known folder.
        let base = FixtureDirectory(local_application_data().unwrap().join(format!(
            "ds-ui-native-{}-{nonce}-{sequence}",
            std::process::id()
        )));
        prepare_directory(base.path()).unwrap();
        let store = UiModuleStore::new(base.path(), UiComponent::Interactions, "canary").unwrap();
        (base, store)
    }
    fn grant_read(path: &Path) {
        let result = std::process::Command::new("icacls.exe")
            .arg(path)
            .args(["/grant", "*S-1-1-0:(R)"])
            .output()
            .unwrap();
        assert!(result.status.success(), "native ACL fixture failed");
    }
    #[test]
    fn windows_ui_store_reuses_sid_owner_and_refuses_permissive_retained_files() {
        let (base, store) = private_fixture();
        let root = base.path().join("interactions/canary");
        validate_directory(&root).unwrap();
        assert!(store.selection().unwrap().is_none());
        let file = root.join("foreign-readable.json");
        fs::write(&file, b"opaque fixture").unwrap();
        drop(read_file(&file).unwrap());
        grant_read(&file);
        assert!(
            store.selection().is_err(),
            "known filenames must not bypass a permissive child DACL"
        );
        assert_eq!(fs::read(&file).unwrap(), b"opaque fixture");
    }
    #[test]
    fn windows_ui_store_refuses_permissive_directories_without_repair() {
        let (base, store) = private_fixture();
        let component = base.path().join("interactions");
        grant_read(&component);
        assert!(store.selection().is_err());
        assert!(
            validate_directory(&component).is_err(),
            "refusal must preserve the existing ACL"
        );
    }
    #[test]
    fn windows_ui_store_single_owner_lock_is_native() {
        let (base, store) = private_fixture();
        let path = base.path().join("interactions/canary/update.lock");
        let held = open_lock(&path).unwrap();
        held.try_lock().unwrap();
        assert!(
            store
                .capture(&|| false)
                .unwrap_err()
                .contains("module_update_busy")
        );
        drop(held);
        assert!(
            store
                .capture(&|| false)
                .unwrap_err()
                .contains("module_not_installed")
        );
    }
    #[test]
    #[ignore = "requires the packaged Canary profile and explicit public UI unit digest"]
    fn windows_published_ui_install_remove_rollback_and_reopen() {
        let digest = std::env::var("DS_UI_QUALIFICATION_UNIT_SHA256")
            .expect("name the exact public UI unit");
        let (base, store) = private_fixture();
        let authority =
            crate::module_updates::capture_release_authority(crate::Lane::Canary).unwrap();
        let grant = authority
            .acquire_ui(UiComponent::Interactions, &digest, &|| false)
            .unwrap();
        let staged = store.stage(grant, |_| {}, &|| false).unwrap();
        assert_eq!(staged.expected_generation, 0);
        let selection = store
            .activate(&staged.receipt_id, 0, ActivationGuard::default(), &|| false)
            .unwrap();
        assert_eq!(selection.generation, 1);
        let captured = store.capture(&|| false).unwrap();
        for (name, _) in UiComponent::Interactions.files() {
            let (bytes, verified) = store
                .member(&digest, name, 0, MAX_MEMBER_CHUNK, &|| false)
                .unwrap();
            assert_eq!(bytes.len() as i64, verified.size_bytes);
        }
        store
            .remove(1, ActivationGuard::default(), &|| false)
            .unwrap();
        assert!(
            store
                .capture(&|| false)
                .unwrap_err()
                .contains("module_removed")
        );
        store
            .rollback(2, ActivationGuard::default(), &|| false)
            .unwrap();
        let reopened =
            UiModuleStore::new(base.path(), UiComponent::Interactions, "canary").unwrap();
        assert_eq!(
            reopened.capture(&|| false).unwrap().receipt,
            captured.receipt
        );
        assert_eq!(reopened.selection().unwrap().unwrap().generation, 3);
        let member = base
            .path()
            .join("interactions/canary/versions")
            .join(&digest)
            .join("ds_interactions_wasm_bg.wasm");
        let original = fs::read(&member).unwrap();
        fs::write(&member, b"tampered").unwrap();
        assert!(reopened.capture(&|| false).is_err());
        fs::write(&member, original).unwrap();
        reopened.capture(&|| false).unwrap();
    }
}

#[cfg(test)]
mod private_output_tests {
    use super::*;
    use std::io::Write;

    struct FixtureDirectory(std::path::PathBuf);
    impl FixtureDirectory {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    impl Drop for FixtureDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn ordinary_fixture() -> FixtureDirectory {
        static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let sequence = SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // Managed TEMP can grant foreign SIDs writes. Create an ordinary
        // inherited directory in the safe current-user known folder instead.
        let directory = FixtureDirectory(local_application_data().unwrap().join(format!(
            "ds-private-output-test-{}-{nonce}-{sequence}",
            std::process::id()
        )));
        fs::create_dir(directory.path()).unwrap();
        grant(directory.path(), "*S-1-1-0:(R)");
        crate::state_windows::validate_directory(directory.path()).unwrap();
        assert!(validate_directory(directory.path()).is_err());
        directory
    }
    fn grant(path: &Path, access: &str) {
        let result = std::process::Command::new("icacls.exe")
            .arg(path)
            .args(["/grant", access])
            .output()
            .unwrap();
        assert!(result.status.success(), "native ACL fixture failed");
    }
    fn acl(path: &Path) -> Vec<u8> {
        let result = std::process::Command::new("icacls.exe")
            .arg(path)
            .output()
            .unwrap();
        assert!(result.status.success(), "native ACL inspection failed");
        result.stdout
    }
    #[test]
    fn windows_private_output_is_owner_only_without_changing_ordinary_parent() {
        let directory = ordinary_fixture();
        let before = acl(directory.path());
        let path = directory.path().join("output.json");
        let mut file = create_private_output_file(&path).unwrap();
        crate::state_windows::validate_file(&file).unwrap();
        validate_private(&file).unwrap();
        file.write_all(b"private output").unwrap();
        drop(file);
        assert_eq!(fs::read(&path).unwrap(), b"private output");
        assert_eq!(acl(directory.path()), before);
        assert!(validate_directory(directory.path()).is_err());
    }
    #[test]
    fn windows_private_output_preserves_existing_file() {
        let directory = ordinary_fixture();
        let before = acl(directory.path());
        let path = directory.path().join("occupied.json");
        fs::write(&path, b"retained original bytes").unwrap();
        let file_acl = acl(&path);
        assert!(create_private_output_file(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), b"retained original bytes");
        assert_eq!(acl(&path), file_acl);
        assert_eq!(acl(directory.path()), before);
    }
    #[test]
    fn windows_private_output_refuses_foreign_writable_parent_before_creation() {
        let directory = ordinary_fixture();
        grant(directory.path(), "*S-1-1-0:(W)");
        let before = acl(directory.path());
        assert!(crate::state_windows::validate_directory(directory.path()).is_err());
        let path = directory.path().join("must-not-exist.json");
        assert!(create_private_output_file(&path).is_err());
        assert!(!path.exists());
        assert_eq!(acl(directory.path()), before);
    }
}
