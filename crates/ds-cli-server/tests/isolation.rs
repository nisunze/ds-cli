//! Project and instance isolation on ONE Server, proven end to end.
//!
//! Normative source: `ds-web/docs/project-and-instance-isolation-contract.md`
//! (§"One verified context per operation", §"Server jobs and multiple
//! clients", §"Concurrency and capacity", acceptance items 2, 5, 6, 7, 8) and
//! the slice contract's §3, whose eight numbered proofs each have a test named
//! `item<N>_…` below.
//!
//! Every assertion here is made at the authoritative boundary: a real loopback
//! listener the tests speak HTTP to, and the durable SQLite queue on disk. No
//! route is stubbed, no decision is mocked, and nothing is checked by reading
//! a source string. There is no gateway anywhere — admission, queueing,
//! execution, restart recovery, capacity and the jobs table are all proven
//! with no upstream present at all, which is the offline-first claim.
//!
//! Two callers are used deliberately:
//!   * `host.raw(…)` is the exact wire, byte for byte. Non-disclosure is a
//!     property of those bytes, so the equality proofs use only these.
//!   * `ds_cli_server::{submit, status, cancel, result, …}` is the real `ds`
//!     client, reading the protected `connection.json`, sending the project on
//!     every call and re-raising the Server's typed refusal. Where a proof
//!     also has to hold for an operator typing `ds`, it goes through these.
//!
//! What is NOT proven here, and why, is stated in the `unproven_…` tests at
//! the bottom: each is `#[ignore]`d with its reason in its own name.

mod fixtures;

use ds_cli_contract::{
    outcome::ExitClass,
    spec::{Arg, Authority, Availability, Chapter, Command, Effect, Execution},
};
use ds_compute_runtime as runtime;
use fixtures::*;
use serde_json::{Value, json};
use std::sync::Arc;

use ds_cli_server::{CANCEL, RESULT, SOLAR_SUBMIT, STATUS, SUBMIT};

/// A 64-character digest that is a perfectly well-formed job id and has never
/// named a job.
fn guessed() -> String {
    "0".repeat(64)
}

// ─────────────────────────────────────────────────────────────────────────
// §3.1  Two projects, side by side, on one Server
// ─────────────────────────────────────────────────────────────────────────

