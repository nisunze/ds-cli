//! The shared native sync runtime, driven end to end through the real
//! [`StoreHost`] over a real `ds_sync_store` on disk, against deterministic
//! fakes for the record, the grant issuer, the storage sessions and the bytes.
//!
//! These are the acceptance behaviours the assignment names: an authenticated
//! open/upload/publish that commits exactly once; an unauthorized identity
//! that moves no byte; restart and reopen from the durable store; a lease held
//! by a second host; a transfer failure; and a cancellation. They run the same
//! `run()` every host runs, so a drift between hosts is a drift here.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use ds_sync_runtime::fakes::{
    self, local_row, record_head, FakeGateway, FakeProducer, FakeReads, TransferMode,
};
use ds_sync_runtime::store::Fence;
use ds_sync_runtime::{
    kernel_sync as sync, open_store, projects_of_fence, run, LocalOutput, LocalRow, Producer,
    SharedStore, SolarPublishOutcome, SolarPublisher, StoreHost, SyncHost, SyncRun,
    TransferReceipt,
};
use sha2::Digest;

const PROJECT: &str = "p1";

fn fence() -> Fence {
    Fence {
        account: "account-1".into(),
        deployment: "https://gateway.example.com".into(),
        install_id: "install-1".into(),
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    store: SharedStore,
    world: fakes::SharedWorld,
    reads_root: std::path::PathBuf,
}

impl Harness {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = open_store(&dir.path().join("sync").join("store.sqlite")).expect("open store");
        Harness {
            reads_root: dir.path().join("sync-downloads"),
            _dir: dir,
            store,
            world: fakes::world(),
        }
    }

    /// The restart a kill gets: a fresh connection to the same file, with
    /// the previous one never closed — no checkpoint, no clean shutdown —
    /// so what the new process sees is exactly what reached the disk (WAL,
    /// `synchronous=FULL`) before the kill. The old connection is leaked on
    /// purpose; dropping it would be the orderly close a crash never has.
    fn reopen_after_kill(&mut self) {
        let path = self._dir.path().join("sync").join("store.sqlite");
        let killed = std::mem::replace(&mut self.store, open_store(&path).expect("reopen store"));
        std::mem::forget(killed);
    }

    /// Seal `rows` the way a producer's seal does: bytes (the fake's
    /// scripted rows) and the store row in one step. The store is the
    /// queue; a row that is only on disk is a legacy batch and is adopted
    /// with a receipt, which is its own test.
    fn seal(&self, rows: &[LocalRow]) {
        let mut store = self.store.lock().unwrap();
        for row in rows {
            store
                .apply(
                    &fence(),
                    ds_sync_runtime::now_ms(),
                    ds_sync_runtime::store::Event::LocalProduced {
                        row: ds_sync_runtime::rows::store_row(PROJECT, row, 1_000),
                    },
                )
                .expect("the seal records its row");
        }
    }

    /// Run one trigger through a fresh host bound to this harness's durable
    /// store — a new host every call, the way a restart gets one. The rows
    /// are sealed first (bytes and row together), as a producer seals them.
    fn run(&self, rows: Vec<ds_sync_runtime::LocalRow>, trigger: sync::Trigger) -> SyncRun {
        self.run_with(rows, trigger, "install-1#100").0
    }

    fn run_with(
        &self,
        rows: Vec<ds_sync_runtime::LocalRow>,
        trigger: sync::Trigger,
        worker_id: &str,
    ) -> (SyncRun, Arc<std::sync::atomic::AtomicBool>) {
        self.seal(&rows);
        let gateway = FakeGateway {
            world: self.world.clone(),
        };
        let producer = FakeProducer::new(self.world.clone(), rows);
        let cancel = producer.cancel.clone();
        let reads = FakeReads {
            world: self.world.clone(),
            root: self.reads_root.clone(),
        };
        let online = || true;
        let host = StoreHost::new(
            self.store.clone(),
            fence(),
            worker_id.to_string(),
            PROJECT,
            &gateway,
            &producer,
            &reads,
            &online,
        )
        .expect("host");
        let outcome = run(&host, PROJECT, trigger).expect("the loop runs");
        (outcome, cancel)
    }
}

fn outcomes(run: &SyncRun) -> Vec<String> {
    run.receipts
        .iter()
        .map(|receipt| format!("{}:{}", receipt.action, receipt.outcome))
        .collect()
}

fn solar_row() -> LocalRow {
    let bytes = b"sealed report input";
    let output = LocalOutput {
        output_id: "report-input".into(),
        format: "json".into(),
        content_type: "application/json".into(),
        sha256: format!("{:x}", sha2::Sha256::digest(bytes)),
        size_bytes: bytes.len() as u64,
    };
    let mut pairs = vec![(output.output_id.clone(), output.sha256.clone())];
    LocalRow {
        identity: sync::Identity {
            engine: "solar".into(),
            operation: "calculate-city-1".into(),
            variant: "default".into(),
        },
        sha256: ds_sync_runtime::inventory_digest(&mut pairs),
        size_bytes: output.size_bytes,
        produced_at_ms: 1_000,
        base_revision: Some(fakes::FINGERPRINT.into()),
        input_base_fingerprint: None,
        readable: true,
        engine_release: "ds-solar-engine@1.2.3".into(),
        engine_build_manifest_sha256: "a".repeat(64),
        grant_engine: "solar".into(),
        resource: "city-1".into(),
        client_publish_id: "solar-job-1".into(),
        outputs: vec![output],
        bytes_locator: "solar:prepared:job:solar-job-1".into(),
    }
}

struct ScriptedSolarPublisher {
    result: Result<SolarPublishOutcome, String>,
    calls: AtomicUsize,
}

impl ScriptedSolarPublisher {
    fn new(result: Result<SolarPublishOutcome, String>) -> Self {
        Self {
            result,
            calls: AtomicUsize::new(0),
        }
    }
}

impl SolarPublisher for ScriptedSolarPublisher {
    fn publish_solar(
        &self,
        _row: &LocalRow,
        guard: &dyn Fn() -> Result<(), String>,
        transfer: &dyn Fn(&str, &str) -> Result<TransferReceipt, String>,
    ) -> Result<SolarPublishOutcome, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        guard()?;
        let receipt = transfer("report-input", "https://storage.example.test/solar-upload")?;
        assert_eq!(receipt.output_id, "report-input");
        self.result.clone()
    }
}

fn solar_host<'a>(
    harness: &'a Harness,
    gateway: &'a dyn ds_sync_runtime::Gateway,
    producer: &'a FakeProducer,
    reads: &'a FakeReads,
    online: &'a dyn Fn() -> bool,
) -> StoreHost<'a> {
    StoreHost::new(
        harness.store.clone(),
        fence(),
        "solar-pump".into(),
        PROJECT,
        gateway,
        producer,
        reads,
        online,
    )
    .expect("solar host")
}

