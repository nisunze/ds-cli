//! The compute job table carries the execution context without a schema
//! change, answers only what a caller may see, bounds and shares the pool
//! from inside its own transactions, and still reads a row written before the
//! context existed.
//!
//! `compute_jobs.row` is one JSON blob, so the context added to
//! `compute_jobs::Job` is a compatible addition — but "compatible" is a claim
//! about a real SQLite file, not about a serde attribute. These tests write
//! and read actual rows.

#[path = "../src/test_directory.rs"]
mod test_directory;
use ds_command_kernel::compute_jobs::{
    EngineKind, EngineLane, EnginePin, EnginePinSource, Event, Job, Phase,
};
use ds_command_kernel::execution_context::{
    ExecutionContext, Limits, Principal, Slot, SOLAR_PROCESSING, TRANSFORMER_PROCESSING,
};
use ds_sync_store::{Census, Error, JobCaller, Store};

const ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const ID_B: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const INPUT: &str = "50d858e0985ecc7f60418aaf0cc5ab587f42c2570a884095a9e8ccacd0f6545c";
const INPUT_B: &str = "5db1fee4b5703808c48078a76768b155b421b210c0761cd6a5d223f4d99f1eaa";
const RESULT: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const DEPLOYMENT: &str = "https://gateway.example";

