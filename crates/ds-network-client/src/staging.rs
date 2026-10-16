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
    create_stage(&root)
}

fn create_stage(root: &std::path::Path) -> Result<tempfile::TempDir, NetworkFailure> {
    let mut builder = tempfile::Builder::new();
    builder.prefix("ds-network-host-");
    // tempfile directories otherwise inherit 0777 masked by the caller's
    // umask. Restrict at creation, before a request or result can enter it.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    builder
        .tempdir_in(root)
        .map_err(|error| NetworkFailure::new("network_scratch_invalid", error.to_string()))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn stage_under_shared_parent_is_private_and_cleans_up() {
        use std::os::unix::fs::PermissionsExt;
        let parent = tempfile::tempdir().unwrap();
        fs::set_permissions(parent.path(), fs::Permissions::from_mode(0o755)).unwrap();
        let path = {
            let stage = create_stage(parent.path()).unwrap();
            let metadata = fs::symlink_metadata(stage.path()).unwrap();
            assert!(metadata.is_dir() && !metadata.file_type().is_symlink());
            assert_eq!(metadata.uid(), unsafe { libc::geteuid() });
            assert_eq!(metadata.mode() & 0o777, 0o700);
            fs::write(stage.path().join("request.json"), b"{}").unwrap();
            stage.path().to_owned()
        };
        assert!(!path.exists());
        assert!(parent.path().is_dir());
    }
}
