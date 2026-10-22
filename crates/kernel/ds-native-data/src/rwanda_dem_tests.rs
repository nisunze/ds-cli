use super::*;

#[test]
fn immutable_catalog_values_match_published_contract() {
    assert_eq!(VERSION, "2026-08-28");
    assert_eq!(COG_SIZE, 882_872_744);
    assert_eq!(COG_CRC32C, 0x31da_d180);
    assert!(INDEX_SHA256.len() == 64 && COVERAGE_SHA256.len() == 64);
    assert!(BLOCK_CACHE_MAX_BYTES < COG_SIZE);
}

#[test]
fn missing_status_reports_resumable_bytes_without_claiming_ready() {
    let root = std::env::temp_dir().join(format!("ds-dem-status-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join(STAGING_DIR)).expect("staging");
    std::fs::write(
        root.join(STAGING_DIR).join(COG_FILE).with_extension("part"),
        b"partial",
    )
    .expect("partial");
    let status = status_at(&root);
    assert_eq!(status.state, "missing");
    assert_eq!(status.partial_bytes, 7);
    assert_eq!(
        status.message.as_deref(),
        Some("An interrupted download can be resumed")
    );
    let _ = std::fs::remove_dir_all(root);
}

// Feedback dbe4d17e: the whole COG and metadata were downloaded, then
// verification failed; status said "missing" and named nothing.
#[test]
fn missing_status_names_the_last_installation_failure() {
    let root = std::env::temp_dir().join(format!("ds-dem-failure-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join(STAGING_DIR)).expect("staging");
    std::fs::write(root.join(STAGING_DIR).join(COG_FILE), b"complete").expect("cog");
    record_install_failure(
        &root,
        "Rwanda DEM COG failed its published size/CRC32C proof",
    );
    let status = status_at(&root);
    assert_eq!(status.state, "missing");
    assert_eq!(
        status.message.as_deref(),
        Some(
            "The last Rwanda DEM installation failed: Rwanda DEM COG failed its published size/CRC32C proof"
        )
    );
    record_install_failure(&root, &"x".repeat(5000));
    assert_eq!(
        last_install_failure(&root).map(|text| text.len()),
        Some(LAST_FAILURE_MAX_BYTES)
    );
    clear_install_failure(&root);
    assert_eq!(
        status_at(&root).message.as_deref(),
        Some("An interrupted download can be resumed")
    );
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn receipt_refuses_mutable_or_mismatched_identity() {
    let root = std::env::temp_dir().join(format!("ds-dem-receipt-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).expect("root");
    let receipt = InstalledReceipt {
        schema: 1,
        contract: CONTRACT.to_string(),
        version: "latest".to_string(),
        mode: "range".to_string(),
        index_sha256: INDEX_SHA256.to_string(),
        coverage_sha256: COVERAGE_SHA256.to_string(),
        provenance_sha256: PROVENANCE_SHA256.to_string(),
        cog_size_bytes: COG_SIZE,
        cog_crc32c: format!("{COG_CRC32C:08x}"),
        cog_observed_sha256: None,
        installed_at: chrono::Utc::now().to_rfc3339(),
    };
    std::fs::write(
        root.join(RECEIPT_FILE),
        serde_json::to_vec(&receipt).expect("receipt"),
    )
    .expect("write receipt");
    let error = verify_metadata(&root).expect_err("mutable version must fail closed");
    assert!(error.contains("immutable catalog"));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn installation_lock_refuses_competing_native_hosts_and_releases_on_exit() {
    let root = tempfile::tempdir().unwrap();
    let first = installation_lock(root.path()).unwrap();
    assert!(installation_lock(root.path()).is_err());
    drop(first);
    assert!(installation_lock(root.path()).is_ok());
}

#[cfg(unix)]
#[test]
fn installation_lock_refuses_linked_paths() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("authored.txt");
    std::fs::write(&target, b"preserve").unwrap();
    std::os::unix::fs::symlink(&target, root.path().join("install.lock")).unwrap();
    assert!(installation_lock(root.path()).is_err());
    assert_eq!(std::fs::read(target).unwrap(), b"preserve");
}

#[test]
fn one_full_resource_retry_retains_the_second_refusal_and_does_not_loop() {
    let attempts = std::cell::Cell::new(0);
    let installs = std::cell::Cell::new(0);
    let outcome = retry_full_local(
        || {
            attempts.set(attempts.get() + 1);
            Ok("full_local_dem_required")
        },
        |outcome| *outcome == "full_local_dem_required",
        || {
            installs.set(installs.get() + 1);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(outcome, "full_local_dem_required");
    assert_eq!(attempts.get(), 2);
    assert_eq!(installs.get(), 1);
    let result = retry_full_local(
        || Ok("completed"),
        |_| false,
        || panic!("no resource acquisition for a completed operation"),
    )
    .unwrap();
    assert_eq!(result, "completed");
}