#[test]
fn solar_publication_records_terminal_receipts_and_does_not_reopen_them() {
    for (result, expected_state) in [
        (
            Ok(SolarPublishOutcome::Published {
                work_id: "work-1".into(),
                head_revision: 4,
            }),
            "published",
        ),
        (
            Ok(SolarPublishOutcome::StoredStale {
                work_id: None,
                detail: "the project input changed".into(),
            }),
            "conflict",
        ),
        (
            Ok(SolarPublishOutcome::Blocked {
                detail: "sealed provenance is absent".into(),
            }),
            "refused",
        ),
    ] {
        let harness = Harness::new();
        let gateway = FakeGateway {
            world: harness.world.clone(),
        };
        let producer = FakeProducer::new(harness.world.clone(), vec![solar_row()]);
        let reads = FakeReads {
            world: harness.world.clone(),
            root: harness.reads_root.clone(),
        };
        let online = || true;
        let publisher = ScriptedSolarPublisher::new(result);
        let host = solar_host(&harness, &gateway, &producer, &reads, &online);
        let receipts = host.run_solar_publications(&publisher).expect("first pump");
        assert_eq!(receipts.len(), 1);
        assert_eq!(publisher.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            host.store_read().expect("store read")["artifacts"][0]["state"],
            expected_state
        );
        if expected_state == "published" {
            let read = host.store_read().expect("published head read");
            assert_eq!(read["heads"][0]["revision"], fakes::FINGERPRINT);
            assert_eq!(read["heads"][0]["head_revision"], 4);
        }
        drop(host);

        // A fresh host is the restart path. No terminal publication returns to
        // the adapter even though its completed compute row is still present.
        let reopened = solar_host(&harness, &gateway, &producer, &reads, &online);
        let second = reopened
            .run_solar_publications(&publisher)
            .expect("restart pump");
        assert!(second.is_empty());
        assert_eq!(publisher.calls.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn solar_transport_failure_is_held_and_retried_by_a_fresh_pump() {
    let harness = Harness::new();
    let gateway = FakeGateway {
        world: harness.world.clone(),
    };
    let producer = FakeProducer::new(harness.world.clone(), vec![solar_row()]);
    let reads = FakeReads {
        world: harness.world.clone(),
        root: harness.reads_root.clone(),
    };
    let online = || true;
    let failed = ScriptedSolarPublisher::new(Err("authority temporarily unavailable".into()));
    let first = solar_host(&harness, &gateway, &producer, &reads, &online);
    let receipts = first
        .run_solar_publications(&failed)
        .expect("failed pass records");
    assert_eq!(receipts[0].outcome, "failed");
    drop(first);

    let succeeding = ScriptedSolarPublisher::new(Ok(SolarPublishOutcome::Published {
        work_id: "work-after-retry".into(),
        head_revision: 5,
    }));
    let restarted = solar_host(&harness, &gateway, &producer, &reads, &online);
    let receipts = restarted
        .run_solar_publications(&succeeding)
        .expect("retry pump");
    assert_eq!(receipts[0].outcome, "published");
    assert_eq!(
        restarted.store_read().expect("store read")["artifacts"][0]["state"],
        "published"
    );
}

#[test]
fn solar_cancellation_racing_a_publication_keeps_the_cancelled_row_refused() {
    struct CancellingPublisher<'a> {
        host: &'a StoreHost<'a>,
    }
    impl SolarPublisher for CancellingPublisher<'_> {
        fn publish_solar(
            &self,
            row: &LocalRow,
            guard: &dyn Fn() -> Result<(), String>,
            transfer: &dyn Fn(&str, &str) -> Result<TransferReceipt, String>,
        ) -> Result<SolarPublishOutcome, String> {
            guard()?;
            let _ = transfer("report-input", "https://storage.example.test/solar-upload")?;
            self.host.cancel(&row.identity)?;
            Ok(SolarPublishOutcome::Published {
                work_id: "remote-work".into(),
                head_revision: 7,
            })
        }
    }

    let harness = Harness::new();
    let gateway = FakeGateway {
        world: harness.world.clone(),
    };
    let producer = FakeProducer::new(harness.world.clone(), vec![solar_row()]);
    let reads = FakeReads {
        world: harness.world.clone(),
        root: harness.reads_root.clone(),
    };
    let online = || true;
    let host = solar_host(&harness, &gateway, &producer, &reads, &online);
    let publisher = CancellingPublisher { host: &host };
    assert!(host
        .run_solar_publications(&publisher)
        .expect("race is handled")
        .is_empty());
    let state = host.store_read().expect("store read")["artifacts"][0]["state"].clone();
    assert_eq!(state, "refused");
    assert!(host
        .run_solar_publications(&publisher)
        .expect("cancelled rows stay terminal")
        .is_empty());
}

// ---------------------------------------------------------------------------

#[test]
fn open_upload_publish_commits_exactly_once_and_replan_is_in_sync() {
    let harness = Harness::new();
    let bytes = vec![7_u8; 4096];
    let outcome = harness.run(vec![local_row(&bytes)], sync::Trigger::Startup);

    assert_eq!(
        outcomes(&outcome),
        vec!["open_grant:granted", "upload:published"],
        "the plan's order is the run's order",
    );

    let world = fakes::lock(&harness.world);
    assert_eq!(world.commits, 1, "the record advanced exactly once");
    assert_eq!(world.declarations.len(), 2, "declare, then restate");
    assert_eq!(
        world.declarations[0], world.declarations[1],
        "the restated declaration is byte-identical",
    );
    assert_eq!(
        world.declarations[0]["base_revisions"]["export-tx_1"],
        fakes::FINGERPRINT,
        "base_revisions is what the snapshot pinned",
    );
    assert_eq!(
        world.declarations[0]["artifacts"][0]["attribution"]["engine_build_manifest_sha256"],
        fakes::BUILD_MANIFEST,
        "every artifact names the build that produced it",
    );
    assert_eq!(world.head_reads, 1, "startup reads the record once");
    assert_eq!(world.transfers.len(), 1, "one output, one transfer");
    drop(world);

    let upload = outcome
        .receipts
        .iter()
        .find(|receipt| receipt.action == "upload")
        .expect("an upload receipt");
    assert_eq!(upload.head_revision, Some(8));
    assert_eq!(upload.revision.as_deref(), Some("head-8"));
    let stored = harness
        .store
        .lock()
        .unwrap()
        .snapshot(
            &fence(),
            &ds_sync_runtime::store::Scope::Project {
                project: PROJECT.into(),
            },
        )
        .expect("stored publication");
    assert_eq!(
        stored.artifacts[0].input_base_fingerprint.as_deref(),
        Some(fakes::FINGERPRINT)
    );
    assert_eq!(stored.artifacts[0].base_revision, None);
    assert_eq!(stored.heads[0].revision, "head-8");
    assert_eq!(
        stored.heads[0].input_base_fingerprint.as_deref(),
        Some(fakes::FINGERPRINT)
    );
    assert_eq!(outcome.replanned.summary.in_sync, 1);
    assert_eq!(outcome.replanned.summary.uploads, 0);
}

#[test]
fn an_unauthorized_identity_moves_no_byte_and_advances_nothing() {
    let harness = Harness::new();
    fakes::lock(&harness.world).refuse_open = Some("the gateway answered 403".to_string());
    let bytes = vec![1_u8; 2048];
    let outcome = harness.run(vec![local_row(&bytes)], sync::Trigger::Startup);

    assert_eq!(
        outcomes(&outcome),
        vec!["open_grant:failed", "upload:grant_open_failed"],
        "no grant, so no publication — and the row says the grant request failed",
    );
    let world = fakes::lock(&harness.world);
    assert_eq!(world.grants_minted, 0, "no grant was minted");
    assert!(world.declarations.is_empty(), "nothing was declared");
    assert_eq!(world.commits, 0, "the record did not advance");
    assert!(world.transfers.is_empty(), "not one byte left this machine");
    let upload = outcome
        .receipts
        .iter()
        .find(|receipt| receipt.action == "upload")
        .expect("an upload receipt");
    // The cause, not its consequence: the authority's own 403 reaches the row
    // the operator reads, so a refusal of access is distinguishable from an
    // unreachable signer or a grant nobody asked for.
    let detail = upload.detail.clone().expect("the row states a cause");
    assert!(
        detail.contains("the gateway answered 403"),
        "the row must carry what the authority answered: {detail}"
    );
    assert!(
        detail.contains("asked for on this run"),
        "and that a request WAS attempted: {detail}"
    );
}

/// The other way a row can end a run uncovered: the request SUCCEEDED and the
/// authority issued a grant for a scope nobody asked about. The row must say
/// that, not "none was asked for on this run" — which would put a second
/// guess exactly where the first one was removed.
#[test]
fn a_grant_issued_for_another_scope_is_not_reported_as_no_request() {
    let harness = Harness::new();
    let row = local_row(b"first report");
    {
        let mut world = fakes::lock(&harness.world);
        world.issue_release = Some("ds-network-reporter@9.9.9+beef".into());
    }

    let outcome = harness.run(vec![row], sync::Trigger::Startup);

    assert_eq!(
        outcomes(&outcome),
        vec!["open_grant:granted", "upload:grant_open_failed"],
        "the door answered, and what it answered does not cover this row",
    );
    let upload = outcome
        .receipts
        .iter()
        .find(|receipt| receipt.action == "upload")
        .expect("an upload receipt");
    let detail = upload.detail.clone().expect("the row states a cause");
    assert!(
        detail.contains("ds-network-reporter@9.9.9+beef") && detail.contains("issued instead"),
        "the row must name the scope that was issued: {detail}"
    );
    assert!(
        !detail.contains("none was asked for"),
        "a request WAS made this run: {detail}"
    );
    let world = fakes::lock(&harness.world);
    assert_eq!(
        world.commits, 0,
        "nothing publishes under a grant for another release"
    );
}

#[test]
fn one_failed_resource_grant_does_not_refuse_other_uploads_in_the_batch() {
    let harness = Harness::new();
    let first = local_row(b"first report");
    let mut second = local_row(b"second report");
    second.identity.operation = "export-tx_2".into();
    second.resource = "tx_2".into();
    second.input_base_fingerprint = Some("source-2".into());
    second.client_publish_id = "publish-22222222-2222-4222-8222-222222222222".into();
    {
        let mut world = fakes::lock(&harness.world);
        world
            .snapshot_base
            .insert("export-tx_2".into(), "source-2".into());
        world.refuse_open_resource = Some("tx_2".into());
    }

    let outcome = harness.run(vec![first, second], sync::Trigger::Startup);

    assert_eq!(
        outcomes(&outcome),
        vec![
            "open_grant:granted",
            "open_grant:failed",
            "upload:published",
            "upload:grant_open_failed",
        ]
    );
    let world = fakes::lock(&harness.world);
    assert_eq!(world.grants_minted, 1);
    assert_eq!(world.commits, 1, "the covered publication advances");
    assert_eq!(world.transfers.len(), 1, "only its bytes move");
    drop(world);
    let failed = outcome
        .receipts
        .iter()
        .find(|receipt| {
            receipt.action == "upload"
                && receipt
                    .identity
                    .as_ref()
                    .is_some_and(|identity| identity.operation == "export-tx_2")
        })
        .expect("the uncovered upload receipt");
    // This row names the resource whose grant failed and the words the
    // authority used, so an operator reading the Sync Center is not told that
    // tx_2 lacks a grant when the truth is that asking for it did not work.
    let detail = failed.detail.clone().expect("the held row states a cause");
    assert!(
        detail.contains("tx_2") && detail.contains("temporarily unreachable"),
        "unexpected detail: {detail}"
    );
    assert_eq!(failed.outcome, "grant_open_failed");
}

#[test]
fn a_publication_from_an_unregistered_build_never_moves_bytes() {
    let harness = Harness::new();
    // A release that registers a different build of the same engine release.
    fakes::lock(&harness.world).admitted_builds = vec!["f".repeat(64)];
    let bytes = vec![2_u8; 4096];
    let outcome = harness.run(vec![local_row(&bytes)], sync::Trigger::Startup);

    assert_eq!(
        outcomes(&outcome),
        vec!["open_grant:granted", "upload:engine_build_not_admitted"],
    );
    let world = fakes::lock(&harness.world);
    assert!(world.declarations.is_empty(), "nothing was declared");
    assert_eq!(world.commits, 0);
    assert!(world.transfers.is_empty(), "not one byte left this machine");
    drop(world);
    let upload = outcome
        .receipts
        .iter()
        .find(|receipt| receipt.action == "upload")
        .expect("an upload receipt");
    let detail = upload.detail.clone().expect("a reason");
    assert!(detail.contains(fakes::BUILD_MANIFEST) && detail.contains(&"f".repeat(64)));
}

#[test]
fn restart_reopens_the_store_and_never_recomputes_a_committed_publish() {
    let harness = Harness::new();
    let bytes = vec![3_u8; 8192];
    let first = harness.run(vec![local_row(&bytes)], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&first),
        vec!["open_grant:granted", "upload:published"]
    );
    assert_eq!(fakes::lock(&harness.world).commits, 1);

    // A restart: a brand-new host on the same durable store, same producer
    // rows still present. It must not re-open a grant, re-upload or re-commit.
    let second = harness.run(vec![local_row(&bytes)], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&second),
        vec!["nothing:in_sync"],
        "the held head from the committed publish answers the restart",
    );
    let world = fakes::lock(&harness.world);
    assert_eq!(world.commits, 1, "the record still advanced exactly once");
    assert_eq!(world.grants_minted, 1, "no second grant was minted");
    assert!(world.transfers.len() == 1, "no second transfer");
    // Startup re-reads the record (once per startup); what makes the restart
    // in_sync without a recompute is the durable head, not a skipped read.
    assert_eq!(world.head_reads, 2);
}

