//! A result sealed while a pass mirrors a long project publishes within one
//! download step, not after the last head. ADERM, 10-23: a fresh Core held 16
//! sealed Solar publications for over an hour behind a ~950 MB mirror. Driven
//! through the real `StoreHost` over a real store on disk, against the fakes.
//!
//! The same mirror proves the stop: ADERM 10-23, a Core still mirroring 11 min
//! after SIGTERM was killed, and its restart waited out the lease. A stop set
//! mid-mirror ends the pass after the step in flight and releases the lease.

#[path = "../src/test_directory.rs"]
mod test_directory;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use ds_sync_runtime::fakes::{self, download_url, local_row, FakeGateway, FakeProducer, FakeReads};
use ds_sync_runtime::store::{Event, Fence, Scope};
use ds_sync_runtime::{
    kernel_sync as sync, open_store, run, run_until_stopped, Gateway, GatewayError, LocalRow,
    Reads, SharedStore, StoreHost, SyncRoute, SyncRun,
};
use serde_json::{json, Value};
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
struct SealingReads<'a> {
    inner: FakeReads,
    store: SharedStore,
    after: usize,
    landed: Mutex<usize>,
    pending: Mutex<Option<LocalRow>>,
    /// Set with the seal: the host is asked to stop while that head lands.
    stop: Option<&'a AtomicBool>,
}

impl Reads for SealingReads<'_> {
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
            if let Some(stop) = self.stop {
                stop.store(true, Ordering::Release);
            }
        }
        Ok(())
    }
}

/// The record's door, asking the host to stop once it has committed
/// `after_commits` publications: a stop that arrives while an upload runs.
struct StoppingGateway<'a> {
    inner: FakeGateway,
    after_commits: Option<u32>,
    stop: &'a AtomicBool,
}

impl Gateway for StoppingGateway<'_> {
    fn post(&self, route: SyncRoute, body: &Value) -> Result<Value, GatewayError> {
        let answer = self.inner.post(route, body);
        if self
            .after_commits
            .is_some_and(|after| fakes::lock(&self.inner.world).commits >= after)
        {
            self.stop.store(true, Ordering::Release);
        }
        answer
    }
}

