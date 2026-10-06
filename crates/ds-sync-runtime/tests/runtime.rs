//! The shared native sync runtime, driven end to end through the real
//! [`StoreHost`] over a real `ds_sync_store` on disk, against deterministic
//! fakes for the record and its door, the storage sessions and the bytes.
//!
//! These are the acceptance behaviours the assignment names: an authenticated
//! open/upload/finalize that commits exactly once; an unauthorized identity
//! that moves no byte; restart and reopen from the durable store; a lease held
//! by a second host; a transfer failure; and a cancellation. They run the same
//! `run()` every host runs, so a drift between hosts is a drift here.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

use ds_sync_runtime::fakes::{
    self, local_row, record_head, FakeGateway, FakeProducer, FakeReads, TransferMode,
};
use ds_sync_runtime::store::Fence;
use ds_sync_runtime::{
    kernel_sync as sync, open_store, projects_of_fence, run, Gateway, GatewayError, LocalOutput,
    LocalRow, Producer, Producers, SharedStore, SolarPublishOutcome, SolarPublisher, StoreHost,
    SyncHost, SyncRoute, SyncRun, TransferReceipt,
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

/// A stand-in for the gateway's missing/expired API credential. It refuses
/// before the record sees the request, including a heads read.
struct CredentialDoor {
    record: FakeGateway,
    refused: AtomicBool,
    calls: AtomicUsize,
}

impl Gateway for CredentialDoor {
    fn post(
        &self,
        route: SyncRoute,
        body: &serde_json::Value,
    ) -> Result<serde_json::Value, GatewayError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.refused.load(Ordering::SeqCst) {
            return Err(GatewayError::refused(401, None, "the gateway answered 401"));
        }
        self.record.post(route, body)
    }
}

fn solar_row() -> LocalRow {
    let bytes = b"sealed report input";
    let output = LocalOutput {
        filename: None,
        paper_size: None,
        presentation: None,
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
        vec!["upload:published"],
        "the plan's order is the run's order",
    );

    let world = fakes::lock(&harness.world);
    assert_eq!(world.commits, 1, "the record advanced exactly once");
    assert_eq!(world.declarations.len(), 2, "open, then finalize");
    assert_eq!(world.declarations[0]["action"], "open");
    assert_eq!(world.declarations[1]["action"], "finalize");
    assert_eq!(
        world.declarations[0]["client_run_id"], world.declarations[1]["client_run_id"],
        "one run, named by the publish id, from open to finalize",
    );
    assert_eq!(
        world.declarations[0]["input_base_fingerprint"],
        fakes::FINGERPRINT,
        "the declaration names the inputs the report was computed from",
    );
    assert_eq!(
        world.declarations[0]["engine_build_manifest_sha256"],
        fakes::BUILD_MANIFEST,
        "the build that produced it travels as provenance",
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
    fakes::lock(&harness.world).refuse_open =
        Some("Publishing reports on this project requires the design.report capability".into());
    let bytes = vec![1_u8; 2048];
    let row = local_row(&bytes);
    let outcome = harness.run(vec![row.clone()], sync::Trigger::Startup);

    assert_eq!(
        outcomes(&outcome),
        vec!["upload:refused", "reclaimed:refused"],
        "the record refused this account, so the publication is settled and its bytes leave",
    );
    let world = fakes::lock(&harness.world);
    assert_eq!(world.opens, 1, "the door was asked once");
    assert_eq!(world.declarations.len(), 1, "nothing was finalized");
    assert_eq!(world.commits, 0, "the record did not advance");
    assert!(world.transfers.is_empty(), "not one byte left this machine");
    drop(world);
    // The cause, not its consequence: the authority's own words reach the
    // row the operator reads.
    let upload = outcome
        .receipts
        .iter()
        .find(|receipt| receipt.action == "upload")
        .expect("an upload receipt");
    let detail = upload.detail.clone().expect("the row states a cause");
    assert!(
        detail.contains("design.report"),
        "the row must carry what the authority answered: {detail}"
    );
    assert_eq!(
        stored_rows(&harness, PROJECT)[0].state,
        ds_sync_runtime::store::ArtifactState::Refused
    );
}

#[test]
fn one_unreachable_open_does_not_hold_other_uploads_in_the_batch() {
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
            .current_base
            .insert("export-tx_2".into(), "source-2".into());
        world.refuse_open_resource = Some("tx_2".into());
    }

    let outcome = harness.run(vec![first, second], sync::Trigger::Startup);

    assert_eq!(
        outcomes(&outcome),
        vec!["upload:published", "upload:failed"]
    );
    let world = fakes::lock(&harness.world);
    assert_eq!(world.opens, 2);
    assert_eq!(world.commits, 1, "the reachable publication advances");
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
        .expect("the held upload receipt");
    // No verdict was reached, so the row is held with the authority's words.
    let detail = failed.detail.clone().expect("the held row states a cause");
    assert!(
        detail.contains("temporarily unreachable"),
        "unexpected detail: {detail}"
    );
    assert_eq!(failed.outcome, "failed");
}

#[test]
fn invariant_6_and_7_credential_refusal_stops_after_one_call_and_recovery_publishes_every_row() {
    use ds_sync_runtime::store::ArtifactState;

    let h = Harness::new();
    let rows: Vec<LocalRow> = (1..=3)
        .map(|number| {
            let mut row = local_row(format!("report-{number}").as_bytes());
            row.identity.operation = format!("export-tx_{number}");
            row.resource = format!("tx_{number}");
            row.client_publish_id = format!("publish-{number}");
            row.bytes_locator = format!("fake:batch:publish-{number}");
            row
        })
        .collect();
    {
        let mut world = fakes::lock(&h.world);
        for row in &rows {
            world
                .current_base
                .insert(row.identity.operation.clone(), fakes::FINGERPRINT.into());
        }
    }
    h.seal(&rows);
    let door = CredentialDoor {
        record: FakeGateway {
            world: h.world.clone(),
        },
        refused: AtomicBool::new(true),
        calls: AtomicUsize::new(0),
    };
    let producer = FakeProducer::new(h.world.clone(), rows);
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let host = StoreHost::new(
        h.store.clone(),
        fence(),
        "desktop#1".into(),
        PROJECT,
        &door,
        &producer,
        &reads,
        &online,
    )
    .unwrap();

    let refused = run(&host, PROJECT, sync::Trigger::LocalChange).unwrap();
    assert_eq!(outcomes(&refused), vec!["upload:credential_refused"]);
    assert_eq!(
        door.calls.load(Ordering::SeqCst),
        1,
        "one refused call per pass"
    );
    assert!(
        fakes::lock(&h.world).declarations.is_empty(),
        "the record was not reached"
    );
    let held = stored_rows(&h, PROJECT);
    assert_eq!(held.len(), 3);
    assert!(held
        .iter()
        .all(|row| row.state == ArtifactState::Held && row.readable));
    let queue = h
        .store
        .lock()
        .unwrap()
        .queue(&fence(), Some(PROJECT), ds_sync_runtime::now_ms())
        .unwrap();
    assert_eq!(queue.queued_batches, 3);
    assert!(queue.stuck);
    assert_eq!(queue.blocked.as_ref().unwrap().cause, "credential_refused");
    assert!(queue.next.contains("sign in again"));
    assert_eq!(
        h.store
            .lock()
            .unwrap()
            .queue_all(Some(PROJECT), ds_sync_runtime::now_ms())
            .unwrap()
            .blocked
            .unwrap()
            .cause,
        "credential_refused",
        "identity-free CLI status reads the same blocker"
    );
    let receipts = host.receipts(10).unwrap();
    assert_eq!(
        receipts
            .iter()
            .filter(|receipt| receipt.outcome == "credential_refused")
            .count(),
        1
    );

    for _ in 0..2 {
        let held_pass = run(&host, PROJECT, sync::Trigger::LocalChange).unwrap();
        assert_eq!(outcomes(&held_pass), vec!["session:blocked"]);
        assert!(held_pass.receipts[0]
            .detail
            .as_deref()
            .unwrap()
            .contains("made no gateway call"));
        assert_eq!(door.calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            host.receipts(10).unwrap().len(),
            receipts.len(),
            "no retry noise persisted"
        );
    }

    door.refused.store(false, Ordering::SeqCst);
    let still_held = run(&host, PROJECT, sync::Trigger::LocalChange).unwrap();
    assert_eq!(outcomes(&still_held), vec!["session:blocked"]);
    assert_eq!(door.calls.load(Ordering::SeqCst), 1);
    let recovered = run(&host, PROJECT, sync::Trigger::Manual).unwrap();
    assert_eq!(
        outcomes(&recovered),
        vec![
            "session:connected",
            "upload:published",
            "upload:published",
            "upload:published",
        ]
    );
    assert_eq!(fakes::lock(&h.world).commits, 3);
    let queue = h
        .store
        .lock()
        .unwrap()
        .queue(&fence(), Some(PROJECT), ds_sync_runtime::now_ms())
        .unwrap();
    assert_eq!(queue.queued_batches, 0);
    assert!(queue.blocked.is_none());
    assert!(!queue.stuck);
    assert!(h
        .store
        .lock()
        .unwrap()
        .queue_all(Some(PROJECT), ds_sync_runtime::now_ms())
        .unwrap()
        .blocked
        .is_none());
    assert!(stored_rows(&h, PROJECT)
        .iter()
        .all(|row| row.state == ArtifactState::Published));
}

