use ds_command_kernel::{
    sync::engines::ENGINES,
    sync_store::{
        self,
        recovery::{self, Command},
        ArtifactRow, ArtifactState, Event, Fence, ReceiptRow, Scope,
    },
};
use ds_sync_store::{PublicationBytes, PublicationPayload, Store};
use serde_json::json;

const SHA: &str = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
fn fence() -> Fence {
    Fence {
        account: "owner".into(),
        deployment: "canary".into(),
        install_id: "install".into(),
    }
}
fn scope() -> Scope {
    Scope::Project {
        project: "project".into(),
    }
}
fn row(engine: &ds_command_kernel::sync::engines::Engine, key: &str, ids: &[&str]) -> ArtifactRow {
    serde_json::from_value(json!({
        "scope": scope(), "engine": engine.name, "operation": "operation", "variant": "default",
        "sha256": sync_store::inventory_digest(&ids.iter().map(|id| (id.to_string(), SHA.to_string())).collect::<Vec<_>>()),
        "size_bytes": 4 * ids.len(), "produced_at_ms": 1, "input_base_fingerprint": "a".repeat(64),
        "engine_release": format!("{}0.1.0{}",engine.release_prefix,if engine.requires_release_commit {format!("+{}","b".repeat(40))} else {String::new()}),
        "engine_build_manifest_sha256": "c".repeat(64), "client_publish_id": key,
        "outputs": ids.iter().map(|id| json!({"output_id": id, "sha256": SHA, "size_bytes": 4})).collect::<Vec<_>>(),
        "bytes_locator": format!("private-local-path:{key}"), "state": "held", "replay_key": key, "updated_at_ms": 1
    })).unwrap()
}
fn seal(store: &mut Store, row: ArtifactRow) {
    let payload = PublicationPayload {
        receipt: format!("immutable closure {}", row.replay_key),
        outputs: row
            .outputs
            .iter()
            .map(|o| PublicationBytes {
                output_id: o.output_id.clone(),
                bytes: b"test".to_vec(),
            })
            .collect(),
    };
    assert!(store
        .seal_publication_payload(&fence(), 1, row, &payload)
        .unwrap()
        .refusals
        .is_empty());
}
fn attempt(row: &ArtifactRow, outcome: &str, code: Option<&str>, detail: &str) -> ReceiptRow {
    serde_json::from_value(json!({"receipt_id":format!("attempt-{}-{outcome}",row.replay_key),"scope":scope(),"action":"upload","identity":row.identity,"outcome":outcome,"detail":detail,"failure_code":code,"client_publish_id":row.client_publish_id,"at_ms":2})).unwrap()
}
fn settle(store: &mut Store, row: &ArtifactRow, outcome: &str, code: Option<&str>, detail: &str) {
    assert!(store
        .apply(
            &fence(),
            2,
            Event::ActionReceipt {
                receipt: attempt(row, outcome, code, detail)
            }
        )
        .unwrap()
        .refusals
        .is_empty());
}

#[test]
fn exact_retry_survives_restart_keeps_bytes_and_refuses_crossed_fences_and_live_workers() {
    for engine in ENGINES {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("sync.sqlite");
        let mut store = Store::open(&database).unwrap();
        let original = row(engine, "publication", &["one"]);
        seal(&mut store, original.clone());
        settle(
            &mut store,
            &original,
            "refused",
            Some("ENGINE_BUILD_NOT_ADMITTED"),
            "build not admitted",
        );
        drop(store);
        let mut store = Store::open(&database).unwrap();
        let preview = store
            .recovery(&fence(), &scope(), 3, Command::Status { limit: 50 })
            .unwrap();
        assert!(preview.rows[0].retry_eligible);
        let queue = store.queue(&fence(), Some("project"), 3).unwrap();
        assert_eq!(queue.held_batches, 1);
        assert_eq!(queue.reclaimable_batches, 0);
        assert!(!serde_json::to_string(&preview)
            .unwrap()
            .contains("private-local-path"));
        let mut other = fence();
        other.account = "other".into();
        let crossed = store
            .recovery(
                &other,
                &scope(),
                3,
                Command::Retry {
                    row: "publication".into(),
                },
            )
            .unwrap();
        assert_eq!(crossed.refusals[0].code, "sync_row_not_found");
        store
            .apply(
                &fence(),
                3,
                Event::LeaseTake {
                    scope: scope(),
                    worker_id: "worker".into(),
                    ttl_ms: 10,
                },
            )
            .unwrap();
        let locked = store
            .recovery(
                &fence(),
                &scope(),
                4,
                Command::Retry {
                    row: "publication".into(),
                },
            )
            .unwrap();
        assert_eq!(locked.refusals[0].code, "sync_worker_active");
        let result = store
            .recovery(
                &fence(),
                &scope(),
                14,
                Command::Retry {
                    row: "publication".into(),
                },
            )
            .unwrap();
        assert!(result.applied);
        let after = store
            .artifact(&fence(), &scope(), "publication")
            .unwrap()
            .unwrap();
        assert_eq!(after.state, ArtifactState::Held);
        assert_eq!(after.replay_key, original.replay_key);
        assert_eq!(after.client_publish_id, original.client_publish_id);
        assert_eq!(after.outputs, original.outputs);
        assert_eq!(after.engine_release, original.engine_release);
        assert_eq!(
            after.engine_build_manifest_sha256,
            original.engine_build_manifest_sha256
        );
        assert_eq!(
            after.input_base_fingerprint,
            original.input_base_fingerprint
        );
        assert_eq!(
            store
                .publication_output(&fence(), "project", "publication", "one")
                .unwrap(),
            Some(b"test".to_vec())
        );
        let repeated = store
            .recovery(
                &fence(),
                &scope(),
                15,
                Command::Retry {
                    row: "publication".into(),
                },
            )
            .unwrap();
        assert_eq!(repeated.refusals[0].code, "sync_row_not_retryable");
    }
}

