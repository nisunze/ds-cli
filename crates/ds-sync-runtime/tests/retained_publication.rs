//! Native producer closure survives offline completion, restart and refusals.
use ds_sync_runtime::store::Fence;
use ds_sync_runtime::{
    fakes, open_store, retained_publication as retained, rows, seal::Recorded, Producer,
    SharedStore,
};
use ds_sync_store::PublicationBytes;

fn fence() -> Fence {
    Fence {
        account: "alice".into(),
        deployment: "https://gateway.example".into(),
        install_id: "install-a".into(),
    }
}
fn row(bytes: &[u8]) -> ds_sync_runtime::LocalRow {
    let mut row = fakes::local_row(bytes);
    row.input_base_fingerprint = Some("b".repeat(64));
    row.engine_build_manifest_sha256 = "a".repeat(64);
    row.bytes_locator = retained::locator(&row.client_publish_id);
    row
}
fn seal(
    store: &SharedStore,
    row: &ds_sync_runtime::LocalRow,
    bytes: &[u8],
) -> Result<Recorded, String> {
    retained::seal(
        store,
        &fence(),
        "project-a",
        row,
        rows::open_declaration("project-a", row),
        vec![PublicationBytes {
            output_id: row.outputs[0].output_id.clone(),
            bytes: bytes.to_vec(),
        }],
        &|old| {
            retained::RetainedProducer {
                store,
                fence: &fence(),
                project: "project-a",
            }
            .reclaim("project-a", &rows::local_row_from_store(old))
        },
    )
}
#[test]
fn offline_completion_survives_restart_and_supports_verified_local_download() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("queue.sqlite");
    let bytes = b"immutable native report";
    let row = row(bytes);
    let store = open_store(&path).unwrap();
    assert!(matches!(
        seal(&store, &row, bytes).unwrap(),
        Recorded::Recorded { .. }
    ));
    drop(store);
    let store = open_store(&path).unwrap();
    let scope = rows::store_scope("project-a");
    let snapshot = store.lock().unwrap().snapshot(&fence(), &scope).unwrap();
    assert_eq!(
        snapshot.artifacts[0].state,
        ds_sync_runtime::store::ArtifactState::Held
    );
    assert_eq!(snapshot.artifacts[0].replay_key, row.client_publish_id);
    let fence = fence();
    let producer = retained::RetainedProducer {
        store: &store,
        fence: &fence,
        project: "project-a",
    };
    assert_eq!(producer.output(&row, "xlsx").unwrap(), bytes);
    assert!(producer.inventory("project-a").unwrap().is_empty());
    let mut declaration = rows::open_declaration("project-a", &row);
    producer
        .amend_open_declaration("project-a", &mut declaration)
        .unwrap();
    assert_eq!(declaration, rows::open_declaration("project-a", &row));
    assert!(matches!(
        seal(&store, &row, bytes).unwrap(),
        Recorded::AlreadyRecorded { .. }
    ));
    let mut retry = row.clone();
    retry.produced_at_ms += 5000;
    retry.base_revision = Some("head-4".into());
    assert!(matches!(
        seal(&store, &retry, bytes).unwrap(),
        Recorded::AlreadyRecorded { .. }
    ));
}
#[test]
fn invalid_closure_never_records_bytes_and_replays_preserve_the_standing_publication() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(&dir.path().join("queue.sqlite")).unwrap();
    let original = row(b"original");
    assert!(seal(&store, &original, b"corrupted").is_err());
    assert!(store
        .lock()
        .unwrap()
        .snapshot(&fence(), &rows::store_scope("project-a"))
        .unwrap()
        .artifacts
        .is_empty());
    seal(&store, &original, b"original").unwrap();
    let mut crossed = original.clone();
    crossed.engine_build_manifest_sha256 = "f".repeat(64);
    assert!(seal(&store, &crossed, b"original").is_err());
    let mut equal = original.clone();
    equal.client_publish_id = "second-replay-key".into();
    equal.bytes_locator = retained::locator(&equal.client_publish_id);
    assert!(matches!(
        seal(&store, &equal, b"original").unwrap(),
        Recorded::AlreadyRecorded { .. }
    ));
    assert_eq!(
        retained::RetainedProducer {
            store: &store,
            fence: &fence(),
            project: "project-a"
        }
        .output(&original, "xlsx")
        .unwrap(),
        b"original"
    );
}
#[test]
fn account_project_output_and_declaration_substitution_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(&dir.path().join("queue.sqlite")).unwrap();
    let row = row(b"retained");
    seal(&store, &row, b"retained").unwrap();
    let mut other = fence();
    other.account = "bob".into();
    assert!(retained::RetainedProducer {
        store: &store,
        fence: &other,
        project: "project-a"
    }
    .output(&row, "xlsx")
    .is_err());
    assert!(retained::RetainedProducer {
        store: &store,
        fence: &fence(),
        project: "project-b"
    }
    .output(&row, "xlsx")
    .is_err());
    let fence = fence();
    let producer = retained::RetainedProducer {
        store: &store,
        fence: &fence,
        project: "project-a",
    };
    assert!(producer.output(&row, "unknown").is_err());
    let mut declaration = rows::open_declaration("project-a", &row);
    declaration["outputs"][0]["sha256"] = serde_json::json!("f".repeat(64));
    assert!(producer
        .amend_open_declaration("project-a", &mut declaration)
        .is_err());
    let mut moved = row.clone();
    moved.outputs[0].filename = Some("switched.xlsx".into());
    assert!(producer.output(&moved, "xlsx").is_err());
    assert!(producer
        .transfer(&row, "xlsx", "https://attacker.invalid/upload")
        .is_err());
}
#[test]
fn supersession_retires_only_old_derived_bytes_and_keeps_new_outputs_readable() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(&dir.path().join("queue.sqlite")).unwrap();
    let old = row(b"old report");
    seal(&store, &old, b"old report").unwrap();
    let mut new = row(b"new report");
    new.client_publish_id = "new-publication".into();
    new.bytes_locator = retained::locator(&new.client_publish_id);
    new.produced_at_ms += 1;
    assert!(matches!(
        seal(&store, &new, b"new report").unwrap(),
        Recorded::Recorded {
            superseded: Some((_, 10)),
            ..
        }
    ));
    let fence = fence();
    let producer = retained::RetainedProducer {
        store: &store,
        fence: &fence,
        project: "project-a",
    };
    assert!(producer.output(&old, "xlsx").is_err());
    assert_eq!(producer.output(&new, "xlsx").unwrap(), b"new report");
    assert_eq!(producer.reclaim("project-a", &old).unwrap(), 0);
    assert_eq!(producer.reclaim("project-a", &new).unwrap(), 10);
}

