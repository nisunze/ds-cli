//! Which live runtime did the operation actually reach?
//!
//! This is the CLI half of the slice's proof for the isolation contract's
//! acceptance items 1 and 4 (`ds-web/docs/project-and-instance-isolation-contract.md`):
//!
//! > 1. Run two Desktop instances on different projects plus independent
//! >    CLI/MCP clients; explicitly route commands and prove each UI, operation
//! >    and result stays with its target. Include equal project names and
//! >    same-project windows.
//! > 4. Exercise stale/ambiguous descriptors, instance restart, port reuse and
//! >    explicit missing targets. Prove **zero misrouted effects**, not just a
//! >    friendly error.
//!
//! "Zero misrouted effects" is why nothing here asserts a message and stops.
//! Every instance is a real loopback server that records what it received, so
//! each test asserts *which* fixture was asked to perform the operation — and,
//! for every refusal, that **no** fixture was asked to perform anything at all.
//! A refusal that reached an instance first is not a refusal.
//!
//! WHAT IS REAL. The descriptors are files in a real registry directory under a
//! temporary `XDG_DATA_HOME`; `ds` finds them itself. The instances are real
//! HTTP servers on real ephemeral ports; `ds` sends the pairing token itself,
//! over its own transport, and a fixture that does not receive its own token
//! answers 401 exactly as an unrelated listener would. The selection is the
//! kernel's, reached through the crate's own resolution path.
//!
//! WHAT IS NOT. `ds`'s `main` is not: a test in this crate cannot build the
//! binary that depends on it. So `fixtures::Invocation` performs the three
//! steps `crates/ds/src/registry.rs` performs around a handler — parse the
//! declared arguments with the contract's own parser, scope the headless
//! observation and the host `--target` exactly as `registry::host_target`
//! reads it, call the handler — and everything the claims are about happens
//! inside that handler. The argv path itself is proven by the one ignored test
//! at the bottom, which drives the real `ds` executable when it has been built.

mod fixtures;

use fixtures::{Bridge, Invocation, Machine, PROJECT_OP, refused, target, untouched};
use serde_json::json;

/// Three live instances. Two hold different projects whose display names are
/// identical; the third holds the first's project. That is acceptance 1's
/// "include equal project names and same-project windows" in one machine.
const ALPHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const BETA: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const GAMMA: &str = "cccccccccccccccccccccccccccccccc";
/// An instance id that names nothing running, for the explicit-missing-target
/// cases. Well-formed on purpose: the refusal must be "not live", not "not an
/// instance id".
const DEAD: &str = "dddddddddddddddddddddddddddddddd";

const SHARED_NAME: &str = "Riverside Extension";

fn three(machine: &Machine) -> (Bridge, Bridge, Bridge) {
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    let beta = Bridge::start(BETA, Some("project-b"), SHARED_NAME);
    let gamma = Bridge::start(GAMMA, Some("project-a"), SHARED_NAME);
    machine.publish(&alpha);
    machine.publish(&beta);
    machine.publish(&gamma);
    (alpha, beta, gamma)
}

// ---------------------------------------------------------------------------
// Acceptance 1 — explicit routing keeps every operation with its target
// ---------------------------------------------------------------------------

/// Each of three live instances, named explicitly, performs its own operation
/// and the other two perform nothing. This is the whole of acceptance 1's
/// "prove each operation and result stays with its target", asserted from the
/// instances' own records rather than from what `ds` printed.
#[test]
fn each_named_instance_performs_its_own_operation_and_the_others_perform_none() {
    let machine = Machine::new();
    let (alpha, beta, gamma) = three(&machine);

    for bridge in [&alpha, &beta, &gamma] {
        let answer = Invocation::signed_in()
            .invoke(
                Some(&target(&bridge.instance_id)),
                &PROJECT_OP,
                json!({ "folder": "/" }),
            )
            .expect("a named live instance performs the operation");
        assert_eq!(
            answer["servedBy"],
            json!(bridge.instance_id),
            "the answer came from the instance that was named",
        );
    }

    // One operation each, and not one more anywhere.
    for bridge in [&alpha, &beta, &gamma] {
        assert_eq!(
            bridge.invoked(),
            vec![PROJECT_OP.operation.to_owned()],
            "instance {} performed the wrong set of operations",
            bridge.instance_id,
        );
    }
}