#[test]
fn a_refused_heads_read_keeps_a_session_receipt_and_blocks_the_held_queue() {
    let h = Harness::new();
    let row = local_row(b"sealed before heads read");
    h.seal(std::slice::from_ref(&row));
    let door = CredentialDoor {
        record: FakeGateway {
            world: h.world.clone(),
        },
        refused: AtomicBool::new(true),
        calls: AtomicUsize::new(0),
    };
    let producer = FakeProducer::new(h.world.clone(), vec![row]);
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let host = StoreHost::new(
        h.store.clone(),
        fence(),
        "desktop#1".into(),
        PROJECT,
        &door,
        &producer,
        &reads,
        &online,
    )
    .unwrap();
    let error = run(&host, PROJECT, sync::Trigger::Navigation).unwrap_err();
    assert_eq!(error, "the gateway answered 401");
    assert_eq!(door.calls.load(Ordering::SeqCst), 1);
    let receipts = host.receipts(10).unwrap();
    assert!(receipts
        .iter()
        .any(|receipt| receipt.action == "session" && receipt.outcome == "credential_refused"));
    let status = host.store_read().unwrap();
    assert_eq!(status["status"]["blocked"]["cause"], "credential_refused");
    assert_eq!(status["status"]["queuedBatches"], 1);
}

#[test]
fn an_accepted_heads_read_clears_the_old_session_block_without_an_upload() {
    use ds_sync_runtime::store::ArtifactState;

    let h = Harness::new();
    let row = solar_row();
    h.seal(std::slice::from_ref(&row));
    let door = CredentialDoor {
        record: FakeGateway {
            world: h.world.clone(),
        },
        refused: AtomicBool::new(true),
        calls: AtomicUsize::new(0),
    };
    let producer = FakeProducer::new(h.world.clone(), vec![row]);
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let host = StoreHost::new(
        h.store.clone(),
        fence(),
        "desktop#1".into(),
        PROJECT,
        &door,
        &producer,
        &reads,
        &online,
    )
    .unwrap();

    assert!(ds_sync_runtime::run::run_reports(&host, PROJECT, sync::Trigger::Manual).is_err());
    assert!(host.credential_block(PROJECT).unwrap().is_some());
    door.refused.store(false, Ordering::SeqCst);
    let recovered =
        ds_sync_runtime::run::run_reports(&host, PROJECT, sync::Trigger::Manual).unwrap();
    assert_eq!(outcomes(&recovered), vec!["session:connected"]);
    assert_eq!(door.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        stored_rows(&h, PROJECT)[0].state,
        ArtifactState::Held,
        "the Solar row is still queued"
    );
    assert!(host.credential_block(PROJECT).unwrap().is_none());
    let local_change =
        ds_sync_runtime::run::run_reports(&host, PROJECT, sync::Trigger::LocalChange).unwrap();
    assert!(!outcomes(&local_change).contains(&"session:blocked".to_string()));
}

#[test]
fn a_blocked_local_change_under_one_fence_does_not_hold_another_fence() {
    let h = Harness::new();
    let row = local_row(b"primary held report");
    h.seal(std::slice::from_ref(&row));
    let refused = CredentialDoor {
        record: FakeGateway {
            world: h.world.clone(),
        },
        refused: AtomicBool::new(true),
        calls: AtomicUsize::new(0),
    };
    let primary_producer = FakeProducer::new(h.world.clone(), vec![row]);
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let primary = StoreHost::new(
        h.store.clone(),
        fence(),
        "primary#1".into(),
        PROJECT,
        &refused,
        &primary_producer,
        &reads,
        &online,
    )
    .unwrap();
    assert_eq!(
        outcomes(&run(&primary, PROJECT, sync::Trigger::LocalChange).unwrap()),
        vec!["upload:credential_refused"]
    );

    let mut other_fence = fence();
    other_fence.account = "another-account".into();
    let mut other_row = local_row(b"another account report");
    other_row.identity.operation = "export-tx_other".into();
    other_row.resource = "tx_other".into();
    other_row.client_publish_id = "publish-other".into();
    other_row.bytes_locator = "fake:batch:publish-other".into();
    fakes::lock(&h.world).current_base.insert(
        other_row.identity.operation.clone(),
        fakes::FINGERPRINT.into(),
    );
    h.store
        .lock()
        .unwrap()
        .apply(
            &other_fence,
            ds_sync_runtime::now_ms(),
            ds_sync_runtime::store::Event::LocalProduced {
                row: ds_sync_runtime::rows::store_row(PROJECT, &other_row, 1_000),
            },
        )
        .unwrap();
    let accepted = FakeGateway {
        world: h.world.clone(),
    };
    let other_producer = FakeProducer::new(h.world.clone(), vec![other_row]);
    let other = StoreHost::new(
        h.store.clone(),
        other_fence.clone(),
        "other#1".into(),
        PROJECT,
        &accepted,
        &other_producer,
        &reads,
        &online,
    )
    .unwrap();
    assert!(other.credential_block(PROJECT).unwrap().is_none());
    assert_eq!(
        outcomes(&run(&other, PROJECT, sync::Trigger::LocalChange).unwrap()),
        vec!["upload:published"]
    );
    assert_eq!(refused.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        primary.credential_block(PROJECT).unwrap().unwrap().cause,
        "credential_refused"
    );
    assert!(other.credential_block(PROJECT).unwrap().is_none());
}

/// A locally built engine — a "+development" release no installer ever
/// registered — publishes like any other: the record asks who the account is,
/// never which binary computed the bytes, and the build is kept as provenance.
#[test]
fn a_locally_built_engine_publishes_and_its_build_travels_as_provenance() {
    let harness = Harness::new();
    let mut row = local_row(&[2_u8; 4096]);
    row.engine_release = "ds-network-reporter@0.1.0+development".into();
    row.engine_build_manifest_sha256 = "f".repeat(64);
    let outcome = harness.run(vec![row], sync::Trigger::Startup);

    assert_eq!(outcomes(&outcome), vec!["upload:published"]);
    let world = fakes::lock(&harness.world);
    assert_eq!(world.commits, 1);
    assert_eq!(
        world.declarations[0]["engine_version"],
        "ds-network-reporter@0.1.0+development"
    );
    assert_eq!(
        world.declarations[0]["engine_build_manifest_sha256"],
        "f".repeat(64)
    );
}

