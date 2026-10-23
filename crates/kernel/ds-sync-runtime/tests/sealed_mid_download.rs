//! A result sealed while a pass mirrors a long project publishes within one
//! download step, not after the last head. ADERM, 10-23: a fresh Core held 16
//! sealed Solar publications for over an hour behind a ~950 MB mirror. Driven
//! through the real `StoreHost` over a real store on disk, against the fakes.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ds_sync_runtime::fakes::{self, download_url, local_row, FakeGateway, FakeProducer, FakeReads};
use ds_sync_runtime::store::{Event, Fence, Scope};
use ds_sync_runtime::{
    kernel_sync as sync, open_store, run, LocalRow, Reads, SharedStore, StoreHost, SyncRun,
};
use serde_json::json;
use sha2::Digest;

const PROJECT: &str = "p1";
/// The shared heads every pass here mirrors, one download step each.
const ROOMS: [&str; 5] = ["a", "b", "c", "d", "e"];

fn fence() -> Fence {
    Fence {
        account: "account-1".into(),
        deployment: "https://gateway.example.com".into(),
        install_id: "install-1".into(),
    }
}

fn seal(store: &SharedStore, row: &LocalRow) {
    store
        .lock()
        .unwrap()
        .apply(
            &fence(),
            ds_sync_runtime::now_ms(),
            Event::LocalProduced {
                row: ds_sync_runtime::rows::store_row(PROJECT, row, 1_000),
            },
        )
        .expect("the seal records its row");
}

/// This machine's own result for `room`, produced after every shared head.
fn result_of(room: &str, digit: char) -> LocalRow {
    let mut row = local_row(format!("local bytes of {room}").as_bytes());
    row.identity.operation = format!("export-{room}");
    row.resource = room.into();
    row.produced_at_ms = ds_sync_runtime::now_ms();
    let run = |count: usize| std::iter::repeat_n(digit, count).collect::<String>();
    row.client_publish_id = format!(
        "publish-{}-{}-4{}-8{}-{}",
        run(8),
        run(4),
        run(3),
        run(3),
        run(12)
    );
    row.bytes_locator = format!("fake:batch:{}", row.client_publish_id);
    row
}

/// One Solar publication whose only output is the calculation's report input.
fn solar(operation: &str, variant: &str, bytes: &[u8], fingerprint: &str, id: &str) -> LocalRow {
    let output = ds_sync_runtime::LocalOutput {
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
        produced_at_ms: ds_sync_runtime::now_ms(),
        base_revision: None,
        input_base_fingerprint: Some(fingerprint.into()),
        readable: true,
        engine_release: "ds-solar-engine@0.1.0".into(),
        engine_build_manifest_sha256: "a".repeat(64),
        resource: "aderm_bere".into(),
        client_publish_id: id.into(),
        outputs: vec![output],
        bytes_locator: format!("solar:prepared:job:{id}"),
    }
}

/// Downloads through the fake objects; the `after`-th head that lands seals
/// `pending` into the store, as a Solar run finishing mid-mirror does.
struct SealingReads {
    inner: FakeReads,
    store: SharedStore,
    after: usize,
    landed: Mutex<usize>,
    pending: Mutex<Option<LocalRow>>,
}

impl Reads for SealingReads {
    fn head_root(&self, project: &str, identity: &sync::Identity) -> Result<PathBuf, String> {
        self.inner.head_root(project, identity)
    }

    fn download_verified(
        &self,
        url: &str,
        destination: &Path,
        sha256: &str,
        size_bytes: u64,
    ) -> Result<(), String> {
        self.inner
            .download_verified(url, destination, sha256, size_bytes)?;
        let mut landed = self.landed.lock().unwrap();
        *landed += 1;
        if *landed == self.after {
            if let Some(row) = self.pending.lock().unwrap().take() {
                seal(&self.store, &row);
            }
        }
        Ok(())
    }
}

struct Mirror {
    dir: tempfile::TempDir,
    store: SharedStore,
    world: fakes::SharedWorld,
}