/// Two instances whose projects share a display name are still two projects.
/// The contract says names alone do not distinguish them; the id does, and the
/// id is what the operator names.
#[test]
fn equal_display_names_are_still_two_projects_told_apart_by_instance_id() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    let beta = Bridge::start(BETA, Some("project-b"), SHARED_NAME);
    machine.publish(&alpha);
    machine.publish(&beta);

    let first = Invocation::signed_in()
        .run(
            &ds_cli_desktop::project::LIST_COMMAND,
            ds_cli_desktop::project::list,
            &["--target", &target(ALPHA)],
        )
        .expect("the named instance answers");
    let second = Invocation::signed_in()
        .run(
            &ds_cli_desktop::project::LIST_COMMAND,
            ds_cli_desktop::project::list,
            &["--target", &target(BETA)],
        )
        .expect("the named instance answers");

    // Identical display names …
    assert_eq!(first["projects"][0]["name"], json!(SHARED_NAME));
    assert_eq!(second["projects"][0]["name"], json!(SHARED_NAME));
    // … two different projects, in two different instances.
    assert_eq!(first["activeProject"], json!("project-a"));
    assert_eq!(second["activeProject"], json!("project-b"));
    assert_eq!(first["servedBy"], json!(ALPHA));
    assert_eq!(second["servedBy"], json!(BETA));
    assert_eq!(alpha.invoked(), vec!["project.list".to_owned()]);
    assert_eq!(beta.invoked(), vec!["project.list".to_owned()]);
}

/// The same project open in two instances is not an error and not a guess: it
/// refuses, names both, and is settled by naming one — which then performs the
/// operation alone.
#[test]
fn the_same_project_in_two_instances_refuses_until_one_is_named() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    let gamma = Bridge::start(GAMMA, Some("project-a"), SHARED_NAME);
    machine.publish(&alpha);
    machine.publish(&gamma);

    let refusal = refused(
        Invocation::on_project("project-a").invoke(None, &PROJECT_OP, json!({})),
        "two instances hold the project",
    );
    assert_eq!(refusal.code(), "desktop_ambiguous");
    let candidates = refusal.detail_value().expect("candidates")["candidates"]
        .as_array()
        .expect("an array")
        .clone();
    assert_eq!(candidates.len(), 2);
    let named: Vec<&str> = candidates
        .iter()
        .filter_map(|candidate| candidate["instance_id"].as_str())
        .collect();
    assert_eq!(named, vec![ALPHA, GAMMA], "both, ordered by id");
    untouched(&[&alpha, &gamma]);

    let answer = Invocation::on_project("project-a")
        .invoke(Some(&target(GAMMA)), &PROJECT_OP, json!({}))
        .expect("naming one settles it");
    assert_eq!(answer["servedBy"], json!(GAMMA));
    assert!(alpha.invoked().is_empty(), "the instance nobody named");
    assert_eq!(gamma.invoked(), vec![PROJECT_OP.operation.to_owned()]);
}

/// The enumeration every instance-targeted refusal points at: one row per live
/// instance, ordered by id, carrying only what a client may show.
#[test]
fn the_enumeration_names_every_live_instance_and_no_token_address_or_account() {
    let machine = Machine::new();
    let (alpha, beta, gamma) = three(&machine);

    let listed = Invocation::signed_in()
        .run(
            &ds_cli_desktop::list::COMMAND,
            ds_cli_desktop::list::run,
            &[],
        )
        .expect("the enumeration answers");

    assert_eq!(listed["live"], json!(3));
    let ids: Vec<&str> = listed["instances"]
        .as_array()
        .expect("rows")
        .iter()
        .filter_map(|row| row["instance_id"].as_str())
        .collect();
    assert_eq!(
        ids,
        vec![ALPHA, BETA, GAMMA],
        "ordered by id, never by read order"
    );
    assert_eq!(listed["compatible"], json!([ALPHA, BETA, GAMMA]));
    for row in listed["instances"].as_array().expect("rows") {
        assert_eq!(row["state"], json!("ready"));
        assert_eq!(row["identity"], json!("minted"));
        assert_eq!(row["windows"], json!(1), "one window open in each");
    }

    let rendered = serde_json::to_string(&listed).expect("encodes");
    for secret in [
        alpha.token.as_str(),
        beta.token.as_str(),
        gamma.token.as_str(),
        "127.0.0.1",
        fixtures::UID,
        fixtures::EMAIL,
        fixtures::AUDIENCE,
    ] {
        assert!(!rendered.contains(secret), "the enumeration named {secret}");
    }
    // Reading the enumeration is not an effect on anything it found.
    untouched(&[&alpha, &beta, &gamma]);
}

