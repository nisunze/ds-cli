//! A held row names the typed cause of the failure that holds it, and says
//! which retry comes next. ADERM 10-23/24: `ds core sync status` showed the
//! row `held` with `failure_code: null` while the gateway timed out or
//! answered 503, and the pump said only "could not read or publish".
//!
//! Driven through the real `StoreHost` over a real store on disk; the status
//! is the store's own `ds.sync-recovery/v1` read that `ds core sync status`
//! answers with.

#[path = "../src/test_directory.rs"]
mod test_directory;

use ds_sync_runtime::fakes::{self, local_row, FakeGateway, FakeProducer, FakeReads};
use ds_sync_runtime::store::recovery::{Command, Decision};
use ds_sync_runtime::store::{Fence, Scope};
use ds_sync_runtime::{
    kernel_sync as sync, open_store, run, Gateway, GatewayError, LocalRow, SharedStore, StoreHost,
    SyncRoute, SyncRun,
};
use serde_json::Value;

const PROJECT: &str = "p1";
const DEADLINE: &str =
    "the DS service took the request and did not answer within the call's deadline";
const SERVICE_503: &str = "the DS gateway answered HTTP 503: the service is failing or rate-limited, not rejecting this credential";

fn fence() -> Fence {
    Fence {
        account: "account-1".into(),
        deployment: "https://gateway.example.com".into(),
        install_id: "install-1".into(),
    }
}

fn scope() -> Scope {
    Scope::Project {
        project: PROJECT.into(),
    }
}

/// The record's door with one planted transport failure: every call whose
/// action is `failing` answers it, as the native transport classified it.
struct PlantedDoor {
    record: FakeGateway,
    failing: Option<(&'static str, GatewayError)>,
}

impl Gateway for PlantedDoor {
    fn post(&self, route: SyncRoute, body: &Value) -> Result<Value, GatewayError> {
        if let Some((action, error)) = &self.failing {
            if body["action"] == *action {
                return Err(error.clone());
            }
        }
        self.record.post(route, body)
    }
}

struct Harness {
    dir: tempfile::TempDir,
    store: SharedStore,
    world: fakes::SharedWorld,
    row: LocalRow,
}

impl Harness {
    fn new() -> Self {
        let dir = test_directory::tempdir().expect("temp dir");
        let store = open_store(&dir.path().join("store.sqlite")).expect("open store");
        let row = local_row(b"report bytes");
        store
            .lock()
            .unwrap()
            .apply(
                &fence(),
                ds_sync_runtime::now_ms(),
                ds_sync_runtime::store::Event::LocalProduced {
                    row: ds_sync_runtime::rows::store_row(PROJECT, &row, 1_000),
                },
            )
            .expect("the seal records its row");
        Harness {
            dir,
            store,
            world: fakes::world(),
            row,
        }
    }

    fn pass(&self, failing: Option<(&'static str, GatewayError)>) -> Result<SyncRun, String> {
        let door = PlantedDoor {
            record: FakeGateway {
                world: self.world.clone(),
            },
            failing,
        };
        let producer = FakeProducer::new(self.world.clone(), vec![self.row.clone()]);
        let reads = FakeReads {
            world: self.world.clone(),
            root: self.dir.path().join("sync-downloads"),
        };
        let online = || true;
        let host = StoreHost::new(
            self.store.clone(),
            fence(),
            "install-1#100".into(),
            PROJECT,
            &door,
            &producer,
            &reads,
            &online,
        )
        .expect("host");
        run(&host, PROJECT, sync::Trigger::Startup)
    }

    /// The row as `ds core sync status` answers it, serialized as it prints.
    fn status(&self) -> Value {
        let result = self
            .store
            .lock()
            .unwrap()
            .recovery(
                &fence(),
                &scope(),
                ds_sync_runtime::now_ms(),
                Command::Status { limit: 50 },
            )
            .expect("status reads");
        let row: &Decision = result
            .rows
            .iter()
            .find(|row| row.id == self.row.client_publish_id)
            .expect("the sealed row is listed");
        serde_json::to_value(row).unwrap()
    }
}

#[test]
fn an_upload_past_its_deadline_holds_the_row_with_deadline_exceeded_and_retries_next_pass() {
    let h = Harness::new();
    let pass = h
        .pass(Some((
            "open",
            GatewayError::unanswered("deadline_exceeded", DEADLINE),
        )))
        .expect("a held upload is a pass, not a failure");
    let receipt = pass
        .receipts
        .iter()
        .find(|receipt| receipt.action == "upload")
        .expect("the upload receipt");
    assert_eq!(receipt.outcome, "failed");
    assert_eq!(receipt.failure_code.as_deref(), Some("deadline_exceeded"));

    let status = h.status();
    assert_eq!(status["state"], "held");
    assert_eq!(status["failure_code"], "deadline_exceeded");
    assert_eq!(status["failure_detail"], DEADLINE);
    assert_eq!(status["next_retry"]["when"], "next_pass");
    assert!(status["next_retry"]["detail"]
        .as_str()
        .unwrap()
        .starts_with("retries next pass"));
}

#[test]
fn a_pass_the_gateway_answers_503_keeps_service_status_on_the_held_row() {
    let h = Harness::new();
    let failed = h.pass(Some((
        "list",
        GatewayError::unanswered("service_status", SERVICE_503),
    )));
    assert!(
        failed.is_err(),
        "a heads read that failed ends the pass before any step"
    );
    // `ds core activity` lists the kept receipt with its typed cause.
    let newest = h
        .store
        .lock()
        .unwrap()
        .receipts(&fence(), &scope(), 1)
        .unwrap()
        .remove(0);
    assert_eq!(
        (newest.action.as_str(), newest.outcome.as_str()),
        ("pass", "failed")
    );
    assert_eq!(newest.failure_code.as_deref(), Some("service_status"));

    let status = h.status();
    assert_eq!(status["state"], "held");
    assert_eq!(status["failure_code"], "service_status");
    assert_eq!(
        status["failure_detail"],
        format!("the project heads could not be read: {SERVICE_503}")
    );
    assert_eq!(status["next_retry"]["when"], "next_pass");
    assert_eq!(fakes::lock(&h.world).opens, 0, "no upload was attempted");
}

#[test]
fn control_a_pass_that_publishes_shows_no_failure_even_after_a_held_one() {
    let clean = Harness::new();
    clean.pass(None).expect("the pass runs");
    let status = clean.status();
    assert_eq!(status["state"], "published");
    assert_eq!(status["failure_code"], Value::Null);
    assert_eq!(status["failure_detail"], Value::Null);
    assert_eq!(status["next_retry"]["when"], "none");

    // The next pass after a planted failure clears what the row shows.
    let healed = Harness::new();
    healed
        .pass(Some((
            "open",
            GatewayError::unanswered("deadline_exceeded", DEADLINE),
        )))
        .expect("held");
    assert_eq!(healed.status()["failure_code"], "deadline_exceeded");
    healed.pass(None).expect("the retry publishes");
    let status = healed.status();
    assert_eq!(status["state"], "published");
    assert_eq!(status["failure_code"], Value::Null);
    assert_eq!(status["failure_detail"], Value::Null);
}