#[test]
fn solar_derived_outputs_use_the_same_offline_closure_and_verified_read_functions() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(&dir.path().join("solar.sqlite")).unwrap();
    let bytes = b"# Reviewed Solar draft";
    let mut row = row(bytes);
    row.identity.engine = "solar".into();
    row.identity.operation = "report-city-a".into();
    row.identity.variant = "apd-draft-en".into();
    row.engine_release = "ds-solar-engine@1.2.3".into();
    row.resource = "city-a".into();
    row.outputs[0].output_id = "apd-draft-en".into();
    row.outputs[0].format = "md".into();
    row.outputs[0].content_type = "text/markdown".into();
    row.outputs[0].filename = Some("apd-draft-en.md".into());
    row.sha256 =
        rows::inventory_digest(&mut [("apd-draft-en".into(), row.outputs[0].sha256.clone())]);
    assert!(matches!(
        seal(&store, &row, bytes).unwrap(),
        Recorded::Recorded { .. }
    ));
    let fence = fence();
    let producer = retained::RetainedProducer {
        store: &store,
        fence: &fence,
        project: "project-a",
    };
    assert_eq!(producer.output(&row, "apd-draft-en").unwrap(), bytes);
    let mut declaration = rows::open_declaration("project-a", &row);
    producer
        .amend_open_declaration("project-a", &mut declaration)
        .unwrap();
    assert_eq!(declaration["engine"], "solar");
}

#[test]
fn corrupted_retained_bytes_fail_verified_reads_before_a_transfer_can_start() {
    let dir = tempfile::tempdir().unwrap();
    let store = open_store(&dir.path().join("corrupt.sqlite")).unwrap();
    let row = row(b"retained immutable report");
    seal(&store, &row, b"retained immutable report").unwrap();
    store
        .lock()
        .unwrap()
        .connection_for_test()
        .execute(
            "UPDATE publication_outputs SET bytes=zeroblob(length(bytes))",
            [],
        )
        .unwrap();
    let fence = fence();
    let producer = retained::RetainedProducer {
        store: &store,
        fence: &fence,
        project: "project-a",
    };
    assert!(producer
        .output(&row, "xlsx")
        .unwrap_err()
        .contains("sealed digest"));
    assert!(producer
        .transfer(
            &row,
            "xlsx",
            "https://storage.googleapis.com/upload/storage/v1/b/fixture/o?upload_id=test"
        )
        .unwrap_err()
        .contains("sealed digest"));
    let snapshot = store
        .lock()
        .unwrap()
        .snapshot(&fence, &rows::store_scope("project-a"))
        .unwrap();
    assert_eq!(
        snapshot.artifacts[0].state,
        ds_sync_runtime::store::ArtifactState::Held
    );
    assert_eq!(snapshot.artifacts[0].sha256, row.sha256);
}

