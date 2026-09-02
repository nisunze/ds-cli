//! Windows protected-state adapter.
//!
//! DPAPI deliberately uses its default current-user scope. The public
//! envelope contains only a version, a store-purpose tag, and a digest of the
//! already-opaque file key. That same domain separation is optional entropy,
//! so moving ciphertext between lanes, audiences, store kinds, or keys fails
//! closed before any state is parsed.

use std::ffi::{OsStr, c_void};
use std::fs::{self, File, OpenOptions};
use std::io;
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Component, Path, PathBuf, Prefix};
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use ds_client_core::StoreError;
use sha2::{Digest, Sha256};
use windows_sys::Win32::Foundation::{
    CloseHandle, ERROR_LOCK_VIOLATION, ERROR_SHARING_VIOLATION, GENERIC_ALL, GENERIC_READ,
    GENERIC_WRITE, HANDLE, LocalFree,
};
use windows_sys::Win32::Security::Authorization::{
    BuildTrusteeWithSidW, GetEffectiveRightsFromAclW, GetSecurityInfo, SE_FILE_OBJECT, TRUSTEE_W,
};
use windows_sys::Win32::Security::Cryptography::{
    CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData,
};
use windows_sys::Win32::Security::{
    ACL, CreateWellKnownSid, DACL_SECURITY_INFORMATION, EqualSid, GetTokenInformation,
    OWNER_SECURITY_INFORMATION, PSID, SECURITY_MAX_SID_SIZE, TOKEN_QUERY, TOKEN_USER, TokenUser,
    WELL_KNOWN_SID_TYPE, WinAuthenticatedUserSid, WinWorldSid,
};
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, DELETE, FILE_APPEND_DATA, FILE_ATTRIBUTE_DIRECTORY,
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_DISPOSITION_INFO, FILE_FLAG_BACKUP_SEMANTICS,
    FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    FILE_WRITE_ATTRIBUTES, FILE_WRITE_DATA, FILE_WRITE_EA, FileDispositionInfo,
    GetFileInformationByHandle, LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx,
    MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW, ReplaceFileW,
    SetFileInformationByHandle, UnlockFileEx, WRITE_DAC, WRITE_OWNER,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::IO::OVERLAPPED;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows_sys::Win32::UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath};
use zeroize::{Zeroize, Zeroizing};

const ENVELOPE_MAGIC: &[u8; 8] = b"DSSTATE\0";
const ENVELOPE_VERSION: u8 = 1;
const ENVELOPE_HEADER: usize = 8 + 1 + 1 + 2 + 32 + 4;
const ENTROPY_DOMAIN: &[u8] = b"ds-cli/native-state/dpapi/v1";

pub(super) fn state_root() -> Result<PathBuf, StoreError> {
    let base = local_app_data()?;
    validate_absolute_local_path(&base)?;
    validate_directory(&base)?;
    let product = base.join("Data Solutions");
    ensure_directory(&product)?;
    let root = product.join("ds");
    ensure_directory(&root)?;
    Ok(root)
}

pub(super) fn probe() -> Result<(), StoreError> {
    let base = local_app_data()?;
    validate_absolute_local_path(&base)?;
    validate_directory(&base)?;
    let product = base.join("Data Solutions");
    validate_optional_directory(&product)?;
    validate_optional_directory(&product.join("ds"))
}

pub(super) fn ensure_directory(path: &Path) -> Result<(), StoreError> {
    validate_absolute_local_path(path)?;
    match fs::create_dir(path) {
        Ok(()) => validate_directory(path),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => validate_directory(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Err(StoreError::UnsafeOrUnreadable)
        }
        Err(_) => Err(StoreError::Unavailable),
    }
}

pub(super) fn create_new(path: &Path) -> Result<File, StoreError> {
    validate_absolute_local_path(path)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| StoreError::Unavailable)?;
    validate_file(&file)?;
    Ok(file)
}

pub(super) fn open_read(path: &Path) -> Result<File, StoreError> {
    validate_absolute_local_path(path)?;
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| StoreError::Unavailable)?;
    validate_file(&file)?;
    Ok(file)
}

pub(super) fn open_lock(path: &Path) -> Result<File, StoreError> {
    validate_absolute_local_path(path)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| StoreError::Unavailable)?;
    validate_file(&file)?;
    Ok(file)
}

pub(super) fn validate_file(file: &File) -> Result<(), StoreError> {
    let info = handle_info(file)?;
    if info.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | FILE_ATTRIBUTE_DIRECTORY) != 0
        || info.nNumberOfLinks != 1
    {
        return Err(StoreError::UnsafeOrUnreadable);
    }
    validate_owner(file)
}