#[test]
fn a_project_the_store_holds_rows_for_is_recovered_after_restart() {
    let harness = Harness::new();
    let bytes = vec![4_u8; 4096];
    harness.run(vec![local_row(&bytes)], sync::Trigger::Startup);

    // A fresh process reads its rows to learn which projects to resume.
    let projects = projects_of_fence(&harness.store, &fence()).expect("projects");
    assert_eq!(projects, vec![PROJECT.to_string()]);
}

/// Invariant (3): two hosts, one store, one project — one holds the lease,
/// the other records who holds it and leaves, executing nothing.
#[test]
fn invariant_3_two_hosts_one_store_one_project_one_holds_the_lease_the_other_records_it_and_leaves()
{
    let harness = Harness::new();
    let bytes = vec![5_u8; 4096];

    // A second host takes the lease and holds it (its host object is not
    // dropped, so its lease is not released) while the first host runs.
    let gateway = FakeGateway {
        world: harness.world.clone(),
    };
    let rows = vec![local_row(&bytes)];
    let producer = FakeProducer::new(harness.world.clone(), rows.clone());
    let reads = FakeReads {
        world: harness.world.clone(),
        root: harness.reads_root.clone(),
    };
    let online = || true;
    let holder = StoreHost::new(
        harness.store.clone(),
        fence(),
        "install-1#200".into(),
        PROJECT,
        &gateway,
        &producer,
        &reads,
        &online,
    )
    .expect("second host");
    // The second host takes the lease and keeps it.
    assert!(
        ds_sync_runtime::SyncHost::take_lease(&holder, PROJECT)
            .expect("take")
            .is_none(),
        "the second host holds the lease",
    );

    let outcome = harness.run(rows, sync::Trigger::Startup);
    assert_eq!(
        outcome
            .receipts
            .iter()
            .map(|r| r.action)
            .collect::<Vec<_>>(),
        vec!["lease"],
        "the first host reports the lease and executes nothing",
    );
    assert_eq!(outcome.receipts[0].outcome, "held");
    assert!(
        outcome.receipts[0]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.starts_with("lease_held: install-1#200#")),
        "the holder is named: {:?}",
        outcome.receipts[0].detail
    );
    let world = fakes::lock(&harness.world);
    assert_eq!(world.commits, 0, "nothing was published under a held lease");
    assert_eq!(world.grants_minted, 0);
    assert!(world.transfers.is_empty());
    // The plan itself is still the honest answer to "what would happen".
    assert_eq!(outcome.plan.summary.uploads, 1);
}

#[test]
fn a_released_lease_lets_the_next_host_drain() {
    let harness = Harness::new();
    let bytes = vec![6_u8; 4096];
    // First host runs to completion and releases its lease on the way out.
    harness.run(vec![local_row(&bytes)], sync::Trigger::Startup);
    // The next host takes the lease cleanly and finds nothing left to do.
    let outcome = harness.run(vec![local_row(&bytes)], sync::Trigger::Manual);
    assert_eq!(outcomes(&outcome), vec!["nothing:in_sync"]);
}

#[test]
fn a_transfer_failure_holds_the_work_and_never_reports_a_commit() {
    let harness = Harness::new();
    fakes::lock(&harness.world).transfer_mode = TransferMode::Failed;
    let bytes = vec![9_u8; 4096];
    let outcome = harness.run(vec![local_row(&bytes)], sync::Trigger::Startup);

    // The grant opened and the transport tried, but the object never committed,
    // so the restated publish still answers upload_required and the run stops
    // at the bounded round ceiling without a commit.
    assert_eq!(
        outcome
            .receipts
            .iter()
            .filter(|r| r.action == "upload")
            .map(|r| r.outcome.clone())
            .collect::<Vec<_>>(),
        vec!["upload_required"],
        "a weak link leaves the work incomplete, not committed",
    );
    let world = fakes::lock(&harness.world);
    assert_eq!(world.commits, 0, "the record did not advance");
    assert!(!world.transfers.is_empty(), "the transport was attempted");

    // The re-plan still wants the upload: the work is held for the next
    // trigger, exactly as offline-first requires.
    drop(world);
    assert_eq!(outcome.replanned.summary.uploads, 1);
}

#[test]
fn a_transport_host_error_fails_the_upload_with_its_reason() {
    let harness = Harness::new();
    fakes::lock(&harness.world).transfer_mode =
        TransferMode::Refused("that publication is no longer committed locally".into());
    let bytes = vec![10_u8; 4096];
    let outcome = harness.run(vec![local_row(&bytes)], sync::Trigger::Startup);

    let upload = outcome
        .receipts
        .iter()
        .find(|receipt| receipt.action == "upload")
        .expect("an upload receipt");
    assert_eq!(upload.outcome, "failed");
    assert_eq!(
        upload.detail.as_deref(),
        Some("that publication is no longer committed locally"),
    );
    assert_eq!(fakes::lock(&harness.world).commits, 0);
}

#[test]
fn a_cancelled_transfer_leaves_the_work_resumable() {
    let harness = Harness::new();
    fakes::lock(&harness.world).transfer_mode = TransferMode::CancelledAfter(1024);
    let bytes = vec![11_u8; 4096];
    let (outcome, cancel) = harness.run_with(
        vec![local_row(&bytes)],
        sync::Trigger::Startup,
        "install-1#100",
    );

    assert!(cancel.load(Ordering::SeqCst), "the transport was cancelled");
    // Cancelled mid-transfer: the object never committed, so the restated
    // publish still needs it and the run ends incomplete — held, not lost.
    let world = fakes::lock(&harness.world);
    assert_eq!(world.commits, 0, "a cancelled transfer commits nothing");
    drop(world);
    assert_eq!(
        outcome.replanned.summary.uploads, 1,
        "the work is still held"
    );
}

#[test]
fn a_local_change_never_reads_the_record() {
    let harness = Harness::new();
    let bytes = vec![12_u8; 4096];
    let row = local_row(&bytes);
    // Seed the store with a head that already matches this machine, read once.
    harness.run(vec![row.clone()], sync::Trigger::Startup);
    let before = fakes::lock(&harness.world).head_reads;

    let outcome = harness.run(vec![row], sync::Trigger::LocalChange);
    assert_eq!(outcomes(&outcome), vec!["nothing:in_sync"]);
    assert_eq!(
        fakes::lock(&harness.world).head_reads,
        before,
        "a local change is answered from the heads already held",
    );
    assert!(!outcome.plan.refresh_remote.needed);
}

#[test]
fn a_head_this_machine_does_not_hold_is_downloaded_and_verified() {
    let harness = Harness::new();
    let head_bytes = vec![13_u8; 4096];
    {
        let mut world = fakes::lock(&harness.world);
        world.record = vec![record_head(&head_bytes, 3, fakes::FINGERPRINT)];
        world.objects.insert(
            fakes::download_url("export-tx_1", "xlsx"),
            head_bytes.clone(),
        );
    }
    let outcome = harness.run(Vec::new(), sync::Trigger::Navigation);

    assert_eq!(outcomes(&outcome), vec!["download:downloaded"]);
    let download = &outcome.receipts[0];
    assert_eq!(download.bytes, Some(4_096));
    assert_eq!(download.head_revision, Some(3));
    assert_eq!(
        fakes::lock(&harness.world).downloads,
        vec!["export-tx_1".to_string()]
    );
}

#[test]
fn a_download_of_corrupted_bytes_fails_rather_than_placing_them() {
    let harness = Harness::new();
    let head_bytes = vec![14_u8; 4096];
    {
        let mut world = fakes::lock(&harness.world);
        world.record = vec![record_head(&head_bytes, 4, fakes::FINGERPRINT)];
        // The record declares the true digest; the storage host serves other
        // bytes. The verified read must refuse them.
        world
            .objects
            .insert(fakes::download_url("export-tx_1", "xlsx"), vec![0_u8; 4096]);
    }
    let outcome = harness.run(Vec::new(), sync::Trigger::Navigation);
    let download = &outcome.receipts[0];
    assert_eq!(download.action, "download");
    assert_eq!(download.outcome, "failed");
}

// ---------------------------------------------------------------------------
// The server-shaped host: store-only producer, no gateway wired yet, verified
// reads. This proves a Tauri-free host can execute the shared loop; a live
// native gateway and producer must be wired before claiming server cloud sync.
// ---------------------------------------------------------------------------

/// Build a server-shaped host over this harness's store.
fn server_host<'a>(
    harness: &'a Harness,
    gateway: &'a dyn ds_sync_runtime::Gateway,
    producer: &'a ds_sync_runtime::StoreOnlyProducer,
    reads: &'a ds_sync_runtime::VerifiedReads,
    online: &'a dyn Fn() -> bool,
) -> StoreHost<'a> {
    StoreHost::new(
        harness.store.clone(),
        fence(),
        "server-worker-1".into(),
        PROJECT,
        gateway,
        producer,
        reads,
        online,
    )
    .expect("server host")
}

#[test]
fn a_server_host_executes_the_loop_and_refuses_to_upload_without_a_gateway() {
    let harness = Harness::new();
    // A row the store already holds (as a verified handoff would leave one) and
    // that the record does not, so the plan wants an upload.
    let bytes = vec![21_u8; 4096];
    seed_store_only_row(&harness, &bytes);

    let gateway = ds_sync_runtime::UnavailableGateway {
        reason: "no native gateway credential is wired to this server yet".into(),
    };
    let producer = ds_sync_runtime::StoreOnlyProducer;
    let reads = ds_sync_runtime::VerifiedReads::new(harness.reads_root.clone());
    let online = || true;
    let host = server_host(&harness, &gateway, &producer, &reads, &online);
    // A local change is answered from held heads (no record read), so the loop
    // runs end to end and reaches the upload, where the missing gateway makes
    // opening a grant fail — nothing is invented, nothing is committed.
    let outcome = run(&host, PROJECT, sync::Trigger::LocalChange).expect("the server loop runs");

    assert_eq!(
        outcome
            .receipts
            .iter()
            .map(|receipt| format!("{}:{}", receipt.action, receipt.outcome))
            .collect::<Vec<_>>(),
        vec!["open_grant:failed", "upload:grant_open_failed"],
        "a server with no gateway refuses cleanly, in order, naming the gateway",
    );
    let world = fakes::lock(&harness.world);
    assert_eq!(world.commits, 0, "the server moved no bytes");
    assert_eq!(world.grants_minted, 0);
}

