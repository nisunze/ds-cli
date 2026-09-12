//! Project and instance isolation on ONE Server, proven end to end.
//!
//! Normative source: `ds-web/docs/project-and-instance-isolation-contract.md`
//! (§"One verified context per operation", §"Server jobs and multiple
//! clients", §"Concurrency and capacity", acceptance items 2, 5, 6, 7, 8) and
//! the slice contract's §3, whose eight numbered proofs each have a test named
//! `item<N>_…` below, plus the standing rulings of 2026-09-11 (one command id
//! per operation, whichever host runs it) and 2026-09-12 §0c (the Server IS
//! the desktop's core, offline-first, one owner).
//!
//! Every assertion here is made at the authoritative boundary: a real loopback
//! listener the tests speak HTTP to, the real `ds` executable where the claim
//! is about what an operator types, and the durable SQLite queue on disk. No
//! route is stubbed, no decision is mocked, and nothing is checked by reading
//! a source string. There is no gateway anywhere and no project directory of
//! any kind — the harness constructs none because the Server holds none — and
//! the harness COUNTS how often either was asked for, so
//! `admission_and_execution_need_no_upstream_at_all` states the offline-first
//! claim as a measurement rather than an inference. Entitlement is the
//! gateway's answer at publication and sync, and the places that answer would
//! be observed are named as unproven below.
//!
//! Three callers are used deliberately:
//!   * `host.raw(…)` is the exact wire, byte for byte. Non-disclosure is a
//!     property of those bytes, so the equality proofs use only these.
//!   * `ds_cli_server::{submit, status, cancel, result, …}` is the `ds`
//!     client's own transport, reading the protected `connection.json`,
//!     sending the project on every call and re-raising the Server's typed
//!     refusal.
//!   * `host.ds([…])` is the REAL `ds` executable, dispatched through its own
//!     declarations. The standing ruling's claim — one command id, the same
//!     answer, whichever host executes it — is about what an operator types,
//!     so it is proven by typing it.
//!
//! Authorization is not one of the fixtures. The proofs that make the
//! offline-first claim about WHO OWNS THIS HOST run the real
//! `ds server serve` as its own process, over a machine holding a real
//! protected device credential, with the network cut by an `LD_PRELOAD` shim
//! — see the section "The SHIPPED Server, offline". The harness's `Allow`
//! authorizer survives only in the proofs that are about something else (the
//! store, the document source, the door), never in one that claims a Server
//! works without an upstream. `the_whole_proof_holds_again_with_the_network_cut`
//! then re-runs every test in this file on a machine that cannot reach the
//! network at all.
//!
//! What is NOT proven here, and why, is stated in the `unproven_…` tests at
//! the bottom: each is `#[ignore]`d with its reason in its own name. Solar
//! EXECUTION on the Server and report export on the Server are among them.

mod fixtures;

use ds_cli_contract::outcome::ExitClass;
use ds_command_kernel::compute_jobs::{Job, Phase};
use ds_command_kernel::execution_context::{ExecutionContext, MAX_PROJECT_CHARS};
use ds_compute_runtime as runtime;
use ds_layer_ops::{ListRequest, Preferences};
use fixtures::*;
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

use ds_cli_server::{CANCEL, RESULT, SERVE, SOLAR_SUBMIT, STATUS, SUBMIT};

/// A 64-character digest that is a perfectly well-formed job id and has never
/// named a job.
fn guessed() -> String {
    "0".repeat(64)
}

/// The layer drawer's one request body, in the shape `ds map layer hide`
/// sends whichever host runs it.
const HIDE_POLES: &[u8] = br#"{"layers":["survey/poles"],"visible":false}"#;

/// Is this canonical family remembered visible in the catalogue answered here?
fn any_visible(catalogue: &Value, family: &str) -> bool {
    catalogue["layers"]
        .as_array()
        .expect("a catalogue")
        .iter()
        .find(|layer| layer["id"] == family)
        .unwrap_or_else(|| panic!("the catalogue holds {family}: {catalogue}"))["visibility"]
        ["any_visible"]
        .as_bool()
        .expect("a folded visibility")
}