pub(super) fn lock_exclusive(
    file: &File,
    wait: Duration,
    poll: Duration,
) -> Result<(), StoreError> {
    let deadline = Instant::now() + wait;
    loop {
        let mut overlapped = OVERLAPPED::default();
        let result = unsafe {
            LockFileEx(
                handle(file),
                LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY,
                0,
                1,
                0,
                &mut overlapped,
            )
        };
        if result != 0 {
            return Ok(());
        }
        let code = io::Error::last_os_error()
            .raw_os_error()
            .unwrap_or_default() as u32;
        if code != ERROR_LOCK_VIOLATION && code != ERROR_SHARING_VIOLATION {
            return Err(StoreError::Unavailable);
        }
        if Instant::now() >= deadline {
            return Err(StoreError::Conflict);
        }
        std::thread::sleep(poll);
    }
}

pub(super) fn unlock(file: &File) -> Result<(), StoreError> {
    let mut overlapped = OVERLAPPED::default();
    if unsafe { UnlockFileEx(handle(file), 0, 1, 0, &mut overlapped) } == 0 {
        Err(StoreError::Unavailable)
    } else {
        Ok(())
    }
}

pub(super) fn protect(path: &Path, plaintext: &[u8]) -> Result<Zeroizing<Vec<u8>>, StoreError> {
    let identity = identity(path)?;
    let mut entropy = entropy(&identity);
    let mut input = blob(plaintext)?;
    let mut entropy_blob = blob(&entropy)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    let description = wide("DS CLI protected state");
    let success = unsafe {
        CryptProtectData(
            &mut input,
            description.as_ptr(),
            &mut entropy_blob,
            null(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    entropy.zeroize();
    if success == 0 || output.pbData.is_null() {
        zero_and_free_blob(&mut output);
        return Err(StoreError::Unavailable);
    }
    let ciphertext = unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize) };
    let Some(total) = ENVELOPE_HEADER.checked_add(ciphertext.len()) else {
        zero_and_free_blob(&mut output);
        return Err(StoreError::UnsafeOrUnreadable);
    };
    let Ok(ciphertext_length) = u32::try_from(ciphertext.len()) else {
        zero_and_free_blob(&mut output);
        return Err(StoreError::UnsafeOrUnreadable);
    };
    let mut envelope = Zeroizing::new(Vec::with_capacity(total));
    envelope.extend_from_slice(ENVELOPE_MAGIC);
    envelope.push(ENVELOPE_VERSION);
    envelope.push(identity.purpose);
    envelope.extend_from_slice(&[0, 0]);
    envelope.extend_from_slice(&identity.key_digest);
    envelope.extend_from_slice(&ciphertext_length.to_le_bytes());
    envelope.extend_from_slice(ciphertext);
    zero_and_free_blob(&mut output);
    Ok(envelope)
}

pub(super) fn unprotect(path: &Path, envelope: &[u8]) -> Result<Vec<u8>, StoreError> {
    let identity = identity(path)?;
    if envelope.len() < ENVELOPE_HEADER
        || &envelope[..8] != ENVELOPE_MAGIC
        || envelope[8] != ENVELOPE_VERSION
        || envelope[9] != identity.purpose
        || envelope[10..12] != [0, 0]
        || envelope[12..44] != identity.key_digest
    {
        return Err(StoreError::UnsafeOrUnreadable);
    }
    let length = u32::from_le_bytes(
        envelope[44..48]
            .try_into()
            .map_err(|_| StoreError::UnsafeOrUnreadable)?,
    ) as usize;
    if length != envelope.len() - ENVELOPE_HEADER {
        return Err(StoreError::UnsafeOrUnreadable);
    }
    let mut entropy = entropy(&identity);
    let mut input = blob(&envelope[ENVELOPE_HEADER..])?;
    let mut entropy_blob = blob(&entropy)?;
    let mut output = CRYPT_INTEGER_BLOB::default();
    let mut description = null_mut();
    let success = unsafe {
        CryptUnprotectData(
            &mut input,
            &mut description,
            &mut entropy_blob,
            null(),
            null(),
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
    };
    entropy.zeroize();
    if !description.is_null() {
        unsafe { LocalFree(description.cast()) };
    }
    if success == 0 || output.pbData.is_null() {
        zero_and_free_blob(&mut output);
        return Err(StoreError::UnsafeOrUnreadable);
    }
    let mut plaintext =
        unsafe { std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec() };
    zero_and_free_blob(&mut output);
    if plaintext.is_empty() {
        plaintext.shrink_to_fit();
    }
    Ok(plaintext)
}

pub(super) fn replace(temp: &Path, destination: &Path) -> Result<(), StoreError> {
    validate_absolute_local_path(temp)?;
    validate_absolute_local_path(destination)?;
    let temp_wide = wide_path(temp);
    let destination_wide = wide_path(destination);
    let destination_exists = destination.exists();
    let replaced = if destination_exists {
        drop(open_read(destination)?);
        unsafe {
            ReplaceFileW(
                destination_wide.as_ptr(),
                temp_wide.as_ptr(),
                null(),
                0,
                null(),
                null(),
            )
        }
    } else {
        unsafe {
            MoveFileExW(
                temp_wide.as_ptr(),
                destination_wide.as_ptr(),
                MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
            )
        }
    };
    if replaced != 0 {
        let replaced = open_read(destination)?;
        validate_file(&replaced)?;
        return Ok(());
    }
    // Never guess whether a failed replacement partly succeeded. Revalidate
    // whatever name remains, and classify path substitution as unsafe.
    for path in [temp, destination] {
        if path.exists() {
            let file = open_read(path)?;
            validate_file(&file)?;
        }
    }
    Err(StoreError::Unavailable)
}

pub(super) fn remove(path: &Path) -> Result<bool, StoreError> {
    validate_absolute_local_path(path)?;
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(StoreError::Unavailable),
        Ok(_) => {}
    }
    let file = OpenOptions::new()
        .access_mode(GENERIC_READ | DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| StoreError::Unavailable)?;
    validate_file(&file)?;
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    if unsafe {
        SetFileInformationByHandle(
            handle(&file),
            FileDispositionInfo,
            (&disposition as *const FILE_DISPOSITION_INFO).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        return Err(StoreError::Unavailable);
    }
    drop(file);
    Ok(true)
}

pub(super) fn sync_directory(path: &Path) -> Result<(), StoreError> {
    validate_absolute_local_path(path)?;
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| StoreError::Unavailable)?;
    let info = handle_info(&file)?;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
    {
        return Err(StoreError::UnsafeOrUnreadable);
    }
    validate_owner(&file)?;
    // Windows has no portable directory-fsync equivalent. The encrypted
    // stage itself is flushed before replacement; first creation additionally
    // uses MOVEFILE_WRITE_THROUGH. This opened handle revalidates the parent
    // before the operation returns success.
    Ok(())
}

fn local_app_data() -> Result<PathBuf, StoreError> {
    let mut raw = null_mut();
    let result = unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, null_mut(), &mut raw) };
    if result < 0 || raw.is_null() {
        if !raw.is_null() {
            unsafe { CoTaskMemFree(raw.cast()) };
        }
        return Err(StoreError::Unavailable);
    }
    let mut length = 0usize;
    unsafe {
        while *raw.add(length) != 0 {
            length += 1;
            if length > 32 * 1024 {
                CoTaskMemFree(raw.cast());
                return Err(StoreError::UnsafeOrUnreadable);
            }
        }
    }
    let path = PathBuf::from(std::ffi::OsString::from_wide(unsafe {
        std::slice::from_raw_parts(raw, length)
    }));
    unsafe { CoTaskMemFree(raw.cast()) };
    Ok(path)
}

