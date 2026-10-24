//! SQLite's own sidecar lifecycle never refuses a concurrent Store open,
//! while a planted hard link, symlink or non-regular sidecar still refuses.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};

const OPENERS: usize = 8;
const ROUNDS: usize = 40;

fn private_directory() -> tempfile::TempDir {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    directory
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

/// Every round starts OPENERS Stores at once on one directory, each closing
/// again straight away, so the last close of one round races the next
/// opener's sidecar admission: SQLite unlinks `-wal`/`-shm` (and `-journal`
/// while the store is first created) exactly while another opener checks them.
#[test]
fn concurrent_startup_opens_of_one_store_all_succeed() {
    let directory = private_directory();
    let path = Arc::new(directory.path().join("sync.sqlite"));
    let mut failures = Vec::new();
    for _ in 0..ROUNDS {
        let barrier = Arc::new(Barrier::new(OPENERS));
        let openers: Vec<_> = (0..OPENERS)
            .map(|_| {
                let barrier = barrier.clone();
                let path = path.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let mut refused = Vec::new();
                    for _ in 0..8 {
                        match ds_sync_store::Store::open(&path) {
                            Ok(store) => drop(store),
                            Err(error) => refused.push(error.to_string()),
                        }
                    }
                    refused
                })
            })
            .collect();
        for opener in openers {
            failures.extend(opener.join().unwrap());
        }
    }
    let mut causes = std::collections::BTreeMap::<String, usize>::new();
    for failure in &failures {
        *causes.entry(failure.clone()).or_default() += 1;
    }
    assert!(
        failures.is_empty(),
        "{} of {} concurrent opens refused: {causes:?}",
        failures.len(),
        ROUNDS * OPENERS * 8,
    );
}

#[test]
fn planted_hard_linked_sidecar_still_refuses() {
    let directory = private_directory();
    let path = directory.path().join("sync.sqlite");
    drop(ds_sync_store::Store::open(&path).unwrap());
    let target = directory.path().join("unrelated.bin");
    std::fs::write(&target, b"untouched").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
    let wal = sidecar(&path, "-wal");
    let _ = std::fs::remove_file(&wal);
    std::fs::hard_link(&target, &wal).unwrap();
    let error = ds_sync_store::Store::open(&path).err().expect("refused");
    assert!(error.to_string().contains("Store sidecar"), "{error}");
    assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
}

#[test]
fn planted_symlinked_sidecar_still_refuses() {
    let directory = private_directory();
    let path = directory.path().join("sync.sqlite");
    drop(ds_sync_store::Store::open(&path).unwrap());
    let target = directory.path().join("unrelated.bin");
    std::fs::write(&target, b"untouched").unwrap();
    let shm = sidecar(&path, "-shm");
    let _ = std::fs::remove_file(&shm);
    std::os::unix::fs::symlink(&target, &shm).unwrap();
    let error = ds_sync_store::Store::open(&path).err().expect("refused");
    assert!(error.to_string().contains("Store sidecar"), "{error}");
    assert_eq!(std::fs::read(&target).unwrap(), b"untouched");
}

#[test]
fn planted_non_regular_sidecar_still_refuses() {
    let directory = private_directory();
    let path = directory.path().join("sync.sqlite");
    drop(ds_sync_store::Store::open(&path).unwrap());
    let journal = sidecar(&path, "-journal");
    let _ = std::fs::remove_file(&journal);
    std::fs::create_dir(&journal).unwrap();
    let error = ds_sync_store::Store::open(&path).err().expect("refused");
    assert!(error.to_string().contains("Store sidecar"), "{error}");
}
