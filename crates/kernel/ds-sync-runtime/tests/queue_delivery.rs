//! The Sync Center queue delivers without manual nudging: a queued row the
//! shared record already carries leaves the queue as published, and a result
//! whose prerequisite was refused parks with a typed reason. Driven through the
//! real `StoreHost` over a real store on disk, against the deterministic fakes.

#[path = "../src/test_directory.rs"]
mod test_directory;
use ds_sync_runtime::fakes::{
    self, local_row, record_head, FakeGateway, FakeProducer, FakeReads, SharedWorld,
};
use ds_sync_runtime::store::{Fence, Scope};
use ds_sync_runtime::{
    kernel_sync as sync, open_store, run, LocalOutput, LocalRow, Receipt, SharedStore, StoreHost,
    SyncHost, SyncRun,
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

struct Queue {
    _dir: tempfile::TempDir,
    store: SharedStore,
    world: SharedWorld,
    reads_root: std::path::PathBuf,
}

impl Queue {
    fn new() -> Self {
        let dir = test_directory::tempdir().expect("temp dir");
        let store = open_store(&dir.path().join("store.sqlite")).expect("open store");
        Queue {
            reads_root: dir.path().join("sync-downloads"),
            _dir: dir,
            store,
            world: fakes::world(),
        }
    }

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

    /// One pass through a fresh host, as the Core pump runs it.
    fn pass(&self, rows: &[LocalRow], trigger: sync::Trigger) -> SyncRun {
        self.with_host(rows, |host| run(host, PROJECT, trigger))
    }

    fn with_host<T>(
        &self,
        rows: &[LocalRow],
        f: impl FnOnce(&StoreHost<'_>) -> Result<T, String>,
    ) -> T {
        let gateway = FakeGateway {
            world: self.world.clone(),
        };
        let producer = FakeProducer::new(self.world.clone(), rows.to_vec());
        let reads = FakeReads {
            world: self.world.clone(),
            root: self.reads_root.clone(),
        };
        let online = || true;
        let host = StoreHost::new(
            self.store.clone(),
            fence(),
            "install-1#100".into(),
            PROJECT,
            &gateway,
            &producer,
            &reads,
            &online,
        )
        .expect("host");
        f(&host).expect("the host call runs")
    }

    fn row_of(&self, operation: &str) -> ds_sync_runtime::store::ArtifactRow {
        self.store
            .lock()
            .unwrap()
            .snapshot(
                &fence(),
                &Scope::Project {
                    project: PROJECT.into(),
                },
            )
            .unwrap()
            .artifacts
            .into_iter()
            .find(|row| row.identity.operation == operation)
            .expect("the row is in the store")
    }

    fn counts(&self) -> ds_sync_runtime::store::DeliveryCounts {
        self.store
            .lock()
            .unwrap()
            .queue(&fence(), Some(PROJECT), ds_sync_runtime::now_ms())
            .unwrap()
            .counts
    }
}

fn outcomes(run: &SyncRun) -> Vec<String> {
    run.receipts
        .iter()
        .map(|receipt| format!("{}:{}", receipt.action, receipt.outcome))
        .collect()
}

#[test]
fn a_queued_row_the_record_already_carries_is_settled_published_without_an_upload() {
    // arjgpydw_nyam: 94 exports published on 09-28, re-adopted as held on
    // 10-15, and planned `in_sync` by every pass while the Sync Center showed
    // them queued. One pass now settles them, and no byte moves.
    let queue = Queue::new();
    let bytes = b"already published export";
    let row = local_row(bytes);
    queue.seal(std::slice::from_ref(&row));
    fakes::lock(&queue.world).record = vec![record_head(bytes, 3, fakes::FINGERPRINT)];
    assert_eq!(queue.counts().queued, 1);

    let first = queue.pass(std::slice::from_ref(&row), sync::Trigger::Startup);
    assert_eq!(outcomes(&first), vec!["in_sync:published"]);
    let world = fakes::lock(&queue.world);
    assert_eq!(world.opens, 0, "nothing is declared again");
    assert_eq!(world.commits, 0, "the record does not advance");
    assert!(world.transfers.is_empty(), "no byte moves");
    drop(world);
    assert_eq!(queue.row_of("export-tx_1").state.as_str(), "published");
    let counts = queue.counts();
    assert_eq!((counts.queued, counts.published), (0, 1));

    // The next pass finds a published row: nothing to settle, no new receipt.
    let second = queue.pass(std::slice::from_ref(&row), sync::Trigger::Manual);
    assert_eq!(outcomes(&second), vec!["nothing:in_sync"]);
}

#[test]
fn a_queued_row_the_record_does_not_carry_still_uploads() {
    // Control: settlement never stands in for an upload.
    let queue = Queue::new();
    let row = local_row(b"new export");
    queue.seal(std::slice::from_ref(&row));
    let pass = queue.pass(std::slice::from_ref(&row), sync::Trigger::Startup);
    assert_eq!(outcomes(&pass), vec!["upload:published"]);
    assert_eq!(fakes::lock(&queue.world).commits, 1);
    assert_eq!(queue.counts().published, 1);
}

fn solar(operation: &str, variant: &str, bytes: &[u8], publish_id: &str) -> LocalRow {
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
            operation: operation.into(),
            variant: variant.into(),
        },
        sha256: ds_sync_runtime::inventory_digest(&mut pairs),
        size_bytes: output.size_bytes,
        produced_at_ms: 1_000,
        base_revision: None,
        input_base_fingerprint: Some(fakes::FINGERPRINT.into()),
        readable: true,
        engine_release: "ds-solar-engine@0.1.0".into(),
        engine_build_manifest_sha256: "a".repeat(64),
        resource: "aderm_bere".into(),
        client_publish_id: publish_id.into(),
        outputs: vec![output],
        bytes_locator: format!("solar:prepared:job:{publish_id}"),
    }
}

#[test]
fn a_report_whose_calculation_was_refused_parks_and_never_holds_the_queue() {
    // arjgpydw_aderm_loc7: the calculations were refused on 10-16 and their
    // reports waited on them forever, counted as queued.
    let queue = Queue::new();
    let calculation = solar("calculate-aderm_bere", "default", b"calc", "job-calc");
    let report = solar("report-aderm_bere", "apd-draft-fr", b"report", "job-report");
    let export = local_row(b"an independent export");
    queue.seal(&[calculation.clone(), report.clone(), export.clone()]);
    let rows = [calculation.clone(), report.clone(), export.clone()];
    queue.with_host(&rows, |host| {
        host.record_receipt(
            PROJECT,
            &Receipt::new("upload", "refused")
                .about(&calculation.identity)
                .for_publication(&calculation)
                .detail("HTTP 400: edge admission validation failed"),
        )
    });
    assert_eq!(queue.row_of("calculate-aderm_bere").state.as_str(), "refused");

    let pass = queue.pass(&rows, sync::Trigger::LocalChange);
    let outcomes = outcomes(&pass);
    assert!(
        outcomes.contains(&"refused:dependency_refused".to_string()),
        "{outcomes:?}"
    );
    assert!(outcomes.contains(&"upload:published".to_string()), "{outcomes:?}");
    let parked = queue.row_of("report-aderm_bere");
    assert_eq!(parked.state.as_str(), "refused");
    assert!(parked
        .state_reason
        .as_deref()
        .is_some_and(|reason| reason.starts_with("dependency_refused")));
    let counts = queue.counts();
    assert_eq!(
        (counts.queued, counts.published, counts.failed),
        (0, 1, 2),
        "nothing waits; the export published and both Solar rows are failures"
    );
}
