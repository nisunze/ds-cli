//! Compile and execute the actual Desktop adapter over the shared owner.
#![allow(dead_code, unused_imports)]
mod connectivity {
    pub fn require_online() -> Result<(), String> {
        Ok(())
    }
}
#[path = "../../../../../ds-web/src-tauri/src/download_manager.rs"]
mod desktop;
use sha2::{Digest, Sha256};
#[test]
fn desktop_adapter_preserves_verified_files_and_refuses_before_transport() {
    let dir = tempfile::tempdir().unwrap();
    let root =
        desktop::ensure_owned_directory(dir.path(), std::path::Path::new("updates")).unwrap();
    let fresh = desktop::create_fresh_private_directory(&root, "desktop").unwrap();
    let file = fresh.join("artifact");
    let bytes = b"exact immutable installed fixture";
    std::fs::write(&file, bytes).unwrap();
    let digest = format!("{:x}", Sha256::digest(bytes));
    let held =
        desktop::reopen_verified_download(&file, bytes.len() as u64, &digest, || Ok(())).unwrap();
    assert_eq!(std::fs::read(held.path()).unwrap(), bytes);
    assert!(
        desktop::reopen_verified_download(&file, bytes.len() as u64 + 1, &digest, || Ok(()))
            .is_err()
    );
    assert!(desktop::download_verified(
        "https://unused.invalid",
        &fresh.join("never"),
        &digest,
        0,
        "invalid",
        |_| panic!("no transfer"),
        || Ok(())
    )
    .is_err());
    assert!(desktop::download_crc32c_verified(
        "https://unused.invalid",
        &fresh.join("never-crc"),
        0,
        0,
        "invalid",
        |_| panic!("no transfer"),
        || Ok(())
    )
    .is_err());
    assert!(!fresh.join("never").exists());
    assert!(!fresh.join("never-crc").exists());
}
