//! Shared host staging policy; the executable receives only private input files.
use super::NetworkFailure;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn invalid(error: impl ToString) -> NetworkFailure {
    NetworkFailure::new("network_scratch_invalid", error.to_string())
}

pub fn private_disk_stage() -> Result<tempfile::TempDir, NetworkFailure> {
    // An operator-selected scratch directory is used as given; the default
    // root is DS's own and is kept private (see `owned_root`).
    let root = match env::var_os("DS_NETWORK_SCRATCH_DIR").or_else(|| env::var_os("TMPDIR")) {
        Some(selected) => {
            let root = PathBuf::from(selected);
            if !root.is_absolute() {
                return Err(invalid("staging root must be absolute"));
            }
            fs::create_dir_all(&root).map_err(invalid)?;
            root
        }
        None => {
            let (base, owned): (PathBuf, &[&str]) = if let Some(path) = env::var_os("LOCALAPPDATA")
            {
                (path.into(), &["DataSolutions", "network-staging"])
            } else if let Some(path) = env::var_os("XDG_CACHE_HOME") {
                (path.into(), &["ds-network", "staging"])
            } else if let Some(path) = env::var_os("HOME") {
                (
                    PathBuf::from(path).join(".cache"),
                    &["ds-network", "staging"],
                )
            } else {
                return Err(invalid("set an absolute disk staging directory"));
            };
            owned_root(&base, owned)?
        }
    };
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

/// Create or reuse DS's default staging root below `base`.
///
/// Core admits a handoff only when every ancestor of the stage is protected
/// against substitution, so the components DS owns are created owner-only and
/// an earlier creation under a group-writable umask (Ubuntu's default 002) is
/// tightened in place. `base` itself (for example `~/.cache`) is not DS's and
/// is only created, owner-only, when it is missing.
fn owned_root(base: &Path, owned: &[&str]) -> Result<PathBuf, NetworkFailure> {
    if !base.is_absolute() {
        return Err(invalid("staging root must be absolute"));
    }
    if !base.exists() {
        private_builder(true).create(base).map_err(invalid)?;
    }
    let mut root = base.to_path_buf();
    for component in owned {
        root.push(component);
        match private_builder(false).create(&root) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => tighten(&root)?,
            Err(error) => return Err(invalid(error)),
        }
    }
    Ok(root)
}

fn private_builder(recursive: bool) -> fs::DirBuilder {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(recursive);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
}

#[cfg(unix)]
fn tighten(path: &Path) -> Result<(), NetworkFailure> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    // A handle opened without following links is the one checked and changed.
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY | libc::O_CLOEXEC)
        .open(path)
        .map_err(|error| invalid(format!("{}: {error}", path.display())))?;
    let metadata = directory.metadata().map_err(invalid)?;
    if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(invalid(format!(
            "{} is not a directory owned by this account",
            path.display()
        )));
    }
    if metadata.mode() & 0o077 != 0 {
        directory
            .set_permissions(fs::Permissions::from_mode(0o700))
            .map_err(invalid)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn tighten(path: &Path) -> Result<(), NetworkFailure> {
    let metadata = fs::symlink_metadata(path).map_err(invalid)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid("staging root must be an ordinary directory"));
    }
    Ok(())
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

    fn mode(path: &std::path::Path) -> u32 {
        fs::symlink_metadata(path).unwrap().mode() & 0o7777
    }

    #[test]
    fn default_root_is_created_owner_only() {
        let home = tempfile::tempdir().unwrap();
        let base = home.path().join(".cache");
        let root = owned_root(&base, &["ds-network", "staging"]).unwrap();
        assert_eq!(root, base.join("ds-network/staging"));
        for path in [&base, &base.join("ds-network"), &root] {
            assert_eq!(mode(path), 0o700, "{}", path.display());
        }
        assert_eq!(mode(create_stage(&root).unwrap().path()), 0o700);
    }

    #[test]
    fn group_writable_default_root_from_umask_002_is_tightened() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let base = home.path().join(".cache");
        fs::create_dir_all(base.join("ds-network/staging")).unwrap();
        for path in [base.join("ds-network"), base.join("ds-network/staging")] {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o775)).unwrap();
        }
        // `base` is not DS-owned and keeps the operator's own mode.
        fs::set_permissions(&base, fs::Permissions::from_mode(0o755)).unwrap();
        let root = owned_root(&base, &["ds-network", "staging"]).unwrap();
        assert_eq!(mode(&base), 0o755);
        assert_eq!(mode(&base.join("ds-network")), 0o700);
        assert_eq!(mode(&root), 0o700);
    }

    #[test]
    fn substituted_owned_component_is_refused_without_following_it() {
        use std::os::unix::fs::PermissionsExt;
        let home = tempfile::tempdir().unwrap();
        let elsewhere = tempfile::tempdir().unwrap();
        fs::set_permissions(elsewhere.path(), fs::Permissions::from_mode(0o775)).unwrap();
        let base = home.path().join(".cache");
        fs::create_dir_all(&base).unwrap();
        std::os::unix::fs::symlink(elsewhere.path(), base.join("ds-network")).unwrap();
        assert!(owned_root(&base, &["ds-network", "staging"]).is_err());
        assert_eq!(mode(elsewhere.path()), 0o775);
    }
}
