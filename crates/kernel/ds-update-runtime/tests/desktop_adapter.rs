//! Exercise the native transfer interface consumed by Desktop and Core.
use ds_update_runtime::transfer::{self as transfer, TransferAdmission};
use sha2::{Digest, Sha256};
#[test]
fn shared_transfer_preserves_verified_files_and_refuses_before_transport() {
    let dir = tempfile::tempdir().unwrap();
    let root =
        transfer::ensure_owned_directory(dir.path(), std::path::Path::new("updates")).unwrap();
    let fresh = transfer::create_fresh_private_directory(&root, "desktop").unwrap();
    let file = fresh.join("artifact");
    let bytes = b"exact immutable installed fixture";
    std::fs::write(&file, bytes).unwrap();
    let digest = format!("{:x}", Sha256::digest(bytes));
    let held =
        transfer::reopen_verified_download(&file, bytes.len() as u64, &digest, || Ok(())).unwrap();
    assert_eq!(std::fs::read(held.path()).unwrap(), bytes);
    assert!(
        transfer::reopen_verified_download(&file, bytes.len() as u64 + 1, &digest, || Ok(()))
            .is_err()
    );
    let admission = TransferAdmission {
        check_cancelled: || Ok(()),
        admit_transport: || panic!("invalid identity must refuse before transport admission"),
    };
    assert!(transfer::download_verified(
        "https://unused.invalid",
        &fresh.join("never"),
        &digest,
        0,
        "invalid",
        |_| panic!("no transfer"),
        admission,
    )
    .is_err());
    assert!(transfer::download_crc32c_verified(
        "https://unused.invalid",
        &fresh.join("never-crc"),
        0,
        0,
        "invalid",
        |_| panic!("no transfer"),
        admission,
    )
    .is_err());
    assert!(!fresh.join("never").exists());
    assert!(!fresh.join("never-crc").exists());
}
