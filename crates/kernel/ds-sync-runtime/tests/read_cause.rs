//! A failed storage object read names its typed cause on the `download`
//! receipt, as a failed gateway call does (`failure_cause.rs`). A download of
//! a stored object that times out, gets a 503 or fails its digest used to show
//! prose with `failure_code: null`; the Sync Center store read
//! (`ds.sync-store-read/v1`, what `ds core sync` and `ds core activity` print)
//! now carries the code and the bounded sentence, and the retry is unchanged:
//! the next pass reads the object again.
//!
//! Driven through the real `StoreHost` over a real store on disk. The planted
//! network failures are real `ureq` errors through the production classifier
//! (`read_cause::request_failure`); a digest or size mismatch is the real
//! `place_verified` rejecting the bytes.

#[path = "../src/test_directory.rs"]
mod test_directory;

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use ds_sync_runtime::fakes::{self, download_url, FakeGateway, FakeProducer, FakeReads};
use ds_sync_runtime::store::Fence;
use ds_sync_runtime::{
    kernel_sync as sync, open_store, read_cause, run, store_read_projection, GatewayError, Reads,
    SharedStore, StoreHost, SyncRun,
};
use serde_json::{json, Value};
use sha2::Digest;

const PROJECT: &str = "p1";
const OPERATION: &str = "export-a";
const SHARED: &[u8] = b"shared bytes of a";

fn fence() -> Fence {
    Fence {
        account: "account-1".into(),
        deployment: "https://gateway.example.com".into(),
        install_id: "install-1".into(),
    }
}

/// The storage host as the pass meets it: the fake objects through the real
/// verification, except that `plant` fails the request first, as the native
/// transport would have.
struct PlantedReads {
    inner: FakeReads,
    plant: Mutex<Option<fn() -> ureq::Error>>,
}

impl Reads for PlantedReads {
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
        static NEVER: AtomicBool = AtomicBool::new(false);
        self.download_verified_cause_until(url, destination, sha256, size_bytes, &NEVER)
            .map_err(String::from)
    }

    fn download_verified_cause_until(
        &self,
        url: &str,
        destination: &Path,
        sha256: &str,
        size_bytes: u64,
        stop: &AtomicBool,
    ) -> Result<(), GatewayError> {
        if let Some(plant) = *self.plant.lock().unwrap() {
            return Err(read_cause::request_failure(&plant()));
        }
        self.inner
            .download_verified_cause_until(url, destination, sha256, size_bytes, stop)
    }
}

struct Mirror {
    dir: tempfile::TempDir,
    store: SharedStore,
    world: fakes::SharedWorld,
}

impl Mirror {
    /// A record holding one shared head, `bytes` stored behind its ticket.
    /// The head declares `SHARED`: stored bytes that differ are a corrupt
    /// object.
    fn new(stored: &[u8]) -> Self {
        let dir = test_directory::tempdir().expect("temp dir");
        let store = open_store(&dir.path().join("store.sqlite")).expect("open store");
        let world = fakes::world();
        {
            let mut world = fakes::lock(&world);
            world
                .objects
                .insert(download_url(OPERATION, "xlsx"), stored.to_vec());
            world.record.push(json!({
                "engine": "network_reporter",
                "operation": OPERATION,
                "variant": "default",
                "head_revision": 1,
                "input_base_fingerprint": fakes::FINGERPRINT,
                "updated_at": "2026-10-23T18:00:00Z",
                "outputs": [{
                    "output_id": "xlsx",
                    "sha256": format!("{:x}", sha2::Sha256::digest(SHARED)),
                    "size_bytes": SHARED.len(),
                }],
            }));
        }
        Mirror { dir, store, world }
    }