#[test]
fn a_server_startup_without_a_gateway_fails_loudly_rather_than_proceeding_blind() {
    let harness = Harness::new();
    let gateway = ds_sync_runtime::UnavailableGateway {
        reason: "no native gateway credential is wired to this server yet".into(),
    };
    let producer = ds_sync_runtime::StoreOnlyProducer;
    let reads = ds_sync_runtime::VerifiedReads::new(harness.reads_root.clone());
    let online = || true;
    let host = server_host(&harness, &gateway, &producer, &reads, &online);
    // Startup must re-read the record; without a gateway that read fails, and
    // the run returns the reason rather than acting on a record it never read.
    let error = run(&host, PROJECT, sync::Trigger::Startup).expect_err("startup needs the record");
    assert!(
        error.contains("no native gateway credential is wired"),
        "{error}"
    );
}

#[test]
fn a_server_host_records_a_handoff_and_survives_restart() {
    let harness = Harness::new();
    let gateway = ds_sync_runtime::UnavailableGateway {
        reason: "no native gateway credential is wired to this server yet".into(),
    };
    // A legacy per-user outbox row handed to the server. Bytes are "verified"
    // by the harness (the server's own producer store would do this live).
    let producer = ds_sync_runtime::StoreOnlyProducer;
    let reads = ds_sync_runtime::VerifiedReads::new(harness.reads_root.clone());
    let online = || true;
    let host = StoreHost::new(
        harness.store.clone(),
        fence(),
        "server-worker-1".into(),
        PROJECT,
        &gateway,
        &producer,
        &reads,
        &online,
    )
    .expect("server host");
    let legacy = serde_json::json!({
        "id": "legacy-1",
        "owner_uid": "account-1",
        "project_id": PROJECT,
        "engine": "network_reporter",
        "operation": "export-tx_1",
        "variant": "default",
        "engine_version": fakes::RELEASE,
        "input_base_fingerprint": fakes::FINGERPRINT,
        "client_publish_id": "publish-legacy-1",
        "status": "synced",
        "outputs": [{
            "output_id": "xlsx",
            "local_path": "network_reporter:batch:publish-legacy-1",
            "sha256": format!("{:x}", sha2::Sha256::digest([21_u8; 4096])),
            "size_bytes": 4096,
        }],
    });
    let result = host
        .handoff(vec![legacy], &|_row| {
            ds_sync_runtime::store::BytesEvidence::Verified {
                locator: "network_reporter:batch:publish-legacy-1".into(),
            }
        })
        .expect("handoff");
    let acknowledged = result["acknowledged"].as_array().expect("acknowledged");
    assert_eq!(acknowledged.len(), 1, "the one row is acknowledged");
    assert_eq!(acknowledged[0]["outcome"], "imported");

    // A restart: the projects to resume are read from the durable store, and
    // the imported row is present under this fence.
    let projects = projects_of_fence(&harness.store, &fence()).expect("projects");
    assert_eq!(projects, vec![PROJECT.to_string()]);

    // The store-read shows the imported artifact under this fence — a fresh
    // host over the same durable store, the way a restart gets one.
    let store_read = {
        let producer = ds_sync_runtime::StoreOnlyProducer;
        let reads = ds_sync_runtime::VerifiedReads::new(harness.reads_root.clone());
        let host = StoreHost::new(
            harness.store.clone(),
            fence(),
            "server-worker-2".into(),
            PROJECT,
            &gateway,
            &producer,
            &reads,
            &online,
        )
        .expect("server host");
        host.store_read().expect("store read")
    };
    assert!(
        store_read["artifacts"].as_array().is_some_and(|rows| rows
            .iter()
            .any(|row| row["client_publish_id"] == "publish-legacy-1")),
        "the imported publication is durable across the restart",
    );
}

/// Seed one held artifact row directly, as a verified handoff would leave it.
fn seed_store_only_row(harness: &Harness, bytes: &[u8]) {
    let gateway = ds_sync_runtime::UnavailableGateway {
        reason: "seed".into(),
    };
    let producer = ds_sync_runtime::StoreOnlyProducer;
    let reads = ds_sync_runtime::VerifiedReads::new(harness.reads_root.clone());
    let online = || true;
    let host = StoreHost::new(
        harness.store.clone(),
        fence(),
        "seed-worker".into(),
        PROJECT,
        &gateway,
        &producer,
        &reads,
        &online,
    )
    .expect("seed host");
    let legacy = serde_json::json!({
        "id": "seed-1",
        "owner_uid": "account-1",
        "project_id": PROJECT,
        "engine": "network_reporter",
        "operation": "export-tx_1",
        "variant": "default",
        "engine_version": fakes::RELEASE,
        "input_base_fingerprint": fakes::FINGERPRINT,
        "client_publish_id": "publish-seed-1",
        "status": "held",
        "outputs": [{
            "output_id": "xlsx",
            "local_path": "network_reporter:batch:publish-seed-1",
            "sha256": format!("{:x}", sha2::Sha256::digest(bytes)),
            "size_bytes": bytes.len(),
        }],
    });
    host.handoff(vec![legacy], &|_row| {
        ds_sync_runtime::store::BytesEvidence::Verified {
            locator: "network_reporter:batch:publish-seed-1".into(),
        }
    })
    .expect("seed handoff");
}

#[test]
fn the_store_read_projection_hides_transfer_state_and_carries_receipts() {
    let harness = Harness::new();
    let bytes = vec![15_u8; 4096];
    harness.run(vec![local_row(&bytes)], sync::Trigger::Startup);

    let gateway = FakeGateway {
        world: harness.world.clone(),
    };
    let producer = FakeProducer::new(harness.world.clone(), vec![local_row(&bytes)]);
    let reads = FakeReads {
        world: harness.world.clone(),
        root: harness.reads_root.clone(),
    };
    let online = || true;
    let host = StoreHost::new(
        harness.store.clone(),
        fence(),
        "install-1#100".into(),
        PROJECT,
        &gateway,
        &producer,
        &reads,
        &online,
    )
    .expect("host");
    let read = host.store_read().expect("store read");
    assert_eq!(read["schema"], "ds.sync-store-read/v1");
    assert!(read["artifacts"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty()));
    for artifact in read["artifacts"].as_array().unwrap() {
        assert!(
            artifact.get("transfer_state").is_none(),
            "the transport's in-flight state never leaves the store read",
        );
    }
    assert!(read["grants"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty()));
}

#[test]
fn two_runs_in_one_process_do_not_share_a_lease_identity() {
    use ds_sync_runtime::SyncHost;
    let h = Harness::new();
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = FakeProducer::new(h.world.clone(), vec![]);
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let one = StoreHost::new(
        h.store.clone(),
        fence(),
        "same-process".into(),
        PROJECT,
        &gateway,
        &producer,
        &reads,
        &online,
    )
    .unwrap();
    let two = StoreHost::new(
        h.store.clone(),
        fence(),
        "same-process".into(),
        PROJECT,
        &gateway,
        &producer,
        &reads,
        &online,
    )
    .unwrap();
    assert_ne!(one.worker_id(), two.worker_id());
    assert!(one.take_lease(PROJECT).unwrap().is_none());
    assert!(two.take_lease(PROJECT).unwrap().is_some());
    one.release_lease(PROJECT);
    assert!(two.take_lease(PROJECT).unwrap().is_none());
    two.release_lease(PROJECT);
}

#[test]
fn authority_loss_during_transfer_prevents_publication() {
    use std::sync::atomic::AtomicBool;
    struct RevokingProducer(FakeProducer, Arc<AtomicBool>);
    impl Producer for RevokingProducer {
        fn inventory(&self, project: &str) -> Result<Vec<LocalRow>, String> {
            self.0.inventory(project)
        }
        fn transfer(
            &self,
            row: &LocalRow,
            output: &str,
            uri: &str,
        ) -> Result<TransferReceipt, String> {
            let receipt = self.0.transfer(row, output, uri)?;
            self.1.store(false, Ordering::SeqCst);
            Ok(receipt)
        }
        fn reclaim(&self, project: &str, row: &LocalRow) -> Result<u64, String> {
            self.0.reclaim(project, row)
        }
    }
    let h = Harness::new();
    let allowed = Arc::new(AtomicBool::new(true));
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = RevokingProducer(
        FakeProducer::new(h.world.clone(), vec![local_row(&[7; 100])]),
        allowed.clone(),
    );
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || allowed.load(Ordering::SeqCst);
    let host = StoreHost::new(
        h.store.clone(),
        fence(),
        "worker".into(),
        PROJECT,
        &gateway,
        &producer,
        &reads,
        &online,
    )
    .unwrap();
    let result = run(&host, PROJECT, sync::Trigger::Startup).unwrap();
    assert!(result
        .receipts
        .iter()
        .all(|receipt| receipt.outcome != "published"));
    assert_eq!(fakes::lock(&h.world).commits, 0);
}

#[test]
fn remote_output_names_cannot_escape_or_replace_head_metadata() {
    for name in [
        "../outside",
        "/outside",
        "head.json",
        "HEAD.JSON",
        "file:stream",
        "a\\b",
    ] {
        let h = Harness::new();
        let mut head = record_head(&[7; 100], 3, fakes::FINGERPRINT);
        head["outputs"][0]["output_id"] = serde_json::json!(name);
        fakes::lock(&h.world).record = vec![head];
        let result = h.run(vec![], sync::Trigger::Navigation);
        assert!(
            result
                .receipts
                .iter()
                .any(|r| r.action == "download" && r.outcome == "failed"),
            "{name}"
        );
        assert!(fakes::lock(&h.world).downloads.is_empty(), "{name}");
    }
}