/// Server and Desktop run the same producer set: a retained Solar publication
/// is amended, never adopted, and freed by the set's Solar producer, whose
/// fence is derived from the host's captured identity.
#[test]
fn the_shared_producer_set_answers_for_retained_solar_bytes_on_every_host() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("store.sqlite");
    let store = open_store(&database).unwrap();
    let bytes = b"# Retained Solar draft";
    let mut row = row(bytes);
    row.identity.engine = "solar".into();
    row.identity.operation = "report-city-a".into();
    row.identity.variant = "apd-draft-en".into();
    row.engine_release = "ds-solar-engine@1.2.3".into();
    row.resource = "city-a".into();
    row.outputs[0].output_id = "apd-draft-en".into();
    row.outputs[0].format = "md".into();
    row.outputs[0].content_type = "text/markdown".into();
    row.outputs[0].filename = Some("apd-draft-en.md".into());
    row.sha256 =
        rows::inventory_digest(&mut [("apd-draft-en".into(), row.outputs[0].sha256.clone())]);
    seal(&store, &row, bytes).unwrap();
    let identity = ds_compute_runtime::HostIdentity {
        owner: "owner-a".into(),
        principal: ds_command_kernel::execution_context::Principal {
            uid: fence().account,
            lane: "canary".into(),
            deployment: fence().deployment,
            install_id: fence().install_id,
        },
    };
    assert_eq!(
        ds_sync_runtime::solar_producer::fence_of(&identity),
        fence()
    );
    let reports = dir.path().join("report-artifacts");
    let freed = ds_sync_runtime::with_producers(
        &database,
        &reports,
        &identity,
        "project-a",
        None,
        |producers| {
            assert!(producers.inventory("project-a")?.is_empty());
            let mut declaration = rows::open_declaration("project-a", &row);
            producers.amend_open_declaration("project-a", &mut declaration)?;
            assert_eq!(declaration["engine"], "solar");
            producers.reclaim("project-a", &row)
        },
    )
    .unwrap();
    assert_eq!(freed, bytes.len() as u64);
    let fence = fence();
    assert!(retained::RetainedProducer {
        store: &store,
        fence: &fence,
        project: "project-a",
    }
    .output(&row, "apd-draft-en")
    .is_err());
}

/// Every host restarts from one inventory read from the work itself; a
/// passive read never creates the store, and another fence's rows are not
/// this host's projects.
#[test]
fn one_publication_inventory_reads_the_work_and_never_creates_the_store() {
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("store.sqlite");
    let reports = dir.path().join("report-artifacts");
    assert!(
        ds_sync_runtime::publication_projects(&database, &reports, &fence(), None)
            .unwrap()
            .is_empty()
    );
    assert!(!database.exists(), "a passive inventory created the store");
    let store = open_store(&database).unwrap();
    seal(&store, &row(b"retained"), b"retained").unwrap();
    let mut other = fence();
    other.account = "bob".into();
    assert_eq!(
        ds_sync_runtime::publication_projects(&database, &reports, &fence(), None).unwrap(),
        std::collections::BTreeSet::from(["project-a".to_string()])
    );
    assert!(
        ds_sync_runtime::publication_projects(&database, &reports, &other, None)
            .unwrap()
            .is_empty()
    );
}

/// Every host answers the kernel's recovery control with the same function:
/// status reads without creating the store, and an unknown row is refused
/// rather than guessed.
#[test]
fn one_recovery_execution_reads_without_creating_and_refuses_unknown_rows() {
    use ds_sync_runtime::store::recovery::{Command, Control, SCHEMA};
    let dir = tempfile::tempdir().unwrap();
    let database = dir.path().join("store.sqlite");
    let control = |command| Control {
        schema: SCHEMA.into(),
        project: "project-a".into(),
        command,
    };
    let status = ds_sync_runtime::recover(
        &database,
        &fence(),
        control(Command::Status { limit: 50 }),
        1,
    )
    .unwrap();
    assert_eq!(status.total, 0);
    assert!(!database.exists(), "a status read created the store");
    let store = open_store(&database).unwrap();
    let held = row(b"retained");
    seal(&store, &held, b"retained").unwrap();
    drop(store);
    let status = ds_sync_runtime::recover(
        &database,
        &fence(),
        control(Command::Status { limit: 50 }),
        2,
    )
    .unwrap();
    assert_eq!(status.total, 1);
    let unknown = ds_sync_runtime::recover(
        &database,
        &fence(),
        control(Command::Retry {
            row: "no-such-row".into(),
        }),
        3,
    )
    .unwrap();
    assert!(!unknown.applied);
    assert_eq!(unknown.refusals[0].code, "sync_row_not_found");
    assert!(ds_sync_runtime::recover(
        &database,
        &fence(),
        control(Command::Status { limit: 0 }),
        4
    )
    .is_err());
}

