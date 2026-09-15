//! The store survives what the runtime contract names: restart, a second
//! worker, another account, an incompatible upgrade, and the handoff.

use ds_command_kernel::sync::Identity;
use ds_command_kernel::sync_store::{
    ArtifactOutput, ArtifactRow, ArtifactState, BytesEvidence, Event, Fence, LegacyHandoff,
    ReceiptRow, Scope,
};
use ds_sync_store::{Error, Store, SCHEMA_VERSION};
use serde_json::json;

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn fence(account: &str) -> Fence {
    Fence {
        account: account.into(),
        deployment: "https://gateway.example".into(),
        install_id: "install-1".into(),
    }
}
fn scope() -> Scope {
    Scope::Project {
        project: "p".into(),
    }
}
fn ident(op: &str) -> Identity {
    Identity {
        engine: "network_reporter".into(),
        operation: op.into(),
        variant: "default".into(),
    }
}
fn row(op: &str, key: &str) -> ArtifactRow {
    ArtifactRow {
        scope: scope(),
        identity: ident(op),
        sha256: SHA.into(),
        size_bytes: 10,
        produced_at_ms: 1,
        base_revision: Some("base".into()),
        input_base_fingerprint: Some("source".into()),
        engine_release: "r".into(),
        engine_build_manifest_sha256: None,
        grant_engine: None,
        resource: None,
        client_publish_id: key.into(),
        outputs: vec![ArtifactOutput {
            output_id: "xlsx".into(),
            format: "xlsx".into(),
            content_type: "application/x".into(),
            sha256: SHA.into(),
            size_bytes: 10,
        }],
        bytes_locator: "batch:1".into(),
        readable: true,
        state: ArtifactState::Held,
        state_reason: None,
        replay_key: key.into(),
        updated_at_ms: 1,
        transfer_state: None,
    }
}

#[test]
fn rows_survive_a_restart_and_stay_fenced_by_account() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sync").join("store.sqlite");
    {
        let mut store = Store::open(&path).unwrap();
        let t = store
            .apply(
                &fence("alice"),
                10,
                Event::LocalProduced {
                    row: row("a", "k1"),
                },
            )
            .unwrap();
        assert_eq!(t.changes.len(), 1);
        let mut receipt = ReceiptRow {
            receipt_id: "r1".into(),
            scope: scope(),
            action: "upload".into(),
            identity: Some(ident("a")),
            outcome: "committed".into(),
            detail: None,
            grant_id: Some("g".into()),
            work_id: Some("w".into()),
            head_revision: Some("rev-2".into()),
            head_sha256: Some(SHA.into()),
            head_counter: Some(2),
            committed_bytes: Some(10),
            at_ms: 20,
        };
        store
            .apply(
                &fence("alice"),
                20,
                Event::ActionReceipt {
                    receipt: receipt.clone(),
                },
            )
            .unwrap();
        // The same receipt twice is one receipt.
        receipt.at_ms = 21;
        store
            .apply(&fence("alice"), 21, Event::ActionReceipt { receipt })
            .unwrap();
    }
    // The process is gone; the file is the truth.
    let store = Store::open(&path).unwrap();
    let snapshot = store.snapshot(&fence("alice"), &scope()).unwrap();
    assert_eq!(snapshot.artifacts.len(), 1);
    assert_eq!(snapshot.artifacts[0].state, ArtifactState::Published);
    assert_eq!(snapshot.heads.len(), 1);
    assert_eq!(snapshot.heads[0].revision, "rev-2");
    assert_eq!(
        store.heads_read_at_ms(&fence("alice"), &scope()).unwrap(),
        Some(20)
    );
    assert_eq!(
        store.receipts(&fence("alice"), &scope(), 10).unwrap().len(),
        1
    );
    // Another account on the same machine sees none of it.
    let other = store.snapshot(&fence("bob"), &scope()).unwrap();
    assert!(other.artifacts.is_empty() && other.heads.is_empty());
    assert_eq!(
        store.heads_read_at_ms(&fence("bob"), &scope()).unwrap(),
        None
    );
}

#[test]
fn a_read_of_an_empty_record_is_still_a_read() {
    let mut store = Store::in_memory().unwrap();
    assert_eq!(
        store.heads_read_at_ms(&fence("alice"), &scope()).unwrap(),
        None
    );
    store
        .apply(
            &fence("alice"),
            50,
            Event::HeadsRead {
                scope: scope(),
                heads: vec![],
            },
        )
        .unwrap();
    assert_eq!(
        store.heads_read_at_ms(&fence("alice"), &scope()).unwrap(),
        Some(50)
    );
    assert!(store
        .snapshot(&fence("alice"), &scope())
        .unwrap()
        .heads
        .is_empty());
}

