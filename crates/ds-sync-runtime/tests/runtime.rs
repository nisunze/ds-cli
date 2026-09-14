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
    kernel_sync as sync, open_store, projects_of_fence, run, LocalOutput, LocalRow, SharedStore,
    SolarPublishOutcome, SolarPublisher, StoreHost, SyncRun, TransferReceipt,
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

    /// Run one trigger through a fresh host bound to this harness's durable
    /// store — a new host every call, the way a restart gets one.
    fn run(&self, rows: Vec<ds_sync_runtime::LocalRow>, trigger: sync::Trigger) -> SyncRun {
        self.run_with(rows, trigger, "install-1#100").0
    }

    fn run_with(
        &self,
        rows: Vec<ds_sync_runtime::LocalRow>,
        trigger: sync::Trigger,
        worker_id: &str,
    ) -> (SyncRun, Arc<std::sync::atomic::AtomicBool>) {
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
        readable: true,
        engine_release: "ds-solar-engine@1.2.3".into(),
        engine_build_manifest_sha256: "a".repeat(64),
        grant_engine: "solar".into(),
        resource: "city-1".into(),
        client_publish_id: "solar-job-1".into(),
        outputs: vec![output],
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
        vec!["open_grant:failed", "upload:failed"],
        "no grant, so no publication",
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
    assert_eq!(
        upload.detail.as_deref(),
        Some("no work grant covers this publication"),
    );
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

#[test]
fn a_lease_held_by_a_second_host_stops_the_first_from_executing() {
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
        vec!["open_grant:failed", "upload:failed"],
        "a server with no gateway refuses cleanly, in order",
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
    use ds_sync_runtime::{LocalRow, Producer, TransferReceipt};
    use std::sync::atomic::AtomicBool;
    struct RevokingProducer(FakeProducer, Arc<AtomicBool>);
    impl Producer for RevokingProducer {
        fn inventory(&self, project: &str) -> Result<Vec<LocalRow>, String> {
            self.0.inventory(project)
        }
        fn locator(&self, row: &LocalRow) -> String {
            self.0.locator(row)
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
    let producer = FakeProducer::new(
        harness.world.clone(),
        vec![local_row(b"report"), solar_row()],
    );
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
        world.record = vec![record_head(b"older output", 1, "old-source")];
    }
    let result = h.run(vec![local_row(b"stale output")], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&result),
        vec!["open_grant:granted", "conflict:source_base_moved"]
    );
    let world = fakes::lock(&h.world);
    assert_eq!(world.grants_minted, 1);
    assert_eq!(world.commits, 0);
    assert!(world.transfers.is_empty());
    assert!(
        world.declarations.is_empty(),
        "stale inputs must never be relabelled with the new grant"
    );
}

#[test]
fn a_changed_source_reopens_a_held_grant_before_publishing_new_reports() {
    let h = Harness::new();
    h.run(vec![local_row(b"first output")], sync::Trigger::Startup);
    fakes::lock(&h.world)
        .snapshot_base
        .insert("export-tx_1".into(), "new-source".into());
    let mut next = local_row(b"next output");
    next.base_revision = Some("new-source".into());
    next.client_publish_id = "publish-22222222-2222-4222-8222-222222222222".into();
    let result = h.run(vec![next], sync::Trigger::LocalChange);
    assert_eq!(
        outcomes(&result),
        vec!["open_grant:granted", "upload:published"]
    );
    assert_eq!(fakes::lock(&h.world).commits, 2);
    assert_eq!(fakes::lock(&h.world).grants_minted, 2);
}
