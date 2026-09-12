//! The compute job table carries the execution context without a schema
//! change, and still reads a row written before the context existed.
//!
//! `compute_jobs.row` is one JSON blob, so the context added to
//! `compute_jobs::Job` is a compatible addition — but "compatible" is a claim
//! about a real SQLite file, not about a serde attribute. These tests write
//! and read actual rows.

use ds_command_kernel::compute_jobs::{EngineKind, Event, Job, Phase};
use ds_command_kernel::execution_context::{ExecutionContext, SOLAR_PROCESSING};
use ds_sync_store::Store;

const ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const INPUT: &str = "50d858e0985ecc7f60418aaf0cc5ab587f42c2570a884095a9e8ccacd0f6545c";
const RESULT: &str = "3333333333333333333333333333333333333333333333333333333333333333";

fn store(directory: &std::path::Path) -> Store {
    let store = Store::open(&directory.join("store.sqlite")).expect("open");
    store.initialize_jobs().expect("jobs table");
    store
}
fn context() -> ExecutionContext {
    ExecutionContext {
        principal_uid: "uid-a".into(),
        lane: "canary".into(),
        deployment: "https://gateway.example".into(),
        install_id: "install-1".into(),
        project: "project-a".into(),
        client: "server-connection:9f2c".into(),
        operation: SOLAR_PROCESSING.into(),
        job_id: ID.into(),
        idempotency_key: "solar-chad-1".into(),
        input_sha256: INPUT.into(),
        admitted_at_ms: 1_700_000_000_000,
    }
}
fn job(context: Option<ExecutionContext>) -> Job {
    Job {
        id: ID.into(),
        owner: "owner-digest".into(),
        lane: "canary".into(),
        input_sha256: INPUT.into(),
        engine: EngineKind::SolarPrepared,
        input_tag: "ds.solar.calculate.prepared/v1".into(),
        phase: Phase::Queued,
        attempts: 0,
        created_at_ms: 1_700_000_000_000,
        updated_at_ms: 1_700_000_000_000,
        worker: None,
        lease_until_ms: 0,
        result_sha256: None,
        error: None,
        context,
    }
}

#[test]
fn a_context_survives_submission_restart_claim_and_completion() {
    let directory = tempfile::tempdir().expect("temp");
    let submitted = store(directory.path())
        .submit_job(&job(Some(context())), b"input")
        .expect("submit");
    assert_eq!(submitted.context, Some(context()));
    // Reopened from disk: the row is read back, not remembered.
    let reopened = store(directory.path())
        .job("owner-digest", "canary", ID)
        .expect("read")
        .expect("present");
    assert_eq!(reopened.context, Some(context()));
    assert_eq!(
        store(directory.path())
            .jobs("owner-digest", "canary", 10)
            .expect("list")[0]
            .context,
        Some(context())
    );
    let (claimed, _) = store(directory.path())
        .claim_job(
            "owner-digest",
            "canary",
            "worker-1",
            1_700_000_001_000,
            30_000,
        )
        .expect("claim")
        .expect("claimable");
    assert_eq!(claimed.context, Some(context()), "a claim never re-scopes");
    let completed = store(directory.path())
        .update_job(
            "owner-digest",
            "canary",
            ID,
            Event::Complete {
                worker: "worker-1",
                result_sha256: RESULT,
            },
            1_700_000_002_000,
            Some(b"result"),
        )
        .expect("complete");
    assert_eq!(
        completed.context,
        Some(context()),
        "a completion never re-scopes"
    );
    assert_eq!(completed.phase, Phase::Completed);
}

#[test]
fn a_row_written_before_the_context_existed_still_claims_and_completes() {
    let directory = tempfile::tempdir().expect("temp");
    // Exactly what a released Server wrote: the blob has no `context` member.
    let legacy = format!(
        r#"{{"id":"{ID}","owner":"owner-digest","lane":"canary","input_sha256":"{INPUT}","engine":"solar_prepared","input_tag":"ds.solar.calculate.prepared/v1","phase":"queued","attempts":0,"created_at_ms":1700000000000,"updated_at_ms":1700000000000,"worker":null,"lease_until_ms":0,"result_sha256":null,"error":null}}"#
    );
    {
        // Written through a plain connection, exactly as the released binary's
        // INSERT did — not through today's `submit_job`.
        let store = store(directory.path());
        drop(store);
        let connection =
            rusqlite::Connection::open(directory.path().join("store.sqlite")).expect("open");
        connection
            .execute(
                "INSERT INTO compute_jobs(id,owner,lane,phase,lease_until,created,row,input) VALUES (?1,'owner-digest','canary','queued',0,1700000000000,?2,?3)",
                rusqlite::params![ID, legacy, b"input".as_slice()],
            )
            .expect("insert a released row");
    }
    let reopened = store(directory.path())
        .job("owner-digest", "canary", ID)
        .expect("read")
        .expect("present");
    assert_eq!(reopened, job(None), "a released row still deserialises");
    assert_eq!(reopened.context, None);
    // It is still workable: recovery of the scope is the host's next step, and
    // nothing about the queue is stranded by the added member.
    let (claimed, input) = store(directory.path())
        .claim_job(
            "owner-digest",
            "canary",
            "worker-1",
            1_700_000_001_000,
            30_000,
        )
        .expect("claim")
        .expect("claimable");
    assert_eq!(input, b"input");
    assert_eq!(claimed.context, None);
    assert_eq!(claimed.phase, Phase::Running);
    // A re-serialised context-less row is byte-identical to the released one,
    // so reopening a released queue does not rewrite every row on disk.
    let mut requeued = claimed.clone();
    requeued.phase = Phase::Queued;
    requeued.attempts = 0;
    requeued.updated_at_ms = 1_700_000_000_000;
    requeued.worker = None;
    requeued.lease_until_ms = 0;
    assert_eq!(serde_json::to_string(&requeued).expect("encode"), legacy);
}

#[test]
fn a_context_carrying_row_is_still_fenced_by_owner_and_lane() {
    let directory = tempfile::tempdir().expect("temp");
    store(directory.path())
        .submit_job(&job(Some(context())), b"input")
        .expect("submit");
    let store = store(directory.path());
    assert!(store
        .job("another-owner", "canary", ID)
        .expect("read")
        .is_none());
    assert!(store
        .job("owner-digest", "stable", ID)
        .expect("read")
        .is_none());
    assert!(store
        .jobs("another-owner", "canary", 10)
        .expect("list")
        .is_empty());
}
