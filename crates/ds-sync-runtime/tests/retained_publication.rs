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