#[test]
fn restart_reopens_the_store_and_never_recomputes_a_committed_publish() {
    let harness = Harness::new();
    let bytes = vec![3_u8; 8192];
    let first = harness.run(vec![local_row(&bytes)], sync::Trigger::Startup);
    assert_eq!(outcomes(&first), vec!["upload:published"]);
    assert_eq!(fakes::lock(&harness.world).commits, 1);

    // A restart: a brand-new host on the same durable store, same producer
    // rows still present. It must not re-open, re-upload or re-commit.
    let second = harness.run(vec![local_row(&bytes)], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&second),
        vec!["nothing:in_sync"],
        "the held head from the committed publish answers the restart",
    );
    let world = fakes::lock(&harness.world);
    assert_eq!(world.commits, 1, "the record still advanced exactly once");
    assert_eq!(world.opens, 1, "the record was not asked twice");
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
    assert_eq!(world.opens, 0);
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

    // The record asked for the bytes and the transport tried, but they never
    // committed, so the record is not asked to publish and the work is held.
    assert_eq!(
        outcome
            .receipts
            .iter()
            .filter(|r| r.action == "upload")
            .map(|r| r.outcome.clone())
            .collect::<Vec<_>>(),
        vec!["failed"],
        "a weak link leaves the work held, not committed",
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
    // A row the store already holds (through the producer seal) and
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
    // runs end to end and reaches the upload, where the missing gateway holds
    // the publication — nothing is invented, nothing is committed.
    let outcome = run(&host, PROJECT, sync::Trigger::LocalChange).expect("the server loop runs");

    assert_eq!(
        outcome
            .receipts
            .iter()
            .map(|receipt| format!("{}:{}", receipt.action, receipt.outcome))
            .collect::<Vec<_>>(),
        vec!["upload:failed"],
        "a server with no gateway holds the row, naming the gateway",
    );
    let world = fakes::lock(&harness.world);
    assert_eq!(world.commits, 0, "the server moved no bytes");
    assert_eq!(world.opens, 0);
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

/// Seed one held artifact through the producer's normal store event.
fn seed_store_only_row(harness: &Harness, bytes: &[u8]) {
    let mut row = local_row(bytes);
    row.client_publish_id = "publish-seed-1".into();
    row.bytes_locator = "network_reporter:batch:publish-seed-1".into();
    harness.seal(&[row]);
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
    assert!(
        read["grants"].as_array().is_some_and(Vec::is_empty),
        "no grant is held: a publication asks the session's own authority"
    );
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
    assert_eq!(world.opens, 0);
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
fn report_pass_does_not_send_solar_through_the_report_door() {
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
            world.opens,
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
            world.opens,
            world.commits,
            world.transfers.len()
        ),
        before
    );
}

#[test]
fn head_inventory_walks_service_default_pages_without_truncation() {
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
    assert_eq!(fakes::lock(&h.world).head_reads, 5);
}

#[test]
fn a_report_from_current_inputs_replaces_an_older_output_after_admission() {
    let h = Harness::new();
    fakes::lock(&h.world).record = vec![record_head(b"older output", 1, "old-source")];
    let result = h.run(vec![local_row(b"new output")], sync::Trigger::Startup);
    assert_eq!(outcomes(&result), vec!["upload:published"]);
    assert_eq!(fakes::lock(&h.world).commits, 1);
}

#[test]
fn stale_report_inputs_are_refused_by_the_record_before_any_upload() {
    let h = Harness::new();
    {
        let mut world = fakes::lock(&h.world);
        world
            .current_base
            .insert("export-tx_1".into(), "source-changed".into());
    }
    let stale = local_row(b"stale output");
    let result = h.run(vec![stale.clone()], sync::Trigger::Startup);
    // The record's verdict is kept, then the bytes go: online overwrites
    // local, and a lost batch left on disk is the 350-batch, 2.08 GB,
    // seven-day queue.
    assert_eq!(
        outcomes(&result),
        vec!["upload:conflict", "reclaimed:conflict"]
    );
    assert_eq!(result.receipts[1].bytes, Some(stale.size_bytes));
    assert_eq!(
        result.reclaimed,
        ds_sync_runtime::Reclaimed {
            batches: 1,
            bytes: stale.size_bytes
        }
    );
    let world = fakes::lock(&h.world);
    assert_eq!(world.opens, 1);
    assert_eq!(world.commits, 0);
    assert!(world.transfers.is_empty(), "no byte moved for stale inputs");
    assert_eq!(
        world.declarations.len(),
        1,
        "the record was never asked to publish"
    );
    assert_eq!(
        world.reclaims,
        vec![(stale.client_publish_id.clone(), stale.size_bytes)]
    );
    drop(world);

    // A fresh producer presents the same sealed bytes again (a machine whose
    // store already holds the verdict while the batch is still on disk): the
    // observation itself frees them, once, and the record is not asked again.
    let recovery = h.run(vec![stale.clone()], sync::Trigger::LocalChange);
    assert_eq!(
        outcomes(&recovery),
        vec!["reclaimed:conflict"],
        "the exact terminal conflict is not executed again; its bytes are freed"
    );
    assert_eq!(recovery.reclaimed.batches, 1);
    assert_eq!(
        fakes::lock(&h.world).opens,
        1,
        "background recovery must not re-declare stale bytes"
    );
}

/// The record re-checks the source inside the transaction that advances the
/// head. A publication whose source moved between `open` and `finalize` is
/// `stored_stale`: settled, and its bytes leave.
#[test]
fn a_source_that_moves_between_open_and_finalize_is_stored_stale_and_reclaimed() {
    let h = Harness::new();
    fakes::lock(&h.world).publish_state = Some("stored_stale".into());
    let row = local_row(b"overtaken at finalize");
    let result = h.run(vec![row.clone()], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&result),
        vec!["upload:stored_stale", "reclaimed:stored_stale"]
    );
    assert!(result.receipts[0]
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("changed before finalization")));
    assert_eq!(
        stored_rows(&h, PROJECT)[0].state,
        ds_sync_runtime::store::ArtifactState::Conflict
    );
    assert_eq!(fakes::lock(&h.world).commits, 0);
}

/// A report that names no source inputs can never be fenced against the
/// record; it is refused locally and never declared.
#[test]
fn a_report_with_no_source_fingerprint_is_refused_without_asking_the_record() {
    let h = Harness::new();
    let mut row = local_row(b"unfenceable");
    row.input_base_fingerprint = None;
    let result = h.run(vec![row], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&result),
        vec!["upload:refused", "reclaimed:refused"]
    );
    assert!(result.receipts[0]
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("names no source inputs")));
    assert_eq!(fakes::lock(&h.world).opens, 0, "the record was not asked");
}