impl Mirror {
    /// A record holding one shared head per room, every one of which this
    /// machine has yet to mirror, and accepting this machine's results.
    fn new(results: &[&str]) -> Self {
        let dir = tempfile::tempdir().expect("temp dir");
        let store = open_store(&dir.path().join("store.sqlite")).expect("open store");
        let world = fakes::world();
        {
            let mut world = fakes::lock(&world);
            for (revision, room) in ROOMS.iter().enumerate() {
                let operation = format!("export-{room}");
                let bytes = format!("shared bytes of {room}").into_bytes();
                world
                    .objects
                    .insert(download_url(&operation, "xlsx"), bytes.clone());
                world.record.push(json!({
                    "engine": "network_reporter",
                    "operation": operation,
                    "variant": "default",
                    "head_revision": revision + 1,
                    "input_base_fingerprint": fakes::FINGERPRINT,
                    "updated_at": "2026-10-23T18:00:00Z",
                    "outputs": [{
                        "output_id": "xlsx",
                        "sha256": format!("{:x}", sha2::Sha256::digest(&bytes)),
                        "size_bytes": bytes.len(),
                    }],
                }));
            }
            for room in results {
                world
                    .current_base
                    .insert(format!("export-{room}"), fakes::FINGERPRINT.into());
            }
        }
        Mirror { dir, store, world }
    }

