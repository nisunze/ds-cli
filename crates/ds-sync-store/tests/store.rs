//! The store survives what the runtime contract names: restart, a second
//! worker, another account, and an incompatible upgrade.

use ds_command_kernel::sync::Identity;
use ds_command_kernel::sync_store::ABANDONED_AFTER_MS;
use ds_command_kernel::sync_store::{
    ArtifactOutput, ArtifactRow, ArtifactState, Event, Fence, ReceiptRow, Scope,
};
use ds_sync_store::{Error, Store, SCHEMA_VERSION};

const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn downgrade_fixture_to_v1(path: &std::path::Path) {
    let raw = rusqlite::Connection::open(path).unwrap();
    raw.execute_batch("BEGIN IMMEDIATE;
        ALTER TABLE artifacts RENAME TO artifacts_v2;
        CREATE TABLE artifacts(account TEXT NOT NULL, deployment TEXT NOT NULL, install_id TEXT NOT NULL,
        scope TEXT NOT NULL, engine TEXT NOT NULL, operation TEXT NOT NULL, variant TEXT NOT NULL,
        replay_key TEXT NOT NULL, state TEXT NOT NULL, updated_at_ms INTEGER NOT NULL, row TEXT NOT NULL,
        PRIMARY KEY(account,deployment,install_id,scope,engine,operation,variant),
        UNIQUE(account,deployment,install_id,replay_key));
        INSERT INTO artifacts SELECT * FROM artifacts_v2; DROP TABLE artifacts_v2;
        UPDATE meta SET value=1 WHERE key='schema_version'; COMMIT;").unwrap();
}

#[test]
fn v1_upgrade_preserves_artifact_bytes_and_receipts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
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
    let before = store.snapshot(&fence("alice"), &scope()).unwrap().artifacts;
    drop(store);
    downgrade_fixture_to_v1(&path);
    let store = Store::open(&path).unwrap();
    assert_eq!(
        store.snapshot(&fence("alice"), &scope()).unwrap().artifacts,
        before
    );
    assert_eq!(
        store
            .connection_for_test()
            .query_row(
                "SELECT value FROM meta WHERE key='schema_version'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
}

#[test]
fn unknown_legacy_or_unversioned_artifacts_are_not_silently_relabelled() {
    for unversioned in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("store.sqlite");
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
        drop(store);
        downgrade_fixture_to_v1(&path);
        let connection = rusqlite::Connection::open(&path).unwrap();
        if unversioned {
            connection
                .execute("DELETE FROM meta WHERE key='schema_version'", [])
                .unwrap();
        } else {
            connection
                .execute(
                    "ALTER TABLE artifacts ADD COLUMN unknown_owner_data TEXT",
                    [],
                )
                .unwrap();
        }
        let before: String = connection
            .query_row("SELECT row FROM artifacts", [], |row| row.get(0))
            .unwrap();
        drop(connection);
        assert!(Store::open(&path).is_err());
        let connection = rusqlite::Connection::open(&path).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT row FROM artifacts", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            before
        );
        assert_eq!(
            connection
                .query_row(
                    "SELECT count(*) FROM meta WHERE key='schema_version' AND value=2",
                    [],
                    |row| row.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
}

#[test]
fn v2_keeps_prior_publication_rows_and_projects_one_standing_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
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
    let mut later = row("a", "k2");
    later.sha256 = "b".repeat(64);
    later.produced_at_ms = 2;
    store
        .apply(&fence("alice"), 2, Event::LocalProduced { row: later })
        .unwrap();
    drop(store);
    let store = Store::open_read_only(&path).unwrap().unwrap();
    assert_eq!(
        store.snapshot(&fence("alice"), &scope()).unwrap().artifacts[0].replay_key,
        "k2"
    );
    assert_eq!(store.artifacts_of_fence(&fence("alice")).unwrap().len(), 1);
    assert_eq!(
        store
            .connection_for_test()
            .query_row("SELECT count(*) FROM artifacts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        2
    );
    let first: String = store
        .connection_for_test()
        .query_row("SELECT row FROM artifacts WHERE replay_key='k1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(
        serde_json::from_str::<ArtifactRow>(&first).unwrap().sha256,
        SHA
    );
    assert!(store
        .snapshot(&fence("bob"), &scope())
        .unwrap()
        .artifacts
        .is_empty());
}

#[test]
fn unknown_v2_layout_refuses_without_relabeling_or_losing_rows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
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
    drop(store);
    downgrade_fixture_to_v1(&path);
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.execute("UPDATE meta SET value=2 WHERE key='schema_version'", [])
        .unwrap();
    drop(raw);
    assert!(Store::open(&path).is_err());
    assert!(Store::open_read_only(&path).is_err());
    let raw = rusqlite::Connection::open(&path).unwrap();
    assert_eq!(
        raw.query_row(
            "SELECT value FROM meta WHERE key='schema_version'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        2
    );
    assert_eq!(
        raw.query_row("SELECT count(*) FROM artifacts", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

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
        acknowledged_work_id: None,
        outputs: vec![ArtifactOutput {
            filename: None,
            paper_size: None,
            presentation: None,
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
            client_publish_id: Some("k1".into()),
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
fn an_acknowledged_write_is_synced_to_disk_before_it_is_acknowledged() {
    // WHY: the owner lost power on 2026-09-20. Under WAL, `synchronous=NORMAL`
    // acknowledges a commit before the log reaches the platter; an
    // acknowledged seal could vanish with the cut. `FULL` is the decision,
    // and this pins it so a later "performance" edit cannot quietly undo it.
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("store.sqlite")).unwrap();
    let synchronous: i64 = store
        .connection_for_test()
        .query_row("PRAGMA synchronous", [], |row| row.get(0))
        .unwrap();
    assert_eq!(synchronous, 2, "2 is FULL; 1 would be NORMAL");
    let journal: String = store
        .connection_for_test()
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal, "wal");
}

#[test]
fn the_queue_is_read_from_the_rows_and_a_read_only_open_leaves_nothing_behind() {
    // WHY: `ds report outbox status` must answer with no session, no gateway
    // and no running host — on exactly the machines where a stopped queue
    // goes unnoticed — and it must not create a store where none was.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sync").join("store.sqlite");
    assert!(
        Store::open_read_only(&path).unwrap().is_none(),
        "no store is an empty queue, not an error"
    );
    assert!(!path.exists(), "a read-only open creates nothing");
    let now = 10 * ABANDONED_AFTER_MS;
    {
        let mut store = Store::open(&path).unwrap();
        let mut waiting = row("a", "k1");
        waiting.produced_at_ms = now - 5_000;
        store
            .apply(&fence("alice"), now, Event::LocalProduced { row: waiting })
            .unwrap();
        let mut other = row("b", "k2");
        other.scope = Scope::Project {
            project: "q".into(),
        };
        other.produced_at_ms = now - ABANDONED_AFTER_MS - 1_000;
        store
            .apply(&fence("alice"), now, Event::LocalProduced { row: other })
            .unwrap();
        store
            .apply(
                &fence("alice"),
                now,
                Event::LeaseTake {
                    scope: scope(),
                    worker_id: "pump".into(),
                    ttl_ms: 60_000,
                },
            )
            .unwrap();
    }
    let reader = Store::open_read_only(&path)
        .unwrap()
        .expect("the store exists");
    let status = reader.queue(&fence("alice"), None, now).unwrap();
    assert_eq!(status.queued_batches, 2);
    assert_eq!(status.projects.len(), 2);
    assert_eq!(status.leases.len(), 1, "the live lease on p is the pump");
    assert!(
        status
            .projects
            .iter()
            .any(|p| p.project_id == "p" && p.pumped),
        "p is pumped"
    );
    assert!(
        status.stuck,
        "q has waited over an hour with no lease on it: stuck"
    );
    let narrowed = reader.queue(&fence("alice"), Some("p"), now).unwrap();
    assert_eq!(narrowed.queued_batches, 1);
    assert!(!narrowed.stuck);
    assert_eq!(
        reader
            .queue(&fence("bob"), None, now)
            .unwrap()
            .queued_batches,
        0,
        "another account's queue is empty"
    );
    // The whole file, every fence: what a status reads with no identity to
    // name one. One store is one owner's.
    let whole = reader.queue_all(None, now).unwrap();
    assert_eq!(whole.queued_batches, 2);
    assert_eq!(whole.leases.len(), 1);
    assert!(whole.stuck);
    assert_eq!(reader.queue_all(Some("p"), now).unwrap().queued_batches, 1);
}

#[test]
fn invariant_5_a_store_fenced_by_account_deployment_and_install_shows_another_fence_nothing() {
    // WHY: invariant (5) of "reliably synced". `rows_survive_a_restart_and_
    // stay_fenced_by_account` proves the account fence; this proves the two
    // others the fence carries, for rows, heads, receipts, leases and the
    // queue reading alike.
    let mut store = Store::in_memory().unwrap();
    let mine = fence("alice");
    store
        .apply(
            &mine,
            10,
            Event::LocalProduced {
                row: row("a", "k1"),
            },
        )
        .unwrap();
    store
        .apply(
            &mine,
            11,
            Event::ActionReceipt {
                receipt: ReceiptRow {
                    receipt_id: "r1".into(),
                    scope: scope(),
                    action: "upload".into(),
                    identity: Some(ident("a")),
                    outcome: "committed".into(),
                    detail: None,
                    grant_id: None,
                    work_id: Some("w".into()),
                    client_publish_id: Some("k1".into()),
                    head_revision: Some("rev-1".into()),
                    head_sha256: None,
                    head_counter: Some(1),
                    committed_bytes: Some(10),
                    at_ms: 11,
                },
            },
        )
        .unwrap();
    store
        .apply(
            &mine,
            12,
            Event::LeaseTake {
                scope: scope(),
                worker_id: "w".into(),
                ttl_ms: 1_000,
            },
        )
        .unwrap();
    let other_deployment = Fence {
        deployment: "https://other.example".into(),
        ..fence("alice")
    };
    let other_install = Fence {
        install_id: "install-2".into(),
        ..fence("alice")
    };
    for foreign in [other_deployment, other_install] {
        let snapshot = store.snapshot(&foreign, &scope()).unwrap();
        assert!(snapshot.artifacts.is_empty() && snapshot.heads.is_empty());
        assert!(snapshot.lease.is_none(), "a lease is fenced too");
        assert!(store.receipts(&foreign, &scope(), 10).unwrap().is_empty());
        assert_eq!(store.queue(&foreign, None, 13).unwrap().queued_batches, 0);
        // The same replay key under another fence is a different row, and
        // the same receipt id another receipt: neither is refused as a
        // duplicate of the fenced one.
        let recorded = store
            .apply(
                &foreign,
                14,
                Event::LocalProduced {
                    row: row("a", "k1"),
                },
            )
            .unwrap();
        assert!(recorded.refusals.is_empty(), "{:?}", recorded.refusals);
        // And the lease of one fence never blocks a worker under another.
        let taken = store
            .apply(
                &foreign,
                15,
                Event::LeaseTake {
                    scope: scope(),
                    worker_id: "w2".into(),
                    ttl_ms: 1_000,
                },
            )
            .unwrap();
        assert!(taken.refusals.is_empty());
    }
    let mine_again = store.snapshot(&mine, &scope()).unwrap();
    assert_eq!(mine_again.artifacts.len(), 1);
    assert_eq!(mine_again.artifacts[0].state, ArtifactState::Published);
    assert_eq!(mine_again.lease.unwrap().worker_id, "w");
}

/// Signed work grants live here: the directory is 0700 and the database and
/// its WAL sidecars 0600, and a store from before the rule is tightened on open.
#[cfg(unix)]
#[test]
fn the_store_and_its_directory_are_private_to_the_account() {
    use std::os::unix::fs::PermissionsExt;
    let mode =
        |path: &std::path::Path| std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sync").join("store.sqlite");
    let mut store = Store::open(&path).unwrap();
    store
        .apply(
            &fence("alice"),
            10,
            Event::LocalProduced {
                row: row("a", "k1"),
            },
        )
        .unwrap();
    assert_eq!(mode(&dir.path().join("sync")), 0o700);
    assert_eq!(mode(&path), 0o600);
    for sidecar in ["store.sqlite-wal", "store.sqlite-shm"] {
        let sidecar = dir.path().join("sync").join(sidecar);
        assert!(sidecar.exists(), "WAL mode keeps {}", sidecar.display());
        assert_eq!(mode(&sidecar), 0o600, "{}", sidecar.display());
    }
    drop(store);

    let old = std::fs::Permissions::from_mode(0o644);
    std::fs::set_permissions(&path, old).unwrap();
    drop(Store::open(&path).unwrap());
    assert_eq!(mode(&path), 0o600, "an existing store is tightened on open");
}

/// The blocked writer proves the read and update share one transaction:
/// reading A before waiting for a competing writer would overwrite B here.
#[test]
fn late_ack_reads_replacement_after_acquiring_the_same_write_transaction() {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};
    static WAITING: AtomicBool = AtomicBool::new(false);
    fn busy(attempt: i32) -> bool {
        WAITING.store(true, Ordering::Release);
        std::thread::sleep(Duration::from_millis(1));
        attempt < 5000
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("store.sqlite");
    let mut writer = Store::open(&path).unwrap();
    writer
        .apply(
            &fence("alice"),
            1,
            Event::LocalProduced {
                row: row("a", "k1"),
            },
        )
        .unwrap();
    let mut late = Store::open(&path).unwrap();
    late.connection_for_test().busy_handler(Some(busy)).unwrap();
    let transaction = rusqlite::Transaction::new_unchecked(
        writer.connection_for_test(),
        rusqlite::TransactionBehavior::Immediate,
    )
    .unwrap();
    let receipt = ReceiptRow {
        receipt_id: "late-A".into(),
        scope: scope(),
        action: "upload".into(),
        identity: Some(ident("a")),
        outcome: "committed".into(),
        detail: None,
        grant_id: None,
        work_id: Some("work-A".into()),
        client_publish_id: Some("k1".into()),
        head_revision: Some("head-A".into()),
        head_sha256: Some(SHA.into()),
        head_counter: Some(1),
        committed_bytes: Some(10),
        at_ms: 3,
    };
    WAITING.store(false, Ordering::Release);
    let pending = std::thread::spawn(move || {
        late.apply(&fence("alice"), 3, Event::ActionReceipt { receipt })
    });
    let deadline = Instant::now() + Duration::from_secs(2);
    while !WAITING.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        WAITING.load(Ordering::Acquire),
        "receipt waits for the owned writer"
    );
    let replacement = row("a", "k2");
    // Fixture-only competing seal on the same exact slot, committed while
    // the late receipt is waiting for its writer transaction.
    transaction
        .execute(
            "UPDATE artifacts SET replay_key=?1, row=?2 WHERE operation='a'",
            rusqlite::params![
                replacement.replay_key,
                serde_json::to_string(&replacement).unwrap()
            ],
        )
        .unwrap();
    transaction.commit().unwrap();
    let transition = pending.join().unwrap().unwrap();
    assert_eq!(transition.refusals[0].code, "publication_replaced");
    let snapshot = writer.snapshot(&fence("alice"), &scope()).unwrap();
    assert_eq!(snapshot.artifacts, vec![replacement]);
    assert!(snapshot.heads.is_empty());
    assert_eq!(
        writer
            .receipts(&fence("alice"), &scope(), 10)
            .unwrap()
            .len(),
        1
    );
}