/// A declaration the project's current inputs do not produce is refused on
/// arrival; a 400 at `finalize` — bytes the record has not seen yet — never
/// settles anything.
#[test]
fn an_unproducible_declaration_is_refused_and_an_unfinished_upload_is_held() {
    use ds_sync_runtime::store::ArtifactState;
    let h = Harness::new();
    fakes::lock(&h.world).invalid_declaration =
        Some("Network Reporter output xlsx does not match project settings".into());
    let result = h.run(vec![local_row(b"not producible")], sync::Trigger::Startup);
    assert_eq!(
        outcomes(&result),
        vec!["upload:refused", "reclaimed:refused"]
    );
    assert_eq!(stored_rows(&h, PROJECT)[0].state, ArtifactState::Refused);

    let h = Harness::new();
    fakes::lock(&h.world).never_commit = true;
    let result = h.run(
        vec![local_row(b"bytes not yet stored")],
        sync::Trigger::Startup,
    );
    assert_eq!(outcomes(&result), vec!["upload:failed"]);
    assert!(result.receipts[0]
        .detail
        .as_deref()
        .is_some_and(|detail| detail.contains("has not finished uploading")));
    assert_eq!(stored_rows(&h, PROJECT)[0].state, ArtifactState::Held);
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
            .current_base
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
    assert_eq!(outcomes(&first), vec!["upload:published"]);
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
            .current_base
            .insert("export-tx_1".into(), "source-changed".into());
        world.refuse_reclaim = Some("the pending-upload lock is held by pid 7".into());
    }
    let stale = local_row(b"stale output");
    let result = h.run(vec![stale.clone()], sync::Trigger::Startup);
    // The verdict's reclaim fails; the closing observation of the inventory
    // meets the same row under the store's verdict and tries again, in the
    // open. Two attempts, two receipts, nothing hidden.
    assert_eq!(
        outcomes(&result),
        vec![
            "upload:conflict",
            "reclaim_failed:conflict",
            "reclaim_failed:conflict"
        ]
    );
    assert!(result.receipts[1]
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

/// The store's verdict on one row, as the record would pass it: the row
/// is `conflict`, still readable, still naming its bytes — a lost row the
/// next observation must free.
fn settle_as_lost(host: &StoreHost<'_>, row: &LocalRow) {
    host.record_receipt(
        PROJECT,
        &ds_sync_runtime::Receipt::new("conflict", "superseded_by_head")
            .about(&row.identity)
            .for_publication(row),
    )
    .expect("the verdict is recorded");
    let stored = host
        .snapshot()
        .unwrap()
        .artifacts
        .into_iter()
        .find(|stored| stored.identity == row.identity)
        .expect("the row is in the store");
    assert_eq!(stored.state.as_str(), "conflict");
    assert!(stored.readable && !stored.bytes_locator.is_empty());
}

#[test]
fn a_lost_row_is_freed_by_the_producer_of_its_engine_never_answered_zero_by_another() {
    // WHY: the Server's report pass and the desktop's one host observed a
    // lost Solar row through the REPORT producer, which holds no batch by
    // that id and answered 0 — and the kernel then cleared the row's locator
    // while the result stayed in the compute store. One producer set keyed
    // by engine: the Solar row is freed by the Solar producer, with its
    // bytes on the receipt, whichever pass observes it; the report producer
    // is never asked about it.
    let h = Harness::new();
    let report = local_row(b"report output");
    let solar = solar_row();
    h.seal(&[report.clone(), solar.clone()]);
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let report_world = fakes::world();
    let solar_world = fakes::world();
    let report_producer = FakeProducer::new(report_world.clone(), vec![report.clone()]);
    let solar_producer = FakeProducer::new(solar_world.clone(), vec![solar.clone()]);
    let producers = Producers::new(vec![
        ("network_reporter", &report_producer as &dyn Producer),
        ("solar", &solar_producer as &dyn Producer),
    ])
    .expect("one producer per engine");
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let host = host_over_producer(&h, &gateway, &producers, &reads, &online);
    settle_as_lost(&host, &solar);

    let rows = host.local(PROJECT).expect("observe");
    assert!(
        rows.iter().all(|row| row.identity != solar.identity),
        "a lost row never re-enters the plan"
    );
    let receipts = host.take_receipts(PROJECT);
    assert_eq!(receipts.len(), 1, "{receipts:?}");
    assert_eq!(receipts[0].action, "reclaimed");
    assert_eq!(receipts[0].outcome, "conflict");
    assert_eq!(
        receipts[0].bytes,
        Some(solar.size_bytes),
        "the bytes freed are the Solar result's, not another producer's 0"
    );
    assert_eq!(
        fakes::lock(&solar_world).reclaims,
        vec![(solar.client_publish_id.clone(), solar.size_bytes)],
        "the Solar producer freed its own row"
    );
    assert!(
        fakes::lock(&report_world).reclaims.is_empty(),
        "the report producer was never asked about a Solar row"
    );
    assert!(
        solar_producer.inventory(PROJECT).unwrap().is_empty(),
        "the result left the machine"
    );
    assert_eq!(
        report_producer.inventory(PROJECT).unwrap().len(),
        1,
        "the report batch, which did not lose, is untouched"
    );
    let stored = stored_rows(&h, PROJECT)
        .into_iter()
        .find(|row| row.identity == solar.identity)
        .unwrap();
    assert!(!stored.readable && stored.bytes_locator.is_empty());
}

#[test]
fn a_lost_row_of_an_engine_with_no_producer_here_is_reclaim_failed_never_a_silent_zero() {
    // WHY: a host with a report producer only — the desktop today — must
    // not tell the store a Solar row's bytes are gone. The reclaim fails
    // with the engine named, the row keeps naming its bytes, and the next
    // observation asks again in the open.
    let h = Harness::new();
    let solar = solar_row();
    h.seal(std::slice::from_ref(&solar));
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let report_producer = FakeProducer::new(fakes::world(), Vec::new());
    let producers = Producers::new(vec![(
        "network_reporter",
        &report_producer as &dyn Producer,
    )])
    .expect("one producer");
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let host = host_over_producer(&h, &gateway, &producers, &reads, &online);
    settle_as_lost(&host, &solar);

    host.local(PROJECT).expect("observe");
    let receipts = host.take_receipts(PROJECT);
    assert_eq!(receipts.len(), 1, "{receipts:?}");
    assert_eq!(receipts[0].action, "reclaim_failed");
    assert_eq!(receipts[0].outcome, "conflict");
    assert!(
        receipts[0]
            .detail
            .as_deref()
            .unwrap()
            .contains("no solar producer on this host"),
        "{:?}",
        receipts[0].detail
    );
    let stored = stored_rows(&h, PROJECT)
        .into_iter()
        .find(|row| row.identity == solar.identity)
        .unwrap();
    assert!(
        stored.readable && stored.bytes_locator == solar.bytes_locator,
        "the row still names its bytes: nothing told the store they left"
    );

    // Asked again, in the open, on the next observation.
    host.local(PROJECT).expect("observe again");
    assert_eq!(host.take_receipts(PROJECT)[0].action, "reclaim_failed");

    // The set itself refuses ambiguity and emptiness.
    assert!(Producers::new(Vec::new()).is_err());
    assert!(Producers::new(vec![
        ("solar", &report_producer as &dyn Producer),
        ("solar", &report_producer as &dyn Producer),
    ])
    .is_err());
}

fn host_over_producer<'a>(
    h: &'a Harness,
    gateway: &'a FakeGateway,
    producer: &'a dyn Producer,
    reads: &'a FakeReads,
    online: &'a dyn Fn() -> bool,
) -> StoreHost<'a> {
    StoreHost::new(
        h.store.clone(),
        fence(),
        "install-1#201".into(),
        PROJECT,
        gateway,
        producer,
        reads,
        online,
    )
    .expect("host")
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
fn a_changed_source_publishes_the_report_computed_from_it() {
    let h = Harness::new();
    h.run(vec![local_row(b"first output")], sync::Trigger::Startup);
    fakes::lock(&h.world)
        .current_base
        .insert("export-tx_1".into(), "new-source".into());
    let mut next = local_row(b"next output");
    next.input_base_fingerprint = Some("new-source".into());
    next.client_publish_id = "publish-22222222-2222-4222-8222-222222222222".into();
    let result = h.run(vec![next], sync::Trigger::LocalChange);
    assert_eq!(outcomes(&result), vec!["upload:published"]);
    assert_eq!(fakes::lock(&h.world).commits, 2);
    assert_eq!(fakes::lock(&h.world).opens, 2);
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
    assert_eq!(outcomes(&resumed), vec!["upload:published"]);
    assert_eq!(fakes::lock(&h.world).commits, 1);
    assert_eq!(
        resumed.receipts[0].client_publish_id.as_deref(),
        Some(row.client_publish_id.as_str())
    );
    assert_eq!(
        stored_rows(&h, PROJECT)[0].acknowledged_work_id,
        resumed.receipts[0].work_id
    );
    assert!(stored_rows(&h, PROJECT)[0].acknowledged_work_id.is_some());

    // (b) Killed after the transfer, before the restated declaration is
    // acknowledged: the bytes are in the session, the record has not
    // advanced, the row is held with the failure as its reason.
    let h = Harness::new();
    fakes::lock(&h.world).refuse_publish_from = Some(1);
    let row = local_row(b"transferred then killed");
    let first = h.run(vec![row.clone()], sync::Trigger::Startup);
    assert_eq!(outcomes(&first), vec!["upload:failed"]);
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
    assert_eq!(outcomes(&first), vec!["upload:published"]);
    let second = h.run(vec![row.clone()], sync::Trigger::Manual);
    assert_eq!(outcomes(&second), vec!["nothing:in_sync"]);
    assert_eq!(second.replanned.summary.in_sync, 1);
    assert_eq!(stored_rows(&h, PROJECT).len(), 1);
    let world = fakes::lock(&h.world);
    assert_eq!(world.commits, 1);
    assert_eq!(world.opens, 1);
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
    assert_eq!(outcomes(&ours), vec!["upload:published"]);
    assert_eq!(stored_rows(&h, PROJECT)[0].state, ArtifactState::Published);
    assert_eq!(stored_rows(&h, OTHER)[0].state, ArtifactState::Held);
    {
        let world = fakes::lock(&h.world);
        assert_eq!(world.commits, 1);
        assert!(
            world
                .declarations
                .iter()
                .all(|declaration| { declaration["client_run_id"] == mine.client_publish_id }),
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
    assert_eq!(outcomes(&theirs_run), vec!["upload:published"]);
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
        .all(|declaration| { declaration["client_run_id"] == theirs.client_publish_id }));
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
        vec!["upload:failed"],
        "the record refuses to publish bytes it does not hold, and the row stays held"
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
    assert!(rows[0].acknowledged_work_id.is_none());
    assert_eq!(
        result.receipts[0].client_publish_id.as_deref(),
        Some(row.client_publish_id.as_str())
    );
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
    assert_eq!(outcomes(&resumed), vec!["upload:published"]);
    assert_eq!(fakes::lock(&h.world).commits, 1);

    // (b) Killed after the bytes went up, before the record acknowledged:
    // the new process resumes from the held row, restates once, and the
    // bytes do not go up twice.
    let mut h = Harness::new();
    fakes::lock(&h.world).refuse_publish_from = Some(1);
    let row = local_row(b"transferred, then the power went");
    let first = h.run(vec![row.clone()], sync::Trigger::Startup);
    assert_eq!(outcomes(&first), vec!["upload:failed"]);
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
        assert_eq!(world.opens, 0);
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
    assert_eq!(outcomes(&mine), vec!["upload:published"]);
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
        .current_base
        .insert("export-tx_2".into(), "source-2".into());
    let first = h.run(vec![published.clone()], sync::Trigger::Startup);
    assert_eq!(outcomes(&first), vec!["upload:published"]);
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
        vec!["adopted:from_producer", "upload:published"]
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

// ---------------------------------------------------------------------------
// Solar rows: the seven invariants, over the same store and the same host,
// through the Solar publication path (`run_solar_publications`). A Solar
// row is sealed at the job's completion (`solar::seal`), under the host's
// local fence; the producer holds NO inventory of its own here — the
// store's rows are the inventory — and its transfer reads the row it is
// handed, as the real producer reads the job the row's locator names.
// ---------------------------------------------------------------------------

use ds_sync_runtime::solar::{self, SealOutcome as SolarSeal};
use std::sync::Mutex;

/// Every job id a seal freed as superseded — the Solar producer's
/// `retire_job_result`, scripted.
type Freed = Arc<Mutex<Vec<String>>>;

fn solar_seal(
    h: &Harness,
    project: &str,
    city: &str,
    job: &str,
    bytes: &[u8],
    freed: &Freed,
) -> SolarSeal {
    solar_seal_under(h, &fence(), project, city, job, bytes, freed)
}

fn solar_seal_under(
    h: &Harness,
    fence: &Fence,
    project: &str,
    city: &str,
    job: &str,
    bytes: &[u8],
    freed: &Freed,
) -> SolarSeal {
    solar_seal_completed_at(h, fence, project, city, job, 1_000, bytes, freed)
}

#[allow(clippy::too_many_arguments)]
fn solar_seal_completed_at(
    h: &Harness,
    fence: &Fence,
    project: &str,
    city: &str,
    job: &str,
    completed_at_ms: u64,
    bytes: &[u8],
    freed: &Freed,
) -> SolarSeal {
    let request = solar::SealRequest {
        project_id: project,
        city_id: city,
        job_id: job,
        completed_at_ms,
        engine_release: "ds-solar-engine@1.2.3",
        engine_build_manifest_sha256: &"a".repeat(64),
        input_base_fingerprint: Some(fakes::FINGERPRINT),
        report_input_sha256: &format!("{:x}", sha2::Sha256::digest(bytes)),
        report_input_size_bytes: bytes.len() as u64,
    };
    let free = |job_id: &str| {
        freed.lock().unwrap().push(job_id.to_string());
        Ok(bytes.len() as u64)
    };
    solar::seal(&h.store, fence, &request, &free).expect("the seal records its row")
}

fn freed() -> Freed {
    Arc::new(Mutex::new(Vec::new()))
}

/// A host over the store with an EMPTY producer inventory: what the store
/// holds is what is published.
fn solar_pump<'a>(
    h: &'a Harness,
    gateway: &'a dyn ds_sync_runtime::Gateway,
    producer: &'a FakeProducer,
    reads: &'a FakeReads,
    online: &'a dyn Fn() -> bool,
    worker: &str,
    project: &str,
) -> StoreHost<'a> {
    StoreHost::new(
        h.store.clone(),
        fence(),
        worker.into(),
        project,
        gateway,
        producer,
        reads,
        online,
    )
    .expect("solar host")
}