#[test]
fn downloaded_replicas_survive_restart_and_never_become_publications() {
    let h = Harness::new();
    {
        let mut world = fakes::lock(&h.world);
        world.record = vec![record_head(&[5; 100], 3, "first")];
        world
            .objects
            .insert(fakes::download_url("export-tx_1", "xlsx"), vec![5; 100]);
    }
    let first = h.run(vec![], sync::Trigger::Startup);
    assert_eq!(first.replanned.summary.downloads, 0);
    let reopened = h.run(vec![], sync::Trigger::Startup);
    assert_eq!(reopened.plan.summary.downloads, 0);
    assert_eq!(fakes::lock(&h.world).downloads.len(), 1);
    assert!(
        h.store
            .lock()
            .unwrap()
            .artifacts_of_fence(&fence())
            .unwrap()
            .is_empty(),
        "a replica is never a producer row"
    );
    {
        let mut world = fakes::lock(&h.world);
        world.record = vec![record_head(&[6; 100], 4, "next")];
        world
            .objects
            .insert(fakes::download_url("export-tx_1", "xlsx"), vec![6; 100]);
    }
    let newer = h.run(vec![], sync::Trigger::Startup);
    assert_eq!(
        newer.plan.summary.downloads, 1,
        "a new head replaces an old replica without a false conflict"
    );
    assert_eq!(newer.replanned.summary.downloads, 0);
    fakes::lock(&h.world).record.clear();
    let removed = h.run(vec![], sync::Trigger::Startup);
    assert_eq!(removed.plan.summary.uploads, 0);
    let world = fakes::lock(&h.world);
    assert_eq!(world.grants_minted, 0);
    assert_eq!(
        world.commits, 0,
        "removed remote data must not be republished from a cache"
    );
    assert!(world.transfers.is_empty());
}

#[test]
fn a_corrupted_replica_is_fetched_again_without_publishing_it() {
    let h = Harness::new();
    {
        let mut world = fakes::lock(&h.world);
        world.record = vec![record_head(&[5; 100], 3, "first")];
        world
            .objects
            .insert(fakes::download_url("export-tx_1", "xlsx"), vec![5; 100]);
    }
    h.run(vec![], sync::Trigger::Startup);
    let file = h
        .reads_root
        .join(PROJECT)
        .join("network_reporter/export-tx_1/default/xlsx");
    std::fs::write(&file, [0; 100]).unwrap();
    let repaired = h.run(vec![], sync::Trigger::LocalChange);
    assert_eq!(repaired.plan.summary.downloads, 1);
    assert_eq!(repaired.plan.summary.uploads, 0);
    assert_eq!(std::fs::read(&file).unwrap(), vec![5; 100]);
    assert_eq!(fakes::lock(&h.world).downloads.len(), 2);
}

#[test]
fn report_pass_does_not_send_solar_through_the_work_grant_protocol() {
    let harness = Harness::new();
    let gateway = FakeGateway {
        world: harness.world.clone(),
    };
    let rows = vec![local_row(b"report"), solar_row()];
    harness.seal(&rows);
    let producer = FakeProducer::new(harness.world.clone(), rows);
    let reads = FakeReads {
        world: harness.world.clone(),
        root: harness.reads_root.clone(),
    };
    let host = StoreHost::new(
        harness.store.clone(),
        fence(),
        "reports".into(),
        PROJECT,
        &gateway,
        &producer,
        &reads,
        &|| true,
    )
    .unwrap();
    let result = ds_sync_runtime::run::run_reports(&host, PROJECT, sync::Trigger::Startup).unwrap();
    assert!(result
        .receipts
        .iter()
        .any(|receipt| receipt.action == "upload" && receipt.outcome == "published"));
    assert!(result.receipts.iter().all(|receipt| receipt
        .identity
        .as_ref()
        .is_none_or(|identity| identity.engine == "network_reporter")));
    let snapshot = host.store_read().unwrap();
    let solar = snapshot["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["engine"] == "solar")
        .unwrap();
    assert_eq!(solar["state"], "held");

    // Local recovery ticks must not turn a completed report into a remote
    // refresh or another publication, even with a held Solar row beside it.
    let before = {
        let world = fakes::lock(&harness.world);
        (
            world.head_reads,
            world.grants_minted,
            world.commits,
            world.transfers.len(),
        )
    };
    for _ in 0..3 {
        let recovery =
            ds_sync_runtime::run::run_reports(&host, PROJECT, sync::Trigger::LocalChange).unwrap();
        assert!(!recovery.plan.refresh_remote.needed);
    }
    let world = fakes::lock(&harness.world);
    assert_eq!(
        (
            world.head_reads,
            world.grants_minted,
            world.commits,
            world.transfers.len()
        ),
        before
    );
}

#[test]
fn head_inventory_crosses_the_services_hundred_row_page_without_truncation() {
    use ds_sync_runtime::SyncHost;
    let h = Harness::new();
    fakes::lock(&h.world).record = (0..101)
        .map(|index| {
            let mut head = record_head(&[7; 100], 3, fakes::FINGERPRINT);
            head["operation"] = serde_json::json!(format!("export-tx_{index}"));
            head
        })
        .collect();
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = FakeProducer::new(h.world.clone(), vec![]);
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let host = StoreHost::new(
        h.store.clone(),
        fence(),
        "install-1#100".into(),
        PROJECT,
        &gateway,
        &producer,
        &reads,
        &online,
    )
    .unwrap();
    let heads = host.heads(PROJECT).unwrap();
    assert_eq!(heads.len(), 101);
    assert_eq!(heads.first().unwrap().identity.operation, "export-tx_0");
    assert_eq!(heads.last().unwrap().identity.operation, "export-tx_100");
    assert_eq!(heads[0].revision, "head-3");
    assert_eq!(
        heads[0].input_base_fingerprint.as_deref(),
        Some(fakes::FINGERPRINT)
    );
    assert_eq!(fakes::lock(&h.world).head_reads, 2);
}

#[test]
fn a_report_from_current_inputs_replaces_an_older_output_after_admission() {
    let h = Harness::new();
    fakes::lock(&h.world).record = vec![record_head(b"older output", 1, "old-source")];
    let result = h.run(vec![local_row(b"new output")], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&result),
        vec!["open_grant:granted", "upload:published"]
    );
    assert_eq!(fakes::lock(&h.world).commits, 1);
}

#[test]
fn stale_report_inputs_are_refused_after_fresh_admission_before_any_upload() {
    let h = Harness::new();
    {
        let mut world = fakes::lock(&h.world);
        world
            .snapshot_base
            .insert("export-tx_1".into(), "source-changed".into());
    }
    let stale = local_row(b"stale output");
    let result = h.run(vec![stale.clone()], sync::Trigger::Startup);
    // The verdict is kept, then the bytes go: online overwrites local, and a
    // lost batch left on disk is the 350-batch, 2.08 GB, seven-day queue.
    assert_eq!(
        outcomes(&result),
        vec![
            "open_grant:granted",
            "conflict:source_base_moved",
            "reclaimed:source_base_moved"
        ]
    );
    assert_eq!(result.receipts[2].bytes, Some(stale.size_bytes));
    assert_eq!(
        result.reclaimed,
        ds_sync_runtime::Reclaimed {
            batches: 1,
            bytes: stale.size_bytes
        }
    );
    let world = fakes::lock(&h.world);
    assert_eq!(world.grants_minted, 1);
    assert_eq!(world.commits, 0);
    assert!(world.transfers.is_empty());
    assert!(
        world.declarations.is_empty(),
        "stale inputs must never be relabelled with the new grant"
    );
    assert_eq!(
        world.reclaims,
        vec![(stale.client_publish_id.clone(), stale.size_bytes)]
    );
    drop(world);

    // A fresh producer presents the same sealed bytes again (a machine whose
    // store already holds the verdict while the batch is still on disk): the
    // observation itself frees them, once, and no grant is reopened.
    let recovery = h.run(vec![stale.clone()], sync::Trigger::LocalChange);
    assert_eq!(
        outcomes(&recovery),
        vec!["reclaimed:conflict"],
        "the exact terminal conflict is not executed again; its bytes are freed"
    );
    assert_eq!(recovery.reclaimed.batches, 1);
    assert_eq!(
        fakes::lock(&h.world).grants_minted,
        1,
        "background recovery must not reopen authority for stale bytes"
    );
}

/// A host over a producer the caller keeps, so what a pass freed stays
/// freed for the next one — the way a real batch directory does.
fn host_over<'a>(
    h: &'a Harness,
    gateway: &'a FakeGateway,
    producer: &'a FakeProducer,
    reads: &'a FakeReads,
    online: &'a dyn Fn() -> bool,
) -> StoreHost<'a> {
    StoreHost::new(
        h.store.clone(),
        fence(),
        "install-1#200".into(),
        PROJECT,
        gateway,
        producer,
        reads,
        online,
    )
    .expect("host")
}

#[test]
fn a_row_the_store_already_holds_as_lost_is_reclaimed_on_observation_once_and_not_again() {
    // WHY: the owner's box held 86 rows the store had settled as `conflict`
    // and their 86 batches on disk, and every daily pass looked at them and
    // left them. Observing the inventory now frees them — through the
    // producer, once — and the second look finds nothing to do.
    let h = Harness::new();
    {
        let mut world = fakes::lock(&h.world);
        world
            .snapshot_base
            .insert("export-tx_1".into(), "source-changed".into());
    }
    let stale = local_row(b"stale output");
    // The store learns the verdict; the fake producer drops the batch on the
    // conflict, as the real one unlinks it.
    h.run(vec![stale.clone()], sync::Trigger::Startup);

    // The same batch is on disk again under the verdict (a store restored
    // beside its artifacts, or a re-export of identical bytes).
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = FakeProducer::new(h.world.clone(), vec![stale.clone()]);
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let host = host_over(&h, &gateway, &producer, &reads, &online);
    fakes::lock(&h.world).reclaims.clear();

    let first = host.local(PROJECT).expect("observe");
    assert!(
        first
            .iter()
            .all(|row| row.client_publish_id != stale.client_publish_id),
        "a lost row never re-enters the plan"
    );
    let receipts = host.take_receipts(PROJECT);
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].action, "reclaimed");
    assert_eq!(receipts[0].outcome, "conflict");
    assert_eq!(receipts[0].bytes, Some(stale.size_bytes));
    assert!(
        producer.inventory(PROJECT).unwrap().is_empty(),
        "the bytes left the machine"
    );
    assert!(
        host.receipts(10)
            .unwrap()
            .iter()
            .any(|row| row.action == "reclaimed" && row.committed_bytes == Some(stale.size_bytes)),
        "the reclaim is kept in the store as evidence"
    );

    host.local(PROJECT).expect("observe again");
    assert!(
        host.take_receipts(PROJECT).is_empty(),
        "a batch already gone is neither a receipt nor an error"
    );
    assert_eq!(
        fakes::lock(&h.world).reclaims,
        vec![(stale.client_publish_id.clone(), stale.size_bytes)],
        "the producer was asked once with effect; the second ask found nothing"
    );

    // Through the loop: the pass reports what its observation freed.
    let producer = FakeProducer::new(h.world.clone(), vec![stale.clone()]);
    let host = host_over(&h, &gateway, &producer, &reads, &online);
    let pass = run(&host, PROJECT, sync::Trigger::Manual).expect("the loop runs");
    assert_eq!(outcomes(&pass), vec!["reclaimed:conflict"]);
    assert_eq!(pass.reclaimed.batches, 1);
    assert_eq!(pass.reclaimed.bytes, stale.size_bytes);
    assert!(
        pass.idle()
            .expect("nothing moved")
            .contains("1 batch reclaimed"),
        "a pass that moved nothing still says what it did: {:?}",
        pass.idle()
    );
    let again = run(&host, PROJECT, sync::Trigger::Manual).expect("the loop runs");
    assert!(again.receipts.is_empty());
    assert_eq!(again.reclaimed, ds_sync_runtime::Reclaimed::default());
    assert!(
        again.idle().expect("nothing moved").contains("0 receipts"),
        "an idle pass is never a silent one"
    );
}