#[test]
fn explicit_artifact_verification_streams_exact_bytes_without_mutating_work() {
    use ds_sync_runtime::store::recovery::{Command, Control, SCHEMA};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("queue.sqlite");
    let bytes = vec![42u8; 2 * 65536 + 17];
    let row = row(&bytes);
    let store = open_store(&path).unwrap();
    seal(&store, &row, &bytes).unwrap();
    let receipt = serde_json::from_value(serde_json::json!({
        "receipt_id":"old-heartbeat-refusal", "scope":rows::store_scope("project-a"),
        "action":"upload", "identity":row.identity, "outcome":"refused",
        "detail":"old untyped refusal", "client_publish_id":row.client_publish_id, "at_ms":2
    }))
    .unwrap();
    store
        .lock()
        .unwrap()
        .apply(
            &fence(),
            2,
            ds_sync_runtime::store::Event::ActionReceipt { receipt },
        )
        .unwrap();
    let command = || Control {
        schema: SCHEMA.into(),
        project: "project-a".into(),
        command: Command::VerifyArtifacts {
            row: row.client_publish_id.clone(),
        },
    };
    let snapshot = serde_json::to_value(
        store
            .lock()
            .unwrap()
            .snapshot(&fence(), &rows::store_scope("project-a"))
            .unwrap(),
    )
    .unwrap();
    drop(store);
    let answer = ds_sync_runtime::recover(&path, &fence(), command(), 5).unwrap();
    assert!(answer.refusals.is_empty());
    assert!(!answer.applied);
    assert!(answer.changes.is_empty());
    let verified = answer.artifact_verification.unwrap();
    assert_eq!(verified.status, "verified");
    assert_eq!(verified.output_count, 1);
    assert_eq!(verified.verified_bytes, bytes.len() as u64);
    assert_eq!(verified.outputs[0].sha256, row.outputs[0].sha256);
    let store = open_store(&path).unwrap();
    assert_eq!(
        snapshot,
        serde_json::to_value(
            store
                .lock()
                .unwrap()
                .snapshot(&fence(), &rows::store_scope("project-a"))
                .unwrap()
        )
        .unwrap()
    );
    let mut other = fence();
    other.account = "other".into();
    let crossed = ds_sync_runtime::recover(&path, &other, command(), 5).unwrap();
    assert_eq!(crossed.refusals[0].code, "sync_row_not_found");
    assert!(crossed.artifact_verification.is_none());
    store
        .lock()
        .unwrap()
        .connection_for_test()
        .execute(
            "UPDATE publication_outputs SET bytes = zeroblob(length(bytes))",
            [],
        )
        .unwrap();
    let ordinary = ds_sync_runtime::recover(
        &path,
        &fence(),
        Control {
            schema: SCHEMA.into(),
            project: "project-a".into(),
            command: Command::Status { limit: 50 },
        },
        6,
    )
    .unwrap();
    assert!(ordinary.artifact_verification.is_none());
    assert!(ordinary.refusals.is_empty());
    let corrupted = ds_sync_runtime::recover(&path, &fence(), command(), 6).unwrap();
    assert_eq!(
        corrupted.artifact_verification.unwrap().status,
        "integrity_mismatch"
    );
    store
        .lock()
        .unwrap()
        .retire_publication_payload(&fence(), "project-a", &row.client_publish_id)
        .unwrap();
    let missing = ds_sync_runtime::recover(&path, &fence(), command(), 7).unwrap();
    assert_eq!(
        missing.artifact_verification.unwrap().status,
        "not_retained"
    );
    assert!(!missing.applied);
}

#[test]
fn artifact_verification_never_creates_an_absent_store() {
    use ds_sync_runtime::store::recovery::{Command, Control, SCHEMA};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("absent.sqlite");
    let result = ds_sync_runtime::recover(
        &path,
        &fence(),
        Control {
            schema: SCHEMA.into(),
            project: "project-a".into(),
            command: Command::VerifyArtifacts {
                row: "publication".into(),
            },
        },
        1,
    )
    .unwrap();
    assert_eq!(result.refusals[0].code, "sync_row_not_found");
    assert!(!path.exists());
}