    fn pass(&self, plant: Option<fn() -> ureq::Error>) -> SyncRun {
        let gateway = FakeGateway {
            world: self.world.clone(),
        };
        let producer = FakeProducer::new(self.world.clone(), Vec::new());
        let reads = PlantedReads {
            inner: FakeReads {
                world: self.world.clone(),
                root: self.dir.path().join("sync-downloads"),
            },
            plant: Mutex::new(plant),
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
        run(&host, PROJECT, sync::Trigger::Startup)
            .expect("a failed read is a receipt, not a failed pass")
    }

    /// The newest `download` receipt as the Sync Center store read prints it.
    fn download_receipt(&self) -> Value {
        let store = self.store.lock().unwrap();
        let read = store_read_projection(
            Some(&store),
            &fence(),
            PROJECT,
            Vec::new(),
            ds_sync_runtime::now_ms(),
        )
        .expect("the store read");
        read["receipts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|receipt| receipt["action"] == "download")
            .cloned()
            .expect("a download receipt is kept")
    }

    fn head_placed(&self) -> bool {
        self.dir
            .path()
            .join("sync-downloads")
            .join(PROJECT)
            .join("network_reporter")
            .join(OPERATION)
            .join("default")
            .join("head.json")
            .exists()
    }
}

fn timed_out() -> ureq::Error {
    ureq::Error::Timeout(ureq::Timeout::Global)
}

fn unavailable() -> ureq::Error {
    ureq::Error::StatusCode(503)
}

fn refused() -> ureq::Error {
    ureq::Error::ConnectionFailed
}

fn gone() -> ureq::Error {
    ureq::Error::StatusCode(404)
}

fn assert_failed(receipt: &Value, code: &str, detail: &str) {
    assert_eq!(receipt["outcome"], "failed");
    assert_eq!(receipt["failure_code"], code);
    let text = receipt["detail"].as_str().expect("the sentence is kept");
    assert!(text.contains(detail), "{text:?} should name {detail:?}");
}

#[test]
fn an_object_read_past_its_deadline_shows_deadline_exceeded() {
    let mirror = Mirror::new(SHARED);
    let pass = mirror.pass(Some(timed_out));
    let receipt = mirror.download_receipt();
    assert_failed(
        &receipt,
        "deadline_exceeded",
        "did not answer within the call's deadline",
    );
    assert!(pass
        .receipts
        .iter()
        .any(|r| r.failure_code.as_deref() == Some("deadline_exceeded")));
    assert!(!mirror.head_placed(), "an unread head is not readable");
}

#[test]
fn an_object_read_the_storage_host_answers_503_shows_service_status() {
    let mirror = Mirror::new(SHARED);
    mirror.pass(Some(unavailable));
    assert_failed(&mirror.download_receipt(), "service_status", "HTTP 503");
    assert!(!mirror.head_placed());
}

#[test]
fn a_refused_connection_shows_connection_refused() {
    let mirror = Mirror::new(SHARED);
    mirror.pass(Some(refused));
    assert_failed(
        &mirror.download_receipt(),
        "connection_refused",
        "storage host",
    );
}

#[test]
fn bytes_whose_digest_differs_show_digest_mismatch_and_are_not_placed() {
    // Same length, different bytes: the object in storage is corrupt.
    let mirror = Mirror::new(b"shared bytes of Z");
    mirror.pass(None);
    assert_failed(
        &mirror.download_receipt(),
        "digest_mismatch",
        "do not match the digest",
    );
    assert!(!mirror.head_placed());
    let root = mirror.dir.path().join("sync-downloads");
    let placed = walk(&root);
    assert!(
        placed.is_empty(),
        "nothing unverified stays on disk: {placed:?}"
    );
}

#[test]
fn a_shorter_object_shows_size_mismatch() {
    let mirror = Mirror::new(b"shared");
    mirror.pass(None);
    assert_failed(
        &mirror.download_receipt(),
        "size_mismatch",
        "ended before the size",
    );
}

#[test]
fn a_verdict_that_is_not_the_networks_carries_no_code() {
    // An expired ticket or a missing object is the storage host's answer, not
    // a transport cause: the sentence stays, the code stays null.
    let mirror = Mirror::new(SHARED);
    mirror.pass(Some(gone));
    let receipt = mirror.download_receipt();
    assert_eq!(receipt["outcome"], "failed");
    assert_eq!(receipt["failure_code"], Value::Null);
    assert_eq!(receipt["detail"], "the storage host answered 404");
}

#[test]
fn control_a_good_read_shows_no_failure_and_a_failed_one_is_read_again_next_pass() {
    let clean = Mirror::new(SHARED);
    let pass = clean.pass(None);
    let receipt = clean.download_receipt();
    assert_eq!(receipt["outcome"], "committed");
    assert_eq!(receipt["failure_code"], Value::Null);
    assert!(pass.receipts.iter().all(|r| r.failure_code.is_none()));
    assert!(clean.head_placed());

    // Retry is unchanged: a failed read keeps no state, the next pass reads
    // the object again and heals the head.
    let healed = Mirror::new(SHARED);
    healed.pass(Some(unavailable));
    assert_eq!(healed.download_receipt()["failure_code"], "service_status");
    healed.pass(None);
    let receipt = healed.download_receipt();
    assert_eq!(receipt["outcome"], "committed");
    assert_eq!(receipt["failure_code"], Value::Null);
    assert!(healed.head_placed());
    assert_eq!(
        fakes::lock(&healed.world).downloads.len(),
        2,
        "one ticket per pass: the failed read is simply read again"
    );
}

#[test]
fn classification_uses_the_gateway_transports_tokens() {
    use std::io::{Error, ErrorKind};
    let code = |error: ureq::Error| read_cause::request_failure(&error).code;
    assert_eq!(
        code(ureq::Error::Timeout(ureq::Timeout::Connect)).as_deref(),
        Some("connect_timed_out")
    );
    assert_eq!(
        code(ureq::Error::HostNotFound).as_deref(),
        Some("name_unresolved")
    );
    assert_eq!(
        code(ureq::Error::Io(Error::from(ErrorKind::ConnectionReset))).as_deref(),
        Some("connection_dropped")
    );
    assert_eq!(
        code(ureq::Error::StatusCode(429)).as_deref(),
        Some("service_status")
    );
    assert_eq!(code(ureq::Error::StatusCode(403)), None);
    // A body that dies mid-object.
    assert_eq!(
        read_cause::body_failure(ErrorKind::TimedOut)
            .code
            .as_deref(),
        Some("deadline_exceeded")
    );
    assert_eq!(
        read_cause::body_failure(ErrorKind::UnexpectedEof)
            .code
            .as_deref(),
        Some("connection_dropped")
    );
    assert_eq!(read_cause::body_failure(ErrorKind::Other).code, None);
    // The network's failures stay "unanswered": the loop's retry is unchanged.
    assert_eq!(
        read_cause::request_failure(&ureq::Error::StatusCode(503)).status,
        None
    );
}

fn walk(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(&directory)
            .into_iter()
            .flatten()
            .flatten()
        {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else {
                files.push(path);
            }
        }
    }
    files
}