#[test]
fn a_published_batch_is_this_machines_copy_of_the_head_and_is_never_reclaimed() {
    // WHY: after `published` the sealed batch's digest IS the head's; it is
    // the readable local copy, not queued work. Reclaiming it would make
    // every publish delete what it just published.
    let h = Harness::new();
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let row = local_row(b"published output");
    h.seal(std::slice::from_ref(&row));
    let producer = FakeProducer::new(h.world.clone(), vec![row.clone()]);
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let host = host_over(&h, &gateway, &producer, &reads, &online);
    let first = run(&host, PROJECT, sync::Trigger::Startup).expect("the loop runs");
    assert_eq!(
        outcomes(&first),
        vec!["open_grant:granted", "upload:published"]
    );
    let second = run(&host, PROJECT, sync::Trigger::Manual).expect("the loop runs");
    assert_eq!(outcomes(&second), vec!["nothing:in_sync"]);
    assert_eq!(second.reclaimed, ds_sync_runtime::Reclaimed::default());
    assert_eq!(
        producer.inventory(PROJECT).unwrap(),
        vec![row],
        "the published batch stays on this machine"
    );
    assert!(
        fakes::lock(&h.world).reclaims.is_empty(),
        "no reclaim was even asked for"
    );
    assert_eq!(second.plan.summary.in_sync, 1);
    assert!(second.idle().expect("in sync").contains("1 in sync"));
}

#[test]
fn a_result_superseded_by_the_head_is_reclaimed_with_its_verdict() {
    // WHY: the second way a batch loses. The record already carries a later
    // computation of the same source; the local one is not a conflict and
    // not an upload, and until now it was also not removed.
    let h = Harness::new();
    fakes::lock(&h.world).record = vec![record_head(b"later output", 3, fakes::FINGERPRINT)];
    let older = local_row(b"older output");
    let result = h.run(vec![older.clone()], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&result),
        vec![
            "conflict:superseded_by_head",
            "reclaimed:superseded_by_head"
        ]
    );
    assert_eq!(result.receipts[1].bytes, Some(older.size_bytes));
    assert_eq!(result.reclaimed.batches, 1);
    assert_eq!(fakes::lock(&h.world).commits, 0);
}

#[test]
fn a_reclaim_the_producer_cannot_perform_is_reported_and_never_silent() {
    // WHY: the producer store's lock can be held, a verification can fail.
    // The verdict still stands and the bytes still wait; the pass says so
    // and the next one tries again in the open.
    let h = Harness::new();
    {
        let mut world = fakes::lock(&h.world);
        world
            .snapshot_base
            .insert("export-tx_1".into(), "source-changed".into());
        world.refuse_reclaim = Some("the pending-upload lock is held by pid 7".into());
    }
    let stale = local_row(b"stale output");
    let result = h.run(vec![stale.clone()], sync::Trigger::Startup);
    // The action's reclaim fails; the closing observation of the inventory
    // meets the same row under the store's verdict and tries again, in the
    // open. Two attempts, two receipts, nothing hidden.
    assert_eq!(
        outcomes(&result),
        vec![
            "open_grant:granted",
            "conflict:source_base_moved",
            "reclaim_failed:source_base_moved",
            "reclaim_failed:conflict"
        ]
    );
    assert!(result.receipts[2]
        .detail
        .as_deref()
        .unwrap()
        .contains("lock is held by pid 7"));
    assert_eq!(result.reclaimed, ds_sync_runtime::Reclaimed::default());
    assert!(
        host_receipt_actions(&h).contains(&"reclaim_failed".to_string()),
        "the failure is kept in the store, not only in this pass's answer"
    );
}

fn host_receipt_actions(h: &Harness) -> Vec<String> {
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = FakeProducer::new(h.world.clone(), Vec::new());
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let host = host_over(h, &gateway, &producer, &reads, &online);
    host.receipts(50)
        .expect("receipts")
        .iter()
        .map(|row| row.action.clone())
        .collect()
}

#[test]
fn a_changed_source_reopens_a_held_grant_before_publishing_new_reports() {
    let h = Harness::new();
    h.run(vec![local_row(b"first output")], sync::Trigger::Startup);
    fakes::lock(&h.world)
        .snapshot_base
        .insert("export-tx_1".into(), "new-source".into());
    let mut next = local_row(b"next output");
    next.input_base_fingerprint = Some("new-source".into());
    next.client_publish_id = "publish-22222222-2222-4222-8222-222222222222".into();
    let result = h.run(vec![next], sync::Trigger::LocalChange);
    assert_eq!(
        outcomes(&result),
        vec!["open_grant:granted", "upload:published"]
    );
    assert_eq!(fakes::lock(&h.world).commits, 2);
    assert_eq!(fakes::lock(&h.world).grants_minted, 2);
}

// ---------------------------------------------------------------------------
// "Reliably synced": the seven invariants, each driven through the real
// StoreHost over a real store on disk. (5) — fences — is in
// ds-sync-store/tests/store.rs, where the fence is persisted.
// ---------------------------------------------------------------------------

fn stored_rows(h: &Harness, project: &str) -> Vec<ds_sync_runtime::store::ArtifactRow> {
    h.store
        .lock()
        .unwrap()
        .snapshot(
            &fence(),
            &ds_sync_runtime::store::Scope::Project {
                project: project.into(),
            },
        )
        .unwrap()
        .artifacts
}

/// Invariant (1): a pass killed after the seal, after the transfer, or
/// before the acknowledgement, then restarted against the same store,
/// neither duplicates nor loses the publication.
#[test]
fn invariant_1_a_pass_killed_after_seal_after_transfer_or_before_ack_resumes_without_duplicating() {
    use ds_sync_runtime::store::ArtifactState;
    // (a) Killed after the seal, before any pass: the row is held in the
    // store and the next process publishes it once.
    let h = Harness::new();
    let row = local_row(b"sealed then killed");
    h.seal(std::slice::from_ref(&row));
    assert_eq!(stored_rows(&h, PROJECT)[0].state, ArtifactState::Held);
    let resumed = h.run(vec![row.clone()], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&resumed),
        vec!["open_grant:granted", "upload:published"]
    );
    assert_eq!(fakes::lock(&h.world).commits, 1);

    // (b) Killed after the transfer, before the restated declaration is
    // acknowledged: the bytes are in the session, the record has not
    // advanced, the row is held with the failure as its reason.
    let h = Harness::new();
    fakes::lock(&h.world).refuse_publish_from = Some(1);
    let row = local_row(b"transferred then killed");
    let first = h.run(vec![row.clone()], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&first),
        vec!["open_grant:granted", "upload:failed"]
    );
    {
        let world = fakes::lock(&h.world);
        assert_eq!(world.transfers.len(), 1, "the bytes went up");
        assert_eq!(world.commits, 0, "the record did not advance");
    }
    let rows = stored_rows(&h, PROJECT);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, ArtifactState::Held);
    assert!(rows[0]
        .state_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("went away")));
    // The restart: the gateway is back. The same declaration is restated,
    // the session is already committed, nothing is transferred twice.
    fakes::lock(&h.world).refuse_publish_from = None;
    let second = h.run(vec![row.clone()], sync::Trigger::Startup);
    assert!(
        outcomes(&second).contains(&"upload:published".to_string()),
        "{:?}",
        outcomes(&second)
    );
    let world = fakes::lock(&h.world);
    assert_eq!(world.commits, 1, "exactly one commit across the two passes");
    assert_eq!(
        world.transfers.len(),
        1,
        "the bytes were not transferred twice"
    );
    drop(world);
    let rows = stored_rows(&h, PROJECT);
    assert_eq!(rows.len(), 1, "one row, not two");
    assert_eq!(rows[0].state, ArtifactState::Published);
    // (c) A third pass finds it in sync: nothing is lost, nothing repeats.
    let third = h.run(vec![row], sync::Trigger::Manual);
    assert_eq!(outcomes(&third), vec!["nothing:in_sync"]);
    assert_eq!(fakes::lock(&h.world).commits, 1);
}

/// Invariant (2): the same `client_publish_id` published twice is one row,
/// and the second pass answers `nothing:in_sync`.
#[test]
fn invariant_2_the_same_publish_id_published_twice_is_one_row_and_in_sync() {
    let h = Harness::new();
    let row = local_row(b"published twice");
    h.seal(std::slice::from_ref(&row));
    // The seal again — a retried export of the same bytes — is refused as
    // already recorded and changes nothing.
    let refused = h
        .store
        .lock()
        .unwrap()
        .apply(
            &fence(),
            2_000,
            ds_sync_runtime::store::Event::LocalProduced {
                row: ds_sync_runtime::rows::store_row(PROJECT, &row, 2_000),
            },
        )
        .unwrap();
    assert_eq!(refused.refusals[0].code, "already_recorded");
    assert!(refused.changes.is_empty());
    let first = h.run(vec![row.clone()], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&first),
        vec!["open_grant:granted", "upload:published"]
    );
    let second = h.run(vec![row.clone()], sync::Trigger::Manual);
    assert_eq!(outcomes(&second), vec!["nothing:in_sync"]);
    assert_eq!(second.replanned.summary.in_sync, 1);
    assert_eq!(stored_rows(&h, PROJECT).len(), 1);
    let world = fakes::lock(&h.world);
    assert_eq!(world.commits, 1);
    assert_eq!(world.grants_minted, 1);
    assert_eq!(world.transfers.len(), 1);
}

