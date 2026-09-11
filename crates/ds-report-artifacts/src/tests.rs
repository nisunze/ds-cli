use super::*;
use std::collections::HashSet;
struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        // macOS commonly spells the temp root through `/var`, which is a
        // symlink to `/private/var`. Canonicalize the harness root so the
        // tests exercise reporter-created links, not a platform alias.
        let temp_root =
            std::fs::canonicalize(std::env::temp_dir()).expect("resolve platform test directory");
        let path = temp_root.join(format!(
            "ds-web-network-reporter-{label}-{}-{:016x}",
            std::process::id(),
            rand::random::<u64>()
        ));
        std::fs::create_dir_all(&path).expect("create test directory");
        Self(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn locator(byte: u8) -> String {
    format!("{LOCATOR_PREFIX}{}", format!("{byte:02x}").repeat(32))
}

fn write_fixture(root: &Path, locator: &str, bytes: &[u8]) -> PathBuf {
    let batch_id = locator_id(locator).expect("valid locator").to_string();
    let sha256 = format!("{:x}", Sha256::digest(bytes));
    let entry = root.join(locator_id(locator).expect("valid locator"));
    std::fs::create_dir_all(&entry).expect("create artifact entry");
    std::fs::write(entry.join(ARTIFACT_FILE), bytes).expect("write artifact");
    let receipt = NetworkReporterArtifactReceipt {
        schema: RECEIPT_SCHEMA,
        batch_id: batch_id.clone(),
        output_id: "shp".to_string(),
        locator: locator.to_string(),
        artifact_file: ARTIFACT_FILE.to_string(),
        size_bytes: bytes.len() as u64,
        sha256: sha256.clone(),
    };
    let mut receipt_bytes = serde_json::to_vec_pretty(&receipt).expect("encode receipt");
    receipt_bytes.push(b'\n');
    std::fs::write(entry.join(RECEIPT_FILE), receipt_bytes).expect("write receipt");
    let pending = HeldDirectory::ensure_absolute(root).expect("hold pending root");
    PendingPublicationCommit::stage(
        &pending,
        &PublicationBatchReceipt {
            schema: 1,
            batch_id,
            owner_uid: "owner-1".to_string(),
            organization_id: Some("org-1".to_string()),
            project_id: "project-1".to_string(),
            root: "eds_project/project-1/eds_design".to_string(),
            client_run_id: "report-0123456789abcdef0123456789abcdef".to_string(),
            engine: "network_reporter".to_string(),
            operation: "export-tx_1".to_string(),
            variant: "default".to_string(),
            engine_version: format!("ds-network-reporter@0.1.0+{}", "a".repeat(40)),
            engine_build_manifest_sha256: String::new(),
            transformer: "tx_1".to_string(),
            transformer_revision: 7,
            input_base_fingerprint: "b".repeat(64),
            room_content_sha256: "c".repeat(64),
            grant_id: Some(format!("grant_{}", "d".repeat(40))),
            snapshot_base: BTreeMap::from([("export-tx_1".to_string(), "b".repeat(64))]),
            client_publish_id: "publish-123e4567-e89b-42d3-a456-426614174000".to_string(),
            outputs: vec![PublicationOutput {
                output_id: "shp".to_string(),
                locator: locator.to_string(),
                filename: "tx_1.shp.zip".to_string(),
                format: "shp".to_string(),
                content_type: "application/zip".to_string(),
                sha256,
                size_bytes: bytes.len() as u64,
                paper_size: None,
                presentation: None,
            }],
        },
    )
    .expect("stage publication receipt")
    .commit()
    .expect("commit publication receipt");
    entry
}

fn read_artifact_from_root(root: &Path, locator: &str, max_bytes: u64) -> Result<Vec<u8>, String> {
    read_artifact_from_root_with_gate(root, locator, max_bytes, |_| {
        Ok(Instant::now() + Duration::from_secs(60))
    })
}
fn discard_artifact_from_root(root: &Path, locator: &str, digest: &str) -> Result<(), String> {
    discard_artifact_from_root_with_gate(root, locator, digest, |_| {
        Ok(Instant::now() + Duration::from_secs(60))
    })
}
fn list_local_artifacts_from_root(
    root: &Path,
    owner: &str,
    project: &str,
    transformer: &str,
) -> Result<Vec<NetworkReporterLocalArtifactSet>, String> {
    list_project_local_artifacts_from_root(root, owner, project, Some(transformer))
}
#[test]
fn locator_contract_rejects_paths_uppercase_and_wrong_lengths() {
    assert!(locator_id(&locator(0xab)).is_ok());
    assert!(locator_id("../../outside").is_err());
    assert!(locator_id(&format!("{LOCATOR_PREFIX}{}", "A".repeat(64))).is_err());
    assert!(locator_id(&format!("{LOCATOR_PREFIX}{}", "a".repeat(63))).is_err());
    assert!(locator_id(&format!("{LOCATOR_PREFIX}{}/x", "a".repeat(64))).is_err());
}

#[test]
fn read_requires_receipt_bound_size_and_digest() {
    let test = TestDirectory::new("read");
    let root = test.0.join("pending-upload");
    let locator = locator(0x11);
    write_fixture(&root, &locator, b"report bytes");

    assert_eq!(
        read_artifact_from_root(&root, &locator, 1024).expect("read verified artifact"),
        b"report bytes"
    );
    assert!(read_artifact_from_root(&root, &locator, 4)
        .expect_err("bounded read must fail")
        .contains("requested"));

    std::fs::write(
        root.join(locator_id(&locator).unwrap()).join(ARTIFACT_FILE),
        b"other bytes!",
    )
    .expect("mutate artifact");
    assert!(read_artifact_from_root(&root, &locator, 1024)
        .expect_err("digest mismatch must fail")
        .contains("digest"));
}

#[test]
fn recovery_candidates_disclose_only_opaque_ids_for_exact_owner() {
    let test = TestDirectory::new("recovery-candidates-owner");
    let root = test.0.join("pending-upload");
    let locator = locator(0x1d);
    write_fixture(&root, &locator, b"recoverable report");
    let batch_id = locator_id(&locator).unwrap();
    let candidates = list_recovery_candidates_from_root(&root, "owner-1").unwrap();
    assert_eq!(candidates.len(), 1);
    assert_eq!(candidates[0].batch_id, batch_id);
    let encoded = serde_json::to_value(&candidates).unwrap();
    assert_eq!(
        encoded[0]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<HashSet<_>>(),
        HashSet::from(["batchId".to_string()])
    );

    assert!(list_recovery_candidates_from_root(&root, "owner-2")
        .expect("foreign owner receipts are silently excluded")
        .is_empty());
}

#[test]
fn committed_batch_recovery_rehashes_without_exposing_paths() {
    let test = TestDirectory::new("recover-committed");
    let root = test.0.join("pending-upload");
    let locator = locator(0x18);
    let bytes = b"recoverable report bytes";
    write_fixture(&root, &locator, bytes);
    let batch_id = locator_id(&locator).unwrap();
    assert!(
        recover_committed_publication_from_root(&root, "owner-2", batch_id,)
            .expect_err("recovery evidence must stay owner-scoped")
            .contains("owner")
    );

    let recovered = recover_committed_publication_from_root(&root, "owner-1", batch_id)
        .expect("recover committed publication")
        .expect("committed publication exists");
    let replay = recover_committed_publication_from_root(&root, "owner-1", batch_id)
        .expect("replay committed recovery")
        .expect("committed publication remains available");
    assert_eq!(replay, recovered);
    assert_eq!(recovered.batch_id, batch_id);
    assert_eq!(recovered.owner_uid, "owner-1");
    assert_eq!(recovered.project_id, "project-1");
    assert_eq!(recovered.artifacts.len(), 1);
    assert_eq!(recovered.artifacts[0].locator, locator);
    assert_eq!(recovered.artifacts[0].size_bytes, bytes.len() as u64);

    let encoded = serde_json::to_value(&recovered).expect("serialize path-free recovery");
    let text = serde_json::to_string(&encoded).unwrap();
    assert!(!text.contains(root.to_string_lossy().as_ref()));
    assert_eq!(
        encoded
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<HashSet<_>>(),
        HashSet::from([
            "batchId".to_string(),
            "ownerUid".to_string(),
            "organizationId".to_string(),
            "projectId".to_string(),
            "root".to_string(),
            "clientRunId".to_string(),
            "engine".to_string(),
            "operation".to_string(),
            "variant".to_string(),
            "engineVersion".to_string(),
            "engineBuildManifestSha256".to_string(),
            "transformer".to_string(),
            "transformerRevision".to_string(),
            "inputBaseFingerprint".to_string(),
            "roomContentSha256".to_string(),
            "snapshotBase".to_string(),
            "clientPublishId".to_string(),
            "artifacts".to_string(),
        ])
    );
    assert_eq!(encoded["artifacts"][0]["filename"], "tx_1.shp.zip");
    assert_eq!(
        encoded["artifacts"][0]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<HashSet<_>>(),
        HashSet::from([
            "outputId".to_string(),
            "locator".to_string(),
            "filename".to_string(),
            "format".to_string(),
            "contentType".to_string(),
            "sha256".to_string(),
            "sizeBytes".to_string(),
        ])
    );
}

#[test]
fn committed_batch_recovery_fails_closed_on_missing_or_changed_bytes() {
    let test = TestDirectory::new("recover-tamper");
    let root = test.0.join("pending-upload");
    let locator = locator(0x19);
    let entry = write_fixture(&root, &locator, b"original report");
    let batch_id = locator_id(&locator).unwrap();
    std::fs::write(entry.join(ARTIFACT_FILE), b"tampered report").unwrap();
    assert!(
        recover_committed_publication_from_root(&root, "owner-1", batch_id)
            .expect_err("changed bytes must fail recovery")
            .contains("digest")
    );
    assert!(
        recover_committed_publication_from_root(&root, "owner-1", &"f".repeat(64))
            .expect("unknown batch lookup")
            .is_none()
    );
}

#[cfg(unix)]
#[test]
fn committed_artifact_read_does_not_wait_for_an_unrelated_pending_upload_mutation() {
    let test = TestDirectory::new("read-live-mutation");
    let root = test.0.join("pending-upload");
    let locator = locator(0x1a);
    write_fixture(&root, &locator, b"committed report");
    let held = HeldDirectory::open_absolute(&root).unwrap().unwrap();
    let live_mutation = held
        .try_lock_file(
            std::ffi::OsStr::new(PENDING_UPLOAD_LOCK_FILE),
            "Network Reporter pending-upload lock",
        )
        .unwrap();

    assert_eq!(
        read_artifact_from_root(&root, &locator, 1024).unwrap(),
        b"committed report"
    );
    drop(live_mutation);
    assert_eq!(
        read_artifact_from_root(&root, &locator, 1024).unwrap(),
        b"committed report"
    );
}

#[cfg(unix)]
#[test]
fn committed_artifact_inventory_does_not_wait_for_an_unrelated_pending_upload_mutation() {
    let test = TestDirectory::new("list-live-mutation");
    let root = test.0.join("pending-upload");
    write_fixture(&root, &locator(0x1d), b"committed report");
    let held = HeldDirectory::open_absolute(&root).unwrap().unwrap();
    let live_mutation = held
        .try_lock_file(
            std::ffi::OsStr::new(PENDING_UPLOAD_LOCK_FILE),
            "Network Reporter pending-upload lock",
        )
        .unwrap();

    let sets = list_local_artifacts_from_root(&root, "owner-1", "project-1", "tx_1")
        .expect("committed inventory remains readable during publication");
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0].artifacts.len(), 1);
    assert_eq!(sets[0].artifacts[0].filename, "tx_1.shp.zip");
    let project =
        list_project_local_artifacts_from_root(&root, "owner-1", "project-1", None).unwrap();
    assert_eq!(project.len(), 1);
    assert_eq!(project[0].transformer, "tx_1");
    assert!(
        list_project_local_artifacts_from_root(&root, "other-owner", "project-1", None)
            .unwrap()
            .is_empty()
    );
    assert!(
        list_project_local_artifacts_from_root(&root, "owner-1", "other-project", None)
            .unwrap()
            .is_empty()
    );
    drop(live_mutation);
}