/// Solar for A (its sealed input names A), a transformer batch and a layer
/// write for B: three admitted operations, two projects, one host, no
/// desktop and no gateway. Each carries its own immutable context, on the
/// wire and in the durable row, and nothing of A is reachable under B.
#[test]
fn item1_two_projects_overlap_on_one_server_with_distinct_contexts_and_no_leakage() {
    let host = Host::start(&[A, B, C], limits(), B);

    // Solar for A. The project is not named at all: the sealed envelope names
    // its own, and that name is authoritative.
    let solar = host.raw("POST", "/v1/solar-processing/solar-a", Some(&solar_submission()));
    assert_eq!(solar.status, 202, "{:?}", solar.json());
    let solar_context = solar.json()["job"]["context"].clone();
    assert_eq!(solar_context["project"], A);
    assert_eq!(solar_context["operation"], "solar_processing");
    assert_eq!(solar_context["principal_uid"], UID);
    assert_eq!(solar_context["lane"], LANE);
    assert_eq!(solar_context["deployment"], DEPLOYMENT);
    let solar_id = solar.json()["job"]["id"].as_str().unwrap().to_owned();

    // A transformer batch for B, through the real `ds server submit`.
    let input = host.input("batch-b.json", &transformer("T-B"));
    let queued = ds_cli_server::submit(
        &host.args(&SUBMIT, &["--key", "batch-b", "--input", &input, "--project", B]),
        &context(),
    )
    .expect("B's batch is admitted");
    assert_eq!(queued["job"]["context"]["project"], B);
    assert_eq!(queued["job"]["context"]["operation"], "transformer_processing");
    let batch_id = queued["job"]["id"].as_str().unwrap().to_owned();
    assert_ne!(solar_id, batch_id);

    // A layer write for B — the third operation, in the third shape, on the
    // same host, admitted under its own context.
    let hidden = host.raw(
        "POST",
        &format!("/v1/layers/visibility?project={B}"),
        Some(br#"{"layers":["survey/poles"],"visible":false}"#),
    );
    assert_eq!(hidden.status, 200, "{:?}", hidden.json());
    assert_eq!(hidden.json()["project"], B);
    assert_eq!(hidden.json()["persisted"], "native_local");

    // Two distinct contexts, from the store on disk rather than from an answer.
    let rows = host.stored(None);
    assert_eq!(rows.len(), 2);
    let projects: Vec<String> = rows
        .iter()
        .map(|job| job.context.as_ref().expect("every row is about a project").project.clone())
        .collect();
    assert!(projects.contains(&A.to_owned()) && projects.contains(&B.to_owned()));
    assert_eq!(host.stored(Some(A)).len(), 1);
    assert_eq!(host.stored(Some(B)).len(), 1);

    // And through the client, per project, with nothing of A under B.
    let only_b = ds_cli_server::status(&host.args(&STATUS, &["--project", B]), &context())
        .expect("B's jobs");
    assert_eq!(only_b["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(only_b["jobs"][0]["id"], batch_id);
    let only_a = ds_cli_server::status(&host.args(&STATUS, &["--project", A]), &context())
        .expect("A's jobs");
    assert_eq!(only_a["jobs"].as_array().unwrap().len(), 1);
    assert_eq!(only_a["jobs"][0]["id"], solar_id);
    // Unnarrowed, the operator of this one account sees both — and each row
    // still says which project it is about.
    let both = host.raw("GET", "/v1/jobs", None);
    assert_eq!(both.json()["jobs"].as_array().unwrap().len(), 2);

    // A project this account is a member of but has no work in holds nothing,
    // and a project it is not a member of answers exactly the same way: an
    // empty list is never a disclosure.
    for project in [C, OUTSIDE] {
        let empty = host.raw("GET", &format!("/v1/jobs?project={project}"), None);
        assert_eq!(empty.status, 200);
        assert_eq!(empty.json(), json!({"jobs": [], "more": false}));
    }

    // The layer write landed under B's preference scope and under no other.
    let listed = host.raw("GET", &format!("/v1/layers?project={B}&limit=100"), None);
    assert_eq!(listed.status, 200, "{:?}", listed.json());
    let poles = listed.json()["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == "survey/poles")
        .cloned()
        .expect("the catalogue still holds the family");
    assert_eq!(poles["visibility"]["any_visible"], false);
}

// ─────────────────────────────────────────────────────────────────────────
// §3.2  A project named for one call never becomes Server state
// ─────────────────────────────────────────────────────────────────────────

/// The saved selection is the client's default, sent explicitly; the Server
/// holds none. Naming C for one call admits that call under C and moves
/// nothing already admitted; naming nothing refuses rather than substituting;
/// and a sealed Solar input's own project outranks a named one.
#[test]
fn item2_naming_a_project_for_one_call_never_rescopes_anything_else() {
    let host = Host::start(&[A, B, C], limits(), A);
    let mut queued = Vec::new();
    for (key, project, name) in [("a1", A, "T-A"), ("b1", B, "T-B")] {
        let input = host.input(&format!("{key}.json"), &transformer(name));
        let value = ds_cli_server::submit(
            &host.args(&SUBMIT, &["--key", key, "--input", &input, "--project", project]),
            &context(),
        )
        .expect("admitted");
        assert_eq!(value["job"]["context"]["project"], project);
        queued.push(value["job"]["id"].as_str().unwrap().to_owned());
    }

    // The caller's selection moves to C mid-flight — which, on the wire, is
    // simply the next call naming C. A's and B's jobs are untouched.
    let input = host.input("c1.json", &transformer("T-C"));
    let under_c = ds_cli_server::submit(
        &host.args(&SUBMIT, &["--key", "c1", "--input", &input, "--project", C]),
        &context(),
    )
    .expect("admitted under the new selection");
    assert_eq!(under_c["job"]["context"]["project"], C);
    for (id, project) in [(&queued[0], A), (&queued[1], B)] {
        let still = host.raw("GET", &format!("/v1/jobs/{id}?project={project}"), None);
        assert_eq!(still.status, 200, "{:?}", still.json());
        assert_eq!(still.json()["job"]["context"]["project"], project);
        assert_eq!(still.json()["job"]["phase"], "queued");
    }

    // With nothing named, the Server substitutes nothing — not the previous
    // caller's project, not the last one it admitted, not a selection of its
    // own, because it holds none.
    let unnamed = host.raw(
        "POST",
        "/v1/transformer-processing/unnamed",
        Some(&transformer("T-X")),
    );
    assert_eq!(unnamed.status, 400, "{:?}", unnamed.json());
    assert_eq!(unnamed.code(), "project_required");
    // Nor does it accept a project this account is not a member of.
    let outside = host.raw(
        "POST",
        &format!("/v1/transformer-processing/outside?project={OUTSIDE}"),
        Some(&transformer("T-X")),
    );
    assert_eq!(outside.status, 400, "{:?}", outside.json());
    assert_eq!(outside.code(), "project_not_visible");

    // A sealed Solar input prepared for A, submitted with --project C: the
    // bytes outrank the query, and the caller is told which is wrong.
    let sealed = host.input("solar.json", &solar_submission());
    let refused = ds_cli_server::solar_submit(
        &host.args(&SOLAR_SUBMIT, &["--key", "solar-c", "--input", &sealed, "--project", C]),
        &context(),
    )
    .expect_err("the sealed project wins");
    assert_eq!(refused.code(), "scope_mismatch");
    assert_eq!(refused.class(), ExitClass::Conflict);
    assert!(refused.remedy_text().is_some());
    // …and the same envelope named honestly is admitted under A.
    let admitted = ds_cli_server::solar_submit(
        &host.args(&SOLAR_SUBMIT, &["--key", "solar-a", "--input", &sealed, "--project", A]),
        &context(),
    )
    .expect("the sealed project, named");
    assert_eq!(admitted["job"]["context"]["project"], A);

    // Three admitted jobs, three projects, and the refusals queued nothing.
    assert_eq!(host.stored(None).len(), 4);
    assert_eq!(host.stored(Some(C)).len(), 1);
}

// ─────────────────────────────────────────────────────────────────────────
// §3.3  One answer for every kind of "not yours"
// ─────────────────────────────────────────────────────────────────────────

/// A foreign principal, a foreign lane, the wrong project and an id that never
/// existed get the SAME bytes — status, class, code, sentence and remedy — on
/// status, cancel and result alike. Anything else is a disclosure.
#[test]
fn item3_a_foreign_principal_lane_project_and_a_guessed_id_are_one_byte_identical_answer() {
    let mut host = Host::start(&[A, B, C], limits(), A);
    let input = host.input("a1.json", &transformer("T-A"));
    let admitted = ds_cli_server::submit(
        &host.args(&SUBMIT, &["--key", "a1", "--input", &input, "--project", A]),
        &context(),
    )
    .expect("admitted");
    let id = admitted["job"]["id"].as_str().unwrap().to_owned();
    let unknown = guessed();

    // Two more listeners over the SAME durable queue: one signed in as another
    // account, one on the other lane. Each presents the durable owner fence
    // its identity really derives, as production does.
    let stranger = host.shadow("uid-somebody-else", LANE, &owner_digest("uid-somebody-else", LANE));
    let other_lane = host.shadow(UID, "canary", &owner_digest(UID, "canary"));
    // And one more that deliberately presents THIS Server's owner digest, so
    // the durable SQL fence separates nothing and only the execution context
    // is left to do it. Production never produces this — the owner digest is
    // derived from the uid — which is exactly why it is worth asking.
    let unfenced = host.shadow("uid-somebody-else", LANE, OWNER);

    for (route, method) in [
        (format!("/v1/jobs/{{}}"), "GET"),
        (format!("/v1/jobs/{{}}/cancel"), "POST"),
        (format!("/v1/jobs/{{}}/result"), "GET"),
    ] {
        let own = |target: &str, project: &str| route.replace("{}", target) + "?project=" + project;
        let answers = vec![
            ("wrong project", host.raw(method, &own(&id, B), None)),
            ("guessed id", host.raw(method, &own(&unknown, B), None)),
            ("guessed id, unnarrowed", host.raw(method, &route.replace("{}", &unknown), None)),
            ("foreign principal", stranger.raw(method, &own(&id, A), None)),
            ("foreign lane", other_lane.raw(method, &own(&id, A), None)),
            ("foreign principal, shared durable fence", unfenced.raw(method, &own(&id, A), None)),
        ];
        let first = &answers[0].1;
        assert_eq!(first.status, 409, "{:?}", first.json());
        assert_eq!(first.json()["error"], "job not found");
        assert_eq!(first.json()["code"], "not_visible");
        for (what, answer) in &answers[1..] {
            assert_eq!(
                answer, first,
                "{method} {route}: `{what}` must be byte-identical to a wrong project"
            );
        }
    }

    // And none of that touched the job.
    let mine = host.raw("GET", &format!("/v1/jobs/{id}?project={A}"), None);
    assert_eq!(mine.json()["job"]["phase"], "queued");

    // A job the caller CAN see, that simply has not finished, stays
    // distinguishable INSIDE its own project — "not yours" and "not yet" are
    // one answer across projects and two answers within one.
    let not_yet = host.raw("GET", &format!("/v1/jobs/{id}/result?project={A}"), None);
    assert_eq!(not_yet.code(), "server_refused");
    assert_ne!(not_yet.json()["error"], "job not found");

    // The client re-raises the same code and class, so an operator plans for
    // one answer whichever door was tried.
    let refused = ds_cli_server::status(
        &host.args(&STATUS, &["--job", &id, "--project", B]),
        &context(),
    )
    .expect_err("another project's job");
    assert_eq!(refused.code(), "not_visible");
    assert_eq!(refused.message(), "job not found");
    assert_eq!(refused.class(), ExitClass::Conflict);

    // Submission is fenced the same way. A second account's work on the same
    // machine is its own: its key derives a different durable id, and neither
    // connection's list ever shows the other's row.
    let theirs = stranger.raw(
        "POST",
        &format!("/v1/transformer-processing/a1?project={A}"),
        Some(&transformer("T-A")),
    );
    assert_eq!(theirs.status, 202, "{:?}", theirs.json());
    assert_eq!(theirs.json()["job"]["context"]["principal_uid"], "uid-somebody-else");
    let their_id = theirs.json()["job"]["id"].as_str().unwrap().to_owned();
    assert_ne!(their_id, id, "the same key under another account is other work");
    let listed = |answer: Value| -> Vec<String> {
        answer["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|job| job["id"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(listed(host.raw("GET", "/v1/jobs", None).json()), vec![id.clone()]);
    assert_eq!(listed(stranger.raw("GET", "/v1/jobs", None).json()), vec![their_id]);

    // Take the durable fence away and the same key DOES land on this
    // connection's row. It is still not handed over: the row itself refuses a
    // context whose principal is not the one that admitted it, so a store that
    // ever stopped deriving its owner from the uid could still not reuse or
    // overwrite another account's result.
    let borrowed = unfenced.raw(
        "POST",
        &format!("/v1/transformer-processing/a1?project={A}"),
        Some(&transformer("T-A")),
    );
    assert!(borrowed.status >= 400, "{:?}", borrowed.json());
    assert!(
        String::from_utf8_lossy(&borrowed.body).contains("compute_job_scope_conflict"),
        "{:?}",
        borrowed.json()
    );

    // On disk, after everything above: the original row, untouched, still
    // about A, still queued, still under the principal that admitted it.
    let mine = host
        .stored(Some(A))
        .into_iter()
        .find(|job| job.id == id)
        .expect("the row is still there");
    assert_eq!(mine.phase, ds_command_kernel::compute_jobs::Phase::Queued);
    let stored = mine.context.expect("its context");
    assert_eq!(stored.project, A);
    assert_eq!(stored.principal_uid, UID);
    assert_eq!(stored.idempotency_key, "a1");
}

/// One Server serves one authenticated account and many of its projects. A
/// request that claims another account is told so by name instead of being
/// quietly served under this one.
#[test]
fn multi_principal_access_is_refused_by_name_rather_than_served_quietly() {
    let host = Host::start(&[A], limits(), A);
    let named = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
        .get(format!("http://{}/v1/jobs", host.address))
        .header("authorization", format!("Bearer {}", host.token))
        .header("x-ds-principal", "uid-somebody-else")
        .call()
        .expect("answered");
    assert_eq!(named.status().as_u16(), 401);
}

// ─────────────────────────────────────────────────────────────────────────
// §3.4  A key is one handle on one piece of work
// ─────────────────────────────────────────────────────────────────────────

/// The same key with the same bytes in the same project is the same job; with
/// another project or other bytes it is a named refusal, never a reused result
/// and never a silent overwrite.
#[test]
fn item4_a_reused_key_may_not_change_its_project_or_its_payload() {
    let host = Host::start(&[A, B], limits(), A);
    let same = host.input("same.json", &transformer("T1"));
    let other = host.input("other.json", &transformer("T2"));
    let submit = |key: &str, input: &str, project: &str| {
        ds_cli_server::submit(
            &host.args(&SUBMIT, &["--key", key, "--input", input, "--project", project]),
            &context(),
        )
    };

    let first = submit("shared", &same, A).expect("admitted");
    let id = first["job"]["id"].as_str().unwrap().to_owned();
    // An idempotent resubmit: the stored job, not a second one.
    let again = submit("shared", &same, A).expect("the same job");
    assert_eq!(again["job"]["id"], id);
    assert_eq!(again["job"]["created_at_ms"], first["job"]["created_at_ms"]);

    let moved = submit("shared", &same, B).expect_err("a key may not move project");
    assert_eq!(moved.code(), "scope_mismatch_for_key");
    assert_eq!(moved.class(), ExitClass::Conflict);
    let changed = submit("shared", &other, A).expect_err("a key may not change bytes");
    assert_eq!(changed.code(), "payload_changed_for_key");
    assert_eq!(changed.class(), ExitClass::Conflict);

    // One row, its original bytes, its original context.
    let rows = host.stored(None);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert_eq!(rows[0].input_sha256, runtime::digest(&transformer("T1")));
    assert_eq!(rows[0].context.as_ref().unwrap().project, A);
    assert_eq!(rows[0].context.as_ref().unwrap().idempotency_key, "shared");
}

// ─────────────────────────────────────────────────────────────────────────
// §3.5  Restart, and rows a released Server wrote
// ─────────────────────────────────────────────────────────────────────────

/// A Server restarted over a durable queue keeps every job in the project it
/// was admitted into, gives a context to the rows written before contexts
/// existed — Solar's from its own sealed bytes, a transformer's from the
/// operator's selection — and never runs the same key twice.
#[test]
fn item5_a_restart_recovers_every_context_including_rows_a_released_server_wrote() {
    let mut host = Host::start_over(&[A, B, C], limits(), A, &legacy_queue_fixture());
    // Two rows the released Server left behind, readable but nameless.
    let legacy = host.stored(None);
    assert_eq!(legacy.len(), 2, "the fixture queue's two pre-slice rows");
    assert!(legacy.iter().all(|job| job.context.is_none()));
    // A nameless row is invisible to any caller narrowing to a project: it is
    // not known to be in one, so it is not claimed to be.
    assert!(host.stored(Some(A)).is_empty());

    // This Server admits new work into the same queue.
    let input = host.input("b1.json", &transformer("T-B"));
    let admitted = ds_cli_server::submit(
        &host.args(&SUBMIT, &["--key", "b1", "--input", &input, "--project", B]),
        &context(),
    )
    .expect("admitted");
    let live = admitted["job"]["id"].as_str().unwrap().to_owned();

    // Restart on the same protected state and the same fixed loopback port.
    host.restart(A);
    // `serve` starts its workers, and starting them is when a released
    // Server's rows get the context they never had — before any worker can
    // claim one. The pool is started with the device paused so the recovery
    // is observed on its own, with nothing executed.
    let workers = host.workers(Arc::new(Paused), 1, Some(C));
    workers.stop();
    drop(workers);

    // Every row now names its project, and a second recovery pass rebuilds
    // nothing: they are stored now, which is what proves the first pass ran.
    let again = runtime::recover_contexts(&host.database(), &host.identity, Some(C))
        .expect("a second pass");
    assert_eq!(again.stored, 3);
    assert_eq!(again.from_sealed_input + again.from_saved_selection, 0);
    assert!(again.unrecoverable.is_empty());

    // The Solar row took its project from its own sealed input …
    let solar = host.raw(
        "GET",
        &format!("/v1/jobs/{}?project={A}", legacy_solar_id()),
        None,
    );
    assert_eq!(solar.status, 200, "{:?}", solar.json());
    assert_eq!(solar.json()["job"]["context"]["project"], A);
    assert_eq!(solar.json()["job"]["context"]["operation"], "solar_processing");
    // … the transformer row, which carries no project by design, from the
    // operator's saved selection …
    let transformer_row = host.raw(
        "GET",
        &format!("/v1/jobs/{}?project={C}", legacy_transformer_id()),
        None,
    );
    assert_eq!(transformer_row.status, 200, "{:?}", transformer_row.json());
    assert_eq!(transformer_row.json()["job"]["context"]["project"], C);
    assert_eq!(
        transformer_row.json()["job"]["context"]["operation"],
        "transformer_processing"
    );
    // … and the row this build admitted kept the project it was admitted into.
    let survivor = host.raw("GET", &format!("/v1/jobs/{live}?project={B}"), None);
    assert_eq!(survivor.status, 200, "{:?}", survivor.json());
    assert_eq!(survivor.json()["job"]["context"]["project"], B);

    // Each is reachable only under its own project, recovered or not.
    for (id, wrong) in [
        (legacy_solar_id(), B),
        (legacy_transformer_id(), A),
        (live.clone(), A),
    ] {
        let hidden = host.raw("GET", &format!("/v1/jobs/{id}?project={wrong}"), None);
        assert_eq!(hidden.status, 409);
        assert_eq!(hidden.json()["error"], "job not found");
    }

    // A different client reconnecting and retrying the identical request after
    // the restart recovers the job by its stable id — it does not queue a
    // second one, so nothing is published twice.
    let retried = ds_cli_server::submit(
        &host.args(&SUBMIT, &["--key", "b1", "--input", &input, "--project", B]),
        &context(),
    )
    .expect("the same job");
    assert_eq!(retried["job"]["id"], live);
    assert_eq!(host.stored(None).len(), 3, "no duplicate row after a restart");
}

/// A transformer row a released Server wrote, and no selection to recover it
/// into, is named rather than guessed: it stays readable and is never claimed.
#[test]
fn item5_a_pre_slice_row_with_no_honest_project_is_named_not_guessed() {
    let host = Host::start_over(&[A, B], limits(), A, &legacy_queue_fixture());
    let recovery = runtime::recover_contexts(&host.database(), &host.identity, None)
        .expect("recovery runs");
    assert_eq!(recovery.from_sealed_input, 1, "Solar names its own project");
    assert_eq!(recovery.from_saved_selection, 0);
    assert_eq!(
        recovery.unrecoverable,
        vec![legacy_transformer_id()],
        "no selection, so no project is invented for the transformer row"
    );
    // It is still the operator's own work: visible unnarrowed, absent from
    // every project, and it holds no project's capacity.
    assert_eq!(host.stored(None).len(), 2);
    assert!(host.stored(Some(A)).iter().all(|job| job.id != legacy_transformer_id()));
    let nameless = host.raw("GET", "/v1/jobs", None);
    assert_eq!(nameless.json()["jobs"].as_array().unwrap().len(), 2);
}

// ─────────────────────────────────────────────────────────────────────────
// §3.6  Revocation while queued
// ─────────────────────────────────────────────────────────────────────────

/// Losing membership of A while A's work is queued fails exactly that work,
/// with a named error, and B's job runs to completion beside it. Revocation is
/// not a UI switch, and it does not drain another project's queue.
#[test]
fn item6_a_membership_revoked_while_queued_fails_only_that_projects_job() {
    let host = Host::start(&[A, B], limits(), A);
    let mut ids = Vec::new();
    for (key, project, name) in [("a1", A, "T-A"), ("b1", B, "T-B")] {
        let input = host.input(&format!("{key}.json"), &transformer(name));
        let value = ds_cli_server::submit(
            &host.args(&SUBMIT, &["--key", key, "--input", &input, "--project", project]),
            &context(),
        )
        .expect("admitted");
        ids.push(value["job"]["id"].as_str().unwrap().to_owned());
    }
    host.directory.revoke(A);

    let workers = host.workers(Arc::new(Allow), 2, None);
    let terminal = |id: &str, project: &str| -> Option<Value> {
        let answer = host.raw("GET", &format!("/v1/jobs/{id}?project={project}"), None);
        let job = answer.json()["job"].clone();
        matches!(job["phase"].as_str(), Some("failed" | "completed" | "cancelled")).then_some(job)
    };
    assert!(
        until(60, || terminal(&ids[0], A).is_some() && terminal(&ids[1], B).is_some()),
        "both jobs reach a terminal phase"
    );
    workers.stop();
    drop(workers);

    let doomed = terminal(&ids[0], A).expect("A's job is terminal");
    assert_eq!(doomed["phase"], "failed");
    assert!(
        doomed["error"].as_str().unwrap().starts_with(runtime::MEMBERSHIP_REVOKED),
        "{doomed}"
    );
    assert_eq!(doomed["context"]["project"], A, "nothing re-scoped it");
    let refused = host.raw("GET", &format!("/v1/jobs/{}/result?project={A}", ids[0]), None);
    assert_eq!(refused.status, 409, "a revoked project's job produced nothing");

    let survivor = terminal(&ids[1], B).expect("B's job is terminal");
    assert_eq!(survivor["phase"], "completed", "{survivor}");
    let out = host.state.path().join("b-result.json");
    let saved = ds_cli_server::result(
        &host.args(
            &RESULT,
            &["--job", &ids[1], "--project", B, "--out", &out.display().to_string()],
        ),
        &context(),
    )
    .expect("B's authorized job finished and its bytes are readable");
    assert!(saved["byte_count"].as_u64().unwrap() > 0);
    assert!(out.exists());

    // On disk: two rows, each still about the project it was admitted into.
    // A revocation failed one job; it drained, cleared and re-scoped nothing.
    let rows = host.stored(None);
    assert_eq!(rows.len(), 2);
    for row in rows {
        let project = if row.id == ids[0] { A } else { B };
        assert_eq!(row.context.expect("retained").project, project);
    }
    assert_eq!(host.stored(Some(A)).len(), 1);
    assert_eq!(host.stored(Some(B)).len(), 1);
}

// ─────────────────────────────────────────────────────────────────────────
// §3.7  Capacity
// ─────────────────────────────────────────────────────────────────────────

/// Saturating one project bounds that project and no other, saturating the
/// host bounds everyone, both answers are typed and carry retry guidance, and
/// a cancellation gives the room straight back.
#[test]
fn item7_capacity_is_typed_bounded_fair_and_released_by_cancellation() {
    let host = Host::start(
        &[A, B],
        ds_command_kernel::execution_context::Limits {
            global_running: 2,
            per_project_running: 1,
            per_project_queued: 2,
            global_queued: 3,
        },
        A,
    );
    let mut ids = Vec::new();
    for (key, name) in [("a1", "T1"), ("a2", "T2")] {
        let input = host.input(&format!("{key}.json"), &transformer(name));
        let value = ds_cli_server::submit(
            &host.args(&SUBMIT, &["--key", key, "--input", &input, "--project", A]),
            &context(),
        )
        .expect("admitted");
        ids.push(value["job"]["id"].as_str().unwrap().to_owned());
    }

    // A is at its configured share. The answer is typed, retryable, says which
    // scope is full and how long to wait — and never names a project.
    let input = host.input("a3.json", &transformer("T3"));
    let refused = ds_cli_server::submit(
        &host.args(&SUBMIT, &["--key", "a3", "--input", &input, "--project", A]),
        &context(),
    )
    .expect_err("A is full");
    assert_eq!(refused.code(), "capacity_exhausted");
    assert_eq!(refused.class(), ExitClass::Unavailable);
    let detail = refused.detail_value().expect("machine-readable retry guidance");
    assert_eq!(detail["scope"], "project");
    assert!(detail["retry_after_ms"].as_u64().is_some_and(|ms| ms > 0));
    assert!(refused.remedy_text().unwrap().contains("retry after"));
    assert!(!refused.message().contains(A), "a capacity refusal names no project");
    // On the wire it is a 429, so a generic HTTP client backs off correctly.
    let wire = host.raw(
        "POST",
        &format!("/v1/transformer-processing/a4?project={A}"),
        Some(&transformer("T4")),
    );
    assert_eq!(wire.status, 429);
    assert_eq!(wire.code(), "capacity_exhausted");
    assert_eq!(wire.json()["scope"], "project");

    // A long queue in A leaves B's configured room untouched: fairness is the
    // point of a per-project bound.
    let input = host.input("b1.json", &transformer("T5"));
    let admitted = ds_cli_server::submit(
        &host.args(&SUBMIT, &["--key", "b1", "--input", &input, "--project", B]),
        &context(),
    )
    .expect("B still has room");
    assert_eq!(admitted["job"]["context"]["project"], B);

    // Now the whole host is full, and it says so as the host rather than as
    // one project.
    let input = host.input("b2.json", &transformer("T6"));
    let global = ds_cli_server::submit(
        &host.args(&SUBMIT, &["--key", "b2", "--input", &input, "--project", B]),
        &context(),
    )
    .expect_err("the host is full");
    assert_eq!(global.code(), "capacity_exhausted");
    assert_eq!(global.detail_value().unwrap()["scope"], "global");

    // Cancelling releases what it held, immediately and for another project.
    let cancelled = ds_cli_server::cancel(
        &host.args(&CANCEL, &["--job", &ids[0], "--project", A]),
        &context(),
    )
    .expect("the operator's own job");
    assert_eq!(cancelled["job"]["phase"], "cancelled");
    let after = ds_cli_server::submit(
        &host.args(&SUBMIT, &["--key", "b2", "--input", &input, "--project", B]),
        &context(),
    )
    .expect("the room came back");
    assert_eq!(after["job"]["context"]["project"], B);
    // Nothing was lost: three admitted, one cancelled, no silent drop.
    assert_eq!(host.stored(None).len(), 4);
}

// ─────────────────────────────────────────────────────────────────────────
// §3.8  Equal names in different projects
// ─────────────────────────────────────────────────────────────────────────

/// The same transformer, the same bytes, the same digest, in two projects: two
/// jobs, two ids, two results, each reachable only under its own project.
#[test]
fn item8_identical_work_in_two_projects_never_collides() {
    let host = Host::start(&[A, B], limits(), A);
    let input = host.input("daily.json", &transformer("T-SAME"));
    let mut ids = Vec::new();
    for (key, project) in [("daily-a", A), ("daily-b", B)] {
        let value = ds_cli_server::submit(
            &host.args(&SUBMIT, &["--key", key, "--input", &input, "--project", project]),
            &context(),
        )
        .expect("admitted");
        assert_eq!(value["job"]["context"]["project"], project);
        ids.push(value["job"]["id"].as_str().unwrap().to_owned());
    }
    assert_ne!(ids[0], ids[1], "one row per project, not one shared row");
    let rows = host.stored(None);
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].input_sha256, rows[1].input_sha256, "the same bytes");

    // Run them for real. Two results, computed independently.
    let workers = host.workers(Arc::new(Allow), 2, None);
    let done = |id: &str, project: &str| {
        host.raw("GET", &format!("/v1/jobs/{id}?project={project}"), None).json()["job"]["phase"]
            == "completed"
    };
    assert!(
        until(60, || done(&ids[0], A) && done(&ids[1], B)),
        "both complete"
    );
    workers.stop();
    drop(workers);

    // Each result is readable under its own project, absent under the other,
    // and lands in its own file: equal names cannot overwrite each other.
    for (id, own, other, name) in [
        (&ids[0], A, B, "a-result.json"),
        (&ids[1], B, A, "b-result.json"),
    ] {
        let out = host.state.path().join(name);
        let saved = ds_cli_server::result(
            &host.args(
                &RESULT,
                &["--job", id, "--project", own, "--out", &out.display().to_string()],
            ),
            &context(),
        )
        .expect("the owner's own result");
        assert!(saved["byte_count"].as_u64().unwrap() > 0);
        let hidden = host.raw("GET", &format!("/v1/jobs/{id}/result?project={other}"), None);
        assert_eq!(hidden.status, 409);
        assert_eq!(hidden.json()["error"], "job not found");
    }
    // Two distinct results on disk, each row still carrying the context it was
    // admitted under: a claim, an execution and a completion re-scope nothing.
    let rows = host.stored(None);
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert!(row.result_sha256.is_some(), "a result each");
        let project = if row.id == ids[0] { A } else { B };
        assert_eq!(row.context.as_ref().expect("retained").project, project);
        assert_eq!(row.context.as_ref().unwrap().operation, "transformer_processing");
    }
}

// ─────────────────────────────────────────────────────────────────────────
// The routes' own rules
// ─────────────────────────────────────────────────────────────────────────

/// `ds map layer …` executed on a Server names its project explicitly, is
/// verified against membership, and is then held against the document that
/// actually comes back. A refused layer request writes nothing.
#[test]
fn a_layer_request_names_its_project_and_is_fenced_to_the_document() {
    let host = Host::start(&[A, B], limits(), A);
    let body = br#"{"layers":["survey/poles"],"visible":false}"#;

    let unnamed = host.raw("POST", "/v1/layers/visibility", Some(body));
    assert_eq!(unnamed.status, 400, "{:?}", unnamed.json());
    assert_eq!(unnamed.code(), "project_required");
    let outside = host.raw(
        "POST",
        &format!("/v1/layers/visibility?project={OUTSIDE}"),
        Some(body),
    );
    assert_eq!(outside.status, 400);
    assert_eq!(outside.code(), "project_not_visible");
    // A project this account IS a member of, but which is not the one the
    // Server's document source is on: refused, with both names in the remedy.
    let elsewhere = host.raw(
        "POST",
        &format!("/v1/layers/visibility?project={B}"),
        Some(body),
    );
    assert_eq!(elsewhere.status, 409);
    assert_eq!(elsewhere.code(), "project_context_changed");
    let remedy = elsewhere.json()["remedy"].as_str().unwrap().to_owned();
    assert!(remedy.contains(A) && remedy.contains(B), "{remedy}");
    // The project the document is for: the catalogue, applied.
    let applied = host.raw("POST", &format!("/v1/layers/visibility?project={A}"), Some(body));
    assert_eq!(applied.status, 200, "{:?}", applied.json());
    assert_eq!(applied.json()["project"], A);

    // Nothing any refusal touched was written: only A's scope has a revision.
    let listed = host.raw("GET", &format!("/v1/layers?project={A}&limit=100"), None);
    assert_eq!(listed.status, 200);
    let poles = listed.json()["layers"]
        .as_array()
        .unwrap()
        .iter()
        .find(|layer| layer["id"] == "survey/poles")
        .cloned()
        .unwrap();
    assert_eq!(poles["visibility"]["any_visible"], false);

    // And through the `ds` transport, with the Server's refusal re-raised
    // literally rather than translated.
    let refused = ds_cli_server::layers_hide(
        &host.args(&LAYER_HIDE, &["--layer", "survey/poles", "--project", B]),
        &context(),
    )
    .expect_err("the document is on another project");
    assert_eq!(refused.code(), "project_context_changed");
    assert_eq!(refused.class(), ExitClass::Conflict);
    let ok = ds_cli_server::layers_hide(
        &host.args(&LAYER_HIDE, &["--layer", "survey/poles", "--project", A]),
        &context(),
    )
    .expect("the document's own project");
    assert_eq!(ok["project"], A);
}

/// The standing ruling's other half: the Server runs the same operation ids
/// the desktop runs, and where it genuinely cannot — there is no rendered map
/// here — it says which host can, by name, and never answers an empty 404.
#[test]
fn an_operation_that_needs_a_rendered_map_is_refused_by_name_over_the_wire() {
    let host = Host::start(&[A], limits(), A);
    let refused = host.raw("POST", "/v1/map/screenshot", Some(b"{}"));
    assert_eq!(refused.status, 503, "{:?}", refused.json());
    assert_eq!(refused.code(), "needs_paired_map");
    assert!(refused.json()["remedy"].as_str().unwrap().contains("--target desktop"));
    let unknown = host.raw("GET", "/v1/nothing", None);
    assert_eq!(unknown.status, 400);
    assert_eq!(unknown.code(), "unsupported_operation");
    assert!(unknown.json()["error"].as_str().unwrap().contains("/v1/nothing"));
}

/// `/v1/activity` reports per-project scope: one entry per project this
/// connection has durable work in, exactly one when the caller narrows, and
/// nothing at all for a project that holds nothing it may see.
#[test]
fn activity_scope_is_one_entry_per_project_that_holds_work() {
    let host = Host::start(&[A, B, C], limits(), A);
    for (key, project, name) in [("a1", A, "T-A"), ("b1", B, "T-B")] {
        let input = host.input(&format!("{key}.json"), &transformer(name));
        ds_cli_server::submit(
            &host.args(&SUBMIT, &["--key", key, "--input", &input, "--project", project]),
            &context(),
        )
        .expect("admitted");
    }
    assert_eq!(
        ds_cli_server::host::project_scopes(&host.app, None).unwrap(),
        vec![A.to_owned(), B.to_owned()]
    );
    assert_eq!(
        ds_cli_server::host::project_scopes(&host.app, Some(B)).unwrap(),
        vec![B.to_owned()]
    );
    assert!(ds_cli_server::host::project_scopes(&host.app, Some(C)).unwrap().is_empty());
    assert!(ds_cli_server::host::project_scopes(&host.app, Some(OUTSIDE)).unwrap().is_empty());
    // The projection itself needs a Sync Center session per project, which
    // this offline proof deliberately has none of, so the route says so rather
    // than answering an empty envelope that could be mistaken for "no work".
    let answer = host.raw("GET", "/v1/activity", None);
    assert_eq!(answer.status, 409, "{:?}", answer.json());
}

/// Nothing reaches the store without the owner bearer, and a revoked device
/// stops every route at the door — including the ones that would otherwise
/// create a queue file.
#[test]
fn an_unauthenticated_call_never_reads_or_creates_anything() {
    let host = Host::start(&[A], limits(), A);
    let denied = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
        .post(format!("http://{}/v1/transformer-processing/x?project={A}", host.address))
        .header("authorization", "Bearer wrong")
        .send(transformer("T1").as_slice())
        .expect("answered");
    assert_eq!(denied.status().as_u16(), 401);
    assert!(!host.database().exists(), "an unauthenticated call created no queue");
}

// ─────────────────────────────────────────────────────────────────────────
// Unproven here — each says in its own name what is missing
// ─────────────────────────────────────────────────────────────────────────

#[test]
#[ignore = "needs a live Canary identity: there is none on this box"]
fn unproven_without_a_live_canary_identity_publication_receipts_carry_their_project() {
    // §3.1's tail and acceptance item 2's "publication receipts": a receipt is
    // minted by the gateway when a completed Solar result is transferred, and
    // this box has no signed-in Canary account to mint one under. Everything
    // up to the transfer — admission, the sealed project, execution, the
    // durable result and its per-project visibility — is proven above.
    unimplemented!("run against a signed-in Canary Server");
}

#[test]
#[ignore = "needs a live Canary identity: there is none on this box"]
fn unproven_without_a_live_canary_identity_a_revoked_projects_publication_refuses_at_the_gateway() {
    // §3.6's tail: revocation stopping a publication is a gateway answer on an
    // effect, not a Server decision, so it cannot be observed without one.
    // The queued-side half — the job fails `membership_revoked` and no other
    // job is touched — is proven in `item6_…` above.
    unimplemented!("run against a signed-in Canary Server");
}

#[test]
#[ignore = "needs a live Canary identity: there is none on this box"]
fn unproven_without_a_live_canary_identity_the_saved_selection_defaults_a_call_with_no_project() {
    // §3.2's head: `ds auth project use` writes the selection into the native
    // protected context, and the client reads it back through
    // `probe_headless_identity`, which on a machine with no packaged native
    // client profile refuses `native_profile_not_configured`. What the Server
    // does with the value once it arrives IS proven above, because the client
    // always sends it as a named project.
    unimplemented!("run against a signed-in Canary Server");
}

#[test]
#[ignore = "needs a live Canary identity: there is none on this box"]
fn unproven_without_a_live_canary_identity_activity_projects_live_sync_center_state() {
    // `/v1/activity`'s per-project envelope needs a gateway session per
    // project. Its scope selection and its pre-startup refusal are proven in
    // `activity_scope_is_one_entry_per_project_that_holds_work`.
    unimplemented!("run against a signed-in Canary Server");
}

// ── the one command declaration this crate does not own yet ─────────────
//
// `ds map layer hide --target server` is the operation; `ds-cli-map` has not
// been given `SERVER_TARGET_ARGS` yet, so the declaration a caller will type
// does not exist to parse against. This is that declaration, stated here so
// the transport is proven against the arguments it is meant to receive, and
// reported as the wiring this slice still owes.
static LAYER_HIDE: Command = Command {
    id: "map.layer.hide",
    path: &["map", "layer", "hide"],
    contract: 1,
    summary: "Hide one canonical layer family.",
    purpose: "The layer drawer's hide, executed against the targeted host.",
    chapter: Chapter::MapPresentation,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("state-dir", "<absolute-path>", "Protected server state directory."),
        Arg::value("lane", "<stable|canary>", "Native authentication lane.")
            .default("stable")
            .choices(&["stable", "canary"]),
        Arg::value("project", "<exact-id>", "Exact ds_project id this call is about."),
        Arg::repeated("layer", "<canonical-id>", "Canonical layer id from the catalogue."),
    ],
    output: "The changed families and this host's remembered visibility.",
    examples: &[],
    refusals: &[],
    reference: None,
    availability: || Availability::Available,
};
