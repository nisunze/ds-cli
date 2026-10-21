//! Persistent, host-owned UUIDv4 installation identity.
//!
//! The authority directory belongs to the host (Tauri or the native server).
//! This module only makes the one identity file race-safe, bounded and
//! symlink-resistant; callers never accept it from a remote response.

use std::fs::{File, OpenOptions};
use std::io::{ErrorKind, Read, Seek, Write};
use std::path::Path;
use uuid::Uuid;

const MAX_INSTALL_ID_BYTES: u64 = 64;

fn read_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
}

fn create_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options
}

fn parse(raw: &str) -> Result<String, String> {
    let exact = raw.strip_suffix('\n').unwrap_or(raw);
    if exact.contains(['\n', '\r']) {
        return Err("native install identity contains trailing data".into());
    }
    let parsed = Uuid::parse_str(exact)
        .map_err(|_| "native install identity is not a canonical UUIDv4".to_string())?;
    if parsed.get_version_num() != 4 || parsed.to_string() != exact {
        return Err("native install identity is not a canonical UUIDv4".into());
    }
    Ok(exact.to_owned())
}

fn read_open(mut file: File) -> Result<String, String> {
    let metadata = file
        .metadata()
        .map_err(|_| "could not inspect native install identity".to_string())?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_INSTALL_ID_BYTES {
        return Err("native install identity is not a bounded regular file".into());
    }
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(MAX_INSTALL_ID_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "could not read native install identity".to_string())?;
    if bytes.len() as u64 > MAX_INSTALL_ID_BYTES {
        return Err("native install identity exceeds its size limit".into());
    }
    parse(std::str::from_utf8(&bytes).map_err(|_| "native install identity is not UTF-8")?)
}

/// Observe an existing installation identity without creating any file or
/// directory. The host still validates its private authority directory.
pub fn load_install_id(path: &Path) -> Result<Option<String>, String> {
    match read_options().open(path) {
        Ok(file) => read_open(file).map(Some),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(_) => Err("could not open native install identity".into()),
    }
}

/// Decode a held, read-only handle whose private ownership/ACL the host has
/// already verified. This avoids reopening a Windows protected-state path.
pub fn read_install_id(file: File) -> Result<String, String> {
    read_open(file)
}

/// Load or create the one host-owned installation UUID. `authority_dir` must
/// already be private and free of symlinks; its validation remains host-owned.
pub fn load_or_create_install_id(path: &Path) -> Result<String, String> {
    match read_options().open(path) {
        Ok(file) => return read_open(file),
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(_) => return Err("could not open native install identity".into()),
    }
    let id = Uuid::new_v4().to_string();
    match create_options().open(path) {
        Ok(mut file) => {
            file.write_all(format!("{id}\n").as_bytes())
                .map_err(|_| "could not write native install identity")?;
            file.sync_all()
                .map_err(|_| "could not sync native install identity")?;
            #[cfg(unix)]
            File::open(
                path.parent()
                    .ok_or("native install identity has no authority directory")?,
            )
            .and_then(|directory| directory.sync_all())
            .map_err(|_| "could not sync native identity authority")?;
            file.rewind()
                .map_err(|_| "could not verify native install identity")?;
            read_open(file)
        }
        Err(error) if error.kind() == ErrorKind::AlreadyExists => read_open(
            read_options()
                .open(path)
                .map_err(|_| "could not open native install identity")?,
        ),
        Err(_) => Err("could not create native install identity".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn identity_is_stable_canonical_uuid_v4() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("install-id");
        let first = load_or_create_install_id(&path).unwrap();
        assert_eq!(first, load_or_create_install_id(&path).unwrap());
        assert_eq!(Uuid::parse_str(&first).unwrap().get_version_num(), 4);
    }

    #[test]
    fn observation_never_creates_missing_identity_or_parents() {
        let root = tempfile::tempdir().unwrap();
        let absent = root.path().join("absent").join("install-id");
        assert_eq!(load_install_id(&absent).unwrap(), None);
        assert!(!absent.parent().unwrap().exists());
        let path = root.path().join("install-id");
        assert_eq!(load_install_id(&path).unwrap(), None);
        assert!(!path.exists());
        let id = load_or_create_install_id(&path).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert_eq!(load_install_id(&path).unwrap(), Some(id));
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[test]
    fn observation_preserves_invalid_bytes_and_refuses_noncanonical_identity() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("install-id");
        for bytes in [b"not-a-uuid".as_slice(), b"", &[b'x'; 65]] {
            std::fs::write(&path, bytes).unwrap();
            assert!(load_install_id(&path).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bytes);
        }
    }

    #[cfg(unix)]
    #[test]
    fn observation_refuses_link_without_following_or_replacing_it() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("install-id");
        let target = root.path().join("target");
        let id = load_or_create_install_id(&target).unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(load_install_id(&path).is_err());
        assert_eq!(load_install_id(&target).unwrap(), Some(id));
        assert!(std::fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink());
    }

    #[cfg(unix)]
    #[test]
    fn created_identity_is_owner_read_write_only() {
        use std::os::unix::fs::MetadataExt;

        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("install-id");
        load_or_create_install_id(&path).unwrap();
        assert_eq!(std::fs::metadata(path).unwrap().mode() & 0o777, 0o600);
    }
}