/// Invariant (4): two projects' rows interleave in one store, and each pass
/// publishes under its own scope only.
#[test]
fn invariant_4_two_projects_rows_interleave_and_each_publishes_under_its_own_scope_only() {
    use ds_sync_runtime::store::ArtifactState;
    const OTHER: &str = "p2";
    let h = Harness::new();
    let mine = local_row(b"project one's report");
    let mut theirs = local_row(b"project two's report");
    theirs.client_publish_id = "publish-22222222-2222-4222-8222-222222222222".into();
    theirs.bytes_locator = "fake:batch:publish-22222222-2222-4222-8222-222222222222".into();
    // Interleaved: theirs is sealed under its own scope between two of ours.
    h.seal(std::slice::from_ref(&mine));
    h.store
        .lock()
        .unwrap()
        .apply(
            &fence(),
            1_001,
            ds_sync_runtime::store::Event::LocalProduced {
                row: ds_sync_runtime::rows::store_row(OTHER, &theirs, 1_001),
            },
        )
        .unwrap();
    let queue = h
        .store
        .lock()
        .unwrap()
        .queue(&fence(), None, 2_000)
        .unwrap();
    assert_eq!(queue.queued_batches, 2);
    assert_eq!(queue.projects.len(), 2);

    // Our pass: our row publishes, theirs is untouched.
    let ours = h.run(vec![mine.clone()], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&ours),
        vec!["open_grant:granted", "upload:published"]
    );
    assert_eq!(stored_rows(&h, PROJECT)[0].state, ArtifactState::Published);
    assert_eq!(stored_rows(&h, OTHER)[0].state, ArtifactState::Held);
    {
        let world = fakes::lock(&h.world);
        assert_eq!(world.commits, 1);
        assert!(
            world
                .declarations
                .iter()
                .all(|declaration| { declaration["client_publish_id"] == mine.client_publish_id }),
            "only our publication was declared"
        );
    }
    // Their pass, through a host bound to their project on the SAME store.
    // The fake record is one project's (its heads are not project-keyed),
    // so the other project gets its own record; the store is what is shared.
    let their_world = fakes::world();
    let gateway = FakeGateway {
        world: their_world.clone(),
    };
    let producer = FakeProducer::new(their_world.clone(), vec![theirs.clone()]);
    let reads = FakeReads {
        world: their_world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let host = StoreHost::new(
        h.store.clone(),
        fence(),
        "install-1#300".into(),
        OTHER,
        &gateway,
        &producer,
        &reads,
        &online,
    )
    .unwrap();
    let theirs_run = run(&host, OTHER, sync::Trigger::Startup).unwrap();
    assert_eq!(
        outcomes(&theirs_run),
        vec!["open_grant:granted", "upload:published"]
    );
    assert_eq!(stored_rows(&h, OTHER)[0].state, ArtifactState::Published);
    assert_eq!(
        fakes::lock(&h.world).commits,
        1,
        "our record saw nothing of theirs"
    );
    let world = fakes::lock(&their_world);
    assert_eq!(world.commits, 1);
    assert!(world
        .declarations
        .iter()
        .all(|declaration| { declaration["client_publish_id"] == theirs.client_publish_id }));
    drop(world);
    let queue = h
        .store
        .lock()
        .unwrap()
        .queue(&fence(), None, 3_000)
        .unwrap();
    assert_eq!(queue.queued_batches, 0);
    assert_eq!(queue.held_batches, 2);
}

/// Invariant (6): a run aborted between receipts — the host loses its
/// authority after the transfer, before the commit — leaves the row held,
/// never published, and the next pass resumes it.
#[test]
fn invariant_6_a_run_aborted_between_receipts_leaves_rows_held_and_the_next_pass_resumes() {
    use ds_sync_runtime::store::ArtifactState;
    use std::sync::atomic::AtomicBool;
    struct AbortingProducer(FakeProducer, Arc<AtomicBool>);
    impl Producer for AbortingProducer {
        fn inventory(&self, project: &str) -> Result<Vec<LocalRow>, String> {
            self.0.inventory(project)
        }
        fn transfer(
            &self,
            row: &LocalRow,
            output: &str,
            uri: &str,
        ) -> Result<TransferReceipt, String> {
            let receipt = self.0.transfer(row, output, uri)?;
            // The abort: the host's authority is gone from here on.
            self.1.store(false, Ordering::SeqCst);
            Ok(receipt)
        }
        fn reclaim(&self, project: &str, row: &LocalRow) -> Result<u64, String> {
            self.0.reclaim(project, row)
        }
    }
    let h = Harness::new();
    let row = local_row(b"aborted mid-pass");
    h.seal(std::slice::from_ref(&row));
    let allowed = Arc::new(AtomicBool::new(true));
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = AbortingProducer(
        FakeProducer::new(h.world.clone(), vec![row.clone()]),
        allowed.clone(),
    );
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || allowed.load(Ordering::SeqCst);
    let host = StoreHost::new(
        h.store.clone(),
        fence(),
        "worker".into(),
        PROJECT,
        &gateway,
        &producer,
        &reads,
        &online,
    )
    .unwrap();
    let aborted = run(&host, PROJECT, sync::Trigger::Startup).unwrap();
    assert!(
        aborted
            .receipts
            .iter()
            .all(|receipt| receipt.outcome != "published"),
        "{:?}",
        outcomes(&aborted)
    );
    let rows = stored_rows(&h, PROJECT);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, ArtifactState::Held, "held, never published");
    assert_eq!(fakes::lock(&h.world).commits, 0);
    drop(host);

    // The next pass, authority restored: it resumes and commits once.
    allowed.store(true, Ordering::SeqCst);
    let resumed = h.run(vec![row], sync::Trigger::Startup);
    assert!(
        outcomes(&resumed).contains(&"upload:published".to_string()),
        "{:?}",
        outcomes(&resumed)
    );
    assert_eq!(stored_rows(&h, PROJECT)[0].state, ArtifactState::Published);
    assert_eq!(fakes::lock(&h.world).commits, 1);
}

/// Invariant (7): `published` is set only from the publish-work commit
/// receipt, never from transport success. A transfer that succeeds while
/// the record never commits leaves the row held and no head.
#[test]
fn invariant_7_transfer_success_without_a_commit_receipt_leaves_the_row_held() {
    use ds_sync_runtime::store::ArtifactState;
    let h = Harness::new();
    fakes::lock(&h.world).never_commit = true;
    let row = local_row(b"uploaded, never committed");
    let result = h.run(vec![row.clone()], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&result),
        vec!["open_grant:granted", "upload:upload_required"],
        "the run stops at its bounded round ceiling without a commit"
    );
    {
        let world = fakes::lock(&h.world);
        assert!(!world.transfers.is_empty(), "the transport succeeded");
        assert_eq!(world.commits, 0, "the record never advanced");
        assert!(
            world.record.is_empty(),
            "no head was written from transport success"
        );
    }
    let rows = stored_rows(&h, PROJECT);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, ArtifactState::Held);
    assert!(h
        .store
        .lock()
        .unwrap()
        .snapshot(
            &fence(),
            &ds_sync_runtime::store::Scope::Project {
                project: PROJECT.into()
            }
        )
        .unwrap()
        .heads
        .is_empty());
    assert_eq!(result.replanned.summary.uploads, 1, "still queued");
}

/// Invariant (1), the restart itself: the process is killed — the store is
/// never closed — between the acknowledged seal and the acknowledgement of
/// the publication, and a new process opens the same file. The row the seal
/// acknowledged is there (WAL, `synchronous=FULL`), the pass converges to
/// one publication, and a further restart finds it in sync. This is the
/// reopen `invariant_1_..._resumes_without_duplicating` drives through one
/// connection, driven through the file.
#[test]
fn invariant_1_killed_between_seal_and_acknowledgement_the_reopened_store_holds_the_seal_and_converges(
) {
    use ds_sync_runtime::store::ArtifactState;
    // (a) Killed right after the seal was acknowledged: the new process
    // finds the row held on disk and publishes it once.
    let mut h = Harness::new();
    let row = local_row(b"sealed, then the power went");
    h.seal(std::slice::from_ref(&row));
    h.reopen_after_kill();
    let rows = stored_rows(&h, PROJECT);
    assert_eq!(rows.len(), 1, "the acknowledged seal reached the disk");
    assert_eq!(rows[0].state, ArtifactState::Held);
    assert_eq!(
        projects_of_fence(&h.store, &fence()).expect("projects"),
        vec![PROJECT.to_string()],
        "the new process learns the project to resume from the file"
    );
    let resumed = h.run(vec![row.clone()], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&resumed),
        vec!["open_grant:granted", "upload:published"]
    );
    assert_eq!(fakes::lock(&h.world).commits, 1);

    // (b) Killed after the bytes went up, before the record acknowledged:
    // the new process resumes from the held row, restates once, and the
    // bytes do not go up twice.
    let mut h = Harness::new();
    fakes::lock(&h.world).refuse_publish_from = Some(1);
    let row = local_row(b"transferred, then the power went");
    let first = h.run(vec![row.clone()], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&first),
        vec!["open_grant:granted", "upload:failed"]
    );
    h.reopen_after_kill();
    let rows = stored_rows(&h, PROJECT);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].state,
        ArtifactState::Held,
        "held on disk, never published by a send"
    );
    assert!(
        h.store
            .lock()
            .unwrap()
            .snapshot(&fence(), &ds_sync_runtime::rows::store_scope(PROJECT))
            .unwrap()
            .lease
            .is_none(),
        "the killed pass's lease was released with its host; a lease that outlived it would expire"
    );
    fakes::lock(&h.world).refuse_publish_from = None;
    let second = h.run(vec![row.clone()], sync::Trigger::Startup);
    assert!(
        outcomes(&second).contains(&"upload:published".to_string()),
        "{:?}",
        outcomes(&second)
    );
    {
        let world = fakes::lock(&h.world);
        assert_eq!(world.commits, 1, "one commit across the kill");
        assert_eq!(world.transfers.len(), 1, "the bytes went up once");
    }
    assert_eq!(stored_rows(&h, PROJECT)[0].state, ArtifactState::Published);

    // (c) Killed after the acknowledgement: the new process finds the
    // publication and repeats nothing.
    h.reopen_after_kill();
    assert_eq!(stored_rows(&h, PROJECT)[0].state, ArtifactState::Published);
    let third = h.run(vec![row], sync::Trigger::Startup);
    assert_eq!(outcomes(&third), vec!["nothing:in_sync"]);
    assert_eq!(fakes::lock(&h.world).commits, 1);
}

