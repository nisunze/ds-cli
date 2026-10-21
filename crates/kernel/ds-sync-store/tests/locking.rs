//! A second same-process Store must not cancel SQLite's kernel locks.
#[cfg(target_os = "linux")]
#[test]
fn second_store_open_preserves_existing_sqlite_posix_locks() {
    fn independent_exclusive_lock(path: &std::path::Path) -> String {
        // POSIX locks are process-scoped: a separate process is essential.
        // Locking the complete private file covers SQLite's locking-byte range.
        // This child never opens SQLite, reads rows or changes file bytes.
        let output = std::process::Command::new("python3")
            .args([
                "-c",
                r#"import os,fcntl,sys
fd=os.open(sys.argv[1],os.O_RDWR)
try:
 try:
  fcntl.lockf(fd,fcntl.LOCK_EX|fcntl.LOCK_NB)
  print('ACQUIRED')
 except BlockingIOError:
  print('BUSY')
finally:
 os.close(fd)
"#,
            ])
            .arg(path)
            .output()
            .expect("Linux Store lock regression requires python3");
        assert!(output.status.success(), "independent lock probe failed");
        String::from_utf8(output.stdout).unwrap().trim().into()
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