/// The fence a paired operation carries is the OWNER WINDOW's generation, and
/// it follows that window: after the one operation allowed to move a project,
/// the next operation is fenced on the number the switch produced.
#[test]
fn the_identity_fence_carries_the_owner_windows_generation_and_follows_it() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    machine.publish(&alpha);

    Invocation::signed_in()
        .invoke(Some(&target(ALPHA)), &PROJECT_OP, json!({}))
        .expect("the operation runs");
    let fence = alpha.fence(0);
    assert_eq!(
        fence["context_generation"],
        json!(2),
        "the owner window's generation"
    );
    assert_eq!(fence["session_revision"], json!(3));
    assert_eq!(fence["project"], json!("project-a"));
    assert_eq!(fence["uid"], json!(fixtures::UID));

    // The explicit switch moves that window, so the next operation is fenced on
    // the generation the switch produced — not on a process-wide counter.
    Invocation::signed_in()
        .run(
            &ds_cli_desktop::project::SWITCH_COMMAND,
            ds_cli_desktop::project::switch,
            &["--project", "project-b", "--target", &target(ALPHA)],
        )
        .expect("the explicit switch");
    Invocation::signed_in()
        .invoke(Some(&target(ALPHA)), &PROJECT_OP, json!({}))
        .expect("the operation runs again");
    let after = alpha
        .received()
        .iter()
        .filter(|request| {
            request.path == "/v1/invoke" && request.operation() == Some(PROJECT_OP.operation)
        })
        .count();
    assert_eq!(after, 2);
    assert_eq!(alpha.fence(2)["context_generation"], json!(3));
}

// ---------------------------------------------------------------------------
// Acceptance 4 — stale, ambiguous, restarted, missing: zero misrouted effects
// ---------------------------------------------------------------------------

/// Two live instances and nothing to choose between them: the refusal happens
/// before either is asked to do anything. Focus, recency and read order decide
/// nothing, because nothing decides.
#[test]
fn ambiguity_refuses_before_any_instance_receives_an_invoke() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    let beta = Bridge::start(BETA, Some("project-b"), SHARED_NAME);
    machine.publish(&alpha);
    machine.publish(&beta);

    let refusal = refused(
        Invocation::signed_in().invoke(None, &PROJECT_OP, json!({})),
        "two instances could serve it",
    );
    assert_eq!(refusal.code(), "desktop_ambiguous");
    assert_eq!(
        refusal.remedy_text(),
        Some("name one with --target desktop:<instance_id>"),
    );
    untouched(&[&alpha, &beta]);
    // Each was asked who it is — and nothing else. Liveness is a handshake,
    // never an effect.
    assert!(alpha.probed() && beta.probed());
}

/// An explicitly named instance that is not live refuses by name. It never
/// falls through — not to the one live instance that would have been the
/// automatic answer, and not to any of three.
#[test]
fn an_explicitly_named_dead_instance_never_falls_through_to_a_live_one() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    machine.publish(&alpha);

    let refusal = refused(
        Invocation::signed_in().invoke(Some(&target(DEAD)), &PROJECT_OP, json!({})),
        "the named instance is not live",
    );
    assert_eq!(refusal.code(), "desktop_target_not_live");
    assert_eq!(
        refusal.detail_value().expect("a target")["target"],
        json!(DEAD),
    );
    untouched(&[&alpha]);

    // And with three live instances, the answer is the same one.
    let beta = Bridge::start(BETA, Some("project-b"), SHARED_NAME);
    let gamma = Bridge::start(GAMMA, Some("project-a"), SHARED_NAME);
    machine.publish(&beta);
    machine.publish(&gamma);
    assert_eq!(
        refused(
            Invocation::signed_in().invoke(Some(&target(DEAD)), &PROJECT_OP, json!({})),
            "still not live",
        )
        .code(),
        "desktop_target_not_live",
    );
    untouched(&[&alpha, &beta, &gamma]);
}

