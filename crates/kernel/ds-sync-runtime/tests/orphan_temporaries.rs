//! A killed or crashed Core leaves `.part-*` temporaries in `sync-downloads`
//! (ADERM core-state-2, 10-24: two orphans after a SIGKILL). The next pass's
//! lease holder removes the ones no live step owns; a step's own in-flight
//! temporary, another project's, and anything not named like one stay.
//!
//! Driven through the real `StoreHost` and `place_verified` over a real store.

#[path = "../src/test_directory.rs"]
mod test_directory;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use ds_sync_runtime::fakes::{self, download_url, record_head, FakeGateway, FakeProducer};
use ds_sync_runtime::store::{Fence, Scope};
use ds_sync_runtime::{
    kernel_sync as sync, open_store, run, Reads, SharedStore, StoreHost, SyncHost, VerifiedReads,
};

const PROJECT: &str = "p1";
const HEAD_BYTES: &[u8] = b"the shared report head, several slices long";

fn fence() -> Fence {
    Fence {
        account: "account-1".into(),
        deployment: "https://gateway.example.com".into(),
        install_id: "install-1".into(),
    }
}

/// A file last written an hour ago, as a kill leaves it.
fn plant(path: &Path, bytes: &[u8]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
    let file = std::fs::File::options().write(true).open(path).unwrap();
    file.set_modified(SystemTime::now() - Duration::from_secs(3600))
        .unwrap();
    path.to_path_buf()
}

fn uuid(digit: char) -> String {
    std::iter::repeat_n(digit, 32).collect()
}

fn temporaries_in(directory: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .map(|entry| entry.unwrap().path())
                .filter(|path| {
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.contains(".part-"))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The fake objects read through the real `place_verified`; mid-object, with
/// its own temporary open, the body sweeps again as of this pass's start,
/// which must leave that temporary in place.
struct InFlightReads {
    world: fakes::SharedWorld,
    root: PathBuf,
    pass_started: SystemTime,
    in_flight: Mutex<Vec<PathBuf>>,
}

struct SweepingBody<'a> {
    bytes: &'a [u8],
    offset: usize,
    reads: &'a InFlightReads,
    directory: PathBuf,
}

impl Read for SweepingBody<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
        if self.offset == 8 {
            let open = temporaries_in(&self.directory);
            assert_eq!(open.len(), 1, "the step's own temporary is open");
            let swept = VerifiedReads::new(self.reads.root.clone())
                .sweep_temporaries(PROJECT, self.reads.pass_started)
                .expect("the sweep runs");
            assert_eq!(
                swept,
                (0, 0),
                "nothing written since the pass began is swept"
            );
            assert!(open[0].exists(), "the in-flight temporary survives a sweep");
            self.reads.in_flight.lock().unwrap().extend(open);
        }
        let count = buffer.len().min(8).min(self.bytes.len() - self.offset);
        buffer[..count].copy_from_slice(&self.bytes[self.offset..self.offset + count]);
        self.offset += count;
        Ok(count)
    }
}

impl Reads for InFlightReads {
    fn head_root(&self, project: &str, identity: &sync::Identity) -> Result<PathBuf, String> {
        VerifiedReads::new(self.root.clone()).head_root(project, identity)
    }

    fn download_verified(
        &self,
        url: &str,
        destination: &Path,
        sha256: &str,
        size_bytes: u64,
    ) -> Result<(), String> {
        let bytes = fakes::lock(&self.world)
            .objects
            .get(url)
            .cloned()
            .ok_or("the storage host answered 404")?;
        let body = SweepingBody {
            bytes: &bytes,
            offset: 0,
            reads: self,
            directory: destination.parent().unwrap().to_path_buf(),
        };
        ds_sync_runtime::reads::place_verified(body, destination, sha256, size_bytes)
    }

    fn sweep_temporaries(&self, project: &str, before: SystemTime) -> Result<(usize, u64), String> {
        VerifiedReads::new(self.root.clone()).sweep_temporaries(project, before)
    }
}

struct Cache {
    _dir: tempfile::TempDir,
    store: SharedStore,
    world: fakes::SharedWorld,
    root: PathBuf,
}

impl Cache {
    fn new() -> Self {
        let dir = test_directory::tempdir().expect("temp dir");
        let store = open_store(&dir.path().join("store.sqlite")).expect("open store");
        let world = fakes::world();
        {
            let mut world = fakes::lock(&world);
            world
                .record
                .push(record_head(HEAD_BYTES, 1, fakes::FINGERPRINT));
            world
                .objects
                .insert(download_url("export-tx_1", "xlsx"), HEAD_BYTES.to_vec());
        }
        Cache {
            root: dir.path().join("sync-downloads"),
            _dir: dir,
            store,
            world,
        }
    }

