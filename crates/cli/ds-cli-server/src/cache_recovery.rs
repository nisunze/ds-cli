//! Core startup's bounded cloud-cache recovery. Never called by generic Store::open.
use ds_sync_store::cache_health::{self, CacheHealth};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
};

const DATABASE: &str = "store.sqlite";
const SUFFIXES: [&str; 4] = ["-wal", "-shm", "-journal", ""];
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct HeldFile {
    pub(crate) name: String,
    pub(crate) size_bytes: u64,
    pub(crate) sha256: String,
}

/// The borrow proves the existing native single-Core owner remains held.
/// Its protected connection and native install identity are settled before this call.
pub(crate) fn prepare(
    directory: &Path,
    listening: &crate::transport::Listening,
) -> Result<(), String> {
    if listening.socket() != crate::transport::socket_path(directory) {
        return Err("Core cache recovery requires this state's existing startup owner".into());
    }
    if let Some(receipt) = prepare_cache(directory)? {
        eprintln!(
            "DS Core quarantined a structurally corrupt cloud cache for recreation; original bytes retained by {}",
            receipt.display()
        );
    }
    Ok(())
}
pub(crate) fn private_file(path: &Path) -> Result<File, String> {
    #[cfg(windows)]
    {
        return ds_cli_auth::windows_host::read_file(path);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|e| e.to_string())?;
        let meta = file.metadata().map_err(|e| e.to_string())?;
        // SAFETY: geteuid has no arguments or pointers.
        if !meta.is_file()
            || meta.uid() != unsafe { libc::geteuid() }
            || meta.mode() & 0o077 != 0
            || meta.nlink() != 1
        {
            return Err("Core cache must remain an owner-only ordinary single-link file".into());
        }
        Ok(file)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err("Core cache recovery requires its protected native platform owner".into())
    }
}
fn held(path: &Path) -> Result<HeldFile, String> {
    held_with_limit(path, u64::MAX)
}
pub(crate) fn held_with_limit(path: &Path, limit: u64) -> Result<HeldFile, String> {
    let file = private_file(path)?;
    let mut file = file.take(limit.saturating_add(1));
    let mut hash = Sha256::new();
    let mut size = 0u64;
    let mut bytes = [0u8; 65536];
    loop {
        let count = file.read(&mut bytes).map_err(|e| e.to_string())?;
        if count == 0 {
            break;
        }
        size += count as u64;
        if size > limit {
            return Err("Core cache exceeds its held file bound".into());
        }
        hash.update(&bytes[..count]);
    }
    Ok(HeldFile {
        name: path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("invalid Core cache name")?
            .into(),
        size_bytes: size,
        sha256: format!("{:x}", hash.finalize()),
    })
}
pub(crate) fn create_private(path: &Path, bytes: &[u8]) -> Result<(), String> {
    #[cfg(any(unix, windows))]
    {
        #[cfg(windows)]
        let mut file = ds_cli_auth::windows_host::create_file(path)?;
        #[cfg(unix)]
        let mut file = {
            use std::os::unix::fs::OpenOptionsExt;
            fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(path)
                .map_err(|e| e.to_string())?
        };
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, bytes);
        Err("Core cache recovery requires protected native files".into())
    }
}

