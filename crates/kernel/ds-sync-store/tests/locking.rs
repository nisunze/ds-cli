//! A second same-process Store must not cancel SQLite's kernel locks.

/// Names the file `lock_probe_child` tries to lock; unset in ordinary runs.
#[cfg(target_os = "linux")]
const PROBE: &str = "DS_STORE_LOCK_PROBE";

/// The independent process of the lock regression: one non-blocking
/// whole-file write lock (SQLite's locking bytes included), reported and
/// released when the process exits. Without the variable it does nothing.
#[cfg(target_os = "linux")]
#[test]
fn lock_probe_child() {
    use std::os::unix::io::AsRawFd;
    let Some(path) = std::env::var_os(PROBE) else {
        return;
    };
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .unwrap();
    // SAFETY: a zeroed flock is a valid "whole file from offset 0" request.
    let mut lock: libc::flock = unsafe { std::mem::zeroed() };
    lock.l_type = libc::F_WRLCK as _;
    lock.l_whence = libc::SEEK_SET as _;
    // SAFETY: the descriptor is open for the duration of the call.
    let acquired = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETLK, &lock) } == 0;
    println!("lock-probe:{}", if acquired { "ACQUIRED" } else { "BUSY" });
}

#[cfg(target_os = "linux")]
#[test]
fn second_store_open_preserves_existing_sqlite_posix_locks() {
    fn independent_exclusive_lock(path: &std::path::Path) -> String {
        // POSIX locks are process-scoped: a separate process is essential.
        // The probe is this test binary again (`lock_probe_child`), which
        // never opens SQLite, reads rows or changes file bytes.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "lock_probe_child", "--nocapture", "--test-threads=1"])
            .env(PROBE, path)
            .output()
            .expect("the lock probe child runs");
        assert!(output.status.success(), "independent lock probe failed");
        let stdout = String::from_utf8(output.stdout).unwrap();
        stdout
            .lines()
            .find_map(|line| line.split_once("lock-probe:").map(|(_, answer)| answer.trim()))
            .expect("the probe reports its answer")
            .into()
    }
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(
        directory.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let path = directory.path().join("store.sqlite");
    let first = ds_sync_store::Store::open(&path).unwrap();
    first
        .connection_for_test()
        .execute_batch("BEGIN IMMEDIATE;")
        .unwrap();
    assert_eq!(
        independent_exclusive_lock(&path),
        "BUSY",
        "first SQLite connection must own real kernel locks"
    );
    let second = ds_sync_store::Store::open(&path).unwrap();
    assert_eq!(
        independent_exclusive_lock(&path),
        "BUSY",
        "ordinary preparation must not close a descriptor for an already-open SQLite inode"
    );
    drop(second);
    assert_eq!(
        independent_exclusive_lock(&path),
        "BUSY",
        "closing only the second SQLite connection must preserve the first owner's locks"
    );
    first
        .connection_for_test()
        .execute_batch("ROLLBACK;")
        .unwrap();
    drop(first);
    assert_eq!(
        independent_exclusive_lock(&path),
        "ACQUIRED",
        "final SQLite owner must release its locks"
    );
}

#[cfg(unix)]
#[test]
fn private_preparation_refuses_links_and_open_parent_without_changing_target() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(
        directory.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();
    let target = directory.path().join("unrelated.bin");
    std::fs::write(&target, b"untouched").unwrap();
    let linked = directory.path().join("linked.sqlite");
    symlink(&target, &linked).unwrap();
    assert!(ds_sync_store::Store::open(&linked).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
    let hard = directory.path().join("hard.sqlite");
    std::fs::hard_link(&target, &hard).unwrap();
    assert!(ds_sync_store::Store::open(&hard).is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
    let open_parent = directory.path().join("open");
    std::fs::create_dir(&open_parent).unwrap();
    std::fs::set_permissions(&open_parent, std::fs::Permissions::from_mode(0o777)).unwrap();
    let path = open_parent.join("store.sqlite");
    assert!(ds_sync_store::Store::open(&path).is_err());
    assert!(!path.exists());
    let private = directory.path().join("private.sqlite");
    let store = ds_sync_store::Store::open(&private).unwrap();
    assert_eq!(
        std::fs::symlink_metadata(&private)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    drop(store);
}

#[cfg(unix)]
#[test]
fn missing_parent_is_created_private_even_with_permissive_host_umask() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = root.path().join("new/private/store.sqlite");
    let store = ds_sync_store::Store::open(&path).unwrap();
    for directory in [root.path().join("new"), root.path().join("new/private")] {
        assert_eq!(
            std::fs::symlink_metadata(directory)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
    }
    assert_eq!(
        std::fs::symlink_metadata(&path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    drop(store);
    let unsafe_parent = root.path().join("existing-shared");
    std::fs::create_dir(&unsafe_parent).unwrap();
    std::fs::set_permissions(&unsafe_parent, std::fs::Permissions::from_mode(0o775)).unwrap();
    let existing = unsafe_parent.join("store.sqlite");
    std::fs::write(&existing, b"do-not-migrate-or-delete").unwrap();
    assert!(ds_sync_store::Store::open(&existing).is_err());
    assert_eq!(
        std::fs::read(&existing).unwrap(),
        b"do-not-migrate-or-delete"
    );
    assert_eq!(
        std::fs::metadata(unsafe_parent)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o775
    );
}