fn validate_absolute_local_path(path: &Path) -> Result<(), StoreError> {
    if !path.is_absolute() {
        return Err(StoreError::UnsafeOrUnreadable);
    }
    let mut components = path.components();
    match components.next() {
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_)) => {}
        _ => return Err(StoreError::UnsafeOrUnreadable),
    }
    if !matches!(components.next(), Some(Component::RootDir)) {
        return Err(StoreError::UnsafeOrUnreadable);
    }
    for component in components {
        match component {
            Component::Normal(value) if safe_component(value) => {}
            _ => return Err(StoreError::UnsafeOrUnreadable),
        }
    }
    Ok(())
}

fn safe_component(value: &OsStr) -> bool {
    !value.is_empty()
        && !value
            .encode_wide()
            .any(|unit| unit == b':' as u16 || unit == 0)
}

fn validate_directory(path: &Path) -> Result<(), StoreError> {
    validate_absolute_local_path(path)?;
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| StoreError::Unavailable)?;
    let info = handle_info(&file)?;
    if info.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0
    {
        return Err(StoreError::UnsafeOrUnreadable);
    }
    validate_owner(&file)
}

fn validate_optional_directory(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(_) => validate_directory(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(StoreError::Unavailable),
    }
}

fn handle_info(file: &File) -> Result<BY_HANDLE_FILE_INFORMATION, StoreError> {
    let mut info = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(handle(file), &mut info) } == 0 {
        Err(StoreError::Unavailable)
    } else {
        Ok(info)
    }
}