/// How one pass is driven; see [`Mirror::pass_as`].
struct Pass {
    worker: &'static str,
    trigger: sync::Trigger,
    after: usize,
    mid_pass: Option<LocalRow>,
    stop_at_head: bool,
    stop_after_commit: Option<u32>,
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
        let dir = test_directory::tempdir().expect("temp dir");
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
        self.pass_as(Pass {
            worker: "install-1#100",
            trigger,
            after,
            mid_pass,
            stop_at_head: false,
            stop_after_commit: None,
        })
    }

    /// One pass by `worker`. With `stop_at_head` it runs as the stopping
    /// pump does (`run_until_stopped`) and the stop is set while the `after`-th
    /// head lands; with `stop_after_commit` it is set once the record has
    /// committed that many publications.
    fn pass_as(&self, pass: Pass) -> (SyncRun, usize) {
        let stop = AtomicBool::new(false);
        let gateway = StoppingGateway {
            inner: FakeGateway {
                world: self.world.clone(),
            },
            after_commits: pass.stop_after_commit,
            stop: &stop,
        };
        let producer = FakeProducer::new(self.world.clone(), Vec::new());
        let reads = SealingReads {
            inner: FakeReads {
                world: self.world.clone(),
                root: self.dir.path().join("sync-downloads"),
            },
            store: self.store.clone(),
            after: pass.after,
            landed: Mutex::new(0),
            pending: Mutex::new(pass.mid_pass),
            stop: pass.stop_at_head.then_some(&stop),
        };
        let online = || true;
        let host = StoreHost::new(
            self.store.clone(),
            fence(),
            pass.worker.into(),
            PROJECT,
            &gateway,
            &producer,
            &reads,
            &online,
        )
        .expect("host");
        let run = if pass.stop_at_head || pass.stop_after_commit.is_some() {
            run_until_stopped(&host, PROJECT, pass.trigger, None, &stop)
        } else {
            run(&host, PROJECT, pass.trigger)
        }
        .expect("the pass runs");
        let landed = *reads.landed.lock().unwrap();
        (run, landed)
    }

    /// The project's live lease holder, if any, as the store answers now.
    fn lease_holder(&self) -> Option<String> {
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
            .lease
            .filter(|lease| lease.expires_at_ms > ds_sync_runtime::now_ms())
            .map(|lease| lease.worker_id)
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

/// Every `pass:stopped` receipt's detail, in order.
fn stops(run: &SyncRun) -> Vec<String> {
    run.receipts
        .iter()
        .filter(|receipt| receipt.action == "pass" && receipt.outcome == "stopped")
        .map(|receipt| receipt.detail.clone().unwrap_or_default())
        .collect()
}

#[test]
fn a_stop_mid_mirror_ends_the_pass_after_the_head_in_flight_and_the_restart_resumes() {
    let mirror = Mirror::new(&["tx_1", "tx_9"]);
    seal(&mirror.store, &result_of("tx_1", '1'));
    // The stop arrives while the second head lands, together with a result
    // sealed by a Solar run finishing at that moment.
    let (stopped, landed) = mirror.pass_as(Pass {
        worker: "install-1#100",
        trigger: sync::Trigger::Manual,
        after: 2,
        mid_pass: Some(result_of("tx_9", '9')),
        stop_at_head: true,
        stop_after_commit: None,
    });
    assert_eq!(
        moves(&stopped),
        [
            "upload:published:export-tx_1",
            "download:downloaded:export-a",
            // In flight when the stop arrived: it finishes, whole.
            "download:downloaded:export-b",
        ]
    );
    assert_eq!(
        stops(&stopped),
        ["the host is stopping; 3 planned upload or download steps wait for the next pass"]
    );
    assert_eq!(landed, 2);
    // The lease is released as the pass returns, not left for its TTL.
    assert_eq!(mirror.lease_holder(), None);
    {
        let world = fakes::lock(&mirror.world);
        assert_eq!(world.downloads, ["export-a", "export-b"]);
        assert_eq!((world.opens, world.commits), (1, 1));
    }
    // What was not reached is still planned; nothing is lost.
    assert_eq!(stopped.replanned.summary.uploads, 1);
    assert_eq!(stopped.replanned.summary.downloads, 3);
    assert_eq!(mirror.state_of("export-tx_9"), "held");

    // The restart is another worker on the same store. It takes the lease at
    // once, publishes the result sealed at the stop, and fetches only the
    // heads the stopped pass did not reach.
    let (restart, landed) = mirror.pass_as(Pass {
        worker: "install-1#200",
        trigger: sync::Trigger::Manual,
        after: 0,
        mid_pass: None,
        stop_at_head: false,
        stop_after_commit: None,
    });
    assert!(
        !restart
            .receipts
            .iter()
            .any(|receipt| receipt.action == "lease"),
        "{:?}",
        restart.receipts
    );
    assert_eq!(
        moves(&restart),
        [
            "upload:published:export-tx_9",
            "download:downloaded:export-c",
            "download:downloaded:export-d",
            "download:downloaded:export-e",
        ]
    );
    assert!(stops(&restart).is_empty());
    assert_eq!(landed, 3);
    let world = fakes::lock(&mirror.world);
    // Every head fetched once, every result declared and committed once.
    assert_eq!(world.downloads.len(), ROOMS.len());
    assert_eq!((world.opens, world.commits), (2, 2));
    assert_eq!(world.transfers.len(), 2);
    drop(world);
    assert_eq!(mirror.state_of("export-tx_9"), "published");
    assert_eq!(restart.replanned.summary.uploads, 0);
    assert_eq!(restart.replanned.summary.downloads, 0);
}

#[test]
fn a_stop_during_an_upload_finishes_that_publication_and_starts_no_other() {
    let mirror = Mirror::new(&["tx_1", "tx_2", "tx_3"]);
    for (room, digit) in [("tx_1", '1'), ("tx_2", '2'), ("tx_3", '3')] {
        seal(&mirror.store, &result_of(room, digit));
    }
    let (stopped, landed) = mirror.pass_as(Pass {
        worker: "install-1#100",
        trigger: sync::Trigger::Manual,
        after: 0,
        mid_pass: None,
        stop_at_head: false,
        stop_after_commit: Some(1),
    });
    assert_eq!(moves(&stopped), ["upload:published:export-tx_1"]);
    assert_eq!(
        stops(&stopped),
        ["the host is stopping; 7 planned upload or download steps wait for the next pass"]
    );
    assert_eq!(landed, 0);
    assert_eq!(mirror.lease_holder(), None);
    {
        let world = fakes::lock(&mirror.world);
        assert_eq!((world.opens, world.commits), (1, 1));
    }
    assert_eq!(mirror.state_of("export-tx_2"), "held");

    let (restart, landed) = mirror.pass(sync::Trigger::LocalChange, 0, None);
    assert_eq!(
        moves(&restart)
            .iter()
            .filter(|step| step.starts_with("upload:"))
            .collect::<Vec<_>>(),
        [
            "upload:published:export-tx_2",
            "upload:published:export-tx_3"
        ]
    );
    assert_eq!(landed, ROOMS.len());
    let world = fakes::lock(&mirror.world);
    // tx_1 is never declared twice.
    assert_eq!((world.opens, world.commits), (3, 3));
    assert_eq!(world.transfers.len(), 3);
}

#[test]
fn without_a_stop_the_stoppable_pass_mirrors_everything_and_a_held_lease_still_fences() {
    // Control: the pump's stoppable pass is the ordinary pass while nobody
    // asks it to stop.
    let mirror = Mirror::new(&["tx_1"]);
    seal(&mirror.store, &result_of("tx_1", '1'));
    let never = AtomicBool::new(false);
    let gateway = FakeGateway {
        world: mirror.world.clone(),
    };
    let producer = FakeProducer::new(mirror.world.clone(), Vec::new());
    let reads = FakeReads {
        world: mirror.world.clone(),
        root: mirror.dir.path().join("sync-downloads"),
    };
    let online = || true;
    let host = StoreHost::new(
        mirror.store.clone(),
        fence(),
        "install-1#100".into(),
        PROJECT,
        &gateway,
        &producer,
        &reads,
        &online,
    )
    .expect("host");
    let pass = run_until_stopped(&host, PROJECT, sync::Trigger::Manual, None, &never)
        .expect("the pass runs");
    assert_eq!(moves(&pass).len(), 1 + ROOMS.len());
    assert!(stops(&pass).is_empty());
    assert_eq!(pass.replanned.summary.downloads, 0);
    assert_eq!(fakes::lock(&mirror.world).downloads.len(), ROOMS.len());

    // Control for the release: a worker that never released (a killed
    // process) still holds the project, and a second worker executes nothing
    // until that lease expires.
    let taken = mirror
        .store
        .lock()
        .unwrap()
        .apply(
            &fence(),
            ds_sync_runtime::now_ms(),
            Event::LeaseTake {
                scope: Scope::Project {
                    project: PROJECT.into(),
                },
                worker_id: "install-1#killed".into(),
                ttl_ms: 300_000,
            },
        )
        .unwrap();
    assert!(taken.refusals.is_empty());
    seal(&mirror.store, &result_of("tx_1", '7'));
    let (fenced, landed) = mirror.pass(sync::Trigger::LocalChange, 0, None);
    assert!(fenced
        .receipts
        .iter()
        .any(|receipt| receipt.action == "lease" && receipt.outcome == "held"));
    assert!(moves(&fenced).is_empty());
    assert_eq!(landed, 0);
    assert_eq!(fakes::lock(&mirror.world).opens, 1);
}

#[test]
fn a_stop_inside_a_head_keeps_its_verified_outputs_for_the_restart() {
    // ADERM 10-24: one Solar report head carried dozens of plot maps, so a
    // stop that waited for the whole head still waited for minutes.
    let mirror = Mirror::new(&[]);
    let outputs = ["map-1", "map-2", "map-3"];
    {
        let mut world = fakes::lock(&mirror.world);
        let mut declared = Vec::new();
        for output in outputs {
            let bytes = format!("plot map {output}").into_bytes();
            declared.push(json!({
                "output_id": output,
                "sha256": format!("{:x}", sha2::Sha256::digest(&bytes)),
                "size_bytes": bytes.len(),
            }));
            world
                .objects
                .insert(download_url("export-big", output), bytes);
        }
        world.record.push(json!({
            "engine": "network_reporter",
            "operation": "export-big",
            "variant": "default",
            "head_revision": 9,
            "input_base_fingerprint": fakes::FINGERPRINT,
            "updated_at": "2026-10-24T00:00:00Z",
            "outputs": declared,
        }));
    }
    // Heads plan in identity order: a, b, big, c, d, e. The stop arrives
    // while big's first output lands.
    let (stopped, landed) = mirror.pass_as(Pass {
        worker: "install-1#100",
        trigger: sync::Trigger::Manual,
        after: 3,
        mid_pass: None,
        stop_at_head: true,
        stop_after_commit: None,
    });
    assert_eq!(
        moves(&stopped),
        [
            "download:downloaded:export-a",
            "download:downloaded:export-b"
        ]
    );
    assert_eq!(
        stops(&stopped),
        ["the host is stopping; 4 planned upload or download steps wait for the next pass"]
    );
    assert_eq!(landed, 3);
    assert_eq!(mirror.lease_holder(), None);
    let root = mirror
        .dir
        .path()
        .join("sync-downloads")
        .join(PROJECT)
        .join("network_reporter")
        .join("export-big")
        .join("default");
    // The head is not placed: no reader can take a partial head for it.
    assert!(!root.join("head.json").exists());
    assert!(root.join("map-1").is_file());
    assert!(!root.join("map-2").exists());

    let (restart, landed) = mirror.pass_as(Pass {
        worker: "install-1#200",
        trigger: sync::Trigger::Manual,
        after: 0,
        mid_pass: None,
        stop_at_head: false,
        stop_after_commit: None,
    });
    assert_eq!(
        moves(&restart),
        [
            "download:downloaded:export-big",
            "download:downloaded:export-c",
            "download:downloaded:export-d",
            "download:downloaded:export-e",
        ]
    );
    // map-1 is kept, not read again: every object crossed the wire once.
    assert_eq!(landed, 2 + 3);
    let world = fakes::lock(&mirror.world);
    assert_eq!(
        world
            .downloads
            .iter()
            .filter(|operation| *operation == "export-big")
            .count(),
        outputs.len()
    );
    assert_eq!(world.downloads.len(), ROOMS.len() + outputs.len());
    drop(world);
    assert!(root.join("head.json").is_file());
    assert_eq!(restart.replanned.summary.downloads, 0);
}