fn solar_outcomes(receipts: &[ds_sync_runtime::Receipt]) -> Vec<String> {
    receipts
        .iter()
        .map(|receipt| format!("{}:{}", receipt.action, receipt.outcome))
        .collect()
}

/// The seal is one acknowledged step after the completion: the row is in
/// the queue before any pump has run, and the pump adopts nothing.
#[test]
fn a_solar_seal_records_the_row_at_completion_and_the_pump_adopts_nothing() {
    use ds_sync_runtime::store::ArtifactState;
    let h = Harness::new();
    let freed = freed();
    let SolarSeal::Sealed(sealed) = solar_seal(&h, PROJECT, "city-1", "job-1", b"first", &freed)
    else {
        panic!("first seal")
    };
    assert_eq!(sealed.row.state, ArtifactState::Held);
    assert_eq!(sealed.row.replay_key, "job-1");
    assert_eq!(sealed.row.bytes_locator, "solar:prepared:job:job-1");
    assert_eq!(sealed.row.identity, solar::identity_of("city-1"));
    assert!(sealed.superseded.is_none());
    let queue = h
        .store
        .lock()
        .unwrap()
        .queue(&fence(), Some(PROJECT), 2_000)
        .unwrap();
    assert_eq!(
        queue.queued_batches, 1,
        "queued with no pump and no session"
    );
    assert_eq!(queue.queued_bytes, b"first".len() as u64);

    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = FakeProducer::new(h.world.clone(), Vec::new());
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let publisher = ScriptedSolarPublisher::new(Ok(SolarPublishOutcome::Published {
        work_id: "work-1".into(),
        head_revision: 1,
    }));
    let host = solar_pump(&h, &gateway, &producer, &reads, &online, "pump", PROJECT);
    let receipts = host.run_solar_publications(&publisher).unwrap();
    assert_eq!(solar_outcomes(&receipts), vec!["upload:published"]);
    assert!(receipts[0].client_publish_id.is_some());
    assert_eq!(
        stored_rows(&h, PROJECT)[0].acknowledged_work_id,
        receipts[0].work_id
    );
    assert!(
        !host_receipt_actions(&h).contains(&"adopted".to_string()),
        "a sealed row is not adopted: {:?}",
        host_receipt_actions(&h)
    );
    assert_eq!(stored_rows(&h, PROJECT)[0].state, ArtifactState::Published);
}

/// A city computed again on this machine: the new seal frees the result
/// it supersedes through the producer, says so, and the row is the new
/// job's. The same job sealed again, or the same bytes under another job,
/// is already recorded and frees nothing.
#[test]
fn a_new_solar_seal_of_a_city_frees_the_job_it_supersedes_and_says_so() {
    use ds_sync_runtime::store::ArtifactState;
    let h = Harness::new();
    let freed = freed();
    assert!(matches!(
        solar_seal(&h, PROJECT, "city-1", "job-1", b"first", &freed),
        SolarSeal::Sealed(_)
    ));
    let SolarSeal::Sealed(newer) = solar_seal(&h, PROJECT, "city-1", "job-2", b"second!", &freed)
    else {
        panic!("second seal")
    };
    assert_eq!(
        newer.superseded,
        Some(("job-1".to_string(), b"second!".len() as u64))
    );
    assert_eq!(*freed.lock().unwrap(), vec!["job-1".to_string()]);
    let rows = stored_rows(&h, PROJECT);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].replay_key, "job-2");
    assert_eq!(rows[0].state, ArtifactState::Held);
    let receipts = h
        .store
        .lock()
        .unwrap()
        .receipts(
            &fence(),
            &ds_sync_runtime::store::Scope::Project {
                project: PROJECT.into(),
            },
            50,
        )
        .unwrap();
    let reclaimed = receipts
        .iter()
        .find(|receipt| receipt.action == "reclaimed")
        .expect("the supersession is evidence");
    assert_eq!(reclaimed.outcome, "superseded_locally");
    assert!(reclaimed.detail.as_deref().unwrap().contains("job-1"));
    assert!(reclaimed.detail.as_deref().unwrap().contains("job-2"));
    // The same job again, and the same bytes under a third job: recorded
    // already, nothing freed — a duplicate result's retention is the
    // owner's ruling, not the seal's.
    assert!(matches!(
        solar_seal(&h, PROJECT, "city-1", "job-2", b"second!", &freed),
        SolarSeal::AlreadyRecorded { ref replay_key, .. } if replay_key == "job-2"
    ));
    assert!(matches!(
        solar_seal(&h, PROJECT, "city-1", "job-3", b"second!", &freed),
        SolarSeal::AlreadyRecorded { ref replay_key, .. } if replay_key == "job-2"
    ));
    assert_eq!(freed.lock().unwrap().len(), 1);
    assert_eq!(stored_rows(&h, PROJECT).len(), 1);
}