#[test]
fn locked_recovery_reaps_only_precommit_reporter_state() {
    let test = TestDirectory::new("reap-precommit");
    let root = test.0.join("pending-upload");
    let committed_locator = locator(0x1b);
    write_fixture(&root, &committed_locator, b"committed report");

    let abandoned_locator = locator(0x1c);
    let abandoned_id = locator_id(&abandoned_locator).unwrap();
    let abandoned = root.join(abandoned_id);
    std::fs::create_dir(&abandoned).unwrap();
    let abandoned_bytes = b"uncommitted report";
    std::fs::write(abandoned.join(ARTIFACT_FILE), abandoned_bytes).unwrap();
    let abandoned_receipt = NetworkReporterArtifactReceipt {
        schema: RECEIPT_SCHEMA,
        batch_id: "9".repeat(64),
        output_id: "shp".to_string(),
        locator: abandoned_locator,
        artifact_file: ARTIFACT_FILE.to_string(),
        size_bytes: abandoned_bytes.len() as u64,
        sha256: format!("{:x}", Sha256::digest(abandoned_bytes)),
    };
    std::fs::write(
        abandoned.join(RECEIPT_FILE),
        serde_json::to_vec(&abandoned_receipt).unwrap(),
    )
    .unwrap();

    let promotion_id = "8".repeat(64);
    let promoting = root.join(format!(".promoting-{promotion_id}"));
    std::fs::create_dir(&promoting).unwrap();
    std::fs::write(promoting.join("partial.bin"), b"partial").unwrap();
    let staged_batch_id = "7".repeat(64);
    let staged_batch = root
        .join(BATCH_DIRECTORY)
        .join(format!(".staging-{staged_batch_id}"));
    std::fs::create_dir(&staged_batch).unwrap();
    std::fs::write(staged_batch.join("partial.json"), b"partial").unwrap();

    let held = HeldDirectory::open_absolute(&root).unwrap().unwrap();
    let lock = held
        .try_lock_file(
            std::ffi::OsStr::new(PENDING_UPLOAD_LOCK_FILE),
            "Network Reporter pending-upload lock",
        )
        .unwrap();
    assert_eq!(
        sweep_abandoned_pending_upload_locked(&held, &lock)
            .unwrap()
            .0,
        2
    );
    assert!(!abandoned.exists());
    assert!(!promoting.exists());
    assert!(!staged_batch.exists());
    assert!(root.join(locator_id(&committed_locator).unwrap()).is_dir());
    assert!(root.join(PENDING_UPLOAD_LOCK_FILE).is_file());
    drop(lock);
    assert_eq!(
        read_artifact_from_root(&root, &committed_locator, 1024).unwrap(),
        b"committed report"
    );
}

