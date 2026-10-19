//! Fixtures shared by the tests of every ds-cli crate (feature `testing`,
//! enabled only from `[dev-dependencies]`).

/// A fresh owner-only (0700) directory on disk, removed when dropped.
///
/// Deliberately not `temp_dir()`: on DS hosts `/tmp` is RAM-backed, and its
/// shared sticky ancestor is refused by snapshot privacy. Tests that write
/// private state or snapshots use this, the way the Kernel's own fixtures do.
#[cfg(unix)]
pub fn private_tempdir(prefix: &str) -> tempfile::TempDir {
    use std::os::unix::fs::PermissionsExt;
    let home = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .filter(|home| home.is_absolute())
        .expect("a disk-backed HOME for private test fixtures");
    tempfile::Builder::new()
        .permissions(std::fs::Permissions::from_mode(0o700))
        .prefix(prefix)
        .tempdir_in(home)
        .expect("a private test directory under HOME")
}

/// Windows keeps the platform's per-user temporary directory, whose ACL is
/// already the user's own.
#[cfg(not(unix))]
pub fn private_tempdir(prefix: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(prefix)
        .tempdir()
        .expect("a private test directory")
}