/// A descriptor is skipped only after its endpoint has been asked, with the
/// pairing token, who it is — and has failed to answer as itself. A live TCP
/// port is not liveness: something else reused it.
#[test]
fn a_stale_descriptor_is_skipped_only_after_a_failed_authenticated_probe() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    // Something is listening on the port the old descriptor names, and it is
    // not the instance that wrote it: every token it is shown is refused.
    let squatter = Bridge::stale(None);
    machine.publish(&alpha);
    machine.publish_unanswered(BETA, squatter.port);

    let answer = Invocation::signed_in()
        .invoke(None, &PROJECT_OP, json!({}))
        .expect("the one live instance");
    assert_eq!(answer["servedBy"], json!(ALPHA));

    let attempted = squatter.received();
    assert_eq!(
        attempted.len(),
        1,
        "the stale endpoint was asked exactly once"
    );
    assert_eq!(attempted[0].path, "/v1/session");
    assert!(
        !attempted[0].authorized,
        "it could not answer as that instance"
    );
    assert!(squatter.invoked().is_empty(), "and was never given work");
    assert_eq!(alpha.invoked(), vec![PROJECT_OP.operation.to_owned()]);
}

/// A restarted instance takes the port its predecessor released. The old
/// descriptor must not reach it: the token is different, so the handshake fails
/// and the dead instance's id stays dead — it is never answered by the process
/// that replaced it.
#[test]
fn a_reused_port_never_answers_to_the_descriptor_of_the_instance_it_replaced() {
    let machine = Machine::new();
    let mut alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    let port = alpha.port;
    machine.publish(&alpha);
    alpha.stop();

    // A new process, on the same port, with its own identity and its own token.
    let successor = Bridge::restart_on(port, BETA, Some("project-b"), SHARED_NAME);
    assert_eq!(successor.port, port);

    // Only the predecessor's descriptor is on disk. It reaches a listening
    // socket and is still not a pairing.
    let refusal = refused(
        Invocation::signed_in().invoke(None, &PROJECT_OP, json!({})),
        "the descriptor's instance is gone",
    );
    assert_eq!(refusal.code(), "desktop_not_paired");
    assert!(
        successor.probed(),
        "the successor was asked, with the old token"
    );
    untouched(&[&successor]);

    // The successor publishes its own descriptor. Now one instance is live —
    // and the predecessor's id is still not it.
    machine.publish(&successor);
    let refusal = refused(
        Invocation::signed_in().invoke(Some(&target(ALPHA)), &PROJECT_OP, json!({})),
        "the old id names nothing live",
    );
    assert_eq!(refusal.code(), "desktop_target_not_live");
    untouched(&[&successor]);

    let answer = Invocation::signed_in()
        .invoke(Some(&target(BETA)), &PROJECT_OP, json!({}))
        .expect("the successor, under its own name");
    assert_eq!(answer["servedBy"], json!(BETA));
    assert_eq!(successor.invoked(), vec![PROJECT_OP.operation.to_owned()]);
}