/// A restart replays every completed job newest first, so a city's older
/// completion is observed AFTER its newer one. The older does not take the
/// city and does not free the newer's bytes: the newest completion is the
/// row whatever order completions are sealed in, and the older's result is
/// freed with the same receipt a seal in order leaves for the row it
/// supersedes. Observed again (a second restart), the older frees again —
/// the producer answers 0 once its bytes are gone — and the row is
/// untouched.
#[test]
fn a_solar_completion_sealed_behind_a_newer_one_frees_its_own_result_and_the_newer_row_stands() {
    use ds_sync_runtime::store::ArtifactState;
    let h = Harness::new();
    let freed: Freed = Arc::new(Mutex::new(Vec::new()));
    let SolarSeal::Sealed(newest) = solar_seal_completed_at(
        &h,
        &fence(),
        PROJECT,
        "city-1",
        "job-2",
        2_000,
        b"second!",
        &freed,
    ) else {
        panic!("the newest completion seals")
    };
    assert!(newest.superseded.is_none());
    let older = solar_seal_completed_at(
        &h,
        &fence(),
        PROJECT,
        "city-1",
        "job-1",
        1_000,
        b"first",
        &freed,
    );
    assert!(
        matches!(
            older,
            SolarSeal::Superseded { ref by, freed: Some(5) } if by == "job-2"
        ),
        "{older:?}"
    );
    assert_eq!(*freed.lock().unwrap(), vec!["job-1".to_string()]);
    let rows = stored_rows(&h, PROJECT);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].replay_key, "job-2");
    assert_eq!(rows[0].state, ArtifactState::Held);
    assert_eq!(rows[0].produced_at_ms, 2_000);
    assert!(rows[0].readable, "the newer row's bytes were not freed");
    let receipts = h
        .store
        .lock()
        .unwrap()
        .receipts(
            &fence(),
            &ds_sync_runtime::store::Scope::Project {
                project: PROJECT.into(),
            },
            50,
        )
        .unwrap();
    assert_eq!(receipts.len(), 1, "{receipts:?}");
    assert_eq!(receipts[0].action, "reclaimed");
    assert_eq!(receipts[0].outcome, "superseded_locally");
    assert_eq!(receipts[0].committed_bytes, Some(5));
    assert!(
        receipts[0].identity.is_none(),
        "a loser that never held the row is freed without transitioning it"
    );
    let detail = receipts[0].detail.as_deref().unwrap();
    assert!(detail.contains("calculate-city-1"), "{detail}");
    assert!(detail.starts_with("job-1 lost to"), "{detail}");
    assert!(detail.ends_with("(job-2)"), "{detail}");
    // The same order the other way round ends in the same state: the
    // newer row, the older's bytes freed, one receipt each.
    let h2 = Harness::new();
    let freed2: Freed = Arc::new(Mutex::new(Vec::new()));
    assert!(matches!(
        solar_seal_completed_at(
            &h2,
            &fence(),
            PROJECT,
            "city-1",
            "job-1",
            1_000,
            b"first",
            &freed2
        ),
        SolarSeal::Sealed(_)
    ));
    assert!(matches!(
        solar_seal_completed_at(
            &h2,
            &fence(),
            PROJECT,
            "city-1",
            "job-2",
            2_000,
            b"second!",
            &freed2
        ),
        SolarSeal::Sealed(_)
    ));
    assert_eq!(*freed2.lock().unwrap(), vec!["job-1".to_string()]);
    let rows2 = stored_rows(&h2, PROJECT);
    assert_eq!(rows2.len(), 1);
    assert_eq!(rows2[0].replay_key, "job-2");
    // The queue counts the one row, in both.
    for harness in [&h, &h2] {
        let queue = harness
            .store
            .lock()
            .unwrap()
            .queue(&fence(), Some(PROJECT), 3_000)
            .unwrap();
        assert_eq!(queue.queued_batches, 1);
        assert_eq!(queue.queued_bytes, b"second!".len() as u64);
    }
}

/// Invariant (1), Solar: a pump killed after the seal, after the transfer,
/// or before the acknowledgement, then restarted against the same store,
/// neither duplicates nor loses the publication.
#[test]
fn invariant_1_solar_a_pump_killed_after_seal_after_transfer_or_before_ack_resumes_without_duplicating(
) {
    use ds_sync_runtime::store::ArtifactState;
    let gateway_of = |h: &Harness| FakeGateway {
        world: h.world.clone(),
    };
    let reads_of = |h: &Harness| FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    // (a) Killed after the seal, before any pump: the row is held in the
    // store and the next process publishes it once.
    let h = Harness::new();
    solar_seal(
        &h,
        PROJECT,
        "city-1",
        "job-1",
        b"sealed then killed",
        &freed(),
    );
    assert_eq!(stored_rows(&h, PROJECT)[0].state, ArtifactState::Held);
    let gateway = gateway_of(&h);
    let producer = FakeProducer::new(h.world.clone(), Vec::new());
    let reads = reads_of(&h);
    let publisher = ScriptedSolarPublisher::new(Ok(SolarPublishOutcome::Published {
        work_id: "work-1".into(),
        head_revision: 1,
    }));
    let resumed = solar_pump(&h, &gateway, &producer, &reads, &online, "pump#2", PROJECT);
    let receipts = resumed.run_solar_publications(&publisher).unwrap();
    assert_eq!(solar_outcomes(&receipts), vec!["upload:published"]);
    assert_eq!(publisher.calls.load(Ordering::SeqCst), 1);

    // (b) Killed after the transfer, before the acknowledgement: the bytes
    // went up, the authority never answered, the row is held with the
    // failure as its reason and nothing is published.
    let h = Harness::new();
    solar_seal(
        &h,
        PROJECT,
        "city-1",
        "job-1",
        b"transferred then killed",
        &freed(),
    );
    let gateway = gateway_of(&h);
    let producer = FakeProducer::new(h.world.clone(), Vec::new());
    let reads = reads_of(&h);
    let dying = ScriptedSolarPublisher::new(Err("the authority went away".into()));
    let first = solar_pump(&h, &gateway, &producer, &reads, &online, "pump#1", PROJECT);
    let receipts = first.run_solar_publications(&dying).unwrap();
    assert_eq!(solar_outcomes(&receipts), vec!["upload:failed"]);
    assert_eq!(
        fakes::lock(&h.world).transfers.len(),
        1,
        "the bytes went up"
    );
    let rows = stored_rows(&h, PROJECT);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, ArtifactState::Held);
    assert!(rows[0]
        .state_reason
        .as_deref()
        .is_some_and(|reason| reason.contains("went away")));
    drop(first);
    // The restart: the authority is back. Published once, one row, one head.
    let publisher = ScriptedSolarPublisher::new(Ok(SolarPublishOutcome::Published {
        work_id: "work-1".into(),
        head_revision: 1,
    }));
    let second = solar_pump(&h, &gateway, &producer, &reads, &online, "pump#2", PROJECT);
    let receipts = second.run_solar_publications(&publisher).unwrap();
    assert_eq!(solar_outcomes(&receipts), vec!["upload:published"]);
    let rows = stored_rows(&h, PROJECT);
    assert_eq!(rows.len(), 1, "one row, not two");
    assert_eq!(rows[0].state, ArtifactState::Published);
    let read = second.store_read().unwrap();
    assert_eq!(read["heads"].as_array().unwrap().len(), 1);
    assert_eq!(read["heads"][0]["head_revision"], 1);
    // (c) A third pump finds nothing to do.
    let third = solar_pump(&h, &gateway, &producer, &reads, &online, "pump#3", PROJECT);
    assert!(third.run_solar_publications(&publisher).unwrap().is_empty());
    assert_eq!(publisher.calls.load(Ordering::SeqCst), 1);
}

