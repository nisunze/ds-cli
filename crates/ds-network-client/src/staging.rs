//! Shared host staging policy; the executable receives only private input files.
use super::NetworkFailure;
use std::{env, fs, path::PathBuf};
pub fn private_disk_stage() -> Result<tempfile::TempDir, NetworkFailure> {
    let root = env::var_os("DS_NETWORK_SCRATCH_DIR")
        .or_else(|| env::var_os("TMPDIR"))
        .map(PathBuf::from)
        .or_else(|| {
            env::var_os("LOCALAPPDATA")
                .map(|path| PathBuf::from(path).join("DataSolutions/network-staging"))
        })
        .or_else(|| {
            env::var_os("XDG_CACHE_HOME").map(|path| PathBuf::from(path).join("ds-network/staging"))
        })
        .or_else(|| {
            env::var_os("HOME").map(|path| PathBuf::from(path).join(".cache/ds-network/staging"))
        })
        .ok_or_else(|| {
            NetworkFailure::new(
                "network_scratch_invalid",
                "set an absolute disk staging directory",
            )
        })?;
    if !root.is_absolute() {
        return Err(NetworkFailure::new(
            "network_scratch_invalid",
            "staging root must be absolute",
        ));
    }
    fs::create_dir_all(&root)
        .map_err(|error| NetworkFailure::new("network_scratch_invalid", error.to_string()))?;
    let metadata = fs::symlink_metadata(&root)
        .map_err(|error| NetworkFailure::new("network_scratch_invalid", error.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(NetworkFailure::new(
            "network_scratch_invalid",
            "staging root must be an ordinary directory",
        ));
    }
    tempfile::Builder::new()
        .prefix("ds-network-host-")
        .tempdir_in(root)
        .map_err(|error| NetworkFailure::new("network_scratch_invalid", error.to_string()))
}