/// Invariant (5), at the runtime: a host under another fence — another
/// install of the same account, or another account — over the same file
/// sees none of this fence's rows, drains none of them, and its pass leaves
/// them held. The store-level fence is proved in `ds-sync-store`; this is
/// the pump honouring it.
#[test]
fn invariant_5_a_host_under_another_fence_drains_nothing_and_leaves_this_fences_rows_held() {
    use ds_sync_runtime::store::ArtifactState;
    let h = Harness::new();
    let row = local_row(b"this fence's queued report");
    h.seal(std::slice::from_ref(&row));
    let other_install = Fence {
        install_id: "install-2".into(),
        ..fence()
    };
    let other_account = Fence {
        account: "account-2".into(),
        ..fence()
    };
    for foreign in [other_install, other_account] {
        // Another install of the same owner, or another owner: its own
        // producer holds nothing for this project on this machine.
        let world = fakes::world();
        let gateway = FakeGateway {
            world: world.clone(),
        };
        let producer = FakeProducer::new(world.clone(), Vec::new());
        let reads = FakeReads {
            world: world.clone(),
            root: h.reads_root.clone(),
        };
        let online = || true;
        let host = StoreHost::new(
            h.store.clone(),
            foreign.clone(),
            "install-2#100".into(),
            PROJECT,
            &gateway,
            &producer,
            &reads,
            &online,
        )
        .unwrap();
        assert!(
            host.snapshot().unwrap().artifacts.is_empty(),
            "{foreign:?} sees no row of this fence"
        );
        assert_eq!(
            h.store
                .lock()
                .unwrap()
                .queue(&foreign, None, 2_000)
                .unwrap()
                .queued_batches,
            0,
            "{foreign:?} has an empty queue"
        );
        let pass = run(&host, PROJECT, sync::Trigger::Startup).unwrap();
        assert!(
            pass.receipts.is_empty(),
            "{foreign:?} has nothing to do: {:?}",
            outcomes(&pass)
        );
        assert_eq!(pass.plan.summary.uploads, 0, "{foreign:?} plans no upload");
        let world = fakes::lock(&world);
        assert_eq!(world.commits, 0, "{foreign:?} published nothing");
        assert_eq!(world.grants_minted, 0);
        assert!(world.transfers.is_empty());
    }
    // This fence's row is still queued, untouched, and the file's whole
    // queue — what a status with no identity reads — still counts it once.
    let rows = stored_rows(&h, PROJECT);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, ArtifactState::Held);
    let store = h.store.lock().unwrap();
    assert_eq!(
        store.queue(&fence(), None, 2_000).unwrap().queued_batches,
        1
    );
    assert_eq!(store.queue_all(None, 2_000).unwrap().queued_batches, 1);
    drop(store);
    // And this fence's own pass drains it once.
    let mine = h.run(vec![row], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&mine),
        vec!["open_grant:granted", "upload:published"]
    );
    assert_eq!(fakes::lock(&h.world).commits, 1);
}

/// Invariant (6), the cancellation and the logout: a cancel retires queued
/// work — the row is `refused:cancelled`, the next pass frees its bytes and
/// never sends them — and never undoes work the record acknowledged; a pass
/// with no authority (logged out, offline) moves nothing and loses nothing.
/// `invariant_6_a_run_aborted_between_receipts_...` is the abort mid-pass.
#[test]
fn invariant_6_cancel_retires_queued_work_frees_its_bytes_and_never_undoes_published_work() {
    use ds_sync_runtime::store::ArtifactState;
    let h = Harness::new();
    // Y: published and acknowledged. X: sealed after it, still queued.
    let published = local_row(b"published and acknowledged");
    let mut queued = local_row(b"queued, then cancelled");
    queued.identity.operation = "export-tx_2".into();
    queued.resource = "tx_2".into();
    queued.input_base_fingerprint = Some("source-2".into());
    queued.client_publish_id = "publish-22222222-2222-4222-8222-222222222222".into();
    queued.bytes_locator = "fake:batch:publish-22222222-2222-4222-8222-222222222222".into();
    fakes::lock(&h.world)
        .snapshot_base
        .insert("export-tx_2".into(), "source-2".into());
    let first = h.run(vec![published.clone()], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&first),
        vec!["open_grant:granted", "upload:published"]
    );
    h.seal(std::slice::from_ref(&queued));
    assert_eq!(
        h.store
            .lock()
            .unwrap()
            .queue(&fence(), None, 2_000)
            .unwrap()
            .queued_batches,
        1
    );

    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = FakeProducer::new(h.world.clone(), vec![published.clone(), queued.clone()]);
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let host = host_over(&h, &gateway, &producer, &reads, &online);

    // The logout: no authority, nothing moves, nothing is lost.
    let logged_out = || false;
    let signed_out = host_over(&h, &gateway, &producer, &reads, &logged_out);
    let offline = run(&signed_out, PROJECT, sync::Trigger::Manual).unwrap();
    assert_eq!(outcomes(&offline), vec!["offline:noted"]);
    drop(signed_out);
    assert_eq!(stored_rows(&h, PROJECT).len(), 2);
    assert!(stored_rows(&h, PROJECT)
        .iter()
        .any(|row| row.state == ArtifactState::Held));
    assert_eq!(
        fakes::lock(&h.world).transfers.len(),
        1,
        "nothing moved while signed out"
    );

    // The cancel retires the queued row, with a receipt.
    let cancelled = host.cancel(&queued.identity).unwrap();
    assert!(cancelled.refusals.is_empty(), "{:?}", cancelled.refusals);
    let retired = stored_rows(&h, PROJECT)
        .into_iter()
        .find(|row| row.identity == queued.identity)
        .unwrap();
    assert_eq!(retired.state, ArtifactState::Refused);
    assert_eq!(retired.state_reason.as_deref(), Some("cancelled"));
    assert!(
        host.receipts(10)
            .unwrap()
            .iter()
            .any(|row| row.action == "cancel" && row.outcome == "cancelled"),
        "the cancellation is kept as evidence"
    );
    // The acknowledged publication is never undone.
    let refused = host.cancel(&published.identity).unwrap();
    assert_eq!(refused.refusals[0].code, "not_cancellable");
    assert!(refused.changes.is_empty());
    assert_eq!(
        stored_rows(&h, PROJECT)
            .into_iter()
            .find(|row| row.identity == published.identity)
            .unwrap()
            .state,
        ArtifactState::Published
    );

    // The next pass: the cancelled bytes are freed through the producer,
    // never sent; the published copy stays this machine's copy of the head.
    let pass = run(&host, PROJECT, sync::Trigger::Manual).unwrap();
    assert_eq!(
        outcomes(&pass),
        vec!["reclaimed:refused", "nothing:in_sync"]
    );
    assert_eq!(pass.reclaimed.batches, 1);
    assert_eq!(pass.reclaimed.bytes, queued.size_bytes);
    {
        let world = fakes::lock(&h.world);
        assert_eq!(
            world.commits, 1,
            "only the acknowledged publication ever committed"
        );
        assert_eq!(
            world.transfers.len(),
            1,
            "the cancelled bytes never went up"
        );
        assert_eq!(
            world.reclaims,
            vec![(queued.client_publish_id.clone(), queued.size_bytes)]
        );
    }
    let inventory = producer.inventory(PROJECT).unwrap();
    assert_eq!(inventory.len(), 1);
    assert_eq!(inventory[0].client_publish_id, published.client_publish_id);
    let retired = stored_rows(&h, PROJECT)
        .into_iter()
        .find(|row| row.identity == queued.identity)
        .unwrap();
    assert_eq!(retired.state, ArtifactState::Refused, "the verdict is kept");
    assert!(!retired.readable, "the bytes are gone");
    let queue = h
        .store
        .lock()
        .unwrap()
        .queue(&fence(), None, 3_000)
        .unwrap();
    assert_eq!(queue.queued_batches, 0);
    assert_eq!(queue.held_batches, 1, "the published copy");
    assert_eq!(queue.reclaimable_batches, 0, "nothing left to free");
    // Once more: nothing repeats.
    let again = run(&host, PROJECT, sync::Trigger::Manual).unwrap();
    assert_eq!(outcomes(&again), vec!["nothing:in_sync"]);
    assert_eq!(fakes::lock(&h.world).commits, 1);
}

/// A batch on disk with no row — sealed before the seal wrote its row — is
/// adopted exactly once, with a receipt, and never again.
#[test]
fn a_batch_the_store_does_not_know_is_adopted_once_with_a_receipt() {
    use ds_sync_runtime::store::ArtifactState;
    let h = Harness::new();
    let legacy = local_row(b"sealed by an older release");
    // Not sealed through the harness: only the producer holds it.
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = FakeProducer::new(h.world.clone(), vec![legacy.clone()]);
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let host = host_over(&h, &gateway, &producer, &reads, &online);
    let first = run(&host, PROJECT, sync::Trigger::Startup).unwrap();
    assert_eq!(
        outcomes(&first),
        vec![
            "adopted:from_producer",
            "open_grant:granted",
            "upload:published"
        ]
    );
    let adopted = &first.receipts[0];
    assert!(adopted
        .detail
        .as_deref()
        .unwrap()
        .contains(&legacy.client_publish_id));
    let rows = stored_rows(&h, PROJECT);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].bytes_locator, legacy.bytes_locator);
    assert_eq!(rows[0].state, ArtifactState::Published);
    assert!(
        host_receipt_actions(&h).contains(&"adopted".to_string()),
        "the adoption is kept in the store"
    );
    let second = run(&host, PROJECT, sync::Trigger::Manual).unwrap();
    assert_eq!(outcomes(&second), vec!["nothing:in_sync"], "adopted once");
}