/// Invariant (2), Solar: the same completed job sealed twice is one row
/// (`already_recorded`), published once, and a later pump answers nothing.
#[test]
fn invariant_2_solar_the_same_job_sealed_twice_is_one_row_and_published_once() {
    let h = Harness::new();
    let freed = freed();
    assert!(matches!(
        solar_seal(&h, PROJECT, "city-1", "job-1", b"published twice", &freed),
        SolarSeal::Sealed(_)
    ));
    // A retried completion projection of the same job: already recorded.
    let again = solar_seal(&h, PROJECT, "city-1", "job-1", b"published twice", &freed);
    assert!(
        matches!(again, SolarSeal::AlreadyRecorded { ref replay_key, .. } if replay_key == "job-1")
    );
    assert!(freed.lock().unwrap().is_empty());
    assert_eq!(stored_rows(&h, PROJECT).len(), 1);
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = FakeProducer::new(h.world.clone(), Vec::new());
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let publisher = ScriptedSolarPublisher::new(Ok(SolarPublishOutcome::Published {
        work_id: "work-1".into(),
        head_revision: 1,
    }));
    let host = solar_pump(&h, &gateway, &producer, &reads, &online, "pump", PROJECT);
    assert_eq!(
        solar_outcomes(&host.run_solar_publications(&publisher).unwrap()),
        vec!["upload:published"]
    );
    assert!(host.run_solar_publications(&publisher).unwrap().is_empty());
    // Sealed once more after publication — the observer replayed — the
    // published row stands and nothing is re-queued.
    assert!(matches!(
        solar_seal(&h, PROJECT, "city-1", "job-1", b"published twice", &freed),
        SolarSeal::AlreadyRecorded {
            state: ds_sync_runtime::store::ArtifactState::Published,
            ..
        }
    ));
    assert_eq!(publisher.calls.load(Ordering::SeqCst), 1);
    assert_eq!(fakes::lock(&h.world).transfers.len(), 1);
    assert_eq!(stored_rows(&h, PROJECT).len(), 1);
}

/// Invariant (3), Solar: two pumps over one store and one project — one
/// holds the lease and publishes, the other records that it is held, names
/// the holder and leaves; once released, the next pump finds nothing left.
#[test]
fn invariant_3_solar_two_pumps_one_store_one_project_one_holds_the_lease_the_other_leaves() {
    struct NestedPump<'a> {
        h: &'a Harness,
        gateway: &'a FakeGateway,
        producer: &'a FakeProducer,
        reads: &'a FakeReads,
        other_receipts: Mutex<Vec<ds_sync_runtime::Receipt>>,
        calls: AtomicUsize,
    }
    impl SolarPublisher for NestedPump<'_> {
        fn publish_solar(
            &self,
            _row: &LocalRow,
            guard: &dyn Fn() -> Result<(), String>,
            transfer: &dyn Fn(&str, &str) -> Result<TransferReceipt, String>,
        ) -> Result<SolarPublishOutcome, String> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            guard()?;
            // While this pump holds the lease, a second pump over the same
            // store and project tries to drain.
            let online = || true;
            let other = solar_pump(
                self.h,
                self.gateway,
                self.producer,
                self.reads,
                &online,
                "pump#other",
                PROJECT,
            );
            let inner = ScriptedSolarPublisher::new(Ok(SolarPublishOutcome::Published {
                work_id: "work-other".into(),
                head_revision: 9,
            }));
            let receipts = other.run_solar_publications(&inner).unwrap();
            assert_eq!(
                inner.calls.load(Ordering::SeqCst),
                0,
                "the other published nothing"
            );
            *self.other_receipts.lock().unwrap() = receipts;
            let _ = transfer("report-input", "https://storage.example.test/solar-upload")?;
            Ok(SolarPublishOutcome::Published {
                work_id: "work-1".into(),
                head_revision: 1,
            })
        }
    }
    let h = Harness::new();
    solar_seal(&h, PROJECT, "city-1", "job-1", b"leased", &freed());
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = FakeProducer::new(h.world.clone(), Vec::new());
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let publisher = NestedPump {
        h: &h,
        gateway: &gateway,
        producer: &producer,
        reads: &reads,
        other_receipts: Mutex::new(Vec::new()),
        calls: AtomicUsize::new(0),
    };
    let holder = solar_pump(
        &h,
        &gateway,
        &producer,
        &reads,
        &online,
        "pump#holder",
        PROJECT,
    );
    let receipts = holder.run_solar_publications(&publisher).unwrap();
    assert_eq!(solar_outcomes(&receipts), vec!["upload:published"]);
    assert_eq!(publisher.calls.load(Ordering::SeqCst), 1);
    let other = publisher.other_receipts.lock().unwrap();
    assert_eq!(solar_outcomes(&other), vec!["lease:held"]);
    assert!(
        other[0]
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("lease_held") && detail.contains("pump#holder")),
        "the holder is named: {:?}",
        other[0].detail
    );
    drop(other);
    drop(holder);
    // Released: the next pump drains, and there is nothing left to drain.
    let next = solar_pump(
        &h,
        &gateway,
        &producer,
        &reads,
        &online,
        "pump#next",
        PROJECT,
    );
    let inner = ScriptedSolarPublisher::new(Ok(SolarPublishOutcome::Published {
        work_id: "work-next".into(),
        head_revision: 2,
    }));
    assert!(next.run_solar_publications(&inner).unwrap().is_empty());
    assert_eq!(fakes::lock(&h.world).transfers.len(), 1);
}

/// Invariant (4), Solar: two projects' rows interleave in one store, and
/// each pump publishes under its own scope only.
#[test]
fn invariant_4_solar_two_projects_rows_interleave_and_each_pump_publishes_its_own_scope_only() {
    use ds_sync_runtime::store::ArtifactState;
    const OTHER: &str = "p2";
    let h = Harness::new();
    let freed = freed();
    solar_seal(&h, PROJECT, "city-1", "job-p1-a", b"ours, first", &freed);
    solar_seal(&h, OTHER, "city-1", "job-p2", b"theirs", &freed);
    solar_seal(&h, PROJECT, "city-2", "job-p1-b", b"ours, second", &freed);
    // The same city name in two projects is two identities under two
    // scopes: nothing was superseded across the fence of a project.
    assert!(freed.lock().unwrap().is_empty());
    let queue = h
        .store
        .lock()
        .unwrap()
        .queue(&fence(), None, 2_000)
        .unwrap();
    assert_eq!(queue.queued_batches, 3);
    assert_eq!(queue.projects.len(), 2);

    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = FakeProducer::new(h.world.clone(), Vec::new());
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let publisher = ScriptedSolarPublisher::new(Ok(SolarPublishOutcome::Published {
        work_id: "work".into(),
        head_revision: 1,
    }));
    let ours = solar_pump(&h, &gateway, &producer, &reads, &online, "pump#p1", PROJECT);
    let receipts = ours.run_solar_publications(&publisher).unwrap();
    assert_eq!(
        solar_outcomes(&receipts),
        vec!["upload:published", "upload:published"]
    );
    assert!(stored_rows(&h, PROJECT)
        .iter()
        .all(|row| row.state == ArtifactState::Published));
    assert_eq!(stored_rows(&h, OTHER)[0].state, ArtifactState::Held);
    assert_eq!(publisher.calls.load(Ordering::SeqCst), 2);

    let theirs = solar_pump(&h, &gateway, &producer, &reads, &online, "pump#p2", OTHER);
    let receipts = theirs.run_solar_publications(&publisher).unwrap();
    assert_eq!(solar_outcomes(&receipts), vec!["upload:published"]);
    assert_eq!(stored_rows(&h, OTHER)[0].state, ArtifactState::Published);
    assert_eq!(publisher.calls.load(Ordering::SeqCst), 3);
    let queue = h
        .store
        .lock()
        .unwrap()
        .queue(&fence(), None, 3_000)
        .unwrap();
    assert_eq!(queue.queued_batches, 0);
    assert_eq!(queue.held_batches, 3);
}