/// A second instance starting cannot remove or overwrite the first's
/// descriptor. The shell writes one file per instance and claims the legacy
/// per-profile file only when no live instance owns it — proven where the
/// writing happens, in `ds-web/src-tauri/src/cli_bridge.rs`
/// (`a_second_instance_writes_its_own_descriptor_and_never_the_first`,
/// `the_legacy_descriptor_is_claimed_only_when_it_is_ours_absent_or_dead`,
/// `exit_removes_only_the_descriptors_this_instance_published`). From the
/// CLI's side, what has to be true is that the first instance's bytes are
/// untouched, that it is still enumerated exactly once despite publishing two
/// files, and that it is still routable by name.
#[test]
fn a_second_instance_leaves_the_first_descriptor_intact_and_still_routable() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    let own = machine.publish(&alpha);
    // The first live instance also refreshes the legacy file for an older `ds`.
    let legacy = machine.publish_legacy(&alpha);
    let own_bytes = std::fs::read(&own).expect("the first descriptor");
    let legacy_bytes = std::fs::read(&legacy).expect("the legacy descriptor");

    // A second instance starts and publishes its own file. It does not claim
    // the legacy one, because the instance that holds it is live.
    let beta = Bridge::start(BETA, Some("project-b"), SHARED_NAME);
    machine.publish(&beta);

    assert_eq!(std::fs::read(&own).expect("still there"), own_bytes);
    assert_eq!(std::fs::read(&legacy).expect("still there"), legacy_bytes);

    // Three descriptor files, two instances: the legacy copy of the first is
    // the same instance, not a third.
    let listed = Invocation::signed_in()
        .run(
            &ds_cli_desktop::list::COMMAND,
            ds_cli_desktop::list::run,
            &[],
        )
        .expect("the enumeration answers");
    assert_eq!(listed["live"], json!(2));
    assert_eq!(listed["compatible"], json!([ALPHA, BETA]));

    let answer = Invocation::signed_in()
        .invoke(Some(&target(ALPHA)), &PROJECT_OP, json!({}))
        .expect("the first instance is still reachable by name");
    assert_eq!(answer["servedBy"], json!(ALPHA));
    assert!(beta.invoked().is_empty());
}

/// A saved CLI selection narrows where project work may go. It never moves a
/// live map: when no live instance holds it, the operation refuses with the
/// explicit switch as its remedy, and the instance that is running receives
/// nothing at all — no `project.switch`, no operation.
#[test]
fn a_saved_selection_narrows_the_route_and_never_sends_project_switch() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    machine.publish(&alpha);

    let refusal = refused(
        Invocation::on_project("project-z").invoke(None, &PROJECT_OP, json!({})),
        "no live instance holds the saved project",
    );
    assert_eq!(refusal.code(), "desktop_project_not_open");
    assert!(
        refusal
            .remedy_text()
            .is_some_and(|remedy| remedy.contains("ds desktop project switch --target")),
        "the remedy is the explicit, instance-qualified switch: {:?}",
        refusal.remedy_text(),
    );
    assert!(
        alpha.invoked().is_empty(),
        "a saved selection reached a live map: {:?}",
        alpha.invoked(),
    );
    assert_eq!(alpha.project().as_deref(), Some("project-a"));

    // The same operation, on the project that instance holds, runs there.
    let answer = Invocation::on_project("project-a")
        .invoke(None, &PROJECT_OP, json!({}))
        .expect("the instance that holds it");
    assert_eq!(answer["servedBy"], json!(ALPHA));
    assert_eq!(alpha.invoked(), vec![PROJECT_OP.operation.to_owned()]);
}

/// The one operation allowed to move a live window's project is the explicit,
/// instance-qualified switch — and it moves exactly the instance it named.
#[test]
fn only_the_explicit_switch_moves_a_project_and_only_in_the_instance_it_named() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    let beta = Bridge::start(BETA, Some("project-b"), SHARED_NAME);
    machine.publish(&alpha);
    machine.publish(&beta);

    let switched = Invocation::signed_in()
        .run(
            &ds_cli_desktop::project::SWITCH_COMMAND,
            ds_cli_desktop::project::switch,
            &["--project", "project-z", "--target", &target(ALPHA)],
        )
        .expect("the explicit switch");
    assert_eq!(switched["changed"], json!(true));
    assert_eq!(switched["activeProject"], json!("project-z"));
    assert_eq!(switched["servedBy"], json!(ALPHA));

    assert_eq!(alpha.invoked(), vec!["project.switch".to_owned()]);
    assert_eq!(alpha.project().as_deref(), Some("project-z"));
    // The other instance neither performed nor moved.
    assert!(beta.invoked().is_empty());
    assert_eq!(beta.project().as_deref(), Some("project-b"));
}