    fn host<'a>(
        &self,
        worker: &str,
        gateway: &'a FakeGateway,
        producer: &'a FakeProducer,
        reads: &'a InFlightReads,
        online: &'a dyn Fn() -> bool,
    ) -> StoreHost<'a> {
        StoreHost::new(
            self.store.clone(),
            fence(),
            worker.into(),
            PROJECT,
            gateway,
            producer,
            reads,
            online,
        )
        .expect("host")
    }
}

#[test]
fn the_lease_holder_removes_orphaned_temporaries_and_never_a_live_steps_own() {
    let cache = Cache::new();
    let head = cache
        .root
        .join(PROJECT)
        .join("network_reporter/export-tx_1/default");
    let report = cache
        .root
        .join(PROJECT)
        .join("solar/report-aderm_kyabe/financial-draft-fr");
    let orphans = [
        plant(&head.join(format!(".xlsx.part-{}", uuid('a'))), b"half"),
        plant(
            &report.join(format!(
                ".plot-maps-784bed2aa464736a73e564762af172cf25ef744b60262e6e8bd04cdf9caca8b9.part-{}",
                uuid('b')
            )),
            b"half a plot map",
        ),
        plant(&report.join(format!(".head.json.part-{}", uuid('c'))), b"{"),
    ];
    let kept = [
        // Another project's cache is another lease's.
        plant(
            &cache
                .root
                .join("p2/network_reporter/export-tx_1/default")
                .join(format!(".xlsx.part-{}", uuid('d'))),
            b"other project",
        ),
        // Not the name `place_verified` writes.
        plant(&report.join("financial-draft-fr"), b"a placed output"),
        plant(&report.join(".notes.part-short"), b"not a temporary"),
        plant(&report.join(format!("xlsx.part-{}", uuid('e'))), b"no dot"),
    ];

    let gateway = FakeGateway {
        world: cache.world.clone(),
    };
    let producer = FakeProducer::new(cache.world.clone(), Vec::new());
    let online = || true;
    let reads = InFlightReads {
        world: cache.world.clone(),
        root: cache.root.clone(),
        pass_started: SystemTime::now(),
        in_flight: Mutex::new(Vec::new()),
    };

    // A worker that does not hold the lease removes nothing.
    let bystander = cache.host("install-1#200", &gateway, &producer, &reads, &online);
    assert!(SyncHost::sweep_temporaries(&bystander, PROJECT)
        .unwrap_err()
        .contains("lease holder"));
    assert!(orphans.iter().all(|path| path.exists()));

    let holder = cache.host("install-1#100", &gateway, &producer, &reads, &online);
    let pass = run(&holder, PROJECT, sync::Trigger::Startup).expect("the pass runs");

    for orphan in &orphans {
        assert!(!orphan.exists(), "{} was swept", orphan.display());
    }
    for path in &kept {
        assert!(path.exists(), "{} is kept", path.display());
    }
    let swept = pass
        .receipts
        .iter()
        .find(|receipt| receipt.action == "sweep")
        .expect("the sweep is evidence");
    assert_eq!(swept.outcome, "removed");
    assert_eq!(swept.bytes, Some(4 + 15 + 1));
    let kept_receipt = cache
        .store
        .lock()
        .unwrap()
        .receipts(
            &fence(),
            &Scope::Project {
                project: PROJECT.into(),
            },
            20,
        )
        .unwrap()
        .into_iter()
        .any(|receipt| receipt.action == "sweep" && receipt.outcome == "removed");
    assert!(kept_receipt, "`ds core activity` lists the sweep");

    // The step's own temporary was open during the mid-object sweep, and
    // the head it became was placed from it.
    assert_eq!(reads.in_flight.lock().unwrap().len(), 1);
    assert!(pass
        .receipts
        .iter()
        .any(|receipt| receipt.action == "download" && receipt.outcome == "downloaded"));
    assert_eq!(std::fs::read(head.join("xlsx")).unwrap(), HEAD_BYTES);
    assert!(
        temporaries_in(&head).is_empty(),
        "nothing is left half written"
    );
}

#[test]
fn control_a_clean_cache_records_no_sweep() {
    let cache = Cache::new();
    let gateway = FakeGateway {
        world: cache.world.clone(),
    };
    let producer = FakeProducer::new(cache.world.clone(), Vec::new());
    let online = || true;
    let reads = InFlightReads {
        world: cache.world.clone(),
        root: cache.root.clone(),
        pass_started: SystemTime::now(),
        in_flight: Mutex::new(Vec::new()),
    };
    let holder = cache.host("install-1#100", &gateway, &producer, &reads, &online);
    let pass = run(&holder, PROJECT, sync::Trigger::Startup).expect("the pass runs");
    assert!(!pass
        .receipts
        .iter()
        .any(|receipt| receipt.action == "sweep"));
}