pub(crate) fn sync_directory(directory: &Path) -> Result<(), String> {
    #[cfg(unix)]
    {
        File::open(directory)
            .and_then(|f| f.sync_all())
            .map_err(|e| e.to_string())
    }
    // The existing protected Windows replacement owner uses write-through.
    #[cfg(windows)]
    {
        ds_cli_auth::windows_host::validate_directory(directory)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = directory;
        Err("Core cache recovery durability unavailable".into())
    }
}
pub(crate) fn move_private(source: &Path, destination: &Path) -> Result<(), String> {
    if destination.try_exists().map_err(|e| e.to_string())? {
        return Err("Core cache quarantine destination already exists".into());
    }
    drop(private_file(source)?);
    #[cfg(windows)]
    {
        ds_cli_auth::windows_host::replace_file(source, destination)?;
    }
    #[cfg(not(windows))]
    {
        fs::rename(source, destination).map_err(|e| e.to_string())?;
    }
    sync_directory(source.parent().ok_or("Core cache has no parent")?)
}
/// Retention never replaces an occupied archive/evidence path, even if it is
/// introduced between admission and rename. Ordinary replacement is unchanged.
pub(crate) fn move_private_no_replace(source: &Path, destination: &Path) -> Result<(), String> {
    drop(private_file(source)?);
    if source.parent() != destination.parent() {
        return Err("Core retention must stay in its protected directory".into());
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::ffi::OsStrExt;
        let from =
            std::ffi::CString::new(source.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
        let to = std::ffi::CString::new(destination.as_os_str().as_bytes())
            .map_err(|e| e.to_string())?;
        // SAFETY: both C strings remain live, and no caller-supplied flags or fd.
        if unsafe {
            libc::renameat2(
                libc::AT_FDCWD,
                from.as_ptr(),
                libc::AT_FDCWD,
                to.as_ptr(),
                libc::RENAME_NOREPLACE,
            )
        } != 0
        {
            return Err(std::io::Error::last_os_error().to_string());
        }
    }
    #[cfg(windows)]
    ds_cli_auth::windows_host::move_file_no_replace(source, destination)?;
    #[cfg(not(any(target_os = "linux", windows)))]
    return Err("Core retention requires its admitted Linux or Windows filesystem owner".into());
    sync_directory(source.parent().ok_or("Core cache has no parent")?)
}

fn prepare_cache(directory: &Path) -> Result<Option<PathBuf>, String> {
    crate::host::prepare_directory(directory)?;
    let database = directory.join(DATABASE);
    let mut files = Vec::new();
    for suffix in SUFFIXES {
        let path = directory.join(format!("{DATABASE}{suffix}"));
        if path.try_exists().map_err(|e| e.to_string())? {
            drop(private_file(&path)?);
            files.push(path);
        }
    }
    if !database.try_exists().map_err(|e| e.to_string())? {
        if !files.is_empty() {
            return Err("Core cache has orphaned SQLite sidecars; state left untouched".into());
        }
        return Ok(None);
    }
    match cache_health::assess(&database).map_err(|e| e.to_string())? {
        CacheHealth::Missing => return Err("Core cache changed during startup assessment".into()),
        CacheHealth::StructurallySound => return Ok(None),
        CacheHealth::StructurallyCorrupt => {}
    }
    let files = files
        .into_iter()
        .map(|path| held(&path).map(|file| (path, file)))
        .collect::<Result<Vec<_>, _>>()?;
    quarantine(directory, &files, &move_private).map(Some)
}
fn quarantine(
    directory: &Path,
    files: &[(PathBuf, HeldFile)],
    rename: &dyn Fn(&Path, &Path) -> Result<(), String>,
) -> Result<PathBuf, String> {
    // Hash only failed caches. Every exact original member is retained; no SQL
    // recovery, row assignment, credential/identity rewrite or project-file scan.
    let mut nonce = [0u8; 16];
    getrandom::getrandom(&mut nonce).map_err(|e| e.to_string())?;
    let nonce = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let prefix = format!("{DATABASE}.corrupt-{nonce}");
    let receipt = directory.join(format!("{prefix}.receipt.json"));
    let bytes=serde_json::to_vec(&serde_json::json!({"schema":"ds.core-cache-quarantine/v1","reason":"sqlite_structural_corruption","files":files.iter().map(|(_,f)|f).collect::<Vec<_>>(),"old_history_recovered":false})).map_err(|e|e.to_string())?;
    create_private(&receipt, &bytes)?;
    sync_directory(directory)?;
    let mut moved: Vec<(PathBuf, PathBuf)> = Vec::new();
    for (source, original) in files {
        let suffix = original
            .name
            .strip_prefix(DATABASE)
            .ok_or("unexpected Core cache member")?;
        let destination = directory.join(format!("{prefix}{suffix}"));
        let attempt = (|| {
            let now = held(source)?;
            if now.size_bytes != original.size_bytes || now.sha256 != original.sha256 {
                return Err("Core cache changed before quarantine".into());
            }
            rename(source, &destination)
        })();
        if let Err(error) = attempt {
            // A platform rename can succeed before its durability check fails.
            // Include that member in rollback rather than claiming no effect.
            if !source.exists() && destination.exists() {
                moved.push((source.clone(), destination.clone()));
            }
            let mut rollback_failed = false;
            for (from, to) in moved.iter().rev() {
                rollback_failed |= move_private(to, from).is_err();
            }
            return Err(format!(
                "Core cache quarantine stopped: {error}; rollback_failed={rollback_failed}; diagnostic receipt {}",
                receipt.display()
            ));
        }
        moved.push((source.clone(), destination));
    }
    // Sidecars move durably before the database, so a crash cannot attach an
    // old WAL/journal to the fresh canonical cache. Failed moves stop startup.
    create_private(
        &directory.join(format!("{prefix}.complete")),
        b"ds.core-cache-quarantine/v1\n",
    )?;
    sync_directory(directory)?;
    Ok(receipt)
}

#[cfg(test)]
#[path = "cache_recovery_tests.rs"]
mod tests;
