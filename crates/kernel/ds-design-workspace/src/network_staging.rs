//! Private disk staging for static package operations without a workspace root.
use std::{env, fs, path::PathBuf};

pub(crate) fn private_stage() -> Result<tempfile::TempDir, String> {
    let root = env::var_os("DS_NETWORK_SCRATCH_DIR")
        .or_else(|| env::var_os("TMPDIR"))
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("LOCALAPPDATA")
                .map(|p| PathBuf::from(p).join("DataSolutions/network-staging"))
        })
        .or_else(|| {
            env::var_os("XDG_CACHE_HOME").map(|p| PathBuf::from(p).join("ds-network/staging"))
        })
        .or_else(|| env::var_os("HOME").map(|p| PathBuf::from(p).join(".cache/ds-network/staging")))
        .ok_or_else(|| {
            "network_scratch_invalid: set an absolute disk staging directory".to_owned()
        })?;
    if !root.is_absolute() {
        return Err("network_scratch_invalid: staging root must be absolute".into());
    }
    fs::create_dir_all(&root).map_err(|e| format!("network_scratch_invalid: {e}"))?;
    let metadata =
        fs::symlink_metadata(&root).map_err(|e| format!("network_scratch_invalid: {e}"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err("network_scratch_invalid: staging root must be an ordinary directory".into());
    }
    // Child staging has tempfile's private permissions and is removed on every exit.
    tempfile::Builder::new()
        .prefix("ds-grid-publication-")
        .tempdir_in(root)
        .map_err(|e| format!("network_scratch_invalid: {e}"))
}