/// A switch the targeted instance did not complete is reported, never assumed.
///
/// `ds` re-observes the instance it addressed, and an answer that claims more
/// than the session shows is a conflict: the next command must not land in a
/// window the operator believes moved.
#[test]
fn a_switch_the_named_instance_did_not_complete_is_reported_not_assumed() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    // The application answers the switch as done and does not move.
    alpha.answer_next_invoke(
        200,
        json!({
            "changed": true,
            "previousProject": "project-a",
            "activeProject": "project-z",
            "instance": ALPHA,
            "window": "main",
        }),
    );
    machine.publish(&alpha);

    let refusal = refused(
        Invocation::signed_in().run(
            &ds_cli_desktop::project::SWITCH_COMMAND,
            ds_cli_desktop::project::switch,
            &["--project", "project-z", "--target", &target(ALPHA)],
        ),
        "the instance came back holding another project",
    );
    assert_eq!(refusal.code(), "auth_context_mismatch");
    let detail = refusal.detail_value().expect("a detail");
    assert_eq!(detail["project"], json!("project-z"));
    assert_eq!(
        detail["instance"],
        json!(ALPHA),
        "the instance it addressed"
    );
    assert_eq!(
        alpha.project().as_deref(),
        Some("project-a"),
        "it really did not move"
    );

    // And a switch to the project it already holds completes, and says so.
    let unchanged = Invocation::signed_in()
        .run(
            &ds_cli_desktop::project::SWITCH_COMMAND,
            ds_cli_desktop::project::switch,
            &["--project", "project-a", "--target", &target(ALPHA)],
        )
        .expect("a switch to the project already open");
    assert_eq!(unchanged["changed"], json!(false));
    assert_eq!(alpha.project().as_deref(), Some("project-a"));
}

/// The window's context changed while the operation was in flight. The
/// application refuses with the kernel's own code; `ds` must relay it as a
/// conflict with its remedy, not degrade it to an untyped `desktop_refused`.
#[test]
fn a_context_generation_refusal_reaches_the_caller_with_its_own_code_and_remedy() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    alpha.answer_next_invoke(
        409,
        json!({
            "error": {
                "class": "conflict",
                "code": "context_generation_stale",
                "message": "this view's project or account changed while that was in flight",
                "remedy": "retry the operation against the view as it is now",
            }
        }),
    );
    machine.publish(&alpha);

    let refusal = refused(
        Invocation::signed_in().invoke(Some(&target(ALPHA)), &PROJECT_OP, json!({})),
        "the view moved under the operation",
    );
    assert_eq!(refusal.code(), "context_generation_stale");
    assert_eq!(
        refusal.remedy_text(),
        Some("retry the operation against the view as it is now"),
    );
    assert_eq!(
        refusal.class(),
        ds_cli_contract::ExitClass::Conflict,
        "a conflict, so a caller retries against the view as it is now",
    );
}

/// The MCP gate reads this enumeration through `ds desktop status` and launches
/// only from `DesktopState::Absent`. Both states a live machine can be in are
/// answered here, in the exact shape `ds-cli-mcp::tools::desktop_status` parses:
/// one live instance is `Paired`, more than one is `Ambiguous`. Neither reaches
/// the launch, so an MCP call can never add a third window to a machine that
/// already cannot say which of two it meant.
///
/// This is the half a decision table cannot prove: that a real machine with
/// real instances on it produces those two states. The other half — that
/// neither state reaches `launch()` — is
/// `ds-cli-mcp::tools::two_live_instances_refuse_a_tool_call_instead_of_launching_a_third`
/// and `an_already_running_desktop_is_never_duplicated`, which drive the gate
/// itself with its steps injected.
#[test]
fn the_mcp_gate_reads_a_live_instance_from_this_enumeration_and_never_reaches_its_launch() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    machine.publish(&alpha);

    // One live instance → `DesktopState::Paired { signed_in, project_selected }`.
    let status = Invocation::signed_in()
        .run(
            &ds_cli_desktop::status::COMMAND,
            ds_cli_desktop::status::run,
            &[],
        )
        .expect("status answers");
    assert_eq!(status["paired"], json!(true));
    assert_eq!(status["signed_in"], json!(true));
    assert_eq!(status["project"], json!("project-a"));
    assert_eq!(status["instance"], json!(ALPHA));

    // Two → `desktop_ambiguous`, whose detail is the instance list the gate
    // turns into `DesktopState::Ambiguous` and refuses on.
    let beta = Bridge::start(BETA, Some("project-b"), SHARED_NAME);
    machine.publish(&beta);
    let refusal = refused(
        Invocation::signed_in().run(
            &ds_cli_desktop::status::COMMAND,
            ds_cli_desktop::status::run,
            &[],
        ),
        "two instances are running",
    );
    assert_eq!(refusal.code(), "desktop_ambiguous");
    let instances: Vec<&str> = refusal.detail_value().expect("instances")["instances"]
        .as_array()
        .expect("an array")
        .iter()
        .filter_map(|instance| instance["instance_id"].as_str())
        .collect();
    assert_eq!(instances, vec![ALPHA, BETA]);
    assert_eq!(refusal.next_commands(), ["ds desktop list"]);
    untouched(&[&alpha, &beta]);
}