/// Invariant (5), Solar: a row sealed under one fence is invisible to a
/// pump under another account, deployment or install of the same file —
/// not read, not published, not counted — while the whole file's queue
/// still counts it for a status with no identity to name a fence.
#[test]
fn invariant_5_solar_a_row_sealed_under_one_fence_is_invisible_to_every_other_fence() {
    use ds_sync_runtime::store::ArtifactState;
    let h = Harness::new();
    solar_seal(&h, PROJECT, "city-1", "job-1", b"fenced", &freed());
    let other_fences = [
        Fence {
            account: "account-2".into(),
            ..fence()
        },
        Fence {
            deployment: "https://canary.example.com".into(),
            ..fence()
        },
        Fence {
            install_id: "install-2".into(),
            ..fence()
        },
    ];
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = FakeProducer::new(h.world.clone(), Vec::new());
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let publisher = ScriptedSolarPublisher::new(Ok(SolarPublishOutcome::Published {
        work_id: "work".into(),
        head_revision: 1,
    }));
    for other in &other_fences {
        let host = StoreHost::new(
            h.store.clone(),
            other.clone(),
            "pump#other".into(),
            PROJECT,
            &gateway,
            &producer,
            &reads,
            &online,
        )
        .unwrap();
        assert!(host.run_solar_publications(&publisher).unwrap().is_empty());
        assert!(host.store_read().unwrap()["artifacts"]
            .as_array()
            .unwrap()
            .is_empty());
        let queue = h.store.lock().unwrap().queue(other, None, 2_000).unwrap();
        assert_eq!(queue.queued_batches, 0);
    }
    assert_eq!(publisher.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        h.store
            .lock()
            .unwrap()
            .queue_all(None, 2_000)
            .unwrap()
            .queued_batches,
        1,
        "the file holds it; the fence decides who drains it"
    );
    let mine = solar_pump(
        &h,
        &gateway,
        &producer,
        &reads,
        &online,
        "pump#mine",
        PROJECT,
    );
    assert_eq!(
        solar_outcomes(&mine.run_solar_publications(&publisher).unwrap()),
        vec!["upload:published"]
    );
    assert_eq!(stored_rows(&h, PROJECT)[0].state, ArtifactState::Published);
    // A Solar row sealed under another fence never supersedes this one.
    let SolarSeal::Sealed(theirs) = solar_seal_under(
        &h,
        &other_fences[0],
        PROJECT,
        "city-1",
        "job-other",
        b"another account's",
        &freed(),
    ) else {
        panic!("sealed under the other fence")
    };
    assert!(theirs.superseded.is_none());
    assert_eq!(stored_rows(&h, PROJECT)[0].replay_key, "job-1");
}

/// Invariant (6), Solar: a pump that loses its authority between the
/// transfer and the acknowledgement records nothing — the row stays held,
/// never published — and the next pump resumes it; a cancellation racing
/// the publication keeps the row refused (`solar_cancellation_racing_...`).
#[test]
fn invariant_6_solar_a_pump_aborted_between_receipts_leaves_the_row_held_and_the_next_resumes() {
    use ds_sync_runtime::store::ArtifactState;
    use std::sync::atomic::AtomicBool;
    struct AbortingPublisher(Arc<AtomicBool>, AtomicUsize);
    impl SolarPublisher for AbortingPublisher {
        fn publish_solar(
            &self,
            _row: &LocalRow,
            guard: &dyn Fn() -> Result<(), String>,
            transfer: &dyn Fn(&str, &str) -> Result<TransferReceipt, String>,
        ) -> Result<SolarPublishOutcome, String> {
            self.1.fetch_add(1, Ordering::SeqCst);
            guard()?;
            let _ = transfer("report-input", "https://storage.example.test/solar-upload")?;
            // The abort: the host's authority is gone from here on.
            self.0.store(false, Ordering::SeqCst);
            Ok(SolarPublishOutcome::Published {
                work_id: "work-1".into(),
                head_revision: 1,
            })
        }
    }
    let h = Harness::new();
    solar_seal(
        &h,
        PROJECT,
        "city-1",
        "job-1",
        b"aborted mid-pass",
        &freed(),
    );
    let allowed = Arc::new(AtomicBool::new(true));
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = FakeProducer::new(h.world.clone(), Vec::new());
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || allowed.load(Ordering::SeqCst);
    let publisher = AbortingPublisher(allowed.clone(), AtomicUsize::new(0));
    let host = solar_pump(&h, &gateway, &producer, &reads, &online, "pump#1", PROJECT);
    let error = host
        .run_solar_publications(&publisher)
        .expect_err("the pass is fenced before it acknowledges");
    assert!(error.contains("unavailable"), "{error}");
    assert_eq!(publisher.1.load(Ordering::SeqCst), 1);
    let rows = stored_rows(&h, PROJECT);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].state, ArtifactState::Held, "held, never published");
    assert!(host.store_read().unwrap()["heads"]
        .as_array()
        .unwrap()
        .is_empty());
    drop(host);

    allowed.store(true, Ordering::SeqCst);
    let publisher = ScriptedSolarPublisher::new(Ok(SolarPublishOutcome::Published {
        work_id: "work-1".into(),
        head_revision: 1,
    }));
    let resumed = solar_pump(&h, &gateway, &producer, &reads, &online, "pump#2", PROJECT);
    assert_eq!(
        solar_outcomes(&resumed.run_solar_publications(&publisher).unwrap()),
        vec!["upload:published"]
    );
    assert_eq!(stored_rows(&h, PROJECT)[0].state, ArtifactState::Published);
    assert_eq!(stored_rows(&h, PROJECT).len(), 1);
}

/// Invariant (7), Solar: `published` is set only from the authority's
/// own published receipt, never from transport success. A transfer that
/// succeeds while the authority never commits leaves the row held and no
/// head; a stale answer is a conflict with no head.
#[test]
fn invariant_7_solar_transfer_success_without_a_published_receipt_leaves_the_row_held() {
    use ds_sync_runtime::store::ArtifactState;
    let h = Harness::new();
    solar_seal(
        &h,
        PROJECT,
        "city-1",
        "job-1",
        b"uploaded, never committed",
        &freed(),
    );
    let gateway = FakeGateway {
        world: h.world.clone(),
    };
    let producer = FakeProducer::new(h.world.clone(), Vec::new());
    let reads = FakeReads {
        world: h.world.clone(),
        root: h.reads_root.clone(),
    };
    let online = || true;
    let never = ScriptedSolarPublisher::new(Err("the authority never answered".into()));
    let host = solar_pump(&h, &gateway, &producer, &reads, &online, "pump#1", PROJECT);
    assert_eq!(
        solar_outcomes(&host.run_solar_publications(&never).unwrap()),
        vec!["upload:failed"]
    );
    assert_eq!(
        fakes::lock(&h.world).transfers.len(),
        1,
        "the transport succeeded"
    );
    let rows = stored_rows(&h, PROJECT);
    assert_eq!(rows[0].state, ArtifactState::Held);
    assert!(host.store_read().unwrap()["heads"]
        .as_array()
        .unwrap()
        .is_empty());
    // Still queued.
    assert_eq!(
        h.store
            .lock()
            .unwrap()
            .queue(&fence(), Some(PROJECT), 2_000)
            .unwrap()
            .queued_batches,
        1
    );
    // A stale answer: terminal, and still no head.
    let stale = ScriptedSolarPublisher::new(Ok(SolarPublishOutcome::StoredStale {
        work_id: Some("work-stale".into()),
        detail: "the snapshot changed".into(),
    }));
    assert_eq!(
        solar_outcomes(&host.run_solar_publications(&stale).unwrap()),
        vec!["upload:stored_stale"]
    );
    assert_eq!(stored_rows(&h, PROJECT)[0].state, ArtifactState::Conflict);
    assert!(host.store_read().unwrap()["heads"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(host.run_solar_publications(&stale).unwrap().is_empty());
}