#[test]
fn receipt_must_bind_the_exact_locator() {
    let test = TestDirectory::new("receipt-binding");
    let root = test.0.join("pending-upload");
    let requested = locator(0x22);
    let entry = write_fixture(&root, &requested, b"artifact");
    let receipt = NetworkReporterArtifactReceipt {
        schema: RECEIPT_SCHEMA,
        batch_id: locator_id(&requested).unwrap().to_string(),
        output_id: "shp".to_string(),
        locator: locator(0x23),
        artifact_file: ARTIFACT_FILE.to_string(),
        size_bytes: 8,
        sha256: format!("{:x}", Sha256::digest(b"artifact")),
    };
    std::fs::write(
        entry.join(RECEIPT_FILE),
        serde_json::to_vec(&receipt).unwrap(),
    )
    .unwrap();

    assert!(read_artifact_from_root(&root, &requested, 1024)
        .expect_err("foreign receipt must fail")
        .contains("does not match"));
}

#[test]
fn read_rejects_undeclared_entry_members() {
    let test = TestDirectory::new("read-undeclared-member");
    let root = test.0.join("pending-upload");
    let locator = locator(0x24);
    let entry = write_fixture(&root, &locator, b"artifact");
    std::fs::write(entry.join("unowned.txt"), b"not part of the receipt").unwrap();

    assert!(read_artifact_from_root(&root, &locator, 1024)
        .expect_err("undeclared members must fail the readable-state invariant")
        .contains("unowned member"));
}