/// Two explicit answers to one question must be the same instance. A pinned
/// descriptor file and a `--target` that name different runtimes refuse before
/// either instance is asked to perform anything — the file's admitted identity
/// may be derived, so the running process is asked who it is and its answer is
/// what the target is held against.
#[test]
fn a_pinned_descriptor_and_a_target_that_disagree_refuse_without_an_effect() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    let beta = Bridge::start(BETA, Some("project-b"), SHARED_NAME);
    let pinned = machine.publish(&alpha);
    machine.publish(&beta);

    let refusal = refused(
        Invocation::signed_in().invoke_pinned(&pinned, Some(&target(BETA)), &PROJECT_OP, json!({})),
        "the pinned file is another instance than the target names",
    );
    assert_eq!(refusal.code(), "desktop_target_mismatch");
    assert_eq!(
        refusal.detail_value().expect("a reason")["reason"],
        json!("descriptor"),
    );
    assert_eq!(
        refusal.detail_value().expect("a reason")["descriptor_instance"],
        json!(ALPHA),
    );
    untouched(&[&alpha, &beta]);

    // And the pinned file alone, with no target, is used verbatim: the legacy
    // explicit path a pinned terminal relies on is unchanged.
    let answer = Invocation::signed_in()
        .invoke_pinned(&pinned, None, &PROJECT_OP, json!({}))
        .expect("a pinned descriptor is used as it always has been");
    assert_eq!(answer["servedBy"], json!(ALPHA));
    assert!(beta.invoked().is_empty());
}

/// A target that is not an instance id is answered before anything is read,
/// probed or sent — by the kernel's vocabulary, not the parser's.
#[test]
fn a_malformed_target_is_refused_before_any_instance_is_read() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    machine.publish(&alpha);

    let refusal = refused(
        Invocation::signed_in().invoke(Some("desktop:not-an-instance"), &PROJECT_OP, json!({})),
        "that is not an instance id",
    );
    assert_eq!(refusal.code(), "desktop_target_mismatch");
    assert_eq!(
        refusal.detail_value().expect("a reason")["reason"],
        json!("malformed"),
    );
    assert!(
        !alpha.probed(),
        "a malformed target was answered by reading the machine",
    );
    untouched(&[&alpha]);
}

// ---------------------------------------------------------------------------
// The argv path, when the binary that owns it has been built
// ---------------------------------------------------------------------------

