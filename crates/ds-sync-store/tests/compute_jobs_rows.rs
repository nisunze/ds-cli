//! The compute job table carries the execution context without a schema
//! change, answers only what a caller may see, and still reads a row written
//! before the context existed.
//!
//! `compute_jobs.row` is one JSON blob, so the context added to
//! `compute_jobs::Job` is a compatible addition — but "compatible" is a claim
//! about a real SQLite file, not about a serde attribute. These tests write
//! and read actual rows.

use ds_command_kernel::compute_jobs::{EngineKind, Event, Job, Phase};
use ds_command_kernel::execution_context::{
    ExecutionContext, Principal, Slot, SOLAR_PROCESSING, TRANSFORMER_PROCESSING,
};
use ds_sync_store::{Census, JobCaller, Store, UNRECOVERED_PROJECT};

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
    job
}
fn always(_: &str) -> bool {
    true
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

#[test]
fn a_context_survives_submission_restart_claim_and_completion() {
    let directory = tempfile::tempdir().expect("temp");
    let submitted = store(directory.path())
        .submit_job(&job(Some(context())), b"input")
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
        .claim_job(
            &caller(),
            "worker-1",
            1_700_000_001_000,
            30_000,
            &mut always,
        )
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
    let directory = tempfile::tempdir().expect("temp");
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
    // And it is never executed while its project is unknown.
    assert!(store(directory.path())
        .claim_job(
            &caller(),
            "worker-1",
            1_700_000_001_000,
            30_000,
            &mut always
        )
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
fn a_legacy_solar_row_recovers_from_its_sealed_input_and_a_transformer_row_from_the_selection() {
    let directory = tempfile::tempdir().expect("temp");
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
    let sealed = |job: &Job, input: &[u8]| {
        (job.engine == EngineKind::SolarPrepared && input == b"sealed for project-b")
            .then(|| "project-b".to_owned())
    };
    let recovery = store(directory.path())
        .recover_contexts(
            &principal(),
            "owner-digest",
            Some("project-a"),
            &sealed,
            1_700_000_500_000,
        )
        .expect("recover");
    assert_eq!(recovery.from_sealed_input, 1);
    assert_eq!(recovery.from_saved_selection, 1);
    assert!(recovery.unrecoverable.is_empty());
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
    let transformer = store(directory.path())
        .job(&caller(), ID_B)
        .expect("read")
        .expect("present")
        .context
        .expect("recovered");
    assert_eq!(transformer.project, "project-a", "the saved selection");
    assert_eq!(transformer.operation, TRANSFORMER_PROCESSING);
    // Recovered rows are visible per project now, and claimable.
    assert_eq!(
        store(directory.path())
            .jobs(&caller().about(Some("project-b")), 10)
            .expect("list")
            .len(),
        1
    );
    // A second pass rewrites nothing: they are stored now.
    let again = store(directory.path())
        .recover_contexts(
            &principal(),
            "owner-digest",
            Some("project-a"),
            &sealed,
            1_700_000_600_000,
        )
        .expect("recover");
    assert_eq!(again.stored, 2);
    assert_eq!(again.from_sealed_input + again.from_saved_selection, 0);
}

#[test]
fn a_transformer_row_with_no_saved_selection_is_named_unrecoverable_not_guessed() {
    let directory = tempfile::tempdir().expect("temp");
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
            None,
            &|_, _| None,
            1_700_000_500_000,
        )
        .expect("recover");
    assert_eq!(recovery.unrecoverable, vec![ID_B.to_owned()]);
    assert!(store(directory.path())
        .job(&caller(), ID_B)
        .expect("read")
        .expect("present")
        .context
        .is_none());
    // It still counts against the host's capacity, under a name no project
    // directory can return, so fairness cannot be gamed with it.
    assert_eq!(
        store(directory.path())
            .census("owner-digest", "canary")
            .expect("census"),
        Census {
            running: vec![],
            queued: vec![Slot {
                project: UNRECOVERED_PROJECT.into()
            }],
        }
    );
}

#[test]
fn a_context_carrying_row_is_still_fenced_by_owner_and_lane() {
    let directory = tempfile::tempdir().expect("temp");
    store(directory.path())
        .submit_job(&job(Some(context())), b"input")
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
    let directory = tempfile::tempdir().expect("temp");
    {
        let mut store = store(directory.path());
        store
            .submit_job(&job(Some(context())), b"input")
            .expect("a");
        store.submit_job(&job_b(), b"input-b").expect("b");
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
fn cancelling_another_projects_job_answers_exactly_what_a_missing_row_answers() {
    let directory = tempfile::tempdir().expect("temp");
    let mut store = store(directory.path());
    store
        .submit_job(&job(Some(context())), b"input")
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
    let directory = tempfile::tempdir().expect("temp");
    let mut store = store(directory.path());
    store
        .submit_job(&job(Some(context())), b"input")
        .expect("a");
    // The same durable id (same owner, lane and key) with another project.
    let moved = job(Some(ExecutionContext {
        project: "project-b".into(),
        ..context()
    }));
    assert!(store
        .submit_job(&moved, b"input")
        .expect_err("scope moved")
        .to_string()
        .contains("compute_job_scope_conflict"));
    // Same project, same bytes: the stored row comes back untouched.
    assert_eq!(
        store
            .submit_job(&job(Some(context())), b"input")
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
        .submit_job(&other_bytes, b"other")
        .expect_err("payload changed")
        .to_string()
        .contains("compute_job_identity_conflict"));
}

#[test]
fn a_row_without_a_context_is_never_written_by_this_build() {
    let directory = tempfile::tempdir().expect("temp");
    let mut store = store(directory.path());
    assert!(store
        .submit_job(&job(None), b"input")
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
        .submit_job(&mismatched, b"input")
        .expect_err("mismatch")
        .to_string()
        .contains("compute_job_context_mismatch"));
}

#[test]
fn the_census_names_running_and_queued_work_by_project_only() {
    let directory = tempfile::tempdir().expect("temp");
    let mut store = store(directory.path());
    store
        .submit_job(&job(Some(context())), b"input")
        .expect("a");
    store.submit_job(&job_b(), b"input-b").expect("b");
    assert_eq!(
        store.census("owner-digest", "canary").expect("census"),
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
        .claim_job(
            &caller(),
            "worker-1",
            1_700_000_001_000,
            30_000,
            &mut always,
        )
        .expect("claim")
        .expect("claimable");
    let census = store.census("owner-digest", "canary").expect("census");
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
    // The census is the whole host's, never one project's: a fairness
    // decision cannot be made from a narrowed view.
    assert_eq!(
        store.census("owner-digest", "stable").expect("other lane"),
        Census::default()
    );
}

#[test]
fn a_claim_skips_the_project_its_capacity_gate_refuses_and_takes_the_next() {
    let directory = tempfile::tempdir().expect("temp");
    let mut store = store(directory.path());
    store
        .submit_job(&job(Some(context())), b"input")
        .expect("a");
    store.submit_job(&job_b(), b"input-b").expect("b");
    // A is older, so it is first in queue order — and refused by the gate.
    let mut asked = Vec::new();
    {
        let mut gate = |project: &str| {
            asked.push(project.to_owned());
            project != "project-a"
        };
        let (claimed, _) = store
            .claim_job(&caller(), "worker-1", 1_700_000_001_000, 30_000, &mut gate)
            .expect("claim")
            .expect("b is claimable");
        assert_eq!(claimed.id, ID_B);
    }
    assert_eq!(asked, vec!["project-a".to_owned(), "project-b".to_owned()]);
    // With every project refused nothing is claimed and nothing is mutated.
    let mut none = |_: &str| false;
    assert!(store
        .claim_job(&caller(), "worker-2", 1_700_000_002_000, 30_000, &mut none)
        .expect("claim")
        .is_none());
    let phases = [ID, ID_B]
        .into_iter()
        .map(|id| store.job(&caller(), id).expect("read").expect("row").phase)
        .collect::<Vec<_>>();
    assert_eq!(phases, vec![Phase::Queued, Phase::Running]);
}