fn validate_owner(file: &File) -> Result<(), StoreError> {
    let mut owner: PSID = null_mut();
    let mut dacl: *mut ACL = null_mut();
    let mut descriptor = null_mut();
    let result = unsafe {
        GetSecurityInfo(
            handle(file),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            null_mut(),
            &mut dacl,
            null_mut(),
            &mut descriptor,
        )
    };
    if result != 0 || owner.is_null() || dacl.is_null() || descriptor.is_null() {
        if !descriptor.is_null() {
            unsafe { LocalFree(descriptor) };
        }
        return Err(StoreError::Unavailable);
    }
    let checked =
        with_current_user_sid(|sid| unsafe { EqualSid(owner, sid) != 0 }).and_then(|equal| {
            if !equal {
                return Ok(false);
            }
            Ok(!well_known_group_can_write(dacl, WinWorldSid)?
                && !well_known_group_can_write(dacl, WinAuthenticatedUserSid)?)
        });
    unsafe { LocalFree(descriptor) };
    match checked {
        Ok(true) => Ok(()),
        Ok(false) => Err(StoreError::UnsafeOrUnreadable),
        Err(error) => Err(error),
    }
}

fn well_known_group_can_write(
    dacl: *const ACL,
    sid_type: WELL_KNOWN_SID_TYPE,
) -> Result<bool, StoreError> {
    let mut sid = [0u8; SECURITY_MAX_SID_SIZE as usize];
    let mut sid_length = sid.len() as u32;
    if unsafe {
        CreateWellKnownSid(
            sid_type,
            null_mut(),
            sid.as_mut_ptr().cast(),
            &mut sid_length,
        )
    } == 0
    {
        return Err(StoreError::Unavailable);
    }
    let mut trustee = TRUSTEE_W::default();
    unsafe { BuildTrusteeWithSidW(&mut trustee, sid.as_mut_ptr().cast()) };
    let mut rights = 0u32;
    if unsafe { GetEffectiveRightsFromAclW(dacl, &trustee, &mut rights) } != 0 {
        return Err(StoreError::Unavailable);
    }
    let write_rights = GENERIC_ALL
        | GENERIC_WRITE
        | DELETE
        | FILE_WRITE_DATA
        | FILE_APPEND_DATA
        | FILE_WRITE_EA
        | FILE_WRITE_ATTRIBUTES
        | WRITE_DAC
        | WRITE_OWNER;
    Ok(rights & write_rights != 0)
}

fn with_current_user_sid<T>(f: impl FnOnce(PSID) -> T) -> Result<T, StoreError> {
    let mut token: HANDLE = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(StoreError::Unavailable);
    }
    let mut required = 0u32;
    unsafe { GetTokenInformation(token, TokenUser, null_mut(), 0, &mut required) };
    if required < std::mem::size_of::<TOKEN_USER>() as u32 || required > 64 * 1024 {
        unsafe { CloseHandle(token) };
        return Err(StoreError::Unavailable);
    }
    let words = (required as usize).div_ceil(std::mem::size_of::<usize>());
    let mut buffer = vec![0usize; words];
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenUser,
            buffer.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    };
    unsafe { CloseHandle(token) };
    if result == 0 {
        return Err(StoreError::Unavailable);
    }
    let user = unsafe { &*(buffer.as_ptr().cast::<TOKEN_USER>()) };
    if user.User.Sid.is_null() {
        return Err(StoreError::Unavailable);
    }
    Ok(f(user.User.Sid))
}

#[derive(Clone, Copy)]
struct Identity {
    purpose: u8,
    key_digest: [u8; 32],
}

fn identity(path: &Path) -> Result<Identity, StoreError> {
    validate_absolute_local_path(path)?;
    let purpose = match path
        .parent()
        .and_then(Path::file_name)
        .and_then(OsStr::to_str)
    {
        Some("credentials") => 1,
        Some("devices") => 2,
        Some("contexts") => 3,
        _ => return Err(StoreError::UnsafeOrUnreadable),
    };
    let key = path
        .file_name()
        .and_then(OsStr::to_str)
        .filter(|key| !key.is_empty() && !key.starts_with('.'))
        .ok_or(StoreError::UnsafeOrUnreadable)?;
    Ok(Identity {
        purpose,
        key_digest: Sha256::digest(key.as_bytes()).into(),
    })
}