#[test]
fn two_workers_on_one_file_never_hold_the_same_lease() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let mut first = Store::open(&path).unwrap();
    let mut second = Store::open(&path).unwrap();
    let taken = first
        .apply(
            &fence("alice"),
            1_000,
            Event::LeaseTake {
                scope: scope(),
                worker_id: "window".into(),
                ttl_ms: 60_000,
            },
        )
        .unwrap();
    assert!(taken.refusals.is_empty());
    let refused = second
        .apply(
            &fence("alice"),
            1_001,
            Event::LeaseTake {
                scope: scope(),
                worker_id: "cli".into(),
                ttl_ms: 60_000,
            },
        )
        .unwrap();
    assert_eq!(refused.refusals[0].code, "lease_held");
    // Abandoned ownership is recovered once the lease is dead.
    let recovered = second
        .apply(
            &fence("alice"),
            61_001,
            Event::LeaseTake {
                scope: scope(),
                worker_id: "cli".into(),
                ttl_ms: 60_000,
            },
        )
        .unwrap();
    assert!(recovered.refusals.is_empty());
    let released = first
        .apply(
            &fence("alice"),
            61_002,
            Event::LeaseRelease {
                scope: scope(),
                worker_id: "window".into(),
            },
        )
        .unwrap();
    assert_eq!(released.refusals[0].code, "lease_held");
}

#[test]
fn a_newer_schema_on_disk_is_refused_and_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    {
        let mut store = Store::open(&path).unwrap();
        store
            .apply(
                &fence("alice"),
                1,
                Event::LocalProduced {
                    row: row("a", "k1"),
                },
            )
            .unwrap();
    }
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute(
        "UPDATE meta SET value = ?1 WHERE key = 'schema_version'",
        [SCHEMA_VERSION + 1],
    )
    .unwrap();
    drop(raw);
    match Store::open(&path) {
        Err(Error::Incompatible { found, supported }) => {
            assert_eq!((found, supported), (SCHEMA_VERSION + 1, SCHEMA_VERSION));
        }
        other => panic!("expected an explicit refusal, got {:?}", other.map(|_| ())),
    }
    let raw = rusqlite::Connection::open(&path).unwrap();
    let kept: i64 = raw
        .query_row("SELECT COUNT(*) FROM artifacts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 1, "the queue is never erased");
}

#[test]
fn the_handoff_imports_across_scopes_once() {
    let mut store = Store::in_memory().unwrap();
    let rows: Vec<LegacyHandoff> = serde_json::from_value(json!([
        {"row": {"kind": "compute_artifact", "id": "legacy-1", "owner_uid": "alice", "project_id": "p", "root": "/r", "client_run_id": "run",
                 "engine": "network_reporter", "operation": "export-a", "variant": "default", "engine_version": "r", "input_base_fingerprint": "b",
                 "client_publish_id": "pub-1", "status": "failed", "phase": "pending", "attempts": 3, "created_at": 4, "updated_at": 5, "synced_at": null,
                 "error": "The producing engine build is not admitted.", "outputs": [{"output_id": "xlsx", "local_path": "/x", "format": "xlsx", "content_type": "application/x", "sha256": SHA, "size_bytes": 7, "state": "pending"}]},
         "bytes": {"kind": "verified", "locator": "batch:pub-1"}},
        {"row": {"kind": "compute_artifact", "id": "legacy-2", "owner_uid": "alice", "project_id": "q", "root": "/r", "client_run_id": "run",
                 "engine": "solar", "operation": "calculate-city", "variant": "default", "engine_version": "s", "input_base_fingerprint": "",
                 "status": "pending", "phase": "pending", "attempts": 0, "created_at": 4, "updated_at": 5, "synced_at": null, "error": null,
                 "outputs": [{"output_id": "result", "local_path": "", "format": "json", "content_type": "application/json", "sha256": "", "size_bytes": 0, "expected_sha256": SHA, "expected_size_bytes": 3, "state": "preparing"}]},
         "bytes": {"kind": "missing", "reason": "no sealed Solar batch on this machine"}}
    ]))
    .unwrap();
    let t = store
        .apply(
            &fence("alice"),
            100,
            Event::HandoffLegacy { rows: rows.clone() },
        )
        .unwrap();
    assert_eq!(t.changes.len(), 4);
    let p = store.snapshot(&fence("alice"), &scope()).unwrap();
    assert_eq!(p.artifacts.len(), 1);
    assert_eq!(p.artifacts[0].state, ArtifactState::Held);
    assert_eq!(p.artifacts[0].replay_key, "pub-1");
    let q = store
        .snapshot(
            &fence("alice"),
            &Scope::Project {
                project: "q".into(),
            },
        )
        .unwrap();
    assert_eq!(q.artifacts[0].state, ArtifactState::Refused);
    assert!(matches!(&rows[1].bytes, BytesEvidence::Missing { .. }));
    let again = store
        .apply(&fence("alice"), 101, Event::HandoffLegacy { rows })
        .unwrap();
    assert!(again.changes.is_empty());
    // The producer recording the same publication later is one row, not two.
    let recorded = store
        .apply(
            &fence("alice"),
            102,
            Event::LocalProduced {
                row: row("export-a", "pub-1"),
            },
        )
        .unwrap();
    assert_eq!(recorded.refusals[0].code, "already_recorded");
}