fn store(directory: &std::path::Path) -> Store {
    let store = Store::open(&directory.join("store.sqlite")).expect("open");
    store.initialize_jobs().expect("jobs table");
    store
}
fn caller<'a>() -> JobCaller<'a> {
    JobCaller::connection("owner-digest", "uid-a", "canary", DEPLOYMENT)
}
fn principal() -> Principal {
    Principal {
        uid: "uid-a".into(),
        lane: "canary".into(),
        deployment: DEPLOYMENT.into(),
        install_id: "install-1".into(),
    }
}
/// Room for everything these tests submit; the bounds are proven with
/// tighter ones where they are the subject.
fn roomy() -> Limits {
    Limits {
        global_running: 8,
        per_project_running: 4,
        per_project_queued: 16,
        global_queued: 64,
    }
}
fn context() -> ExecutionContext {
    ExecutionContext {
        principal_uid: "uid-a".into(),
        lane: "canary".into(),
        deployment: DEPLOYMENT.into(),
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
    let mut job = Job {
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
        engine_pin: None,
        context,
    };
    job.engine_pin = job.context.as_ref().map(|_| pin(&job));
    job
}
fn pin(job: &Job) -> EnginePin {
    EnginePin {
        schema: "ds.compute.engine-pin/v1".into(),
        engine: job.engine.clone(),
        owner: job.owner.clone(),
        lane: EngineLane::parse(&job.lane).unwrap(),
        context: job.context.clone().unwrap(),
        source: EnginePinSource::NativeLaneCapture,
        executable: "/native-owned/immutable-test-engine".into(),
        executable_sha256: "a".repeat(64),
        identity: serde_json::json!({"storage_test": true}),
        workers: 1,
        development_override: true,
    }
}

#[test]
fn admission_captures_once_inside_the_insert_and_never_for_duplicate_or_full_queue() {
    use std::cell::Cell;
    let directory = test_directory::tempdir().unwrap();
    let mut store = store(directory.path());
    let calls = Cell::new(0);
    let first = job(Some(context()));
    let accepted = store
        .submit_job_capturing_pin(&first, b"input", roomy(), || {
            calls.set(calls.get() + 1);
            Ok(Some(pin(&first)))
        })
        .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(accepted.engine_pin, first.engine_pin);
    let duplicate = store
        .submit_job_capturing_pin(&first, b"input", roomy(), || {
            panic!("duplicate probes no addon")
        })
        .unwrap();
    assert_eq!(duplicate, accepted);
    let tight = Limits {
        global_running: 1,
        per_project_running: 1,
        global_queued: 1,
        per_project_queued: 1,
    };
    let refusal = store
        .submit_job_capturing_pin(&nth("project-a", 2), b"second", tight, || {
            panic!("capacity refusal probes no addon")
        })
        .unwrap_err();
    assert!(refusal.to_string().contains("capacity_exhausted"));
    let other = nth("project-b", 3);
    assert!(store
        .submit_job_capturing_pin(&other, b"other", roomy(), || Err(Error::Kernel(
            "capture-refused".into()
        )))
        .is_err());
    assert!(
        store.job(&caller(), &other.id).unwrap().is_none(),
        "failed capture commits no row"
    );
}

#[test]
fn historical_and_misbound_pins_are_parked_without_blocking_valid_work_or_cancellation() {
    let directory = test_directory::tempdir().unwrap();
    drop(store(directory.path()));
    let mut historical = nth("project-a", 1);
    historical.engine_pin = None;
    insert_row(directory.path(), &historical, b"old");
    let mut corrupt = nth("project-b", 2);
    corrupt.engine_pin.as_mut().unwrap().context.project = "wrong-project".into();
    insert_row(directory.path(), &corrupt, b"bad");
    let valid = nth("project-c", 3);
    let mut store = store(directory.path());
    store.submit_job(&valid, b"valid", roomy()).unwrap();
    let census = store
        .census("owner-digest", "canary", 1_700_000_001_000)
        .unwrap();
    assert_eq!(census.queued.len(), 1);
    let claimed = store
        .claim_job(&caller(), "worker", 1_700_000_001_000, 30_000, roomy())
        .unwrap()
        .unwrap()
        .0;
    assert_eq!(claimed.id, valid.id);
    assert_eq!(
        store.job_input(&caller(), &historical.id).unwrap().unwrap(),
        b"old"
    );
    let cancelled = store
        .update_job(
            &caller(),
            &historical.id,
            Event::Cancel,
            1_700_000_002_000,
            None,
        )
        .unwrap();
    assert_eq!(cancelled.phase, Phase::Cancelled);
    assert!(store.job(&caller(), &corrupt.id).unwrap().is_some());
}
/// The second project's job: another id, another sealed digest, everything
/// else the same connection.
fn job_b() -> Job {
    let mut job = job(Some(ExecutionContext {
        project: "project-b".into(),
        job_id: ID_B.into(),
        idempotency_key: "solar-lome-1".into(),
        input_sha256: INPUT_B.into(),
        ..context()
    }));
    job.id = ID_B.into();
    job.input_sha256 = INPUT_B.into();
    job.created_at_ms = 1_700_000_000_001;
    job.updated_at_ms = 1_700_000_000_001;
    job.engine_pin = Some(pin(&job));
    job
}
/// The n-th job of `project`, created in order of `n`, with its own id and
/// digest so that many can share one queue.
fn nth(project: &str, n: u64) -> Job {
    // Ids and digests must be lowercase hex: the project's last letter (a, b,
    // c) leads the id and trails the digest, so they never collide.
    let tag = project.chars().last().expect("a project name");
    let id = format!("{tag}{n:0>63x}");
    let digest = format!("{:0>63x}{tag}", n + 7);
    let mut job = job(Some(ExecutionContext {
        project: project.into(),
        job_id: id.clone(),
        idempotency_key: format!("{project}-{n}"),
        input_sha256: digest.clone(),
        ..context()
    }));
    job.id = id;
    job.input_sha256 = digest;
    job.created_at_ms = 1_700_000_000_000 + n;
    job.updated_at_ms = job.created_at_ms;
    job.engine_pin = Some(pin(&job));
    job
}
fn claim(store: &mut Store, worker: &str, now: u64, limits: Limits) -> Option<String> {
    store
        .claim_job(&caller(), worker, now, 30_000, limits)
        .expect("claim")
        .map(|(job, _)| job.context.expect("claimed rows carry a context").project)
}
/// A row exactly as a released Server wrote it: the blob has no `context`
/// member, inserted through a plain connection, not through `submit_job`.
fn insert_legacy(directory: &std::path::Path, id: &str, engine: &str, tag: &str, input: &[u8]) {
    let row = format!(
        r#"{{"id":"{id}","owner":"owner-digest","lane":"canary","input_sha256":"{INPUT}","engine":"{engine}","input_tag":"{tag}","phase":"queued","attempts":0,"created_at_ms":1700000000000,"updated_at_ms":1700000000000,"worker":null,"lease_until_ms":0,"result_sha256":null,"error":null}}"#
    );
    let connection = rusqlite::Connection::open(directory.join("store.sqlite")).expect("open");
    connection
        .execute(
            "INSERT INTO compute_jobs(id,owner,lane,phase,lease_until,created,row,input) VALUES (?1,'owner-digest','canary','queued',0,1700000000000,?2,?3)",
            rusqlite::params![id, row, input],
        )
        .expect("insert a released row");
}
/// A row this build would never write, put there the only way it can be: as
/// bytes, through a plain connection.
fn insert_row(directory: &std::path::Path, job: &Job, input: &[u8]) {
    let connection = rusqlite::Connection::open(directory.join("store.sqlite")).expect("open");
    connection
        .execute(
            "INSERT INTO compute_jobs(id,owner,lane,phase,lease_until,created,row,input) VALUES (?1,?2,?3,'queued',0,?4,?5,?6)",
            rusqlite::params![
                job.id,
                job.owner,
                job.lane,
                job.created_at_ms,
                serde_json::to_string(job).expect("row encodes"),
                input
            ],
        )
        .expect("insert a row this build would refuse");
}

#[test]
fn a_context_survives_submission_restart_claim_and_completion() {
    let directory = test_directory::tempdir().expect("temp");
    let submitted = store(directory.path())
        .submit_job(&job(Some(context())), b"input", roomy())
        .expect("submit");
    assert_eq!(submitted.context, Some(context()));
    // Reopened from disk: the row is read back, not remembered.
    let reopened = store(directory.path())
        .job(&caller(), ID)
        .expect("read")
        .expect("present");
    assert_eq!(reopened.context, Some(context()));
    assert_eq!(
        store(directory.path()).jobs(&caller(), 10).expect("list")[0].context,
        Some(context())
    );
    let (claimed, _) = store(directory.path())
        .claim_job(&caller(), "worker-1", 1_700_000_001_000, 30_000, roomy())
        .expect("claim")
        .expect("claimable");
    assert_eq!(claimed.context, Some(context()), "a claim never re-scopes");
    let completed = store(directory.path())
        .update_job(
            &caller(),
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
fn a_row_written_before_the_context_existed_reads_but_is_never_claimed() {
    let directory = test_directory::tempdir().expect("temp");
    drop(store(directory.path()));
    insert_legacy(
        directory.path(),
        ID,
        "solar_prepared",
        "ds.solar.calculate.prepared/v1",
        b"input",
    );
    let reopened = store(directory.path())
        .job(&caller(), ID)
        .expect("read")
        .expect("present");
    assert_eq!(reopened, job(None), "a released row still deserialises");
    assert_eq!(reopened.context, None);
    // Its own connection still sees it — an operator must be able to look at
    // work the host refuses to guess about — but nothing narrowed does.
    assert!(store(directory.path())
        .job(&caller().about(Some("project-a")), ID)
        .expect("read")
        .is_none());
    assert!(store(directory.path())
        .jobs(&caller().about(Some("project-a")), 10)
        .expect("list")
        .is_empty());
    // And it is never executed while its project is unknown.
    assert!(store(directory.path())
        .claim_job(&caller(), "worker-1", 1_700_000_001_000, 30_000, roomy())
        .expect("claim")
        .is_none());
    // A re-serialised context-less row is byte-identical to the released one,
    // so reopening a released queue does not rewrite every row on disk.
    let legacy: String = rusqlite::Connection::open(directory.path().join("store.sqlite"))
        .expect("open")
        .query_row("SELECT row FROM compute_jobs WHERE id=?1", [ID], |row| {
            row.get(0)
        })
        .expect("row");
    assert_eq!(serde_json::to_string(&reopened).expect("encode"), legacy);
}

#[test]
fn a_legacy_solar_row_recovers_from_its_sealed_input_and_a_transformer_row_never_does() {
    let directory = test_directory::tempdir().expect("temp");
    drop(store(directory.path()));
    insert_legacy(
        directory.path(),
        ID,
        "solar_prepared",
        "ds.solar.calculate.prepared/v1",
        b"sealed for project-b",
    );
    insert_legacy(
        directory.path(),
        ID_B,
        "fast_lv",
        "ds.fast-lv.request/v1",
        b"a transformer batch",
    );
    let historical_row: String = rusqlite::Connection::open(directory.path().join("store.sqlite"))
        .unwrap()
        .query_row("SELECT row FROM compute_jobs WHERE id=?1", [ID_B], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        store(directory.path())
            .job(&caller(), ID_B)
            .unwrap()
            .unwrap()
            .engine,
        EngineKind::LvProcessing,
        "historical queue rows read through the explicit engine alias"
    );
    let sealed = |job: &Job, input: &[u8]| {
        (job.engine == EngineKind::SolarPrepared && input == b"sealed for project-b")
            .then(|| "project-b".to_owned())
    };
    let recovery = store(directory.path())
        .recover_contexts(&principal(), "owner-digest", &sealed, 1_700_000_500_000)
        .expect("recover");
    assert_eq!(recovery.from_sealed_input, 1);
    assert_eq!(
        recovery.unrecoverable,
        vec![ID_B.to_owned()],
        "a transformer row names no project and nothing else may name one for it"
    );
    let after_read_and_recovery: String =
        rusqlite::Connection::open(directory.path().join("store.sqlite"))
            .unwrap()
            .query_row("SELECT row FROM compute_jobs WHERE id=?1", [ID_B], |row| {
                row.get(0)
            })
            .unwrap();
    assert_eq!(
        after_read_and_recovery, historical_row,
        "read compatibility never rewrites stored historical bytes"
    );
    let solar = store(directory.path())
        .job(&caller(), ID)
        .expect("read")
        .expect("present")
        .context
        .expect("recovered");
    assert_eq!(solar.project, "project-b", "the sealed bytes name it");
    assert_eq!(solar.operation, SOLAR_PROCESSING);
    assert_eq!(
        solar.admitted_at_ms, 1_700_000_000_000,
        "the row's own admission moment, not the moment it reopened"
    );
    assert!(
        store(directory.path())
            .job(&caller(), ID_B)
            .expect("read")
            .expect("present")
            .context
            .is_none(),
        "the transformer row is left exactly as it was written"
    );
    // Its project is recovered, but no historical executable can be invented.
    assert_eq!(
        store(directory.path())
            .jobs(&caller().about(Some("project-b")), 10)
            .expect("list")
            .len(),
        1
    );
    // A second pass rewrites nothing: the one it could recover is stored now,
    // and the one it could not is named again rather than guessed at.
    let again = store(directory.path())
        .recover_contexts(&principal(), "owner-digest", &sealed, 1_700_000_600_000)
        .expect("recover");
    assert_eq!(again.stored, 1);
    assert_eq!(again.from_sealed_input, 0);
    assert_eq!(again.unrecoverable, vec![ID_B.to_owned()]);
    assert_eq!(again.unpinned_count, 2);
    assert!(store(directory.path())
        .claim_job(&caller(), "no-latest", 1_700_000_601_000, 30_000, roomy())
        .unwrap()
        .is_none());
}

/// Reopening a queue must cost the rows that need something, not the queue.
/// A Server the owner leaves running for weeks restarts over a table of
/// finished work whose sealed Solar inputs are tens of megabytes each; reading
/// every one of them to discover it already has a context is the unbounded
/// allocation the contract forbids, paid every time for nothing.
#[test]
fn reopening_a_queue_reads_only_the_rows_that_still_need_a_context() {
    let directory = test_directory::tempdir().expect("temp");
    let mut store = store(directory.path());
    store
        .submit_job(&job(Some(context())), b"sealed for project-a", roomy())
        .expect("submit");
    let connection =
        || rusqlite::Connection::open(directory.path().join("store.sqlite")).expect("open");
    let stored_row = || -> String {
        connection()
            .query_row("SELECT row FROM compute_jobs WHERE id=?1", [ID], |row| {
                row.get(0)
            })
            .expect("stored row")
    };
    let before = stored_row();
    // The proof that the row is not merely left alone but never read: its
    // input column is made into something no `Vec<u8>` can be got out of, so
    // any recovery that selected it would fail outright.
    connection()
        .execute("UPDATE compute_jobs SET input=1 WHERE id=?1", [ID])
        .expect("unreadable bytes");
    insert_legacy(
        directory.path(),
        ID_B,
        "solar_prepared",
        "ds.solar.calculate.prepared/v1",
        b"sealed for project-b",
    );
    let read = std::cell::RefCell::new(Vec::new());
    let sealed = |job: &Job, input: &[u8]| {
        read.borrow_mut().push(job.id.clone());
        (input == b"sealed for project-b").then(|| "project-b".to_owned())
    };
    let recovery = store
        .recover_contexts(&principal(), "owner-digest", &sealed, 1_700_000_500_000)
        .expect("recover");
    assert_eq!(recovery.stored, 1);
    assert_eq!(recovery.from_sealed_input, 1);
    assert!(recovery.unrecoverable.is_empty());
    assert_eq!(
        read.into_inner(),
        vec![ID_B.to_owned()],
        "only the row without a context had its sealed bytes read"
    );
    assert_eq!(
        before,
        stored_row(),
        "a row that had a context was not rewritten"
    );
}

#[test]
fn a_transformer_row_written_before_contexts_is_named_unrecoverable_not_guessed() {
    let directory = test_directory::tempdir().expect("temp");
    drop(store(directory.path()));
    insert_legacy(
        directory.path(),
        ID_B,
        "fast_lv",
        "ds.fast-lv.request/v1",
        b"a transformer batch",
    );
    let recovery = store(directory.path())
        .recover_contexts(
            &principal(),
            "owner-digest",
            &|_, _| None,
            1_700_000_500_000,
        )
        .expect("recover");
    assert_eq!(recovery.unrecoverable, vec![ID_B.to_owned()]);
    let mut store = store(directory.path());
    assert!(store
        .job(&caller(), ID_B)
        .expect("read")
        .expect("present")
        .context
        .is_none());
    // It is not in flight: never claimed, so never running and never waiting
    // for a worker. Counting it as another project's waiting work would refuse
    // every project its share for as long as the row exists.
    assert_eq!(
        store
            .census("owner-digest", "canary", 1_700_000_500_000)
            .expect("census"),
        Census::default()
    );
    let one_slot = Limits {
        per_project_running: 1,
        ..roomy()
    };
    store
        .submit_job(&nth("project-a", 1), b"a1", one_slot)
        .expect("a1");
    store
        .submit_job(&nth("project-a", 2), b"a2", one_slot)
        .expect("a2");
    assert_eq!(
        claim(&mut store, "worker-1", 1_700_000_600_000, one_slot).as_deref(),
        Some("project-a")
    );
    // A is at its share of one, and nothing else that could run is waiting:
    // the unrecoverable row does not hold A back.
    assert_eq!(
        claim(&mut store, "worker-2", 1_700_000_600_001, one_slot).as_deref(),
        Some("project-a")
    );
}

#[test]
fn a_row_written_before_contexts_is_never_adopted_by_a_later_submission() {
    let directory = test_directory::tempdir().expect("temp");
    drop(store(directory.path()));
    insert_legacy(
        directory.path(),
        ID_B,
        "fast_lv",
        "ds.fast-lv.request/v1",
        b"a transformer batch",
    );
    let mut store = store(directory.path());
    // This build's job ids digest the project, so a released Server's id can
    // no longer be re-derived from a key: reaching this row at all takes
    // deliberate aim. Even then it is not adopted — a row admitted before
    // contexts stays exactly as it was written, readable and unrun.
    let mut named = job(Some(ExecutionContext {
        project: "project-a".into(),
        job_id: ID_B.into(),
        operation: TRANSFORMER_PROCESSING.into(),
        idempotency_key: "tx-batch-1".into(),
        ..context()
    }));
    named.id = ID_B.into();
    named.engine = EngineKind::LvProcessing;
    named.input_tag = "ds.fast-lv.request/v1".into();
    assert!(store
        .submit_job(&named, b"another batch", roomy())
        .expect_err("other bytes")
        .to_string()
        .contains("compute_job_identity_conflict"));
    let refused = store
        .submit_job(&named, b"a transformer batch", roomy())
        .expect_err("never adopted");
    assert!(
        refused.to_string().contains("context_unrecoverable"),
        "{refused}"
    );
    assert!(
        store
            .job(&caller(), ID_B)
            .expect("read")
            .expect("present")
            .context
            .is_none(),
        "the released row is left exactly as it was"
    );
    assert!(store
        .job(&caller().about(Some("project-a")), ID_B)
        .expect("read")
        .is_none());
    assert!(claim(&mut store, "worker-1", 1_700_000_600_000, roomy()).is_none());
    let again = store
        .recover_contexts(
            &principal(),
            "owner-digest",
            &|_, _| None,
            1_700_000_700_000,
        )
        .expect("recover");
    assert_eq!(again.stored, 0);
    assert_eq!(again.unrecoverable, vec![ID_B.to_owned()]);
}

#[test]
fn a_context_carrying_row_is_still_fenced_by_owner_and_lane() {
    let directory = test_directory::tempdir().expect("temp");
    store(directory.path())
        .submit_job(&job(Some(context())), b"input", roomy())
        .expect("submit");
    let store = store(directory.path());
    let other_owner = JobCaller::connection("another-owner", "uid-a", "canary", DEPLOYMENT);
    let other_lane = JobCaller::connection("owner-digest", "uid-a", "stable", DEPLOYMENT);
    assert!(store.job(&other_owner, ID).expect("read").is_none());
    assert!(store.job(&other_lane, ID).expect("read").is_none());
    assert!(store.jobs(&other_owner, 10).expect("list").is_empty());
}

#[test]
fn two_projects_overlap_on_one_connection_and_neither_can_read_the_other() {
    let directory = test_directory::tempdir().expect("temp");
    {
        let mut store = store(directory.path());
        store
            .submit_job(&job(Some(context())), b"input", roomy())
            .expect("a");
        store.submit_job(&job_b(), b"input-b", roomy()).expect("b");
        store
            .update_job(
                &caller(),
                ID,
                Event::Claim {
                    worker: "worker-1",
                    lease_ms: 30_000,
                },
                1_700_000_001_000,
                None,
            )
            .expect("claim a");
        store
            .update_job(
                &caller(),
                ID,
                Event::Complete {
                    worker: "worker-1",
                    result_sha256: RESULT,
                },
                1_700_000_002_000,
                Some(b"result"),
            )
            .expect("complete a");
    }
    let store = store(directory.path());
    let a = caller().about(Some("project-a"));
    let b = caller().about(Some("project-b"));
    assert_eq!(store.jobs(&caller(), 10).expect("all").len(), 2);
    assert_eq!(
        store
            .jobs(&a, 10)
            .expect("a")
            .iter()
            .map(|job| job.id.clone())
            .collect::<Vec<_>>(),
        vec![ID.to_owned()]
    );
    assert_eq!(
        store
            .jobs(&b, 10)
            .expect("b")
            .iter()
            .map(|job| job.id.clone())
            .collect::<Vec<_>>(),
        vec![ID_B.to_owned()]
    );
    // Status, result and the sealed input of A, requested as B: all absent,
    // and absent in exactly the way a guessed id is absent.
    assert!(store.job(&b, ID).expect("status").is_none());
    assert!(store.job_result(&b, ID).expect("result").is_none());
    assert!(store.job_input(&b, ID).expect("input").is_none());
    let guessed = "0".repeat(64);
    assert!(store.job(&b, &guessed).expect("guess").is_none());
    assert!(store.job(&a, &guessed).expect("guess").is_none());
    // A's own project still reads all three.
    assert!(store.job(&a, ID).expect("status").is_some());
    assert_eq!(
        store.job_result(&a, ID).expect("result").unwrap(),
        b"result"
    );
    assert_eq!(store.job_input(&a, ID).expect("input").unwrap(), b"input");
    // A foreign principal, lane or deployment reads nothing either.
    for foreign in [
        JobCaller::connection("owner-digest", "uid-b", "canary", DEPLOYMENT),
        JobCaller::connection("owner-digest", "uid-a", "stable", DEPLOYMENT),
        JobCaller::connection("owner-digest", "uid-a", "canary", "https://other.example"),
    ] {
        assert!(store.job(&foreign, ID).expect("status").is_none());
        assert!(store.jobs(&foreign, 10).expect("list").is_empty());
    }
}

#[test]
fn a_narrowed_page_is_not_shortened_by_another_projects_rows() {
    let directory = test_directory::tempdir().expect("temp");
    let mut store = store(directory.path());
    // A's one row is the oldest; B's three are newer.
    store
        .submit_job(&nth("project-a", 1), b"a1", roomy())
        .expect("a1");
    for n in 2..=4 {
        store
            .submit_job(&nth("project-b", n), format!("b{n}").as_bytes(), roomy())
            .expect("b");
    }
    let a = caller().about(Some("project-a"));
    let page = store.jobs(&a, 1).expect("page");
    assert_eq!(page.len(), 1, "a page of one is one of A's rows, not empty");
    assert_eq!(page[0].context.as_ref().unwrap().project, "project-a");
    // The keyset page is narrowed the same way and walks B alone.
    let b = caller().about(Some("project-b"));
    let first = store.jobs_page(&b, None, 2).expect("first");
    assert_eq!(first.len(), 2);
    let cursor = (first[1].created_at_ms, first[1].id.as_str());
    let second = store.jobs_page(&b, Some(cursor), 2).expect("second");
    assert_eq!(second.len(), 1);
    assert!(first
        .iter()
        .chain(&second)
        .all(|job| job.context.as_ref().unwrap().project == "project-b"));
    // Newest-first ties agree between the first page and the keyset page.
    assert_eq!(
        store.jobs(&caller(), 10).expect("all"),
        store.jobs_page(&caller(), None, 10).expect("all paged")
    );
}

#[test]
fn cancelling_another_projects_job_answers_exactly_what_a_missing_row_answers() {
    let directory = test_directory::tempdir().expect("temp");
    let mut store = store(directory.path());
    store
        .submit_job(&job(Some(context())), b"input", roomy())
        .expect("a");
    let b = caller().about(Some("project-b"));
    let cross = store
        .update_job(&b, ID, Event::Cancel, 1_700_000_003_000, None)
        .expect_err("cross-project cancel");
    let unknown = store
        .update_job(&b, &"0".repeat(64), Event::Cancel, 1_700_000_003_000, None)
        .expect_err("unknown cancel");
    assert_eq!(cross.to_string(), unknown.to_string());
    assert_eq!(
        store
            .job(&caller(), ID)
            .expect("read")
            .expect("present")
            .phase,
        Phase::Queued,
        "the refused cancel changed nothing"
    );
}

#[test]
fn a_resubmitted_key_may_not_move_its_project_or_operation() {
    let directory = test_directory::tempdir().expect("temp");
    let mut store = store(directory.path());
    store
        .submit_job(&job(Some(context())), b"input", roomy())
        .expect("a");
    // The same durable id (same owner, lane and key) with another project.
    let moved = job(Some(ExecutionContext {
        project: "project-b".into(),
        ..context()
    }));
    assert!(store
        .submit_job(&moved, b"input", roomy())
        .expect_err("scope moved")
        .to_string()
        .contains("compute_job_scope_conflict"));
    // Same project, same bytes: the stored row comes back untouched, and it
    // consumes no room — a queue with no space left still answers it.
    let full = Limits {
        global_queued: 1,
        ..roomy()
    };
    assert_eq!(
        store
            .submit_job(&job(Some(context())), b"input", full)
            .expect("idempotent"),
        job(Some(context()))
    );
    // Same key, other bytes: refused by the row's own identity.
    let mut other_bytes = job(Some(ExecutionContext {
        input_sha256: INPUT_B.into(),
        ..context()
    }));
    other_bytes.input_sha256 = INPUT_B.into();
    assert!(store
        .submit_job(&other_bytes, b"other", roomy())
        .expect_err("payload changed")
        .to_string()
        .contains("compute_job_identity_conflict"));
}

#[test]
fn a_row_without_a_context_is_never_written_by_this_build() {
    let directory = test_directory::tempdir().expect("temp");
    let mut store = store(directory.path());
    assert!(store
        .submit_job(&job(None), b"input", roomy())
        .expect_err("no context")
        .to_string()
        .contains("compute_job_without_execution_context"));
    // A context bound to another job id is corrupt, not a second opinion
    // about what the row is.
    let mismatched = job(Some(ExecutionContext {
        job_id: ID_B.into(),
        ..context()
    }));
    assert!(store
        .submit_job(&mismatched, b"input", roomy())
        .expect_err("mismatch")
        .to_string()
        .contains("compute_job_context_mismatch"));
}

#[test]
fn a_submission_is_bounded_inside_the_write_transaction() {
    let directory = test_directory::tempdir().expect("temp");
    let mut store = store(directory.path());
    let tight = Limits {
        per_project_queued: 2,
        global_queued: 3,
        ..roomy()
    };
    store
        .submit_job(&nth("project-a", 1), b"a1", tight)
        .expect("a1");
    store
        .submit_job(&nth("project-a", 2), b"a2", tight)
        .expect("a2");
    let project = store
        .submit_job(&nth("project-a", 3), b"a3", tight)
        .expect_err("A's queue is full");
    let Error::Refused(refusal) = &project else {
        panic!("a capacity refusal is typed, got {project}");
    };
    assert_eq!(refusal.code, "capacity_exhausted");
    assert_eq!(refusal.scope, Some("project"));
    assert!(refusal.retry_after_ms.is_some_and(|ms| ms >= 1_000));
    store
        .submit_job(&nth("project-b", 4), b"b4", tight)
        .expect("B still gets in");
    let global = store
        .submit_job(&nth("project-b", 5), b"b5", tight)
        .expect_err("the host queue is full");
    assert!(matches!(&global, Error::Refused(r) if r.scope == Some("global")));
    assert_eq!(
        store.jobs(&caller(), 10).expect("rows").len(),
        3,
        "a refused submission wrote nothing"
    );
    // A cancelled row leaves the queue and frees its place.
    store
        .update_job(
            &caller().about(Some("project-a")),
            &nth("project-a", 1).id,
            Event::Cancel,
            1_700_000_001_000,
            None,
        )
        .expect("cancel");
    store
        .submit_job(&nth("project-b", 5), b"b5", tight)
        .expect("room again");
    drop(store);
    // Six submissions racing for a queue of three: exactly three are written,
    // because the bound is read and applied in the transaction that inserts.
    // The three are one project's, so the bound under test is its own — and a
    // project's bound is smaller than the host's, which is why the global
    // queue here is four.
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
    let path = directory.path().join("race.sqlite");
    Store::open(&path)
        .expect("open")
        .initialize_jobs()
        .expect("table");
    let race = Limits {
        per_project_queued: 3,
        global_queued: 4,
        ..roomy()
    };
    let outcomes = (1..=6u64)
        .map(|n| {
            let (path, barrier) = (path.clone(), barrier.clone());
            std::thread::spawn(move || {
                let mut store = Store::open(&path).expect("open");
                barrier.wait();
                store
                    .submit_job(&nth("project-c", n), format!("c{n}").as_bytes(), race)
                    .map(|_| ())
                    .map_err(|error| error.to_string())
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|handle| handle.join().expect("thread"))
        .collect::<Vec<_>>();
    assert_eq!(outcomes.iter().filter(|o| o.is_ok()).count(), 3);
    assert!(outcomes
        .iter()
        .filter_map(|o| o.as_ref().err())
        .all(|e| e.contains("capacity_exhausted")));
    assert_eq!(
        Store::open(&path)
            .expect("open")
            .jobs(&caller(), 10)
            .expect("rows")
            .len(),
        3
    );
}

#[test]
fn the_census_names_running_and_queued_work_by_project_only() {
    let directory = test_directory::tempdir().expect("temp");
    let mut store = store(directory.path());
    store
        .submit_job(&job(Some(context())), b"input", roomy())
        .expect("a");
    store.submit_job(&job_b(), b"input-b", roomy()).expect("b");
    let now = 1_700_000_001_000;
    assert_eq!(
        store.census("owner-digest", "canary", now).expect("census"),
        Census {
            running: vec![],
            queued: vec![
                Slot {
                    project: "project-a".into()
                },
                Slot {
                    project: "project-b".into()
                }
            ],
        }
    );
    store
        .claim_job(&caller(), "worker-1", now, 30_000, roomy())
        .expect("claim")
        .expect("claimable");
    let census = store
        .census("owner-digest", "canary", now + 1)
        .expect("census");
    assert_eq!(
        census.running,
        vec![Slot {
            project: "project-a".into()
        }]
    );
    assert_eq!(
        census.queued,
        vec![Slot {
            project: "project-b".into()
        }]
    );
    // A running row whose lease has expired is waiting to be claimed again,
    // not running: a dead worker's row must not fill the pool.
    let later = store
        .census("owner-digest", "canary", now + 30_001)
        .expect("census");
    assert!(later.running.is_empty());
    assert_eq!(later.queued.len(), 2);
    // …and the pool with room for exactly one lets that row be reclaimed. A
    // single-slot pool has no share to divide, so its per-project limit is one.
    let one = Limits {
        global_running: 1,
        per_project_running: 1,
        ..roomy()
    };
    let (reclaimed, _) = store
        .claim_job(&caller(), "worker-2", now + 30_001, 30_000, one)
        .expect("claim")
        .expect("the expired lease is claimable");
    assert_eq!(reclaimed.id, ID);
    assert_eq!(reclaimed.attempts, 2);
    // The census is the whole host's, never one project's: a fairness
    // decision cannot be made from a narrowed view.
    assert_eq!(
        store
            .census("owner-digest", "stable", now)
            .expect("other lane"),
        Census::default()
    );
}

/// A row whose stored context does not read is work no worker can ever take:
/// `claim_job` skips it and no caller can see it. So it must be in no census
/// either. Counted, it would spend a project's queue bound for ever, and it
/// would stand for ever as another project's waiting work — refusing every
/// project the second slot its share allows, on behalf of work that will
/// never run.
#[test]
fn a_row_whose_context_does_not_read_is_in_no_census_and_in_no_ones_way() {
    let directory = test_directory::tempdir().expect("temp");
    let mut store = store(directory.path());
    let mut wedged = job_b();
    wedged.context.as_mut().expect("context").project = String::new();
    assert!(
        store.submit_job(&wedged, b"wedged", roomy()).is_err(),
        "this build never writes such a row itself"
    );
    insert_row(directory.path(), &wedged, b"wedged");
    let now = 1_700_000_001_000;
    assert_eq!(
        store.census("owner-digest", "canary", now).expect("census"),
        Census::default(),
        "work that can never be claimed is not work in flight"
    );
    assert!(store.job(&caller(), ID_B).expect("read").is_none());
    assert_eq!(claim(&mut store, "worker-0", now, roomy()), None);
    // The queue bound is spent on work that can run, and on nothing else.
    let tight = Limits {
        global_running: 4,
        per_project_running: 1,
        per_project_queued: 1,
        global_queued: 1,
    };
    store
        .submit_job(&job(Some(context())), b"input", tight)
        .expect("the wedged row holds no one's place in the queue");
    // …and it yields no share: project-a takes a second slot because nothing
    // that can actually run is waiting for one.
    let share = Limits {
        global_running: 4,
        per_project_running: 1,
        per_project_queued: 8,
        global_queued: 16,
    };
    store
        .submit_job(&nth("project-a", 2), b"input-a2", share)
        .expect("a second batch for the same project");
    assert_eq!(
        claim(&mut store, "worker-1", now, share).as_deref(),
        Some("project-a")
    );
    assert_eq!(
        claim(&mut store, "worker-2", now, share).as_deref(),
        Some("project-a"),
        "a row nothing can claim may not hold a share against a project that can"
    );
}

#[test]
fn a_claim_skips_the_project_over_its_share_and_takes_the_next_that_can_run() {
    let directory = test_directory::tempdir().expect("temp");
    let mut store = store(directory.path());
    let share = Limits {
        global_running: 4,
        per_project_running: 1,
        ..roomy()
    };
    // Queue order: a1, a2, b1 — A is older, and would take everything.
    store
        .submit_job(&nth("project-a", 1), b"a1", share)
        .expect("a1");
    store
        .submit_job(&nth("project-a", 2), b"a2", share)
        .expect("a2");
    store
        .submit_job(&nth("project-b", 3), b"b3", share)
        .expect("b3");
    let now = 1_700_000_001_000;
    assert_eq!(
        claim(&mut store, "w1", now, share).as_deref(),
        Some("project-a")
    );
    // A holds its share and B waits under its own: a2 is skipped for b3.
    assert_eq!(
        claim(&mut store, "w2", now + 1, share).as_deref(),
        Some("project-b")
    );
    // A is over its share, but nothing waits that could take the slot, so the
    // machine is used rather than idled.
    assert_eq!(
        claim(&mut store, "w3", now + 2, share).as_deref(),
        Some("project-a")
    );
    assert_eq!(claim(&mut store, "w4", now + 3, share), None);
    // With every worker busy nothing is claimed and nothing is mutated.
    store
        .submit_job(&nth("project-b", 4), b"b4", share)
        .expect("b4");
    let one = Limits {
        global_running: 1,
        ..share
    };
    assert_eq!(claim(&mut store, "w5", now + 4, one), None);
    let phases = [1, 2, 3, 4]
        .into_iter()
        .map(|n| {
            let id = if n == 3 || n == 4 {
                nth("project-b", n).id
            } else {
                nth("project-a", n).id
            };
            store.job(&caller(), &id).expect("read").expect("row").phase
        })
        .collect::<Vec<_>>();
    assert_eq!(
        phases,
        vec![
            Phase::Running,
            Phase::Running,
            Phase::Running,
            Phase::Queued
        ]
    );
}

#[test]
fn concurrent_claims_keep_the_share_because_the_census_is_read_in_the_claim() {
    let directory = test_directory::tempdir().expect("temp");
    let path = directory.path().join("store.sqlite");
    {
        let mut store = store(directory.path());
        // Four of A, older, then two of B. A share of two each, a pool of four.
        for n in 1..=4 {
            store
                .submit_job(&nth("project-a", n), format!("a{n}").as_bytes(), roomy())
                .expect("a");
        }
        for n in 5..=6 {
            store
                .submit_job(&nth("project-b", n), format!("b{n}").as_bytes(), roomy())
                .expect("b");
        }
    }
    let share = Limits {
        global_running: 4,
        per_project_running: 2,
        ..roomy()
    };
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
    let claimed = (0..6)
        .map(|i| {
            let (path, barrier) = (path.clone(), barrier.clone());
            std::thread::spawn(move || {
                let mut store = Store::open(&path).expect("open");
                barrier.wait();
                claim(&mut store, &format!("w{i}"), 1_700_000_001_000 + i, share)
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .filter_map(|handle| handle.join().expect("thread"))
        .collect::<Vec<_>>();
    // Whatever the interleaving: the pool is full, and A — older and eager —
    // holds exactly its share while B holds the rest.
    assert_eq!(claimed.len(), 4);
    assert_eq!(claimed.iter().filter(|p| *p == "project-a").count(), 2);
    assert_eq!(claimed.iter().filter(|p| *p == "project-b").count(), 2);
    let census = Store::open(&path)
        .expect("open")
        .census("owner-digest", "canary", 1_700_000_001_010)
        .expect("census");
    assert_eq!(census.running.len(), 4);
    assert_eq!(census.queued.len(), 2);
}

#[test]
fn a_retired_result_frees_only_the_bytes_and_keeps_the_row_as_evidence() {
    // WHY: a Solar publication that lost is freed the way a report batch is —
    // its bytes leave, the job row stays — and asking again frees nothing.
    let directory = test_directory::tempdir().expect("temp");
    let mut store = store(directory.path());
    store
        .submit_job(&job(Some(context())), b"input", roomy())
        .expect("submit");
    store.submit_job(&job_b(), b"input-b", roomy()).expect("b");
    for (id, worker) in [(ID, "worker-1"), (ID_B, "worker-2")] {
        store
            .update_job(
                &caller(),
                id,
                Event::Claim {
                    worker,
                    lease_ms: 30_000,
                },
                1_700_000_001_000,
                None,
            )
            .expect("claim");
        store
            .update_job(
                &caller(),
                id,
                Event::Complete {
                    worker,
                    result_sha256: RESULT,
                },
                1_700_000_002_000,
                Some(b"result bytes"),
            )
            .expect("complete");
    }
    // Another project's caller frees nothing and sees nothing.
    let b = caller().about(Some("project-b"));
    assert_eq!(store.retire_job_result(&b, ID).expect("fenced"), 0);
    assert_eq!(
        store.job_result(&caller(), ID).expect("result").unwrap(),
        b"result bytes"
    );
    let freed = store.retire_job_result(&caller(), ID).expect("retire");
    assert_eq!(freed, b"result bytes".len() as u64);
    assert!(store.job_result(&caller(), ID).expect("result").is_none());
    let evidence = store.job(&caller(), ID).expect("row").expect("still there");
    assert_eq!(evidence.phase, Phase::Completed);
    assert_eq!(evidence.result_sha256.as_deref(), Some(RESULT));
    assert!(store.job_input(&caller(), ID).expect("input").is_some());
    assert_eq!(
        store.retire_job_result(&caller(), ID).expect("again"),
        0,
        "a result already gone is not an error"
    );
    assert_eq!(
        store.job_result(&caller(), ID_B).expect("b").unwrap(),
        b"result bytes",
        "only the named job's bytes left"
    );
}