fn entropy(identity: &Identity) -> Zeroizing<Vec<u8>> {
    let mut hash = Sha256::new();
    hash.update(ENTROPY_DOMAIN);
    hash.update([0, identity.purpose]);
    hash.update(identity.key_digest);
    Zeroizing::new(hash.finalize().to_vec())
}

fn blob(bytes: &[u8]) -> Result<CRYPT_INTEGER_BLOB, StoreError> {
    let length = u32::try_from(bytes.len()).map_err(|_| StoreError::UnsafeOrUnreadable)?;
    Ok(CRYPT_INTEGER_BLOB {
        cbData: length,
        pbData: bytes.as_ptr().cast_mut(),
    })
}

fn zero_and_free_blob(blob: &mut CRYPT_INTEGER_BLOB) {
    if !blob.pbData.is_null() {
        unsafe {
            std::slice::from_raw_parts_mut(blob.pbData, blob.cbData as usize).zeroize();
            LocalFree(blob.pbData.cast());
        }
        blob.pbData = null_mut();
        blob.cbData = 0;
    }
}

fn handle(file: &File) -> HANDLE {
    file.as_raw_handle().cast::<c_void>()
}

fn wide(value: &str) -> Vec<u16> {
    OsStr::new(value).encode_wide().chain(Some(0)).collect()
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQUENCE: AtomicU64 = AtomicU64::new(1);

    fn test_path(store: &str, suffix: &str) -> PathBuf {
        let root = state_root().expect("Windows protected-state root");
        let store = root.join(store);
        ensure_directory(&store).expect("Windows protected-state store");
        let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
        store.join(format!(
            "windows-test-{}-{sequence}-{suffix}.json",
            std::process::id()
        ))
    }

    #[test]
    fn dpapi_envelope_is_not_plaintext_and_is_key_bound() {
        let first = test_path("credentials", "first");
        let second = test_path("credentials", "second");
        let secret = b"refresh-token-must-never-be-plaintext";
        let protected = protect(&first, secret).expect("protect");
        assert!(!protected.windows(secret.len()).any(|part| part == secret));
        let restored = Zeroizing::new(unprotect(&first, &protected).expect("unprotect"));
        assert_eq!(restored.as_slice(), secret);
        assert_eq!(
            unprotect(&second, &protected),
            Err(StoreError::UnsafeOrUnreadable)
        );
    }

    #[test]
    fn encrypted_atomic_file_round_trips_without_plaintext_on_disk() {
        let destination = test_path("devices", "roundtrip");
        let stage = destination.parent().unwrap().join(format!(
            ".{}.stage",
            destination.file_name().unwrap().to_string_lossy()
        ));
        let secret = b"device-private-state-must-never-be-plaintext";
        let protected = protect(&destination, secret).expect("protect");
        let mut file = create_new(&stage).expect("create stage");
        file.write_all(&protected).expect("write stage");
        file.sync_all().expect("sync stage");
        drop(file);
        replace(&stage, &destination).expect("replace");

        let mut disk = Vec::new();
        open_read(&destination)
            .expect("open encrypted state")
            .read_to_end(&mut disk)
            .expect("read encrypted state");
        assert!(!disk.windows(secret.len()).any(|part| part == secret));
        let restored = Zeroizing::new(unprotect(&destination, &disk).expect("unprotect"));
        assert_eq!(restored.as_slice(), secret);
        disk.zeroize();
        fs::remove_file(&destination).expect("cleanup");
    }

    #[test]
    fn lock_is_exclusive_and_hardlinks_are_refused() {
        let lock_path = test_path("contexts", "lock");
        let first = open_lock(&lock_path).expect("first lock handle");
        let second = open_lock(&lock_path).expect("second lock handle");
        lock_exclusive(&first, Duration::from_millis(50), Duration::from_millis(5))
            .expect("first lock");
        assert_eq!(
            lock_exclusive(&second, Duration::from_millis(50), Duration::from_millis(5)),
            Err(StoreError::Conflict)
        );
        unlock(&first).expect("unlock");
        drop(first);
        drop(second);
        fs::remove_file(&lock_path).expect("lock cleanup");

        let original = test_path("devices", "original");
        let linked = test_path("devices", "linked");
        let mut file = create_new(&original).expect("original");
        file.write_all(b"ciphertext fixture").expect("write");
        drop(file);
        fs::hard_link(&original, &linked).expect("hard link");
        assert!(matches!(
            open_read(&original),
            Err(StoreError::UnsafeOrUnreadable)
        ));
        fs::remove_file(&linked).expect("linked cleanup");
        fs::remove_file(&original).expect("original cleanup");
    }
}