#[cfg(unix)]
#[test]
fn read_rejects_symlinked_entry_and_artifact() {
    use std::os::unix::fs::symlink;

    let test = TestDirectory::new("symlink");
    let root = test.0.join("pending-upload");
    std::fs::create_dir_all(&root).unwrap();
    let outside = test.0.join("outside");
    let linked_locator = locator(0x31);
    write_fixture(&outside, &linked_locator, b"outside");
    symlink(
        outside.join(locator_id(&linked_locator).unwrap()),
        root.join(locator_id(&linked_locator).unwrap()),
    )
    .unwrap();
    assert!(read_artifact_from_root(&root, &linked_locator, 1024)
        .expect_err("linked entry must fail")
        .contains("linked"));

    let artifact_locator = locator(0x32);
    let entry = write_fixture(&root, &artifact_locator, b"inside");
    std::fs::remove_file(entry.join(ARTIFACT_FILE)).unwrap();
    let external_file = test.0.join("external.bin");
    std::fs::write(&external_file, b"inside").unwrap();
    symlink(external_file, entry.join(ARTIFACT_FILE)).unwrap();
    // The held-directory child sweep rejects the link before the artifact
    // open's own "regular file" guard can fire; either layer failing closed
    // on a link-specific error satisfies this test's property.
    let linked_artifact_error = read_artifact_from_root(&root, &artifact_locator, 1024)
        .expect_err("linked artifact must fail");
    assert!(
        linked_artifact_error.contains("linked") || linked_artifact_error.contains("regular file"),
        "unexpected linked-artifact error: {linked_artifact_error}"
    );
}