#[test]
fn sanitation_pins_the_complete_snapshot_and_archives_only_complete_supersession_without_deleting_bytes(
) {
    for engine in ENGINES {
        let dir = tempfile::tempdir().unwrap();
        let database = dir.path().join("sync.sqlite");
        let mut store = Store::open(&database).unwrap();
        let old = row(engine, "old", &["one", "two"]);
        seal(&mut store, old.clone());
        settle(
            &mut store,
            &old,
            "conflict",
            Some("CONFLICT"),
            "source input moved",
        );
        let partial = row(engine, "partial", &["one"]);
        seal(&mut store, partial);
        let before = store
            .recovery(&fence(), &scope(), 3, Command::SanitizePreview { limit: 1 })
            .unwrap();
        assert!(before.more);
        assert_eq!(before.total, 2);
        assert_eq!(before.rows[0].action, "keep");
        let complete = row(engine, "complete", &["one", "two"]);
        seal(&mut store, complete);
        let changed = store
            .recovery(
                &fence(),
                &scope(),
                4,
                Command::SanitizeApply {
                    digest: before.digest,
                },
            )
            .unwrap();
        assert_eq!(changed.refusals[0].code, "sync_sanitation_refused");
        assert!(
            !store
                .artifact(&fence(), &scope(), "old")
                .unwrap()
                .unwrap()
                .archived
        );
        let preview = store
            .recovery(&fence(), &scope(), 4, Command::SanitizePreview { limit: 1 })
            .unwrap();
        assert_eq!(preview.total, 3);
        let applied = store
            .recovery(
                &fence(),
                &scope(),
                5,
                Command::SanitizeApply {
                    digest: preview.digest,
                },
            )
            .unwrap();
        assert!(applied.applied);
        assert!(
            store
                .artifact(&fence(), &scope(), "old")
                .unwrap()
                .unwrap()
                .archived
        );
        assert!(
            !store
                .artifact(&fence(), &scope(), "partial")
                .unwrap()
                .unwrap()
                .archived
        );
        assert_eq!(
            store
                .publication_output(&fence(), "project", "old", "two")
                .unwrap(),
            Some(b"test".to_vec())
        );
        assert!(store.snapshot(&fence(), &scope()).unwrap().heads.is_empty());
        drop(store);
        let readonly = Store::open_read_only(&database).unwrap().unwrap();
        // The immutable closure and output survived both archival and restart.
        assert_eq!(
            readonly
                .publication_payload(&fence(), "project", "old")
                .unwrap(),
            Some("immutable closure old".into())
        );
    }
}

#[test]
fn terminal_source_integrity_permission_and_untyped_failures_never_reopen() {
    for engine in ENGINES {
        for (outcome, code, detail) in [
            ("conflict", Some("CONFLICT"), "source input moved"),
            ("refused", Some("INVALID_INPUT"), "output bytes changed"),
            (
                "refused",
                Some("INSUFFICIENT_PERMISSIONS"),
                "build not admitted",
            ),
            (
                "refused",
                None,
                "The producing engine build is not admitted",
            ),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let mut store = Store::open(&dir.path().join("sync.sqlite")).unwrap();
            let row = row(engine, "current", &["one"]);
            seal(&mut store, row.clone());
            settle(&mut store, &row, outcome, code, detail);
            let result = store
                .recovery(
                    &fence(),
                    &scope(),
                    3,
                    Command::Retry {
                        row: "current".into(),
                    },
                )
                .unwrap();
            assert_eq!(result.refusals[0].code, "sync_row_not_retryable");
            assert!(!result.applied);
            assert!(result.changes.is_empty());
        }
    }
}

#[test]
fn public_json_owner_is_closed_and_matches_native_without_leaking_locators() {
    let original = row(&ENGINES[0], "current", &["one"]);
    let request = recovery::Request {
        schema: recovery::SCHEMA.into(),
        fence: fence(),
        scope: scope(),
        now_ms: 3,
        entries: vec![recovery::Entry {
            row: original,
            standing: true,
            last_attempt: None,
        }],
        lease: None,
        command: Command::Status { limit: 50 },
    };
    let mut json = serde_json::to_value(&request).unwrap();
    let encoded = recovery::evaluate(&serde_json::to_vec(&json).unwrap()).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&encoded).unwrap(),
        serde_json::to_value(recovery::resolve(request).unwrap()).unwrap()
    );
    assert!(!encoded.contains("private-local-path"));
    json["command"]["force"] = json!(true);
    assert!(recovery::evaluate(&serde_json::to_vec(&json).unwrap()).is_err());
}