/// Everything above drives the handlers `crates/ds/src/registry.rs` dispatches
/// to. This drives the executable itself, over the same fixtures, with the
/// registry directory in the CHILD's environment.
///
/// It is ignored by default because `cargo test -p ds-cli-desktop` does not
/// build the `ds` binary — a crate's own test cannot build the binary that
/// depends on it. Run it with the binary present:
///
/// ```text
/// cargo build -p ds --bin ds
/// cargo test -p ds-cli-desktop --test instances -- --ignored
/// ```
#[test]
#[ignore = "needs the ds executable: cargo build -p ds --bin ds, then run with --ignored"]
fn ignored_unless_the_ds_executable_is_built_argv_reaches_the_named_instance() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    let beta = Bridge::start(BETA, Some("project-b"), SHARED_NAME);
    machine.publish(&alpha);
    machine.publish(&beta);
    let executable = fixtures::ds_executable();

    let listed = fixtures::run_ds(
        &executable,
        &machine,
        &["desktop", "list", "--output", "json"],
    );
    assert_eq!(listed["status"], json!("ok"), "{listed}");
    assert_eq!(listed["data"]["live"], json!(2));

    let status = fixtures::run_ds(
        &executable,
        &machine,
        &[
            "desktop",
            "status",
            "--target",
            &target(BETA),
            "--output",
            "json",
        ],
    );
    assert_eq!(status["data"]["instance"], json!(BETA));
    assert_eq!(status["data"]["project"], json!("project-b"));

    let refusal = fixtures::run_ds(
        &executable,
        &machine,
        &[
            "desktop",
            "status",
            "--target",
            &target(DEAD),
            "--output",
            "json",
        ],
    );
    assert_eq!(refusal["error"]["code"], json!("desktop_target_not_live"));
    untouched(&[&alpha, &beta]);
}

/// Keep the crate's own vocabulary in this file honest: every code asserted
/// above is one the kernel publishes, so a rename cannot leave a green test
/// asserting a string nothing emits.
#[test]
fn every_refusal_this_proof_asserts_is_a_code_the_kernel_publishes() {
    let published: Vec<&str> = ds_command_kernel::desktop_instance::REFUSALS.to_vec();
    for code in [
        "descriptor_unusable",
        "desktop_not_paired",
        "desktop_ambiguous",
        "desktop_target_not_live",
        "desktop_target_mismatch",
        "desktop_project_not_open",
        "context_generation_stale",
    ] {
        assert!(published.contains(&code), "{code} is not a kernel refusal");
    }
}

/// **The one sub-claim of acceptance 4 this slice does not yet meet.**
///
/// On a machine where `ds` has no native profile of its own — nothing has told
/// it who is running it, which is a supported configuration and the whole
/// reason `discover::adopted_requirement` exists — dispatch scopes NO headless
/// observation (`registry::scope_headless_identity` maps the target *inside*
/// the identity, and there is no identity to put it in). The host `--target` is
/// dropped with it — `DS_TARGET` too, since it is that flag's default and is
/// read in the same place — so a paired command that explicitly names a dead
/// instance routes to whichever live one is compatible instead of refusing.
/// Measured through the built executable, against one live fixture instance:
///
/// ```text
/// $ ds desktop project list --target desktop:dddd…dddd --output json
/// {"status":"ok","data":{…,"servedBy":"aaaa…aaaa"}}          exit 0
/// $ ds desktop status       --target desktop:dddd…dddd --output json
/// {"status":"error","error":{"code":"desktop_target_not_live",…}}   exit 2
/// ```
///
/// `desktop status` is right because it reads its own declared flag
/// (`ops::declared_target`); every command that takes the host from the scoped
/// observation is wrong. `--desktop-descriptor` is unaffected: it is passed to
/// the handler as an argument and never rides the observation.
///
/// The contract is explicit — "An explicitly targeted dead or mismatched
/// instance refuses; it never falls through to another instance" — so this test
/// states the requirement rather than the behaviour, and is ignored until the
/// target is carried independently of the observation. The fix is in the CLI's
/// dispatch (`crates/ds/src/registry.rs::scope_headless_identity` with
/// `ds-cli-desktop::ops`), not in this crate's resolution: `bridge::paired_on`
/// already refuses correctly for every target it is actually given, which every
/// other test in this file proves.
#[test]
#[ignore = "GAP: dispatch drops --target when no native profile is configured; fix in ds/src/registry.rs::scope_headless_identity, then un-ignore"]
fn ignored_until_dispatch_carries_a_target_with_no_native_profile_a_dead_instance_still_refuses() {
    let machine = Machine::new();
    let alpha = Bridge::start(ALPHA, Some("project-a"), SHARED_NAME);
    machine.publish(&alpha);

    let refusal = refused(
        Invocation::unprovisioned().invoke(Some(&target(DEAD)), &PROJECT_OP, json!({})),
        "the named instance is not live, whoever is asking",
    );
    assert_eq!(refusal.code(), "desktop_target_not_live");
    untouched(&[&alpha]);
}