#[cfg(unix)]
#[test]
fn read_rejects_a_symlinked_root_ancestor() {
    use std::os::unix::fs::symlink;

    let test = TestDirectory::new("symlinked-root");
    let real_root = test.0.join("real-pending-upload");
    let locator = locator(0x33);
    write_fixture(&real_root, &locator, b"outside through root link");
    let linked_root = test.0.join("pending-upload");
    symlink(&real_root, &linked_root).unwrap();

    assert!(read_artifact_from_root(&linked_root, &locator, 1024)
        .expect_err("linked root must fail confinement")
        .contains("linked"));
}

#[test]
fn discard_rejects_expected_digest_mismatch_without_mutation() {
    let test = TestDirectory::new("discard-expected-mismatch");
    let root = test.0.join("pending-upload");
    let locator = locator(0x41);
    let entry = write_fixture(&root, &locator, b"published artifact");

    let error = discard_artifact_from_root(&root, &locator, &"0".repeat(64))
        .expect_err("wrong publication digest must fail");
    assert!(error.contains("does not match"));
    assert!(entry.join(ARTIFACT_FILE).is_file());
    assert!(entry.join(RECEIPT_FILE).is_file());
    assert!(!entry.join(DISCARDING_FILE).exists());
}

#[test]
fn discard_rejects_changed_bytes_and_restores_the_artifact_name() {
    let test = TestDirectory::new("discard-bytes-mismatch");
    let root = test.0.join("pending-upload");
    let locator = locator(0x42);
    let entry = write_fixture(&root, &locator, b"original");
    let expected = format!("{:x}", Sha256::digest(b"original"));
    std::fs::write(entry.join(ARTIFACT_FILE), b"tampered").unwrap();

    assert!(discard_artifact_from_root(&root, &locator, &expected)
        .expect_err("changed artifact must fail")
        .contains("digest"));
    assert!(entry.join(ARTIFACT_FILE).is_file());
    assert!(!entry.join(DISCARDING_FILE).exists());
    assert!(entry.join(RECEIPT_FILE).is_file());
}