/// One job's phase, straight off the wire.
fn phase(host: &Host, id: &str, project: &str) -> String {
    host.raw("GET", &format!("/v1/jobs/{id}?project={project}"), None)
        .json()["job"]["phase"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

/// Every canonical family in one catalogue answer.
fn families(catalogue: &Value) -> Vec<String> {
    catalogue["layers"]
        .as_array()
        .expect("a catalogue")
        .iter()
        .map(|layer| layer["id"].as_str().unwrap_or_default().to_owned())
        .collect()
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
    let host = Host::start(limits());

    // Solar for A. The project is not named at all: the sealed envelope names
    // its own, and that name is authoritative. The envelope is a workspace
    // file and the route takes its PATH — the Server reads this machine's
    // filesystem exactly as the desktop does.
    let solar = host.raw(
        "POST",
        "/v1/solar-processing/solar-a",
        Some(&host.sealed_at("solar-a.json", &solar_submission())),
    );
    assert_eq!(solar.status, 202, "{:?}", solar.json());
    let solar_context = solar.json()["job"]["context"].clone();
    assert_eq!(solar_context["project"], A);
    assert_eq!(solar_context["operation"], "solar_processing");
    assert_eq!(solar_context["principal_uid"], UID);
    assert_eq!(solar_context["lane"], LANE);
    assert_eq!(solar_context["deployment"], DEPLOYMENT);
    let solar_id = solar.json()["job"]["id"].as_str().unwrap().to_owned();

    // What the Server digested is what it READ, not the path it was handed:
    // the same envelope at a second path is the same job, not a second one.
    let elsewhere = host.raw(
        "POST",
        "/v1/solar-processing/solar-a",
        Some(&host.sealed_at("solar-a-again.json", &solar_submission())),
    );
    assert_eq!(elsewhere.status, 202, "{:?}", elsewhere.json());
    assert_eq!(elsewhere.json()["job"]["id"], json!(solar_id));
    // And a path is a path on THIS machine: the client and the host share a
    // filesystem but not a working directory, so a relative one is refused
    // rather than resolved against wherever the host happens to run.
    let relative = host.raw(
        "POST",
        &format!("/v1/solar-processing/solar-rel?project={A}"),
        Some(br#"{"input_path":"prepared.json"}"#),
    );
    assert_eq!(relative.status, 400, "{:?}", relative.json());
    assert!(
        relative.stringify().contains("absolute"),
        "{}",
        relative.stringify()
    );

    // A transformer batch for B, through the real `ds server submit`.
    let input = host.input("batch-b.json", &transformer("T-B"));
    let queued = ds_cli_server::submit(
        &host.args(
            &SUBMIT,
            &["--key", "batch-b", "--input", &input, "--project", B],
        ),
        &context(),
    )
    .expect("B's batch is admitted");
    assert_eq!(queued["job"]["context"]["project"], B);
    assert_eq!(
        queued["job"]["context"]["operation"],
        "transformer_processing"
    );
    let batch_id = queued["job"]["id"].as_str().unwrap().to_owned();
    assert_ne!(solar_id, batch_id);

    // A layer write for B — the third operation, in the third shape, on the
    // same host, admitted under its own context and served out of B's own
    // document.
    let hidden = host.raw(
        "POST",
        &format!("/v1/layers/visibility?project={B}"),
        Some(HIDE_POLES),
    );
    assert_eq!(hidden.status, 200, "{:?}", hidden.json());
    assert_eq!(hidden.json()["project"], B);
    assert_eq!(hidden.json()["persisted"], "native_local");

    // Two distinct contexts, from the store on disk rather than from an answer.
    let rows = host.stored(None);
    assert_eq!(rows.len(), 2);
    let projects: Vec<String> = rows
        .iter()
        .map(|job| {
            job.context
                .as_ref()
                .expect("every row is about a project")
                .project
                .clone()
        })
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

    // A project that holds no work answers an empty list, whether the owner
    // has ever named it or not: the Server has no directory that could make
    // the two differ, so an empty list is never a disclosure.
    for project in [C, OUTSIDE] {
        let empty = host.raw("GET", &format!("/v1/jobs?project={project}"), None);
        assert_eq!(empty.status, 200);
        assert_eq!(empty.json(), json!({"jobs": [], "more": false}));
    }

    // The layer write landed under B's preference scope, in B's own
    // catalogue, and under no other.
    let listed = host.raw("GET", &format!("/v1/layers?project={B}&limit=100"), None);
    assert_eq!(listed.status, 200, "{:?}", listed.json());
    assert_eq!(listed.json()["project"], B);
    assert!(
        families(&listed.json()).contains(&only_in(B).to_owned()),
        "B's own catalogue, not another project's: {:?}",
        families(&listed.json())
    );
    assert!(!any_visible(&listed.json(), "survey/poles"));
    // The source was opened for B and for nothing else: a Server that read a
    // selection of its own would have opened something here.
    assert_eq!(host.layers.opened(), vec![B.to_owned(), B.to_owned()]);
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
    let host = Host::start(limits());
    let mut queued = Vec::new();
    for (key, project, name) in [("a1", A, "T-A"), ("b1", B, "T-B")] {
        let input = host.input(&format!("{key}.json"), &transformer(name));
        let value = ds_cli_server::submit(
            &host.args(
                &SUBMIT,
                &["--key", key, "--input", &input, "--project", project],
            ),
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
    // Nor does it repair a name outside the kernel's bound into one.
    let padded = host.raw(
        "POST",
        "/v1/transformer-processing/padded?project=%20padded",
        Some(&transformer("T-X")),
    );
    assert_eq!(padded.status, 400, "{:?}", padded.json());
    assert_eq!(padded.code(), "context_corrupt");
    // A project named for the very first time is admitted on the owner's
    // word: nothing was fetched to allow C above, and nothing is fetched now.
    // `OUTSIDE` is a project this account cannot even read a layer document
    // for, and the compute door still admits it — admission is the owner's
    // word, and entitlement is the gateway's answer where an effect leaves.
    let first = host.raw(
        "POST",
        &format!("/v1/transformer-processing/first?project={OUTSIDE}"),
        Some(&transformer("T-X")),
    );
    assert_eq!(first.status, 202, "{:?}", first.json());
    assert_eq!(first.json()["job"]["context"]["project"], OUTSIDE);

    // A sealed Solar input prepared for A, submitted with --project C: the
    // bytes outrank the query, and the caller is told which is wrong.
    let sealed = host.input("solar.json", &solar_submission());
    let refused = ds_cli_server::solar_submit(
        &host.args(
            &SOLAR_SUBMIT,
            &["--key", "solar-c", "--input", &sealed, "--project", C],
        ),
        &context(),
    )
    .expect_err("the sealed project wins");
    assert_eq!(refused.code(), "scope_mismatch");
    assert_eq!(refused.class(), ExitClass::Conflict);
    assert!(refused.remedy_text().is_some());
    // …and the same envelope named honestly is admitted under A.
    let admitted = ds_cli_server::solar_submit(
        &host.args(
            &SOLAR_SUBMIT,
            &["--key", "solar-a", "--input", &sealed, "--project", A],
        ),
        &context(),
    )
    .expect("the sealed project, named");
    assert_eq!(admitted["job"]["context"]["project"], A);

    // Five admitted jobs, four projects, and the refusals queued nothing.
    assert_eq!(host.stored(None).len(), 5);
    assert_eq!(host.stored(Some(C)).len(), 1);
    assert_eq!(host.stored(Some(OUTSIDE)).len(), 1);
    // Not one of those calls opened a document source or asked for a gateway
    // session: naming a project is not a lookup.
    assert!(host.layers.opened().is_empty());
    assert_eq!(host.gateway.asked(), 0);
}

// ─────────────────────────────────────────────────────────────────────────
// §3.3  One answer for every kind of "not yours"
// ─────────────────────────────────────────────────────────────────────────

/// A foreign principal, a foreign lane, the wrong project and an id that never
/// existed get the SAME bytes — status, class, code, sentence and remedy — on
/// status, cancel and result alike. Anything else is a disclosure.
#[test]
fn item3_a_foreign_principal_lane_project_and_a_guessed_id_are_one_byte_identical_answer() {
    let mut host = Host::start(limits());
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
    // its identity really derives, as production does. (Two accounts never
    // share one Server process — the model is one owner per Server — so this
    // is two Servers on one machine over one queue, which is the only way the
    // question can be asked at all.)
    let stranger = host.shadow(
        "uid-somebody-else",
        LANE,
        &owner_digest("uid-somebody-else", LANE),
    );
    let other_lane = host.shadow(UID, "canary", &owner_digest(UID, "canary"));
    // And one more that deliberately presents THIS Server's owner digest, so
    // the durable SQL fence separates nothing and only the execution context
    // is left to do it. Production never produces this — the owner digest is
    // derived from the uid — which is exactly why it is worth asking.
    let unfenced = host.shadow("uid-somebody-else", LANE, OWNER);

    for (route, method) in [
        ("/v1/jobs/{}", "GET"),
        ("/v1/jobs/{}/cancel", "POST"),
        ("/v1/jobs/{}/result", "GET"),
    ] {
        let own = |target: &str, project: &str| route.replace("{}", target) + "?project=" + project;
        let answers = [
            ("wrong project", host.raw(method, &own(&id, B), None)),
            ("guessed id", host.raw(method, &own(&unknown, B), None)),
            (
                "guessed id, unnarrowed",
                host.raw(method, &route.replace("{}", &unknown), None),
            ),
            (
                "foreign principal",
                stranger.raw(method, &own(&id, A), None),
            ),
            ("foreign lane", other_lane.raw(method, &own(&id, A), None)),
            (
                "foreign principal, shared durable fence",
                unfenced.raw(method, &own(&id, A), None),
            ),
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
    assert_eq!(
        theirs.json()["job"]["context"]["principal_uid"],
        "uid-somebody-else"
    );
    let their_id = theirs.json()["job"]["id"].as_str().unwrap().to_owned();
    assert_ne!(
        their_id, id,
        "the same key under another account is other work"
    );
    let listed = |answer: Value| -> Vec<String> {
        answer["jobs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|job| job["id"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        listed(host.raw("GET", "/v1/jobs", None).json()),
        vec![id.clone()]
    );
    assert_eq!(
        listed(stranger.raw("GET", "/v1/jobs", None).json()),
        vec![their_id]
    );

    // Take the durable fence away and the same key DOES land on this
    // connection's row. It is still not handed over: the row itself refuses a
    // context whose principal is not the one that admitted it, so a store that
    // ever stopped deriving its owner from the uid could still not reuse or
    // overwrite another account's result. The answer is the row's own typed
    // statement — a key that already names other work — relayed by name and
    // never a host fault, and it says nothing about whose work that is.
    let borrowed = unfenced.raw(
        "POST",
        &format!("/v1/transformer-processing/a1?project={A}"),
        Some(&transformer("T-A")),
    );
    assert_eq!(borrowed.status, 409, "{:?}", borrowed.json());
    assert_eq!(
        borrowed.code(),
        "scope_mismatch_for_key",
        "{:?}",
        borrowed.json()
    );
    assert!(
        !String::from_utf8_lossy(&borrowed.body).contains(UID),
        "the refusal must not name the principal that holds the key: {:?}",
        borrowed.json()
    );

    // On disk, after everything above: the original row, untouched, still
    // about A, still queued, still under the principal that admitted it.
    let mine = host
        .stored(Some(A))
        .into_iter()
        .find(|job| job.id == id)
        .expect("the row is still there");
    assert_eq!(mine.phase, Phase::Queued);
    let stored = mine.context.expect("its context");
    assert_eq!(stored.project, A);
    assert_eq!(stored.principal_uid, UID);
    assert_eq!(stored.idempotency_key, "a1");
}

/// One Server is signed in as exactly one owner, and its owner-only loopback
/// bearer is that owner's. There is no header that names an account and no
/// second identity this process can have, so the whole rule is the bearer: a
/// different one is 401, and the refusal names nobody. Many users are many
/// machines, which is the deployment model and not a gap.
#[test]
fn a_different_bearer_is_denied_without_naming_a_principal() {
    let host = Host::start(limits());
    let stranger_token = "f".repeat(64);

    // A well-formed bearer that is not this Server's owner's, on a read and on
    // a write alike.
    for (method, path, body) in [
        ("GET", "/v1/jobs".to_owned(), None),
        (
            "POST",
            format!("/v1/transformer-processing/x?project={A}"),
            Some(transformer("T1")),
        ),
        (
            "POST",
            format!("/v1/layers/visibility?project={A}"),
            Some(HIDE_POLES.to_vec()),
        ),
    ] {
        let denied = host.as_bearer(&stranger_token, method, &path, body.as_deref());
        assert_eq!(denied.status, 401, "{method} {path}: {:?}", denied.json());
        // The whole body. It names no account, no uid, no principal and no
        // project — there is nothing here for a caller to learn.
        assert_eq!(denied.json(), json!({"error": "server access denied"}));
        for secret in [UID, OWNER, DEPLOYMENT, A] {
            assert!(
                !String::from_utf8_lossy(&denied.body).contains(secret),
                "a denial discloses nothing: {}",
                denied.stringify()
            );
        }
    }

    // And the header a client used to send to name an account is not read at
    // all: with the owner's own bearer the answer is byte-identical with and
    // without it, so there is no second identity to claim.
    let plain = host.raw("GET", "/v1/jobs", None);
    let with_header = host.raw_with(
        "GET",
        "/v1/jobs",
        None,
        &[("x-ds-principal", "uid-somebody-else")],
    );
    assert_eq!(plain.status, 200, "{:?}", plain.json());
    assert_eq!(with_header, plain);
}

// ─────────────────────────────────────────────────────────────────────────
// §3.4  A key is one handle on one piece of work, inside its project
// ─────────────────────────────────────────────────────────────────────────

/// Inside one project a key is the caller's single handle: the same bytes are
/// the same job, other bytes are a named refusal, and the same key used for a
/// second operation is a named refusal too — never a reused result and never a
/// silent overwrite. Across projects it is not a collision at all, which
/// `equal_keys_in_two_projects_are_two_jobs` proves next.
#[test]
fn item4_a_reused_key_is_one_handle_on_one_piece_of_work_inside_its_project() {
    let host = Host::start(limits());
    let same = host.input("same.json", &transformer("T1"));
    let other = host.input("other.json", &transformer("T2"));
    let submit = |key: &str, input: &str, project: &str| {
        ds_cli_server::submit(
            &host.args(
                &SUBMIT,
                &["--key", key, "--input", input, "--project", project],
            ),
            &context(),
        )
    };

    let first = submit("shared", &same, A).expect("admitted");
    let id = first["job"]["id"].as_str().unwrap().to_owned();
    // An idempotent resubmit: the stored job, not a second one.
    let again = submit("shared", &same, A).expect("the same job");
    assert_eq!(again["job"]["id"], id);
    assert_eq!(again["job"]["created_at_ms"], first["job"]["created_at_ms"]);

    // Other bytes under the same key in the same project.
    let changed = submit("shared", &other, A).expect_err("a key may not change bytes");
    assert_eq!(changed.code(), "payload_changed_for_key");
    assert_eq!(changed.class(), ExitClass::Conflict);

    // A second OPERATION under the same key in the same project: the sealed
    // Solar envelope names A, so it derives the very id the transformer batch
    // holds, and the kernel refuses rather than adopting the row.
    let moved = host.raw(
        "POST",
        &format!("/v1/solar-processing/shared?project={A}"),
        Some(&host.sealed_at("sealed.json", &solar_submission())),
    );
    assert_eq!(moved.status, 409, "{:?}", moved.json());
    assert_eq!(moved.code(), "scope_mismatch_for_key");
    assert!(
        moved.json()["remedy"]
            .as_str()
            .expect("a remedy")
            .contains("--key"),
        "{:?}",
        moved.json()
    );

    // One row, its original bytes, its original context, its original
    // operation.
    let rows = host.stored(None);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, id);
    assert_eq!(rows[0].input_sha256, runtime::digest(&transformer("T1")));
    let stored = rows[0].context.as_ref().expect("its context");
    assert_eq!(stored.project, A);
    assert_eq!(stored.idempotency_key, "shared");
    assert_eq!(stored.operation, "transformer_processing");
}

/// The owner's daily key, in two of their projects, is TWO pieces of work.
///
/// The durable id digests (owner, lane, project, key), so equal keys in A and
/// B never meet: neither refuses the other, neither adopts the other's row,
/// neither is reachable under the other's project, and inside each project the
/// key is still that project's one idempotent handle.
#[test]
fn equal_keys_in_two_projects_are_two_jobs() {
    let host = Host::start(limits());
    let input = host.input("daily.json", &transformer("T-DAILY"));
    let submit = |project: &str| {
        ds_cli_server::submit(
            &host.args(
                &SUBMIT,
                &["--key", "daily", "--input", &input, "--project", project],
            ),
            &context(),
        )
        .unwrap_or_else(|error| panic!("`daily` is admitted in {project}: {error:?}"))
    };

    let mut ids = Vec::new();
    for project in [A, B, C] {
        let value = submit(project);
        let id = value["job"]["id"].as_str().unwrap().to_owned();
        assert_eq!(value["job"]["context"]["project"], project);
        assert_eq!(value["job"]["context"]["idempotency_key"], "daily");
        // The id is the kernel's own derivation, project included: this is the
        // rule, not an accident of ordering.
        assert_eq!(id, runtime::job_id(OWNER, LANE, project, "daily"));
        assert!(!ids.contains(&id), "one key, three projects, three ids");
        ids.push(id);
    }
    assert_eq!(host.stored(None).len(), 3);
    let rows = host.stored(None);
    assert!(
        rows.iter()
            .all(|row| row.input_sha256 == rows[0].input_sha256),
        "the very same bytes, three times"
    );

    // Inside a project the key is still one handle: the same key and bytes in
    // A answer A's row, unchanged, and never B's.
    let repeat = submit(A);
    assert_eq!(repeat["job"]["id"], ids[0]);
    assert_eq!(host.stored(None).len(), 3, "no fourth row");

    // Each is reachable only under its own project, and a cancellation in one
    // is not a cancellation in another.
    for (index, project) in [A, B, C].iter().enumerate() {
        for other in [A, B, C, OUTSIDE].iter().filter(|name| *name != project) {
            let hidden = host.raw(
                "GET",
                &format!("/v1/jobs/{}?project={other}", ids[index]),
                None,
            );
            assert_eq!(hidden.status, 409);
            assert_eq!(hidden.json()["error"], "job not found");
        }
    }
    let cancelled = ds_cli_server::cancel(
        &host.args(&CANCEL, &["--job", &ids[0], "--project", A]),
        &context(),
    )
    .expect("A's own job");
    assert_eq!(cancelled["job"]["phase"], "cancelled");
    for (index, project) in [(1, B), (2, C)] {
        let untouched = host.raw(
            "GET",
            &format!("/v1/jobs/{}?project={project}", ids[index]),
            None,
        );
        assert_eq!(untouched.json()["job"]["phase"], "queued");
    }
}

// ─────────────────────────────────────────────────────────────────────────
// §3.5  Restart, and rows a released Server wrote
// ─────────────────────────────────────────────────────────────────────────

/// A Server restarted over a durable queue keeps every job in the project it
/// was admitted into, gives a context to the row written before contexts
/// existed that names its own project — Solar's, from its own sealed bytes —
/// leaves the one that names none alone, and never runs the same key twice.
#[test]
fn item5_a_restart_recovers_every_context_including_rows_a_released_server_wrote() {
    let mut host = Host::start_over(limits(), &legacy_queue_fixture());
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
    host.restart();
    // `serve` starts its workers, and starting them is when a released
    // Server's rows get whatever context they can honestly be given — before
    // any worker can claim one. The pool is started with the device paused so
    // the recovery is observed on its own, with nothing executed.
    let workers = host.workers(Arc::new(Paused), 1);
    workers.stop();
    drop(workers);

    // A second recovery pass rebuilds nothing: the Solar row is stored now,
    // which is what proves the first pass ran, and the transformer row is
    // named again rather than quietly adopted.
    let again = runtime::recover_contexts(&host.database(), &host.identity).expect("a second pass");
    assert_eq!(again.stored, 2, "the recovered Solar row and the live one");
    assert_eq!(again.from_sealed_input, 0);
    assert_eq!(again.unrecoverable, vec![legacy_transformer_id()]);

    // The Solar row took its project from its own sealed input …
    let solar = host.raw(
        "GET",
        &format!("/v1/jobs/{}?project={A}", legacy_solar_id()),
        None,
    );
    assert_eq!(solar.status, 200, "{:?}", solar.json());
    assert_eq!(solar.json()["job"]["context"]["project"], A);
    assert_eq!(
        solar.json()["job"]["context"]["operation"],
        "solar_processing"
    );
    // … the transformer row, which carries no project by design, took none:
    // it is readable to its own connection and belongs to no project at all,
    // including the one an operator happens to have selected.
    for project in [A, B, C, OUTSIDE] {
        let hidden = host.raw(
            "GET",
            &format!("/v1/jobs/{}?project={project}", legacy_transformer_id()),
            None,
        );
        assert_eq!(hidden.status, 409, "{:?}", hidden.json());
        assert_eq!(hidden.json()["error"], "job not found");
    }
    let nameless = host.raw(
        "GET",
        &format!("/v1/jobs/{}", legacy_transformer_id()),
        None,
    );
    assert_eq!(nameless.status, 200, "{:?}", nameless.json());
    assert!(nameless.json()["job"]["context"].is_null());
    // … and the row this build admitted kept the project it was admitted into.
    let survivor = host.raw("GET", &format!("/v1/jobs/{live}?project={B}"), None);
    assert_eq!(survivor.status, 200, "{:?}", survivor.json());
    assert_eq!(survivor.json()["job"]["context"]["project"], B);

    // Each recovered or admitted row is reachable only under its own project.
    for (id, wrong) in [(legacy_solar_id(), B), (live.clone(), A)] {
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
    assert_eq!(
        host.stored(None).len(),
        3,
        "no duplicate row after a restart"
    );
}

/// A transformer row a released Server wrote names no project, and nothing
/// outside its own bytes may name one for it — not this machine's saved
/// selection, not the operator's last call, not the project the recovery
/// happens to run under. There is no argument for one: `recover_contexts`
/// takes the queue and the connection identity and nothing else.
///
/// So the row stays exactly as it is: readable to its own connection, absent
/// from every project, never claimed, never executed, never published. What
/// the owner gets instead is one sentence and one remedy, and the remedy works.
#[test]
fn a_legacy_transformer_row_is_readable_but_never_recovered_into_the_saved_selection() {
    let host = Host::start_over(limits(), &legacy_queue_fixture());
    let recovery =
        runtime::recover_contexts(&host.database(), &host.identity).expect("recovery runs");
    assert_eq!(recovery.from_sealed_input, 1, "Solar names its own project");
    assert_eq!(recovery.stored, 0, "neither row carried a context");
    assert_eq!(
        recovery.unrecoverable,
        vec![legacy_transformer_id()],
        "no project is invented for a row whose bytes name none"
    );

    // It is still the operator's own work: visible unnarrowed, absent from
    // every project, and it holds no project's capacity.
    assert_eq!(host.stored(None).len(), 2);
    for project in [A, B, C, OUTSIDE] {
        assert!(
            host.stored(Some(project))
                .iter()
                .all(|job| job.id != legacy_transformer_id()),
            "the nameless row is not in {project}"
        );
    }
    let readable = host.raw(
        "GET",
        &format!("/v1/jobs/{}", legacy_transformer_id()),
        None,
    );
    assert_eq!(readable.status, 200, "{:?}", readable.json());
    assert_eq!(readable.json()["job"]["phase"], "queued");
    assert!(readable.json()["job"]["context"].is_null());

    // And it is never claimed. Asked directly of the durable queue — the same
    // call a worker makes — the recovered Solar row is claimable and the
    // nameless one is passed over, whatever the caller does.
    let mut store = host.store();
    let mut claimed = Vec::new();
    for attempt in 0..3 {
        let taken = store
            .claim_job(
                &host.identity.caller(None),
                &format!("worker-{attempt}"),
                runtime::now_ms(),
                60_000,
                limits(),
            )
            .expect("the queue answers");
        match taken {
            Some((job, _)) => claimed.push(job.id),
            None => break,
        }
    }
    assert_eq!(
        claimed,
        vec![legacy_solar_id()],
        "only the row that names its own project is ever claimed"
    );

    // Deliberately resubmitting its exact bytes under its released id is the
    // one door left to that row, and it is a named refusal with the one
    // remedy sentence — not an adoption into whatever project asked.
    let caller = host.identity.caller(None);
    let row = store
        .job(&caller, &legacy_transformer_id())
        .expect("read")
        .expect("the nameless row");
    let bytes = store
        .job_input(&caller, &legacy_transformer_id())
        .expect("read")
        .expect("its bytes");
    let adopted = Job {
        phase: Phase::Queued,
        attempts: 0,
        worker: None,
        lease_until_ms: 0,
        result_sha256: None,
        error: None,
        context: Some(ExecutionContext {
            principal_uid: UID.to_owned(),
            lane: LANE.to_owned(),
            deployment: DEPLOYMENT.to_owned(),
            install_id: "install-1".to_owned(),
            project: C.to_owned(),
            client: "cli:1".to_owned(),
            operation: "transformer_processing".to_owned(),
            job_id: legacy_transformer_id(),
            idempotency_key: "legacy".to_owned(),
            input_sha256: row.input_sha256.clone(),
            admitted_at_ms: runtime::now_ms(),
        }),
        ..row
    };
    let refused = store
        .submit_job(&adopted, &bytes, limits())
        .expect_err("a released row is never adopted into a project");
    let ds_sync_store::Error::Refused(refusal) = refused else {
        panic!("the row's own typed answer, not a host fault: {refused}");
    };
    assert_eq!(refusal.code, "context_unrecoverable");
    let failure = ds_cli_server::server_sync::sessions::refusal_failure(&refusal);
    assert_eq!(failure.class(), ExitClass::Conflict);
    assert_eq!(
        failure.remedy_text(),
        Some(
            "read the job's stored input with ds server input, then resubmit it under an explicit --project"
        ),
        "one sentence, identical wherever this code is raised"
    );

    // The remedy is not advice, it works: the same bytes under an explicit
    // project are new work with their own id, and the nameless row is
    // untouched beside it.
    let resubmitted = ds_cli_server::submit(
        &host.args(
            &SUBMIT,
            &[
                "--key",
                "legacy-again",
                "--input",
                &host.input("legacy-again.json", &bytes),
                "--project",
                C,
            ],
        ),
        &context(),
    )
    .expect("the remedy the refusal names");
    assert_eq!(resubmitted["job"]["context"]["project"], C);
    assert_ne!(resubmitted["job"]["id"], json!(legacy_transformer_id()));
    assert_eq!(host.stored(None).len(), 3);
    assert_eq!(host.stored(Some(C)).len(), 1);
}

// ─────────────────────────────────────────────────────────────────────────
// §3.6  Execution needs no upstream; revocation is the gateway's answer
// ─────────────────────────────────────────────────────────────────────────

/// Two projects' work runs to completion on a host that has no directory, no
/// gateway and no network. Nothing about A's job is consulted to run B's, and
/// each keeps the context it was admitted under. What a revocation stops — a
/// publication — is the gateway's answer on that effect and is named as
/// unproven below, because there is no gateway here to give it.
#[test]
fn item6_execution_needs_no_directory_and_no_upstream_and_one_project_never_touches_another() {
    let host = Host::start(limits());
    let mut ids = Vec::new();
    for (key, project, name) in [("a1", A, "T-A"), ("b1", B, "T-B")] {
        let input = host.input(&format!("{key}.json"), &transformer(name));
        let value = ds_cli_server::submit(
            &host.args(
                &SUBMIT,
                &["--key", key, "--input", &input, "--project", project],
            ),
            &context(),
        )
        .expect("admitted");
        ids.push(value["job"]["id"].as_str().unwrap().to_owned());
    }

    // A claimed job runs exactly what was admitted: there is nothing to
    // re-check, so an outage cannot fail one. Both jobs execute for real.
    let workers = host.workers(Arc::new(Allow), 2);
    let terminal = |id: &str, project: &str| -> Option<Value> {
        let answer = host.raw("GET", &format!("/v1/jobs/{id}?project={project}"), None);
        let job = answer.json()["job"].clone();
        matches!(
            job["phase"].as_str(),
            Some("failed" | "completed" | "cancelled")
        )
        .then_some(job)
    };
    assert!(
        until(60, || terminal(&ids[0], A).is_some()
            && terminal(&ids[1], B).is_some()),
        "both jobs reach a terminal phase"
    );
    workers.stop();
    drop(workers);

    for (id, project) in [(&ids[0], A), (&ids[1], B)] {
        let done = terminal(id, project).expect("terminal");
        assert_eq!(done["phase"], "completed", "{done}");
        assert_eq!(done["context"]["project"], project, "nothing re-scoped it");
        assert!(
            done["error"].is_null(),
            "no membership fault of any kind exists on this host: {done}"
        );
    }
    let out = host.state.path().join("b-result.json");
    let saved = ds_cli_server::result(
        &host.args(
            &RESULT,
            &[
                "--job",
                &ids[1],
                "--project",
                B,
                "--out",
                &out.display().to_string(),
            ],
        ),
        &context(),
    )
    .expect("B's job finished and its bytes are readable");
    assert!(saved["byte_count"].as_u64().unwrap() > 0);
    assert!(out.exists());
    // And A's result is not B's: each is readable only under its own project.
    let crossed = host.raw(
        "GET",
        &format!("/v1/jobs/{}/result?project={B}", ids[0]),
        None,
    );
    assert_eq!(crossed.status, 409);
    assert_eq!(crossed.json()["error"], "job not found");

    // On disk: two rows, each still about the project it was admitted into.
    let rows = host.stored(None);
    assert_eq!(rows.len(), 2);
    for row in rows {
        let project = if row.id == ids[0] { A } else { B };
        assert_eq!(row.context.expect("retained").project, project);
    }
    assert_eq!(host.stored(Some(A)).len(), 1);
    assert_eq!(host.stored(Some(B)).len(), 1);
}

/// Offline-first, stated as a measurement rather than an inference.
///
/// The whole local path — admit, queue, claim, execute, read the result,
/// cancel, restart, recover, list — is walked for two projects, and the two
/// upstream-shaped boundaries this harness could construct are counted the
/// whole way: NO gateway session is ever opened, and no document source is
/// ever touched. There is no online branch to take and no directory to be
/// stale, so there is nothing here that could behave differently offline.
///
/// What it deliberately does NOT prove is authorization: this host runs
/// behind the harness's `Allow`, because what is being counted here is the
/// store and the document source. The offline claim about AUTHORIZATION is
/// made where it belongs — against the production `NativeAuthorizer` in the
/// real `ds server serve` process, in
/// `the_server_starts_and_serves_with_no_gateway_and_the_real_authorizer` and
/// `losing_the_gateway_changes_no_answer` below.
#[test]
fn admission_and_execution_need_no_upstream_at_all() {
    let mut host = Host::start(limits());
    let mut ids = Vec::new();
    for (key, project, name) in [("a1", A, "T-A"), ("b1", B, "T-B"), ("b2", B, "T-B2")] {
        let input = host.input(&format!("{key}.json"), &transformer(name));
        let value = ds_cli_server::submit(
            &host.args(
                &SUBMIT,
                &["--key", key, "--input", &input, "--project", project],
            ),
            &context(),
        )
        .expect("admitted with nothing upstream");
        ids.push(value["job"]["id"].as_str().unwrap().to_owned());
    }
    // One is cancelled before it can run, so the cancel path is on this route
    // too.
    ds_cli_server::cancel(
        &host.args(&CANCEL, &["--job", &ids[2], "--project", B]),
        &context(),
    )
    .expect("cancelled");

    let workers = host.workers(Arc::new(Allow), 2);
    assert!(
        until(60, || phase(&host, &ids[0], A) == "completed"
            && phase(&host, &ids[1], B) == "completed"),
        "both ran to completion with no upstream present"
    );
    workers.stop();
    drop(workers);

    let out = host.state.path().join("a-result.json");
    ds_cli_server::result(
        &host.args(
            &RESULT,
            &[
                "--job",
                &ids[0],
                "--project",
                A,
                "--out",
                &out.display().to_string(),
            ],
        ),
        &context(),
    )
    .expect("the result is on this machine");

    // Restart and recover, still with nothing upstream.
    host.restart();
    let recovery =
        runtime::recover_contexts(&host.database(), &host.identity).expect("recovery runs");
    assert_eq!(recovery.stored, 3);
    assert!(recovery.unrecoverable.is_empty());
    assert_eq!(host.raw("GET", "/v1/jobs", None).status, 200);
    assert_eq!(phase(&host, &ids[0], A), "completed");

    // The measurement.
    assert_eq!(
        host.gateway.asked(),
        0,
        "no Sync Center session was constructed anywhere on this path"
    );
    assert!(
        host.layers.opened().is_empty(),
        "no layer document source was opened either: {:?}",
        host.layers.opened()
    );
    assert_eq!(host.layers.reads(), 0);
}

// ─────────────────────────────────────────────────────────────────────────
// §3.7  Capacity
// ─────────────────────────────────────────────────────────────────────────

/// Saturating one project bounds that project and no other, saturating the
/// host bounds everyone, both answers are typed and carry retry guidance, and
/// a cancellation gives the room straight back.
#[test]
fn item7_capacity_is_typed_bounded_fair_and_released_by_cancellation() {
    let host = Host::start(ds_command_kernel::execution_context::Limits {
        global_running: 2,
        per_project_running: 1,
        per_project_queued: 2,
        global_queued: 3,
    });
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
    let detail = refused
        .detail_value()
        .expect("machine-readable retry guidance");
    assert_eq!(detail["scope"], "project");
    assert!(detail["retry_after_ms"].as_u64().is_some_and(|ms| ms > 0));
    assert!(refused.remedy_text().unwrap().contains("retry after"));
    assert!(
        !refused.message().contains(A),
        "a capacity refusal names no project"
    );
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

/// A per-project share that could hold the whole pool is not a share: one
/// project would run every worker while another waits. `ds server serve`
/// refuses it by name, from the numbers alone — before it authenticates
/// anything, opens a queue or binds a port — and the refusal states the
/// usable range for THIS host.
///
/// Only refused shares are asked for here, deliberately: an accepted one
/// would send `serve` on to `auth::identity`, which refreshes a real
/// credential, and this proof touches no identity and no network.
#[test]
fn a_per_project_share_that_could_hold_the_whole_pool_is_refused_as_no_share() {
    let host = Host::start(limits());
    let serve = |workers: &str, share: &str| {
        ds_cli_server::serve(
            &host.args(&SERVE, &["--workers", workers, "--per-project", share]),
            &context(),
        )
    };

    // A one-worker host is the case every machine can run, so its sentence is
    // asserted whole: a share of the entire host, and a share of nothing, are
    // one answer with the usable range in it.
    let one = "--per-project must leave a worker for a second project: 1..1 on a host with 1";
    assert_eq!(
        serve("1", "2")
            .expect_err("two of one worker is the whole host")
            .message(),
        one
    );
    assert_eq!(
        serve("1", "0")
            .expect_err("a share of nothing runs nothing")
            .message(),
        one
    );

    // And on this host at its measured size, where the pool is whatever the
    // CPU and memory allow: a share equal to it is refused the same way, and
    // the range scales.
    let capacity = runtime::capacity();
    let whole_pool = serve(&capacity.to_string(), &capacity.max(2).to_string())
        .expect_err("a share that could hold the whole pool is no share");
    assert!(
        whole_pool
            .message()
            .starts_with("--per-project must leave a worker for a second project"),
        "the share is what was refused, not --workers: {}",
        whole_pool.message()
    );
    assert!(
        whole_pool
            .message()
            .contains(&format!("on a host with {capacity}")),
        "{}",
        whole_pool.message()
    );

    // Nothing was authenticated, nothing was bound and no queue was created:
    // the numbers are decided before any of that.
    assert!(!host.database().exists());

    // And the default the declaration promises: half, never fewer than one.
    assert_eq!(ds_cli_server::default_per_project(4), 2);
    assert_eq!(ds_cli_server::default_per_project(2), 1);
    assert_eq!(ds_cli_server::default_per_project(1), 1);
}

// ─────────────────────────────────────────────────────────────────────────
// §3.8  Equal names in different projects
// ─────────────────────────────────────────────────────────────────────────

/// The same transformer, the same bytes, the same digest, in two projects: two
/// jobs, two ids, two results, each reachable only under its own project.
#[test]
fn item8_identical_work_in_two_projects_never_collides() {
    let host = Host::start(limits());
    let input = host.input("daily.json", &transformer("T-SAME"));
    let mut ids = Vec::new();
    for (key, project) in [("daily-a", A), ("daily-b", B)] {
        let value = ds_cli_server::submit(
            &host.args(
                &SUBMIT,
                &["--key", key, "--input", &input, "--project", project],
            ),
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
    let workers = host.workers(Arc::new(Allow), 2);
    let done = |id: &str, project: &str| {
        host.raw("GET", &format!("/v1/jobs/{id}?project={project}"), None)
            .json()["job"]["phase"]
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
                &[
                    "--job",
                    id,
                    "--project",
                    own,
                    "--out",
                    &out.display().to_string(),
                ],
            ),
            &context(),
        )
        .expect("the owner's own result");
        assert!(saved["byte_count"].as_u64().unwrap() > 0);
        let hidden = host.raw(
            "GET",
            &format!("/v1/jobs/{id}/result?project={other}"),
            None,
        );
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
        assert_eq!(
            row.context.as_ref().unwrap().operation,
            "transformer_processing"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────
// The routes' own rules
// ─────────────────────────────────────────────────────────────────────────

/// The five answers `docs-routes.md` §2 promises a layer request, each proven
/// over the real listener: no project named, a name outside the kernel's
/// bound, a project this account cannot read, a source that answers about
/// another project, and the project the caller named. A refused layer request
/// writes nothing.
#[test]
fn a_layer_request_names_its_project_and_is_fenced_to_the_document() {
    let host = Host::start(limits());

    let unnamed = host.raw("POST", "/v1/layers/visibility", Some(HIDE_POLES));
    assert_eq!(unnamed.status, 400, "{:?}", unnamed.json());
    assert_eq!(unnamed.code(), "project_required");

    let padded = host.raw(
        "POST",
        "/v1/layers/visibility?project=%20padded",
        Some(HIDE_POLES),
    );
    assert_eq!(padded.status, 400, "{:?}", padded.json());
    assert_eq!(padded.code(), "context_corrupt");

    // A project the owner's account cannot read is the answer from where the
    // account is established — the gateway's own refusal, relayed — and never
    // something the Server decided from a directory it does not hold.
    let outside = host.raw(
        "POST",
        &format!("/v1/layers/visibility?project={OUTSIDE}"),
        Some(HIDE_POLES),
    );
    assert_eq!(outside.status, 401, "{:?}", outside.json());
    assert_eq!(outside.code(), "auth_rejected");

    // A source that answers about another project than the one it was opened
    // for stops the request, with both names in the remedy.
    host.layers.answer_about_another_project(true);
    let changed = host.raw(
        "POST",
        &format!("/v1/layers/visibility?project={B}"),
        Some(HIDE_POLES),
    );
    assert_eq!(changed.status, 409, "{:?}", changed.json());
    assert_eq!(changed.code(), "project_context_changed");
    let remedy = changed.json()["remedy"].as_str().unwrap().to_owned();
    assert!(
        remedy.contains(B) && remedy.contains("someone-elses-project"),
        "{remedy}"
    );
    host.layers.answer_about_another_project(false);

    // The project the caller named, and the account can read: applied.
    let applied = host.raw(
        "POST",
        &format!("/v1/layers/visibility?project={A}"),
        Some(HIDE_POLES),
    );
    assert_eq!(applied.status, 200, "{:?}", applied.json());
    assert_eq!(applied.json()["project"], A);

    // Nothing any refusal touched was written: A has the hide, B does not.
    let a = host.raw("GET", &format!("/v1/layers?project={A}&limit=100"), None);
    assert_eq!(a.status, 200, "{:?}", a.json());
    assert!(!any_visible(&a.json(), "survey/poles"));
    let b = host.raw("GET", &format!("/v1/layers?project={B}&limit=100"), None);
    assert_eq!(b.status, 200, "{:?}", b.json());
    assert!(
        any_visible(&b.json(), "survey/poles"),
        "the refused write under B left B alone"
    );

    // And through the real `ds`, with the Server's refusal re-raised literally
    // rather than translated: one command id, the same codes, whichever host
    // ran it.
    host.layers.answer_about_another_project(true);
    let refused = host.ds(&[
        "map",
        "layer",
        "hide",
        "--target",
        "server",
        "--project",
        B,
        "--layer",
        "survey/poles",
        "--output",
        "json",
    ]);
    assert_eq!(refused.envelope["status"], "error", "{}", refused.stdout);
    assert_eq!(refused.envelope["command"], "map.layer.hide");
    assert_eq!(
        refused.envelope["error"]["code"], "project_context_changed",
        "{}",
        refused.stdout
    );
    assert_eq!(refused.envelope["error"]["class"], "conflict");
    host.layers.answer_about_another_project(false);
    let ok = host.ds(&[
        "map",
        "layer",
        "hide",
        "--target",
        "server",
        "--project",
        A,
        "--layer",
        "survey/poles",
        "--output",
        "json",
    ]);
    assert_eq!(ok.envelope["status"], "ok", "{}{}", ok.stdout, ok.stderr);
    assert_eq!(ok.envelope["data"]["project"], A);
}

/// One running Server, one authenticated owner, two of that owner's projects
/// read side by side: the source is opened for the project the caller named,
/// each project answers with its own catalogue and its own remembered
/// visibility, and neither read moves the other. No restart, no selection, no
/// directory.
#[test]
fn a_layer_read_for_a_second_authorized_project_is_served() {
    let host = Host::start(limits());

    // Hide a family in A only.
    let hidden = host.raw(
        "POST",
        &format!("/v1/layers/visibility?project={A}"),
        Some(HIDE_POLES),
    );
    assert_eq!(hidden.status, 200, "{:?}", hidden.json());

    // A's catalogue: A's own layers, A's own hide.
    let a = host.raw("GET", &format!("/v1/layers?project={A}&limit=100"), None);
    assert_eq!(a.status, 200, "{:?}", a.json());
    assert_eq!(a.json()["project"], A);
    assert_eq!(a.json()["lane"], LANE);
    assert_eq!(a.json()["visibility_source"], "native_local");
    assert!(families(&a.json()).contains(&only_in(A).to_owned()));
    assert!(!any_visible(&a.json(), "survey/poles"));

    // B's catalogue, from the SAME running host, with no restart between:
    // B's own layers, and B's own visibility, which A's hide never touched.
    let b = host.raw("GET", &format!("/v1/layers?project={B}&limit=100"), None);
    assert_eq!(b.status, 200, "{:?}", b.json());
    assert_eq!(b.json()["project"], B);
    assert!(families(&b.json()).contains(&only_in(B).to_owned()));
    assert!(
        !families(&b.json()).contains(&only_in(A).to_owned()),
        "B was not served A's document: {:?}",
        families(&b.json())
    );
    assert!(any_visible(&b.json(), "survey/poles"));

    // A third, likewise, and then A again: still its own, still hidden.
    let c = host.raw("GET", &format!("/v1/layers?project={C}&limit=100"), None);
    assert_eq!(c.json()["project"], C);
    assert!(families(&c.json()).contains(&only_in(C).to_owned()));
    let a_again = host.raw("GET", &format!("/v1/layers?project={A}&limit=100"), None);
    assert!(!any_visible(&a_again.json(), "survey/poles"));

    // Every source this host opened was opened for the project the caller
    // named, in order, and for nothing else.
    assert_eq!(
        host.layers.opened(),
        vec![
            A.to_owned(),
            A.to_owned(),
            B.to_owned(),
            C.to_owned(),
            A.to_owned()
        ]
    );
    // And a project the account cannot read is refused where the account is
    // established, not filtered out of a list this host kept.
    let refused = host.raw("GET", &format!("/v1/layers?project={OUTSIDE}"), None);
    assert_eq!(refused.status, 401, "{:?}", refused.json());
    assert_eq!(refused.code(), "auth_rejected");
}

/// The standing ruling of 2026-09-11, typed by an operator: `ds map layer
/// list` is ONE command id, and `--target server` answers what the desktop's
/// own branch of the same command answers — same shape, same values, byte for
/// byte, with no host provenance field to except.
#[test]
fn ds_map_layer_list_target_server_answers_the_same_shape_as_the_desktop() {
    let host = Host::start(limits());
    // A remembered hide, so the answer is not the trivial default and a
    // difference in how each host folds visibility would show.
    assert_eq!(
        host.raw(
            "POST",
            &format!("/v1/layers/visibility?project={A}"),
            Some(HIDE_POLES)
        )
        .status,
        200
    );

    // The Server's answer, through the real `ds`, as an operator types it.
    let run = host.ds(&[
        "map",
        "layer",
        "list",
        "--target",
        "server",
        "--project",
        A,
        "--limit",
        "100",
        "--output",
        "json",
    ]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    assert_eq!(run.envelope["status"], "ok", "{}", run.stdout);
    assert_eq!(run.envelope["command"], "map.layer.list");
    let served = run.envelope["data"].clone();

    // The desktop's own answer to the same request: the shared owner
    // (`ds_layer_ops::list`), the same document source opened for the same
    // project, the same preference root — which is exactly what `--target
    // desktop --project <id>` reaches through `Native::for_project`.
    let mut documents = host.layers.desktop_documents(A);
    let desktop = ds_layer_ops::list(
        &mut documents,
        &Preferences::at(host.layers.preference_root()),
        &ListRequest {
            refresh: false,
            limit: Some(100),
            zoom: None,
        },
    )
    .expect("the desktop's own branch of this command");

    assert_eq!(
        served, desktop,
        "one command id, one answer: the Server's answer and the desktop's differ nowhere"
    );
    // Named explicitly, so a future field that IS host provenance has to be
    // added here on purpose rather than quietly excepted.
    assert_eq!(served["project"], A);
    assert_eq!(served["lane"], LANE);
    assert_eq!(served["visibility_source"], "native_local");
    assert!(!any_visible(&served, "survey/poles"));
    assert!(
        served["layer_count"]
            .as_u64()
            .is_some_and(|count| count > 0)
    );
}

/// The standing ruling's other half: the Server runs the same operation ids
/// the desktop runs, and where it genuinely cannot — there is no rendered map
/// here — it says which host can, by name, and never answers an empty 404.
#[test]
fn an_operation_that_needs_a_rendered_map_is_refused_by_name_over_the_wire() {
    let host = Host::start(limits());
    let refused = host.raw("POST", "/v1/map/screenshot", Some(b"{}"));
    assert_eq!(refused.status, 503, "{:?}", refused.json());
    assert_eq!(refused.code(), "needs_paired_map");
    assert!(
        refused.json()["remedy"]
            .as_str()
            .unwrap()
            .contains("--target desktop")
    );
    let unknown = host.raw("GET", "/v1/nothing", None);
    assert_eq!(unknown.status, 400);
    assert_eq!(unknown.code(), "unsupported_operation");
    assert!(
        unknown.json()["error"]
            .as_str()
            .unwrap()
            .contains("/v1/nothing")
    );
}

/// `/v1/activity` reports per-project scope: one entry per project this
/// connection has durable work in, exactly one when the caller narrows, and
/// nothing at all for a project that holds nothing it may see.
#[test]
fn activity_scope_is_one_entry_per_project_that_holds_work() {
    let host = Host::start(limits());
    for (key, project, name) in [("a1", A, "T-A"), ("b1", B, "T-B")] {
        let input = host.input(&format!("{key}.json"), &transformer(name));
        ds_cli_server::submit(
            &host.args(
                &SUBMIT,
                &["--key", key, "--input", &input, "--project", project],
            ),
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
    assert!(
        ds_cli_server::host::project_scopes(&host.app, Some(C))
            .unwrap()
            .is_empty()
    );
    assert!(
        ds_cli_server::host::project_scopes(&host.app, Some(OUTSIDE))
            .unwrap()
            .is_empty()
    );
    // The projection itself needs the Solar Sync Center the running host
    // starts, which this offline proof deliberately has none of, so the route
    // says so rather than answering an empty envelope that could be mistaken
    // for "no work" — and it says so before it asks anything of anyone.
    let answer = host.raw("GET", "/v1/activity", None);
    assert_eq!(answer.status, 409, "{:?}", answer.json());
    assert!(
        answer.stringify().contains("before server startup"),
        "the pre-startup refusal, named: {}",
        answer.stringify()
    );
    assert_eq!(
        host.gateway.asked(),
        0,
        "not even this route constructed a gateway session"
    );
}

/// A project whose work is older than the newest page of the queue is still a
/// project this Server has work in.
///
/// `/v1/activity` answers per project, and which projects it covers is read
/// from the durable rows. Reading only the newest page of them would make a
/// long-lived host — the deployment model here: a process the owner may leave
/// up indefinitely — quietly stop reporting the project it started with, and
/// "no activity" for a project that has some is the one answer this route
/// must never give. So the scope is PAGED, and this is the row that proves it:
/// the oldest job on the host, one full page behind, under its own project.
#[test]
fn activity_scope_covers_a_project_older_than_one_page_of_the_queue() {
    // A deep queue, and a share that is genuinely a share: the kernel refuses
    // limits whose per-project bound equals the pool it divides (a share that
    // could hold everything is no share), so the deep pool is 4096 and one
    // project's half of it is still far more than the thousand rows below.
    let host = Host::start(ds_command_kernel::execution_context::Limits {
        global_running: 4,
        per_project_running: 2,
        per_project_queued: 2048,
        global_queued: 4096,
    });
    let mut store = host.store();
    // A page is a thousand rows, so B's single job sits behind one full page
    // of A's. Written straight to the durable store: what is under test is
    // how a long queue is READ, and a thousand admissions through the wire
    // would be a thousand engine validations to prove nothing extra.
    let mut queue = |project: &str, created_at_ms: u64, key: &str| {
        let context = ExecutionContext {
            principal_uid: UID.to_owned(),
            lane: LANE.to_owned(),
            deployment: DEPLOYMENT.to_owned(),
            install_id: "install-1".to_owned(),
            project: project.to_owned(),
            client: "cli:1".to_owned(),
            operation: "transformer_processing".to_owned(),
            job_id: runtime::job_id(OWNER, LANE, project, key),
            idempotency_key: key.to_owned(),
            input_sha256: runtime::digest(key.as_bytes()),
            admitted_at_ms: created_at_ms,
        };
        let job = Job {
            id: context.job_id.clone(),
            owner: OWNER.to_owned(),
            lane: LANE.to_owned(),
            input_sha256: context.input_sha256.clone(),
            engine: ds_command_kernel::compute_jobs::EngineKind::FastLv,
            input_tag: "ds.fast-lv.request/v1".to_owned(),
            phase: Phase::Queued,
            attempts: 0,
            created_at_ms,
            updated_at_ms: created_at_ms,
            worker: None,
            lease_until_ms: 0,
            result_sha256: None,
            error: None,
            context: Some(context),
        };
        store
            .submit_job(&job, key.as_bytes(), host.limits)
            .expect("the durable queue takes the row");
    };
    queue(B, 1_700_000_000_000, "the-oldest-row");
    for row in 0..1_000 {
        queue(A, 1_700_000_001_000 + row, &format!("a-{row}"));
    }

    let scopes = ds_cli_server::host::project_scopes(&host.app, None).expect("the scopes");
    assert!(
        scopes.contains(&B.to_owned()),
        "B's only job is one page behind A's and its project vanished: {scopes:?}"
    );
    assert_eq!(scopes, vec![A.to_owned(), B.to_owned()]);
    // And narrowing still answers about exactly the project named.
    assert_eq!(
        ds_cli_server::host::project_scopes(&host.app, Some(B)).expect("narrowed"),
        vec![B.to_owned()]
    );
    assert!(
        ds_cli_server::host::project_scopes(&host.app, Some(C))
            .expect("narrowed")
            .is_empty()
    );
}

/// Nothing reaches the store without the owner bearer, and a revoked device
/// stops every route at the door — including the ones that would otherwise
/// create a queue file.
#[test]
fn an_unauthenticated_call_never_reads_or_creates_anything() {
    let host = Host::start(limits());
    let denied = ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
        .post(format!(
            "http://{}/v1/transformer-processing/x?project={A}",
            host.address
        ))
        .header("authorization", "Bearer wrong")
        .send(transformer("T1").as_slice())
        .expect("answered");
    assert_eq!(denied.status().as_u16(), 401);
    assert!(
        !host.database().exists(),
        "an unauthenticated call created no queue"
    );
}

// ─────────────────────────────────────────────────────────────────────────
// The SHIPPED Server, offline: the real process, the real authorizer, the
// real protected credential, and no network at all
//
// The second adversarial pass refuted offline-first for the wiring that
// actually ships. `serve` refreshed a credential through the gateway before
// it bound its port, and every route re-authorized through the same refresh,
// so a Server could not START without an upstream and answered 401 to
// everything within fifteen seconds of losing one. Only the stub `Authorizer`
// had ever been exercised offline.
//
// These proofs run the real `ds server serve` as its own process over a
// machine that holds a real protected device credential, with the network cut
// out from under it by an `LD_PRELOAD` shim. Nothing about authorization is
// injected; the only thing this file supplies is the credential a real
// `ds auth link` would have left.
// ─────────────────────────────────────────────────────────────────────────

/// The device this machine is signed in as, and the one that replaces it.
const FIRST_DEVICE: &str = "device-first";
const SECOND_DEVICE: &str = "device-second";

/// `ds-cli-server::auth::OBSERVE_INTERVAL` plus a margin. The refuted build
/// turned every route into a 401 within this window of losing its upstream,
/// so a proof that the answer does NOT move has to outlive it.
const PAST_THE_OBSERVATION_WINDOW: Duration = Duration::from_secs(18);

/// The whole shipped path, with nothing upstream in existence: the Server
/// starts, admits, queues, executes, answers and stops.
///
/// Every layer of authorization here is the production one — `serve` reads the
/// credential this machine holds, `NativeAuthorizer` re-reads it for each
/// request and each claim — and the network is cut, so none of it can reach a
/// gateway even by accident. A single named refusal, `server_owner_changed`,
/// is the only thing that may stop a route, and it appears nowhere.
#[test]
fn the_server_starts_and_serves_with_no_gateway_and_the_real_authorizer() {
    let machine = device_home::DeviceHome::linked(LANE, FIRST_DEVICE);
    let server = LiveServer::start(&machine, 2);

    // Starting at all is the first half of the refuted claim: the process got
    // past authorization and bound its port with no upstream in existence.
    assert!(
        server.said().contains("DS server ready at"),
        "{}",
        server.said()
    );

    let submitted = server.raw(
        "POST",
        &format!("/v1/transformer-processing/live-a?project={A}"),
        Some(&transformer("T-LIVE")),
    );
    assert_eq!(submitted.status, 202, "{}", submitted.stringify());
    let id = submitted.json()["job"]["id"]
        .as_str()
        .expect("an admitted job")
        .to_owned();
    assert_eq!(submitted.json()["job"]["context"]["project"], A);

    // Its own workers claim and run it — a claim re-authorizes through the
    // same production authorizer, so execution offline is the same claim as
    // admission offline.
    assert!(
        until(60, || server
            .raw("GET", &format!("/v1/jobs/{id}?project={A}"), None)
            .json()["job"]["phase"]
            == "completed"),
        "the Server never finished its own work offline: {}",
        server.said()
    );

    // Every route this host has, answered by a process with no upstream.
    for (method, path, expected) in [
        ("GET", format!("/v1/jobs?project={A}"), 200),
        ("GET", format!("/v1/jobs/{id}?project={A}"), 200),
        ("GET", format!("/v1/jobs/{id}/input?project={A}"), 200),
        ("GET", format!("/v1/jobs/{id}/result?project={A}"), 200),
        ("GET", "/v1/activity".to_owned(), 200),
        // The layer drawer is the one route that genuinely wants an upstream:
        // a project's layer document is fetched where the account is
        // established. Offline it says exactly that, retryably.
        ("GET", format!("/v1/layers?project={A}"), 503),
    ] {
        let answer = server.raw(method, &path, None);
        assert_eq!(
            answer.status,
            expected,
            "{method} {path}: {}",
            answer.stringify()
        );
        assert_ne!(
            answer.code(),
            "server_owner_changed",
            "{method} {path} was stopped by authorization with no gateway present"
        );
    }
    // And the difference matters: the drawer's 503 is the DOCUMENT source
    // saying it could not reach the gateway, retryable and about one project,
    // never this host deciding it no longer belongs to its owner.
    let layers = server.raw("GET", &format!("/v1/layers?project={A}"), None);
    assert_eq!(
        layers.code(),
        "device_auth_transient",
        "{}",
        layers.stringify()
    );
    assert_eq!(layers.json()["retryable"], true);

    // And the operator's own command, through the real binary, against the
    // real process.
    let listed = server.ds(&["server", "activity", "--project", A, "--output", "json"]);
    assert_eq!(listed.code, 0, "{} {}", listed.stdout, listed.stderr);
    assert_eq!(listed.envelope["status"], "ok", "{}", listed.stdout);

    // Cancelling work that already finished answers about the JOB, offline,
    // by name — the shape of an answer that got past the door and reached the
    // durable queue.
    let cancelled = server.raw("POST", &format!("/v1/jobs/{id}/cancel?project={A}"), None);
    assert_eq!(cancelled.status, 409, "{}", cancelled.stringify());
    assert!(
        cancelled.stringify().contains("job_already_terminal"),
        "{}",
        cancelled.stringify()
    );

    // Nothing in the whole run asked for a gateway and got one, and the
    // refresher said so rather than swallowing it.
    assert!(
        server
            .said()
            .contains("credential refresh did not reach the gateway"),
        "the refresher must attempt, fail and SAY so: {}",
        server.said()
    );
    assert!(
        server.said().contains("the host is unaffected"),
        "{}",
        server.said()
    );
}

/// Losing the gateway changes no answer — proven past the window in which the
/// refuted build turned every route into a 401.
#[test]
fn losing_the_gateway_changes_no_answer() {
    let machine = device_home::DeviceHome::linked(LANE, FIRST_DEVICE);
    let server = LiveServer::start(&machine, 1);
    let submitted = server.raw(
        "POST",
        &format!("/v1/transformer-processing/steady?project={B}"),
        Some(&transformer("T-STEADY")),
    );
    assert_eq!(submitted.status, 202, "{}", submitted.stringify());
    // Let the work finish first, so the durable row this proof compares is
    // settled: what must not move is the ANSWER, and a job still running
    // would move it by doing its job.
    let id = submitted.json()["job"]["id"]
        .as_str()
        .expect("an admitted job")
        .to_owned();
    assert!(
        until(60, || server
            .raw("GET", &format!("/v1/jobs/{id}?project={B}"), None)
            .json()["job"]["phase"]
            == "completed"),
        "{}",
        server.said()
    );

    let before = server.raw("GET", &format!("/v1/jobs?project={B}"), None);
    assert_eq!(before.status, 200, "{}", before.stringify());

    // The upstream is gone and stays gone: the refresher has already tried and
    // failed, and the shim guarantees every further attempt fails too.
    assert!(
        until(30, || server
            .said()
            .contains("credential refresh did not reach the gateway")),
        "the refresher never even tried: {}",
        server.said()
    );
    std::thread::sleep(PAST_THE_OBSERVATION_WINDOW);

    let after = server.raw("GET", &format!("/v1/jobs?project={B}"), None);
    assert_eq!(
        (before.status, before.body),
        (after.status, after.body),
        "the same question, the same answer, with no upstream on either side"
    );
    // And a call it has never seen before is still admitted, so what survived
    // is the host and not one cached answer.
    let again = server.raw(
        "POST",
        &format!("/v1/transformer-processing/steady-two?project={B}"),
        Some(&transformer("T-STEADY-2")),
    );
    assert_eq!(again.status, 202, "{}", again.stringify());
    assert_ne!(again.code(), "server_owner_changed");
}

/// The one thing that stops this host, over the wire, by its own name: this
/// machine now holds somebody else's credential.
#[test]
fn a_changed_on_disk_credential_stops_the_host_with_server_owner_changed() {
    let machine = device_home::DeviceHome::linked(LANE, FIRST_DEVICE);
    let server = LiveServer::start(&machine, 1);
    assert_eq!(server.raw("GET", "/v1/jobs", None).status, 200);
    assert_ne!(
        machine.binding(FIRST_DEVICE),
        machine.binding(SECOND_DEVICE),
        "the two devices must be two credentials for this proof to mean anything"
    );

    // A different device, in the same protected place. Nothing is told to the
    // running host; it observes its own machine.
    machine.link(SECOND_DEVICE);
    assert!(
        until(40, || server.raw("GET", "/v1/jobs", None).status == 401),
        "a changed credential on disk must stop this host: {}",
        server.said()
    );

    let stopped = server.raw("GET", "/v1/jobs", None);
    assert_eq!(stopped.code(), "server_owner_changed");
    assert_eq!(stopped.json()["class"], "unauthorized");
    assert!(
        stopped.json()["error"]
            .as_str()
            .unwrap_or_default()
            .contains("credential changed"),
        "{}",
        stopped.stringify()
    );
    assert!(
        stopped.json()["remedy"]
            .as_str()
            .unwrap_or_default()
            .contains("restart"),
        "{}",
        stopped.stringify()
    );
    // It says what happened and nothing about who: no uid, no device, no
    // account, no project.
    let said = stopped.stringify();
    for secret in [
        device_home::UID,
        device_home::EMAIL,
        SECOND_DEVICE,
        FIRST_DEVICE,
        A,
    ] {
        assert!(!said.contains(secret), "{said} names {secret}");
    }

    // Signing the machine out entirely is the same answer, not a worse one.
    machine.unlink();
    assert!(
        until(40, || server.raw("GET", "/v1/jobs", None).code()
            == "server_owner_changed"),
        "a machine with no credential holds no host: {}",
        server.said()
    );
}

// ─────────────────────────────────────────────────────────────────────────
// The request door
// ─────────────────────────────────────────────────────────────────────────

/// A full door is a typed refusal in the kernel's own vocabulary, with a scope
/// of its own — not the bare 429 with a sentence and nothing else that the
/// second pass found.
#[test]
fn the_request_door_answers_a_typed_capacity_refusal() {
    // Two places, both filled by real requests parked inside the host.
    let host = Arc::new(Host::start_with_door(limits(), 2));
    host.layers.hold();
    let holders: Vec<_> = (0..2)
        .map(|_| {
            let host = host.clone();
            std::thread::spawn(move || host.raw("GET", &format!("/v1/layers?project={A}"), None))
        })
        .collect();
    assert!(
        until(20, || host.layers.inside() == 2),
        "both requests must actually be in flight, not merely started"
    );

    let refused = host.raw("GET", "/v1/jobs", None);
    assert_eq!(refused.status, 429, "{}", refused.stringify());
    assert_eq!(refused.code(), "capacity_exhausted");
    assert_eq!(refused.json()["class"], "unavailable");
    assert_eq!(refused.json()["retryable"], true);
    // The door is its own scope: it is not the queue, and nothing a caller
    // cancels empties it, so its remedy is its own too.
    assert_eq!(refused.json()["scope"], "door");
    assert_eq!(refused.json()["retry_after_ms"], 500);
    assert!(
        refused.json()["remedy"]
            .as_str()
            .unwrap_or_default()
            .contains("500 ms"),
        "{}",
        refused.stringify()
    );
    // A capacity answer names counts, never a project or an identity.
    let said = refused.stringify();
    for secret in [A, B, UID, OWNER, host.token.as_str()] {
        assert!(!said.contains(secret), "{said} names {secret}");
    }

    // The door clears as requests finish — and the requests that were holding
    // it were answered, not dropped.
    host.layers.release();
    for holder in holders {
        let held = holder.join().expect("a held request finishes");
        assert_eq!(held.status, 200, "{}", held.stringify());
    }
    assert_eq!(host.raw("GET", "/v1/jobs", None).status, 200);
}

// ─────────────────────────────────────────────────────────────────────────
// The row a released Server left behind
// ─────────────────────────────────────────────────────────────────────────

/// The remedy on `context_unrecoverable` is now something an owner can carry
/// out: the row has no result, but it has its own request bytes, and
/// `ds server input` returns them.
#[test]
fn a_legacy_queued_row_is_readable_by_input_and_resubmittable() {
    let host = Host::start_over(limits(), &legacy_queue_fixture());
    let stranded = legacy_transformer_id();

    // The row that will never run: queued, with no project of its own, and
    // nothing at `result` to read — which is what made the old remedy
    // impossible to follow.
    let status = host.raw("GET", &format!("/v1/jobs/{stranded}"), None);
    assert_eq!(status.json()["job"]["phase"], "queued");
    let no_result = host.raw("GET", &format!("/v1/jobs/{stranded}/result"), None);
    assert_eq!(no_result.status, 409, "{}", no_result.stringify());

    // Its input, byte for byte, off the wire.
    let returned = host.raw("GET", &format!("/v1/jobs/{stranded}/input"), None);
    assert_eq!(returned.status, 200, "{}", returned.stringify());
    let stored = host
        .store()
        .job_input(&host.identity.caller(None), &stranded)
        .expect("read")
        .expect("the row's own bytes");
    assert_eq!(
        returned.body, stored,
        "the exact bytes it was admitted with"
    );

    // And through the command the remedy names, typed by an operator.
    let out = host.state.path().join("stranded-input.json");
    let saved = host.ds(&[
        "server",
        "input",
        "--job",
        &stranded,
        "--out",
        &out.display().to_string(),
        "--output",
        "json",
    ]);
    assert_eq!(saved.code, 0, "{} {}", saved.stdout, saved.stderr);
    assert_eq!(
        std::fs::read(&out).expect("the saved input"),
        stored,
        "ds server input saves the same bytes the wire returned"
    );
    assert_eq!(saved.envelope["data"]["byte_count"], stored.len());

    // The remedy carried out: the same bytes, under an explicit project, are
    // new work with their own id — and the stranded row is untouched.
    let resubmitted = ds_cli_server::submit(
        &host.args(
            &SUBMIT,
            &[
                "--key",
                "rescued",
                "--input",
                &out.display().to_string(),
                "--project",
                C,
            ],
        ),
        &context(),
    )
    .expect("the remedy the refusal names");
    assert_eq!(resubmitted["job"]["context"]["project"], C);
    assert_ne!(resubmitted["job"]["id"], json!(stranded));
    assert_eq!(host.stored(Some(C)).len(), 1);
    assert_eq!(
        host.raw("GET", &format!("/v1/jobs/{stranded}"), None)
            .json()["job"]["phase"],
        "queued",
        "reading a row's input changes nothing about it"
    );

    // The new door is fenced exactly like the old ones: another project's job
    // and an id that never existed are one answer.
    for path in [
        format!("/v1/jobs/{stranded}/input?project={A}"),
        format!("/v1/jobs/{}/input", guessed()),
        format!("/v1/jobs/{}/input?project={A}", guessed()),
    ] {
        let hidden = host.raw("GET", &path, None);
        assert_eq!(hidden.status, 409, "{path}: {}", hidden.stringify());
        assert_eq!(hidden.code(), "not_visible");
        assert_eq!(hidden.json()["error"], "job not found");
    }
    // And a bearer that is not this owner's reads nobody's input.
    let foreign = host.as_bearer(
        &"f".repeat(64),
        "GET",
        &format!("/v1/jobs/{stranded}/input"),
        None,
    );
    assert_eq!(foreign.status, 401, "{}", foreign.stringify());
}

// ─────────────────────────────────────────────────────────────────────────
// A project id is one path segment
// ─────────────────────────────────────────────────────────────────────────

/// Percent-encode a value so a query string carries it exactly as typed,
/// separators and all.
fn as_query_value(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// A name that could mean a different place on disk is refused at every door
/// it reaches, and nothing is written on the way.
#[test]
fn a_path_like_project_id_is_refused_before_anything_is_written() {
    let host = Host::start(limits());
    let mut path_like: Vec<String> = [
        "..",
        ".",
        "A/../B",
        "project-a/project-b",
        "/project-a",
        "A\\B",
        "A B",
        " project-a",
        "project-a ",
    ]
    .iter()
    .map(|value| (*value).to_string())
    .collect();
    // The bound is part of the same grammar and is the kernel's own number,
    // asked for rather than written down.
    path_like.push("p".repeat(MAX_PROJECT_CHARS + 1));

    for value in path_like.iter().map(String::as_str) {
        // At the wire, where a caller can type anything.
        let refused = host.raw(
            "POST",
            &format!(
                "/v1/transformer-processing/path-like?project={}",
                as_query_value(value)
            ),
            Some(&transformer("T-PATH")),
        );
        assert_eq!(refused.status, 400, "{value:?}: {}", refused.stringify());
        assert_eq!(refused.code(), "context_corrupt", "{value:?}");
        assert!(
            refused.json()["error"]
                .as_str()
                .unwrap_or_default()
                .contains("one path segment"),
            "{value:?} must be told the grammar, not merely refused: {}",
            refused.stringify()
        );

        // And in the client, before the value ever becomes a query at all.
        let client = ds_cli_server::submit(
            &host.args(
                &SUBMIT,
                &[
                    "--key",
                    "path-like",
                    "--input",
                    &host.input("path-like.json", &transformer("T-PATH")),
                    "--project",
                    value,
                ],
            ),
            &context(),
        )
        .expect_err("a path expression is not a project id");
        assert_eq!(client.code(), "context_corrupt", "{value:?}");
    }

    // Nothing was admitted, under any name, by any door.
    assert!(host.stored(None).is_empty());
    for project in [A, B, C, "..", "A/../B"] {
        assert!(host.stored(Some(project)).is_empty(), "{project}");
    }

    // The real binary answers the same way, and its refusal carries the one
    // remedy an operator can act on.
    let typed = host.ds(&[
        "server",
        "status",
        "--job",
        &guessed(),
        "--project",
        "A/../B",
        "--output",
        "json",
    ]);
    assert_ne!(typed.code, 0);
    assert_eq!(typed.envelope["error"]["code"], "context_corrupt");
    assert_eq!(
        typed.envelope["error"]["remedy"],
        "copy one exact ds_project value from ds auth project list"
    );

    // The names the product actually uses are untouched by the rule, and so
    // is the longest one it allows: the bound refuses what is over it, not
    // what is at it.
    let longest = "p".repeat(MAX_PROJECT_CHARS);
    for (index, ordinary) in [A, B, C, OUTSIDE, "aderm", "p", longest.as_str()]
        .iter()
        .enumerate()
    {
        let admitted = host.raw(
            "POST",
            &format!("/v1/transformer-processing/ordinary-{index}?project={ordinary}"),
            Some(&transformer("T-OK")),
        );
        assert_eq!(admitted.status, 202, "{ordinary}: {}", admitted.stringify());
        assert_eq!(admitted.json()["job"]["context"]["project"], *ordinary);
    }
}

// ─────────────────────────────────────────────────────────────────────────
// A share is smaller than the pool it divides
// ─────────────────────────────────────────────────────────────────────────

/// The invariant used to live only in the Server's flag parser, so anything
/// else that built `Limits` could hand the kernel a share that was no share.
/// It lives in the kernel now, and a host configured that way admits nothing.
#[test]
fn a_share_equal_to_the_pool_is_a_configuration_fault() {
    use ds_command_kernel::execution_context::{Fault, Intent, Limits, capacity};
    let ask = |limits: Limits| capacity(limits, Intent::Claim, &[], &[], A);

    // A share equal to its pool, and a share larger than it: one project may
    // hold the whole host while another waits, which is the thing the share
    // exists to prevent.
    for (limits, named) in [
        (
            Limits {
                global_running: 4,
                per_project_running: 4,
                per_project_queued: 8,
                global_queued: 16,
            },
            "per_project_running",
        ),
        (
            Limits {
                global_running: 4,
                per_project_running: 8,
                per_project_queued: 8,
                global_queued: 16,
            },
            "per_project_running",
        ),
        (
            Limits {
                global_running: 4,
                per_project_running: 2,
                per_project_queued: 16,
                global_queued: 16,
            },
            "per_project_queued",
        ),
    ] {
        let Err(Fault::Hard(message)) = ask(limits) else {
            panic!("{named}: a share that is not a share is a host fault, not a refusal");
        };
        assert!(message.contains(named), "{message}");
        assert!(
            message.contains("a share must be smaller than the pool it divides"),
            "{message}"
        );
    }

    // The one exception the smallest rented VM needs: a pool of one cannot be
    // divided, so its share is exactly one — and two of one worker is still
    // a fault.
    assert!(
        ask(Limits {
            global_running: 1,
            per_project_running: 1,
            per_project_queued: 1,
            global_queued: 2,
        })
        .is_ok(),
        "a single-worker host has no share to give and says so by being 1"
    );
    assert!(matches!(
        ask(Limits {
            global_running: 1,
            per_project_running: 2,
            per_project_queued: 1,
            global_queued: 2,
        }),
        Err(Fault::Hard(_))
    ));

    // And the Server's own default is a share at every size it can run at.
    for workers in [1_usize, 2, 3, 4, 8, 64] {
        let share = ds_cli_server::default_per_project(workers);
        assert!(
            ask(Limits {
                global_running: workers,
                per_project_running: share,
                per_project_queued: share * 4,
                global_queued: workers * 8,
            })
            .is_ok(),
            "the host's own default must satisfy the kernel's rule at {workers} workers"
        );
    }

    // A host that was configured that way anyway admits nothing at all: the
    // durable queue asks the kernel inside its own write transaction, so the
    // fault reaches the caller as the host's, by name, over the wire.
    let host = Host::start(Limits {
        global_running: 4,
        per_project_running: 4,
        per_project_queued: 16,
        global_queued: 16,
    });
    let refused = host.raw(
        "POST",
        &format!("/v1/transformer-processing/no-share?project={A}"),
        Some(&transformer("T-NO-SHARE")),
    );
    // A host fault, not a caller's refusal: the caller is told the host is
    // broken (502, class `failed`, not retryable) rather than that its request
    // was wrong.
    assert_eq!(refused.status, 502, "{}", refused.stringify());
    assert_eq!(refused.code(), "server_refused");
    assert_eq!(refused.json()["class"], "failed");
    assert_eq!(refused.json()["retryable"], false);
    assert!(
        refused
            .stringify()
            .contains("a share must be smaller than the pool it divides"),
        "{}",
        refused.stringify()
    );
    assert!(host.stored(None).is_empty(), "nothing was written");
}

// ─────────────────────────────────────────────────────────────────────────
// The whole proof again, on a machine with no network at all
// ─────────────────────────────────────────────────────────────────────────

/// Every test in this file, re-run in a second process that cannot reach the
/// network at all — and the same counts.
///
/// The proofs above arrange for there to be no gateway. This one removes the
/// possibility of one: the child runs with `tests/fixtures/no_network.c`
/// preloaded, so every non-loopback `connect` is `ENETUNREACH` and every
/// non-loopback name lookup is `EAI_FAIL`, for it and for every `ds` and
/// `ds server serve` it starts. Offline-first stops being a property of how
/// these tests are written and becomes a property of the machine they run on.
///
/// The counts are not written down here. The child is asked how many tests
/// this binary has (`--list`) and must account for exactly that many, with
/// none failed — so the assertion cannot rot as tests are added.
#[test]
fn the_whole_proof_holds_again_with_the_network_cut() {
    // In the child this test does not re-run the suite — it proves the cut it
    // is running under is real, so that every other test in that run means
    // what it says. Both halves are this test's own name.
    if std::env::var_os(fixtures::CUT_NETWORK_SHIM).is_some() {
        let gateway = std::net::TcpStream::connect(("fixture.ue.gateway.dev", 443));
        assert!(
            gateway.is_err(),
            "a name lookup got out of a cut network: {gateway:?}"
        );
        let elsewhere =
            std::net::TcpStream::connect((std::net::Ipv4Addr::new(203, 0, 113, 1), 443));
        assert!(
            elsewhere.is_err(),
            "a connection got out of a cut network: {elsewhere:?}"
        );
        // …and the machine itself still works, or nothing else in this run
        // would prove anything about the Server.
        assert!(std::net::TcpListener::bind("127.0.0.1:0").is_ok());
        return;
    }
    let Some(shim) = cut_network() else {
        eprintln!(
            "SKIPPED the_whole_proof_holds_again_with_the_network_cut: this machine has no C \
             compiler (cc), so the LD_PRELOAD shim could not be built and the suite proves \
             offline-first by arrangement rather than by force"
        );
        return;
    };
    let binary = std::env::current_exe().expect("this test binary");
    // Build `ds` HERE, where the network is still whole: a child that had to
    // build it might need a registry it cannot reach, and that would be this
    // harness failing rather than the Server.
    let _ = ds_binary();

    let listed = std::process::Command::new(&binary)
        .arg("--list")
        .output()
        .expect("this binary lists its own tests");
    let expected = String::from_utf8_lossy(&listed.stdout)
        .lines()
        .filter(|line| line.ends_with(": test"))
        .count();
    assert!(expected > 20, "the listing found {expected} tests");

    let again = std::process::Command::new(&binary)
        .env("LD_PRELOAD", &shim)
        .env(fixtures::CUT_NETWORK_SHIM, &shim)
        .output()
        .expect("the same tests run again");
    let transcript = format!(
        "{}{}",
        String::from_utf8_lossy(&again.stdout),
        String::from_utf8_lossy(&again.stderr)
    );
    assert!(
        again.status.success(),
        "the proof does not hold with the network cut:\n{transcript}"
    );

    let summary = transcript
        .lines()
        .find(|line| line.starts_with("test result:"))
        .unwrap_or_else(|| panic!("no summary in:\n{transcript}"))
        .to_owned();
    let count = |what: &str| -> usize {
        summary
            .split(';')
            .find(|part| part.trim_end().ends_with(what))
            .and_then(|part| {
                part.split_whitespace()
                    .rev()
                    .nth(1)
                    .and_then(|value| value.parse().ok())
            })
            .unwrap_or_else(|| panic!("no {what} count in {summary}"))
    };
    assert_eq!(count("failed"), 0, "{summary}");
    assert_eq!(
        count("passed") + count("ignored"),
        expected,
        "every test this binary has must be accounted for with the network cut: {summary}"
    );
    // That the cut was REAL is the child's own first test, above: had the
    // shim not loaded, that test would have failed and this run would not
    // have succeeded.
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
    // up to the transfer — admission, the sealed project, the durable result
    // and its per-project visibility — is proven above.
    unimplemented!("run against a signed-in Canary Server");
}

#[test]
#[ignore = "needs a live Canary identity: there is none on this box"]
fn unproven_without_a_live_canary_identity_a_solar_job_runs_to_a_published_result_on_the_server() {
    // Solar EXECUTION on the Server is NOT proven anywhere in this file. A
    // Solar job is admitted here (its sealed project, its context, its id, its
    // per-project visibility and its refusals all are), and the transformer
    // engine really runs, but a Solar run ends at a publication the gateway
    // has to accept, and there is no gateway on this box to accept it. Nothing
    // above should be read as a claim that Solar completes on a Server.
    unimplemented!("run against a signed-in Canary Server");
}

#[test]
#[ignore = "needs a live Canary identity: there is none on this box"]
fn unproven_without_a_live_canary_identity_report_export_on_the_server_writes_its_artifact() {
    // `server_reports` takes the job's context project and then needs a Sync
    // Center session for it to read the report inputs and write the artifact.
    // The scope decision is proven (`activity_scope_…`, and the routes' own
    // admission), the export itself is not. Report export on the Server
    // remains unproven here.
    unimplemented!("run against a signed-in Canary Server");
}

#[test]
#[ignore = "needs a live Canary identity: there is none on this box"]
fn unproven_without_a_live_canary_identity_a_revoked_projects_publication_refuses_at_the_gateway() {
    // §3.6: revocation is a gateway answer on an effect — a publication or a
    // sync for project A is refused there, and nothing of B's is touched
    // because B publishes through its own session. The Server makes no
    // revocation decision of its own (it holds no directory to make one
    // from), so there is nothing to observe without a gateway. What IS
    // proven above, in `item6_…` and `admission_and_execution_…`, is the other
    // half: the whole local path needs no upstream at all.
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