    /// One pass, as the Core pump runs it, sealing `mid_pass` once the
    /// `after`-th head has landed.
    fn pass(
        &self,
        trigger: sync::Trigger,
        after: usize,
        mid_pass: Option<LocalRow>,
    ) -> (SyncRun, usize) {
        let gateway = FakeGateway {
            world: self.world.clone(),
        };
        let producer = FakeProducer::new(self.world.clone(), Vec::new());
        let reads = SealingReads {
            inner: FakeReads {
                world: self.world.clone(),
                root: self.dir.path().join("sync-downloads"),
            },
            store: self.store.clone(),
            after,
            landed: Mutex::new(0),
            pending: Mutex::new(mid_pass),
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
        let run = run(&host, PROJECT, trigger).expect("the pass runs");
        let landed = *reads.landed.lock().unwrap();
        (run, landed)
    }

    fn state_of(&self, operation: &str) -> String {
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
            .state
            .as_str()
            .to_string()
    }
}

/// `action:outcome:operation` for every upload and download receipt, in order.
fn moves(run: &SyncRun) -> Vec<String> {
    run.receipts
        .iter()
        .filter(|receipt| matches!(receipt.action, "upload" | "download"))
        .map(|receipt| {
            format!(
                "{}:{}:{}",
                receipt.action,
                receipt.outcome,
                receipt
                    .identity
                    .as_ref()
                    .map(|identity| identity.operation.as_str())
                    .unwrap_or_default()
            )
        })
        .collect()
}

#[test]
fn a_result_sealed_mid_mirror_publishes_within_one_download_step() {
    let mirror = Mirror::new(&["tx_1", "tx_9"]);
    // Sealed before the pass: it leads, as it already did.
    seal(&mirror.store, &result_of("tx_1", '1'));
    let (pass, landed) = mirror.pass(sync::Trigger::Manual, 1, Some(result_of("tx_9", '9')));

    assert_eq!(
        moves(&pass),
        [
            "upload:published:export-tx_1",
            "download:downloaded:export-a",
            // Sealed while the first head downloaded: published before the
            // second head is fetched, not after the fifth.
            "upload:published:export-tx_9",
            "download:downloaded:export-b",
            "download:downloaded:export-c",
            "download:downloaded:export-d",
            "download:downloaded:export-e",
        ]
    );
    // Control: the mirror still completes, and each result is declared and
    // committed exactly once.
    assert_eq!(landed, ROOMS.len());
    let world = fakes::lock(&mirror.world);
    assert_eq!(world.downloads.len(), ROOMS.len());
    assert_eq!((world.opens, world.commits), (2, 2));
    assert_eq!(world.transfers.len(), 2);
    drop(world);
    assert_eq!(mirror.state_of("export-tx_9"), "published");
    assert_eq!(pass.replanned.summary.uploads, 0);
    assert_eq!(pass.replanned.summary.downloads, 0);

    // The next pass finds everything in sync: nothing is uploaded or fetched
    // twice.
    let (again, landed) = mirror.pass(sync::Trigger::LocalChange, 0, None);
    assert!(moves(&again).is_empty(), "{:?}", moves(&again));
    assert_eq!(landed, 0);
    let world = fakes::lock(&mirror.world);
    assert_eq!((world.opens, world.commits), (2, 2));
    assert_eq!(world.downloads.len(), ROOMS.len());
}

#[test]
fn a_head_this_pass_replaced_with_its_own_result_is_not_fetched() {
    // The result sealed mid-mirror is a newer computation of a room whose
    // older shared head is still later in this pass's download list.
    let mirror = Mirror::new(&["e"]);
    let (pass, landed) = mirror.pass(sync::Trigger::Manual, 1, Some(result_of("e", '5')));
    assert_eq!(
        moves(&pass),
        [
            "download:downloaded:export-a",
            "upload:published:export-e",
            "download:downloaded:export-b",
            "download:downloaded:export-c",
            "download:downloaded:export-d",
        ]
    );
    assert!(pass
        .receipts
        .iter()
        .any(|receipt| receipt.action == "nothing"
            && receipt.outcome == "in_sync"
            && receipt
                .identity
                .as_ref()
                .is_some_and(|identity| identity.operation == "export-e")));
    assert_eq!(landed, ROOMS.len() - 1);
    let world = fakes::lock(&mirror.world);
    assert!(!world
        .downloads
        .iter()
        .any(|operation| operation == "export-e"));
    assert_eq!((world.opens, world.commits), (1, 1));
    drop(world);
    assert_eq!(mirror.state_of("export-e"), "published");
}

#[test]
fn a_result_the_record_held_is_asked_once_per_pass_however_many_steps_follow() {
    // Control for the yield: a row that already had its turn this pass and
    // was held (a 503 at `open`) is not asked again before every download.
    let mirror = Mirror::new(&["tx_1"]);
    seal(&mirror.store, &result_of("tx_1", '1'));
    fakes::lock(&mirror.world).refuse_open_resource = Some("tx_1".into());
    let (pass, landed) = mirror.pass(sync::Trigger::Manual, 0, None);
    assert_eq!(
        moves(&pass)
            .iter()
            .filter(|step| step.starts_with("upload:"))
            .collect::<Vec<_>>(),
        ["upload:failed:export-tx_1"]
    );
    assert_eq!(landed, ROOMS.len());
    assert_eq!(fakes::lock(&mirror.world).opens, 1);
    assert_eq!(mirror.state_of("export-tx_1"), "held");

    // The record recovers: the next pass publishes it once.
    fakes::lock(&mirror.world).refuse_open_resource = None;
    let (next, landed) = mirror.pass(sync::Trigger::LocalChange, 0, None);
    assert_eq!(moves(&next), ["upload:published:export-tx_1"]);
    assert_eq!(landed, 0);
    let world = fakes::lock(&mirror.world);
    assert_eq!((world.opens, world.commits), (2, 1));
}

#[test]
fn a_report_waiting_on_a_calculation_this_pass_published_follows_it_before_the_mirror() {
    // ADERM's held rows were Solar calculations and the reports computed
    // from them. The plan holds a report until its calculation's head
    // carries the exact report input, so the report used to wait for the
    // whole mirror and then a second pass.
    let calculation_bytes = b"calculation report input";
    let report_input = format!("{:x}", sha2::Sha256::digest(calculation_bytes));
    let calculation = solar(
        "calculate-aderm_bere",
        "default",
        calculation_bytes,
        fakes::FINGERPRINT,
        "job-calc",
    );
    let report = solar(
        "report-aderm_bere",
        "apd-draft-fr",
        b"report",
        &report_input,
        "job-report",
    );
    let mirror = Mirror::new(&[]);
    {
        let mut world = fakes::lock(&mirror.world);
        world
            .current_base
            .insert("calculate-aderm_bere".into(), fakes::FINGERPRINT.into());
        world
            .current_base
            .insert("report-aderm_bere".into(), report_input.clone());
    }
    seal(&mirror.store, &calculation);
    seal(&mirror.store, &report);
    let (pass, landed) = mirror.pass(sync::Trigger::Manual, 0, None);
    assert!(pass.plan.actions.iter().any(|action| matches!(
        action,
        sync::Action::Nothing { identity, reason }
            if identity.operation == "report-aderm_bere" && *reason == "waiting_on_dependency"
    )));
    assert_eq!(
        moves(&pass),
        [
            "upload:published:calculate-aderm_bere",
            "upload:published:report-aderm_bere",
            "download:downloaded:export-a",
            "download:downloaded:export-b",
            "download:downloaded:export-c",
            "download:downloaded:export-d",
            "download:downloaded:export-e",
        ]
    );
    assert_eq!(landed, ROOMS.len());
    assert_eq!(fakes::lock(&mirror.world).commits, 2);
    assert_eq!(mirror.state_of("report-aderm_bere"), "published");
    assert_eq!(pass.replanned.summary.uploads, 0);
}