#[test]
fn discard_is_digest_checked_and_idempotent() {
    let test = TestDirectory::new("discard-success");
    let root = test.0.join("pending-upload");
    let locator = locator(0x43);
    let bytes = b"published artifact";
    let entry = write_fixture(&root, &locator, bytes);
    let expected = format!("{:x}", Sha256::digest(bytes));

    discard_artifact_from_root(&root, &locator, &expected).expect("discard verified artifact");
    assert!(!entry.exists());
    discard_artifact_from_root(&root, &locator, &expected)
        .expect("repeated discard is already complete");
}

#[test]
fn discard_resumes_after_artifact_was_already_removed() {
    let test = TestDirectory::new("discard-resume");
    let root = test.0.join("pending-upload");
    let locator = locator(0x44);
    let bytes = b"published artifact";
    let entry = write_fixture(&root, &locator, bytes);
    let expected = format!("{:x}", Sha256::digest(bytes));
    std::fs::remove_file(entry.join(ARTIFACT_FILE)).unwrap();

    discard_artifact_from_root(&root, &locator, &expected)
        .expect("receipt-authorized retry completes partial discard");
    assert!(!entry.exists());
}

#[test]
fn nonregular_artifact_is_rejected_without_deletion() {
    let test = TestDirectory::new("nonregular");
    let root = test.0.join("pending-upload");
    let locator = locator(0x45);
    let entry = write_fixture(&root, &locator, b"artifact");
    std::fs::remove_file(entry.join(ARTIFACT_FILE)).unwrap();
    std::fs::create_dir(entry.join(ARTIFACT_FILE)).unwrap();
    let expected = format!("{:x}", Sha256::digest(b"artifact"));

    assert!(discard_artifact_from_root(&root, &locator, &expected)
        .expect_err("directory artifact must fail")
        .contains("regular file"));
    assert!(entry.join(RECEIPT_FILE).is_file());
}

#[test]
fn receipt_reader_rejects_oversized_files_before_parsing() {
    let test = TestDirectory::new("oversized-receipt");
    let root = test.0.join("pending-upload");
    let locator = locator(0x46);
    let entry = write_fixture(&root, &locator, b"artifact");
    let mut receipt = File::create(entry.join(RECEIPT_FILE)).unwrap();
    receipt
        .write_all(&vec![b'x'; (MAX_RECEIPT_BYTES + 1) as usize])
        .unwrap();

    assert!(read_artifact_from_root(&root, &locator, 1024)
        .expect_err("oversized receipt must fail")
        .contains("exceeds"));
}

#[test]
fn streamed_recovery_is_independent_of_the_preview_memory_limit() {
    let test = TestDirectory::new("large-streamed-artifact");
    let path = test.0.join("large-report");
    let mut file = File::create(&path).unwrap();
    let chunk = vec![0x5a; 1024 * 1024];
    let mut hash = Sha256::new();
    let size = HARD_ARTIFACT_LIMIT_BYTES + chunk.len() as u64;
    for _ in 0..(size / chunk.len() as u64) {
        file.write_all(&chunk).unwrap();
        hash.update(&chunk);
    }
    file.sync_all().unwrap();
    let digest = format!("{:x}", hash.finalize());
    let mut file = File::open(&path).unwrap();
    assert!(verify_open_artifact(&mut file, size, &digest, false)
        .unwrap()
        .is_empty());
    let mut file = File::open(&path).unwrap();
    assert!(verify_open_artifact(&mut file, size, &"0".repeat(64), false).is_err());
}
