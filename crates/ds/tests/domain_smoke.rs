//! Semantic smoke: every offline command, against real domain data.
//!
//! The other suites prove the *shape* of the surface — that help matches the
//! descriptor, that codes are documented, that budgets hold. None of them can
//! catch the failure that actually happened twice while this domain was being
//! built:
//!
//! * `ds pls reference-closure` defaulted `--limit` to 50 against a task that
//!   bounds it at 32, so every call refused;
//! * `ds dsgrid-exchange inspect` filtered capabilities by comparing a
//!   `Debug` spelling against `"Available"` — a variant that does not exist —
//!   so a source set with ten ready conversions reported none.
//!
//! Both compiled. Both had correct help, documented refusals and a passing
//! contract suite. What they did not have was anyone running them against a
//! real file and looking at the answer.
//!
//! So each command here is invoked exactly as a caller would, against the
//! fixtures in `ds-network`, and the answer is asserted to be *specifically*
//! right — not merely well-formed. An assertion like "at least one capability
//! is offered" is what makes this suite worth having; an assertion that the
//! response parses is not.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

/// Whether a command can be DISCOVERED without a window paired. A server
/// command reports `available`. A window command reports `requires_window` —
/// since 2026-09-19 it says so instead of certifying itself available on a
/// machine with no window, which is what let the collaboration ratchet below
/// pass as a false green for months. Either way the descriptor is readable;
/// that is what these smoke tests guard, so both tokens pass here.
fn discoverable(command: &Value) -> bool {
    matches!(
        command["availability"].as_str(),
        Some("available" | "requires_window")
    )
}

#[test]
fn style_purposes_are_a_named_read_and_preserve_native_auth_refusals() {
    let described = native_ds(&["capabilities", "style.purpose.index", "--output", "json"]);
    assert_eq!(described.code, 0, "{}", described.envelope);
    let command = &described.envelope["data"]["command"];
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "local_auth_state");
    assert!(
        command["purpose"]
            .as_str()
            .unwrap()
            .contains("separate Templates authority")
    );
    assert!(
        command["refusals"]
            .as_array()
            .unwrap()
            .iter()
            .any(|refusal| refusal["code"] == "style_purpose_line_type_missing")
    );
    let result = native_ds(&[
        "style",
        "purpose",
        "index",
        "--project",
        "named-project",
        "--target",
        "print",
        "--lane",
        "canary",
        "--output",
        "json",
    ]);
    assert_ne!(result.code, 0);
    assert_eq!(result.envelope["error"]["code"], "headless_signed_out");
    assert!(result.envelope["data"].is_null());

    let planned = native_ds(&["capabilities", "style.purpose.plan", "--output", "json"]);
    assert_eq!(planned.code, 0, "{}", planned.envelope);
    let command = &planned.envelope["data"]["command"];
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "local_auth_state");
    assert_eq!(command["requires"], "server");
    let refused = native_ds(&[
        "style",
        "purpose",
        "plan",
        "--project",
        "named-project",
        "--purpose",
        "transformer_a0",
        "--lane",
        "canary",
        "--output",
        "json",
    ]);
    assert_ne!(refused.code, 0);
    assert_eq!(refused.envelope["error"]["code"], "headless_signed_out");
    assert!(refused.envelope["data"].is_null());
}

#[test]
fn saved_lv_analysis_read_is_discoverable_and_refuses_existing_output_before_auth() {
    let described = native_ds(&[
        "capabilities",
        "design.lv.analysis-read",
        "--output",
        "json",
    ]);
    assert_eq!(described.code, 0, "{}", described.envelope);
    let command = &described.envelope["data"]["command"];
    assert_eq!(command["effect"], "local_file_write");
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["availability"], "available");
    assert!(
        command["purpose"]
            .as_str()
            .unwrap()
            .contains("never computes")
    );
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("owner.zip");
    std::fs::write(&path, b"owner bytes").unwrap();
    let refused = native_ds(&[
        "design",
        "lv",
        "analysis-read",
        "--project",
        "explicit-project",
        "--transformer",
        "T1",
        "--out",
        path.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert_ne!(refused.code, 0);
    assert_eq!(
        refused.envelope["error"]["code"],
        "lv_analysis_output_exists"
    );
    assert_eq!(std::fs::read(path).unwrap(), b"owner bytes");
}

#[test]
fn lv_project_run_is_discoverable_and_print_preflight_has_no_project_effect() {
    let described = ds(&["capabilities", "design.lv.project-run", "--output", "json"]);
    assert_eq!(described.code, 0);
    let command = &described.envelope["data"]["command"];
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["confirmation_required"], true);
    assert!(
        command["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|input| input["name"] == "resume")
    );
    // This fixture owns an absent reporter, independent of the installed
    // browser or the admitted reporter used by other engine-parity tests.
    let fixture = tempfile::tempdir().unwrap();
    let directory = fixture.path().join("run");
    let missing_reporter = fixture.path().join("missing-ds-report");
    assert!(!directory.exists());
    assert!(!missing_reporter.exists());
    let refused = run_ds_with_reporter(
        &[
            "design",
            "lv",
            "project-run",
            "--project",
            "explicit-project",
            "--transformer",
            "T1",
            "--out-dir",
            directory.to_str().unwrap(),
            "--print-a4",
            "--yes",
            "--output",
            "json",
        ],
        true,
        Some(&missing_reporter),
    );
    assert_ne!(refused.code, 0);
    assert_eq!(
        refused.envelope["error"]["code"], "reporter_engine_missing",
        "{}",
        refused.envelope
    );
    assert!(refused.envelope["data"].is_null());
    assert!(!directory.exists());
    assert_eq!(std::fs::read_dir(fixture.path()).unwrap().count(), 0);
}



#[test]
fn print_removal_is_a_confirmed_headless_project_operation() {
    let described = ok(&["capabilities", "report.artifact.remove", "--output", "json"]);
    assert_eq!(described["command"]["authority"], "headless_project");
    assert_eq!(described["command"]["effect"], "global_write");
    assert!(
        described["command"]["output"]
            .as_str()
            .unwrap()
            .contains("bytes_deleted=false")
    );
    assert_eq!(
        refusal(&[
            "report",
            "artifact",
            "remove",
            "--project",
            "test-project",
            "--scope",
            "mv",
            "--transformer",
            "mv_data",
            "--filename",
            "map.pdf",
            "--gcs-path",
            "gs://bucket/map.pdf",
            "--sha256",
            &"a".repeat(64),
            "--yes",
            "--output",
            "json",
        ]),
        "native_profile_not_configured"
    );
}

/// Design commands the collaboration test reaches that are still bound to a
/// window. 27 on 2026-09-19; 0 on 2026-09-20, when tags, groups, comments,
/// consumer grouping, known columns and materials took the kernel's design
/// doors under the native credential and `design.sync.*` was retired with
/// the window-private queue it controlled. This list only SHRINKS: a new
/// window-bound collaboration command cannot be added without widening it
/// deliberately, and the fence in `lens_core_boundary.rs` refuses that.
const COLLABORATION_WINDOW_BACKLOG: &[&str] = &[];

mod common;

const NATIVE_AUTH_CODES: &[&str] = &[
    "native_profile_not_configured",
    "native_profile_unsafe",
    "native_profile_digest_mismatch",
    "headless_signed_out",
    "headless_project_not_selected",
];

#[test]
fn native_lv_save_refuses_missing_receipt_before_authentication() {
    let result = native_ds(&[
        "design",
        "lv",
        "project-save",
        "--source",
        "/__ds_absent__/source.json",
        "--input",
        "/__ds_absent__/input.json",
        "--result",
        "/__ds_absent__/result.json",
        "--process-receipt",
        "/__ds_absent__/process.json",
        "--transformer",
        "T1",
        "--operation-id",
        "smoke-save",
        "--yes",
        "--output",
        "json",
    ]);
    assert_ne!(result.code, 0);
    assert_eq!(
        result.envelope["error"]["code"], "fast_lv_save_input_invalid",
        "{}",
        result.envelope
    );
}

#[test]
fn core_engine_reports_the_installed_solar_owner_without_auth_or_server() {
    let result = native_ds(&["core", "engine", "--output", "json"]);
    // Solar is an independently installed addon since Core decoupling: with
    // none installed here the answer is its named refusal, never a guess.
    if result.envelope["error"]["code"] == "solar_engine_missing" {
        assert_eq!(result.code, 3, "{}", result.stdout);
        assert!(
            result.envelope["error"]["remedy"]
                .as_str()
                .is_some_and(|remedy| remedy.contains("DS_SOLAR_ENGINE_BIN")),
            "{}",
            result.stdout
        );
        eprintln!("SKIPPED identity comparison: no Solar addon is installed or selected");
        return;
    }
    assert_eq!(result.code, 0, "{}", result.stderr);
    assert_eq!(result.envelope["status"], "ok");
    let mut expected = ds_solar_contracts::EngineIdentity::current();
    expected.schemas = ds_solar_contracts::EngineIdentity::stamped_schema_inventory();
    assert_eq!(expected.name, "ds-solar-engine");
    assert!(!expected.schemas.is_empty());
    // The installed addon answers with its build identity (ds.engine-build/v1):
    // the same engine name and version, and the stamped schemas it serves.
    let data = &result.envelope["data"];
    assert_eq!(data["schema"], "ds.engine-build/v1");
    assert_eq!(data["name"], expected.name);
    assert_eq!(data["version"], expected.version);
    assert_eq!(data["schemas"], serde_json::to_value(&expected.schemas).unwrap());
}

#[test]
fn a_server_call_names_its_project_before_it_needs_a_credential_or_a_host() {
    // A project id outside its bound is the caller's mistake, and it is
    // answered under the kernel's own name for that situation -- not a generic
    // server failure, and not after a round trip. Nothing here signs in,
    // reaches a socket or reads a state directory.
    let padded = native_ds(&[
        "core",
        "status",
        "--project",
        " padded",
        "--output",
        "json",
    ]);
    assert_eq!(padded.envelope["error"]["code"], "context_corrupt");
    assert_eq!(padded.code, 2, "an unusable flag is invalid input");
    assert!(
        padded.envelope["error"]["remedy"]
            .as_str()
            .unwrap_or_default()
            .contains("ds auth project list"),
        "a refused project must say where an exact one comes from: {}",
        padded.stdout
    );

    // And with no project named and no saved selection to default to, the call
    // refuses here rather than letting a Server pick one. `native_ds` runs
    // against an empty DS_CONFIG_HOME, so this is the no-selection case.
    let unscoped = native_refusal(&["core", "activity", "--output", "json"]);
    assert!(
        unscoped == "project_required" || NATIVE_AUTH_CODES.contains(&unscoped.as_str()),
        "an unscoped Server call must refuse by name, not proceed; got `{unscoped}`"
    );
}

#[test]
fn hosting_answers_an_impossible_share_without_signing_in() {
    // `--per-project` bounds one project's share of the host. It is measured
    // and parsed locally, so an impossible value is refused on any machine --
    // including one that has never run `ds auth login`, which is exactly the
    // machine that can now start a host.
    for (value, expected) in [("0", "1.."), ("100000", "1.."), ("half", "whole number")] {
        let run = native_ds(&[
            "core",
            "serve",
            "--per-project",
            value,
            "--output",
            "json",
        ]);
        assert_ne!(run.code, 0, "`--per-project {value}` must refuse");
        let message = run.envelope["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .to_owned();
        assert!(
            message.contains(expected),
            "`--per-project {value}` refused with `{message}`, which does not state its bound"
        );
    }
}

#[test]
fn the_layer_drawer_has_one_command_id_and_it_is_not_the_servers() {
    // Retired, and retired means unreachable: `ds server layers …` must not
    // resolve at all, so a caller is sent to the one id that names the
    // operation instead of finding a second one that happens to still work.
    let retired = ds(&["core", "layers", "list", "--output", "json"]);
    assert_ne!(retired.code, 0, "`ds server layers list` still resolves");
    assert_eq!(retired.envelope["status"], "error");
    assert!(
        !ds(&["core", "--help"]).stdout.contains("layers"),
        "`ds server --help` still offers the retired layer commands"
    );
    // The surviving id is the one that takes a host, not a second name for it.
    assert_eq!(ds(&["map", "layer", "list", "--help"]).code, 0);
}

/// The other half of the same ruling: the surviving id takes a HOST, and the
/// default host is the one it always ran on.
///
/// The declared default is the desktop, and an invocation without the flag
/// still ends at the native authentication boundary.
#[test]
fn the_default_target_is_the_desktop_as_before() {
    for id in [
        "map.layer.list",
        "map.layer.default",
        "map.layer.show",
        "map.layer.hide",
        "map.layer.reorder",
    ] {
        let command = ok(&["capabilities", id, "--output", "json"])["command"].clone();
        let inputs = command["inputs"].as_array().expect("inputs").clone();
        let named = |name: &str| {
            inputs
                .iter()
                .find(|input| input["name"] == name)
                .unwrap_or_else(|| panic!("`{id}` declares --{name}"))
                .clone()
        };
        let target = named("target");
        assert_eq!(target["default"], "desktop", "`{id}`'s default host moved");
        assert_eq!(target["required"], false);
        // Deliberately NOT a closed choice set: `desktop:<instance>` has to
        // reach the handler to be refused by its own name rather than by the
        // parser's generic `invalid_choice`.
        assert!(
            target["choices"].is_null(),
            "`{id}`'s --target must accept desktop:<instance> to refuse it by name"
        );
        // The project is for THIS call and always named: the saved selection
        // is never read, so no host may choose one.
        assert_eq!(named("project")["required"], true);
        assert_eq!(named("state-dir")["required"], false);
    }

    // Untargeted, the four commands land where they always did.
    for args in [
        vec!["map", "layer", "list"],
        vec!["map", "layer", "show", "--layer", "survey/poles"],
        vec!["map", "layer", "hide", "--layer", "survey/poles"],
        vec![
            "map",
            "layer",
            "reorder",
            "--order",
            "survey/poles=10",
            "--yes",
        ],
    ] {
        let mut args = args;
        args.extend_from_slice(&["--project", "test-project", "--output", "json"]);
        let code = native_refusal(&args);
        assert!(
            NATIVE_AUTH_CODES.contains(&code.as_str()),
            "`ds {}` now ends in `{code}`, not at the native boundary it always ended at",
            args.join(" ")
        );
    }
}

/// The layer drawer's four operations run against the Server under THEIR OWN
/// ids, with the same arguments, and the routing decision is the only
/// difference. Nothing here reaches a socket: what is proven is that
/// `--target server` executes the Server transport rather than the desktop,
/// that the project is resolved and sent by the client, and that every code
/// this path can produce is one the command documents.
#[test]
fn a_map_layer_op_runs_the_same_against_the_server() {
    // The client names the project before it sends anything: without one the
    // parser refuses here (the saved selection is never read), rather than
    // letting a host pick.
    for args in [
        vec!["map", "layer", "list", "--target", "core"],
        vec![
            "map",
            "layer",
            "hide",
            "--layer",
            "survey/poles",
            "--target",
            "core",
        ],
        vec![
            "map",
            "layer",
            "show",
            "--layer",
            "survey/poles",
            "--target",
            "core",
        ],
        vec![
            "map",
            "layer",
            "reorder",
            "--order",
            "survey/poles=10",
            "--yes",
            "--target",
            "core",
        ],
    ] {
        let mut args = args;
        args.extend_from_slice(&["--output", "json"]);
        assert_eq!(
            native_refusal(&args),
            "missing_input",
            "`ds {}` did not require its own project",
            args.join(" ")
        );
    }

    // Named a project, the same call reaches for the Server's protected
    // connection -- and says so when there is none, instead of silently
    // running on the desktop.
    let unreachable = native_ds(&[
        "map",
        "layer",
        "hide",
        "--layer",
        "survey/poles",
        "--target",
        "core",
        "--project",
        "project-a",
        "--state-dir",
        "/nonexistent/ds-server-state",
        "--output",
        "json",
    ]);
    assert_eq!(unreachable.envelope["error"]["code"], "server_refused");

    // A project id outside the kernel's bound is the caller's mistake, under
    // the kernel's own word for it, before a socket is opened.
    let padded = native_ds(&[
        "map",
        "layer",
        "list",
        "--target",
        "core",
        "--project",
        " padded",
        "--output",
        "json",
    ]);
    assert_eq!(padded.envelope["error"]["code"], "context_corrupt");

    // One host per invocation, named. An instance is a shape this build knows
    // and refuses by name; anything else is not a host at all.
    let instance = native_ds(&[
        "map",
        "layer",
        "list",
        "--project",
        "test-project",
        "--target",
        "desktop:kigali",
        "--output",
        "json",
    ]);
    assert_eq!(
        instance.envelope["error"]["code"],
        "target_instance_unsupported"
    );
    assert!(
        instance.envelope["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .contains("kigali"),
        "the refusal names the instance that was asked for: {}",
        instance.stdout
    );
    assert_eq!(
        native_refusal(&[
            "map",
            "layer",
            "list",
            "--project",
            "test-project",
            "--target",
            "cloud",
            "--output",
            "json"
        ]),
        "unknown_target"
    );

    // And every one of those codes is in the command's own documented
    // refusals, so a caller plans for this path from --help alone.
    for (id, expected) in [
        ("map.layer.list", "context_corrupt"),
        ("map.layer.list", "unknown_target"),
        ("map.layer.list", "target_instance_unsupported"),
        ("map.layer.hide", "project_required"),
        ("map.layer.hide", "server_refused"),
        ("map.layer.reorder", "server_owner_changed"),
        ("map.layer.reorder", "invalid_order"),
    ] {
        let command = ok(&["capabilities", id, "--output", "json"])["command"].clone();
        let documented: Vec<&str> = command["refusals"]
            .as_array()
            .expect("refusals")
            .iter()
            .map(|refusal| refusal["code"].as_str().expect("code"))
            .collect();
        assert!(
            documented.contains(&expected),
            "`{id}` can answer `{expected}` and does not document it: {documented:?}"
        );
        // And the converse, because a documented refusal a command cannot
        // reach is prose: no layer operation needs a rendered map, so none of
        // them claims the refusal for one. That claim belongs to the host that
        // answers it.
        for unreachable in ["needs_paired_map", "multi_principal_unsupported"] {
            assert!(
                !documented.contains(&unreachable),
                "`{id}` documents `{unreachable}`: the layer drawer needs no rendered map, \
                 and a second account never reaches an operation -- the owner-only socket \
                 stops it at the door"
            );
        }
    }
    // Both of those belong to the command that DOES answer them.
    let hosting = ok(&["capabilities", "core.serve", "--output", "json"])["command"]["refusals"]
        .as_array()
        .expect("refusals")
        .iter()
        .map(|refusal| refusal["code"].as_str().unwrap_or_default().to_owned())
        .collect::<Vec<_>>();
    for owed in ["needs_paired_map", "multi_principal_unsupported"] {
        assert!(
            hosting.contains(&owed.to_owned()),
            "`ds server serve` answers `{owed}` and must declare it: {hosting:?}"
        );
    }
}

struct Run {
    envelope: Value,
    stdout: String,
    stderr: String,
    code: i32,
}

fn ds(args: &[&str]) -> Run {
    run_ds(args, false)
}
fn native_ds(args: &[&str]) -> Run {
    run_ds(args, true)
}
fn native_refusal(args: &[&str]) -> String {
    native_ds(args).envelope["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_owned()
}

fn named_pm_args<'a>(args: &[&'a str]) -> Vec<&'a str> {
    let mut named = args.to_vec();
    named.extend(["--project", "test-project"]);
    named
}

fn native_pm_refusal(args: &[&str]) -> String {
    native_refusal(&named_pm_args(args))
}


fn pm_refusal(args: &[&str]) -> String {
    refusal(&named_pm_args(args))
}
fn run_ds(args: &[&str], native: bool) -> Run {
    run_ds_with_reporter(args, native, None)
}
fn run_ds_with_reporter(args: &[&str], native: bool, reporter: Option<&std::path::Path>) -> Run {
    let config = temp_root("native-smoke-auth");
    // The development catalogue is the reviewed action vocabulary written out
    // literally, not a hashed release artefact: it carries no profile digest
    // and no producer stages it. When the closed vocabulary changes, its
    // literals are edited here in the same commit as the change.
    let bundle = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-cli-auth/tests/fixtures/development-catalog.json");
    let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
    // Smoke tests must never discover or contact the operator's running app.
    // Valid bridge inputs reach a deliberately absent descriptor; invalid
    // inputs must still be refused before discovery.
    command
        .args(args)
        .env("NO_COLOR", "1")
        .env("DS_SRE_REPORTING", "off")
        .env("DS_DESKTOP_DESCRIPTOR", config.join("no-desktop.json"));
    if native {
        command
            .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", &bundle)
            .env("DS_CONFIG_HOME", &config);
    }
    if let Some(reporter) = reporter {
        command.env("DS_REPORT_BIN", reporter);
    }
    let output = command.output().expect("ds binary runs");
    let _ = std::fs::remove_dir_all(config);
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    Run {
        envelope: serde_json::from_str(&stdout).unwrap_or(Value::Null),
        stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

#[test]
fn desktop_sync_plan_answers_headlessly_and_refuses_a_malformed_inventory() {
    let dir = tempfile::tempdir().unwrap();
    let local = dir.path().join("local.json");
    let remote = dir.path().join("remote.json");
    let sha = "a".repeat(64);
    std::fs::write(
        &local,
        serde_json::json!([{"engine":"network_reporter","operation":"export-a","sha256":sha,"size_bytes":1,
            "produced_at_ms":1,"engine_release":"ds-network-reporter@0.1.0+0123456789abcdef0123456789abcdef01234567"}])
        .to_string(),
    )
    .unwrap();
    std::fs::write(&remote, "[]").unwrap();
    let plan = ok(&[
        "desktop",
        "sync",
        "plan",
        "--project",
        "p",
        "--local",
        local.to_str().unwrap(),
        "--remote",
        remote.to_str().unwrap(),
        "--now-ms",
        "10000",
        "--output",
        "json",
    ]);
    assert_eq!(plan["schema"], "ds.sync-plan/v1");
    assert_eq!(plan["actions"][0]["kind"], "open_grant");
    assert_eq!(plan["actions"][1]["kind"], "upload");
    assert_eq!(plan["grant_valid"], false);
    let offline = ok(&[
        "desktop",
        "sync",
        "plan",
        "--project",
        "p",
        "--offline",
        "--now-ms",
        "1",
        "--output",
        "json",
    ]);
    assert_eq!(offline["actions"][0]["kind"], "offline");
    std::fs::write(&local, "{not json").unwrap();
    let refused = ds(&[
        "desktop",
        "sync",
        "plan",
        "--project",
        "p",
        "--local",
        local.to_str().unwrap(),
        "--output",
        "json",
    ]);
    assert_eq!(refused.envelope["error"]["code"], "sync_plan_invalid");
    let described = ok(&["capabilities", "desktop.sync.plan", "--output", "json"]);
    assert_eq!(described["command"]["authority"], "none");
}

#[test]
fn published_read_checks_selectors_and_never_falls_back_to_local_files() {
    let invalid = ds(&[
        "desktop",
        "sync",
        "published",
        "--project",
        &"p".repeat(201),
        "--operation",
        "export-agasharu",
        "--output",
        "json",
    ]);
    assert_eq!(invalid.envelope["error"]["code"], "sync_invalid_input");
    let absent = ds(&[
        "desktop",
        "sync",
        "published",
        "--project",
        "project",
        "--operation",
        "export-agasharu",
        "--output-id",
        "pdf__huye",
        "--output",
        "json",
    ]);
    assert_eq!(absent.envelope["error"]["code"], "desktop_unreachable");
}

#[test]
fn sync_sanitation_bounds_and_digest_are_checked_before_pairing() {
    let preview = ds(&[
        "desktop",
        "sync",
        "sanitize",
        "preview",
        "--project",
        "project",
        "--limit",
        "201",
        "--output",
        "json",
    ]);
    assert_eq!(preview.envelope["error"]["code"], "sync_invalid_input");
    let apply = ds(&[
        "desktop",
        "sync",
        "sanitize",
        "apply",
        "--project",
        "project",
        "--digest",
        "not-a-digest",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(apply.envelope["error"]["code"], "sync_invalid_input");
    let confirm = ds(&[
        "desktop",
        "sync",
        "sanitize",
        "apply",
        "--project",
        "project",
        "--digest",
        &"a".repeat(64),
        "--output",
        "json",
    ]);
    assert_eq!(confirm.envelope["error"]["code"], "confirmation_required");
}

#[test]
fn native_sync_recovery_is_project_explicit_and_validates_before_server_io() {
    for (id, effect) in [
        ("core.sync.status", "read_only"),
        ("core.sync.retry", "global_write"),
        ("core.sync.sanitize.preview", "read_only"),
        ("core.sync.sanitize.apply", "global_write"),
    ] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let command = &descriptor["command"];
        assert_eq!(command["effect"], effect);
        assert_eq!(command["requires"], "server");
        let project = command["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|input| input["name"] == "project")
            .unwrap();
        assert_eq!(project["required"], true);
        assert!(project["default"].is_null());
    }
    for action in [vec!["status"], vec!["sanitize", "preview"]] {
        let mut args = vec!["core", "sync"];
        args.extend(action);
        args.extend(["--project", "project", "--limit", "201", "--output", "json"]);
        assert_eq!(refusal(&args), "sync_invalid_input");
    }
    assert_eq!(
        refusal(&[
            "core",
            "sync",
            "retry",
            "--project",
            "project",
            "--row",
            "publication",
            "--output",
            "json"
        ]),
        "confirmation_required"
    );
    assert_eq!(
        refusal(&[
            "core",
            "sync",
            "retry",
            "--project",
            "project",
            "--row",
            "../publication",
            "--yes",
            "--output",
            "json"
        ]),
        "sync_invalid_input"
    );
    assert_eq!(
        refusal(&[
            "core",
            "sync",
            "sanitize",
            "apply",
            "--project",
            "project",
            "--digest",
            "not-a-digest",
            "--yes",
            "--output",
            "json"
        ]),
        "sync_invalid_input"
    );
    assert_eq!(
        refusal(&[
            "core",
            "sync",
            "sanitize",
            "apply",
            "--project",
            "project",
            "--digest",
            &"a".repeat(64),
            "--output",
            "json"
        ]),
        "confirmation_required"
    );
}

#[test]
fn solar_local_clean_is_project_explicit_confirmed_and_has_no_recovery_options() {
    let command = ok(&["capabilities", "solar.project.clean", "--output", "json"]);
    assert_eq!(command["command"]["effect"], "global_write");
    let inputs = command["command"]["inputs"].as_array().unwrap();
    assert!(inputs.iter().any(|input| input["name"] == "project" && input["required"] == true));
    assert!(!inputs.iter().any(|input| input["name"] == "digest" || input["name"] == "action"));
    assert_eq!(refusal(&["solar", "project", "clean", "--workspace", "missing", "--project", "project", "--output", "json"]), "confirmation_required");
    let schema = ds_cli_server::solar_application_schema(Some("project_clean")).unwrap();
    assert!(schema.to_string().contains("project_id"));
}

#[test]
fn printing_lifecycle_writes_are_registered_and_stop_before_native_authority() {
    for args in [
        vec![
            "report",
            "layout",
            "create",
            "--scope",
            "global",
            "--request",
            "missing.json",
            "--output",
            "json",
        ],
        vec![
            "report",
            "layout",
            "update",
            "--scope",
            "project",
            "--request",
            "missing.json",
            "--output",
            "json",
        ],
        vec![
            "report",
            "layout",
            "delete",
            "--scope",
            "global",
            "--id",
            "sample",
            "--expected-revision",
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "--output",
            "json",
        ],
        vec![
            "report",
            "layout",
            "copy",
            "--request",
            "missing.json",
            "--output",
            "json",
        ],
    ] {
        let run = native_ds(&args);
        assert_eq!(run.code, 2, "{}", args.join(" "));
        assert_eq!(
            run.envelope["error"]["code"],
            "confirmation_required",
            "{} crossed confirmation into file or native authority",
            args.join(" ")
        );
    }
}

#[test]
fn printing_context_seeding_is_bound_to_one_visible_desktop_project() {
    let descriptor = ok(&[
        "capabilities",
        "desktop.printing.seed-context",
        "--output",
        "json",
    ]);
    let command = &descriptor["command"];
    assert_eq!(command["authority"], "desktop_user");
    assert_eq!(command["effect"], "local_file_write");
    let inputs = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("input name"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        inputs,
        BTreeSet::from(["desktop-descriptor", "target", "transformer"])
    );
    assert!(
        !inputs.contains("project"),
        "printing context seeding must use the paired desktop's visible project"
    );

    let descriptor_path = temp_root("printing-seed-unreachable")
        .join("session.json")
        .display()
        .to_string();
    let code = refusal(&[
        "desktop",
        "printing",
        "seed-context",
        "--transformer",
        "T-1042",
        "--desktop-descriptor",
        &descriptor_path,
        "--output",
        "json",
    ]);
    assert!(
        PAIRING_CODES.contains(&code.as_str()),
        "a valid seed request must reach the paired project's owner, got `{code}`"
    );
}

#[test]
fn printing_artifact_access_is_exact_project_owned_and_copy_only_creates_a_new_file() {
    let read = ok(&[
        "capabilities",
        "desktop.printing.artifact.read",
        "--output",
        "json",
    ]);
    assert_eq!(read["command"]["authority"], "desktop_user");
    assert_eq!(read["command"]["effect"], "read_only");
    let copy = ok(&[
        "capabilities",
        "desktop.printing.artifact.copy",
        "--output",
        "json",
    ]);
    assert_eq!(copy["command"]["authority"], "desktop_user");
    assert_eq!(copy["command"]["effect"], "local_file_write");
    let names = copy["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|value| value["name"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "desktop-descriptor",
            "intent",
            "out",
            "output-id",
            "project",
            "target",
            "transformer"
        ])
    );
    for command in [&read["command"], &copy["command"]] {
        let intent = command["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|arg| arg["name"] == "intent")
            .unwrap();
        assert_eq!(intent["choices"], json!(["preview"]));
        assert_eq!(intent["required"], false);
        assert!(intent.get("default").is_none());
    }
}

#[test]
fn desktop_sync_exposes_path_free_status_and_guards_exact_row_retry() {
    let status = ok(&["capabilities", "desktop.sync.status", "--output", "json"]);
    let command = &status["command"];
    assert_eq!(command["authority"], "desktop_user");
    assert_eq!(command["effect"], "read_only");
    let inputs = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("input name"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        inputs,
        BTreeSet::from(["desktop-descriptor", "limit", "project", "target"])
    );

    let run = native_ds(&[
        "desktop",
        "sync",
        "retry",
        "--project",
        "project-1",
        "--row",
        "compute:uid-1:network_reporter:batch-1:default",
        "--output",
        "json",
    ]);
    assert_eq!(run.code, 2);
    assert_eq!(run.envelope["error"]["code"], "confirmation_required");
}




// ---------------------------------------------------------------------------
// capabilities --requires
// ---------------------------------------------------------------------------

/// The aggregate an operator asks before trusting a server: what still needs
/// the window, and is this domain clear?
///
/// Asserted against named commands on both sides, because a filter that
/// returns *something* proves nothing — the bug this shape of code actually
/// produces is a comparison that matches everything or nothing.
#[test]
fn capabilities_requires_separates_the_window_from_the_server() {
    // The lens domain is the one that must come back, and its page is bounded
    // and reported, not silently cut.
    let window = ok(&[
        "capabilities",
        "map",
        "--requires",
        "window",
        "--limit",
        "5",
        "--output",
        "json",
    ]);
    assert_eq!(window["tier"], "requires");
    assert_eq!(window["requires"], "window");
    assert_eq!(window["domain"], "map");
    // 46 since the paired Profile gained `map.profile.results` (8a776a32) and
    // `map.profile.calculation` (3355763b): both read or drive the live
    // Profile in the window, behind `paired_availability`.
    // Owner-requested live RAM controls are a window intent. Engineering and
    // full-history admission remain the headless native engine's authority.
    // 49 since `map.profile.pick` (a43c9c95) reads the mounted Profile pick.
    // 48 since the 3D map and `map.renderer.configure` were removed.
    assert_eq!(window["matched"], 48, "map's window commands");
    let ids: Vec<&str> = window["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|row| row["id"].as_str().expect("id"))
        .collect();
    assert_eq!(ids.len(), 5, "--limit is honoured");
    assert!(
        ids.contains(&"map.camera.set"),
        "moving the camera is the window's own work: {ids:?}"
    );
    assert_eq!(window["more"]["shown"], 5);
    assert_eq!(window["more"]["matched"], 48);

    // Survey moved to the server. If a survey command ever needs the window
    // again, this is where it is noticed.
    let survey = ok(&[
        "capabilities",
        "survey",
        "--requires",
        "window",
        "--output",
        "json",
    ]);
    assert_eq!(survey["matched"], 0, "survey is server-first");
    assert!(
        survey["results"].as_array().expect("results").is_empty(),
        "a domain with no window commands returns none: {survey}"
    );
    assert!(
        survey["more"].is_null(),
        "nothing was truncated, so nothing is reported as truncated"
    );

    // …and the same command appears on the server side, so "0 window" is an
    // answer about where it runs, not a command that vanished from the filter.
    let headless = ok(&[
        "capabilities",
        "survey",
        "--requires",
        "server",
        "--limit",
        "50",
        "--output",
        "json",
    ]);
    let headless_ids: Vec<&str> = headless["results"]
        .as_array()
        .expect("results")
        .iter()
        .map(|row| row["id"].as_str().expect("id"))
        .collect();
    assert!(
        headless_ids.contains(&"survey.form.read"),
        "reading a form is a server read: {headless_ids:?}"
    );

    // Every command in a domain is one or the other, and the descriptor
    // carries the same fact the filter sorted on.
    let index = ok(&["capabilities", "survey", "--output", "json"]);
    let registered = index["commands"].as_array().expect("commands").len();
    assert_eq!(
        headless["matched"].as_u64().expect("matched") as usize,
        registered,
        "survey's whole index answers on the server"
    );
    let descriptor = ok(&["capabilities", "survey.form.read", "--output", "json"]);
    assert_eq!(descriptor["command"]["requires"], "server");
    let paired = ok(&["capabilities", "map.camera.set", "--output", "json"]);
    assert_eq!(paired["command"]["requires"], "window");

    // One question at a time: two filters over one set would leave the caller
    // guessing which one shaped the answer.
    assert_eq!(
        refusal(&[
            "capabilities",
            "--requires",
            "window",
            "--search",
            "map",
            "--output",
            "json",
        ]),
        "conflicting_selector"
    );
    assert_eq!(
        refusal(&[
            "capabilities",
            "map.camera.set",
            "--requires",
            "window",
            "--output",
            "json",
        ]),
        "conflicting_selector",
        "a command id already carries `requires` in its own descriptor"
    );
}

fn ok(args: &[&str]) -> Value {
    let run = ds(args);
    assert_eq!(
        run.code,
        0,
        "`ds {}` exited {}: {}{}",
        args.join(" "),
        run.code,
        run.stdout,
        run.stderr
    );
    run.envelope["data"].clone()
}

fn temp_root(label: &str) -> PathBuf {
    static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "ds-cli-{label}-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ))
}

// ---------------------------------------------------------------------------
// workstation
// ---------------------------------------------------------------------------

#[test]
fn workstation_windows_libreoffice_plan_is_explicitly_mutation_free() {
    let data = ok(&[
        "workstation",
        "plan",
        "--component",
        "libreoffice",
        "--platform",
        "windows",
        "--output",
        "json",
    ]);
    assert_eq!(data["mutated"], false);
    assert_eq!(data["authorized"], false);
    assert_eq!(data["implementation"], "available");
    assert_eq!(
        data["constraints"]["windows_libreoffice_lifecycle_proven"],
        true
    );
    assert_eq!(data["evidence"]["state"], "proven");
    assert!(
        data["steps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|step| step.as_str().unwrap_or("").contains("SHA-256"))
    );
}

#[test]
fn workstation_rwanda_acquisition_plan_names_the_receipt_proof() {
    let data = ok(&[
        "workstation",
        "plan",
        "--component",
        "rwanda-reference",
        "--output",
        "json",
    ]);
    assert_eq!(data["implementation"], "available");
    assert!(data["steps"].as_array().unwrap().iter().any(|step| {
        let step = step.as_str().unwrap_or("");
        step.contains("license") && step.contains("SHA-256")
    }));
}

#[cfg(not(windows))]
#[test]
fn workstation_install_refuses_an_unproven_platform_component() {
    let run = ds(&[
        "workstation",
        "install",
        "--component",
        "git-bash",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(run.code, 3, "{}{}", run.stdout, run.stderr);
    assert_eq!(
        run.envelope["error"]["code"],
        "workstation_mutation_unsupported"
    );
}

#[cfg(not(windows))]
#[test]
fn workstation_configure_refuses_outside_native_windows() {
    let run = ds(&[
        "workstation",
        "configure",
        "--component",
        "git-bash",
        "--target",
        "vscode",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(run.code, 3, "{}{}", run.stdout, run.stderr);
    assert_eq!(
        run.envelope["error"]["code"],
        "workstation_mutation_unsupported"
    );
}

// On the platform where these commands can change a machine, their smoke
// coverage must not disappear. Deliberately omit --yes: this exercises the
// same dispatch choke point without installing or configuring anything.
#[cfg(windows)]
#[test]
fn workstation_install_is_confirmation_gated_on_native_windows() {
    let run = ds(&[
        "workstation",
        "install",
        "--component",
        "git-bash",
        "--output",
        "json",
    ]);
    assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
    assert_eq!(run.envelope["error"]["code"], "confirmation_required");
}

#[cfg(windows)]
#[test]
fn workstation_configure_is_confirmation_gated_on_native_windows() {
    let run = ds(&[
        "workstation",
        "configure",
        "--component",
        "git-bash",
        "--target",
        "vscode",
        "--output",
        "json",
    ]);
    assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
    assert_eq!(run.envelope["error"]["code"], "confirmation_required");
}

// ---------------------------------------------------------------------------
// dsgrid
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// library
// ---------------------------------------------------------------------------












/// The real package's assets: the original PLS-CADD upload comes out
/// byte-for-byte as the committed backup, a delivered backup attaches bound
/// to the snapshot and detaches back to the same model and asset contents, and the model's own
/// files are never writable.


/// A governed revision's assets need the project owner, but an output that
/// already exists is refused before any credential or network, and a
/// signed-out machine gets a code the command declares.








// ---------------------------------------------------------------------------
// dsgrid model — the paired application's local model lifecycle
// ---------------------------------------------------------------------------
//
// A local model lives inside a running application, so there is no fixture
// that can stand in for one: an assertion about which model occupies Profile
// needs the desktop, and the desktop is not present on CI.
//
// What *is* assertable everywhere is the boundary this family exists to hold,
// and it is a vocabulary boundary — the one a reverted `dsgrid model
// create/import/convert` family got wrong. Each check below is a negative
// control for one thing that family conflated, and every one of them holds on
// a machine with no application at all.















/// Client GIS files (owner rule, 2026-09-29). A real model's KMZ and
/// Shapefile ZIP carry no tension sections, no terrain points and no internal
/// exchange report unless asked; `--include-layer` brings a layer back, and a
/// layer id the projection does not have is refused before anything runs.








// ---------------------------------------------------------------------------
// pls
// ---------------------------------------------------------------------------




















// ---------------------------------------------------------------------------
// pls desktop
// ---------------------------------------------------------------------------

/// The catalogue answers on any host, and the decisions the desktop verbs
/// rely on are read back from the one embedded in this build: Exit answers
/// "Save changes" No, AutoSag goes through the Section Table, the Repair
/// Wizard stops a run.


/// Off Windows every verb that drives PLS-CADD refuses `windows_only` at the
/// dispatch gate, before it validates a path, extracts a driver or creates a
/// folder — and says what to do instead.


/// The toolkit is the one desktop verb that only writes files: on this host
/// it writes every embedded adapter, its manifest pins match what it wrote,
/// the Word adapter is where the PLS-CADD drivers call it, and a second run
/// into the same folder refuses rather than overwriting.




// ---------------------------------------------------------------------------
// Cross-cutting
// ---------------------------------------------------------------------------



// ---------------------------------------------------------------------------
// data elevation
// ---------------------------------------------------------------------------

#[test]
fn admin_boundary_scope_is_validated_before_the_authority_is_reached() {
    let cases: &[&[&str]] = &[
        &[
            "data",
            "admin-bounds",
            "list",
            "--country",
            "kenya",
            "--level",
            "province",
        ],
        &["data", "admin-bounds", "list", "--level", "county"],
        &[
            "data",
            "admin-bounds",
            "list",
            "--level",
            "sector",
            "--parent-code",
            "1",
        ],
        &[
            "data",
            "admin-bounds",
            "list",
            "--level",
            "province",
            "--parent-code",
            "1",
        ],
        &["data", "admin-bounds", "read", "--code", "110"],
        &["data", "admin-bounds", "read", "--code", "11x2"],
    ];
    for args in cases {
        let mut argv = args.to_vec();
        argv.extend(["--output", "json"]);
        let run = native_ds(&argv);
        assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
        assert_eq!(
            run.envelope["error"]["code"],
            "invalid_admin_scope",
            "`ds {}` reached the authority before exact scope validation",
            args.join(" ")
        );
    }
}

#[test]
fn the_national_hierarchy_is_read_without_a_window() {
    // A country's boundaries have no project and nothing to render. Both reads
    // stop at the native identity boundary, which is the proof they are asking
    // the gateway rather than looking for a paired application.
    for descriptor in ["data.admin-bounds.list", "data.admin-bounds.read"] {
        let command = &ok(&["capabilities", descriptor, "--output", "json"])["command"];
        assert_eq!(command["authority"], "headless_user");
        let inputs = command["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .map(|input| input["name"].as_str().expect("input name"))
            .collect::<BTreeSet<_>>();
        assert!(inputs.contains("lane"), "{descriptor} cannot choose a lane");
        assert!(
            !inputs.contains("desktop-descriptor") && !inputs.contains("to-map"),
            "{descriptor} still carries a window input: {inputs:?}"
        );
    }
    for argv in [
        vec![
            "data",
            "admin-bounds",
            "list",
            "--level",
            "village",
            "--parent-code",
            "110101",
            "--output",
            "json",
        ],
        vec![
            "data",
            "admin-bounds",
            "read",
            "--code",
            "11010102",
            "--output",
            "json",
        ],
    ] {
        let code = native_refusal(&argv);
        assert!(
            NATIVE_AUTH_CODES.contains(&code.as_str()),
            "an exact hierarchy read stopped at `{code}`, not at the national authority"
        );
    }
    // `--geometry-out` is settled before anything is read, so an occupied path
    // is answered without a credential and without a request.
    let taken = temp_root("admin-bounds-geometry").join("boundary.geojson");
    std::fs::create_dir_all(taken.parent().expect("parent")).expect("a temporary directory");
    std::fs::write(&taken, b"{}").expect("a file in the way");
    assert_eq!(
        native_refusal(&[
            "data",
            "admin-bounds",
            "read",
            "--code",
            "11010102",
            "--geometry-out",
            taken.to_str().expect("path"),
            "--output",
            "json",
        ]),
        "output_refused"
    );
    let _ = std::fs::remove_dir_all(taken.parent().expect("parent"));
}

#[test]
fn native_elevation_validates_absolute_paths_before_pairing() {
    let absolute_source = temp_root("elevation-source")
        .join("points.csv")
        .display()
        .to_string();
    let absolute_out = temp_root("elevation-out")
        .join("points.geojson")
        .display()
        .to_string();
    for args in [
        vec![
            "data",
            "elevation",
            "attach",
            "--source",
            "points.csv",
            "--out",
            absolute_out.as_str(),
            "--output",
            "json",
        ],
        vec![
            "data",
            "elevation",
            "attach",
            "--source",
            absolute_source.as_str(),
            "--out",
            "points.geojson",
            "--output",
            "json",
        ],
    ] {
        let run = ds(&args);
        assert_eq!(run.code, 2, "{}{}", run.stdout, run.stderr);
        assert_eq!(run.envelope["error"]["code"], "absolute_path_required");
    }
}

// ---------------------------------------------------------------------------
// map
// ---------------------------------------------------------------------------
//
// The map lives inside a running application, so there is no fixture that can
// stand in for it: an assertion about what a layer looks like on screen needs
// the desktop, and the desktop is not present on CI.
//
// What *is* assertable everywhere, and is where this domain's real bugs would
// live, is the input contract. Every handler here validates its own flags
// before it opens the bridge, so a malformed invocation must refuse with the
// same typed code whether or not an application is running. That is not a
// shape test: it is the ordering claim the whole domain rests on, and it is
// exactly what would break if a handler were rewritten to resolve the paired
// session first — after which none of these codes would ever be seen by
// anyone without the desktop installed.

/// A features file holding one line, for the geometry checks.
fn line_geojson() -> String {
    let root = temp_root("map-line");
    std::fs::create_dir_all(&root).expect("temp directory is writable");
    let path = root.join("line.geojson");
    std::fs::write(
        &path,
        r#"{"type":"FeatureCollection","features":[
            {"type":"Feature","properties":{"name":"a"},
             "geometry":{"type":"LineString","coordinates":[[30.0,-1.9],[30.1,-1.95]]}}]}"#,
    )
    .expect("temp geojson is writable");
    path.display().to_string()
}

/// The refusal code a call came back with, or "" if it succeeded.
fn refusal(args: &[&str]) -> String {
    let run = ds(args);
    if run.envelope["status"] == "ok" {
        return String::new();
    }
    run.envelope["error"]["code"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

/// The pairing states a well-formed map call may legitimately end in. Which
/// one depends on the machine; that it is one of these does not.
const PAIRING_CODES: &[&str] = &[
    "auth_context_mismatch",
    "desktop_not_paired",
    "desktop_ambiguous",
    "desktop_unreachable",
    "desktop_unreadable",
    "desktop_refused",
    "desktop_operation_unsupported",
    "desktop_signed_out",
    "pairing_rejected",
    // Since instances replaced install profiles as the unit of pairing: the
    // machine may hold several live instances, and a saved project selection
    // that none of them has open is refused rather than switched into one.
    "desktop_project_not_open",
    "desktop_target_not_live",
    "desktop_target_mismatch",
];

// ---------------------------------------------------------------------------
// Desktop instances: two live runtimes, on one machine that is not this one
// ---------------------------------------------------------------------------

/// A machine with its own app-data root and its own live instances.
///
/// Every descriptor `ds` can find here is one this test wrote, and every
/// endpoint it can reach is one of these listeners: `XDG_DATA_HOME` is
/// redirected, so the operator's own DS GridDesign is invisible to it and is
/// never probed, contacted or counted. That is the standing rule for this
/// suite, and instance routing is exactly the surface that would break it.
struct Machine {
    root: PathBuf,
    instances: Vec<Instance>,
}

struct Instance {
    id: String,
    port: u16,
    /// Every `/v1/invoke` this instance received: the operation, and the fence
    /// it arrived with. What a routing test proves is which instance received
    /// what — and, just as much, which one received nothing.
    received: Arc<Mutex<Vec<Value>>>,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Machine {
    fn new(label: &str) -> Self {
        let root = temp_root(label);
        std::fs::create_dir_all(&root).expect("app data root");
        Self {
            root,
            instances: Vec::new(),
        }
    }

    /// One live instance: a descriptor in the registry directory, and a
    /// listener that answers the authenticated handshake as the shell does.
    fn live(&mut self, id: &str, project: &str) -> &Instance {
        self.instance(id, project, true, true)
    }

    /// A descriptor whose endpoint answers 401: the port was taken by
    /// something that is not this bridge, or the app died and something else
    /// reused it. Live to a connection, dead to a handshake.
    fn stale(&mut self, id: &str) -> &Instance {
        self.instance(id, "project-gone", false, true)
    }

    fn instance(&mut self, id: &str, project: &str, paired: bool, publish: bool) -> &Instance {
        let session = json!({
            "session_revision": 4,
            "map_revision": 1,
            "connected": true,
            "signed_in": true,
            "uid": "uid-a",
            "lane": "canary",
            "credential_audience_sha256": "c".repeat(64),
            "project": project,
            "instance_id": id,
            "build": "2026.9.12+1",
            "started_at_ms": 1_757_000_000_000u64,
            "map": { "open": true, "layers": [] },
            "windows": [{ "label": "main", "project": project, "generation": 2 }],
        });
        let token = format!("{id}-token-{id}");
        let instance = Instance::start(id, &token, session, paired);
        if publish {
            let directory = self
                .root
                .join("rw.datasolutions.desktop.canary")
                .join("cli-bridge.d");
            std::fs::create_dir_all(&directory).expect("registry directory");
            std::fs::write(
                directory.join(format!("{id}.json")),
                serde_json::to_vec(&json!({
                    "version": 1,
                    "url": format!("http://127.0.0.1:{}", instance.port),
                    "token": token,
                    "pid": 4711,
                    "instance_id": id,
                    "lane": "canary",
                    "build": "2026.9.12+1",
                    "started_at_ms": 1_757_000_000_000u64,
                }))
                .expect("descriptor encodes"),
            )
            .expect("descriptor");
        }
        self.instances.push(instance);
        self.instances.last().expect("just pushed")
    }

    fn instance_by(&self, id: &str) -> &Instance {
        self.instances
            .iter()
            .find(|instance| instance.id == id)
            .expect("an instance this machine started")
    }

    /// `ds`, run against this machine and no other. `DS_DESKTOP_DESCRIPTOR` is
    /// removed rather than left unset: the suite's default points every other
    /// test at a descriptor that does not exist, and these tests are about
    /// what automatic enumeration finds.
    fn ds(&self, args: &[&str]) -> Run {
        self.ds_with(&[], args)
    }

    /// The same, for a session that set a default in its environment.
    fn ds_with(&self, environment: &[(&str, &str)], args: &[&str]) -> Run {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
        command
            .args(args)
            .env("NO_COLOR", "1")
            .env("XDG_DATA_HOME", &self.root)
            .env_remove("DS_DESKTOP_DESCRIPTOR")
            .env_remove("DS_TARGET");
        for (name, value) in environment {
            command.env(name, value);
        }
        let output = command.output().expect("ds binary runs");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        Run {
            envelope: serde_json::from_str(&stdout).unwrap_or(Value::Null),
            stdout,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            code: output.status.code().unwrap_or(-1),
        }
    }
}

impl Drop for Machine {
    fn drop(&mut self) {
        for instance in &mut self.instances {
            instance.stop.store(true, Ordering::Relaxed);
            if let Some(worker) = instance.worker.take() {
                let _ = worker.join();
            }
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Instance {
    fn start(id: &str, token: &str, session: Value, paired: bool) -> Self {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("address").port();
        listener.set_nonblocking(true).expect("nonblocking");
        let received = Arc::new(Mutex::new(Vec::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let worker = {
            let received = Arc::clone(&received);
            let stop = Arc::clone(&stop);
            let token = token.to_owned();
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => serve(stream, &token, &session, paired, &received),
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            std::thread::sleep(std::time::Duration::from_millis(5));
                        }
                        Err(_) => break,
                    }
                }
            })
        };
        Self {
            id: id.to_owned(),
            port,
            received,
            stop,
            worker: Some(worker),
        }
    }

    fn operations(&self) -> Vec<String> {
        self.received
            .lock()
            .expect("received")
            .iter()
            .filter_map(|body| body["operation"].as_str().map(str::to_owned))
            .collect()
    }
}

/// One request, answered the way the shell answers it: the authenticated
/// session, or the operation's own result. Anything without the pairing secret
/// is 401, which is what makes a stale descriptor stale.
fn serve(
    mut stream: std::net::TcpStream,
    token: &str,
    session: &Value,
    paired: bool,
    received: &Arc<Mutex<Vec<Value>>>,
) {
    use std::io::{BufRead, BufReader, Read, Write};

    // The listener polls for connections without blocking; on Linux the
    // accepted socket inherits that, and a read that returns `WouldBlock`
    // before the request has arrived looks to a client exactly like a bridge
    // that hung up. This connection is served synchronously.
    stream.set_nonblocking(false).expect("blocking connection");
    let mut reader = BufReader::new(stream.try_clone().expect("clone"));
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    let mut length = 0usize;
    let mut authorized = false;
    loop {
        let mut line = String::new();
        // The end of the headers is an empty line, and a client that hung up
        // reads as one too: either way there is nothing more to read here.
        if reader.read_line(&mut line).is_err() || line.trim().is_empty() {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if let Some(value) = lower.strip_prefix("content-length:") {
            length = value.trim().parse().unwrap_or(0);
        }
        if lower.starts_with("authorization:") {
            authorized = line.trim().ends_with(token);
        }
    }
    let mut body = vec![0_u8; length];
    if length > 0 && reader.read_exact(&mut body).is_err() {
        return;
    }
    let (status, payload) = if !authorized || !paired {
        (401, json!({ "error": "pairing_required" }))
    } else if request.starts_with("POST /v1/invoke") {
        let invocation: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
        let receipt = if invocation["operation"] == "map.grid.lasso" {
            json!({"model_id":"model-a", "revision":"rev-b", "selection":{"entity_ids":["pole-2","pole-1"],"primary":"pole-2","kind":"structures","structures":[{"id":"pole-2","number":2},{"id":"pole-1","number":1}]}, "native_marker":"window-receipt"})
        } else {
            json!({ "ok": true })
        };
        received.lock().expect("received").push(invocation);
        (200, receipt)
    } else {
        (200, session.clone())
    };
    let payload = serde_json::to_string(&payload).expect("encodes");
    let _ = write!(
        stream,
        "HTTP/1.1 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
        if status == 200 { "OK" } else { "Unauthorized" },
        payload.len()
    );
}

const INSTANCE_ONE: &str = "11111111111111111111111111111111";
const INSTANCE_TWO: &str = "22222222222222222222222222222222";
const INSTANCE_THREE: &str = "33333333333333333333333333333333";

#[test]
fn grid_lasso_sends_one_declared_query_and_returns_the_exact_window_receipt() {
    let mut machine = Machine::new("grid-lasso");
    machine.live(INSTANCE_ONE, "project-held-by-window");
    let contract = machine.ds(&["capabilities", "map.grid.lasso", "--output", "json"]);
    let command = &contract.envelope["data"]["command"];
    assert_eq!(command["effect"], "local_ui");
    assert_eq!(command["authority"], "desktop_pairing");
    assert_eq!(command["requires"], "window");
    let input_names: BTreeSet<_> = command["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|arg| arg["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        input_names,
        BTreeSet::from([
            "model",
            "revision",
            "space",
            "polygon",
            "predicate",
            "family",
            "filter",
            "mode",
            "action",
            "all-layers",
            "target",
            "desktop-descriptor"
        ])
    );
    for (space, polygon) in [
        ("profile", "[[0,0],[1,0],[1,1]]"),
        (
            "plan",
            r#"{"type":"Polygon","coordinates":[[[30,-2],[30.1,-2],[30.1,-1.9],[30,-2]]]}"#,
        ),
    ] {
        let run = machine.ds(&[
            "map",
            "grid",
            "lasso",
            "--model",
            "model-a",
            "--revision",
            "rev-b",
            "--space",
            space,
            "--polygon",
            polygon,
            "--family",
            "structures",
            "--output",
            "json",
        ]);
        assert_eq!(run.code, 0, "{} {}", run.stdout, run.stderr);
        assert_eq!(
            run.envelope["data"],
            json!({"model_id":"model-a", "revision":"rev-b", "selection":{"entity_ids":["pole-2","pole-1"],"primary":"pole-2","kind":"structures","structures":[{"id":"pole-2","number":2},{"id":"pole-1","number":1}]}, "native_marker":"window-receipt"})
        );
    }
    let received = machine.instance_by(INSTANCE_ONE).received.lock().unwrap();
    assert_eq!(received.len(), 2);
    for (invocation, space) in received.iter().zip(["profile", "plan"]) {
        assert_eq!(invocation["operation"], "map.grid.lasso");
        let payload = &invocation["arguments"];
        assert_eq!(payload["model_id"], "model-a");
        assert_eq!(payload["expected_revision"], "rev-b");
        assert_eq!(payload["space"], space);
        assert_eq!(payload["families"], json!(["structures"]));
        // Omitted controls use the native owner's defaults or remembered
        // choices, rather than a second default supplied by the CLI.
        assert!(payload.get("predicate").is_none());
        assert!(payload.get("mode").is_none());
        assert_eq!(
            ds_cli_desktop::ops::undeclared_key(&ds_cli_map::GRID_LASSO, payload),
            None
        );
        assert!(payload.get("axis_pin_digest").is_none());
        assert!(payload.get("project").is_none());
    }
}

#[test]
fn grid_lasso_invalid_inputs_never_pair_or_invoke() {
    let mut machine = Machine::new("grid-lasso-invalid");
    machine.live(INSTANCE_ONE, "project-a");
    for (extra, code) in [
        (vec!["--polygon", "[[0,0],[1,1]]"], "invalid_grid_lasso"),
        (
            vec![
                "--polygon",
                "[[0,0],[1,0],[1,1]]",
                "--filter",
                "{\"rows\":[]}",
            ],
            "invalid_grid_lasso",
        ),
        (
            vec!["--polygon", "[[0,0],[1,0],[1,1]]", "--mode", "toggle"],
            "invalid_choice",
        ),
    ] {
        let mut args = vec![
            "map",
            "grid",
            "lasso",
            "--model",
            "model-a",
            "--revision",
            "rev-b",
            "--space",
            "profile",
            "--desktop-descriptor",
            "/missing/lasso-descriptor.json",
            "--output",
            "json",
        ];
        args.extend(extra);
        let run = machine.ds(&args);
        assert_eq!(run.envelope["error"]["code"], code, "{}", run.stdout);
    }
    assert!(machine.instance_by(INSTANCE_ONE).operations().is_empty());
}

#[test]
fn two_live_instances_refuse_without_an_explicit_target() {
    let mut machine = Machine::new("desktop-two-instances");
    machine.live(INSTANCE_ONE, "project-a");
    machine.live(INSTANCE_TWO, "project-b");

    // Both are enumerated, ordered by identity and never by which file was
    // read first, and neither is described with anything secret.
    let listed = machine.ds(&["desktop", "list", "--output", "json"]);
    assert_eq!(listed.code, 0, "{}", listed.stderr);
    let data = &listed.envelope["data"];
    assert_eq!(data["live"], 2, "{}", listed.stdout);
    assert_eq!(data["instances"][0]["instance_id"], INSTANCE_ONE);
    assert_eq!(data["instances"][1]["instance_id"], INSTANCE_TWO);
    assert_eq!(data["instances"][0]["project"], "project-a");
    assert_eq!(data["instances"][1]["state"], "ready");
    for secret in ["token", "127.0.0.1", "uid-a", "credential_audience"] {
        assert!(
            !listed.stdout.contains(secret),
            "`ds desktop list` named `{secret}`: {}",
            listed.stdout
        );
    }

    // And an operation that could go to either goes to neither.
    let refused = machine.ds(&["desktop", "status", "--output", "json"]);
    assert_eq!(
        refused.envelope["error"]["code"], "desktop_ambiguous",
        "{}",
        refused.stdout
    );
    let named: Vec<&str> = refused.envelope["error"]["detail"]["instances"]
        .as_array()
        .expect("the choices")
        .iter()
        .filter_map(|instance| instance["instance_id"].as_str())
        .collect();
    assert_eq!(named, vec![INSTANCE_ONE, INSTANCE_TWO]);

    // Naming one settles it, and the answer is about that instance.
    let one = machine.ds(&[
        "desktop",
        "status",
        "--target",
        &format!("desktop:{INSTANCE_TWO}"),
        "--output",
        "json",
    ]);
    assert_eq!(one.code, 0, "{}", one.stderr);
    assert_eq!(one.envelope["data"]["instance"], INSTANCE_TWO);
    assert_eq!(one.envelope["data"]["project"], "project-b");

    // A session can name it once instead of on every command — and the flag
    // still wins, because a default that overrode what a caller typed would
    // not be a default.
    let session = machine.ds_with(
        &[("DS_TARGET", &format!("desktop:{INSTANCE_ONE}"))],
        &["desktop", "status", "--output", "json"],
    );
    assert_eq!(session.code, 0, "{}", session.stderr);
    assert_eq!(session.envelope["data"]["instance"], INSTANCE_ONE);
    let overridden = machine.ds_with(
        &[("DS_TARGET", &format!("desktop:{INSTANCE_ONE}"))],
        &[
            "desktop",
            "status",
            "--target",
            &format!("desktop:{INSTANCE_TWO}"),
            "--output",
            "json",
        ],
    );
    assert_eq!(overridden.envelope["data"]["instance"], INSTANCE_TWO);

    // And a host that does not perform this operation is answered by name
    // rather than by routing it somewhere that would.
    let elsewhere = machine.ds_with(
        &[("DS_TARGET", "core")],
        &["desktop", "status", "--output", "json"],
    );
    assert_eq!(
        elsewhere.envelope["error"]["code"], "target_host_unsupported",
        "{}",
        elsewhere.stdout
    );
}

/// Feedback 641143eb: `desktop_ambiguous` tells the caller to name an instance
/// with `--target desktop:<instance_id>`, and the design commands used to
/// refuse that flag as unknown. Every one of them takes it now, the named
/// instance alone receives the work, and `--desktop-descriptor` still works.
#[test]
fn the_design_commands_take_the_target_their_ambiguity_refusal_names() {
    let mut machine = Machine::new("design-target");
    machine.live(INSTANCE_ONE, "project-a");
    machine.live(INSTANCE_TWO, "project-b");
    let descriptor_one = machine
        .root
        .join("rw.datasolutions.desktop.canary")
        .join("cli-bridge.d")
        .join(format!("{INSTANCE_ONE}.json"));

    let commands: [&[&str]; 4] = [
        &["map", "design", "read", "--transformer", "T-1042"],
        &[
            "map",
            "design",
            "select",
            "--transformer",
            "T-1042",
            "--layer",
            "lv_lines",
        ],
        &[
            "map",
            "design",
            "set",
            "--transformer",
            "T-1042",
            "--layer",
            "lv_lines",
            "--id",
            "f1",
            "--set",
            "phase=A",
        ],
        &["map", "design", "save", "--transformer", "T-1042", "--yes"],
    ];
    let target_two = format!("desktop:{INSTANCE_TWO}");
    for command in commands {
        let name = command[..3].join(" ");

        // Nothing names an instance: refused, and the remedy names a flag this
        // very command accepts.
        let mut args = command.to_vec();
        args.extend(["--output", "json"]);
        let refused = machine.ds(&args);
        assert_eq!(
            refused.envelope["error"]["code"], "desktop_ambiguous",
            "{name}: {}",
            refused.stdout
        );
        let remedy = refused.envelope["error"]["remedy"]
            .as_str()
            .expect("remedy");
        assert!(
            remedy.contains("--target desktop:<instance_id>"),
            "{name}: {remedy}"
        );

        // The flag the refusal named is accepted, and routes to that instance.
        let before = machine.instance_by(INSTANCE_TWO).operations().len();
        let mut args = command.to_vec();
        args.extend(["--target", &target_two, "--output", "json"]);
        let routed = machine.ds(&args);
        assert_ne!(
            routed.envelope["error"]["code"], "unknown_flag",
            "{name}: {}",
            routed.stdout
        );
        assert!(
            machine.instance_by(INSTANCE_TWO).operations().len() > before,
            "{name} did not reach the named instance: {}",
            routed.stdout
        );
    }
    assert!(
        machine.instance_by(INSTANCE_ONE).operations().is_empty(),
        "the instance nobody named must receive nothing"
    );

    // The descriptor file keeps working, and it reaches the instance it names.
    let descriptor = descriptor_one.to_string_lossy().into_owned();
    let by_file = machine.ds(&[
        "map",
        "design",
        "read",
        "--transformer",
        "T-1042",
        "--desktop-descriptor",
        &descriptor,
        "--output",
        "json",
    ]);
    assert_ne!(
        by_file.envelope["error"]["code"], "desktop_ambiguous",
        "{}",
        by_file.stdout
    );
    assert!(
        !machine.instance_by(INSTANCE_ONE).operations().is_empty(),
        "--desktop-descriptor must still reach the instance whose file it names"
    );
}

#[test]
fn an_explicit_dead_instance_never_falls_through() {
    let mut machine = Machine::new("desktop-dead-target");
    machine.live(INSTANCE_ONE, "project-a");

    // Exactly one instance is live and would have been the automatic answer.
    // Naming another is still refused, by name, and reaches nothing.
    let refused = machine.ds(&[
        "desktop",
        "status",
        "--target",
        &format!("desktop:{INSTANCE_THREE}"),
        "--output",
        "json",
    ]);
    assert_eq!(
        refused.envelope["error"]["code"], "desktop_target_not_live",
        "{}",
        refused.stdout
    );
    assert_eq!(
        refused.envelope["error"]["detail"]["target"], INSTANCE_THREE,
        "the refusal names the target the caller asked for"
    );
    assert!(
        machine.instance_by(INSTANCE_ONE).operations().is_empty(),
        "the live instance must not have been sent the work of another"
    );

    // A target that is not an instance id at all is refused before anything is
    // read, and under the same vocabulary.
    let malformed = machine.ds(&[
        "desktop",
        "status",
        "--target",
        "desktop:the-second-one",
        "--output",
        "json",
    ]);
    assert_eq!(
        malformed.envelope["error"]["code"], "desktop_target_mismatch",
        "{}",
        malformed.stdout
    );
    // And a host that is not a host at all says so rather than guessing.
    let unknown = machine.ds(&["desktop", "status", "--target", "cloud", "--output", "json"]);
    assert_eq!(unknown.envelope["error"]["code"], "unknown_target");
}

#[test]
fn a_stale_descriptor_is_skipped_only_after_a_verified_handshake() {
    let mut machine = Machine::new("desktop-stale-descriptor");
    // Both descriptors point at a listener that accepts a connection. Only one
    // of them answers the authenticated session request, and a TCP handshake
    // is deliberately not enough to be a candidate: a port an unrelated local
    // process reused must never out-rank the live desktop.
    machine.stale(INSTANCE_TWO);
    machine.live(INSTANCE_ONE, "project-a");

    let listed = machine.ds(&["desktop", "list", "--output", "json"]);
    assert_eq!(listed.code, 0, "{}", listed.stderr);
    assert_eq!(listed.envelope["data"]["live"], 1, "{}", listed.stdout);
    assert_eq!(
        listed.envelope["data"]["instances"][0]["instance_id"],
        INSTANCE_ONE
    );

    // So the sole live instance is not made ambiguous by the leftover, and the
    // leftover is not silently the answer either.
    let status = machine.ds(&["desktop", "status", "--output", "json"]);
    assert_eq!(status.code, 0, "{}", status.stderr);
    assert_eq!(status.envelope["data"]["instance"], INSTANCE_ONE);
    assert_eq!(status.envelope["data"]["project"], "project-a");
}

#[test]
fn the_saved_selection_never_switches_a_live_map() {
    let mut machine = Machine::new("desktop-no-switch");
    machine.live(INSTANCE_ONE, "project-a");

    let zoom = machine.ds(&[
        "map",
        "zoom",
        "--bbox",
        "29.9,-2.1,30.2,-1.85",
        "--output",
        "json",
    ]);
    assert!(
        zoom.code == 0
            || PAIRING_CODES.contains(&zoom.envelope["error"]["code"].as_str().unwrap_or_default()),
        "a well-formed map call ended in `{}`: {}",
        zoom.envelope["error"]["code"],
        zoom.stdout
    );
    // Whatever else happened, the CLI did not move this window's project. The
    // automatic `project.switch` that used to precede map work is retired: a
    // saved CLI selection narrows which instance may serve an operation and is
    // never an instruction to the window a person is looking at.
    let operations = machine.instance_by(INSTANCE_ONE).operations();
    assert!(
        !operations
            .iter()
            .any(|operation| operation == "project.switch"),
        "a map command sent `project.switch` to a live window: {operations:?}"
    );

    // The explicit switch is the one that may, and it is the operation the
    // caller asked for rather than a step before another one.
    let switched = machine.ds(&[
        "desktop",
        "project",
        "switch",
        "--project",
        "project-b",
        "--target",
        &format!("desktop:{INSTANCE_ONE}"),
        "--output",
        "json",
    ]);
    let operations = machine.instance_by(INSTANCE_ONE).operations();
    assert_eq!(
        operations
            .iter()
            .filter(|operation| *operation == "project.switch")
            .count(),
        1,
        "the explicit switch sends exactly one switch: {operations:?}"
    );
    // This fixture keeps answering with its original project, so the switch is
    // reported as not completed rather than as done — which is the other half
    // of the rule: a switch nobody performed is never reported as performed.
    assert_eq!(
        switched.envelope["error"]["code"], "auth_context_mismatch",
        "{}",
        switched.stdout
    );
}

#[test]
fn map_validates_its_own_inputs_before_it_opens_the_bridge() {
    // Each of these is malformed in exactly one way, and the expected code is
    // the one that names that way — never a pairing code. A handler that
    // resolved the desktop first would return `desktop_not_paired` for every
    // row here on any machine without the application.
    let cases: &[(&[&str], &str)] = &[
        // A transposed box is the commonest way `zoom` goes wrong, and the
        // application would refuse it too — one round trip later.
        (
            &["map", "zoom", "--bbox", "30.2,-1.85,29.9,-2.1"],
            "invalid_bbox",
        ),
        (&["map", "zoom", "--bbox", "29.9,-2.1,30.2"], "invalid_bbox"),
        (&["map", "zoom"], "zoom_target"),
        (
            &[
                "map",
                "zoom",
                "--bbox",
                "29.9,-2.1,30.2,-1.85",
                "--layer",
                "sketch:x",
            ],
            "zoom_target",
        ),
        (
            &[
                "map",
                "zoom",
                "--bbox",
                "29.9,-2.1,30.2,-1.85",
                "--padding",
                "900",
            ],
            "invalid_number",
        ),
        (&["map", "view", "--limit", "0"], "invalid_number"),
        (
            &[
                "map",
                "points-along",
                "--layer",
                "sketch:x",
                "--interval-m",
                "0",
            ],
            "invalid_number",
        ),
        (
            &[
                "map",
                "outliers",
                "--layer",
                "sketch:x",
                "--threshold",
                "40",
            ],
            "invalid_number",
        ),
        (
            &["map", "outliers", "--layer", "sketch:x", "--limit", "500"],
            "invalid_number",
        ),
        (
            &[
                "map",
                "random-points",
                "--layer",
                "sketch:x",
                "--min-spacing-m",
                "0",
            ],
            "invalid_number",
        ),
        (
            &[
                "map",
                "draw",
                "--name",
                "n",
                "--geometry",
                "Point",
                "--features",
                "/definitely/not/here.geojson",
            ],
            "features_not_found",
        ),
        (
            &[
                "map",
                "design",
                "select",
                "--transformer",
                "T",
                "--where",
                "nope",
            ],
            "invalid_pair",
        ),
        (
            &[
                "map",
                "design",
                "select",
                "--transformer",
                "T",
                "--bbox",
                "1,2,3",
            ],
            "invalid_bbox",
        ),
        (
            &[
                "map",
                "design",
                "process",
                "--transformer",
                "T",
                "--request",
                "/nonexistent-lv-request.json",
            ],
            "differential_request_invalid",
        ),
        // `ds map ui open` is a panel door, not a UI driver. A selector, an
        // expression and a URL each have to be refused here — after the
        // bridge is open it is too late for the distinction to mean anything,
        // because the argument would already have been sent.
        (
            &[
                "map",
                "ui",
                "open",
                "--target",
                "style-center",
                "--ref",
                "#legend",
            ],
            "ui_ref_not_semantic",
        ),
        (
            &[
                "map",
                "ui",
                "open",
                "--target",
                "attribute-table",
                "--ref",
                "http://127.0.0.1/panel",
            ],
            "ui_ref_not_semantic",
        ),
        // A relative path and a non-PNG are the two ways `--out` goes wrong,
        // and both are cheaper to name here than after a full render.
        (
            &["map", "evidence", "capture", "--out", "step-1.png"],
            "evidence_out_invalid",
        ),
        (
            &[
                "map",
                "evidence",
                "capture",
                "--out",
                "/evidence/step-1.jpg",
            ],
            "evidence_out_invalid",
        ),
    ];

    for (args, expected) in cases {
        let mut argv: Vec<&str> = args.to_vec();
        argv.extend(["--output", "json"]);
        let code = refusal(&argv);
        assert_eq!(
            &code,
            expected,
            "`ds {}` refused with `{code}`, not `{expected}`. Either the \
             validation moved after the bridge call, or the code changed.",
            args.join(" ")
        );
    }

    // The geometry check reads the caller's own file and names the feature
    // that is wrong — the answer the application cannot give, because by the
    // time it looks the file is a payload.
    let line = line_geojson();
    let code = refusal(&[
        "map",
        "draw",
        "--name",
        "n",
        "--geometry",
        "Polygon",
        "--features",
        &line,
        "--output",
        "json",
    ]);
    assert_eq!(
        code, "geometry_mismatch",
        "a LineString file declared as a Polygon layer must be refused locally"
    );
}

#[test]
fn map_draw_names_the_feature_whose_geometry_is_wrong() {
    // "The response parses" is not a test. This asserts the refusal carries
    // the index of the offending feature and the types actually found, which
    // is the difference between fixing a file and re-exporting it blind.
    let line = line_geojson();
    let run = ds(&[
        "map",
        "draw",
        "--name",
        "n",
        "--geometry",
        "Point",
        "--features",
        &line,
        "--output",
        "json",
    ]);
    assert_eq!(run.envelope["error"]["code"], "geometry_mismatch");
    let detail = &run.envelope["error"]["detail"];
    assert_eq!(detail["index"], 0, "the refusal must name which feature");
    assert_eq!(detail["declared"], "Point");
    assert_eq!(
        detail["found"],
        serde_json::json!(["LineString"]),
        "the refusal must report the geometry the file actually holds"
    );
    assert_eq!(run.code, 2, "a malformed input is exit class 2");
}

#[test]
fn map_design_set_refuses_an_edit_with_nothing_to_write() {
    // `--set` is declared required, so the parser catches an absent one; this
    // is the other way to arrive with nothing, and it must not reach the
    // bridge and stage an empty change.
    assert_eq!(
        refusal(&[
            "map",
            "design",
            "set",
            "--transformer",
            "T",
            "--set",
            "=value",
            "--output",
            "json",
        ]),
        "invalid_pair"
    );
    assert_eq!(
        refusal(&[
            "map",
            "design",
            "set",
            "--transformer",
            "T",
            "--output",
            "json"
        ]),
        "missing_input",
        "--set is required and the parser must say so by name"
    );
}

#[test]
fn map_design_set_refuses_non_scalar_and_duplicate_values_before_pairing() {
    assert_eq!(
        refusal(&[
            "map",
            "design",
            "set",
            "--transformer",
            "T",
            "--set",
            "tags:=[1]",
            "--output",
            "json",
        ]),
        "invalid_property_value"
    );
    assert_eq!(
        refusal(&[
            "map",
            "design",
            "set",
            "--transformer",
            "T",
            "--set",
            "enabled=true",
            "--set",
            "enabled:=true",
            "--output",
            "json",
        ]),
        "duplicate_property"
    );
}

#[test]
fn map_design_save_cannot_run_without_confirmation() {
    // The only command in the domain that writes to the project. The gate is
    // in dispatch, before the handler, so this holds no matter what the
    // handler does — and it must hold on a machine with no desktop at all.
    let run = ds(&[
        "map",
        "design",
        "save",
        "--transformer",
        "T-1042",
        "--output",
        "json",
    ]);
    assert_eq!(
        run.envelope["error"]["code"], "confirmation_required",
        "`ds map design save` reached past the confirmation gate"
    );
    assert_ne!(run.code, 0);
}

#[test]
fn retired_map_version_twins_are_absent_from_live_discovery() {
    let map = ok(&["capabilities", "map", "--output", "json"]);
    let commands = map["commands"].as_array().unwrap();
    for retired in ["map.design.version.begin", "map.design.version.list"] {
        assert!(!commands.iter().any(|command| command["id"] == retired));
    }
    assert!(
        commands
            .iter()
            .any(|command| command["id"] == "map.design.version.play")
    );
    assert!(
        commands
            .iter()
            .any(|command| command["id"] == "map.design.version.compare")
    );
}

#[test]
fn map_design_delete_requires_an_explicit_selector() {
    // An empty selector would match the whole room; "delete everything" must
    // never be the accident default of forgetting a flag — refused locally,
    // by name, before the bridge is even opened.
    assert_eq!(
        refusal(&[
            "map",
            "design",
            "delete",
            "--transformer",
            "T-1042",
            "--output",
            "json",
        ]),
        "selector_required",
        "an unselected delete must be refused before it reaches the bridge"
    );
}

#[test]
fn map_design_geometry_rejects_a_non_object_geometry_locally() {
    // The geometry contract is validated on this side too: a string that is
    // not JSON must fail by name without a desktop present.
    assert_eq!(
        refusal(&[
            "map",
            "design",
            "geometry",
            "--transformer",
            "T-1042",
            "--id",
            "lv_poles#1",
            "--geometry",
            "not-json",
            "--output",
            "json",
        ]),
        "invalid_geometry",
        "a malformed --geometry must be refused before the bridge is opened"
    );
}

#[test]
fn map_design_report_cannot_run_without_confirmation() {
    // Report export writes durable artifacts, so dispatch requires --yes
    // exactly like save — and the gate must hold with no desktop at all.
    let run = ds(&[
        "map",
        "design",
        "report",
        "--transformer",
        "T-1042",
        "--output",
        "json",
    ]);
    assert_eq!(
        run.envelope["error"]["code"], "confirmation_required",
        "`ds map design report` reached past the confirmation gate"
    );
    assert_ne!(run.code, 0);
}

#[test]
fn map_design_attach_print_validates_before_pairing() {
    let unconfirmed = ds(&[
        "map",
        "design",
        "attach-print",
        "--path",
        "/deliverables/T-1042-A3.pdf",
        "--transformer",
        "T-1042",
        "--map-family",
        "lv-atlas",
        "--layout",
        "LV A3",
        "--paper-size",
        "A3",
        "--orientation",
        "landscape",
        "--output",
        "json",
    ]);
    assert_eq!(
        unconfirmed.envelope["error"]["code"], "confirmation_required",
        "a durable cartographic attachment must require --yes"
    );

    assert_eq!(
        refusal(&[
            "map",
            "design",
            "attach-print",
            "--path",
            "/deliverables/project-A1.pdf",
            "--layout",
            "Project A1",
            "--map-family",
            "mv-map",
            "--paper-size",
            "A1",
            "--orientation",
            "landscape",
            "--yes",
            "--output",
            "json",
        ]),
        "transformer_required",
        "transformer scope must name its transformer before bridge discovery"
    );
}

#[test]
fn map_evidence_capture_will_not_overwrite_a_frame_without_being_asked_and_confirmed() {
    // The gate that matters for this command. `local_file_write` is not in
    // dispatch's confirmation set, so the whole of what stands between a model
    // and an overwritten piece of evidence is the handler — and it has to hold
    // on a machine with no desktop, which is every CI machine.
    let root = temp_root("map-evidence");
    std::fs::create_dir_all(&root).expect("temp directory is writable");
    let existing = root.join("step-3.png");
    std::fs::write(&existing, b"an earlier frame").expect("temp file is writable");
    let existing = existing.display().to_string();

    assert_eq!(
        refusal(&[
            "map", "evidence", "capture", "--out", &existing, "--output", "json"
        ]),
        "evidence_exists",
        "an existing frame must be refused rather than silently re-shot"
    );
    assert_eq!(
        refusal(&[
            "map",
            "evidence",
            "capture",
            "--out",
            &existing,
            "--replace",
            "--output",
            "json",
        ]),
        "confirmation_required",
        "--replace without --yes must stop before the bridge is opened"
    );

    // Confirmed, the local checks are satisfied and the call proceeds to the
    // desktop — which is not here, so it ends in a pairing state rather than
    // in either local refusal. That ordering is the claim: the gate is not
    // merely reachable, it is the only thing that was stopping the call.
    let confirmed = refusal(&[
        "map",
        "evidence",
        "capture",
        "--out",
        &existing,
        "--replace",
        "--yes",
        "--output",
        "json",
    ]);
    assert!(
        PAIRING_CODES.contains(&confirmed.as_str()),
        "a confirmed replace ended in `{confirmed}`, not at the pairing boundary"
    );
    assert_eq!(
        std::fs::read(&existing).expect("the frame is still there"),
        b"an earlier frame",
        "no invocation in this test may touch the file; the application writes it"
    );

    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn map_evidence_capture_declares_a_fixed_receipt_and_no_way_to_record() {
    // Two claims a caller reads before it ever runs the command, held to the
    // descriptor rather than to prose: the receipt is the seven fields, and
    // there is no recording door. Video is a third-party tool's job, and the
    // way that stays true is that no flag here could ever start or bound one.
    let descriptor = ok(&["capabilities", "map.evidence.capture", "--output", "json"]);
    let command = &descriptor["command"];
    let output = command["output"].as_str().expect("output");
    for field in [
        "path",
        "bytes",
        "sha256",
        "dimensions",
        "scope",
        "view",
        "ui",
    ] {
        assert!(
            output.contains(field),
            "the declared output no longer names the receipt field `{field}`"
        );
    }

    let inputs: BTreeSet<&str> = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("input name"))
        .collect();
    assert_eq!(
        inputs,
        ["out", "scope", "replace", "target", "desktop-descriptor"]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        "a flag appeared on the capture contract; a recorder would arrive as one"
    );

    let scope: BTreeSet<&str> = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .find(|input| input["name"] == "scope")
        .expect("--scope is declared")["choices"]
        .as_array()
        .expect("choices")
        .iter()
        .map(|choice| choice.as_str().expect("choice"))
        .collect();
    assert_eq!(
        scope,
        ["map", "app"].into_iter().collect::<BTreeSet<_>>(),
        "--scope is a closed pair: the canvas, or the whole window"
    );
}

#[test]
fn map_ui_open_offers_named_panels_and_no_way_to_address_the_interface() {
    // The reason this command is safe to have at all: the target is a closed
    // set of published panels, and there is no second input that could carry a
    // selector, a coordinate or a script. New destinations stay semantic and
    // must be explicitly declared on both sides of the bridge.
    let command = ok(&["capabilities", "map.ui.open", "--output", "json"])["command"].clone();
    let inputs: BTreeSet<&str> = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("input name"))
        .collect();
    assert_eq!(
        inputs,
        ["target", "ref", "desktop-descriptor"]
            .into_iter()
            .collect::<BTreeSet<_>>(),
        "`ds map ui open` grew an input; it takes a published panel and a published ref"
    );

    let targets: Vec<&str> = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .find(|input| input["name"] == "target")
        .expect("--target is declared")["choices"]
        .as_array()
        .expect("choices")
        .iter()
        .map(|choice| choice.as_str().expect("choice"))
        .collect();
    assert_eq!(
        targets,
        vec![
            "attribute-table",
            "style-center",
            "selection-properties",
            "data",
            "software",
            "project-printing",
            "project-control",
            "sync-center",
            "notification-center",
            "transformers",
            "report-preview"
        ],
    );

    // A ref the application publishes reaches the bridge; a selector does not.
    let published = refusal(&[
        "map",
        "ui",
        "open",
        "--target",
        "selection-properties",
        "--ref",
        "TX-1042.lv_lines/17",
        "--output",
        "json",
    ]);
    assert!(
        PAIRING_CODES.contains(&published.as_str()),
        "a published reference ended in `{published}`, not at the pairing boundary"
    );
}

#[test]
fn map_layer_management_keeps_project_and_desktop_local_effects_separate() {
    let add = ok(&["capabilities", "map.layer.add", "--output", "json"])["command"].clone();
    assert_eq!(add["authority"], "none");
    assert_eq!(add["effect"], "local_file_write");
    let list = ok(&["capabilities", "map.layer.list", "--output", "json"])["command"].clone();
    assert_eq!(list["authority"], "headless_project");
    assert_eq!(list["effect"], "local_auth_state");
    let reorder = ok(&["capabilities", "map.layer.reorder", "--output", "json"])["command"].clone();
    assert_eq!(reorder["authority"], "headless_project");
    assert_eq!(reorder["effect"], "global_write");
    let default = ok(&["capabilities", "map.layer.default", "--output", "json"])["command"].clone();
    assert_eq!(default["authority"], "headless_project");
    assert_eq!(default["effect"], "global_write");

    let confirmation = refusal(&[
        "map",
        "layer",
        "reorder",
        "--project",
        "test-project",
        "--order",
        "roads=100",
        "--output",
        "json",
    ]);
    assert_eq!(confirmation, "confirmation_required");

    let bad_visibility = refusal(&[
        "map",
        "layer",
        "visibility",
        "--layer",
        "tile-1",
        "--visible",
        "toggle",
        "--output",
        "json",
    ]);
    assert_eq!(bad_visibility, "invalid_choice");
}

#[test]
fn tile_managed_outputs_are_headless_and_catalogue_are_headless() {
    for (id, effect, inputs) in [
        (
            "tile.status",
            "local_auth_state",
            BTreeSet::from(["lane", "project", "type"]),
        ),
        (
            "tile.preflight",
            "local_auth_state",
            BTreeSet::from(["lane", "project", "type"]),
        ),
        (
            "tile.plan",
            "local_auth_state",
            BTreeSet::from(["force", "lane", "project", "type"]),
        ),
        (
            "tile.generate",
            "global_write",
            BTreeSet::from(["force", "lane", "project", "type"]),
        ),
    ] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let command = &descriptor["command"];
        assert_eq!(command["authority"], "headless_project", "{id}");
        assert_eq!(command["effect"], effect, "{id}");
        let actual = command["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .map(|input| input["name"].as_str().expect("input name"))
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, inputs, "{id}");
        // Every tile operation names its project; none reads the saved
        // selection (the Server has no active project).
        assert!(actual.contains("project"), "{id} must name its project");
        assert!(
            !actual.contains("desktop-descriptor"),
            "{id} still depends on Desktop"
        );
    }

    for id in ["tile.list", "tile.add", "tile.remove"] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let command = &descriptor["command"];
        assert_eq!(command["authority"], "headless_project", "{id}");
        assert!(
            command["inputs"]
                .as_array()
                .expect("inputs")
                .iter()
                .any(|input| input["name"] == "project"),
            "{id} must name its project"
        );
        assert!(
            !command["inputs"]
                .as_array()
                .expect("inputs")
                .iter()
                .any(|input| input["name"] == "desktop-descriptor"),
            "{id} must execute without Desktop"
        );
    }

    assert_eq!(
        refusal(&[
            "tile",
            "generate",
            "--project",
            "test-project",
            "--type",
            "design",
            "--lane",
            "stable",
            "--output",
            "json",
        ]),
        "confirmation_required"
    );
}

#[test]
fn global_tile_list_is_a_domain_bounded_native_recovery_read() {
    let descriptor = ok(&["capabilities", "tile.global.list", "--output", "json"]);
    let command = &descriptor["command"];
    // A reference publication belongs to the product, so the authority is the
    // restored native user and the lane is the only host fact it carries.
    assert_eq!(command["authority"], "headless_user");
    assert_eq!(command["effect"], "read_only");
    let inputs = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("input name"))
        .collect::<BTreeSet<_>>();
    assert_eq!(inputs, BTreeSet::from(["domain", "lane"]));

    let code = native_refusal(&[
        "tile",
        "global",
        "list",
        "--domain",
        "network_template",
        "--output",
        "json",
    ]);
    assert!(
        NATIVE_AUTH_CODES.contains(&code.as_str()),
        "a valid recovery read stopped at `{code}`, not the native reference catalog"
    );

    // Publication is retired hosted compute with no edge owner yet: it refuses
    // before any credential is restored or anything is sent.
    assert_eq!(
        native_refusal(&[
            "tile",
            "global",
            "generate",
            "--domain",
            "network_template",
            "--name",
            "Rwanda roads",
            "--country",
            "RW",
            "--source",
            "not-a-catalog-identity",
            "--maxzoom",
            "12",
            "--yes",
            "--output",
            "json",
        ]),
        "native_compute_required"
    );
}

#[test]
fn hiding_a_reference_publication_is_a_headless_governed_write() {
    let descriptor = ok(&["capabilities", "tile.global.access", "--output", "json"]);
    let command = &descriptor["command"];
    // A publication belongs to the product: the restored native user and the
    // lane are all this write carries, and no window is involved.
    assert_eq!(command["authority"], "headless_user");
    assert_eq!(command["effect"], "global_write");
    let inputs = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("input name"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        inputs,
        BTreeSet::from([
            "allow-app-role",
            "allow-project",
            "allow-project-role",
            "allow-user",
            "lane",
            "mode",
            "tile",
        ])
    );

    // Restricting a publication changes what the whole product mounts, so it
    // is confirmed like any other governed write.
    assert_eq!(
        refusal(&[
            "tile",
            "global",
            "access",
            "--tile",
            "rwanda_data_elementary_schools",
            "--mode",
            "restricted",
            "--output",
            "json",
        ]),
        "confirmation_required"
    );

    // An allowlist under `all` is a grant nothing evaluates, and a reader
    // named twice is silently collapsed by the catalog: both are refused
    // before a native user is restored, not recorded.
    for allowlist in [
        vec!["--allow-user", "operator@example.com"],
        vec!["--allow-project", "czgmdwth_gisagara"],
    ] {
        let mut argv = vec![
            "tile",
            "global",
            "access",
            "--tile",
            "rwanda_data_elementary_schools",
            "--mode",
            "all",
        ];
        argv.extend(allowlist);
        argv.extend(["--yes", "--output", "json"]);
        assert_eq!(native_refusal(&argv), "global_tile_selection_invalid");
    }
    assert_eq!(
        native_refusal(&[
            "tile",
            "global",
            "access",
            "--tile",
            "rwanda_data_elementary_schools",
            "--mode",
            "restricted",
            "--allow-user",
            "operator@example.com",
            "--allow-user",
            "Operator@Example.com",
            "--yes",
            "--output",
            "json",
        ]),
        "global_tile_selection_invalid"
    );

    // A usable policy passes the owner's bounds and stops only at this
    // machine's native identity — never at a paired window.
    let code = native_refusal(&[
        "tile",
        "global",
        "access",
        "--tile",
        "rwanda_data_elementary_schools",
        "--mode",
        "restricted",
        "--allow-app-role",
        "platform_admin",
        "--yes",
        "--output",
        "json",
    ]);
    assert!(
        NATIVE_AUTH_CODES.contains(&code.as_str()),
        "a usable access policy stopped at `{code}`, not the native reference catalog"
    );
}

#[test]
fn background_project_operations_are_map_independent_and_use_the_declared_project_context() {
    for (id, effect, inputs) in [
        // Both name their project since 2026-09-18: a headless read whose
        // answer depends on this machine's saved selection is not one answer.
        (
            "design.status",
            "local_auth_state",
            BTreeSet::from([
                "desc",
                "filter",
                "findings",
                "lane",
                "out",
                "project",
                "read",
                "search",
                "sort",
                "transformer",
            ]),
        ),
        (
            "design.dashboard",
            "local_auth_state",
            BTreeSet::from(["lane", "project", "tz-offset-minutes"]),
        ),
        (
            "design.transformer.inventory",
            "local_auth_state",
            BTreeSet::from(["lane", "project", "transformer"]),
        ),
        (
            "design.transformer.retire",
            "global_write",
            BTreeSet::from(["lane", "project", "reason", "transformer"]),
        ),
        (
            "design.transformer.restore",
            "global_write",
            BTreeSet::from(["lane", "project", "transformer"]),
        ),
        // `group-by` and `where` (2026-09-26): the scope selected from tags,
        // one archive per leaf group — the preview and the publish take the
        // same two, so the plan and the run cannot disagree.
        (
            "report.project.scope",
            "local_auth_state",
            BTreeSet::from(["group-by", "lane", "project", "transformer", "where"]),
        ),
        (
            "report.project.compounded",
            "artifact_write",
            BTreeSet::from([
                "combine-per-group",
                "composition",
                "file-level",
                "force",
                "group-by",
                "lane",
                "project",
                "transformer",
                "where",
            ]),
        ),
        // The cloud twin of `report.project.export`: the same three inputs
        // as the Compounded Report's scope — the names, the lane, and the
        // framework's `--yes` — and not one option more.
        (
            "report.project.compute",
            "artifact_write",
            BTreeSet::from(["lane", "project", "transformer"]),
        ),
        (
            "report.project.archives",
            "local_auth_state",
            BTreeSet::from(["lane", "project"]),
        ),
        (
            "report.project.settings",
            "local_auth_state",
            BTreeSet::from(["lane", "project"]),
        ),
        (
            "report.project.outputs.set",
            "global_write",
            BTreeSet::from(["lane", "project", "selection"]),
        ),
    ] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let command = &descriptor["command"];
        assert_eq!(command["authority"], "headless_project", "{id}");
        assert_eq!(command["effect"], effect, "{id}");
        let actual = command["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .map(|input| input["name"].as_str().expect("input name"))
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, inputs, "{id}");
        // Every background project operation names its project; none reads
        // the saved selection (the Server has no active project).
        assert_eq!(
            actual.contains("project"),
            matches!(id, "design.status" | "design.dashboard")
                || id.starts_with("design.transformer.")
                || id.starts_with("report.project."),
            "{id} disagrees with how it is meant to reach a project"
        );
        assert!(
            !actual.contains("desktop-descriptor"),
            "{id} depends on Desktop"
        );
    }

    // Scope refusals are local and precede any credential restore; the
    // development catalog makes the native availability gate pass so the
    // command's own input validation is what answers.
    let headless = |args: &[&str]| -> String {
        let mut named = args.to_vec();
        if args.starts_with(&["report", "project"]) || args.starts_with(&["design", "transformer"])
        {
            named.extend(["--project", "test-project"]);
        }
        // Use the isolated native runner: this gate must never restore the
        // operator's credential or discover their running Desktop.
        let result = native_ds(&named);
        result.envelope["error"]["code"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    };
    // Canonical aliases are de-duplicated before the request. Reaching the
    // auth gate proves the duplicate did not become an ambiguous receipt.
    assert_eq!(
        headless(&[
            "design",
            "transformer",
            "retire",
            "--reason",
            "dup",
            "--yes",
            "--output",
            "json"
        ]),
        "invalid_transformer_scope"
    );
    assert_eq!(
        headless(&[
            "design",
            "transformer",
            "retire",
            "--transformer",
            "tx_a",
            "--transformer",
            "tx_a",
            "--reason",
            "dup",
            "--yes",
            "--output",
            "json",
        ]),
        "headless_signed_out"
    );
    assert_eq!(
        headless(&[
            "design",
            "transformer",
            "retire",
            "--transformer",
            "tx_a",
            "--reason",
            "  ",
            "--yes",
            "--output",
            "json",
        ]),
        "invalid_reason"
    );
    assert_eq!(
        headless(&[
            "report",
            "project",
            "scope",
            "--transformer",
            " tx_a",
            "--output",
            "json"
        ]),
        "invalid_transformer_scope"
    );
    // The computed identities a report produces are refused with their own
    // code, under every alias, and — like the scope bound above — before any
    // credential restore, so the plan and the publish cannot disagree about
    // who may participate.
    for reserved in ["collisions", "combined_transformer", "all_transformers"] {
        assert_eq!(
            headless(&[
                "report",
                "project",
                "scope",
                "--transformer",
                reserved,
                "--output",
                "json"
            ]),
            "reserved_transformer_identity",
            "`--transformer {reserved}` must refuse locally"
        );
        assert_eq!(
            headless(&[
                "report",
                "project",
                "compute",
                "--transformer",
                reserved,
                "--yes",
                "--output",
                "json"
            ]),
            "reserved_transformer_identity",
            "`compute --transformer {reserved}` must refuse locally"
        );
    }
    // A grouped Combined Report (owner ruling 2026-09-26) selects its scope
    // from tags, so naming transformers beside it is refused, and a malformed
    // group-by or filter is the kernel's verdict — all before any credential
    // is restored. A well-formed grouping reaches the native gate, which is
    // where it reads the tag listing, inventory and projection.
    for command in ["scope", "compounded"] {
        let run = |extra: &[&str]| {
            let mut args = vec!["report", "project", command];
            args.extend_from_slice(extra);
            args.extend(["--yes", "--output", "json"]);
            headless(&args)
        };
        assert_eq!(
            run(&["--group-by", "city", "--transformer", "tx_a"]),
            "combined_group_scope_conflict",
            "{command}: --transformer and --group-by are alternatives"
        );
        assert_eq!(
            run(&["--where", "phase=i", "--transformer", "tx_a"]),
            "combined_group_scope_conflict",
            "{command}: --where selects the scope too"
        );
        for malformed in [
            &["--where", "phase"][..],
            &["--where", "phase= i"][..],
            &["--group-by", "city", "--group-by", "city"][..],
            &["--group-by", " city"][..],
        ] {
            assert_eq!(
                run(malformed),
                "combined_group_request_invalid",
                "{command} {malformed:?} must refuse locally"
            );
        }
        assert_eq!(
            run(&[
                "--group-by",
                "district",
                "--group-by",
                "city",
                "--where",
                "phase=i"
            ]),
            "headless_signed_out",
            "{command}: a well-formed grouping reaches the native gate"
        );
    }
    assert_eq!(
        headless(&[
            "report",
            "project",
            "compounded",
            "--group-by",
            "city",
            "--output",
            "json"
        ]),
        "confirmation_required",
        "a grouped run publishes archives, so it confirms first"
    );
    // The cloud computation confirms before anything else, then needs the
    // native user: neither costs a round trip.
    assert_eq!(
        headless(&["report", "project", "compute", "--output", "json"]),
        "confirmation_required"
    );
    assert_eq!(
        headless(&["report", "project", "compute", "--yes", "--output", "json"]),
        "headless_signed_out"
    );
    // The printing output policy reaches the same gate: a settings read with
    // no restorable native user refuses by code and performs no request, and
    // a save refuses its own document locally — before any credential — so an
    // unusable selection never costs a round trip. Confirmation comes first
    // of all.
    assert_eq!(
        headless(&["report", "project", "settings", "--output", "json"]),
        "headless_signed_out"
    );
    let selections = tempfile::tempdir().expect("temp dir");
    let valid = selections.path().join("outputs.json");
    std::fs::write(
        &valid,
        r#"{"schema":"ds.design-output-selection/v1","geospatial":["gpkg"],"tabular":["xlsx"],
            "execution":{"gpkg":["web"]}}"#,
    )
    .expect("selection written");
    let invalid = selections.path().join("not-a-selection.json");
    std::fs::write(&invalid, r#"{"formats":["gpkg"]}"#).expect("selection written");
    let set = |path: &std::path::Path, confirm: bool| {
        let mut args = vec![
            "report",
            "project",
            "outputs",
            "set",
            "--selection",
            path.to_str().expect("utf-8 path"),
        ];
        if confirm {
            args.push("--yes");
        }
        args.extend(["--output", "json"]);
        headless(&args)
    };
    assert_eq!(set(&valid, false), "confirmation_required");
    assert_eq!(set(&invalid, true), "invalid_output_selection");
    assert_eq!(
        set(&selections.path().join("absent.json"), true),
        "invalid_output_selection"
    );
    assert_eq!(set(&valid, true), "headless_signed_out");

    // Confirmation is decided by dispatch before any availability or input check.
    assert_eq!(
        refusal(&[
            "design",
            "transformer",
            "restore",
            "--project",
            "test-project",
            "--transformer",
            "tx_a",
            "--output",
            "json"
        ]),
        "confirmation_required"
    );
}

/// A batch of 86 Gisagara transformers lost 59 sheets because one A0 failed
/// to lay out beside four finished artifacts and the verifier declared the
/// whole run invalid — finished, correct work discarded, never synced, never
/// pulled, left on the compute. The shape that fixes it has to be readable
/// BEFORE the run: a caller must be able to see from the descriptor alone
/// that a partly-successful export answers rather than refuses, and that the
/// answer names the format it lost precisely enough to act on.
#[test]
fn a_partly_successful_export_is_declared_as_a_result_not_a_refusal() {
    let descriptor = ok(&["capabilities", "report.project.export", "--output", "json"]);
    let command = &descriptor["command"];
    let output = command["output"].as_str().expect("output");
    for member in [
        "failed_formats",
        "output_id",
        "remedy",
        "partial_formats",
        // "the A0 failed" is not actionable; the knob that decides it is.
        "overflow/panels/row_mm",
    ] {
        assert!(
            output.contains(member),
            "the declared output no longer names `{member}`, so a caller cannot \
             tell a partial result from a complete one"
        );
    }
    // And the refusal it is no longer: `export_blocked` now means the engine
    // produced nothing at all.
    let blocked = command["refusals"]
        .as_array()
        .expect("refusals")
        .iter()
        .find(|refusal| refusal["code"] == "export_blocked")
        .expect("export_blocked stays documented");
    let when = blocked["when"].as_str().expect("when");
    assert!(
        when.contains("NO format"),
        "export_blocked must say it is the nothing-completed case: {when}"
    );
    assert!(
        !blocked["remedy"]
            .as_str()
            .expect("remedy")
            .trim()
            .is_empty(),
        "every refusal keeps a remedy"
    );
}

/// A publishing export reuses the data outputs the project already holds
/// from the same inputs and engine build, and regenerates every print. The
/// one way to say "regenerate everything" is `--force`: it must be declared
/// as a switch, parse to the command's own documented refusals rather than
/// the parser's `unknown_flag`, and the declared output must name where a
/// caller reads what was reused and why the rest was generated.
#[test]
fn export_reuse_is_declared_forceable_and_reported() {
    let descriptor = ok(&["capabilities", "report.project.export", "--output", "json"]);
    let command = &descriptor["command"];
    let force = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .find(|input| input["name"] == "force")
        .expect("--force is declared");
    assert_eq!(force["kind"], "switch");
    let purpose = command["purpose"].as_str().expect("purpose");
    assert!(
        purpose.contains("Prints regenerate; data may reuse"),
        "a caller must see from the descriptor that a print is never reused: {purpose}"
    );
    let output = command["output"].as_str().expect("output");
    for member in [
        "reuse",
        "work_id",
        "generate reasons",
        "every output was reused",
    ] {
        assert!(output.contains(member), "output no longer names `{member}`");
    }
    let documented: Vec<&str> = command["refusals"]
        .as_array()
        .expect("refusals")
        .iter()
        .filter_map(|refusal| refusal["code"].as_str())
        .collect();
    let out_dir = temp_root("export-force");
    let code = native_refusal(&[
        "report",
        "project",
        "export",
        "--force",
        "--out-dir",
        out_dir.to_str().expect("utf-8 temp path"),
        "--project",
        "test-project",
        "--output",
        "json",
    ]);
    let _ = std::fs::remove_dir_all(&out_dir);
    assert_ne!(code, "unknown_flag", "--force must parse");
    assert!(
        documented.contains(&code.as_str()),
        "a signed-out --force export refuses with a documented code, not {code:?}"
    );
}

/// The collision read: a report cannot be produced while a collision stands,
/// so `ds` has to be able to say how many there are. It reads the project-wide
/// document the report owner wrote; it starts no detection and takes no
/// project, transformer or descriptor.
#[test]
fn the_vocabulary_housekeeping_family_names_the_report_defect_each_verb_removes() {
    // Reporting rewrites any customer or meter description its catalog does
    // not know into a fallback, so the catalog silently decides engineering
    // totals. Seeding alone cannot repair a catalog that is already wrong:
    // these four verbs retire, rename, unbind and reorder, and each one's
    // contract has to say which part of that rewrite it governs — otherwise a
    // caller reaches for the wrong one and moves the fallback by accident.
    for (id, path, promise, complete) in [
        (
            "design.customer-categories.retire",
            ["design", "customer-categories", "retire"],
            "first in the catalog",
            vec!["--name", "Pauvre"],
        ),
        (
            "design.customer-categories.retire-unnamed",
            ["design", "customer-categories", "retire-unnamed"],
            "no canonical name",
            vec![],
        ),
        (
            "design.customer-categories.rename",
            ["design", "customer-categories", "rename"],
            "the old name",
            vec!["--from", "Education_I", "--to", "Primary School"],
        ),
        (
            "design.customer-categories.unbind",
            ["design", "customer-categories", "unbind"],
            "a label two categories claim",
            vec!["--alias", "Ecole Primaire", "--category", "School"],
        ),
        (
            "design.meter-types.default",
            ["design", "meter-types", "default"],
            "first named row",
            vec!["--name", "Three Phase"],
        ),
    ] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let command = &descriptor["command"];
        assert_eq!(command["path"], serde_json::json!(path));
        assert_eq!(command["authority"], "headless_project");
        assert_eq!(
            command["effect"], "global_write",
            "`{id}` edits the project's shared vocabulary"
        );
        let purpose = command["purpose"].as_str().expect("purpose");
        assert!(
            purpose.contains(promise),
            "`{id}` must say which part of the report rewrite it governs: {purpose}"
        );
        // The project is named on every call; the saved selection is never read.
        let inputs = command["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .map(|input| input["name"].as_str().expect("input name"))
            .collect::<BTreeSet<_>>();
        assert!(inputs.contains("lane"));
        assert!(inputs.contains("project"));
        // A vocabulary edit reaches the confirmation gate before it reaches a
        // credential: nothing about a catalog is read until the caller means
        // it.
        let mut gated: Vec<&str> = path.to_vec();
        gated.extend(complete);
        gated.extend(["--project", "test-project", "--output", "json"]);
        assert_eq!(
            native_ds(&gated).envelope["error"]["code"],
            "confirmation_required",
            "`{id}` must stop at the global write gate"
        );
    }

    // Only `ensure` may create a row. Every verb above orders, renames or
    // removes what is already seeded, so none of them can quietly invent the
    // category a caller misspelled.
    let ensure = ok(&[
        "capabilities",
        "design.meter-types.ensure",
        "--output",
        "json",
    ]);
    assert_eq!(
        ensure["command"]["path"],
        serde_json::json!(["design", "meter-types", "ensure"])
    );

    // The catalog read is what tells a caller the rewrite is coming. Its
    // contract has to promise the three facts the rows themselves never show.
    let read = ok(&["capabilities", "design.categories.read", "--output", "json"]);
    let output = read["command"]["output"].as_str().expect("output");
    for promised in ["fallback", "two categories claim", "Unnamed rows"] {
        assert!(
            output.to_lowercase().contains(&promised.to_lowercase()),
            "`design categories read` must promise `{promised}`: {output}"
        );
    }
}

#[test]
fn design_collisions_reads_the_project_document_and_starts_nothing() {
    let descriptor = ok(&["capabilities", "design.collisions", "--output", "json"]);
    let command = &descriptor["command"];
    assert_eq!(command["path"], serde_json::json!(["design", "collisions"]));
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "local_auth_state");
    assert_eq!(command["execution"], "sync");
    let inputs = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("input name"))
        .collect::<BTreeSet<_>>();
    // The lane and the project, named on every call: the saved selection is
    // never read. Contract 3 retired --regions/--limit (vector_retirement).
    assert_eq!(inputs, BTreeSet::from(["lane", "project"]));

    // A call that names no project is refused locally, never answered for a
    // selection.
    assert_eq!(
        native_ds(&["design", "collisions", "--output", "json"]).envelope["error"]["code"],
        "missing_input"
    );

    // And an undeclared lane is answered locally, before any credential is
    // restored.
    assert_eq!(
        native_ds(&[
            "design",
            "collisions",
            "--project",
            "test-project",
            "--lane",
            "staging",
            "--output",
            "json"
        ])
        .envelope["error"]["class"],
        "invalid_input"
    );
}

/// ONE design-domain migration, `kind` transformer|dsgrid. Both kinds ride
/// one verb; the plan writes nothing; neither needs a window. The selection
/// bounds and the same-project refusal are answered locally, before any
/// credential is restored, so a malformed batch never reaches the service.
#[test]
fn design_migrate_carries_both_design_kinds_on_one_verb_and_refuses_locally() {
    for (id, path, effect) in [
        (
            "design.migrate.plan",
            ["design", "migrate", "plan"],
            "read_only",
        ),
        (
            "design.migrate.apply",
            ["design", "migrate", "apply"],
            "global_write",
        ),
    ] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let command = &descriptor["command"];
        assert_eq!(command["path"], serde_json::json!(path));
        assert_eq!(command["effect"], effect, "{id}");
        // Migrating between projects needs no window.
        assert_eq!(command["authority"], "headless_project", "{id}");
        let inputs = command["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .map(|input| input["name"].as_str().expect("input name"))
            .collect::<BTreeSet<_>>();
        assert_eq!(
            inputs,
            BTreeSet::from([
                "source-project",
                "project",
                "kind",
                "item",
                "overwrite",
                "lane"
            ]),
            "{id}"
        );
        // Both design kinds are one flag on one verb, not two command families.
        let kind = command["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .find(|input| input["name"] == "kind")
            .expect("kind input");
        assert_eq!(
            kind["choices"],
            serde_json::json!(["transformer", "dsgrid"]),
            "{id}"
        );
    }

    // A selection larger than one transaction's batch is refused here, with a
    // remedy, rather than half-applied by the service.
    let names = (0..201)
        .map(|index| format!("tx{index}"))
        .collect::<Vec<_>>();
    let mut too_many = vec![
        "design",
        "migrate",
        "plan",
        "--source-project",
        "source_one",
        "--project",
        "target_one",
        "--kind",
        "transformer",
    ];
    for name in &names {
        too_many.extend(["--item", name.as_str()]);
    }
    too_many.extend(["--output", "json"]);
    let refusal = native_ds(&too_many);
    assert_eq!(refusal.envelope["error"]["code"], "invalid_selection");
    assert!(
        refusal.envelope["error"]["remedy"]
            .as_str()
            .unwrap_or_default()
            .contains("200"),
        "the bound must be named in the remedy: {:?}",
        refusal.envelope["error"]
    );

    // A kind from another domain is refused, not quietly answered as a
    // transformer migration: Solar and Survey own their own endpoints. The
    // declared choices refuse it in the parser, before any credential is
    // restored; `unknown_kind` is the same refusal from the handler.
    let foreign_kind = native_ds(&[
        "design",
        "migrate",
        "plan",
        "--source-project",
        "source_one",
        "--project",
        "target_one",
        "--kind",
        "city",
        "--item",
        "TX-1",
        "--output",
        "json",
    ]);
    assert!(
        matches!(
            foreign_kind.envelope["error"]["code"].as_str(),
            Some("invalid_choice") | Some("unknown_kind")
        ),
        "a Solar kind must be refused by the design migration, not answered: {:?}",
        foreign_kind.envelope["error"]
    );

    // Migration is stateless: the destination is an operand, and naming the
    // source as the destination is refused before any credential is used.
    let same = native_ds(&[
        "design",
        "migrate",
        "plan",
        "--source-project",
        "source_one",
        "--project",
        "source_one",
        "--kind",
        "transformer",
        "--item",
        "TX-1",
        "--output",
        "json",
    ]);
    assert_eq!(same.envelope["error"]["code"], "same_project");
}

/// Survey-data migration is stateless and headless: `--source-project` INTO
/// `--project`, both required, on a server with no window. The paired-Desktop
/// command that took the destination from the window's active project is
/// gone, and the local refusals answer before any credential is restored.
#[test]
fn survey_migrate_names_both_projects_and_needs_no_window() {
    for (id, path, effect) in [
        (
            "survey.migrate.plan",
            ["survey", "migrate", "plan"],
            "read_only",
        ),
        (
            "survey.migrate.apply",
            ["survey", "migrate", "apply"],
            "global_write",
        ),
    ] {
        let command = &ok(&["capabilities", id, "--output", "json"])["command"];
        assert_eq!(command["path"], serde_json::json!(path));
        assert_eq!(command["effect"], effect, "{id}");
        assert_eq!(command["authority"], "headless_project", "{id}");
        assert_eq!(command["requires"], "server", "{id}");
        assert_eq!(command["reference"], "docs/reference/migration.md", "{id}");
        let inputs = command["inputs"].as_array().expect("inputs");
        let names: BTreeSet<&str> = inputs
            .iter()
            .map(|input| input["name"].as_str().expect("input name"))
            .collect();
        assert_eq!(
            names,
            BTreeSet::from(["source-project", "project", "lane"]),
            "{id}"
        );
        for name in ["source-project", "project"] {
            assert!(
                inputs
                    .iter()
                    .any(|input| input["name"] == name && input["required"] == true),
                "`{id}` must require --{name}"
            );
        }
    }
    for retired in ["map.survey.migrate.plan", "map.survey.migrate.apply"] {
        assert_eq!(
            refusal(&["capabilities", retired, "--output", "json"]),
            "unknown_selector",
            "{retired} survives beside its headless replacement"
        );
    }

    let plan = |source: &str, destination: &str| {
        native_refusal(&[
            "survey",
            "migrate",
            "plan",
            "--source-project",
            source,
            "--project",
            destination,
            "--output",
            "json",
        ])
    };
    assert_eq!(plan("source_one", "source_one"), "same_project");
    assert_eq!(plan("source_one", "../target"), "invalid_project");
    assert_eq!(
        refusal(&[
            "survey",
            "migrate",
            "apply",
            "--source-project",
            "source_one",
            "--project",
            "target_one",
            "--output",
            "json",
        ]),
        "confirmation_required",
        "the copy writes, so it waits for --yes"
    );
}

/// The headless Design read spine: `ds design status` answers from the native
/// credential and the selected project, or it refuses in words. It never
/// reaches for a browser, a map, a Desktop descriptor, or a project override.
#[test]
fn design_bulk_plan_previews_a_batch_before_anything_is_dispatched() {
    let descriptor = ok(&["capabilities", "design.bulk.plan", "--output", "json"]);
    let command = &descriptor["command"];
    assert_eq!(
        command["path"],
        serde_json::json!(["design", "bulk", "plan"])
    );
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "local_auth_state");
    assert_eq!(command["execution"], "sync");
    assert_eq!(
        command["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .map(|input| input["name"].as_str().expect("input name"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([
            "action",
            "capability",
            "combined-mirror",
            "lane",
            "project",
            "transformer"
        ])
    );
    // The verb is a closed set, answered locally before any credential.
    assert_eq!(
        native_ds(&[
            "design", "bulk", "plan", "--action", "detonate", "--output", "json"
        ])
        .envelope["error"]["code"],
        "invalid_choice"
    );
    // And a malformed scope is refused before the credential too.
    assert_eq!(
        native_refusal(&[
            "design",
            "bulk",
            "plan",
            "--project",
            "test-project",
            "--action",
            "save",
            "--transformer",
            " tx_a",
            "--output",
            "json"
        ]),
        "invalid_transformer_scope"
    );
}

#[test]
fn design_download_plan_answers_scope_urls_and_placement_headlessly() {
    let descriptor = ok(&["capabilities", "design.download.plan", "--output", "json"]);
    let command = &descriptor["command"];
    assert_eq!(
        command["path"],
        serde_json::json!(["design", "download", "plan"])
    );
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "local_auth_state");
    assert_eq!(
        command["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .map(|input| input["name"].as_str().expect("input name"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["format", "lane", "project", "transformer"])
    );
    assert_eq!(
        native_ds(&[
            "design", "download", "plan", "--format", "docx", "--output", "json"
        ])
        .envelope["error"]["code"],
        "invalid_choice"
    );
    assert_eq!(
        native_refusal(&[
            "design",
            "download",
            "plan",
            "--project",
            "test-project",
            "--transformer",
            " tx_a",
            "--output",
            "json"
        ]),
        "invalid_transformer_scope"
    );
}

#[test]
fn design_version_status_uses_an_explicit_project_without_desktop_context() {
    let descriptor = ok(&["capabilities", "design.version.status", "--output", "json"]);
    let command = &descriptor["command"];
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "read_only");
    assert_eq!(
        command["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|input| input["name"].as_str().unwrap())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["project", "kind", "object", "lane"])
    );
}

#[test]
fn design_version_begin_is_a_confirmed_headless_project_write() {
    let descriptor = ok(&["capabilities", "design.version.begin", "--output", "json"]);
    let command = &descriptor["command"];
    assert_eq!(
        command["path"],
        serde_json::json!(["design", "version", "begin"])
    );
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "global_write");
    assert_eq!(command["confirmation_required"], true);
    assert_eq!(
        command["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .map(|input| input["name"].as_str().expect("input name"))
            .collect::<BTreeSet<_>>(),
        // `--milestone` and `--expected-source` (2026-09-25): an MV submission
        // marker carries its label and pins the head revision it reviewed.
        BTreeSet::from([
            "idempotency-key",
            "lane",
            "reason",
            "project",
            "kind",
            "object",
            "milestone",
            "expected-source"
        ])
    );

    let unconfirmed = native_ds(&[
        "design",
        "version",
        "begin",
        "--project",
        "p-1",
        "--kind",
        "lv_transformer",
        "--object",
        "tx_a",
        "--reason",
        "Issued for construction",
        "--idempotency-key",
        "tx-a-ifc-1",
        "--output",
        "json",
    ]);
    assert_eq!(
        unconfirmed.envelope["error"]["code"],
        "confirmation_required"
    );
}

#[test]
fn superseded_design_version_restore_is_retired_and_unreachable() {
    // The ordinal restore was removed (7af03397); version revisions are
    // promoted through `design.version.revision.promote`. Retired means no
    // descriptor and no dispatch, so nothing can reach the old write.
    assert_eq!(
        refusal(&["capabilities", "design.version.restore", "--output", "json"]),
        "unknown_selector"
    );
    let promote = ok(&[
        "capabilities",
        "design.version.revision.promote",
        "--output",
        "json",
    ]);
    assert_eq!(promote["command"]["authority"], "headless_project");
    assert_eq!(promote["command"]["effect"], "global_write");
    assert_eq!(promote["command"]["confirmation_required"], true);
}

#[test]
fn design_conflict_reads_never_claim_room_state_they_cannot_see() {
    for (id, path) in [
        ("design.conflict.list", vec!["design", "conflict", "list"]),
        ("design.conflict.check", vec!["design", "conflict", "check"]),
    ] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let command = &descriptor["command"];
        assert_eq!(command["path"], serde_json::json!(path));
        assert_eq!(command["authority"], "headless_project");
        assert_eq!(command["effect"], "local_auth_state");
        // Both say, in their own output contract, that a working copy is a
        // fact this client may not hold.
        assert!(
            command["output"]
                .as_str()
                .expect("output")
                .contains("room_state"),
            "{id} must declare the room state it reports"
        );
    }
    assert_eq!(
        ok(&["capabilities", "design.conflict.list", "--output", "json"])["command"]["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .map(|input| input["name"].as_str().expect("input name"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["lane", "project"])
    );
    // `check` names a transformer; naming none is refused locally.
    assert_eq!(
        native_refusal(&[
            "design",
            "conflict",
            "check",
            "--project",
            "test-project",
            "--output",
            "json"
        ]),
        "invalid_transformer_scope"
    );
    assert_eq!(
        native_refusal(&[
            "design",
            "conflict",
            "check",
            "--project",
            "test-project",
            "--transformer",
            " tx_a",
            "--output",
            "json"
        ]),
        "invalid_transformer_scope"
    );
}

#[test]
fn design_presence_status_reports_the_bounds_and_refuses_to_invent_rooms() {
    let descriptor = ok(&["capabilities", "design.presence.status", "--output", "json"]);
    let command = &descriptor["command"];
    assert_eq!(
        command["path"],
        serde_json::json!(["design", "presence", "status"])
    );
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "local_auth_state");
    assert_eq!(
        command["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .map(|input| input["name"].as_str().expect("input name"))
            .collect::<BTreeSet<_>>(),
        BTreeSet::from(["lane", "project"])
    );
    assert!(
        command["output"]
            .as_str()
            .expect("output")
            .contains("room_state"),
        "presence must declare the room state it reports"
    );
    // A lane it does not offer is refused locally, before any credential.
    assert_eq!(
        native_ds(&[
            "design", "presence", "status", "--lane", "nightly", "--output", "json"
        ])
        .envelope["error"]["code"],
        "invalid_choice"
    );
}

#[test]
fn design_status_reads_the_headless_project_and_never_reaches_for_a_browser() {
    let descriptor = ok(&["capabilities", "design.status", "--output", "json"]);
    let command = &descriptor["command"];
    assert_eq!(command["path"], serde_json::json!(["design", "status"]));
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "local_auth_state");
    assert_eq!(command["execution"], "sync");
    let inputs = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("input name"))
        .collect::<BTreeSet<_>>();
    assert_eq!(
        inputs,
        BTreeSet::from([
            "desc",
            "filter",
            "findings",
            "lane",
            "out",
            "project",
            "read",
            "search",
            "sort",
            "transformer"
        ])
    );

    // The project is NAMED, not inherited. This was the opposite assertion
    // until 2026-09-18: the read followed this machine's saved selection, so
    // the same command answered differently on two terminals. A read an agent
    // repeats has to mean one thing, so omitting `--project` is now the
    // refusal and passing one is ordinary.
    assert_eq!(
        native_ds(&["design", "status", "--output", "json"]).envelope["error"]["code"],
        "missing_input"
    );
    assert_eq!(
        native_refusal(&["design", "status", "--project", " p-1", "--output", "json"]),
        "project_required",
        "a padded project id is named as such before any credential is restored"
    );
    assert_eq!(
        native_refusal(&["design", "status", "--project", "a/b", "--output", "json"]),
        "context_corrupt",
        "a project id that is not one path segment is refused by name"
    );

    // A malformed scope is answered locally, before any credential restore.
    assert_eq!(
        native_refusal(&[
            "design",
            "status",
            "--project",
            "p_smoke",
            "--transformer",
            " tx_a",
            "--output",
            "json"
        ]),
        "invalid_transformer_scope"
    );

    // So is a malformed selector: the register's question is validated before
    // any credential is touched, and each way of being wrong is named.
    for (filter, why) in [
        ("bogus", "not <dimension=value>"),
        ("colour=red", "an unknown dimension"),
        ("admin-planet=x", "an unknown admin level"),
        ("sync=", "no value"),
    ] {
        let run = native_ds(&[
            "design",
            "status",
            "--project",
            "p_smoke",
            "--filter",
            filter,
            "--output",
            "json",
        ]);
        assert_eq!(
            run.envelope["error"]["code"], "design_status_query_invalid",
            "--filter {filter} ({why})"
        );
        assert_eq!(run.envelope["error"]["class"], "invalid_input");
    }
    assert_eq!(
        native_refusal(&[
            "design",
            "status",
            "--project",
            "p_smoke",
            "--filter",
            "admin-district=Karongi",
            "--filter",
            "admin-sector=Gihango",
            "--output",
            "json"
        ]),
        "design_status_query_invalid"
    );
    // A sort key outside the vocabulary is the CLI's own choices refusal.
    assert_eq!(
        native_ds(&[
            "design",
            "status",
            "--project",
            "p_smoke",
            "--sort",
            "colour",
            "--output",
            "json",
        ])
        .envelope["error"]["code"],
        "invalid_choice"
    );

    // With the development catalogue and no restorable native user, the read
    // stops at the native auth boundary — the same typed refusal every other
    // native read gives. Nothing here contacts a service or an application.
    for args in [
        vec![
            "design",
            "status",
            "--project",
            "p_smoke",
            "--output",
            "json",
        ],
        vec![
            "design",
            "status",
            "--project",
            "p_smoke",
            "--transformer",
            "tx_a",
            "--lane",
            "canary",
            "--output",
            "json",
        ],
    ] {
        let run = native_ds(&args);
        // Unauthorized, not invalid input: the request was well formed and
        // there is simply no verified principal behind it.
        assert_eq!(run.code, 4, "{}", args.join(" "));
        assert_eq!(run.envelope["error"]["class"], "unauthorized");
        assert_eq!(
            run.envelope["error"]["code"],
            "headless_signed_out",
            "{}",
            args.join(" ")
        );
        assert!(
            run.stderr.is_empty() || !run.stderr.contains("http"),
            "{} touched a network surface: {}",
            args.join(" "),
            run.stderr
        );
    }
}

/// The Dashboard is the same headless spine, folded once: the same credential,
/// the same refusals, the same absence of a browser — and a model whose every
/// attention note is the shared health verdict, so `ds` and the register can
/// never disagree about a project's state.
#[test]
fn design_dashboard_is_the_same_headless_read_folded_once() {
    let descriptor = ok(&["capabilities", "design.dashboard", "--output", "json"]);
    let command = &descriptor["command"];
    assert_eq!(command["path"], serde_json::json!(["design", "dashboard"]));
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "local_auth_state");
    assert_eq!(command["execution"], "sync");
    assert_eq!(command["availability"], "unavailable");
    let inputs = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("input name"))
        .collect::<BTreeSet<_>>();
    // No `--transformer`: every percentage here is measured against the fleet,
    // so a dashboard over a subset would be a different question. And no
    // `--fast`: there is one way to process a transformer, so the dashboard
    // has no lane split to read the project through.
    assert_eq!(
        inputs,
        BTreeSet::from(["lane", "project", "tz-offset-minutes"])
    );
    assert_eq!(
        native_ds(&[
            "design",
            "dashboard",
            "--project",
            "p_smoke",
            "--fast",
            "--output",
            "json"
        ])
        .envelope["error"]["code"],
        "unknown_flag"
    );
    for retired in ["lanes", "fast_lane", "process_lane_count", "Fast lane"] {
        assert!(
            !command["output"]
                .as_str()
                .expect("output")
                .contains(retired),
            "the dashboard descriptor still speaks of `{retired}`"
        );
    }
    // The descriptor says out loud that a headless client holds no live
    // diagnostics, so a reader is never surprised by their absence.
    assert!(
        command["output"]
            .as_str()
            .expect("output")
            .contains("headless client holds none"),
    );

    // The project is the argument. Until 2026-09-18 this asserted the exact
    // opposite — `--project` was an `unknown_flag` and the Dashboard followed
    // the saved selection. It now refuses to guess which project was meant.
    assert_eq!(
        native_ds(&["design", "dashboard", "--output", "json"]).envelope["error"]["code"],
        "missing_input"
    );
    assert_eq!(
        native_refusal(&["design", "dashboard", "--project", "..", "--output", "json"]),
        "context_corrupt"
    );
    assert_eq!(
        native_ds(&[
            "design",
            "dashboard",
            "--project",
            "p_smoke",
            "--transformer",
            "tx_a",
            "--output",
            "json"
        ])
        .envelope["error"]["code"],
        "unknown_flag"
    );

    // With no restorable native user the read stops at the auth boundary —
    // the same typed refusal the rest of the spine gives, and no network.
    for args in [
        vec![
            "design",
            "dashboard",
            "--project",
            "p_smoke",
            "--output",
            "json",
        ],
        vec![
            "design",
            "dashboard",
            "--project",
            "p_smoke",
            "--lane",
            "canary",
            "--output",
            "json",
        ],
    ] {
        let run = native_ds(&args);
        assert_eq!(run.code, 4, "{}", args.join(" "));
        assert_eq!(run.envelope["error"]["class"], "unauthorized");
        assert_eq!(
            run.envelope["error"]["code"],
            "headless_signed_out",
            "{}",
            args.join(" ")
        );
        assert!(
            run.stderr.is_empty() || !run.stderr.contains("http"),
            "{} touched a network surface: {}",
            args.join(" "),
            run.stderr
        );
    }
}

/// Design Activities is the one Design answer that spans projects, and the one
/// most able to mislead: it is about people. So the smoke test checks the two
/// things a reader's trust actually rests on — that a sweep cannot be taken
/// casually or in parallel, and that every reply says out loud what a capture
/// cannot show.
#[test]
fn design_activities_capture_is_deliberate_sequential_and_honest_about_its_gaps() {
    let sweep = ok(&[
        "capabilities",
        "design.activities.sweep",
        "--output",
        "json",
    ])["command"]
        .clone();
    assert_eq!(
        sweep["path"],
        serde_json::json!(["design", "activities", "sweep"])
    );
    assert_eq!(sweep["effect"], "artifact_write");
    assert_eq!(sweep["authority"], "headless_user");
    assert_eq!(sweep["requires"], "server");
    // A durable capture of record, so `--yes` is mandatory — and the descriptor
    // says why it must be walked one project at a time.
    assert_eq!(sweep["confirmation_required"], true);
    let purpose = sweep["purpose"].as_str().expect("purpose");
    assert!(purpose.contains("ONE full"), "{purpose}");
    assert!(purpose.contains("Nothing is scheduled"), "{purpose}");
    assert!(purpose.contains("never aborts the sweep"), "{purpose}");
    // The default bound is small on purpose: every full scan queues behind
    // every other one on the same ds-brain instance.
    let limit = sweep["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .find(|input| input["name"] == "limit")
        .expect("--limit");
    assert_eq!(limit["default"], "25");

    let read =
        ok(&["capabilities", "design.activities.read", "--output", "json"])["command"].clone();
    assert_eq!(read["effect"], "read_only");
    assert_eq!(read["authority"], "headless_user");
    assert_eq!(read["confirmation_required"], false);
    let purpose = read["purpose"].as_str().expect("purpose");
    assert!(purpose.contains("no row carries a device"), "{purpose}");
    assert!(purpose.contains("ENTIRE history"), "{purpose}");

    // An unconfirmed sweep refuses before it reads a credential, let alone a
    // project — the confirmation is the gate, not an afterthought.
    assert_eq!(
        native_refusal(&["design", "activities", "sweep", "--output", "json"]),
        "confirmation_required"
    );
    // A relative store is refused locally, by name, before any credential.
    assert_eq!(
        native_refusal(&[
            "design",
            "activities",
            "read",
            "--state-dir",
            "relative/store",
            "--output",
            "json"
        ]),
        "store_unsafe"
    );
    assert_eq!(
        native_refusal(&[
            "design",
            "activities",
            "read",
            "--limit",
            "0",
            "--output",
            "json"
        ]),
        "invalid_number"
    );
    // With no restorable native user, reading the store stops at the auth
    // boundary and never opens a socket.
    let run = native_ds(&["design", "activities", "read", "--output", "json"]);
    assert_eq!(run.envelope["error"]["code"], "headless_signed_out");
    assert!(
        run.stderr.is_empty() || !run.stderr.contains("http"),
        "the offline read touched a network surface: {}",
        run.stderr
    );
}



#[test]
fn profile_analysis_is_an_explicit_native_set_action_without_an_extra_window_command() {
    let descriptor = ok(&["capabilities", "map.profile.set", "--output", "json"]);
    assert_eq!(descriptor["command"]["effect"], "local_ui");
    assert_eq!(descriptor["command"]["authority"], "desktop_pairing");
    let action = descriptor["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|arg| arg["name"] == "action")
        .unwrap();
    assert_eq!(
        action["choices"],
        serde_json::json!(["fit", "rebuild", "analyze"])
    );
    assert_eq!(
        refusal(&[
            "map",
            "profile",
            "set",
            "--action",
            "compute-arbitrary",
            "--output",
            "json"
        ]),
        "invalid_choice"
    );
}

#[test]
fn profile_edit_mode_requires_reviewed_history_before_contacting_desktop() {
    let history = r#"{"model_revision":"rev:head","initial_revision":"rev:base","undo_depth":0,"redo_depth":0,"history_pin":"pin"}"#;
    for extra in [
        vec!["--history", r#"{"project":"untrusted"}"#],
        vec!["--history", history, "--action", "analyze"],
    ] {
        let mut args = vec![
            "map",
            "profile",
            "set",
            "--edit-mode",
            "true",
            "--model",
            "local-test",
            "--output",
            "json",
        ];
        args.extend(extra);
        assert_eq!(refusal(&args), "invalid_profile_view");
    }
}

#[test]
fn ui_camera_computation_is_absent_from_cli() {
    let index = ok(&["capabilities", "map", "--output", "json"]);
    assert!(
        index["commands"]
            .as_array()
            .unwrap()
            .iter()
            .all(|command| command["id"] != "map.canvas.camera")
    );
    assert_eq!(
        refusal(&["map", "canvas", "camera", "--output", "json"]),
        "unknown_command"
    );
}

#[test]
fn every_map_command_is_reachable_without_the_desktop_installed() {
    // Availability here is deliberately unconditional: dispatch checks it
    // before parsing, so a gate would make `--desktop-descriptor` — the flag
    // that names a descriptor discovery did not find — unreachable, and would
    // put every input refusal above out of reach on a machine with no app.
    let index = ok(&["capabilities", "map", "--output", "json"]);
    let commands = index["commands"].as_array().expect("commands");
    let actual: BTreeSet<&str> = commands
        .iter()
        .map(|command| command["id"].as_str().expect("command id"))
        .collect();
    let expected: BTreeSet<&str> = [
        "map.print.schema",
        "map.data.inspect",
        "map.data.list",
        "map.data.upload",
        "map.data.remove",
        "map.view",
        "map.camera.set",
        "map.profile.view",
        "map.profile.set",
        "map.profile.select",
        "map.profile.pick",
        "map.profile.usage",
        "map.profile.retype",
        "map.profile.edit",
        "map.profile.save",
        "map.profile.results",
        "map.profile.calculation",
        "map.profile.issues",
        "map.profile.filter",
        "map.grid.lasso",
        "map.draw",
        "map.remove",
        "map.zoom",
        "map.layer.list",
        "map.layer.default",
        "map.layer.reorder",
        "map.layer.remote-list",
        "map.layer.add",
        "map.layer.remove",
        "map.layer.visibility",
        "map.layer.show",
        "map.layer.hide",
        "map.local.list",
        "map.local.register",
        "map.local.rename",
        "map.layer.registry",
        "map.layer.registry-edit",
        "map.local.remove",
        "map.ui.open",
        "map.evidence.capture",
        "map.points-along",
        "map.random-points",
        "map.outliers",
        "map.line-difference",
        "map.survey.download",
        "map.design.open",
        "map.design.read",
        "map.design.discard",
        "map.design.layer-to-local",
        "map.design.upload-to-local",
        "map.design.select",
        "map.design.set",
        "map.design.create",
        "map.design.delete",
        "map.design.geometry",
        "map.design.setup",
        "map.design.version.play",
        "map.design.version.compare",
        "map.design.process",
        "map.design.batch.process",
        "map.design.batch.save",
        "map.design.save",
        "map.design.list",
        "map.design.pin",
        "map.design.report",
        "map.design.attach-print",
        "map.design.upload.inspect",
        "map.design.upload.stage",
    ]
    .into_iter()
    .collect();
    assert_eq!(
        actual, expected,
        "map command coverage list changed; add a specific smoke assertion for the new command before accepting it"
    );
    // The retained overlay registry (6ccc4c9c) is a headless machine-local
    // read and its one local edit; neither needs a window or an account.
    for (id, effect) in [
        ("map.layer.registry", "read_only"),
        ("map.layer.registry-edit", "local_file_write"),
    ] {
        let command = &ok(&["capabilities", id, "--output", "json"])["command"];
        assert_eq!(command["effect"], effect, "{id}");
        assert_eq!(command["authority"], "none", "{id}");
        assert_eq!(command["requires"], "server", "{id}");
    }
    let history = r#"{"model_revision":"rev:head","initial_revision":"rev:head","undo_depth":0,"redo_depth":0,"history_pin":"pin"}"#;
    assert_eq!(
        refusal(&[
            "map",
            "profile",
            "save",
            "--model",
            "local-test",
            "--history",
            history,
            "--output",
            "json"
        ]),
        "confirmation_required"
    );
    assert_eq!(
        refusal(&[
            "map",
            "profile",
            "save",
            "--model",
            "local-test",
            "--history",
            "{}",
            "--yes",
            "--output",
            "json"
        ]),
        "invalid_profile_model_request"
    );
    assert_eq!(
        refusal(&[
            "map",
            "profile",
            "usage",
            "--model",
            " ",
            "--revision",
            "rev:a",
            "--structure",
            "pole-40",
            "--output",
            "json"
        ]),
        "invalid_profile_model_request"
    );
    assert_eq!(
        refusal(&[
            "map",
            "profile",
            "retype",
            "--model",
            "a",
            "--revision",
            "rev:a",
            "--structure",
            "pole-40",
            "--type",
            "type-b",
            "--output",
            "json"
        ]),
        "confirmation_required"
    );
    assert_eq!(
        refusal(&[
            "map",
            "profile",
            "issues",
            "--model",
            "a",
            "--revision",
            "rev:a",
            "--request",
            "{\"filter\":{\"nature\":\"invented-predicate\"}}",
            "--output",
            "json"
        ]),
        "invalid_profile_query"
    );
    assert_eq!(
        refusal(&[
            "map",
            "profile",
            "filter",
            "--model",
            "a",
            "--revision",
            "rev:a",
            "--table",
            "structures",
            "--mode",
            "intersect",
            "--query",
            "{}",
            "--output",
            "json"
        ]),
        "confirmation_required"
    );
    assert_eq!(
        refusal(&[
            "map",
            "profile",
            "filter",
            "--model",
            "a",
            "--revision",
            "rev:a",
            "--table",
            "structures",
            "--mode",
            "intersect",
            "--query",
            "{\"filters\":[{\"column\":\"number\",\"op\":\"invented\",\"value\":\"40\"}]}",
            "--dry-run",
            "--output",
            "json"
        ]),
        "invalid_profile_model_request"
    );
    let live_usage = ok(&["capabilities", "map.profile.usage", "--output", "json"]);
    assert_eq!(live_usage["command"]["effect"], "read_only");
    assert!(
        live_usage["command"]["output"]
            .as_str()
            .unwrap()
            .contains("native QueryResults page")
    );
    let profile_select = ok(&["capabilities", "map.profile.select", "--output", "json"]);
    assert_eq!(profile_select["command"]["effect"], "local_ui");
    assert_eq!(profile_select["command"]["authority"], "desktop_pairing");
    assert_eq!(profile_select["command"]["contract"], 2);
    let selection_purpose = profile_select["command"]["purpose"]
        .as_str()
        .expect("purpose");
    assert!(selection_purpose.contains("Native scene order owns"));
    assert!(selection_purpose.contains("closed structure_id/attachment_point_id JSON pair"));
    assert!(selection_purpose.contains("one occurrence"));
    assert_eq!(
        refusal(&[
            "map",
            "profile",
            "select",
            "--model",
            " ",
            "--revision",
            "rev-a",
            "--from",
            "pole-1",
            "--to",
            "pole-1",
            "--output",
            "json",
        ]),
        "invalid_profile_selection"
    );
    let printing = ok(&["map", "print", "schema", "--output", "json"]);
    assert!(
        printing["sections"]
            .as_array()
            .is_some_and(|s| s.len() == 5)
    );
    for command in commands {
        if matches!(
            command["id"].as_str(),
            Some(
                "map.layer.list"
                    | "map.layer.default"
                    | "map.layer.reorder"
                    | "map.layer.show"
                    | "map.layer.hide"
                    | "map.data.list"
                    | "map.data.upload"
                    | "map.data.remove"
            )
        ) {
            // Native discovery reflects this build's packaged profile. It
            // may be configured even when no Desktop is installed or paired.
            assert!(matches!(
                command["availability"].as_str(),
                Some("available" | "unavailable")
            ));
            let descriptor = ok(&[
                "capabilities",
                command["id"].as_str().unwrap(),
                "--output",
                "json",
            ]);
            assert_eq!(descriptor["command"]["requires"], "server");
        } else {
            assert!(
                discoverable(command),
                "`{}` reports {} and gates on discovery, which puts --desktop-descriptor out of reach",
                command["id"],
                command["availability"]
            );
        }
    }
}

#[test]
fn profile_pick_is_read_only_and_refuses_invalid_css_points_before_pairing() {
    let descriptor = ok(&["capabilities", "map.profile.pick", "--output", "json"]);
    let command = &descriptor["command"];
    assert_eq!(command["effect"], "read_only");
    assert_eq!(command["authority"], "desktop_pairing");
    assert!(
        command["purpose"]
            .as_str()
            .unwrap()
            .contains("mounted canvas")
    );
    assert!(command["output"].as_str().unwrap().contains("8 MiB"));
    assert!(
        command["output"]
            .as_str()
            .unwrap()
            .contains("attachment_ref")
    );
    let history = r#"{"model_revision":"head","initial_revision":"initial","undo_depth":2,"redo_depth":1,"history_pin":"native-prefix"}"#;
    for x in ["NaN", "inf", "not-a-number"] {
        assert_eq!(
            refusal(&[
                "map",
                "profile",
                "pick",
                "--model",
                "model-a",
                "--history",
                history,
                "--context",
                "retained-context",
                "--x",
                x,
                "--y",
                "1",
                "--output",
                "json"
            ]),
            "invalid_profile_pick"
        );
    }
    let view = ok(&["capabilities", "map.profile.view", "--output", "json"]);
    assert_eq!(view["command"]["contract"], 7);
    assert!(
        view["command"]["output"]
            .as_str()
            .unwrap()
            .contains("pick_context")
    );
}

#[test]
fn map_design_open_is_the_one_discoverable_visible_context_entry() {
    let descriptor = ok(&["capabilities", "map.design.open", "--output", "json"]);
    let command = &descriptor["command"];
    assert_eq!(command["authority"], "project");
    assert_eq!(command["effect"], "local_ui");
    assert_eq!(command["confirmation_required"], false);
    let inputs = command["inputs"].as_array().expect("inputs");
    assert_eq!(
        inputs
            .iter()
            .map(|input| input["name"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["transformer", "target", "desktop-descriptor"]
    );
    assert_eq!(inputs[0]["required"], true);
    let refusals = command["refusals"]
        .as_array()
        .expect("refusals")
        .iter()
        .map(|refusal| refusal["code"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        refusals,
        BTreeSet::from([
            "desktop_not_paired",
            "desktop_ambiguous",
            // Opening a design context is project work, so it can meet the
            // window that is on another project — refused by name now, where
            // it used to be switched underneath the operator.
            "desktop_project_not_open",
            "desktop_unreachable",
            "pairing_rejected",
            "desktop_signed_out",
            "desktop_refused",
            "transformer_not_found",
            "auth_context_mismatch",
            "project_mismatch",
            "dirty_room",
            "desktop_operation_unsupported",
            "desktop_unreadable",
        ])
    );

    for query in [
        "map open transformer",
        "transformer edit context",
        "activate design",
    ] {
        let result = ok(&["capabilities", "--search", query, "--output", "json"]);
        let matches = result["results"].as_array().expect("search results");
        assert!(
            matches.iter().any(|row| row["id"] == "map.design.open"),
            "`{query}` did not find the canonical context-entry command: {matches:?}"
        );
    }
}

#[test]
fn map_design_pin_exposes_complete_working_set_parity() {
    let descriptor = ok(&["capabilities", "map.design.pin", "--output", "json"]);
    let command = &descriptor["command"];
    assert_eq!(command["contract"], 2);
    assert_eq!(command["authority"], "project");
    assert_eq!(command["effect"], "local_ui");
    let mode = command["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .find(|input| input["name"] == "mode")
        .expect("mode input");
    assert_eq!(
        mode["choices"],
        json!(["read", "set", "add", "remove", "unpin", "load", "clear"])
    );

    let search = ok(&[
        "capabilities",
        "--search",
        "read unpin clear load selection working set",
        "--output",
        "json",
    ]);
    assert!(
        search["results"]
            .as_array()
            .expect("search results")
            .iter()
            .any(|row| row["id"] == "map.design.pin")
    );
}

#[test]
fn map_design_version_history_is_discoverable_and_governed() {
    let expected = [
        (
            "map.design.version.play",
            "local_ui",
            vec!["transformer", "version", "revision", "target", "desktop-descriptor"],
        ),
        (
            "map.design.version.compare",
            "local_ui",
            vec!["transformer", "from", "to", "target", "desktop-descriptor"],
        ),
    ];
    for (id, effect, inputs) in expected {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let command = &descriptor["command"];
        assert_eq!(command["authority"], "project", "{id}");
        assert_eq!(command["effect"], effect, "{id}");
        assert_eq!(command["confirmation_required"], false, "{id}");
        assert_eq!(
            command["inputs"]
                .as_array()
                .expect("inputs")
                .iter()
                .map(|input| input["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            inputs,
            "{id}",
        );
    }

    for (query, id) in [
        ("play transformer version", "map.design.version.play"),
        ("compare design version head", "map.design.version.compare"),
    ] {
        let result = ok(&["capabilities", "--search", query, "--output", "json"]);
        assert!(
            result["results"]
                .as_array()
                .expect("results")
                .iter()
                .any(|row| row["id"] == id),
            "`{query}` did not discover {id}",
        );
    }
}

#[test]
fn a_well_formed_map_call_stops_at_confirmation_or_pairing() {
    // This test intentionally never passes --yes. A real paired desktop could
    // otherwise execute a shared write while running a smoke suite. Reads
    // reach the pairing boundary; writes must stop at confirmation first.
    let descriptor = temp_root("map-smoke-unreachable")
        .join("session.json")
        .display()
        .to_string();
    for args in [
        vec!["map", "view"],
        vec!["map", "zoom", "--bbox", "29.9,-2.1,30.2,-1.85"],
        vec![
            "map",
            "ui",
            "open",
            "--target",
            "attribute-table",
            "--ref",
            "master/customers",
        ],
        vec!["map", "zoom", "--layer", "sketch-does-not-exist"],
        vec!["map", "remove", "--layer", "sketch-does-not-exist"],
        vec![
            "map",
            "points-along",
            "--layer",
            "sketch:x",
            "--interval-m",
            "25",
        ],
        vec!["map", "random-points", "--layer", "sketch:x"],
        vec!["map", "outliers", "--layer", "sketch:x"],
        vec![
            "map",
            "line-difference",
            "--source-layer",
            "sketch:incoming",
            "--base-layer",
            "sketch:base",
            "--name",
            "difference",
        ],
        vec!["map", "design", "read", "--transformer", "T-1042"],
        vec![
            "map",
            "design",
            "layer-to-local",
            "--transformer",
            "T-1042",
            "--layer",
            "lv_lines",
            "--name",
            "base",
        ],
        vec![
            "map",
            "design",
            "upload-to-local",
            "--path",
            "survey.zip",
            "--source-layer",
            "lv_lines",
            "--name",
            "incoming",
        ],
        vec!["map", "design", "select", "--transformer", "T-1042"],
        vec!["map", "design", "set", "--transformer", "T", "--set", "a=b"],
        vec![
            "map",
            "design",
            "create",
            "--transformer",
            "T",
            "--source-layer",
            "sketch:difference",
            "--target-layer",
            "lv_lines",
        ],
        vec!["map", "design", "process", "--transformer", "T-1042"],
        vec![
            "map",
            "design",
            "setup",
            "--survey-layer",
            "edcl_customers_survey",
            "--preset",
            "drafting",
            "--dry-run",
        ],
        vec![
            "map",
            "design",
            "delete",
            "--transformer",
            "T-1042",
            "--where",
            "pole_status=duplicate",
        ],
        vec![
            "map",
            "design",
            "geometry",
            "--transformer",
            "T-1042",
            "--id",
            "lv_poles#1",
            "--geometry",
            r#"{"type":"Point","coordinates":[30.06,-1.95]}"#,
        ],
        vec!["map", "design", "list"],
        vec![
            "map",
            "design",
            "pin",
            "--transformer",
            "T-1042",
            "--mode",
            "add",
        ],
        vec![
            "map",
            "design",
            "batch",
            "process",
            "--transformer",
            "T-1042",
        ],
        vec!["map", "design", "batch", "save", "--transformer", "T-1042"],
        vec!["map", "design", "upload", "inspect", "--path", "survey.zip"],
        vec![
            "map",
            "design",
            "upload",
            "stage",
            "--source",
            "T-1042=survey.zip",
        ],
        vec!["map", "design", "save", "--transformer", "T-1042"],
        vec!["map", "design", "report", "--transformer", "T-1042"],
        vec![
            "map",
            "design",
            "attach-print",
            "--path",
            "/deliverables/T-1042-A3.pdf",
            "--transformer",
            "T-1042",
            "--map-family",
            "lv-atlas",
            "--layout",
            "LV A3",
            "--paper-size",
            "A3",
            "--orientation",
            "landscape",
        ],
    ] {
        let mut argv = args.clone();
        argv.extend(["--desktop-descriptor", &descriptor, "--output", "json"]);
        let code = refusal(&argv);
        assert!(
            code == "confirmation_required" || PAIRING_CODES.contains(&code.as_str()),
            "`ds {}` failed with `{code}`, which is neither the confirmation \
             gate for a write nor a pairing outcome for a read.",
            args.join(" ")
        );
    }
}

// ---------------------------------------------------------------------------
// design
// ---------------------------------------------------------------------------
//
// Native LV processing has two file-owned commands in this domain: a
// governed project snapshot export and the project-free native process. They are intentionally
// exercised here before the paired collaboration refusals below.

#[test]
#[ignore = "requires an explicitly selected compatible Network executable"]
fn design_lv_process_runs_the_native_batch_without_project_or_desktop_state() {
    let root = temp_root("native-fast-lv");
    std::fs::create_dir_all(&root).unwrap();
    let input_path = root.join("request.json");
    let output_path = root.join("result.json");
    let job = |name: &str| {
        json!({
            "transformer_name": name,
            "gdfs": {
                "tr": {
                    "type": "FeatureCollection",
                    "features": [{
                        "type": "Feature",
                        "id": format!("{name}-tr"),
                        "geometry": { "type": "Point", "coordinates": [30.0, -2.0] },
                        "properties": { "name": name, "names": name }
                    }]
                },
                "lv_lines": {
                    "type": "FeatureCollection",
                    "features": [{
                        "type": "Feature",
                        "id": format!("{name}-line"),
                        "geometry": { "type": "LineString", "coordinates": [[30.0, -2.0], [30.0004, -2.0]] },
                        "properties": {}
                    }]
                },
                "customers": { "type": "FeatureCollection", "features": [] }
            },
            "settings": {}
        })
    };
    std::fs::write(
        &input_path,
        serde_json::to_vec(&json!({
            "schema": "ds.fast-lv.request/v1",
            "jobs": [job("T2"), job("T1")]
        }))
        .unwrap(),
    )
    .unwrap();

    let input = input_path.display().to_string();
    let output = output_path.display().to_string();
    let run = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "design", "lv", "process", "--input", &input, "--out", &output, "--output", "json",
        ])
        // A stale explicit Desktop descriptor would fail any bridge-backed
        // command. The native LV process must never inspect it.
        .env("DS_DESKTOP_DESCRIPTOR", root.join("stale-desktop.json"))
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    assert_eq!(
        run.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let envelope: Value = serde_json::from_slice(&run.stdout).unwrap();
    let receipt = &envelope["data"];
    assert_eq!(receipt["execution_environment"], "native");
    assert_eq!(receipt["jobs"], 2);
    assert_eq!(receipt["succeeded"], 2);
    assert_eq!(receipt["failed"], 0);
    assert_eq!(receipt["results"][0]["transformer_name"], "T2");
    assert_eq!(receipt["results"][1]["transformer_name"], "T1");

    let result: Value = serde_json::from_slice(&std::fs::read(&output_path).unwrap()).unwrap();
    assert_eq!(result["schema"], "ds.fast-lv.result/v1");
    assert_eq!(result["jobs"][0]["transformer_name"], "T2");
    assert!(result.get("ds_project").is_none());
    assert!(result["jobs"][0].get("ds_project").is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn design_lv_voltage_drop_checks_reg_compliance_without_project_or_desktop_state() {
    let root = temp_root("native-lv-voltage-drop");
    std::fs::create_dir_all(&root).unwrap();
    let input_path = root.join("request.json");
    let output_path = root.join("result.vd.json");
    let electrical = |params: Value| params.to_string();
    let abc = |name: &str, r: f64, x: f64, amps: f64| {
        json!({
            "clean_name": name,
            "electrical_params": electrical(json!({
                "r_ohm_per_km": r, "x_ohm_per_km": x, "max_i_ka": amps / 1000.0
            })),
        })
    };
    // The seeds `design lv project-export --project-config` carries: the
    // builder's project settings, cos φ, the customer category that gives a
    // load its watts, the ABC ladder, one service cable and the transformer
    // sizes, each with its electrical parameters.
    let config = json!({
        "project_settings": [
            { "parameter": "cos_phi", "value": 0.85 },
            { "parameter": "sc_real_length", "value": "x + (x*0.01) + 6" },
            { "parameter": "max_service_length", "value": 37 },
            { "parameter": "pole_spacing", "value": 50 },
        ],
        "cust_category": [{
            "clean_name": "Residential",
            "peak_power_w": 250,
            "misspelled_names": ["Residential"],
        }],
        "lv_lines": [
            abc("3 x 35 + 54.6mm² ABC", 0.868, 0.0982, 132.0),
            abc("3 x 70 + 54.6mm² ABC", 0.443, 0.0893, 205.0),
        ],
        "service_cable_sizes": [{
            "service_cable_size": "2 x 6 mm²",
            "meter_type": "Single Phase",
            "electrical_params": electrical(json!({
                "r_ohm_per_km": 3.08, "x_ohm_per_km": 0.09, "max_i_ka": 0.039
            })),
        }],
        "transfo_sizes": [{
            "transfo_sizes": 100,
            "electrical_params": electrical(json!({ "vk_percent": 4.0, "vkr_percent": 1.45 })),
        }],
    });
    // A 100 kVA transformer and four 250 W single-phase customers beside a
    // ~200 m line: comfortably within REG's ±10 %.
    let customers: Vec<Value> = (0..4)
        .map(|index| {
            json!({
                "type": "Feature",
                "id": format!("c{index}"),
                "geometry": { "type": "Point", "coordinates": [30.0003 + 0.0004 * index as f64, -2.00008] },
                "properties": {
                    "load": "Residential, 250", "meter_type": "Single Phase",
                    "service_cable_size": "2 x 6 mm²",
                }
            })
        })
        .collect();
    std::fs::write(
        &input_path,
        serde_json::to_vec(&json!({
            "schema": "ds.fast-lv.request/v1",
            "jobs": [{
                "transformer_name": "VD1",
                "gdfs": {
                    "tr": { "type": "FeatureCollection", "features": [{
                        "type": "Feature", "id": "VD1-tr",
                        "geometry": { "type": "Point", "coordinates": [30.0, -2.0] },
                        "properties": { "name": "VD1", "names": "VD1", "transfo_size": 100 }
                    }]},
                    "lv_lines": { "type": "FeatureCollection", "features": [{
                        "type": "Feature", "id": "VD1-line",
                        "geometry": { "type": "LineString", "coordinates": [[30.0, -2.0], [30.0018, -2.0]] },
                        "properties": { "cable_size": "3 x 35 + 54.6mm² ABC" }
                    }]},
                    "customers": { "type": "FeatureCollection", "features": customers },
                },
                "settings": {},
                "config_dfs": config,
            }]
        }))
        .unwrap(),
    )
    .unwrap();

    let input = input_path.display().to_string();
    let output = output_path.display().to_string();
    let run = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "design",
            "lv",
            "voltage-drop",
            "--input",
            &input,
            "--out",
            &output,
            "--output",
            "json",
        ])
        // Like the process it wraps, the check never inspects a Desktop.
        .env("DS_DESKTOP_DESCRIPTOR", root.join("stale-desktop.json"))
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    assert_eq!(
        run.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    let envelope: Value = serde_json::from_slice(&run.stdout).unwrap();
    let receipt = &envelope["data"];
    assert_eq!(receipt["method"], "ds-lv-vd/1");
    assert_eq!(receipt["jobs"], 1);
    assert_eq!(
        receipt["succeeded"],
        1,
        "{}",
        std::fs::read_to_string(&output_path).unwrap_or_default()
    );
    let row = &receipt["results"][0];
    assert_eq!(row["transformer_name"], "VD1");
    assert_eq!(row["status"], "calculated");
    assert_eq!(row["customers"], 4);
    assert_eq!(row["customers_failing"], 0);
    assert_eq!(row["compliant"], true);
    assert_eq!(row["sizing_status"], "compliant_as_drawn");
    let worst = row["worst_vd_pct"].as_f64().expect("a worst drop");
    assert!(worst > 0.0 && worst < 10.0, "worst drop {worst} %");

    let result: Value = serde_json::from_slice(&std::fs::read(&output_path).unwrap()).unwrap();
    assert_eq!(result["schema"], "ds.lv-voltage-drop.result/v1");
    let customers = result["jobs"][0]["layers"]["customers"]["features"]
        .as_array()
        .expect("processed customers");
    assert_eq!(customers.len(), 4);
    assert!(customers.iter().all(|customer| {
        customer["properties"]["vd_pct"]
            .as_f64()
            .is_some_and(|pct| pct > 0.0)
            && customer["properties"]["vd_ok"] == true
    }));
    // Four equal single-phase loads are balanced across all three phases.
    let phases: BTreeSet<&str> = customers
        .iter()
        .filter_map(|customer| customer["properties"]["connection_phase"].as_str())
        .collect();
    assert_eq!(phases.len(), 3, "{phases:?}");
    let _ = std::fs::remove_dir_all(&root);
}

/// The owner's per-customer load question, asked of the real binary: one
/// ~1.2 km 3 x 35 mm² feeder of 150 Residential customers (250 W at
/// saturation, 60 W at commissioning) fails within its outlook as
/// configured, complies at year 0, and complies in every year at 100 W. Each
/// run echoes its scenario, and none of them changes the request file.
#[test]
fn design_lv_voltage_drop_scenarios_answer_the_per_customer_load_question() {
    let root = temp_root("native-lv-voltage-drop-scenario");
    std::fs::create_dir_all(&root).unwrap();
    let input_path = root.join("request.json");
    let electrical = |params: Value| params.to_string();
    let abc = |name: &str, r: f64, x: f64, amps: f64| {
        json!({
            "clean_name": name,
            "electrical_params": electrical(json!({
                "r_ohm_per_km": r, "x_ohm_per_km": x, "max_i_ka": amps / 1000.0
            })),
        })
    };
    let config = json!({
        "project_settings": [
            { "parameter": "cos_phi", "value": 0.85 },
            { "parameter": "sc_real_length", "value": "x + (x*0.01) + 6" },
            { "parameter": "max_service_length", "value": 37 },
            { "parameter": "pole_spacing", "value": 50 },
        ],
        "cust_category": [{
            "clean_name": "Residential",
            "peak_power_w": 250,
            "initial_power_w": 60,
            "misspelled_names": ["Residential"],
        }],
        "lv_lines": [
            abc("3 x 35 + 54.6mm² ABC", 0.868, 0.0982, 132.0),
            abc("3 x 70 + 54.6mm² ABC", 0.443, 0.0893, 205.0),
            abc("3 x 120 + 54.6mm² ABC", 0.253, 0.0855, 300.0),
        ],
        "service_cable_sizes": [{
            "service_cable_size": "2 x 6 mm²",
            "meter_type": "Single Phase",
            "electrical_params": electrical(json!({
                "r_ohm_per_km": 3.08, "x_ohm_per_km": 0.09, "max_i_ka": 0.039
            })),
        }],
        "transfo_sizes": [
            { "transfo_sizes": 100, "electrical_params": electrical(json!({ "vk_percent": 4.0, "vkr_percent": 1.45 })) },
            { "transfo_sizes": 160, "electrical_params": electrical(json!({ "vk_percent": 4.0, "vkr_percent": 1.375 })) },
        ],
    });
    let length_deg = 0.0108;
    let customers: Vec<Value> = (0..150)
        .map(|index| {
            let x = 30.0002 + (length_deg - 0.0002) * index as f64 / 150.0;
            json!({
                "type": "Feature",
                "id": format!("c{index}"),
                "geometry": { "type": "Point", "coordinates": [x, -2.00008] },
                "properties": {
                    "load": "Residential, 250", "meter_type": "Single Phase",
                    "service_length": 10.0, "service_cable_size": "2 x 6 mm²",
                }
            })
        })
        .collect();
    let request = serde_json::to_vec(&json!({
        "schema": "ds.fast-lv.request/v1",
        "jobs": [{
            "transformer_name": "LONG",
            "gdfs": {
                "tr": { "type": "FeatureCollection", "features": [{
                    "type": "Feature", "id": "LONG-tr",
                    "geometry": { "type": "Point", "coordinates": [30.0, -2.0] },
                    "properties": { "name": "LONG", "names": "LONG", "transfo_size": 100 }
                }]},
                "lv_lines": { "type": "FeatureCollection", "features": [{
                    "type": "Feature", "id": "LONG-line",
                    "geometry": { "type": "LineString", "coordinates": [[30.0, -2.0], [30.0 + length_deg, -2.0]] },
                    "properties": { "cable_size": "3 x 35 + 54.6mm² ABC" }
                }]},
                "customers": { "type": "FeatureCollection", "features": customers },
            },
            "settings": {},
            "config_dfs": config,
        }]
    }))
    .unwrap();
    std::fs::write(&input_path, &request).unwrap();

    let input = input_path.display().to_string();
    let check = |name: &str, format: &str, flags: &[&str]| {
        let output = root.join(name).display().to_string();
        let mut args = vec![
            "design",
            "lv",
            "voltage-drop",
            "--input",
            &input,
            "--out",
            &output,
        ];
        args.extend_from_slice(flags);
        args.extend_from_slice(&["--output", format]);
        Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(&args)
            .env("DS_DESKTOP_DESCRIPTOR", root.join("stale-desktop.json"))
            .env("NO_COLOR", "1")
            .output()
            .expect("ds binary runs")
    };
    let receipt = |name: &str, flags: &[&str]| -> Value {
        let run = check(name, "json", flags);
        assert_eq!(
            run.status.code(),
            Some(0),
            "{}",
            String::from_utf8_lossy(&run.stdout)
        );
        let envelope: Value = serde_json::from_slice(&run.stdout).unwrap();
        envelope["data"].clone()
    };

    // As configured: fails within the outlook, which starts compliant.
    let base = receipt("base.vd.json", &[]);
    let row = &base["results"][0];
    assert_eq!(row["compliant"], false, "{row}");
    assert_eq!(row["design_year"], 5, "{row}");
    let failing = row["first_failing_year"]
        .as_u64()
        .expect("the drawn design fails in an outlook year");
    assert!((1..=5).contains(&failing), "{row}");
    assert_eq!(row["schedule"].as_array().map(Vec::len), Some(6), "{row}");
    assert_eq!(row["schedule"][0]["as_drawn"]["compliant"], true, "{row}");
    assert_eq!(
        base["scenario"],
        json!({ "project_settings": {}, "loads": [] })
    );
    let written: Value =
        serde_json::from_slice(&std::fs::read(root.join("base.vd.json")).unwrap()).unwrap();
    assert_eq!(row["schedule"], written["jobs"][0]["sizing"]["schedule"]);

    // Year 0 at the 60 W commissioning load: the same feeder complies.
    let early = receipt("year0.vd.json", &["--year", "0", "--no-outlook"]);
    let row = &early["results"][0];
    assert_eq!(row["design_year"], 0, "{row}");
    assert_eq!(row["compliant"], true, "{row}");
    assert_eq!(row["schedule"], json!([]), "{row}");
    assert_eq!(
        early["scenario"]["project_settings"],
        json!({ "vd_design_year": 0, "vd_staged_plan": false })
    );

    // Residential at 100 W: complies in every outlook year.
    let light = receipt("res100.vd.json", &["--load", "Residential=100"]);
    let row = &light["results"][0];
    assert_eq!(row["compliant"], true, "{row}");
    assert_eq!(row["customers_failing"], 0, "{row}");
    assert_eq!(row["first_failing_year"], Value::Null, "{row}");
    assert_eq!(
        light["scenario"]["loads"],
        json!([{
            "category": "Residential", "saturation_w": 100.0, "initial_w": 100.0,
            "customer_loads_rewritten": 150,
        }])
    );

    // The human outlook: one line per year, then the first failing year.
    let text = check("text.vd.json", "human", &[]);
    assert_eq!(text.status.code(), Some(0));
    let text = String::from_utf8_lossy(&text.stdout);
    for year in 0..=5 {
        assert!(
            text.contains(&format!("  year {year}: worst drop ")),
            "{text}"
        );
    }
    assert!(
        text.contains(&format!("First failing year: {failing}.")),
        "{text}"
    );

    // `--set` takes voltage-drop settings only, and refuses before writing.
    let refused = check("refused.vd.json", "json", &["--set", "cos_phi=0.9"]);
    assert_ne!(refused.status.code(), Some(0));
    let envelope: Value = serde_json::from_slice(&refused.stdout).unwrap();
    assert_eq!(
        envelope["error"]["code"], "vd_scenario_setting_refused",
        "{envelope}"
    );
    assert!(!root.join("refused.vd.json").exists());

    assert_eq!(std::fs::read(&input_path).unwrap(), request);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn design_lv_project_export_refuses_an_existing_artifact_before_auth_or_desktop() {
    let root = temp_root("native-fast-lv-project-export");
    std::fs::create_dir_all(&root).unwrap();
    let output_path = root.join("request.json");
    std::fs::write(&output_path, b"operator-owned").unwrap();
    // The reviewed development catalogue every native smoke uses; an inline
    // copy here fell behind the profile schema (v30 against compiled v31).
    let profile_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-cli-auth/tests/fixtures/development-catalog.json");
    let output = output_path.display().to_string();
    let run = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args([
            "design",
            "lv",
            "project-export",
            "--project",
            "test-project",
            "--transformer",
            "T-1",
            "--out",
            &output,
            "--output",
            "json",
        ])
        .env("DS_NATIVE_CLIENT_PROFILE_BUNDLE", &profile_path)
        .env("DS_DESKTOP_DESCRIPTOR", root.join("stale-desktop.json"))
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    assert_eq!(
        run.status.code(),
        Some(5),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let envelope: Value = serde_json::from_slice(&run.stdout).unwrap();
    assert_eq!(envelope["error"]["code"], "fast_lv_request_output_exists");
    assert_eq!(std::fs::read(&output_path).unwrap(), b"operator-owned");
    let _ = std::fs::remove_dir_all(&root);
}

//
// Design collaboration lives behind ds-brain and is reached under the native
// credential, so no fixture can stand in for a saved selection or a comment
// thread. What IS assertable on every machine, and where this domain's real
// bugs would live, is the ordering claim: every handler validates its own
// inputs and stops at the confirmation gate BEFORE it restores the native
// user. If a handler were rewritten to restore the credential first, none of
// these codes would ever be seen by anyone who is signed out.

#[test]
fn design_validates_its_own_inputs_before_it_opens_the_bridge() {
    assert_eq!(
        native_refusal(&[
            "design",
            "tag",
            "define",
            "--project",
            "test-project",
            "--definition",
            "scope",
            "--name",
            "Scope",
            "--values",
            " , ,",
            "--output",
            "json",
            "--yes",
        ]),
        "invalid_value_list",
        "a list flag with nothing in it must be refused before a project round trip"
    );
    let many = (0..600)
        .map(|index| format!("t{index}"))
        .collect::<Vec<_>>()
        .join(",");
    // Saved selections are native from Slice 14b, so the member bound is proven
    // under the development catalogue: a native command's availability gate
    // runs before its `run`, and the point of the assertion is unchanged — the
    // bound is refused locally, before any credential is spent.
    assert_eq!(
        native_refusal(&[
            "design",
            "selection",
            "save",
            "--project",
            "test-project",
            "--name",
            "Everything",
            "--transformers",
            &many,
            "--output",
            "json",
            "--yes",
        ]),
        "too_many_values",
        "a selection over the member bound must be refused locally, not by a rejected write"
    );
    assert_eq!(
        native_refusal(&[
            "design",
            "comment",
            "post",
            "--project",
            "test-project",
            "--body",
            "Looks short.",
            "--output",
            "json",
            "--yes",
        ]),
        "missing_comment_target",
        "posting with neither a thread nor a complete anchor must name the choice, not a key"
    );
    assert_eq!(
        native_refusal(&[
            "design",
            "attachment",
            "list",
            "--kind",
            "survey_entry",
            "--object",
            "x",
            "--output",
            "json",
        ]),
        "invalid_choice",
        "an object kind outside the closed set must be refused by the parser"
    );
    // Group identifiers are governed project metadata, not a hardcoded City /
    // Phase vocabulary. A syntactically valid unknown id therefore reaches
    // the native credential, on its way to ds-brain which resolves the live
    // definitions.
    assert_eq!(
        native_refusal(&[
            "design",
            "group",
            "preview",
            "--project",
            "test-project",
            "--group",
            "region",
            "--transformers",
            "kigali_a",
            "--output",
            "json",
        ]),
        "headless_signed_out",
        "a dynamic group id must reach the paired project"
    );
    // The batch bound is one transaction's write budget; the projection's is a
    // whole project's export. 300 is over the first and well inside the second,
    // which is the distinction a single shared bound would have erased.
    let three_hundred = (0..300)
        .map(|index| format!("t{index}"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        native_refusal(&[
            "design",
            "group",
            "preview",
            "--project",
            "test-project",
            "--group",
            "city",
            "--transformers",
            &three_hundred,
            "--output",
            "json",
        ]),
        "too_many_values",
        "an over-large batch must be refused locally, not by a rejected write"
    );
    let over_projection = (0..(ds_cli_design::group::MAX_PROJECTION_TRANSFORMERS + 1))
        .map(|index| format!("t{index}"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        native_refusal(&[
            "design",
            "group",
            "export",
            "--project",
            "test-project",
            "--transformers",
            &over_projection,
            "--output",
            "json",
        ]),
        "too_many_values",
        "an over-large projection must be refused locally, not by a rejected read"
    );
}

#[test]
fn unified_tagging_commands_validate_their_own_inputs_before_the_bridge() {
    // The grouping purpose is a CLOSED vocabulary. A consumer with a private
    // grouping rule is what this contract removes, so an invented purpose is
    // refused here rather than reaching a server that would refuse it anyway.
    assert_eq!(
        native_refusal(&[
            "design",
            "consumer-grouping",
            "read",
            "--project",
            "test-project",
            "--purpose",
            "district_sector",
            "--output",
            "json",
        ]),
        // The closed set is the argument's declared `choices`, enforced by the
        // parser. A second runtime check here would be one more place to drift.
        "invalid_choice",
        "a purpose outside the closed set must be refused by the parser"
    );
    // Both real purposes get past local validation and reach the native
    // credential.
    for purpose in ["solar_report", "report_archive"] {
        let code = native_refusal(&[
            "design",
            "consumer-grouping",
            "read",
            "--project",
            "test-project",
            "--purpose",
            purpose,
            "--output",
            "json",
        ]);
        assert_eq!(
            code, "headless_signed_out",
            "`{purpose}` is a real purpose and must reach the native credential, got {code}"
        );
    }

    // A consumer grouping IS its ordered definition ids. The flag borrowed
    // from `design group export` was optional and read an omission as one
    // untagged group — true of the projection, false here, so the documented
    // invocation reached the bridge and came back with a server string. It is
    // now the parser's answer, before any round trip.
    assert_eq!(
        native_refusal(&[
            "design",
            "consumer-grouping",
            "preview",
            "--project",
            "test-project",
            "--transformers",
            "T-smoke",
            "--output",
            "json",
        ]),
        "missing_input",
        "a grouping with no definitions must be refused locally, not by the server"
    );
    assert_eq!(
        native_refusal(&[
            "design",
            "consumer-grouping",
            "preview",
            "--project",
            "test-project",
            "--transformers",
            "T-smoke",
            "--definition-ids",
            " , ",
            "--output",
            "json",
        ]),
        "invalid_value_list",
        "an empty definition list must be refused locally"
    );
    let over_definitions = (0..17)
        .map(|index| format!("d{index}"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        native_refusal(&[
            "design",
            "consumer-grouping",
            "preview",
            "--project",
            "test-project",
            "--transformers",
            "T-smoke",
            "--definition-ids",
            &over_definitions,
            "--output",
            "json",
        ]),
        "too_many_values",
        "more than 16 grouping dimensions must be refused locally"
    );
    // The digest this command fences on is minted by `consumer-grouping
    // preview`. Help that named `design group preview` sent the operator to a
    // command whose assignment-batch digest apply cannot accept.
    let apply_help = native_ds(&[
        "design",
        "consumer-grouping",
        "apply",
        "--project",
        "test-project",
        "--help",
    ]);
    assert_eq!(apply_help.code, 0);
    assert!(
        apply_help
            .stdout
            .contains("`ds design consumer-grouping preview`"),
        "apply --help does not name the producer of its digest:\n{}",
        apply_help.stdout
    );
    assert!(
        !apply_help.stdout.contains("design group preview"),
        "apply --help still sends the operator to the assignment-batch preview:\n{}",
        apply_help.stdout
    );

    // The enrichment bound is one transaction's write budget: 50 transformers
    // times six administrative levels plus the definition upserts and the audit
    // row. An over-large batch is refused locally, not by a rejected write.
    let over_enrichment = (0..(ds_cli_design::tag::enrich::MAX_ENRICHMENT_TRANSFORMERS + 1))
        .map(|index| format!("t{index}"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(
        native_refusal(&[
            "design",
            "tag",
            "enrich-preview",
            "--project",
            "test-project",
            "--transformers",
            &over_enrichment,
            "--output",
            "json",
        ]),
        "too_many_values",
        "an over-large enrichment must be refused locally, not by a rejected write"
    );
    // A set within the bound reaches the native credential on its way to
    // ds-brain, which owns the governed location evidence. `ds` resolves no
    // administrative anything.
    let code = native_refusal(&[
        "design",
        "tag",
        "enrich-preview",
        "--project",
        "test-project",
        "--transformers",
        "kigali_a,kigali_b",
        "--output",
        "json",
    ]);
    assert_eq!(
        code, "headless_signed_out",
        "the enrichment plan is ds-brain's answer, never the CLI's, got {code}"
    );
}

#[test]
fn a_projection_covers_a_whole_project_where_a_batch_covers_one_transaction() {
    // A live project already carries 202 transformers. Bounding the export at
    // the batch's 200 would split the one report it exists to serve into two
    // documents with two digests, which the report consumer pins separately and
    // will not join.
    let batch = ds_cli_design::group::MAX_GROUP_BATCH;
    let projection = ds_cli_design::group::MAX_PROJECTION_TRANSFORMERS;
    assert_eq!(
        batch, 200,
        "the batch bound moved; it is a transaction budget, not a page size"
    );
    assert!(
        projection >= 2_000,
        "the projection bound is {projection}; it must cover at least 2000 explicit transformers"
    );
    // 202 and the bound itself both reach the native credential rather than
    // a local refusal, so the whole-project export is genuinely available.
    for count in [202, ds_cli_design::group::MAX_PROJECTION_TRANSFORMERS] {
        let names = (0..count)
            .map(|index| format!("t{index}"))
            .collect::<Vec<_>>()
            .join(",");
        let code = native_refusal(&[
            "design",
            "group",
            "export",
            "--project",
            "test-project",
            "--transformers",
            &names,
            "--output",
            "json",
        ]);
        assert_eq!(
            code, "headless_signed_out",
            "a {count}-transformer export ended in `{code}`, not at the native credential"
        );
    }

    // `list` is a read, and ds-brain stopped charging it a transaction's write
    // budget: `designTagGroupListingLimit` is 2,000 against the batch's 200.
    // Carrying it as the LISTING's own constant is what made that a one-line
    // change here rather than a second stress finding — the two numbers were
    // equal only because ds-brain said so, never because a read spends
    // transactions, and when the server separated them this followed.
    let listing = ds_cli_design::group::MAX_GROUP_LISTING;
    assert_eq!(
        listing, 2_000,
        "the listing bound tracks ds-brain's designTagGroupListingLimit"
    );
    assert!(
        listing > batch,
        "a read must not be bounded by a transaction's write budget"
    );
    let over_listing = (0..(listing + 1))
        .map(|index| format!("t{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let run = native_ds(&[
        "design",
        "group",
        "list",
        "--project",
        "test-project",
        "--transformers",
        &over_listing,
        "--output",
        "json",
    ]);
    assert_eq!(
        run.envelope["error"]["code"], "too_many_values",
        "an over-long listing must be refused locally, not by the server that bounds it the same"
    );
    let remedy = run.envelope["error"]["remedy"].as_str().unwrap_or_default();
    assert!(
        remedy.contains("ds design group export"),
        "the listing's refusal must name the read that covers a whole project, got `{remedy}`"
    );

    // And the bound is the server's, not one short of it: a full listing gets
    // past local validation to the native credential.
    let full_listing = (0..listing)
        .map(|index| format!("t{index}"))
        .collect::<Vec<_>>()
        .join(",");
    let code = native_refusal(&[
        "design",
        "group",
        "list",
        "--project",
        "test-project",
        "--transformers",
        &full_listing,
        "--output",
        "json",
    ]);
    assert_eq!(
        code, "headless_signed_out",
        "a {listing}-transformer listing ended in `{code}`, not at the native credential"
    );
}

#[test]
fn a_governed_group_batch_cannot_be_applied_without_the_plan_it_was_previewed_as() {
    // The digest is the fence. `ds` carries the one the server returned and
    // never mints one, so a caller that skips the preview has no digest to
    // send and dispatch refuses on the missing input rather than committing a
    // batch nobody approved.
    assert_eq!(
        refusal(&[
            "design",
            "group",
            "apply",
            "--project",
            "test-project",
            "--group",
            "city",
            "--transformers",
            "kigali_a",
            "--value",
            "kigali",
            "--output",
            "json",
            "--yes",
        ]),
        "missing_input",
        "apply without --digest must be refused before the bridge opens"
    );
    // Clearing is its own command, so forgetting --value can never silently
    // become an unassign.
    assert_eq!(
        refusal(&[
            "design",
            "group",
            "apply",
            "--project",
            "test-project",
            "--group",
            "city",
            "--transformers",
            "kigali_a",
            "--digest",
            "abc123",
            "--output",
            "json",
            "--yes",
        ]),
        "missing_input",
        "apply with no --value must refuse rather than clear the group"
    );
    // Both refusals come from the parser because both flags are DECLARED
    // required on apply. `--value` is deliberately not shared with `preview`,
    // where omitting it is the meaningful way to plan an unassign.
}

#[test]
fn every_design_write_refuses_without_confirmation() {
    // Every write in this domain mutates governed project state, so dispatch
    // must stop it before the bridge opens.
    for args in [
        vec![
            "design",
            "selection",
            "save",
            "--project",
            "test-project",
            "--name",
            "Week 32",
            "--transformers",
            "kigali_a",
        ],
        vec![
            "design",
            "selection",
            "archive",
            "--project",
            "test-project",
            "--selection",
            "sel-1",
        ],
        vec![
            "design",
            "selection",
            "assign",
            "--project",
            "test-project",
            "--selection",
            "sel-1",
            "--title",
            "Review",
        ],
        vec![
            "design",
            "attachment",
            "publish",
            "--kind",
            "lv_transformer",
            "--object",
            "kigali_a",
            "--path",
            "/tmp/a.bak",
        ],
        vec!["design", "attachment", "retire", "--attachment", "att-1"],
        vec![
            "design",
            "attachment",
            "set-latest",
            "--attachment",
            "att-1",
            "--revision",
            "rev-1",
        ],
        vec![
            "design",
            "comment",
            "redact",
            "--project",
            "test-project",
            "--thread",
            "thread-1",
            "--comment",
            "c-1",
            "--expected-version",
            "2",
            "--reason",
            "personal data",
        ],
        vec![
            "design",
            "tag",
            "define",
            "--project",
            "test-project",
            "--definition",
            "scope",
            "--name",
            "Scope",
            "--values",
            "a,b",
        ],
        vec![
            "design",
            "tag",
            "set",
            "--project",
            "test-project",
            "--kind",
            "lv_transformer",
            "--object",
            "kigali_a",
            "--definition",
            "scope",
            "--values",
            "a",
        ],
        vec![
            "design",
            "comment",
            "post",
            "--project",
            "test-project",
            "--thread",
            "thread-1",
            "--body",
            "Agreed.",
        ],
        vec![
            "design",
            "comment",
            "resolve",
            "--project",
            "test-project",
            "--thread",
            "thread-1",
        ],
        vec![
            "design",
            "comment",
            "promote",
            "--project",
            "test-project",
            "--thread",
            "thread-1",
        ],
        vec![
            "design",
            "group",
            "apply",
            "--project",
            "test-project",
            "--group",
            "city",
            "--transformers",
            "kigali_a",
            "--value",
            "kigali",
            "--digest",
            "abc123",
        ],
        vec![
            "design",
            "group",
            "unassign",
            "--project",
            "test-project",
            "--group",
            "phasing",
            "--transformers",
            "kigali_a",
            "--digest",
            "abc123",
        ],
    ] {
        let mut argv = args.clone();
        if args.get(1) == Some(&"attachment") {
            argv.extend(["--project", "project_a"]);
        }
        argv.extend(["--output", "json"]);
        assert_eq!(
            refusal(&argv),
            "confirmation_required",
            "`ds {}` reached past the confirmation gate",
            args.join(" ")
        );
    }
}

#[test]
fn autoprocess_plan_answers_the_three_admissions_from_one_document() {
    let root = temp_root("autoprocess-plan");
    std::fs::create_dir_all(&root).unwrap();
    let changes = root.join("edits.json");
    std::fs::write(
        &changes,
        r#"{
          "trigger": {
            "reason": "attribute-cell-edit",
            "changed_fields": ["plan_kva"],
            "vocabulary": {"lockable_cells": [], "status_fields": []}
          },
          "differential_scope": {
            "gdfs": {"lv_lines":{"type":"FeatureCollection","features":[{"id":"f1","properties":{"path_id":"path1"}}]},"lv_poles":{"type":"FeatureCollection","features":[{"id":"p1","properties":{"path_id":"path1"}}]}},
            "differential": {"selected_feeders": [], "auto_process": {
                "differential_enabled": true, "is_mv_session": false,
                "accumulator_bound": true, "force_full": false,
                "changed_features": [{"layer_name":"lv_poles","feature_id":"p1"}]
            }},
            "customer_source_addresses": []
          },
          "cadence": {
            "enabled": true, "running": false, "queued": true, "force": false,
            "pending_edit_count": 3, "window_started_at_ms": 1000,
            "cadence": {"min_pending_edits": 5, "max_pending_seconds": 1},
            "now_ms": 1000
          }
        }"#,
    )
    .unwrap();
    let path = changes.display().to_string();
    let data = ok(&[
        "design",
        "autoprocess",
        "plan",
        "--changes",
        &path,
        "--now-ms",
        "1100",
        "--output",
        "json",
    ]);
    // The sizing pin is inside the rule, not a transcribed list.
    assert_eq!(data["trigger"]["schedule"], true);
    assert_eq!(data["differential_scope"]["scope"], "feeders");
    assert_eq!(data["differential_scope"]["feeders"], json!(["f1"]));
    // `now_ms` is an input, not a clock the kernel reads: --now-ms 1100 is
    // 100 ms into the one-second window, so 900 ms of it remain. The document
    // alone would have answered 1000.
    assert_eq!(data["cadence"]["decision"], "wait");
    assert_eq!(data["cadence"]["wait_ms"], 900);
    assert_eq!(
        data["cadence"]["reason_key"],
        "autoprocess_window_remaining"
    );
    assert_eq!(data["cadence"]["waiting_not_executing"], true);

    let descriptor = ds(&[
        "capabilities",
        "design.autoprocess.plan",
        "--output",
        "json",
    ])
    .envelope["data"]["command"]
        .clone();
    assert_eq!(descriptor["effect"], "read_only");
    assert_eq!(descriptor["authority"], "none");

    // A cadence document that declares no `now_ms` is answered from this
    // machine's clock, which is what --now-ms says it defaults to. It used to
    // refuse with a missing-field error instead.
    let unpinned = root.join("unpinned.json");
    std::fs::write(
        &unpinned,
        r#"{"cadence": {"enabled": true, "running": false, "queued": true, "force": true,
            "pending_edit_count": 1, "window_started_at_ms": 0,
            "cadence": {"min_pending_edits": 5, "max_pending_seconds": 60}}}"#,
    )
    .unwrap();
    let clocked = ok(&[
        "design",
        "autoprocess",
        "plan",
        "--changes",
        &unpinned.display().to_string(),
        "--output",
        "json",
    ]);
    assert_eq!(clocked["cadence"]["decision"], "wait");
    assert_eq!(
        clocked["cadence"]["reason_key"],
        "autoprocess_forced_settle"
    );

    // A section the kernel does not answer is named, not silently dropped.
    let stray = root.join("stray.json");
    std::fs::write(&stray, r#"{"cadence_wait": {}}"#).unwrap();
    assert_eq!(
        refusal(&[
            "design",
            "autoprocess",
            "plan",
            "--changes",
            &stray.display().to_string(),
            "--output",
            "json",
        ]),
        "autoprocess_request_invalid"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn force_gate_check_mints_a_grant_bound_to_its_gesture() {
    let unforced = ok(&[
        "design",
        "force-gate",
        "check",
        "--action",
        "retry_process",
        "--output",
        "json",
    ]);
    assert_eq!(unforced["policy"]["requires_grant"], false);
    assert_eq!(unforced["policy"]["reason_key"], "force_not_requested");

    let forced = ok(&[
        "design",
        "force-gate",
        "check",
        "--action",
        "retry_process",
        "--force",
        "--targets",
        "28",
        "--output",
        "json",
    ]);
    assert_eq!(forced["policy"]["requires_grant"], true);
    assert_eq!(forced["policy"]["target_count"], 28);
    // The page's prompt is not reachable from here, and neither is the code:
    // a wrong answer mints nothing.
    let refused = ok(&[
        "design",
        "force-gate",
        "check",
        "--action",
        "retry_process",
        "--force",
        "--gesture",
        "g1",
        "--confirm",
        "not-the-code",
        "--now-ms",
        "1000",
        "--output",
        "json",
    ]);
    assert_eq!(refused["grant"]["granted"], false);
    assert!(refused["grant"]["token"].is_null());

    let descriptor = ds(&[
        "capabilities",
        "design.force-gate.check",
        "--output",
        "json",
    ])
    .envelope["data"]["command"]
        .clone();
    assert_eq!(descriptor["effect"], "read_only");
    assert_eq!(descriptor["authority"], "none");

    // A confirmation with no gesture has nothing to bind to.
    assert_eq!(
        refusal(&[
            "design",
            "force-gate",
            "check",
            "--action",
            "retry_process",
            "--force",
            "--confirm",
            "xxx",
            "--output",
            "json",
        ]),
        "force_gate_input_invalid"
    );
}

#[test]
fn design_data_lane_is_a_native_project_read() {
    let descriptor = ds(&["capabilities", "design.data.lane", "--output", "json"]).envelope["data"]
        ["command"]
        .clone();
    assert_eq!(descriptor["effect"], "read_only");
    assert_eq!(descriptor["authority"], "headless_project");
    // No desktop, no bridge: it refuses on the native session, never on pairing.
    let code = refusal(&["design", "data", "lane", "--output", "json"]);
    assert!(
        !PAIRING_CODES.contains(&code.as_str()) && !code.is_empty(),
        "`ds design data lane` ended in `{code}`, which is a bridge state"
    );
}

#[test]
fn saved_selections_are_native_and_ds_brain_still_decides_membership() {
    // The decision did not move. ds-brain evaluates a saved selection's
    // membership and keeps it; what Slice 14b changed is residency — the five
    // commands no longer need a paired browser to ask, because the credential
    // now lives here. So: `headless_project`, never a pairing state.
    for (id, effect) in [
        ("design.selection.list", "read_only"),
        ("design.selection.read", "read_only"),
        ("design.selection.save", "global_write"),
        ("design.selection.archive", "global_write"),
        ("design.selection.assign", "global_write"),
    ] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        assert_eq!(descriptor["command"]["effect"], effect, "`{id}` effect");
        assert_eq!(
            descriptor["command"]["authority"], "headless_project",
            "`{id}` reads this machine's own selected project"
        );
    }
    for args in [
        vec![
            "design",
            "selection",
            "list",
            "--project",
            "test-project",
            "--output",
            "json",
        ],
        vec![
            "design",
            "selection",
            "read",
            "--project",
            "test-project",
            "--selection",
            "sel-smoke",
            "--output",
            "json",
        ],
    ] {
        let code = native_refusal(&args);
        assert!(
            !PAIRING_CODES.contains(&code.as_str()) && !code.is_empty(),
            "`{}` ended in `{code}`, which is a bridge state",
            args.join(" ")
        );
    }
    // A write still needs the confirmation every governed write needs, and the
    // refusal arrives before a credential is spent.
    assert_eq!(
        native_refusal(&[
            "design",
            "selection",
            "archive",
            "--project",
            "test-project",
            "--selection",
            "sel-smoke",
            "--output",
            "json",
        ]),
        "confirmation_required",
        "an unconfirmed archive must refuse before it reaches the project"
    );
    assert_eq!(
        native_refusal(&[
            "design",
            "selection",
            "save",
            "--project",
            "test-project",
            "--name",
            "smoke",
            "--transformers",
            ",,",
            "--yes",
            "--output",
            "json"
        ]),
        "invalid_value_list",
        "empty member lists must be refused before the project is reached",
    );
}

#[test]
fn a_network_document_copy_names_another_project_before_anything_is_sent() {
    // A copy from the destination itself, or of a part that does not exist, is
    // refused locally: nothing reaches ds-brain, and the refusal is documented.
    for args in [
        &[
            "design",
            "config",
            "copy",
            "plan",
            "--project",
            "test-project",
            "--source-project",
            "test-project",
        ][..],
        &[
            "design",
            "config",
            "copy",
            "plan",
            "--project",
            "test-project",
            "--source-project",
            "template_tchad",
            "--part",
            "layers",
        ][..],
    ] {
        let mut call = args.to_vec();
        call.extend(["--output", "json"]);
        assert_eq!(
            native_refusal(&call),
            "network_documents_invalid",
            "{args:?}"
        );
    }
}

#[test]
fn every_design_command_is_discoverable_without_the_desktop_installed() {
    // Every native command is discoverable and honestly unavailable when this
    // test build has no digest-pinned release catalog; the kernel-local ones
    // are available. Nothing in this domain requires a window.
    let index = ok(&["capabilities", "design", "--output", "json"]);
    let commands = index["commands"].as_array().expect("commands");
    assert_eq!(
        commands.len(),
        // Base 101, minus four retired commands, plus three version and five
        // attachment/comment commands added on 2026-09-25, plus the LV
        // voltage-drop check (2026-09-28), one-shot project run (2026-09-30),
        // the governed tag project-list read, and LV analysis availability,
        // plus the two network document copy commands (2026-10-06).
        // Seven native repair controls, source resolution and verified backup/restore.
        // 2026-10-12/13: version catalogue, import (prepare and native),
        // renumber and five revision controls replaced the ordinal restore;
        // 2026-10-18: the three saved MV model commands; 2026-10-19: local
        // collision detection and publication, and the retained MV model
        // catalogue. 120 + 12 - 1 + 3.
        134,
        "the design domain should expose its whole family: {commands:?}"
    );
    for command in commands {
        let id = command["id"].as_str().unwrap_or("?");
        assert_eq!(
            command["availability"],
            if matches!(
                id,
                "design.intake.upload"
                    | "design.features.select"
                    | "design.tag.project-list"
                    | "design.group.project-preview"
                    | "design.group.project-apply"
                    | "design.group.project-export"
                    | "design.config.sheets"
                    | "design.config.read"
                    | "design.config.diff"
                    | "design.config.set"
                    | "design.config.save"
                    | "design.config.rule-set.duplicate"
                    | "design.config.copy.plan"
                    | "design.config.copy.apply"
                    | "design.feeder-limits.read"
                    | "design.feeder-limits.set"
                    | "design.categories.read"
                    | "design.meter-types.ensure"
                    | "design.meter-types.default"
                    | "design.customer-categories.alias"
                    | "design.customer-categories.unbind"
                    | "design.customer-categories.rename"
                    | "design.customer-categories.retire"
                    | "design.customer-categories.retire-unnamed"
                    | "design.lv.project-export"
                    | "design.lv.project-save"
                    | "design.lv.project-run"
                    | "design.status"
                    | "design.collisions"
                    // Local detection reads the same native spine and its
                    // publication is a governed native write.
                    | "design.collisions.detect"
                    | "design.collisions.publish"
                    // Project-to-project migration is a bulk service call on
                    // the same native spine: honestly unavailable in a build
                    // with no digest-pinned release catalog.
                    | "design.migrate.plan"
                    | "design.migrate.apply"
                    // Pinned context reads the same native status spine.
                    | "design.pinned.preview"
                    | "design.dashboard"
                    // Design Activities reads the same native spine, once per
                    // project, so it is honestly unavailable in a build with
                    // no digest-pinned release catalog.
                    | "design.activities.sweep"
                    | "design.activities.read"
                    | "design.transformer.inventory"
                    | "design.transformer.retire"
                    | "design.transformer.restore"
                    // The slice-14a previews read the same native spine as
                    // `design.status`, so they are honestly unavailable in a
                    // build with no digest-pinned release catalog.
                    | "design.bulk.plan"
                    | "design.download.plan"
                    | "design.download.fetch"
                    | "design.version.status"
                    // Published-version listing and comparison went headless
                    // on 2026-09-14 (no paired desktop): same native spine.
                    | "design.version.list"
                    | "design.version.compare"
                    | "design.version.begin"
                    | "design.version.show"
                    | "design.version.begin-batch"
                    | "design.version.summaries"
                    | "design.version.catalog"
                    | "design.version.import"
                    | "design.version.renumber"
                    | "design.version.revision.show"
                    | "design.version.revision.bind"
                    | "design.version.revision.publish"
                    | "design.version.revision.promote"
                    | "design.model.list"
                    | "design.model.export"
                    | "design.model.publish"
                    | "design.model.catalog"
                    | "design.attachment.list"
                    | "design.attachment.list-project"
                    | "design.attachment.show"
                    | "design.attachment.versions"
                    | "design.attachment.publish"
                    | "design.attachment.download"
                    | "design.attachment.set-latest"
                    | "design.attachment.retire"
                    | "design.conflict.list"
                    | "design.conflict.check"
                    | "design.presence.status"
                    | "design.data.lane"
                    // Saved selections are native from Slice 14b: the decision
                    // stayed with ds-brain, the residency moved here.
                    | "design.selection.list"
                    | "design.selection.read"
                    | "design.selection.save"
                    | "design.selection.archive"
                    | "design.selection.assign"
                    // Tags, groups, consumer grouping, comments, known
                    // columns and materials are native from 2026-09-20 (the
                    // kernel's design doors under the native credential).
                    | "design.tag.list"
                    | "design.tag.query"
                    | "design.tag.define"
                    | "design.tag.set"
                    | "design.tag.enrich-preview"
                    | "design.tag.enrich-apply"
                    | "design.group.list"
                    | "design.group.preview"
                    | "design.group.apply"
                    | "design.group.unassign"
                    | "design.group.export"
                    | "design.consumer-grouping.preview"
                    | "design.consumer-grouping.apply"
                    | "design.consumer-grouping.read"
                    | "design.consumer-grouping.archive"
                    | "design.comment.list"
                    | "design.comment.read"
                    | "design.comment.post"
                    | "design.comment.resolve"
                    | "design.comment.promote"
                    | "design.comment.redact"
                    | "design.known-columns.list"
                    | "design.known-columns.set"
                    | "design.materials.preview"
                    | "design.materials.apply"
                    | "design.lv.analysis-read"
                    | "design.repair.publish"
                    | "design.transformer.backup"
                    | "design.transformer.restore-deleted"
            ) {
                "unavailable"
            } else if COLLABORATION_WINDOW_BACKLOG.contains(&id) {
                // Window-bound, and since 2026-09-19 honest about it.
                "requires_window"
            } else {
                "available"
            },
            "`{id}` has the wrong packaging or Desktop availability"
        );
    }
    // A well-formed collaboration read gets as far as the native credential
    // and no further — never a window.
    assert_eq!(
        native_refusal(&[
            "design",
            "tag",
            "list",
            "--project",
            "test-project",
            "--kind",
            "lv_transformer",
            "--object",
            "T-smoke",
            "--output",
            "json",
        ]),
        "headless_signed_out",
        "a well-formed design read must end at the native credential"
    );
}

/// Versioned deliverables have a verb for every attachment action ds-brain
/// serves, and each new verb refuses locally what it can before a credential
/// is consulted: an existing `--out` file is never replaced, a digest pin is
/// MV-only, a page and a fence are bounded, and a redaction needs the version
/// the moderator read. Well-formed calls end at the native credential.
#[test]
fn attachment_versioning_verbs_refuse_locally_and_never_replace_a_file() {
    // The kernel's policy offers `make_latest`; the verb behind it exists and
    // is a governed write.
    let set_latest = ok(&[
        "capabilities",
        "design.attachment.set-latest",
        "--output",
        "json",
    ]);
    assert_eq!(set_latest["command"]["effect"], "global_write");

    let root = temp_root("attachment-download");
    std::fs::create_dir_all(&root).unwrap();
    let existing = root.join("LINE_A_v2.bak");
    std::fs::write(&existing, b"shipped").unwrap();
    let download = |out: &std::path::Path| {
        native_ds(&[
            "design",
            "attachment",
            "download",
            "--project",
            "test-project",
            "--attachment",
            "att-line-a-bak",
            "--revision",
            "rev_2",
            "--out",
            out.to_str().unwrap(),
            "--output",
            "json",
        ])
    };
    let refused = download(&existing);
    assert_eq!(
        refused.envelope["error"]["code"], "attachment_output_invalid",
        "an existing destination is refused before any byte is authorized: {}",
        refused.stdout
    );
    assert_eq!(std::fs::read(&existing).unwrap(), b"shipped");
    let fresh = root.join("fresh.bak");
    let signed_out = download(&fresh);
    assert_eq!(
        signed_out.envelope["error"]["code"],
        "design_attachment_refused"
    );
    assert_eq!(
        signed_out.envelope["error"]["detail"]["cause"], "headless_signed_out",
        "a well-formed fetch ends at the native credential: {}",
        signed_out.stdout
    );
    assert!(!fresh.exists(), "nothing is written without verified bytes");

    // A version digest pins an MV content revision only; the owner refuses
    // the transformer pin before any upload or credential.
    let bak = root.join("sheet.pdf");
    std::fs::write(&bak, b"bytes").unwrap();
    let digest = "a".repeat(64);
    let pinned = native_ds(&[
        "design",
        "attachment",
        "publish",
        "--project",
        "test-project",
        "--kind",
        "lv_transformer",
        "--object",
        "kigali_a",
        "--version",
        "v2",
        "--version-digest",
        &digest,
        "--path",
        bak.to_str().unwrap(),
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(
        pinned.envelope["error"]["code"],
        "design_attachment_refused"
    );
    assert_eq!(
        pinned.envelope["error"]["detail"]["cause"],
        "auth_input_invalid"
    );
    // The same flags on an MV pin reach the credential.
    let mv = native_ds(&[
        "design",
        "attachment",
        "publish",
        "--project",
        "test-project",
        "--kind",
        "mv_model",
        "--object",
        "line_a",
        "--version",
        "rev-2",
        "--version-digest",
        &digest,
        "--media-type",
        "application/octet-stream",
        "--source-kind",
        "pls_cadd",
        "--source-ref",
        "submission 2",
        "--no-latest",
        "--path",
        bak.to_str().unwrap(),
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(
        mv.envelope["error"]["detail"]["cause"],
        "headless_signed_out"
    );
    let _ = std::fs::remove_dir_all(&root);

    // Bounds a caller can read from help are enforced before a round trip.
    for (args, code) in [
        (
            vec![
                "design",
                "attachment",
                "list-project",
                "--project",
                "test-project",
                "--limit",
                "501",
            ],
            "invalid_number",
        ),
        (
            vec![
                "design",
                "attachment",
                "set-latest",
                "--project",
                "test-project",
                "--attachment",
                "att-1",
                "--revision",
                "rev-1",
                "--expected-version",
                "0",
                "--yes",
            ],
            "invalid_number",
        ),
        (
            vec![
                "design",
                "comment",
                "redact",
                "--project",
                "test-project",
                "--thread",
                "thread-1",
                "--comment",
                "c-1",
                "--expected-version",
                "0",
                "--reason",
                "personal data",
                "--yes",
            ],
            "invalid_number",
        ),
    ] {
        let mut argv = args.clone();
        argv.extend(["--output", "json"]);
        assert_eq!(native_refusal(&argv), code, "`ds {}`", args.join(" "));
    }
    // A redaction without its reason is not a redaction.
    assert_ne!(
        native_refusal(&[
            "design",
            "comment",
            "redact",
            "--project",
            "test-project",
            "--thread",
            "thread-1",
            "--comment",
            "c-1",
            "--expected-version",
            "2",
            "--yes",
            "--output",
            "json",
        ]),
        "headless_signed_out"
    );
    // Well-formed reads end at the native credential, never a window.
    for args in [
        vec![
            "design",
            "attachment",
            "list-project",
            "--project",
            "test-project",
        ],
        vec![
            "design",
            "attachment",
            "show",
            "--project",
            "test-project",
            "--attachment",
            "att-1",
        ],
        vec![
            "design",
            "attachment",
            "versions",
            "--project",
            "test-project",
            "--attachment",
            "att-1",
        ],
    ] {
        let mut argv = args.clone();
        argv.extend(["--output", "json"]);
        let run = native_ds(&argv);
        assert_eq!(
            run.envelope["error"]["detail"]["cause"],
            "headless_signed_out",
            "`ds {}`: {}",
            args.join(" "),
            run.stdout
        );
    }
}

/// `design.transformer.download` and `design.sync.*` controlled a window's
/// private browser stores (its room cache, its reconciliation queue). Retired
/// on 2026-09-20 rather than given a second store: the native reporter reads
/// rooms from the service, and the kernel sync store is the only queue.
#[test]
fn window_cache_controls_are_retired_not_relocated() {
    for id in [
        "design.transformer.download",
        "design.sync.status",
        "design.sync.cancel",
        "design.sync.resume",
    ] {
        let data = ok(&["capabilities", "design", "--output", "json"]);
        assert!(
            !data["commands"]
                .as_array()
                .expect("commands")
                .iter()
                .any(|row| row["id"] == id),
            "`{id}` is retired and must not be registered"
        );
    }
    assert_eq!(
        refusal(&["design", "sync", "status", "--output", "json"]),
        "unknown_command"
    );
    assert_eq!(
        refusal(&["design", "transformer", "download", "--output", "json"]),
        "unknown_command"
    );
}

#[test]
fn design_reads_are_reads_and_design_writes_are_governed_writes() {
    // The effect classification is what a caller plans around, and it is the
    // one thing a reviewer cannot infer from a command's name: `retire` sounds
    // destructive and is a soft, reversible governed write, while `download`
    // sounds like it moves bytes and changes nothing shared.
    // Saved selections left this loop in Slice 14b: they are `headless_project`
    // now, asserted in `saved_selections_are_native_and_ds_brain_still_decides`.
    for (id, effect) in [
        ("design.attachment.list", "read_only"),
        ("design.attachment.list-project", "read_only"),
        ("design.attachment.show", "read_only"),
        ("design.attachment.versions", "read_only"),
        // `--out` writes the verified bytes to a new local file; the signed
        // URL alone stays the no-`--out` answer. Nothing shared changes.
        ("design.attachment.download", "local_file_write"),
        ("design.tag.list", "read_only"),
        ("design.tag.query", "read_only"),
        ("design.known-columns.list", "read_only"),
        ("design.comment.list", "read_only"),
        ("design.comment.read", "read_only"),
        ("design.attachment.publish", "global_write"),
        ("design.attachment.set-latest", "global_write"),
        ("design.attachment.retire", "global_write"),
        ("design.tag.define", "global_write"),
        ("design.tag.set", "global_write"),
        ("design.known-columns.set", "global_write"),
        ("design.comment.post", "global_write"),
        ("design.comment.promote", "global_write"),
        // Redaction clears text the server does not keep: a governed write.
        ("design.comment.redact", "global_write"),
        // A governed group's preview and its report projection are reads: they
        // decide and describe, and neither writes a byte. That is what keeps
        // both usable on a project that accepts no changes.
        ("design.group.list", "read_only"),
        ("design.group.preview", "read_only"),
        ("design.group.export", "read_only"),
        ("design.group.apply", "global_write"),
        ("design.group.unassign", "global_write"),
        ("design.consumer-grouping.preview", "read_only"),
        ("design.consumer-grouping.apply", "global_write"),
    ] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        assert_eq!(
            descriptor["command"]["effect"], effect,
            "`{id}` declares the wrong blast radius"
        );
        assert_eq!(
            descriptor["command"]["authority"], "headless_project",
            "`{id}` must require the restored native user's selected project"
        );
    }
    let descriptor = ok(&["capabilities", "design.features.select", "--output", "json"]);
    assert_eq!(descriptor["command"]["effect"], "local_auth_state");
    assert_eq!(descriptor["command"]["authority"], "headless_project");
    let descriptor = ok(&[
        "capabilities",
        "design.lv.project-export",
        "--output",
        "json",
    ]);
    assert_eq!(descriptor["command"]["effect"], "local_file_write");
    assert_eq!(descriptor["command"]["authority"], "headless_project");
}

// ---------------------------------------------------------------------------
// work
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// design collaboration
// ---------------------------------------------------------------------------

#[test]
fn design_collaboration_is_a_complete_headless_project_surface() {
    // Saved Transformer Status selections, versioned attachments, tags and
    // comment threads are project records, not map-owned state. All commands
    // must therefore be discoverable without an open map; reads reach only
    // the native credential and writes stop at the global confirmation gate.
    let index = ok(&["capabilities", "design", "--output", "json"]);
    let commands = index["commands"].as_array().expect("commands");
    let actual: BTreeSet<&str> = commands
        .iter()
        .map(|command| command["id"].as_str().expect("id"))
        .filter(|id| {
            !id.starts_with("design.lv.")
                && !id.starts_with("design.transformer.")
                && !id.starts_with("design.project.")
                && !id.starts_with("design.config.")
                && !id.starts_with("design.data.")
                && !id.starts_with("design.feeder-limits.")
                && !id.starts_with("design.categories.")
                && !id.starts_with("design.meter-types.")
                && !id.starts_with("design.customer-categories.")
                // The native read spine is not bridge collaboration; its own
                // test pins its availability and its refusals. The Dashboard
                // is the same spine, folded once.
                && *id != "design.status"
                // Process settings resolve in the kernel over the engine's
                // catalogue; no project, bridge or desktop state is read.
                && *id != "design.process.settings"
                // AutoProcess admission and the force gate are pure kernel
                // policy over inputs the caller supplies: no project, bridge
                // or desktop state is read.
                && *id != "design.autoprocess.plan"
                && *id != "design.force-gate.check"
                // The Dashboard is the same headless read folded once.
                && *id != "design.dashboard"
                // The collision read answers from the same headless status
                // call; it is not a governed record operation. Local detection
                // and its publication are an engine run plus one storage write,
                // pinned by their own tests, never bridge collaboration.
                && *id != "design.collisions"
                && !id.starts_with("design.collisions.")
                // Pinned context is READ-ONLY CONTEXT: it plans a fetch, folds
                // rooms and projects their properties. It records nothing, so
                // it is not a governed record operation either.
                && *id != "design.pinned.preview"
                // The slice-14a previews are the same spine folded by the
                // shared kernel — a preview of a batch, a download, a version
                // state, an overwrite gate or a lease pass. None reaches the
                // bridge; `every_design_command_is_discoverable_*` pins their
                // availability and `semantic_coverage` their authority.
                && !id.starts_with("design.bulk.")
                && !id.starts_with("design.download.")
                && !id.starts_with("design.version.")
                && !id.starts_with("design.conflict.")
                && !id.starts_with("design.presence.")
                // Saved selections are native from Slice 14b; ds-brain still
                // decides membership, but `ds` no longer needs a paired
                // browser to ask it.
                && !id.starts_with("design.selection.")
                // Design intake is a headless Rust upload/process workflow,
                // not a paired collaboration record operation.
                && !id.starts_with("design.intake.")
                // Project-to-project design migration is a bulk service call
                // against two projects, not a governed record operation on
                // one. It holds no collaboration record and never reaches the
                // bridge; `design_migrate_carries_both_design_kinds_on_one_
                // verb_and_refuses_locally` pins its contract.
                && !id.starts_with("design.migrate.")
                // Design Activities is a headless account-wide capture and an
                // offline fold over what it retained. It holds no governed
                // record and never reaches the bridge.
                && !id.starts_with("design.activities.")
                // Native repair is a file workflow; only publish is project-owned.
                && !id.starts_with("design.repair.")
                // Saved MV model reads, packages and the retained catalogue are
                // native grid-owner operations, not bridge collaboration records.
                && !id.starts_with("design.model.")
        })
        .collect();
    let expected: BTreeSet<&str> = [
        "design.features.select",
        "design.tag.project-list",
        "design.group.project-preview",
        "design.group.project-apply",
        "design.group.project-export",
        "design.attachment.list",
        "design.attachment.list-project",
        "design.attachment.show",
        "design.attachment.versions",
        "design.attachment.publish",
        "design.attachment.download",
        "design.attachment.set-latest",
        "design.attachment.retire",
        "design.tag.list",
        "design.tag.query",
        "design.tag.define",
        "design.tag.set",
        "design.materials.preview",
        "design.materials.apply",
        "design.known-columns.list",
        "design.known-columns.set",
        "design.group.list",
        "design.group.preview",
        "design.group.apply",
        "design.group.unassign",
        "design.group.export",
        "design.consumer-grouping.preview",
        "design.consumer-grouping.apply",
        "design.consumer-grouping.read",
        "design.consumer-grouping.archive",
        // Governed administrative location: the preview plans, the apply
        // commits exactly that plan.
        "design.tag.enrich-preview",
        "design.tag.enrich-apply",
        "design.comment.list",
        "design.comment.read",
        "design.comment.post",
        "design.comment.resolve",
        "design.comment.promote",
        "design.comment.redact",
    ]
    .into_iter()
    .collect();
    assert_eq!(
        actual, expected,
        "design collaboration must expose every governed record operation"
    );

    let writes: BTreeSet<&str> = [
        "design.materials.apply",
        "design.attachment.publish",
        "design.attachment.retire",
        "design.tag.define",
        "design.tag.set",
        "design.known-columns.set",
        "design.comment.post",
        "design.comment.resolve",
        "design.comment.promote",
        "design.comment.redact",
        // The governed group's two committing actions. `list`, `preview` and
        // `export` are reads: they decide and describe, and neither writes a
        // byte, which is what keeps all three usable on a project that accepts
        // no changes.
        "design.group.apply",
        "design.group.unassign",
        "design.consumer-grouping.apply",
        // Archiving a plan retires it; reading one does not.
        "design.consumer-grouping.archive",
        // The enrichment preview is a read for the same reason every other
        // preview here is: it holds no write and works on a project that
        // accepts no changes.
        "design.tag.enrich-apply",
    ]
    .into_iter()
    .collect();
    for command in commands {
        let id = command["id"].as_str().expect("id");
        // Offline LV compute and the headless transformer lifecycle are not
        // bridge collaboration; their own tests pin their availability.
        if id.starts_with("design.lv.")
            || id.starts_with("design.transformer.")
            || id.starts_with("design.project.")
            || id.starts_with("design.config.")
            || id.starts_with("design.data.")
            || id.starts_with("design.feeder-limits.")
            || id.starts_with("design.categories.")
            || id.starts_with("design.meter-types.")
            || id.starts_with("design.customer-categories.")
            || id == "design.status"
            || id == "design.dashboard"
            || id == "design.collisions"
            || id.starts_with("design.collisions.")
            || id == "design.pinned.preview"
            || id == "design.process.settings"
            || id == "design.autoprocess.plan"
            || id == "design.force-gate.check"
            || id.starts_with("design.bulk.")
            || id.starts_with("design.download.")
            || id.starts_with("design.version.")
            || id.starts_with("design.conflict.")
            || id.starts_with("design.presence.")
            || id.starts_with("design.selection.")
            || id.starts_with("design.intake.")
            || id.starts_with("design.attachment.")
            || id.starts_with("design.activities.")
            || id.starts_with("design.repair.")
            // Saved MV model reads/exports/publication feed printing, not
            // collaboration records; the discoverability test pins them.
            || id.starts_with("design.model.")
            // Project-to-project migration is a bulk service call against two
            // projects, pinned by its own test.
            || id.starts_with("design.migrate.")
        {
            continue;
        }
        if matches!(
            id,
            "design.features.select"
                | "design.tag.project-list"
                | "design.group.project-preview"
                | "design.group.project-apply"
                | "design.group.project-export"
        ) {
            assert_eq!(command["availability"], "unavailable");
            assert_eq!(
                command["effect"],
                if id == "design.group.project-apply" {
                    "global_write"
                } else {
                    "local_auth_state"
                }
            );
            assert_eq!(command["authority"], "headless_project");
            continue;
        }
        if COLLABORATION_WINDOW_BACKLOG.contains(&id) {
            assert_eq!(
                command["availability"], "requires_window",
                "`{id}` left the window backlog; delete it from COLLABORATION_WINDOW_BACKLOG"
            );
        } else {
            // Native availability: honestly unavailable in a test build with
            // no digest-pinned release catalog, never `requires_window`.
            assert_eq!(
                command["availability"], "unavailable",
                "`{id}` must not require an open map; a new window-bound collaboration \
                 command widens COLLABORATION_WINDOW_BACKLOG deliberately or not at all"
            );
            assert_eq!(
                command["authority"], "headless_project",
                "`{id}` must run under the restored native user's selected project"
            );
        }
        assert_eq!(
            command["effect"],
            if writes.contains(id) {
                "global_write"
            } else {
                "read_only"
            }
        );
        if writes.contains(id) {
            let mut args = match id {
                "design.attachment.publish" => vec![
                    "design",
                    "attachment",
                    "publish",
                    "--kind",
                    "lv_transformer",
                    "--object",
                    "T-smoke",
                    "--path",
                    "smoke.bak",
                ],
                "design.attachment.retire" => vec![
                    "design",
                    "attachment",
                    "retire",
                    "--attachment",
                    "att-smoke",
                ],
                "design.tag.define" => vec![
                    "design",
                    "tag",
                    "define",
                    "--project",
                    "test-project",
                    "--definition",
                    "scope",
                    "--name",
                    "Scope",
                    "--values",
                    "smoke",
                ],
                "design.tag.set" => vec![
                    "design",
                    "tag",
                    "set",
                    "--project",
                    "test-project",
                    "--kind",
                    "lv_transformer",
                    "--object",
                    "T-smoke",
                    "--definition",
                    "scope",
                ],
                "design.materials.apply" => vec![
                    "design",
                    "materials",
                    "apply",
                    "--project",
                    "test-project",
                    "--template",
                    "master",
                    "--rule-set",
                    "rule_x",
                    "--row",
                    "pole-x",
                    "--digest",
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                ],
                "design.known-columns.set" => vec![
                    "design",
                    "known-columns",
                    "set",
                    "--project",
                    "test-project",
                    "--layer",
                    "lv_lines",
                    "--field",
                    "tag_delivery_area",
                    "--visibility",
                    "published",
                ],
                "design.comment.post" => vec![
                    "design",
                    "comment",
                    "post",
                    "--project",
                    "test-project",
                    "--kind",
                    "lv_transformer",
                    "--object",
                    "T-smoke",
                    "--body",
                    "Smoke",
                ],
                "design.comment.resolve" => {
                    vec![
                        "design",
                        "comment",
                        "resolve",
                        "--project",
                        "test-project",
                        "--thread",
                        "thread-smoke",
                    ]
                }
                "design.comment.promote" => {
                    vec![
                        "design",
                        "comment",
                        "promote",
                        "--project",
                        "test-project",
                        "--thread",
                        "thread-smoke",
                    ]
                }
                "design.comment.redact" => vec![
                    "design",
                    "comment",
                    "redact",
                    "--project",
                    "test-project",
                    "--thread",
                    "thread-smoke",
                    "--comment",
                    "c-smoke",
                    "--expected-version",
                    "3",
                    "--reason",
                    "smoke",
                ],
                "design.group.apply" => vec![
                    "design",
                    "group",
                    "apply",
                    "--project",
                    "test-project",
                    "--group",
                    "delivery_area",
                    "--transformers",
                    "T-smoke",
                    "--value",
                    "kigali",
                    "--digest",
                    "digest-smoke",
                ],
                "design.group.unassign" => vec![
                    "design",
                    "group",
                    "unassign",
                    "--project",
                    "test-project",
                    "--group",
                    "delivery_phase",
                    "--transformers",
                    "T-smoke",
                    "--digest",
                    "digest-smoke",
                ],
                // --definition-ids is required here, unlike the projection
                // flag this command used to borrow: a consumer grouping has
                // no untagged-only plan, so an omission never reaches
                // confirmation.
                "design.consumer-grouping.apply" => vec![
                    "design",
                    "consumer-grouping",
                    "apply",
                    "--project",
                    "test-project",
                    "--transformers",
                    "T-smoke",
                    "--definition-ids",
                    "city",
                    "--digest",
                    "digest-smoke",
                ],
                "design.consumer-grouping.archive" => {
                    vec![
                        "design",
                        "consumer-grouping",
                        "archive",
                        "--project",
                        "test-project",
                        "--purpose",
                        "report_archive",
                    ]
                }
                "design.tag.enrich-apply" => vec![
                    "design",
                    "tag",
                    "enrich-apply",
                    "--project",
                    "test-project",
                    "--transformers",
                    "T-smoke",
                    "--digest",
                    "digest-smoke",
                ],
                _ => unreachable!("write inventory and command surface diverged: {id}"),
            };
            args.extend(["--output", "json"]);
            assert_eq!(
                native_refusal(&args),
                "confirmation_required",
                "`{id}` reached past confirmation"
            );
        }
    }

    // Well-formed reads reach the native credential.
    for args in [
        vec![
            "design",
            "tag",
            "list",
            "--project",
            "test-project",
            "--kind",
            "lv_transformer",
            "--object",
            "T-smoke",
        ],
        vec![
            "design",
            "tag",
            "query",
            "--project",
            "test-project",
            "--choice",
            "city:equals:kigali",
        ],
        vec![
            "design",
            "comment",
            "list",
            "--project",
            "test-project",
            "--kind",
            "lv_transformer",
            "--object",
            "T-smoke",
        ],
        vec![
            "design",
            "comment",
            "read",
            "--project",
            "test-project",
            "--thread",
            "thread-smoke",
        ],
        vec![
            "design",
            "known-columns",
            "list",
            "--project",
            "test-project",
        ],
        vec![
            "design",
            "group",
            "list",
            "--project",
            "test-project",
            "--transformers",
            "T-smoke",
        ],
        vec![
            "design",
            "consumer-grouping",
            "read",
            "--project",
            "test-project",
        ],
    ] {
        let mut argv = args.clone();
        argv.extend(["--output", "json"]);
        assert_eq!(
            native_refusal(&argv),
            "headless_signed_out",
            "`ds {}` must end at the native credential, not before and not at a window",
            args.join(" ")
        );
    }

    assert_eq!(
        native_refusal(&[
            "design",
            "tag",
            "define",
            "--project",
            "test-project",
            "--definition",
            "score",
            "--name",
            "Score",
            "--value-type",
            "number",
            "--values",
            "one",
            "--yes",
            "--output",
            "json",
        ]),
        "invalid_tag_input",
        "a numeric definition must not disguise strings as a vocabulary",
    );
    assert_eq!(
        native_refusal(&[
            "design",
            "tag",
            "set",
            "--project",
            "test-project",
            "--kind",
            "lv_transformer",
            "--object",
            "T-smoke",
            "--definition",
            "score",
            "--values",
            "one",
            "--number",
            "1",
            "--yes",
            "--output",
            "json",
        ]),
        "invalid_tag_input",
        "an assignment must carry exactly one value representation",
    );
    assert_eq!(
        native_refusal(&[
            "design",
            "tag",
            "query",
            "--project",
            "test-project",
            "--number",
            "completion:gte:NaN",
            "--output",
            "json",
        ]),
        "invalid_number",
        "numeric query predicates must be finite before pairing",
    );
    let mut too_many_filters = vec!["design", "tag", "query", "--project", "test-project"];
    for _ in 0..=ds_cli_design::MAX_TAG_QUERY_FILTERS {
        too_many_filters.extend(["--presence", "city:exists"]);
    }
    too_many_filters.extend(["--output", "json"]);
    assert_eq!(
        native_refusal(&too_many_filters),
        "too_many_tag_filters",
        "a query must enforce the backend's predicate-read bound before pairing",
    );
    assert_eq!(
        native_refusal(&[
            "design",
            "tag",
            "query",
            "--project",
            "test-project",
            "--presence",
            "city:exists",
            "--limit",
            "2001",
            "--output",
            "json",
        ]),
        "invalid_number",
        "a result bound past the backend's complete-project limit must refuse locally",
    );
}

#[test]
fn pm_validates_its_own_inputs_before_any_round_trip() {
    // Every refusal below must be reachable on a machine with no credential,
    // because that is every CI machine — and because a caller that transposed
    // a date should hear which flag was wrong, not that nobody is signed in.
    // The development catalogue makes the native availability gate pass so
    // the command's own input validation is what answers.
    assert_eq!(
        native_pm_refusal(&[
            "pm", "task", "update", "--task", "T-1", "--output", "json", "--yes"
        ]),
        "nothing_to_update",
        "an update with no change must be refused before a project round trip"
    );
    assert_eq!(
        native_pm_refusal(&[
            "pm",
            "task",
            "create",
            "--title",
            "Stake the route",
            "--start",
            "01-09-2026",
            "--output",
            "json",
            "--yes",
        ]),
        "invalid_date",
        "a transposed day and month must be refused locally; the engine would accept it"
    );
    assert_eq!(
        native_pm_refusal(&[
            "pm", "task", "create", "--title", "Orphan", "--kind", "child", "--output", "json",
            "--yes",
        ]),
        "invalid_task_shape",
        "a child with no parent must be refused before any round trip"
    );
    assert_eq!(
        native_pm_refusal(&[
            "pm",
            "task",
            "update",
            "--task",
            "T-1",
            "--progress",
            "140",
            "--output",
            "json",
            "--yes",
        ]),
        "invalid_number",
        "progress is a percent and must be held to 0..100"
    );
    assert_eq!(
        native_pm_refusal(&[
            "pm", "task", "assign", "--task", "T-1", "--output", "json", "--yes"
        ]),
        "invalid_assignment",
        "an assign that names nobody has no intent to send"
    );
    assert_eq!(
        native_pm_refusal(&[
            "pm",
            "task",
            "assign",
            "--task",
            "T-1",
            "--owner",
            "a@example.com",
            "--request",
            "b@example.com",
            "--output",
            "json",
            "--yes",
        ]),
        "invalid_assignment",
        "asking and transferring are two different intents"
    );
    assert_eq!(
        native_pm_refusal(&[
            "pm",
            "task",
            "assign",
            "--task",
            "T-1",
            "--request",
            "not-an-address",
            "--output",
            "json",
            "--yes",
        ]),
        "invalid_email",
        "a person flag that is not an address must be refused locally"
    );
    assert_eq!(
        native_pm_refusal(&["pm", "task", "list", "--limit", "500", "--output", "json"]),
        "invalid_number",
        "a page larger than the kernel returns must be refused by the bound it names"
    );
    assert_eq!(
        native_pm_refusal(&[
            "pm",
            "task",
            "respond",
            "--task",
            "T-1",
            "--response",
            "maybe",
            "--output",
            "json",
            "--yes",
        ]),
        "invalid_choice",
        "the assignment loop has two answers, and `maybe` is not one of them"
    );
}

/// Deleted-transformer recovery and the plan executor are Server commands
/// that refuse their own inputs before any credential is restored, then need
/// the signed-in native user; restoring is a confirmed write.
#[test]
fn deleted_transformer_recovery_and_download_fetch_check_inputs_before_the_native_user() {
    for (id, effect) in [
        ("design.transformer.backup", "read_only"),
        ("design.transformer.restore-deleted", "global_write"),
        ("design.download.fetch", "local_file_write"),
    ] {
        let command = ok(&["capabilities", id, "--output", "json"])["command"].clone();
        assert_eq!(command["requires"], "server", "{id}");
        assert_eq!(command["authority"], "headless_project", "{id}");
        assert_eq!(command["effect"], effect, "{id}");
    }
    let root = temp_root("recovery");
    std::fs::create_dir_all(&root).expect("temp root");
    let taken = root.join("taken.dsgrid");
    std::fs::write(&taken, b"keep").expect("existing file");
    let object = "transformers/1788000000-p1-TX-1.dsgrid";
    let backup = |extra: &[&str]| {
        let mut argv = vec![
            "design",
            "transformer",
            "backup",
            "--project",
            "p1",
            "--transformer",
            "TX-1",
            "--object",
            object,
            "--output",
            "json",
        ];
        argv.extend_from_slice(extra);
        native_refusal(&argv)
    };
    assert_eq!(backup(&["--generation", "0"]), "backup_pin_invalid");
    assert_eq!(backup(&["--sha256", "ABC"]), "backup_pin_invalid");
    assert_eq!(
        backup(&["--out", taken.to_str().expect("utf-8")]),
        "output_exists"
    );
    assert_eq!(backup(&[]), "headless_signed_out");
    assert_eq!(std::fs::read(&taken).expect("kept"), b"keep");
    let digest = "a".repeat(64);
    let restore = |yes: bool, sha: &str| {
        let mut argv = vec![
            "design",
            "transformer",
            "restore-deleted",
            "--project",
            "p1",
            "--transformer",
            "TX-1",
            "--object",
            object,
            "--generation",
            "17",
            "--sha256",
            sha,
            "--output",
            "json",
        ];
        if yes {
            argv.push("--yes");
        }
        native_refusal(&argv)
    };
    assert_eq!(restore(false, &digest), "confirmation_required");
    assert_eq!(restore(true, "nothex"), "backup_pin_invalid");
    assert_eq!(restore(true, &digest), "headless_signed_out");
    let fetch = |dir: &std::path::Path| {
        native_refusal(&[
            "design",
            "download",
            "fetch",
            "--project",
            "p1",
            "--out-dir",
            dir.to_str().expect("utf-8"),
            "--output",
            "json",
        ])
    };
    assert_eq!(fetch(&root), "download_dir_invalid");
    let empty = root.join("empty");
    assert_eq!(fetch(&empty), "headless_signed_out");
    assert!(
        !empty.exists(),
        "nothing is created before the plan is read"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// `ds pm task attach` is one confirmed write on a Server: it stops at the
/// confirmation gate, names a missing or empty file before any credential is
/// restored, and only then needs the signed-in native user.
#[test]
fn pm_task_attach_is_one_confirmed_write_that_checks_its_file_first() {
    let command = ok(&["capabilities", "pm.task.attach", "--output", "json"])["command"].clone();
    assert_eq!(command["requires"], "server", "{command}");
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "global_write");
    let root = temp_root("pm-attach");
    std::fs::create_dir_all(&root).expect("temp root");
    let empty = root.join("empty.pdf");
    std::fs::write(&empty, b"").expect("empty file");
    let plan = root.join("crossing survey.pdf");
    std::fs::write(&plan, b"%PDF-1.7 crossing").expect("file");
    let attach = |file: &std::path::Path, yes: bool| {
        let mut argv = vec![
            "pm".to_string(),
            "task".into(),
            "attach".into(),
            "--task".into(),
            "T-1".into(),
            "--file".into(),
            file.to_str().expect("utf-8").into(),
            "--output".into(),
            "json".into(),
        ];
        if yes {
            argv.push("--yes".into());
        }
        argv
    };
    let refused = |argv: Vec<String>, native: bool| {
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        if native {
            native_pm_refusal(&argv)
        } else {
            pm_refusal(&argv)
        }
    };
    assert_eq!(
        refused(attach(&plan, false), false),
        "confirmation_required"
    );
    assert_eq!(
        refused(attach(&root.join("absent.pdf"), true), true),
        "attachment_file_invalid"
    );
    assert_eq!(
        refused(attach(&empty, true), true),
        "attachment_file_invalid"
    );
    assert_eq!(refused(attach(&plan, true), true), "headless_signed_out");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn every_work_write_refuses_without_confirmation() {
    // Project Management is shared state governed by ds-brain. Every write
    // here is `global_write`, so dispatch must stop it before any credential
    // is restored — including `respond`, which is the one a person is most
    // likely to script.
    for args in [
        vec!["pm", "task", "create", "--title", "Stake the route"],
        vec![
            "pm",
            "task",
            "update",
            "--task",
            "T-1",
            "--delivery",
            "in_progress",
        ],
        vec![
            "pm",
            "task",
            "assign",
            "--task",
            "T-1",
            "--request",
            "pilot@example.com",
        ],
        vec![
            "pm",
            "task",
            "respond",
            "--task",
            "T-1",
            "--response",
            "accept",
        ],
        // The third-party loop (task-proposals.md): every one is a global
        // write and stops at the gate before the native owner is asked.
        vec![
            "pm", "task", "propose", "--title", "Survey", "--hours", "12",
        ],
        vec!["pm", "task", "request-admission", "--task", "T-1"],
        vec!["pm", "task", "admit", "--task", "T-1", "--under", "root"],
        vec!["pm", "task", "decline", "--task", "T-1", "--reason", "no"],
        vec!["pm", "task", "log-hours", "--task", "T-1", "--hours", "4"],
    ] {
        let mut argv = args.clone();
        argv.extend(["--output", "json"]);
        assert_eq!(
            pm_refusal(&argv),
            "confirmation_required",
            "`ds {}` reached past the confirmation gate",
            args.join(" ")
        );
    }
}

#[test]
fn the_proposal_loop_is_server_native_and_refuses_its_inputs_before_the_round_trip() {
    // A third party proposes, the PM admits: five commands that need a
    // Server and no window (task-proposals.md). Every descriptor says so, and
    // the input refusals a caller can hit locally are named before the native
    // owner is asked — a missing required flag, a malformed number, a note
    // over the bound.
    for id in [
        "pm.task.propose",
        "pm.task.request-admission",
        "pm.task.admit",
        "pm.task.decline",
        "pm.task.log-hours",
    ] {
        let command = ok(&["capabilities", id, "--output", "json"])["command"].clone();
        assert_eq!(
            command["requires"], "server",
            "`{id}` must not need a window"
        );
        assert_eq!(
            command["authority"], "headless_project",
            "`{id}` is the native user's write"
        );
        assert_eq!(command["effect"], "global_write");
        let args: Vec<&str> = command["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .filter_map(|arg| arg["name"].as_str())
            .collect();
        assert!(
            args.contains(&"lane"),
            "`{id}` takes --lane like every Server command"
        );
        assert!(
            args.contains(&"id"),
            "`{id}` takes --id so a lost answer is retried against the same ledger entry"
        );
    }
    // Under the native profile the handler runs and every local refusal is
    // named before the owner is asked; a well-formed call ends at the
    // signed-out gate, never at a window.
    assert_eq!(
        native_pm_refusal(&[
            "pm", "task", "propose", "--hours", "12", "--output", "json", "--yes"
        ]),
        "missing_input",
        "a proposal without a title is refused before the native owner is asked"
    );
    assert_eq!(
        native_pm_refusal(&[
            "pm", "task", "admit", "--task", "T-1", "--output", "json", "--yes"
        ]),
        "missing_input",
        "an admission names where the task lands"
    );
    assert_eq!(
        native_pm_refusal(&[
            "pm",
            "task",
            "admit",
            "--task",
            "T-1",
            "--under",
            "root",
            "--position",
            "-1",
            "--output",
            "json",
            "--yes"
        ]),
        "invalid_number",
        "a position is a non-negative index"
    );
    let long_note = "n".repeat(301);
    assert_eq!(
        native_pm_refusal(&[
            "pm",
            "task",
            "request-admission",
            "--task",
            "T-1",
            "--note",
            &long_note,
            "--output",
            "json",
            "--yes"
        ]),
        "bound_exceeded",
        "a note over the contract's 300 characters is refused locally"
    );
    assert_eq!(
        native_pm_refusal(&[
            "pm",
            "task",
            "log-hours",
            "--task",
            "T-1",
            "--hours",
            "4",
            "--id",
            "x",
            "--output",
            "json",
            "--yes"
        ]),
        "invalid_command_id",
        "a command id is shaped for the ledger before it is sent"
    );
    assert_eq!(
        native_pm_refusal(&[
            "pm", "task", "decline", "--task", "T-1", "--reason", "  ", "--output", "json", "--yes"
        ]),
        "reason_required",
        "a blank reason is no reason"
    );
    let well_formed = native_pm_refusal(&[
        "pm",
        "task",
        "log-hours",
        "--task",
        "T-1",
        "--hours",
        "4",
        "--output",
        "json",
        "--yes",
    ]);
    assert!(
        NATIVE_AUTH_CODES.contains(&well_formed.as_str()),
        "a well-formed hours entry ends at the native sign-in gate, not a window: {well_formed}"
    );
}

#[test]
fn every_work_command_is_reachable_without_the_desktop_installed() {
    // Same reasoning as the map domain: dispatch checks availability before
    // parsing, so a discovery gate would put `--desktop-descriptor` and every
    // input refusal above out of reach on a machine with no application.
    let index = ok(&["capabilities", "pm", "--output", "json"]);
    let commands = index["commands"].as_array().expect("commands");
    let actual: BTreeSet<&str> = commands
        .iter()
        .map(|command| command["id"].as_str().expect("command id"))
        .collect();
    let expected: BTreeSet<&str> = [
        "pm.plan",
        "pm.project.list",
        "pm.task.comment",
        "pm.task.comments",
        "pm.task.subdivide",
        "pm.task.progress",
        "pm.member.list",
        "pm.deletion.inventory",
        "pm.deletion.read",
        "pm.deletion.restore",
        "pm.task.list",
        "pm.task.read",
        "pm.task.checklist.read",
        "pm.task.checklist.edit",
        "pm.task.checklist.promote",
        "pm.task.create",
        "pm.task.update",
        "pm.task.delete",
        "pm.task.assign",
        "pm.task.attach",
        "pm.task.respond",
        "pm.task.propose",
        "pm.task.request-admission",
        "pm.task.admit",
        "pm.task.decline",
        "pm.task.log-hours",
        "pm.task.geometry.read",
        "pm.task.geometry.clear",
        "pm.task.detach",
        "pm.overview",
        "pm.model-link.add",
        "pm.model-link.remove",
        "pm.model-link.list",
        "pm.task.block",
        "pm.task.unblock",
        "pm.note.create",
        "pm.note.read",
        "pm.note.query",
        "pm.note.context",
        "pm.note.versions",
        "pm.note.update",
        "pm.note.backup",
        "pm.note.retire",
        "pm.note.restore",
        "pm.note.consolidate",
        "pm.note.checklist.read",
        "pm.note.checklist.edit",
        "pm.document.copy",
        "pm.mail-thread.ingest",
        "pm.record.list",
        "pm.record.read",
        "pm.record.thread",
        "pm.record.create",
        "pm.record.reply",
        "pm.record.update",
        "pm.party.list",
        "pm.party.create",
        "pm.party.update",
    ]
    .into_iter()
    .collect();
    assert_eq!(
        actual, expected,
        "work command coverage list changed; add a specific smoke assertion for the new command before accepting it"
    );
    for command in commands {
        if command["authority"] == "headless_project" || command["authority"] == "headless_user" {
            continue;
        }
        assert!(
            discoverable(command),
            "`{}` reports {} and gates on discovery, which puts --desktop-descriptor out of reach",
            command["id"],
            command["availability"]
        );
    }
    // Reads must never be behind the confirmation gate, and writes must never
    // be outside it. Getting this backwards is silent until an operator runs
    // an unattended session.
    for command in commands {
        let effect = command["effect"].as_str().expect("effect");
        let id = command["id"].as_str().expect("id");
        let write = matches!(
            id,
            "pm.task.create"
                | "pm.task.checklist.edit"
                | "pm.task.checklist.promote"
                | "pm.deletion.restore"
                | "pm.task.update"
                | "pm.task.delete"
                | "pm.task.assign"
                | "pm.task.attach"
                | "pm.task.respond"
                | "pm.task.propose"
                | "pm.task.request-admission"
                | "pm.task.admit"
                | "pm.task.decline"
                | "pm.task.log-hours"
                | "pm.task.geometry.clear"
                | "pm.task.detach"
                | "pm.note.update"
                | "pm.note.backup"
                | "pm.note.retire"
                | "pm.note.restore"
                | "pm.note.consolidate"
                | "pm.model-link.add"
                | "pm.model-link.remove"
                | "pm.task.block"
                | "pm.task.unblock"
                | "pm.task.comment"
                | "pm.task.subdivide"
                | "pm.task.progress"
                | "pm.note.create"
                | "pm.note.checklist.edit"
                | "pm.document.copy"
                | "pm.mail-thread.ingest"
                | "pm.record.create"
                | "pm.record.reply"
                | "pm.record.update"
                | "pm.party.create"
                | "pm.party.update"
        );
        assert_eq!(
            effect,
            if write { "global_write" } else { "read_only" },
            "`{id}` declares the wrong effect class for its blast radius"
        );
    }
}

#[test]
fn model_links_pin_an_exact_catalog_revision_and_refuse_before_any_round_trip() {
    for (id, effect) in [
        ("pm.model-link.add", "global_write"),
        ("pm.model-link.remove", "global_write"),
        ("pm.model-link.list", "read_only"),
    ] {
        let command = ok(&["capabilities", id, "--output", "json"])["command"].clone();
        assert_eq!(command["authority"], "headless_project");
        assert_eq!(command["requires"], "server");
        assert_eq!(command["effect"], effect);
    }
    let add = |extra: &[&str]| {
        let mut args = vec!["pm", "model-link", "add"];
        args.extend_from_slice(extra);
        args.extend(["--output", "json"]);
        native_pm_refusal(&args)
    };
    // A write is behind the confirmation gate.
    assert_eq!(
        add(&["--task", "T4", "--model", "model-a", "--revision", "rev-1"]),
        "confirmation_required"
    );
    // Exactly one source.
    assert_eq!(
        add(&[
            "--task",
            "T4",
            "--note",
            "N1",
            "--model",
            "model-a",
            "--revision",
            "rev-1",
            "--yes"
        ]),
        "model_link_source_invalid"
    );
    assert_eq!(
        add(&["--model", "model-a", "--revision", "rev-1", "--yes"]),
        "model_link_source_invalid"
    );
    // A display name is never an identity, and a pin needs a revision or a version.
    assert_eq!(
        add(&[
            "--task",
            "T4",
            "--model",
            "Feeder 7",
            "--revision",
            "rev-1",
            "--yes"
        ]),
        "model_link_invalid"
    );
    assert_eq!(
        add(&["--task", "T4", "--model", "model-a", "--yes"]),
        "model_link_invalid"
    );
    assert_eq!(
        add(&[
            "--task",
            "T4",
            "--model",
            "model-a",
            "--version",
            "0",
            "--yes"
        ]),
        "model_link_invalid"
    );
    // A shaped request reaches native authorization; a signed-out catalog read
    // is not reported as a missing version.
    assert!(
        NATIVE_AUTH_CODES.contains(
            &add(&[
                "--task",
                "T4",
                "--model",
                "model-a",
                "--revision",
                "rev-1",
                "--yes"
            ])
            .as_str()
        ),
        "a shaped model link reaches native authorization without a desktop"
    );
    assert_eq!(
        native_pm_refusal(&["pm", "model-link", "list", "--output", "json"]),
        "model_link_source_invalid"
    );
    assert_eq!(
        native_pm_refusal(&[
            "pm",
            "model-link",
            "list",
            "--task",
            "T4",
            "--model",
            "model-a",
            "--output",
            "json"
        ]),
        "model_link_source_invalid"
    );
}

#[test]
fn project_note_create_is_a_confirmed_project_write_with_one_canonical_input() {
    let described=ok(&["capabilities","pm.note.create","--output","json"]);
    let command=&described["command"];
    assert_eq!(command["authority"],"headless_project");
    assert_eq!(command["effect"],"global_write");
    assert_eq!(command["confirmation_required"],true);
    let inputs=command["inputs"].as_array().unwrap();
    for required in ["project","input"] {
        assert!(inputs.iter().any(|i|i["name"]==required&&i["required"]==true));
    }
    assert!(!inputs.iter().any(|i|matches!(i["name"].as_str(),Some("title"|"body"|"body-file"|"transformer"))));
    let dir=temp_root("canonical-note-create");
    std::fs::create_dir_all(&dir).unwrap();
    let input=dir.join("observation.json");
    std::fs::write(&input,serde_json::to_vec(&json!({
        "operation_id":"smoke-note-create","id":"smoke-observation","reason":"Signed-out authoring admission fixture",
        "draft":{"subject":"Reviewed fixture","body":"Literal test observation.","tags":[],
            "memory":{"schema":"ds.project-note-memory/v1","note_type":"observation","lifecycle":"active",
                "provenance":{"kind":"user_instruction","source_author":"Test author"},
                "scope":{"kind":"project"},"claims":[],"evidence":[]}}
    })).unwrap()).unwrap();
    let args=["pm","note","create","--input",input.to_str().unwrap(),"--output","json"];
    assert_eq!(native_pm_refusal(&args),"confirmation_required");
    let mut confirmed=args.to_vec();confirmed.push("--yes");
    assert!(NATIVE_AUTH_CODES.contains(&native_pm_refusal(&confirmed).as_str()),"canonical input reaches native authorization without a desktop or network write");
    // Availability guards a build without its packaged native profile before
    // handler JSON parsing. The retired public flag is still refused at admission.
    let mut obsolete=confirmed.clone();obsolete.extend(["--body","retired quick-note field"]);
    assert_eq!(native_pm_refusal(&obsolete),"unknown_flag");
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn deletion_recovery_is_project_scoped_and_restores_only_after_confirmation() {
    for (id, effect) in [
        ("pm.deletion.inventory", "read_only"),
        ("pm.deletion.read", "read_only"),
        ("pm.deletion.restore", "global_write"),
    ] {
        let command = ok(&["capabilities", id, "--output", "json"])["command"].clone();
        assert_eq!(command["authority"], "headless_project");
        assert_eq!(command["requires"], "server");
        assert_eq!(command["effect"], effect);
        assert!(
            command["inputs"]
                .as_array()
                .expect("inputs")
                .iter()
                .any(|input| input["name"] == "project" && input["required"] == true)
        );
    }
    assert_eq!(
        native_pm_refusal(&[
            "pm",
            "deletion",
            "inventory",
            "--limit",
            "101",
            "--output",
            "json"
        ]),
        "invalid_number"
    );
    assert_eq!(
        native_pm_refusal(&["pm", "deletion", "read", "--output", "json"]),
        "missing_input"
    );
    let restore = [
        "pm",
        "deletion",
        "restore",
        "--backup",
        "delete-1234",
        "--base-revision",
        "7",
        "--command-id",
        "restore-1234",
        "--output",
        "json",
    ];
    assert_eq!(native_pm_refusal(&restore), "confirmation_required");
    let mut invalid_revision = restore.to_vec();
    invalid_revision[6] = "-1";
    invalid_revision.push("--yes");
    assert_eq!(native_pm_refusal(&invalid_revision), "invalid_number");
    let mut confirmed = restore.to_vec();
    confirmed.push("--yes");
    assert!(
        NATIVE_AUTH_CODES.contains(&native_pm_refusal(&confirmed).as_str()),
        "a shaped restore reaches native authorization without a desktop"
    );
}

#[test]
fn task_delete_requires_reviewed_revision_stable_id_and_confirmation() {
    let described = ok(&["capabilities", "pm.task.delete", "--output", "json"]);
    let command = &described["command"];
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["requires"], "server");
    assert_eq!(command["effect"], "global_write");
    assert!(
        command["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .any(|input| input["name"] == "project" && input["required"] == true)
    );

    let delete = [
        "pm",
        "task",
        "delete",
        "--task",
        "T-0007",
        "--base-revision",
        "7",
        "--command-id",
        "delete-1234",
        "--output",
        "json",
    ];
    assert_eq!(native_pm_refusal(&delete), "confirmation_required");
    let mut missing_task = delete.to_vec();
    missing_task.splice(3..5, []);
    missing_task.push("--yes");
    assert_eq!(native_pm_refusal(&missing_task), "missing_input");
    let mut invalid_revision = delete.to_vec();
    invalid_revision[6] = "-1";
    invalid_revision.push("--yes");
    assert_eq!(native_pm_refusal(&invalid_revision), "invalid_number");
    let mut invalid_id = delete.to_vec();
    invalid_id[8] = "delete.short";
    invalid_id.push("--yes");
    assert_eq!(native_pm_refusal(&invalid_id), "invalid_command_id");
    let mut confirmed = delete.to_vec();
    confirmed.push("--yes");
    assert!(
        NATIVE_AUTH_CODES.contains(&native_pm_refusal(&confirmed).as_str()),
        "a shaped deletion reaches native authorization without a desktop"
    );
}



#[test]
fn feedback_is_one_confirmed_shared_write() {
    let index = ok(&["capabilities", "feedback", "--output", "json"]);
    let commands = index["commands"].as_array().expect("commands");
    // The domain is the loop: report a gap, find it again, close it. Reading
    // the backlog must stay outside the confirmation gate and both writes
    // inside it — getting that backwards is silent until an unattended
    // session either cannot read or closes something without being asked.
    let effects: Vec<(&str, &str)> = commands
        .iter()
        .map(|command| {
            (
                command["id"].as_str().expect("id"),
                command["effect"].as_str().expect("effect"),
            )
        })
        .collect();
    assert_eq!(
        effects,
        [
            ("feedback.submit", "global_write"),
            ("feedback.list", "read_only"),
            ("feedback.note", "global_write"),
            ("feedback.close", "global_write"),
            ("feedback.draft", "read_only"),
        ]
    );
    for command in commands {
        if command["authority"] == "headless_user" {
            continue;
        }
        assert!(
            discoverable(command),
            "`{}` reports {} and gates on discovery, which puts --desktop-descriptor out of reach",
            command["id"],
            command["availability"]
        );
    }

    // `ds feedback` reaches the shared backlog as the restored native user, so
    // the fixture profile below is what keeps a synthetic report out of the
    // real backlog: it authenticates against nothing.
    let base = vec![
        "feedback",
        "submit",
        "--title",
        "Missing export",
        "--detail",
        "Live discovery found no export. Acceptance: expose one typed export.",
        "--component",
        "ds-cli",
        "--agent",
        "test-agent",
    ];
    let mut unconfirmed = base.to_vec();
    unconfirmed.extend(["--output", "json"]);
    assert_eq!(
        refusal(&unconfirmed),
        "confirmation_required",
        "feedback reached the shared backlog without explicit confirmation"
    );

    let mut confirmed = base.to_vec();
    confirmed.extend(["--yes", "--output", "json"]);
    let code = native_refusal(&confirmed);
    assert!(
        NATIVE_AUTH_CODES.contains(&code.as_str()),
        "a valid feedback report ended in `{code}`, not a native authentication outcome"
    );

    // FEEDBACK 9ab57db2: a report names a live repository; a retired label is
    // refused before anything is sent, with the repository that owns it.
    let mut retired = confirmed.clone();
    retired[7] = "ds-grid-engine/sag";
    let refused = native_ds(&retired);
    assert_eq!(
        refused.envelope["error"]["code"],
        "feedback_component_unknown"
    );
    assert!(
        refused.envelope["error"]["message"]
            .as_str()
            .is_some_and(|message| message.contains("ds-network owns it")),
        "{}",
        refused.stdout
    );
}

/// `ds feedback draft --last` turns the newest journalled refusal into a ready
/// report: the command id and code, never a value; with reporting off nothing
/// is journalled and the answer says so instead of looking like a quiet day.
#[test]
fn feedback_draft_turns_the_last_refusal_into_a_ready_report() {
    let state = temp_root("feedback-draft-state");
    let run = |args: &[&str], reporting: &str| {
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(args)
            .env("NO_COLOR", "1")
            .env("XDG_STATE_HOME", &state)
            .env("DS_SRE_REPORTING", reporting)
            .env_remove("DS_NATIVE_CLIENT_PROFILE_BUNDLE")
            .env_remove("DS_CLI_SURFACE")
            .env_remove("DS_CLI_AGENT")
            .output()
            .expect("ds runs");
        serde_json::from_slice::<Value>(&output.stdout).unwrap_or(Value::Null)
    };
    let draft = ["feedback", "draft", "--last", "--output", "json"];
    let off = run(&draft, "off");
    assert_eq!(off["data"]["reporting"], "off");
    assert_eq!(off["data"]["drafts"], json!([]));

    let refused = run(
        &[
            "feedback",
            "submit",
            "--title",
            "VALUE-TITLE",
            "--detail",
            "VALUE-DETAIL",
            "--component",
            "ds-cli",
            "--agent",
            "VALUE-AGENT",
            "--output",
            "json",
        ],
        "on",
    );
    assert_eq!(refused["error"]["code"], "confirmation_required");
    let answer = run(&draft, "on");
    let drafts = answer["data"]["drafts"].as_array().expect("drafts");
    assert_eq!(drafts.len(), 1, "{answer}");
    assert_eq!(
        drafts[0]["title"],
        "ds feedback.submit refused: confirmation_required"
    );
    assert_eq!(drafts[0]["component"], "ds-cli/feedback");
    assert_eq!(drafts[0]["kind"], "friction");
    assert!(
        drafts[0]["context"]["sre_event"]
            .as_str()
            .is_some_and(|id| id.starts_with("inv_"))
    );
    assert!(!answer.to_string().contains("VALUE-"), "{answer}");
    assert!(
        run(
            &[
                "feedback", "draft", "--last", "--n", "11", "--output", "json"
            ],
            "on"
        )["error"]["code"]
            == "invalid_count"
    );
    let _ = std::fs::remove_dir_all(&state);
}

/// The backlog is read as a DIFFERENCE, and the discipline lives in the API.
///
/// What this can prove without a signed-in machine is the shape of the promise:
/// the caller never hands over a timestamp it had to remember, it hands back an
/// opaque token the backlog issued — or nothing at all, and the backlog reads
/// from where the account left off. `--since` is gone precisely because it made
/// remembering the caller's job, which meant a local file on one box and a full
/// rescan on every other one.
#[test]
fn feedback_list_takes_an_issued_cursor_and_never_a_caller_timestamp() {
    let descriptor = ok(&["capabilities", "feedback.list", "--output", "json"])["command"].clone();
    let flags: Vec<&str> = descriptor["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("name"))
        .collect();
    assert!(
        !flags.contains(&"since"),
        "`--since` made the CALLER responsible for remembering where it was: {flags:?}"
    );
    assert!(
        flags.contains(&"cursor") && flags.contains(&"all"),
        "the difference read needs an issued token and a way to ignore it: {flags:?}"
    );

    // A token over the 4096-byte bound (d86769ae) is refused before a round
    // trip, with the code and the remedy a caller can act on.
    let hand_built = native_ds(&[
        "feedback",
        "list",
        "--cursor",
        &"x".repeat(4097),
        "--output",
        "json",
    ]);
    assert_ne!(hand_built.code, 0);
    assert_eq!(hand_built.envelope["error"]["code"], "invalid_text");

    // And the refusals a difference read can hit are declared, with remedies.
    let codes: Vec<&str> = descriptor["refusals"]
        .as_array()
        .expect("refusals")
        .iter()
        .map(|refusal| refusal["code"].as_str().expect("code"))
        .collect();
    for required in ["feedback_cursor_rejected", "feedback_backlog_too_large"] {
        assert!(
            codes.contains(&required),
            "`{required}` is reachable but undeclared: {codes:?}"
        );
    }
}

/// A session that touches a report either closes it with evidence or says what
/// it waits on. `note` is the second half of that, and it is a write that never
/// moves the report: its status choices do not exist, because there are none.
#[test]
fn feedback_note_records_a_blocker_and_can_never_close_a_report() {
    let descriptor = ok(&["capabilities", "feedback.note", "--output", "json"])["command"].clone();
    let flags: Vec<&str> = descriptor["inputs"]
        .as_array()
        .expect("inputs")
        .iter()
        .map(|input| input["name"].as_str().expect("name"))
        .collect();
    assert!(
        !flags.contains(&"status") && !flags.contains(&"resolution"),
        "a note must not be able to settle a report: {flags:?}"
    );
    assert!(flags.contains(&"blocked-on") && flags.contains(&"unblock"));

    // A settled report is refused by name, and that name is not the version
    // conflict — the two want opposite things from the caller.
    let codes: Vec<&str> = descriptor["refusals"]
        .as_array()
        .expect("refusals")
        .iter()
        .map(|refusal| refusal["code"].as_str().expect("code"))
        .collect();
    assert!(codes.contains(&"feedback_settled") && codes.contains(&"feedback_conflict"));
    assert!(codes.contains(&"feedback_note_limit"));

    // The note bound refuses locally rather than truncating what somebody wrote.
    let over_bound = native_ds(&[
        "feedback",
        "note",
        "--id",
        "feedback-1",
        "--text",
        &"x".repeat(1_001),
        "--yes",
        "--output",
        "json",
    ]);
    assert_ne!(over_bound.code, 0);
    assert_eq!(over_bound.envelope["error"]["code"], "invalid_text");

    // A write on shared state is never taken from an unconfirmed invocation.
    let unconfirmed = ds(&[
        "feedback",
        "note",
        "--id",
        "feedback-1",
        "--text",
        "Waits on the canary deploy.",
        "--output",
        "json",
    ]);
    assert_ne!(unconfirmed.code, 0);
    assert_eq!(
        unconfirmed.envelope["error"]["code"],
        "confirmation_required"
    );
}

#[test]
fn feedback_validates_context_before_it_is_sent() {
    let code = native_refusal(&[
        "feedback",
        "submit",
        "--title",
        "Missing export",
        "--detail",
        "Live discovery found no export.",
        "--component",
        "ds-cli",
        "--agent",
        "test-agent",
        "--context",
        "not-a-pair",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(code, "invalid_context");
}

#[test]
fn a_well_formed_pm_call_ends_at_the_native_credential_and_never_at_a_window() {
    // Whatever this machine holds, a correct invocation must end at the
    // native credential — never an input refusal, never an internal error,
    // and never a pairing outcome: since 2026-09-20 no `ds pm` command has a
    // window path. With the development catalogue and an empty config home
    // that end is `headless_signed_out`, reached AFTER every local check.
    let calls: Vec<Vec<&str>> = vec![
        vec!["pm", "plan"],
        vec!["pm", "task", "list"],
        vec![
            "pm",
            "task",
            "list",
            "--state",
            "blocked",
            "--assignee",
            "pilot@example.com",
            "--placement",
            "inbox",
            "--limit",
            "25",
            "--page",
            "1",
        ],
        vec!["pm", "task", "read", "--task", "T-1"],
        vec![
            "pm",
            "task",
            "create",
            "--title",
            "Stake the route",
            "--kind",
            "milestone",
            "--start",
            "2026-09-01",
            "--yes",
        ],
        vec![
            "pm",
            "task",
            "update",
            "--task",
            "T-1",
            "--title",
            "Stake the MV route",
            "--priority",
            "high",
            "--delivery",
            "in_progress",
            "--progress",
            "40",
            "--finish",
            "2026-09-20",
            "--yes",
        ],
        vec![
            "pm",
            "task",
            "assign",
            "--task",
            "T-1",
            "--request",
            "pilot@example.com",
            "--yes",
        ],
        vec![
            "pm",
            "task",
            "assign",
            "--task",
            "T-1",
            "--withdraw",
            "--yes",
        ],
        vec![
            "pm",
            "task",
            "respond",
            "--task",
            "T-1",
            "--response",
            "accept",
            "--yes",
        ],
        vec!["pm", "record", "list", "--category", "review"],
        vec!["pm", "record", "read", "--record", "R-1"],
    ];
    for args in &calls {
        let mut argv = args.clone();
        argv.extend(["--output", "json"]);
        assert_eq!(
            native_pm_refusal(&argv),
            "headless_signed_out",
            "`ds {}` must end at the native credential, not before and not at a window",
            args.join(" ")
        );
    }

    // And the surface says so: nothing in `pm` needs a window.
    let index = ok(&["capabilities", "pm", "--output", "json"]);
    for command in index["commands"].as_array().expect("commands") {
        let expected_authority = if command["id"] == "pm.project.list" {
            "headless_user"
        } else {
            "headless_project"
        };
        assert_eq!(
            command["authority"], expected_authority,
            "`{}` has the wrong authority",
            command["id"]
        );
        assert_ne!(
            command["availability"], "requires_window",
            "`{}` still claims a window",
            command["id"]
        );
    }
}

// ---------------------------------------------------------------------------
// assets
// ---------------------------------------------------------------------------

#[test]
fn assets_validates_its_own_inputs_before_any_round_trip() {
    // Every refusal below must be reachable on a machine with no credential,
    // because that is every CI machine — and because a caller who pasted half
    // an asset id should hear which flag was wrong, not that nobody is signed
    // in. The development catalogue makes the native availability gate pass
    // so the command's own input validation is what answers.
    let long_query = "contract ".repeat(30); // 270 characters; the bound is 200
    let long_layer = "L".repeat(96); // the local layer name is bounded at 80, as the desktop bounds it
    let missing_file = temp_root("assets-ingest-missing").display().to_string();
    let new_file = temp_root("assets-read-out").display().to_string();
    // An existing path — the temp directory itself — and a path under a
    // directory that does not exist. The desktop shares this filesystem, so
    // both are known here, before any round trip.
    let existing_path = std::env::temp_dir().display().to_string();
    let orphan_file = temp_root("assets-read-no-such-dir")
        .join("lot3.pdf")
        .display()
        .to_string();
    let projected = "sys:transformer_report:AGASHARU/2026-08-report.pdf";
    let minted = "a_7kq3nr2v0b1c";

    let cases: Vec<(Vec<&str>, &str)> = vec![
        // A page larger than the application returns must be refused by the
        // bound it names, not one round trip later.
        (vec!["assets", "list", "--limit", "500"], "invalid_number"),
        // A transposed day and month is the commonest filter mistake there
        // is, and `2026-01-09` for the ninth of September is a valid date
        // that quietly lists the wrong eight months.
        (
            vec!["assets", "list", "--since", "01-09-2026"],
            "invalid_date",
        ),
        (
            vec!["assets", "list", "--folder", "/contracts"],
            "invalid_folder_path",
        ),
        // The kind vocabulary is closed at the parser, so a plausible synonym
        // never reaches the catalogue as an empty result.
        (
            vec!["assets", "list", "--kind", "spreadsheet"],
            "invalid_choice",
        ),
        (vec!["assets", "tree", "--depth", "40"], "invalid_number"),
        // A truncated paste of a minted id.
        (
            vec!["assets", "tree", "--into", "a_7kq3nr2v"],
            "invalid_asset_id",
        ),
        (vec!["assets", "tree", "--link", "t_4812"], "invalid_link"),
        // A tree read filters on one link; a second is refused rather than
        // folded or dropped, and by this domain's code rather than the
        // parser's `repeated_flag`.
        (
            vec![
                "assets",
                "tree",
                "--link",
                "pm_task:t_1",
                "--link",
                "pm_task:t_2",
            ],
            "invalid_link",
        ),
        (
            vec!["assets", "tree", "--query", &long_query],
            "invalid_query",
        ),
        // A relative destination: the desktop writes this file, so a path
        // relative to *this* process's directory means nothing there.
        (
            vec![
                "assets",
                "read",
                "--asset",
                minted,
                "--out",
                "Downloads/lot3.pdf",
            ],
            "invalid_out_path",
        ),
        (
            vec!["assets", "read", "--asset", "lot3.pdf", "--out", &new_file],
            "invalid_asset_id",
        ),
        // An existing destination is never overwritten, and it is refused
        // here rather than after a project round trip.
        (
            vec!["assets", "read", "--asset", minted, "--out", &existing_path],
            "invalid_out_path",
        ),
        (
            vec!["assets", "read", "--asset", minted, "--out", &orphan_file],
            "invalid_out_path",
        ),
        // A link with a segment too many is a mistake, not an odd id; the
        // desktop adapter reads it the same way.
        (
            vec!["assets", "tree", "--link", "pm_task:t_4812:extra"],
            "invalid_link",
        ),
        (
            vec!["assets", "preview", "--asset", minted, "--pages", "9"],
            "invalid_number",
        ),
        (
            vec!["assets", "preview", "--asset", minted, "--rows", "5000"],
            "invalid_number",
        ),
        // A classify with no change flag is a round trip that could not have
        // changed anything.
        (
            vec!["assets", "classify", "--asset", minted, "--yes"],
            "nothing_to_update",
        ),
        (
            vec![
                "assets",
                "classify",
                "--asset",
                minted,
                "--sensitivity",
                "secret",
                "--yes",
            ],
            "invalid_choice",
        ),
        // A projected row is the kernel's reading of something the project
        // already owns. Classifying it would be classifying a view.
        (
            vec![
                "assets", "classify", "--asset", projected, "--status", "durable", "--yes",
            ],
            "projected_asset_read_only",
        ),
        (
            vec![
                "assets",
                "promote",
                "--asset",
                minted,
                "--as-layer",
                &long_layer,
            ],
            "invalid_layer_name",
        ),
        // Linking to nothing has no intent to send.
        (
            vec!["assets", "attach", "--asset", minted, "--yes"],
            "invalid_attachment",
        ),
        // A task and a DS object are two different links.
        (
            vec![
                "assets",
                "attach",
                "--asset",
                minted,
                "--task",
                "t_4812",
                "--object-type",
                "transformer",
                "--entity-id",
                "TX-104",
                "--yes",
            ],
            "invalid_attachment",
        ),
        (
            vec![
                "assets", "attach", "--asset", projected, "--task", "t_4812", "--yes",
            ],
            "projected_asset_read_only",
        ),
        (
            vec!["assets", "ingest", "--path", "lot3.pdf", "--yes"],
            "invalid_source_path",
        ),
        (
            vec!["assets", "ingest", "--path", &missing_file, "--yes"],
            "invalid_source_path",
        ),
        (
            vec!["assets", "folder", "--path", "/contracts", "--yes"],
            "invalid_folder_path",
        ),
        (
            vec![
                "assets",
                "folder",
                "--path",
                "contracts/2026",
                "--rename-to",
                "../elsewhere",
                "--yes",
            ],
            "invalid_folder_path",
        ),
        // A declaration under a projected root would be a second folder
        // authority, and it would vanish on the next refresh.
        (
            vec![
                "assets",
                "folder",
                "--path",
                "Transformers/AGASHARU",
                "--yes",
            ],
            "projected_asset_read_only",
        ),
    ];

    for (args, expected) in cases {
        let mut argv = args.clone();
        argv.extend(["--project", "test_project"]);
        argv.extend(["--output", "json"]);
        assert_eq!(
            native_refusal(&argv),
            expected,
            "`ds {}` must be refused locally as `{expected}`, before any project round trip",
            args.join(" ")
        );
    }
}

#[test]
fn every_assets_write_refuses_without_confirmation() {
    // The catalogue is shared, governed state: a classification drives who may
    // read the bytes, an ingest publishes a file to everyone on the project,
    // and a folder default is inherited by everything filed under it. Each is
    // `global_write`, so dispatch must stop it before the bridge opens.
    for args in [
        vec![
            "assets",
            "classify",
            "--asset",
            "a_7kq3nr2v0b1c",
            "--status",
            "durable",
        ],
        vec![
            "assets",
            "attach",
            "--asset",
            "a_7kq3nr2v0b1c",
            "--task",
            "t_4812",
        ],
        vec!["assets", "ingest", "--path", "/tmp/lot3.pdf"],
        vec!["assets", "folder", "--path", "contracts/2026"],
    ] {
        let mut argv = args.clone();
        argv.extend(["--project", "test_project"]);
        argv.extend(["--output", "json"]);
        assert_eq!(
            refusal(&argv),
            "confirmation_required",
            "`ds {}` reached past the confirmation gate",
            args.join(" ")
        );
    }

    // And the other way round, which is the half that fails silently: reading
    // an asset to a new file and promoting one to a local layer change nothing
    // anyone else can see, so neither may sit behind a gate that would stop an
    // unattended session from ever reading a document.
    let out = temp_root("assets-unconfirmed-read").display().to_string();
    for args in [
        vec![
            "assets",
            "read",
            "--asset",
            "a_7kq3nr2v0b1c",
            "--out",
            out.as_str(),
        ],
        vec![
            "assets",
            "promote",
            "--asset",
            "a_7kq3nr2v0b1c",
            "--as-layer",
            "Lot3-poles",
        ],
    ] {
        let mut argv = args.clone();
        argv.extend(["--project", "test_project"]);
        argv.extend(["--output", "json"]);
        assert_ne!(
            refusal(&argv),
            "confirmation_required",
            "`ds {}` is behind the confirmation gate, but it writes nothing anyone else can see",
            args.join(" ")
        );
    }
}

#[test]
fn shared_network_commands_expose_tag_identity_and_manual_entry() {
    let published = ok(&["capabilities", "assets.map.publish", "--output", "json"]);
    assert_eq!(published["command"]["authority"], "headless_project");
    assert_eq!(published["command"]["effect"], "global_write");
    assert_eq!(
        refusal(&[
            "assets",
            "map",
            "publish",
            "--file",
            "/missing-map.pdf",
            "--project",
            "test_project",
            "--yes",
            "--output",
            "json"
        ]),
        "native_profile_not_configured"
    );
    let maps = ok(&["capabilities", "assets.maps", "--output", "json"]);
    assert_eq!(maps["command"]["authority"], "headless_project");
    assert!(
        maps["command"]["output"]
            .as_str()
            .unwrap()
            .contains("tag-group-map")
    );
    assert_eq!(
        refusal(&[
            "assets",
            "maps",
            "--project",
            "test_project",
            "--output",
            "json"
        ]),
        "native_profile_not_configured"
    );
    let resolved = ok(&["capabilities", "assets.resolve", "--output", "json"]);
    let command = &resolved["command"];
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "read_only");
    assert!(
        command["output"]
            .as_str()
            .unwrap()
            .contains("manual_entry_allowed=true")
    );
    let inputs = command["inputs"].as_array().unwrap();
    assert!(inputs.iter().any(|input| input["name"] == "tag-definition"));
    assert!(!inputs.iter().any(|input| matches!(
        input["name"].as_str(),
        Some("url" | "bucket" | "object-path")
    )));
    let seeded = ok(&["capabilities", "solar.network.resolve", "--output", "json"]);
    assert!(
        seeded["command"]["purpose"]
            .as_str()
            .unwrap()
            .contains("Missing artifacts seed manual entry")
    );
    let saved = ok(&["capabilities", "solar.network.save", "--output", "json"]);
    assert_eq!(saved["command"]["authority"], "none");
    assert_eq!(saved["command"]["effect"], "local_file_write");
    assert!(
        saved["command"]["purpose"]
            .as_str()
            .unwrap()
            .contains("empty sources object is valid")
    );
}

#[test]
fn assets_backup_plans_files_and_zip_without_identity_or_a_desktop() {
    let directory = tempfile::tempdir().unwrap();
    let request = directory.path().join("ledger.json");
    let request_path = request.display().to_string();
    let rows = json!([
        {
            "ds_project": "archived-project",
            "transformer_name": "alpha",
            "backup_layer_uri": "gs://backup/transformers/alpha.dsgrid",
            "deletion_reason": "Duplicate import",
            "source_country": "RW",
            "district": "Gisagara",
            "source_admin_bounds_json": "{\"district\":\"Gisagara\"}"
        },
        {
            "ds_project": "archived-project",
            "transformer_name": "beta",
            "backup_json_uri": "gs://backup/transformers/beta.firestore.json"
        }
    ]);
    std::fs::write(
        &request,
        serde_json::to_vec(&json!({"action": "project", "rows": rows})).unwrap(),
    )
    .unwrap();
    // Native dispatch runs with an empty identity home and an absent desktop.
    // Planning must work from this ledger, without resolving an active project.
    let projected = native_ds(&[
        "assets",
        "backup",
        "plan",
        "--request",
        &request_path,
        "--output",
        "json",
    ]);
    assert_eq!(projected.code, 0, "{}", projected.envelope);
    let projected_rows = &projected.envelope["data"]["rows"];
    assert_eq!(projected_rows[0]["archive"]["format"], "dsgrid");
    assert_eq!(projected_rows[1]["archive"]["format"], "json");
    assert_eq!(projected_rows[0]["deletion_reason"], "Duplicate import");
    assert_eq!(
        projected_rows[0]["administrative_location"]["district"],
        "Gisagara"
    );
    assert_eq!(projected_rows[0]["location_missing"], false);

    for (selected_ids, delivery, count) in [
        (json!([projected_rows[0]["id"]]), "file", 1),
        (
            json!([projected_rows[0]["id"], projected_rows[1]["id"]]),
            "zip",
            2,
        ),
    ] {
        std::fs::write(
            &request,
            serde_json::to_vec(
                &json!({"action": "download", "rows": rows, "selected_ids": selected_ids}),
            )
            .unwrap(),
        )
        .unwrap();
        let planned = native_ds(&[
            "assets",
            "backup",
            "plan",
            "--request",
            &request_path,
            "--output",
            "json",
        ]);
        assert_eq!(planned.code, 0, "{}", planned.envelope);
        assert_eq!(planned.envelope["data"]["delivery"], delivery);
        assert_eq!(
            planned.envelope["data"]["targets"]
                .as_array()
                .unwrap()
                .len(),
            count
        );
    }
}

#[test]
fn asset_download_names_read_authority_and_refuses_unsafe_targets_before_auth() {
    let described = native_ds(&["capabilities", "assets.download", "--output", "json"]);
    assert_eq!(described.code, 0, "{}", described.envelope);
    let command = &described.envelope["data"]["command"];
    assert_eq!(command["effect"], "read_only");
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["requires"], "server");
    assert_eq!(command["confirmation_required"], false);
    assert!(
        command["purpose"]
            .as_str()
            .unwrap()
            .contains("No asset bytes are fetched")
    );

    for (asset, expected) in [
        ("not_an_asset", "invalid_asset_id"),
        (
            "sys:grid_export:model:export:pdf",
            "origin_read_unavailable",
        ),
        ("a_4v6w339ffevs", "headless_signed_out"),
    ] {
        let refused = native_ds(&[
            "assets",
            "download",
            "--project",
            "named-project",
            "--asset",
            asset,
            "--output",
            "json",
        ]);
        assert_ne!(refused.code, 0, "{}", refused.envelope);
        assert_eq!(
            refused.envelope["error"]["code"], expected,
            "{}",
            refused.envelope
        );
        assert!(refused.envelope["data"].is_null());
    }
    assert_eq!(
        native_refusal(&[
            "assets",
            "download",
            "--asset",
            "a_4v6w339ffevs",
            "--output",
            "json"
        ]),
        "missing_input",
        "a download reference must name its project before restoring credentials"
    );
}

#[test]
fn every_assets_command_is_reachable_without_the_desktop_installed() {
    // Every command of this domain is headless since 2026-09-20; the index
    // is pinned so a new command gets its own smoke assertion, and the effect
    // classes are pinned because they decide what an unattended session may
    // run.
    let index = ok(&["capabilities", "assets", "--output", "json"]);
    let commands = index["commands"].as_array().expect("commands");
    let actual: BTreeSet<&str> = commands
        .iter()
        .map(|command| command["id"].as_str().expect("command id"))
        .collect();
    let expected: BTreeSet<&str> = [
        "assets.list",
        "assets.tree",
        "assets.versions",
        "assets.read",
        "assets.download",
        "assets.preview",
        "assets.classify",
        "assets.promote",
        "assets.attach",
        "assets.ingest",
        "assets.folder",
        "assets.reference",
        "assets.resolve",
        "assets.maps",
        "assets.map.publish",
        "assets.backup.plan",
        "assets.semantic.lookup",
        "assets.semantic.read",
        "assets.semantic.export",
        "assets.semantic.import",
    ]
    .into_iter()
    .collect();
    assert_eq!(
        actual, expected,
        "assets command coverage list changed; add a specific smoke assertion for the new command before accepting it"
    );
    // Semantic document reads stay read-only or local; only the explicit
    // import writes the project and therefore asks for confirmation.
    for (id, effect, confirm) in [
        ("assets.semantic.lookup", "read_only", false),
        ("assets.semantic.read", "local_file_write", false),
        ("assets.semantic.export", "local_file_write", false),
        ("assets.semantic.import", "global_write", true),
    ] {
        let command = &ok(&["capabilities", id, "--output", "json"])["command"];
        assert_eq!(command["effect"], effect, "{id}");
        assert_eq!(command["confirmation_required"], confirm, "{id}");
    }
    for command in commands {
        assert_ne!(
            command["availability"], "requires_window",
            "`{}` still claims a window",
            command["id"]
        );
        if command["id"] != "assets.backup.plan" {
            let id = command["id"].as_str().expect("id");
            let descriptor = ok(&["capabilities", id, "--output", "json"]);
            let project = descriptor["command"]["inputs"]
                .as_array()
                .and_then(|inputs| inputs.iter().find(|input| input["name"] == "project"))
                .unwrap_or_else(|| panic!("{id} lacks --project"));
            assert_eq!(project["required"], true, "{id} allows an implicit project");
            assert!(project["default"].is_null(), "{id} defaults its project");
        }
    }
    assert_eq!(
        refusal(&["assets", "list", "--output", "json"]),
        "missing_input",
        "an asset read without --project must refuse before credential access"
    );
    // The index (2026-09-25): an unqualified list is the project's timeline,
    // and the version history answers only for an id the index can hold.
    let list = ok(&["capabilities", "assets.list", "--output", "json"]);
    let order = list["command"]["inputs"]
        .as_array()
        .and_then(|inputs| inputs.iter().find(|input| input["name"] == "order"))
        .expect("assets.list declares --order");
    assert_eq!(
        order["default"], "recent",
        "the timeline is the default read"
    );
    assert_eq!(order["choices"], serde_json::json!(["recent", "name"]));
    assert_eq!(
        native_refusal(&[
            "assets",
            "versions",
            "--asset",
            "sys:",
            "--project",
            "test_project",
            "--output",
            "json",
        ]),
        "invalid_asset_id",
        "a malformed id must be refused before credential access"
    );
    // The effect class is what decides whether an unattended session may run
    // the command at all, and it is invisible until one does. `read` writes
    // one local file, `promote` adds a layer to this machine's prepared local
    // store, and the four catalogue writes change what everyone on the
    // project sees.
    for command in commands {
        let id = command["id"].as_str().expect("id");
        let expected = match id {
            "assets.list" | "assets.tree" | "assets.versions" | "assets.preview"
            | "assets.download" | "assets.resolve" | "assets.maps" | "assets.backup.plan" => {
                "read_only"
            }
            "assets.semantic.lookup" => "read_only",
            "assets.read"
            | "assets.promote"
            | "assets.semantic.read"
            | "assets.semantic.export" => "local_file_write",
            _ => "global_write",
        };
        assert_eq!(
            command["effect"].as_str().expect("effect"),
            expected,
            "`{id}` declares the wrong effect class for its blast radius"
        );
    }
}

#[test]
fn a_well_formed_assets_call_ends_at_the_native_credential_and_never_at_a_window() {
    // Whatever this machine holds, a correct invocation must end at the
    // native credential — never an input refusal, never an internal error,
    // and never a pairing outcome: since 2026-09-20 no `ds assets` command has
    // a window path. With the development catalogue and an empty config home
    // that end is `headless_signed_out`, reached AFTER every local check.
    let out = temp_root("assets-smoke-out").display().to_string();
    let source = temp_root("assets-smoke-source.pdf");
    std::fs::write(&source, b"%PDF-1.7 smoke\n").expect("temp ingest source is writable");
    let source = source.display().to_string();
    let calls: Vec<Vec<&str>> = vec![
        vec!["assets", "list"],
        vec![
            "assets",
            "list",
            "--folder",
            "contracts/2026",
            "--kind",
            "doc",
            "--status",
            "durable",
            "--sensitivity",
            "internal",
            "--since",
            "2026-09-01",
            "--limit",
            "25",
        ],
        vec![
            "assets",
            "tree",
            "--folder",
            "contracts",
            "--depth",
            "2",
            "--kind",
            "user",
            "--query",
            "lot3",
            "--link",
            "pm_task:t_4812",
        ],
        vec!["assets", "tree", "--into", "a_7kq3nr2v0b1c"],
        vec![
            "assets",
            "list",
            "--order",
            "name",
            "--folder",
            "Transformers/TX-104",
            "--refresh",
        ],
        vec!["assets", "tree", "--refresh", "--depth", "2"],
        vec!["assets", "versions", "--asset", "sys:transformers:TX-104"],
        vec![
            "assets",
            "read",
            "--asset",
            "a_7kq3nr2v0b1c",
            "--member",
            "Lot3/gis/poles.shp",
            "--out",
            out.as_str(),
        ],
        vec![
            "assets",
            "preview",
            "--asset",
            "a_7kq3nr2v0b1c",
            "--member",
            "Lot3/gis/poles.shp",
            "--pages",
            "2",
            "--rows",
            "50",
        ],
        vec![
            "assets",
            "classify",
            "--asset",
            "a_7kq3nr2v0b1c",
            "--kind",
            "doc",
            "--status",
            "durable",
            "--folder",
            "contracts/2026/epc",
            "--sensitivity",
            "restricted",
            "--reason",
            "signed original",
            "--yes",
        ],
        vec![
            "assets",
            "promote",
            "--asset",
            "a_7kq3nr2v0b1c",
            "--member",
            "Lot3/gis/poles.shp",
            "--as-layer",
            "Lot3-poles",
        ],
        vec![
            "assets",
            "attach",
            "--asset",
            "a_7kq3nr2v0b1c",
            "--task",
            "t_4812",
            "--yes",
        ],
        vec![
            "assets",
            "attach",
            "--asset",
            "a_7kq3nr2v0b1c",
            "--object-type",
            "transformer",
            "--entity-id",
            "TX-104",
            "--detach",
            "--yes",
        ],
        vec![
            "assets",
            "ingest",
            "--path",
            source.as_str(),
            "--folder",
            "contracts/2026",
            "--sensitivity",
            "internal",
            "--yes",
        ],
        vec![
            "assets",
            "folder",
            "--path",
            "contracts/2026/epc",
            "--sensitivity",
            "internal",
            "--status",
            "fresh",
            "--yes",
        ],
        vec![
            "assets",
            "folder",
            "--path",
            "contracts/2026/epc",
            "--rename-to",
            "epc-signed",
            "--yes",
        ],
    ];
    for args in &calls {
        let mut argv = args.clone();
        argv.extend(["--project", "test_project"]);
        argv.extend(["--output", "json"]);
        assert_eq!(
            native_refusal(&argv),
            "headless_signed_out",
            "`ds {}` must end at the native credential, not before and not at a window",
            args.join(" ")
        );
    }

    let _ = std::fs::remove_file(&source);
}

// ---------------------------------------------------------------------------
// sre
// ---------------------------------------------------------------------------

#[test]
fn sre_is_two_global_read_only_commands_with_stable_defaults() {
    let index = ok(&["capabilities", "sre", "--output", "json"]);
    let commands = index["commands"].as_array().expect("commands");
    let actual: BTreeSet<&str> = commands
        .iter()
        .map(|command| command["id"].as_str().expect("command id"))
        .collect();
    let expected: BTreeSet<&str> = ["sre.overview", "sre.events"].into_iter().collect();
    assert_eq!(
        actual, expected,
        "SRE command coverage list changed; add a specific smoke assertion for the new command before accepting it"
    );
    for command in commands {
        assert_eq!(command["effect"], "read_only");
        assert_eq!(
            command["authority"], "headless_user",
            "platform reliability is global to the signed-in user, and needs no window"
        );
    }

    let events = ok(&["capabilities", "sre.events", "--output", "json"]);
    let inputs = events["command"]["inputs"].as_array().expect("inputs");
    let input = |name: &str| {
        inputs
            .iter()
            .find(|input| input["name"] == name)
            .unwrap_or_else(|| panic!("missing --{name}"))
    };
    assert_eq!(input("days")["default"], "3");
    assert_eq!(input("limit")["default"], "50");
    assert_eq!(input("scan-limit")["default"], "1000");
    assert_eq!(input("outcome")["default"], "failure");
    assert_eq!(
        input("outcome")["choices"],
        serde_json::json!(["failure", "success", "all"])
    );
    assert_eq!(input("project")["required"], false);
}

#[test]
fn capability_search_finds_sre_by_operator_vocabulary() {
    for query in ["recent errors", "diagnostic logs", "reliability events"] {
        let data = ok(&["capabilities", "--search", query, "--output", "json"]);
        let results = data["results"].as_array().expect("search results");
        assert!(
            results.iter().any(|row| row["id"] == "sre.events"),
            "`{query}` did not find sre.events: {results:?}"
        );
    }
}





#[test]
fn sre_validates_bounds_and_filters_before_the_window_is_opened() {
    // Every one of these is refused by name on a machine that has never signed
    // in, because a typo is a local answer and not a round trip.
    for (flag, value, expected) in [
        ("days", "0", "invalid_number"),
        ("days", "366", "invalid_number"),
        ("limit", "0", "invalid_number"),
        ("limit", "251", "invalid_number"),
        ("scan-limit", "0", "invalid_number"),
        ("scan-limit", "5001", "invalid_number"),
        ("service", " ds-brain", "invalid_text"),
        ("event-lane", "canary ", "invalid_text"),
    ] {
        assert_eq!(
            native_refusal(&[
                "sre",
                "events",
                &format!("--{flag}"),
                value,
                "--output",
                "json"
            ]),
            expected,
            "--{flag} {value} reached the reliability authority"
        );
    }
    assert_eq!(
        native_refusal(&["sre", "events", "--outcome", "unknown", "--output", "json"]),
        "invalid_choice"
    );
    // `--lane` means the caller's credential lane here exactly as it does
    // everywhere else in `ds`; the lane an event was recorded on is a filter
    // with its own name.
    assert_eq!(
        native_refusal(&["sre", "events", "--lane", "production", "--output", "json"]),
        "invalid_choice"
    );
}

#[test]
fn well_formed_sre_reads_reach_only_the_native_authentication_boundary() {
    // Neither read asks for a window and neither asks for a project. On a
    // machine with no restored user they stop at authentication — which is the
    // whole point: before 2026-09-18 they stopped at `desktop_not_paired`, so
    // platform health was unreadable from a server.
    for args in [
        vec!["sre", "overview"],
        vec![
            "sre",
            "events",
            "--days",
            "7",
            "--limit",
            "25",
            "--scan-limit",
            "500",
            "--service",
            "ds-brain",
            "--outcome",
            "all",
            "--category",
            "timeout",
            "--event-lane",
            "stable",
            "--action",
            "query_table",
            "--project",
            "project-1",
            "--source",
            "forwarded_compute",
        ],
    ] {
        let mut argv = args.clone();
        argv.extend(["--output", "json"]);
        let code = native_refusal(&argv);
        // A signed-in account without reliability access is refused by the
        // route as `auth_rejected` — the code it emits, not a domain twin.
        assert!(
            code.is_empty()
                || NATIVE_AUTH_CODES.contains(&code.as_str())
                || code == "auth_rejected",
            "`ds {}` failed with `{code}`, which is neither an answer nor an \
             authentication outcome",
            args.join(" ")
        );
        assert_ne!(
            code,
            "desktop_not_paired",
            "`ds {}` still asks for a window",
            args.join(" ")
        );
    }
}

// ---------------------------------------------------------------------------
// shell
// ---------------------------------------------------------------------------

#[test]
fn shell_status_tells_this_shell_from_a_new_one() {
    // The PATH is the fixture here. With the built binary's own directory
    // first, `ds` resolves to this executable; with nothing on it, the answer
    // is a remedy — never a guess.
    let executable = PathBuf::from(env!("CARGO_BIN_EXE_ds"));
    let directory = executable.parent().expect("binary directory");
    let leading = std::env::join_paths([directory.to_path_buf()]).expect("path");

    let status = |path: &std::ffi::OsStr| {
        let output = Command::new(&executable)
            .args(["shell", "status", "--output", "json"])
            .env("NO_COLOR", "1")
            .env("PATH", path)
            .output()
            .expect("ds runs");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let envelope: Value =
            serde_json::from_slice(&output.stdout).expect("shell status emits JSON");
        envelope["data"].clone()
    };

    let data = status(leading.as_os_str());
    assert_eq!(
        data["reachable"], true,
        "the binary's own directory leads the PATH: {data}"
    );
    assert!(
        data["executable"]
            .as_str()
            .is_some_and(|path| PathBuf::from(path).is_file()),
        "the executable is a real file: {data}"
    );
    assert!(data["resolves_to"].as_str().is_some());
    assert_eq!(data["others"].as_array().map_or(0, Vec::len), 0);
    assert!(
        data["remedy"].is_null(),
        "nothing to fix when this shell already finds it: {data}"
    );
    assert!(
        data["registration"]["kind"]
            .as_str()
            .is_some_and(|kind| kind == "windows_user_path" || kind == "unix_local_bin"),
        "the registration names its platform: {data}"
    );

    let data = status(std::ffi::OsStr::new("/nonexistent"));
    assert_eq!(data["reachable"], false);
    assert!(data["resolves_to"].is_null());
    assert!(
        data["remedy"].as_str().is_some_and(
            |remedy| remedy.contains("ds shell register") || remedy.contains("new terminal")
        ),
        "an unreachable ds names the fix: {data}"
    );

    // Doctor folds the two answers into one word, and never fails on them.
    let output = Command::new(&executable)
        .args(["doctor", "--output", "json"])
        .env("NO_COLOR", "1")
        .env("PATH", &leading)
        .output()
        .expect("ds runs");
    let envelope: Value = serde_json::from_slice(&output.stdout).expect("doctor emits JSON");
    assert_eq!(envelope["data"]["shell"]["status"], "reachable");
}

/// `ds feedback` is a paired domain, so a machine without DS GridDesign
/// cannot prove what a close does to the backlog. What it *can* prove — and
/// what a caller depends on — is that every bound is enforced here, before the
/// backlog is called, so a malformed triage call costs one local refusal
/// rather than a round trip and a service error.
#[test]
fn feedback_triage_bounds_are_enforced_before_the_call() {
    let over_limit = native_ds(&["feedback", "list", "--limit", "999", "--output", "json"]);
    assert_ne!(over_limit.code, 0);
    assert_eq!(over_limit.envelope["error"]["code"], "invalid_number");
    assert_eq!(over_limit.envelope["error"]["detail"]["max"], 50);

    // The resolution is the record a reader sees instead of reopening the
    // investigation; the bound is ds-brain's own, held here by hand.
    let long_resolution = "x".repeat(1_001);
    let over_resolution = native_ds(&[
        "feedback",
        "close",
        "--id",
        "feedback-1",
        "--resolution",
        &long_resolution,
        "--yes",
        "--output",
        "json",
    ]);
    assert_ne!(over_resolution.code, 0);
    assert_eq!(over_resolution.envelope["error"]["code"], "invalid_text");

    // Closing names the two addressed statuses only. Reopening a report stays
    // a human triage decision in the application.
    let reopen = native_ds(&[
        "feedback",
        "close",
        "--id",
        "feedback-1",
        "--resolution",
        "Reopening.",
        "--status",
        "open",
        "--yes",
        "--output",
        "json",
    ]);
    assert_ne!(reopen.code, 0);
    assert_eq!(reopen.envelope["error"]["code"], "invalid_choice");

    // A triage decision on shared state is never taken from an unconfirmed
    // invocation, whatever else is right about it.
    let unconfirmed = ds(&[
        "feedback",
        "close",
        "--id",
        "feedback-1",
        "--resolution",
        "Fixed by this session.",
        "--output",
        "json",
    ]);
    assert_ne!(unconfirmed.code, 0);
    assert_eq!(
        unconfirmed.envelope["error"]["code"],
        "confirmation_required"
    );

    // A well-formed triage call must reach the backlog and stop at whatever
    // the identity allows — never an input refusal. The id below belongs to no
    // report, so a signed-in machine refuses it rather than closing anything.
    for args in [
        vec!["feedback", "list", "--view", "all", "--output", "json"],
        vec![
            "feedback",
            "close",
            "--id",
            "ds-cli-smoke-no-such-report",
            "--resolution",
            "Verified against the acceptance condition.",
            "--yes",
            "--output",
            "json",
        ],
    ] {
        let code = native_refusal(&args);
        assert!(
            code.is_empty()
                || NATIVE_AUTH_CODES.contains(&code.as_str())
                || matches!(
                    code.as_str(),
                    "feedback_not_found" | "feedback_not_permitted" | "feedback_conflict"
                ),
            "`ds {}` failed with `{code}`, which is neither a native authentication \
             outcome nor a named triage refusal",
            args.join(" ")
        );
    }
}

// ---------------------------------------------------------------------------
// style — the cartographic axis
// ---------------------------------------------------------------------------

/// Everything `ds style cartography` can decide before it authenticates.
///
/// The axis is deliberately field-free, so unlike the second dimension there
/// is no `ds style read` value list to check a flag against. That makes what
/// *is* locally decidable worth pinning: the bounds, the two closed
/// vocabularies, the seamless tile sizes, and the two contradictions that
/// cannot be right under any document state.
#[test]
fn style_cartography_validates_its_own_inputs_before_it_authenticates() {
    for (case, args, expected) in [
        (
            "a call that asks for nothing",
            vec![
                "style",
                "cartography",
                "plan",
                "--project",
                "test-project",
                "--ref",
                "master/mv_lines",
            ],
            "invalid_cartography",
        ),
        (
            "arrow size sent with a dashed line in the same call",
            vec![
                "style",
                "cartography",
                "plan",
                "--project",
                "test-project",
                "--ref",
                "master/mv_lines",
                "--line-type",
                "dashed",
                "--direction-size",
                "14",
            ],
            "invalid_cartography",
        ),
        (
            "hatch detail sent with the pattern that removes the hatch",
            vec![
                "style",
                "cartography",
                "plan",
                "--project",
                "test-project",
                "--ref",
                "master/service_areas",
                "--fill-pattern",
                "solid",
                "--pattern-color",
                "#B45309",
            ],
            "invalid_cartography",
        ),
        (
            "an arrow smaller than the marker bound",
            vec![
                "style",
                "cartography",
                "plan",
                "--project",
                "test-project",
                "--ref",
                "master/water_mains",
                "--line-type",
                "directional",
                "--direction-size",
                "5",
            ],
            "invalid_number",
        ),
        (
            "arrows spaced beyond the bound",
            vec![
                "style",
                "cartography",
                "plan",
                "--project",
                "test-project",
                "--ref",
                "master/water_mains",
                "--line-type",
                "directional",
                "--direction-spacing",
                "1001",
            ],
            "invalid_number",
        ),
        (
            "a casing wider than the bound",
            vec![
                "style",
                "cartography",
                "plan",
                "--project",
                "test-project",
                "--ref",
                "master/mv_lines",
                "--casing-width",
                "21",
            ],
            "invalid_number",
        ),
        (
            "a hatch stroke wider than the bound",
            vec![
                "style",
                "cartography",
                "plan",
                "--project",
                "test-project",
                "--ref",
                "master/service_areas",
                "--pattern-stroke",
                "7",
            ],
            "invalid_number",
        ),
        (
            "a casing colour that is not a colour",
            vec![
                "style",
                "cartography",
                "plan",
                "--project",
                "test-project",
                "--ref",
                "master/mv_lines",
                "--casing-color",
                "navy",
            ],
            "invalid_color",
        ),
        (
            "a line type outside the closed vocabulary",
            vec![
                "style",
                "cartography",
                "plan",
                "--project",
                "test-project",
                "--ref",
                "master/mv_lines",
                "--line-type",
                "dash_dot",
            ],
            "invalid_choice",
        ),
        (
            "a fill pattern outside the closed vocabulary",
            vec![
                "style",
                "cartography",
                "plan",
                "--project",
                "test-project",
                "--ref",
                "master/service_areas",
                "--fill-pattern",
                "hatch",
            ],
            "invalid_choice",
        ),
        (
            // MapLibre repeats a pattern by tiling its image, so only the
            // powers of two meet without a visible seam at every tile edge.
            "a pattern tile size that would seam",
            vec![
                "style",
                "cartography",
                "plan",
                "--project",
                "test-project",
                "--ref",
                "master/service_areas",
                "--fill-pattern",
                "crosshatch",
                "--pattern-spacing",
                "12",
            ],
            "invalid_choice",
        ),
        (
            "a publish with no confirmation",
            vec![
                "style",
                "cartography",
                "set",
                "--project",
                "test-project",
                "--ref",
                "master/mv_lines",
                "--line-type",
                "dashed",
            ],
            "confirmation_required",
        ),
    ] {
        let mut args = args;
        args.extend(["--output", "json"]);
        assert_eq!(
            native_refusal(&args),
            expected,
            "{case}: `ds {}` did not refuse with `{expected}`",
            args.join(" ")
        );
    }

    // Every seamless size is accepted, so the closed choice is a real bound
    // rather than one spelling that happens to work.
    for spacing in ["4", "8", "16", "32"] {
        let code = native_refusal(&[
            "style",
            "cartography",
            "plan",
            "--project",
            "test-project",
            "--ref",
            "master/service_areas",
            "--fill-pattern",
            "crosshatch",
            "--pattern-spacing",
            spacing,
            "--output",
            "json",
        ]);
        assert!(
            code.is_empty() || NATIVE_AUTH_CODES.contains(&code.as_str()),
            "`--pattern-spacing {spacing}` is seamless but was refused with `{code}`"
        );
    }

    // Adjusting arrow spacing on a ref that already carries the directional
    // type is legitimate: `ds` has not read the document, so it must not
    // invent a contradiction. This one is well formed and may only end where
    // the native user is restored.
    let adjust = native_refusal(&[
        "style",
        "cartography",
        "plan",
        "--project",
        "test-project",
        "--ref",
        "master/water_mains",
        "--direction-spacing",
        "140",
        "--output",
        "json",
    ]);
    assert!(
        adjust.is_empty() || NATIVE_AUTH_CODES.contains(&adjust.as_str()),
        "adjusting arrow spacing alone failed with `{adjust}`, which is not an \
         authentication outcome"
    );
}

/// Each of the three scenarios the cartography commands exist for is one
/// well-formed call that gets as far as the native user and stops there —
/// never an input refusal, and never `style_refused`, which here would mean a
/// handler built a property key the kernel's closed instruction does not name.
#[test]
fn a_well_formed_cartography_call_stops_at_confirmation_or_authentication() {
    for args in [
        // Directional water-flow lines.
        vec![
            "style",
            "cartography",
            "plan",
            "--project",
            "test-project",
            "--ref",
            "master/water_mains",
            "--line-type",
            "directional",
            "--direction-size",
            "14",
            "--direction-spacing",
            "140",
            "--output",
            "json",
        ],
        // Satellite-contrast casing, on the fractional bound.
        vec![
            "style",
            "cartography",
            "plan",
            "--project",
            "test-project",
            "--ref",
            "master/mv_lines",
            "--casing-color",
            "#0F172A",
            "--casing-width",
            "2.5",
            "--output",
            "json",
        ],
        // Crosshatched proposed service areas, every pattern flag at once so
        // every declared key is built at least once by a real invocation.
        vec![
            "style",
            "cartography",
            "plan",
            "--project",
            "test-project",
            "--ref",
            "master/service_areas",
            "--fill-pattern",
            "crosshatch",
            "--pattern-color",
            "#B45309",
            "--pattern-background",
            "#FFFFFF00",
            "--pattern-spacing",
            "8",
            "--pattern-stroke",
            "1",
            "--output",
            "json",
        ],
        // The same, published.
        vec![
            "style",
            "cartography",
            "set",
            "--project",
            "test-project",
            "--ref",
            "master/service_areas",
            "--fill-pattern",
            "crosshatch",
            "--pattern-color",
            "#B45309",
            "--pattern-spacing",
            "8",
            "--yes",
            "--output",
            "json",
        ],
    ] {
        let code = native_refusal(&args);
        assert!(
            code.is_empty() || NATIVE_AUTH_CODES.contains(&code.as_str()),
            "`ds {}` failed with `{code}`, which is not an authentication outcome",
            args.join(" ")
        );
    }
}

/// `ds style` used to declare `paired_availability` on all fifteen commands
/// while `--host` defaulted to `native`: the contract said a window was
/// required, the default route never opened one, and a server was told the
/// whole domain was unavailable. One route means one answer, and this is the
/// shape of it — no host question in the descriptor, and a machine without a
/// native profile refused by name rather than for want of a window.
#[test]
fn ds_style_asks_no_host_question_and_refuses_without_a_window() {
    let domain = ok(&["capabilities", "style", "--output", "json"]);
    let ids: Vec<String> = domain["commands"]
        .as_array()
        .expect("style commands")
        .iter()
        .filter_map(|command| command["id"].as_str())
        .map(str::to_owned)
        .collect();
    assert_eq!(
        ids.len(),
        ds_cli_style::DOMAIN.commands.len(),
        "every style leaf is registered"
    );
    for id in &ids {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let command = &descriptor["command"];
        let flags: Vec<&str> = command["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .filter_map(|input| input["name"].as_str())
            .collect();
        if id == "style.instruction.schema" {
            assert!(
                flags.is_empty(),
                "pure schema discovery needs no project or lane"
            );
            assert!(command["refusals"].as_array().unwrap().is_empty());
            continue;
        }
        assert!(
            flags.contains(&"lane"),
            "`{id}` does not name the lane it authenticates on"
        );
        assert!(
            flags.contains(&"project"),
            "`{id}` does not name the project it reads or writes"
        );
        if flags.contains(&"target") {
            assert!(
                matches!(id.as_str(), "style.resolve" | "style.purpose.index"),
                "only governed resolution or purposes declare a semantic target"
            );
        }
        for windowed in ["host", "desktop-descriptor"] {
            assert!(
                !flags.contains(&windowed),
                "`{id}` still asks `--{windowed}`, which only a paired window needed"
            );
        }
        let codes: Vec<&str> = command["refusals"]
            .as_array()
            .expect("refusals")
            .iter()
            .filter_map(|refusal| refusal["code"].as_str())
            .collect();
        assert!(
            codes.contains(&"headless_signed_out") && codes.contains(&"context_corrupt"),
            "`{id}` must document the two conditions its one route actually has: {codes:?}"
        );
        assert!(
            !codes.iter().any(|code| code.starts_with("desktop_")),
            "`{id}` still documents a paired window's refusal: {codes:?}"
        );
    }

    // And the behaviour, not only the descriptor: with no native profile the
    // refusal names the profile, never a missing window.
    let code = native_refusal(&[
        "style",
        "list",
        "--project",
        "test-project",
        "--output",
        "json",
    ]);
    assert!(
        code.is_empty() || NATIVE_AUTH_CODES.contains(&code.as_str()),
        "`ds style list` refused with `{code}`"
    );
}

#[test]
fn label_field_is_exact_and_a_valid_plan_reaches_project_authority() {
    let valid = native_refusal(&[
        "style",
        "label",
        "plan",
        "--project",
        "test-project",
        "--ref",
        "gt/roads_print",
        "--field",
        "road_no",
        "--output",
        "json",
    ]);
    assert!(
        valid.is_empty() || NATIVE_AUTH_CODES.contains(&valid.as_str()),
        "a valid label field stopped before project authority with `{valid}`"
    );

    assert_eq!(
        native_refusal(&[
            "style",
            "label",
            "plan",
            "--project",
            "test-project",
            "--ref",
            "gt/roads_print",
            "--field",
            " road_no",
            "--output",
            "json",
        ]),
        "invalid_label",
        "surrounding whitespace must not silently select another field"
    );
    assert_eq!(
        native_refusal(&[
            "style",
            "label",
            "set",
            "--project",
            "test-project",
            "--ref",
            "gt/roads_print",
            "--field",
            "road_no",
            "--output",
            "json",
        ]),
        "confirmation_required",
        "label publication must require review and confirmation"
    );
}

/// The three scenarios are what an operator says, not what the flags are
/// called. Discovery has to survive that gap, or the axis stays unreachable
/// to anyone who did not already know it exists.
#[test]
fn capability_search_finds_cartography_by_operator_vocabulary() {
    for (query, expected) in [
        ("water flow direction", "style.cartography.plan"),
        ("directional arrows", "style.cartography.plan"),
        ("satellite contrast casing", "style.cartography.plan"),
        ("dashed line type", "style.cartography.plan"),
        (
            "crosshatch proposed service areas",
            "style.cartography.plan",
        ),
        ("hatching a fill", "style.cartography.plan"),
        ("publish casing satellite", "style.cartography.set"),
    ] {
        let data = ok(&["capabilities", "--search", query, "--output", "json"]);
        let results = data["results"].as_array().expect("search results");
        assert!(
            results.iter().any(|row| row["id"] == expected),
            "`{query}` did not find {expected}: {results:?}"
        );
    }
}

/// Each scenario is also a declared example, so an agent that has found the
/// command is handed the exact invocation instead of a set of flags to
/// assemble. A summary check would not catch this: an example that stops
/// carrying its scenario's flags is how the vocabulary a caller searches for
/// and the contract they then run drift apart.
#[test]
fn every_cartography_scenario_is_a_declared_example_of_its_own_command() {
    for id in ["style.cartography.plan", "style.cartography.set"] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let command = &descriptor["command"];
        let examples: Vec<String> = command["examples"]
            .as_array()
            .expect("examples")
            .iter()
            .filter_map(|example| example["command"].as_str())
            .map(str::to_string)
            .collect();
        for scenario in [
            "--line-type directional",
            "--casing-color",
            "--fill-pattern crosshatch",
        ] {
            assert!(
                examples.iter().any(|example| example.contains(scenario)),
                "`{id}` declares no example for `{scenario}`: {examples:?}"
            );
        }
        let path = command["path"]
            .as_array()
            .expect("path")
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" ");
        for example in &examples {
            assert!(
                example.starts_with(&format!("ds {path} ")),
                "`{id}` example `{example}` does not invoke `ds {path}`"
            );
        }
    }
}

#[test]
fn project_forms_native_reads_include_explicit_selected_project_commands() {
    let native = ok(&[
        "capabilities",
        "survey.project-forms.list",
        "--output",
        "json",
    ]);
    assert_eq!(native["command"]["authority"], "headless_project");
    assert_eq!(native["command"]["effect"], "local_auth_state");
    let names = native["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(names, BTreeSet::from(["lane", "limit", "project"]));
    let settings = ok(&[
        "capabilities",
        "survey.project-form.settings",
        "--output",
        "json",
    ]);
    assert_eq!(settings["command"]["authority"], "headless_project");
    assert_eq!(settings["command"]["effect"], "local_auth_state");
    let names = settings["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(names, BTreeSet::from(["form", "lane", "project"]));
    assert!(
        settings["command"]["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|arg| arg["name"] != "desktop-descriptor")
    );

    let query = ok(&["capabilities", "survey.query", "--output", "json"]);
    assert_eq!(query["command"]["authority"], "headless_project");
    assert_eq!(query["command"]["effect"], "local_auth_state");
    let names = query["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "distinct-field",
            "filter",
            "form",
            "group-by",
            "lane",
            "limit",
            "metric",
            "order",
            "project",
        ])
    );
    for forbidden in [
        "url",
        "body",
        "token",
        "raw",
        "entry",
        "media",
        "desktop-descriptor",
    ] {
        assert!(
            query["command"]["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|arg| arg["name"] != forbidden),
            "survey.query exposed --{forbidden}"
        );
    }

    let entries = ok(&["capabilities", "survey.entries.select", "--output", "json"]);
    assert_eq!(entries["command"]["authority"], "headless_project");
    assert_eq!(entries["command"]["effect"], "local_auth_state");
    let names = entries["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from(["bbox", "form", "lane", "limit", "project"])
    );
    for forbidden in [
        "url",
        "method",
        "body",
        "token",
        "cursor",
        "wkt",
        "geojson",
        "fields",
        "media",
        "deleted",
        "include-deleted",
        "force",
        "authority",
        "desktop-descriptor",
    ] {
        assert!(
            entries["command"]["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|arg| arg["name"] != forbidden),
            "survey.entries.select exposed --{forbidden}"
        );
    }
    let invalid = native_ds(&[
        "survey",
        "entries",
        "select",
        "--project",
        "test-project",
        "--form",
        "poles",
        "--bbox",
        "not-a-bbox",
        "--output",
        "json",
    ]);
    assert_eq!(invalid.code, 2);
    assert_eq!(invalid.envelope["error"]["code"], "survey_entries_invalid");

    let changes = ok(&["capabilities", "survey.entries.changes", "--output", "json"]);
    assert_eq!(changes["command"]["authority"], "headless_project");
    assert_eq!(changes["command"]["effect"], "local_auth_state");
    let names = changes["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "cursor",
            "form",
            "lane",
            "limit",
            "project",
            "updated-after"
        ])
    );
    for forbidden in [
        "url",
        "method",
        "body",
        "token",
        "fields",
        "media",
        "deleted",
        "include-deleted",
        "force",
        "authority",
        "desktop-descriptor",
        "auto-pagination",
    ] {
        assert!(
            changes["command"]["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|arg| arg["name"] != forbidden),
            "survey.entries.changes exposed --{forbidden}"
        );
    }
    let invalid_changes = native_ds(&[
        "survey",
        "entries",
        "changes",
        "--project",
        "test-project",
        "--form",
        "poles",
        "--updated-after",
        "not-a-time",
        "--output",
        "json",
    ]);
    assert_eq!(invalid_changes.code, 2);
    assert_eq!(
        invalid_changes.envelope["error"]["code"],
        "survey_entries_changes_invalid"
    );

    // A route the lane never published and a scope the caller may not read are
    // both HTTP 404, and from 2026-08-20 to 2026-09-18 every one of the Survey
    // read paths was the first while every operator was told the second. The
    // two refusals therefore have to be separately documented on each of these
    // commands, each with its own way out: one says update or publish the
    // route, the other says check the project and the form.
    for (id, scope_code) in [
        ("survey.query", "survey_scope_not_found"),
        ("survey.entries.select", "survey_entries_scope_not_found"),
        ("survey.entries.changes", "survey_entries_scope_not_found"),
    ] {
        let refusals = ok(&["capabilities", id, "--output", "json"])["command"]["refusals"]
            .as_array()
            .expect("refusals")
            .clone();
        let codes = refusals
            .iter()
            .map(|refusal| refusal["code"].as_str().unwrap_or_default().to_owned())
            .collect::<BTreeSet<_>>();
        assert!(
            codes.contains("survey_route_unavailable"),
            "`{id}` can answer `survey_route_unavailable` and does not document it: {codes:?}"
        );
        assert!(
            codes.contains(scope_code),
            "`{id}` no longer documents `{scope_code}`: {codes:?}"
        );
        let route = refusals
            .iter()
            .find(|refusal| refusal["code"] == "survey_route_unavailable")
            .expect("route refusal");
        // The declaration and the refusal are one constant in ds-cli-auth, so
        // this pins the text a caller plans against rather than a copy of it.
        assert_eq!(
            route["remedy"],
            json!(ds_cli_auth::SURVEY_ROUTE_UNAVAILABLE_REFUSAL.remedy),
            "`{id}` no longer declares the refusal the route itself emits"
        );
        assert_eq!(
            route["code"],
            json!(ds_cli_auth::SURVEY_ROUTE_UNAVAILABLE_REFUSAL.code)
        );
        let scope = refusals
            .iter()
            .find(|refusal| refusal["code"] == scope_code)
            .expect("scope refusal");
        assert_ne!(
            route["remedy"], scope["remedy"],
            "`{id}` gives the same remedy for an absent route and an invisible scope"
        );
    }

    let create = ok(&["capabilities", "survey.entries.create", "--output", "json"]);
    assert_eq!(create["command"]["authority"], "headless_project");
    assert_eq!(create["command"]["effect"], "global_write");
    assert_eq!(create["command"]["execution"], "sync");
    assert_eq!(create["command"]["confirmation_required"], true);
    let create_help = native_ds(&["survey", "entries", "create", "--help"]);
    assert_eq!(create_help.code, 0);
    assert!(
        create_help
            .stdout
            .contains("--idempotency-key <opaque-key>")
    );
    assert!(create_help.stdout.contains("Firestore committed"));
    assert!(create_help.stdout.contains("BigQuery mirror unconfirmed"));
    let names = create["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "context-key",
            "created-at",
            "doc-id",
            "document",
            "form",
            "idempotency-key",
            "lane",
            "project",
        ])
    );
    for forbidden in [
        "url",
        "method",
        "body",
        "token",
        "origin",
        "operation",
        "retry",
        "force",
        "authority",
        "desktop-descriptor",
    ] {
        assert!(
            create["command"]["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|arg| arg["name"] != forbidden),
            "survey.entries.create exposed --{forbidden}"
        );
    }
    let unconfirmed_create = native_ds(&[
        "survey",
        "entries",
        "create",
        "--project",
        "test-project",
        "--form",
        "poles",
        "--doc-id",
        "pole-1",
        "--idempotency-key",
        "opaque-1",
        "--created-at",
        "2026-08-30T00:00:00Z",
        "--document",
        "missing.json",
        "--output",
        "json",
    ]);
    assert_eq!(unconfirmed_create.code, 2);
    assert_eq!(
        unconfirmed_create.envelope["error"]["code"],
        "confirmation_required"
    );
    let confirmed_invalid_create = native_ds(&[
        "survey",
        "entries",
        "create",
        "--project",
        "test-project",
        "--form",
        "poles",
        "--doc-id",
        "pole-1",
        "--idempotency-key",
        "opaque-1",
        "--created-at",
        "2026-08-30T00:00:00Z",
        "--document",
        "missing.json",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(confirmed_invalid_create.code, 2);
    assert_eq!(
        confirmed_invalid_create.envelope["error"]["code"],
        "survey_entry_create_document_invalid"
    );

    let import = ok(&["capabilities", "survey.entries.import", "--output", "json"]);
    assert_eq!(import["command"]["authority"], "headless_project");
    assert_eq!(import["command"]["effect"], "global_write");
    assert_eq!(import["command"]["confirmation_required"], true);
    let names = import["command"]["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|arg| arg["name"].as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "checkpoint",
            "file",
            "form",
            "lane",
            "on-error",
            "project",
            "receipt"
        ])
    );
    for forbidden in [
        "concurrency",
        "retry",
        "origin",
        "operation",
        "created-by",
        "token",
        "url",
        "method",
        "source-provenance",
    ] {
        assert!(
            import["command"]["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|arg| arg["name"] != forbidden),
            "survey.entries.import exposed --{forbidden}"
        );
    }
    let root = temp_root("survey-import-preauth");
    std::fs::create_dir_all(&root).unwrap();
    let missing = root.join("missing.ndjson").display().to_string();
    let checkpoint = root.join("state.json").display().to_string();
    let receipt = root.join("receipt.ndjson").display().to_string();
    let invalid_import = native_ds(&[
        "survey",
        "entries",
        "import",
        "--project",
        "test-project",
        "--form",
        "poles",
        "--file",
        &missing,
        "--checkpoint",
        &checkpoint,
        "--receipt",
        &receipt,
        "--yes",
        "--output",
        "json",
    ]);
    #[cfg(not(windows))]
    {
        assert_eq!(invalid_import.code, 2);
        assert_eq!(
            invalid_import.envelope["error"]["code"], "survey_entries_import_source_invalid",
            "the complete local source contract must fail before profile or auth access"
        );
    }
    #[cfg(windows)]
    {
        assert_eq!(invalid_import.code, 3);
        assert_eq!(
            invalid_import.envelope["error"]["code"],
            "survey_entries_import_windows_state_unavailable",
            "Windows must retain the protected-state refusal before import or auth"
        );
    }
    assert!(!PathBuf::from(checkpoint).exists());
    assert!(!PathBuf::from(receipt).exists());
    std::fs::remove_dir_all(root).unwrap();

    let legacy = ok(&[
        "capabilities",
        "survey.project-forms.read",
        "--output",
        "json",
    ]);
    assert_eq!(legacy["command"]["authority"], "headless_project");
    assert!(
        legacy["command"]["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg["name"] == "project" && arg["required"] == true)
    );

    let legacy_editor = ok(&[
        "capabilities",
        "survey.project-form.editor",
        "--output",
        "json",
    ]);
    assert_eq!(legacy_editor["command"]["authority"], "headless_project");
    assert!(
        legacy_editor["command"]["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|arg| arg["name"] == "project" && arg["required"] == true)
    );
}

#[test]
fn survey_discovery_examples_capture_required_project_and_form() {
    // Non-runnable examples still guide callers; placeholders excuse unknown
    // values, not omitted required inputs. Gate the two repaired contracts.
    for id in ["survey.query", "map.layer.list"] {
        let described = ok(&["capabilities", id, "--output", "json"]);
        let command = &described["command"];
        let required = command["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|input| input["required"] == true)
            .map(|input| format!("--{}", input["name"].as_str().unwrap()))
            .collect::<Vec<_>>();
        assert!(required.contains(&"--project".to_owned()));
        for example in command["examples"].as_array().unwrap() {
            let text = example["command"].as_str().unwrap();
            let words = text.split_whitespace().collect::<Vec<_>>();
            for flag in &required {
                let position = words.iter().position(|word| *word == flag.as_str());
                assert!(
                    position.is_some_and(|position| words
                        .get(position + 1)
                        .is_some_and(|value| !value.starts_with("--"))),
                    "{id} example omits required {flag}: {text}"
                );
            }
        }
    }

    let query = ok(&["capabilities", "survey.query", "--output", "json"]);
    let quality = query["command"]["examples"]
        .as_array()
        .unwrap()
        .iter()
        .find(|example| example["command"].as_str().unwrap().contains("--filter"))
        .expect("bounded quality example")["command"]
        .as_str()
        .unwrap();
    let arguments = quality
        .split_whitespace()
        .skip(1)
        .map(|word| {
            word.trim_matches('\'')
                .replace("<project-id>", "test-project")
                .replace("<form-slug>", "test_form")
        })
        .collect::<Vec<_>>();
    assert!(arguments.windows(2).any(|pair| pair == ["--limit", "1"]));
    let args = arguments.iter().map(String::as_str).collect::<Vec<_>>();
    // The advertised command parses through the real boundary, then refuses
    // our empty native identity. It cannot read an operator's project.
    assert_eq!(native_refusal(&args), "headless_signed_out");
    let mut without_project = arguments;
    let position = without_project
        .iter()
        .position(|word| word == "--project")
        .unwrap();
    without_project.drain(position..position + 2);
    assert_eq!(
        native_refusal(
            &without_project
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        ),
        "missing_input"
    );
}

#[test]
fn survey_discovery_skill_routes_to_grammar_and_keeps_runtime_layer_boundary() {
    let survey = include_str!("../../../skills/ds-survey-lifecycle/references/read-data.md");
    assert!(survey.contains("ds capabilities survey.query --output json"));
    assert!(survey.contains("ds survey query --help"));
    let layers = ok(&["capabilities", "map.layer.list", "--output", "json"]);
    assert_eq!(layers["command"]["contract"], 5);
    assert!(
        layers["command"]["output"]
            .as_str()
            .unwrap()
            .contains("No desktop runtime state is read.")
    );
    let skill = include_str!("../../../skills/ds-layer-management/SKILL.md")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    assert!(!skill.contains("runtime_layers"));
    assert!(skill.contains("not loaded desktop runtime state"));
    assert!(skill.contains("their runtime host"));

    // Skill examples are outside Command::examples, but capture the same
    // explicit project. Check command snippets without treating route-only
    // mentions as invocations or duplicating their argument contracts.
    let tags = include_str!("../../../skills/ds-design-tag-groups/SKILL.md");
    let mut examples = 0;
    for snippet in tags.split('`').skip(1).step_by(2) {
        let words = snippet.split_whitespace().collect::<Vec<_>>().join(" ");
        for command in words.split("ds design ").skip(1) {
            if command.contains(" --") {
                assert!(
                    command.contains("--project <id>"),
                    "unscoped tag example: {command}"
                );
                examples += 1;
            }
        }
    }
    assert!(examples > 0, "tag examples were not inspected");
}

#[test]
fn no_survey_example_names_a_deployment_form_slug() {
    // A form slug belongs to one deployment, so an example cannot carry a real
    // one — and the Survey chapter carried `lv_poles_survey`, a slug nothing in
    // this repository defines, in nine examples plus its reference and skill.
    // It read as real, so the first attempt spent a round trip on
    // `survey_scope_not_found`. `contract.rs` only executes examples marked
    // runnable, which none of these are, so no other suite can see it. What is
    // checkable is the convention the chapter already uses for every other
    // caller-supplied value — `<opaque-key>`, `<exact-next-cursor>` — plus the
    // one command that turns the placeholder into a real slug.
    let index = ok(&["capabilities", "survey", "--output", "json"]);
    let ids: Vec<String> = index["commands"]
        .as_array()
        .expect("commands")
        .iter()
        .map(|command| command["id"].as_str().expect("id").to_owned())
        .collect();
    assert!(!ids.is_empty(), "the survey chapter declares no command");

    let mut placeholders = 0usize;
    for id in ids {
        let descriptor = ok(&["capabilities", &id, "--output", "json"]);
        let examples = descriptor["command"]["examples"]
            .as_array()
            .expect("examples")
            .clone();
        let mut names_a_form = false;
        for example in &examples {
            let text = example["command"].as_str().expect("example command");
            let words: Vec<&str> = text.split_whitespace().collect();
            for (position, word) in words.iter().enumerate() {
                let value = match *word {
                    "--form" | "--slug" => words.get(position + 1).copied().unwrap_or_default(),
                    _ => match word
                        .strip_prefix("--form=")
                        .or_else(|| word.strip_prefix("--slug="))
                    {
                        Some(value) => value,
                        None => continue,
                    },
                };
                names_a_form = true;
                placeholders += 1;
                assert!(
                    value.starts_with('<') && value.ends_with('>'),
                    "`{id}` example `{text}` names the form `{value}`. A slug is \
                     per-deployment: examples carry a `<form-slug>` placeholder and \
                     name the appropriate form discovery command."
                );
            }
        }
        // The placeholder is only honest if the example also says where the real
        // slug comes from; otherwise it trades one guess for another.
        if names_a_form {
            // Data reads need participating project forms; global masters do
            // not establish that the selected project uses a form.
            let discovery = match id.as_str() {
                "survey.query"
                | "survey.entries.select"
                | "survey.entries.read"
                | "survey.entries.changes" => "ds survey project-forms list",
                // The working-area choice is over the project's own catalogue,
                // which its read answers with the exact slugs.
                "survey.working-area.select" => "ds survey working-area forms",
                "survey.form.purge-plan" | "survey.form.purge" => {
                    "ds survey forms purge-candidates"
                }
                _ => "ds survey forms list",
            };
            assert!(
                examples.iter().any(|example| {
                    example["note"]
                        .as_str()
                        .unwrap_or_default()
                        .contains(discovery)
                }),
                "`{id}` documents a `<form-slug>` placeholder but no example names \
                 the discovery command that resolves it"
            );
        }
    }
    assert!(
        placeholders >= 9,
        "only {placeholders} survey examples were checked; the nine that carried a \
         fabricated slug must all still be inspected here"
    );
}

#[test]
fn native_layers_persist_without_a_desktop_and_gis_inspection_pins_exact_bytes() {
    let root = temp_root("headless-layers");
    std::fs::create_dir_all(&root).unwrap();
    let invoke = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(args)
            .args(["--output", "json"])
            .env("DS_LAYER_HOME", root.join("layers"))
            .env("DS_NATIVE_STATE_DIR", root.join("auth"))
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(output.status.success(), "{value}");
        value["data"].clone()
    };
    let added = invoke(&[
        "map",
        "layer",
        "add",
        "--name",
        "Offline imagery",
        "--kind",
        "xyz",
        "--url",
        "https://tiles.example/{z}/{x}/{y}.png",
    ]);
    assert_eq!(added["persisted"], "native_local");
    let id = added["layer"].as_str().unwrap();
    let hidden = invoke(&[
        "map",
        "layer",
        "visibility",
        "--layer",
        id,
        "--visible",
        "false",
    ]);
    assert_eq!(hidden["visible"], false);
    let listed = invoke(&["map", "layer", "remote-list"]);
    assert_eq!(listed["layer_count"], 1);
    assert_eq!(listed["layers"][0]["id"], id);
    assert_eq!(listed["layers"][0]["visible"], false);
    let removed = invoke(&["map", "layer", "remove", "--layer", id]);
    assert_eq!(removed["removed"], true);
    assert_eq!(invoke(&["map", "layer", "remote-list"])["layer_count"], 0);
    let file = root.join("roads.geojson");
    std::fs::write(&file, b"abc").unwrap();
    let inspected = invoke(&["map", "data", "inspect", "--path", file.to_str().unwrap()]);
    assert_eq!(inspected["bytes"], 3);
    assert_eq!(
        inspected["sha256"],
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    assert_eq!(inspected["uploaded"], false);
    for id in ["map.data.list", "map.data.upload", "map.data.remove"] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        assert_eq!(descriptor["command"]["authority"], "headless_project");
        assert!(
            descriptor["command"]["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|a| a["name"] != "desktop-descriptor")
        );
    }
    assert_eq!(
        refusal(&[
            "map",
            "data",
            "upload",
            "--project",
            "test-project",
            "--path",
            file.to_str().unwrap(),
            "--output",
            "json"
        ]),
        "confirmation_required"
    );
    assert_eq!(
        refusal(&[
            "map",
            "data",
            "remove",
            "--project",
            "test-project",
            "--upload",
            "roads",
            "--output",
            "json"
        ]),
        "confirmation_required"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn prepared_local_layers_live_a_whole_life_on_disk_and_never_touch_the_original() {
    // The four `ds map local` commands against a real store: prepare a layer
    // from a real GeoJSON file, reopen the store, rename, refuse the removal
    // that was not confirmed, remove, and then look at the file the layer was
    // prepared from. That last assertion is the one this command family exists
    // to keep: the kernel's receipt names the layer's own payload and nothing
    // else, so a removal cannot reach a source.
    let root = temp_root("prepared-local-layers");
    std::fs::create_dir_all(&root).unwrap();
    let source = root.join("roads.geojson");
    let original = br#"{"type":"FeatureCollection","features":[
      {"type":"Feature","properties":{"n":1},"geometry":{"type":"LineString","coordinates":[[30.05,-1.95],[30.06,-1.94]]}},
      {"type":"Feature","properties":{"n":2},"geometry":{"type":"LineString","coordinates":[[30.06,-1.94],[30.07,-1.93]]}}]}"#;
    std::fs::write(&source, original).unwrap();

    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(args)
            .args(["--output", "json"])
            .env("DS_LAYER_HOME", root.join("layers"))
            .env("NO_COLOR", "1")
            .output()
            .unwrap();
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        value
    };
    let ok = |args: &[&str]| {
        let value = run(args);
        assert_eq!(value["status"], "ok", "{value}");
        value["data"].clone()
    };
    let refused = |args: &[&str]| {
        let value = run(args);
        assert_eq!(value["status"], "error", "{value}");
        value["error"]["code"]
            .as_str()
            .unwrap_or_default()
            .to_owned()
    };

    // Reading a store that was never opened creates nothing at all.
    assert_eq!(ok(&["map", "local", "list"])["layer_count"], 0);
    assert!(!root.join("layers").exists());

    // Neither does a refused write. The kernel is asked over the catalogue as
    // it was read — an absent one is an empty registry — so the refusal
    // arrives before this host has created the lane's directory, its lock or
    // anything else under DS_LAYER_HOME.
    assert_eq!(
        refused(&["map", "local", "remove", "--layer", "sketch-nope", "--yes"]),
        "unknown_layer"
    );
    assert_eq!(
        refused(&[
            "map",
            "local",
            "rename",
            "--layer",
            "sketch-nope",
            "--name",
            "Trunk roads"
        ]),
        "unknown_layer"
    );
    assert!(
        !root.join("layers").exists(),
        "a refused write opened a store that was never opened"
    );

    let prepared = ok(&[
        "map",
        "local",
        "register",
        "--file",
        source.to_str().unwrap(),
        "--name",
        "Access roads",
        "--geometry",
        "linestring",
    ]);
    let id = prepared["layer"].as_str().unwrap().to_owned();
    assert!(id.starts_with("sketch-"), "the kernel mints the id: {id}");
    assert_eq!(prepared["feature_count"], 2, "counted from the file itself");
    assert_eq!(prepared["source_kind"], "import");
    assert_eq!(prepared["color"], "#2563eb", "the palette is the kernel's");
    assert_eq!(prepared["persisted"], "native_local");
    let payload = PathBuf::from(prepared["payload"]["path"].as_str().unwrap());
    assert_eq!(
        std::fs::read(&payload).unwrap(),
        original,
        "a verbatim copy"
    );

    // A separate process is a restart: the catalogue is the only memory.
    let listed = ok(&["map", "local", "list"]);
    assert_eq!(listed["layer_count"], 1);
    assert_eq!(listed["layers"][0]["layer"], id.as_str());
    assert_eq!(listed["layers"][0]["origin"]["kind"], "import_file");
    assert_eq!(listed["layers"][0]["origin"]["label"], "roads.geojson");

    // Another DS account's catalogue, and another lane's, are their own: the
    // layer is not there to list, rename or remove, the refusal is the
    // kernel's `unknown_layer`, and this account's payload is not reached.
    assert_eq!(
        ok(&["map", "local", "list", "--account", "other"])["layer_count"],
        0
    );
    assert_eq!(
        ok(&["map", "local", "list", "--lane", "canary"])["layer_count"],
        0
    );
    assert_eq!(
        refused(&[
            "map",
            "local",
            "remove",
            "--layer",
            &id,
            "--account",
            "other",
            "--yes"
        ]),
        "unknown_layer"
    );
    assert_eq!(
        refused(&[
            "map", "local", "rename", "--layer", &id, "--lane", "canary", "--name", "Theirs"
        ]),
        "unknown_layer"
    );
    assert!(
        payload.is_file(),
        "a removal under another account never reaches this account's payload"
    );
    assert_eq!(
        ok(&["map", "local", "list"])["layers"][0]["name"],
        "Access roads"
    );

    let renamed = ok(&[
        "map",
        "local",
        "rename",
        "--layer",
        &id,
        "--name",
        "Trunk roads",
    ]);
    assert_eq!(renamed["applied"], "rename");
    assert_eq!(
        ok(&["map", "local", "list"])["layers"][0]["name"],
        "Trunk roads"
    );
    // An imported layer keeps the name it had inside its own file.
    assert_eq!(
        ok(&["map", "local", "list"])["layers"][0]["source_layer_name"],
        "Access roads"
    );

    assert_eq!(
        refused(&["map", "local", "remove", "--layer", &id]),
        "confirmation_required"
    );
    assert_eq!(
        refused(&["map", "local", "remove", "--layer", "sketch-nope", "--yes"]),
        "unknown_layer",
        "the kernel's own refusal code, not a local rewording"
    );
    assert_eq!(
        refused(&[
            "map",
            "local",
            "register",
            "--file",
            source.to_str().unwrap(),
            "--name",
            "Wrong",
            "--geometry",
            "point"
        ]),
        "invalid_payload",
        "a file whose features are not the declared geometry is refused"
    );

    let removed = ok(&["map", "local", "remove", "--layer", &id, "--yes"]);
    assert_eq!(removed["removed"], true);
    assert_eq!(
        removed["release"],
        json!([{"kind": "features_payload", "id": id}]),
        "the receipt names only the record the layer owns"
    );
    assert_eq!(removed["deleted"], 1);
    assert!(!payload.exists(), "the copied payload is gone");
    assert_eq!(ok(&["map", "local", "list"])["layer_count"], 0);

    assert_eq!(
        std::fs::read(&source).unwrap(),
        original,
        "the file the layer was prepared from is never moved, modified or deleted"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn retired_survey_workspace_is_not_discoverable_and_never_touches_existing_data() {
    let root = temp_root("retired-survey-workspace");
    std::fs::create_dir_all(&root).unwrap();
    let retained = root.join("survey.sqlite");
    let original = b"retained user workspace bytes";
    std::fs::write(&retained, original).unwrap();
    let index = ok(&["capabilities", "survey", "--output", "json"]);
    assert!(index["commands"].as_array().unwrap().iter().all(|command| {
        !command["id"]
            .as_str()
            .unwrap()
            .starts_with("survey.workspace.")
    }));
    for action in ["init", "prepare", "collect", "list", "sync"] {
        let id = format!("survey.workspace.{action}");
        assert_eq!(
            refusal(&["capabilities", &id, "--output", "json"]),
            "unknown_selector"
        );
        let result = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args([
                "survey",
                "workspace",
                action,
                "--workspace",
                root.to_str().unwrap(),
                "--yes",
                "--output",
                "json",
            ])
            .output()
            .unwrap();
        assert!(!result.status.success(), "retired operation {action} ran");
        assert_eq!(std::fs::read(&retained).unwrap(), original);
        assert_eq!(std::fs::read_dir(&root).unwrap().count(), 1);
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn custom_print_area_refuses_bad_selection_before_pairing() {
    for codes in [vec!["24"], vec!["2401", "2401"]] {
        let mut args = vec![
            "desktop",
            "printing",
            "custom",
            "area",
            "--project",
            "p1",
            "--id",
            "huye",
            "--output",
            "json",
        ];
        for code in codes {
            args.extend(["--sector", code]);
        }
        assert_eq!(refusal(&args), "custom_print_area_invalid");
    }
    let descriptor = ok(&[
        "capabilities",
        "desktop.printing.custom.area",
        "--output",
        "json",
    ]);
    assert_eq!(descriptor["command"]["effect"], "read_only");
    assert_eq!(descriptor["command"]["authority"], "desktop_user");
}

#[test]
fn map_export_and_catalog_contracts_keep_native_render_and_project_attachment_distinct() {
    let export = ok(&[
        "capabilities",
        "desktop.printing.map.export",
        "--output",
        "json",
    ]);
    assert_eq!(export["command"]["effect"], "artifact_write");
    assert_eq!(export["command"]["confirmation_required"], true);
    let list = ok(&[
        "capabilities",
        "desktop.printing.map.list",
        "--output",
        "json",
    ]);
    assert_eq!(list["command"]["effect"], "read_only");
    assert!(
        list["command"]["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["name"] == "project" && a["required"] == true)
    );
}

#[test]
fn map_attachment_retry_requires_confirmation_and_canonical_filename() {
    let descriptor = ok(&[
        "capabilities",
        "desktop.printing.map.attach",
        "--output",
        "json",
    ]);
    assert_eq!(descriptor["command"]["effect"], "global_write");
    assert_eq!(descriptor["command"]["confirmation_required"], true);
    assert!(
        descriptor["command"]["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["name"] == "filename" && a["required"] == true)
    );
}

// ---------------------------------------------------------------------------
// style read — what a layer's fields carry, with no map
// ---------------------------------------------------------------------------

/// The acceptance test for the field-domain migration, run as an assertion.
///
/// `ds style read` used to emit `"onMap": null` unconditionally, because the
/// only producer that field ever had was `map.queryRenderedFeatures` in the
/// browser: `ds` could not answer what values a layer's fields carry, what
/// scalar type each one is, or how many features hold each value. The Style
/// Center recovered all three from whatever MapLibre happened to be painting,
/// so the answer depended on the operator's viewport.
///
/// This runs the exact two kernel calls the command now makes — `field_domain`
/// over canonical project features, then `describe_style` over the published
/// catalogue — against the REAL style-catalog snapshot ds-client-core records
/// from the gateway, with no browser, no DOM and no map instance in the
/// process. What it asserts is specifically true of that fixture, not merely
/// well-formed.
/// The recorded gateway fixtures live beside the client that records them —
/// `ds-client-core`, kernel-owned since 2026-09-14 — in the sibling
/// ds-command-kernel checkout. The sibling by default, `DS_COMMAND_KERNEL_DIR`
/// when the layout differs — a git worktree of this repository is two levels
/// deeper, and a silently skipped parity check is worse than none.
fn web_fixture(name: &str) -> PathBuf {
    let root = match std::env::var_os("DS_COMMAND_KERNEL_DIR") {
        Some(explicit) => PathBuf::from(explicit),
        None => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../ds-command-kernel"),
    };
    root.join("crates/ds-client-core/tests/fixtures").join(name)
}

#[test]
fn style_read_answers_what_canonical_data_holds_with_no_map() {
    let snapshot: serde_json::Value = serde_json::from_slice(
        &std::fs::read(web_fixture("style-catalog-snapshot.json"))
            .expect("the recorded style catalogue is readable"),
    )
    .expect("the recorded style catalogue is JSON");

    // Canonical features of a DS Grid structures layer, as a transformer
    // context reports them: property bags only, geometry never travels.
    let features = serde_json::json!([
        {"properties": {"label": "S1", "station_m": 0, "type_id": 4, "alignment_id": "A1"}},
        {"properties": {"label": "S2", "station_m": 137.5, "type_id": 4, "alignment_id": "A1"}},
        {"properties": {"label": "S3", "station_m": 1e21, "type_id": "4", "alignment_id": "A1"}}
    ]);
    let editor = snapshot["style_editors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|editor| editor["style_ref"] == "ds_grid/structures")
        .expect("the recorded catalogue publishes ds_grid/structures");

    let report = ds_command_kernel::field_domain::evaluate(
        &serde_json::to_vec(&serde_json::json!({
            "schema": ds_command_kernel::field_domain::REQUEST_SCHEMA,
            "operation": "observe",
            "layer": "ds_grid/structures",
            "canonical": {"kind": "local", "source": "design_room"},
            "features": features,
            "declared": editor["field_domains"],
            "present": editor["present_values"],
            "published": editor["field_values"],
            "fields": editor["available_fields"],
        }))
        .unwrap(),
    )
    .expect("canonical features are observable");

    let described = ds_command_kernel::style_plan::describe_style(
        &snapshot,
        "ds_grid/structures",
        Some(&report),
    )
    .expect("the recorded catalogue describes ds_grid/structures");

    // The field that used to be structurally unfillable now carries an answer.
    assert!(described.get("onMap").is_none());
    assert_eq!(described["observed"]["status"], "derived");
    assert_eq!(described["observed"]["source"], "design_room");
    assert_eq!(described["observed"]["features"], 3);

    // A MapLibre `match` compares by TYPE. `label` is a string, `station_m` is
    // a number, and `type_id` is carried inconsistently — which is exactly the
    // case a renderer scan reported as whatever was on screen at the time.
    assert_eq!(described["fieldTypes"]["label"], "string");
    assert_eq!(described["fieldTypes"]["station_m"], "number");
    assert_eq!(described["fieldTypes"]["type_id"], "mixed");

    // Counts and the typed labels an arm must carry, both per value.
    assert_eq!(
        described["fieldCounts"]["alignment_id"],
        serde_json::json!({"A1": 3})
    );
    assert_eq!(
        described["fieldMatchLabels"]["station_m"]["137.5"],
        serde_json::json!(137.5)
    );
    // Spelled the way the product spells it, not the way Rust's Display does.
    assert_eq!(
        described["fieldValues"]["station_m"],
        serde_json::json!(["0", "137.5", "1e+21"])
    );

    // Without a canonical source, the answer is a NAMED refusal, never a
    // silent empty vocabulary and never a renderer read.
    let unobserved = ds_command_kernel::style_plan::describe_style(&snapshot, "gt/roads", None)
        .expect("the recorded catalogue describes gt/roads");
    assert_eq!(
        unobserved["observed"]["code"],
        "canonical_source_not_supplied"
    );
    assert!(
        unobserved["observed"]["remedy"]
            .as_str()
            .is_some_and(|remedy| !remedy.is_empty())
    );
}

#[test]
fn printable_inventory_is_headless_and_limits_before_reading_credentials() {
    let descriptor = ok(&["capabilities", "report.transformers", "--output", "json"]);
    let command = &descriptor["command"];
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "local_auth_state");
    let inputs: BTreeSet<_> = command["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|input| input["name"].as_str().unwrap())
        .collect();
    assert_eq!(inputs, BTreeSet::from(["lane", "limit", "project"]));
    assert_eq!(
        native_refusal(&[
            "report",
            "transformers",
            "--project",
            "p1",
            "--limit",
            "0",
            "--output",
            "json"
        ]),
        "report_plan_invalid"
    );
    assert!(
        NATIVE_AUTH_CODES.contains(
            &native_refusal(&[
                "report",
                "transformers",
                "--project",
                "p1",
                "--output",
                "json"
            ])
            .as_str()
        )
    );
    // The project is named on every call; the saved selection is never read.
    assert_eq!(
        native_refusal(&["report", "transformers", "--output", "json"]),
        "missing_input"
    );
}

#[test]
fn report_inventory_plan_matches_native_projection_and_refuses_foreign_facts_without_auth() {
    let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        "../../../ds-command-kernel/tests/fixtures/report-inventory/held-six-old-legacy-three.json",
    );
    let request = fixture.to_str().unwrap();
    let descriptor = ok(&["capabilities", "report.plan", "--output", "json"]);
    assert_eq!(descriptor["command"]["contract"], 3);
    assert_eq!(descriptor["command"]["authority"], "none");
    assert_eq!(descriptor["command"]["effect"], "read_only");
    let actual = ok(&[
        "report",
        "plan",
        "--action",
        "inventory",
        "--request",
        request,
        "--output",
        "json",
    ]);
    let mut facts: Value = serde_json::from_slice(&std::fs::read(&fixture).unwrap()).unwrap();
    facts["command"] = json!("inventory");
    let native: Value = serde_json::from_str(
        &ds_command_kernel::report::evaluate(&serde_json::to_vec(&facts).unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(actual, native["result"]);
    assert_eq!(actual["outputs"].as_array().unwrap().len(), 6);
    assert_eq!(actual["generated_at_ms"], 1790640000000_u64);
    assert!(
        actual["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|file| file["publication_phase"] == "queued")
    );
    facts.as_object_mut().unwrap().remove("command");
    facts["snapshot"]["project"] = json!("another-project");
    let temp = tempfile::tempdir().unwrap();
    let wrong = temp.path().join("foreign.json");
    std::fs::write(&wrong, serde_json::to_vec(&facts).unwrap()).unwrap();
    assert_eq!(
        refusal(&[
            "report",
            "plan",
            "--action",
            "inventory",
            "--request",
            wrong.to_str().unwrap(),
            "--output",
            "json"
        ]),
        "report_plan_invalid"
    );
}

#[test]
fn symbol_icon_collision_control_is_discoverable_without_print_screen_aliasing() {
    let descriptor = ok(&["capabilities", "style.appearance.plan", "--output", "json"]);
    let inputs = descriptor["command"]["inputs"].as_array().unwrap();
    let overlap = inputs
        .iter()
        .find(|input| input["name"] == "icon-overlap")
        .unwrap();
    assert_eq!(overlap["choices"], json!(["on", "off"]));
    assert_eq!(
        refusal(&[
            "style",
            "appearance",
            "plan",
            "--project",
            "test-project",
            "--ref",
            "gt/primary_schools",
            "--icon-overlap",
            "maybe",
            "--output",
            "json"
        ]),
        "invalid_choice"
    );
}

#[test]
fn printing_plans_answer_headlessly_from_the_same_kernel() {
    let root = temp_root("printing-plan");
    std::fs::create_dir_all(&root).unwrap();
    let request = root.join("request.json");
    let file = request.to_str().unwrap();
    std::fs::write(
        &request,
        r#"{"results":[{"transformer":"b","status":"error"},{"transformer":"a","status":"ok"}]}"#,
    )
    .unwrap();
    let out = ok(&[
        "report",
        "plan",
        "--action",
        "batch-outcome",
        "--request",
        file,
        "--output",
        "json",
    ]);
    assert_eq!(out["failed"], 1);
    assert_eq!(out["results"][0]["transformer"], "a");
    std::fs::write(
        &request,
        r#"{"project":"p1","transformer":"combined_transformer"}"#,
    )
    .unwrap();
    assert_eq!(
        ok(&[
            "report",
            "plan",
            "--action",
            "export",
            "--request",
            file,
            "--output",
            "json"
        ])["kind"],
        "combined"
    );
    std::fs::write(
        &request,
        r#"{"project":"p2","transformer":"combined_transformer"}"#,
    )
    .unwrap();
    assert_eq!(
        ok(&[
            "report",
            "plan",
            "--action",
            "export",
            "--request",
            file,
            "--output",
            "json"
        ])["project"],
        "p2"
    );
    std::fs::write(&request, r#"{"command":"export"}"#).unwrap();
    assert_eq!(
        refusal(&[
            "report",
            "plan",
            "--action",
            "export",
            "--request",
            file,
            "--output",
            "json"
        ]),
        "report_plan_invalid"
    );
    std::fs::write(&request, r#"{"snapshot":{"project":"p1","now_ms":300000,"rows":[{"name":"tx_a","phase":"generated","updated_at_ms":100000},{"name":"tx_b","phase":"generating","updated_at_ms":110000}]}}"#).unwrap();
    let activity = ok(&[
        "report",
        "plan",
        "--action",
        "activity",
        "--request",
        file,
        "--output",
        "json",
    ]);
    assert_eq!(activity["generated"], 1);
    assert_eq!(activity["published"], 0);
    assert_eq!(activity["stalled"], true);
    assert_eq!(activity["waiting_on"], "tx_b");
    let mut layout_request = json!({"op":"render_plan","layout":ds_command_kernel::printing::default_layout(),"layer_count":0});
    std::fs::write(&request, serde_json::to_vec(&layout_request).unwrap()).unwrap();
    let missing = ok(&[
        "report",
        "layout",
        "edit",
        "--request",
        file,
        "--output",
        "json",
    ]);
    assert_eq!(missing["ready"], false);
    assert_eq!(missing["refusals"][0]["code"], "print_preview_needs_layers");
    layout_request["layer_count"] = json!(1);
    std::fs::write(&request, serde_json::to_vec(&layout_request).unwrap()).unwrap();
    assert_eq!(
        ok(&[
            "report",
            "layout",
            "edit",
            "--request",
            file,
            "--output",
            "json"
        ])["ready"],
        true
    );
}

#[test]
fn settings_writes_require_confirmation_before_file_or_native_io() {
    for args in [
        vec![
            "design",
            "config",
            "set",
            "--sheet",
            "project_settings",
            "--parameter",
            "max_feeder_cable_size",
            "--value",
            "70",
        ],
        vec![
            "design",
            "config",
            "save",
            "--sheet",
            "lv_poles_rules",
            "--file",
            "/this-settings-file-does-not-exist.json",
        ],
        vec![
            "design",
            "config",
            "rule-set",
            "duplicate",
            "--sheet",
            "lv_poles_rules",
            "--source",
            "rule_edcl",
            "--target",
            "copy",
        ],
    ] {
        let mut args = args;
        args.extend(["--project", "test-project", "--output", "json"]);
        assert_eq!(refusal(&args), "confirmation_required");
    }
}

#[test]
fn solar_city_creation_is_an_editable_offline_entry_point() {
    let value = ok(&[
        "capabilities",
        "solar.project.city.create",
        "--output",
        "json",
    ]);
    let command = &value["command"];
    assert_eq!(command["authority"], "none");
    assert_eq!(command["effect"], "local_file_write");
    assert_eq!(command["confirmation_required"], false);
    let inputs = command["inputs"].as_array().unwrap();
    for name in ["workspace", "city", "name"] {
        assert!(
            inputs
                .iter()
                .any(|input| input["name"] == name && input["required"] == true)
        );
    }
    assert!(
        inputs
            .iter()
            .any(|input| input["name"] == "input" && input["required"] != true)
    );
    assert!(
        command["purpose"]
            .as_str()
            .unwrap()
            .contains("preserves the existing city")
    );
    assert!(
        command["output"]
            .as_str()
            .unwrap()
            .contains("Missing inputs are an editable draft")
    );
}

#[test]
fn solar_migration_is_one_solar_verb_with_a_kind_inside_it() {
    // ONE endpoint, two kinds, both inside the Solar domain. A design kind
    // reaching this command would mean the cross-domain migration registry the
    // owner ruled against had been built after all.
    for id in ["solar.migrate.plan", "solar.migrate.apply"] {
        let value = ok(&["capabilities", id, "--output", "json"]);
        let command = &value["command"];
        assert_eq!(command["authority"], "headless_project");
        let inputs = command["inputs"].as_array().unwrap();
        for name in ["project", "source-project", "kind"] {
            assert!(
                inputs
                    .iter()
                    .any(|input| input["name"] == name && input["required"] == true),
                "`{id}` must require --{name}"
            );
        }
        assert!(
            command["purpose"]
                .as_str()
                .unwrap()
                .contains("COMPUTATION RESULTS NEVER MIGRATE")
                || command["purpose"]
                    .as_str()
                    .unwrap()
                    .contains("never_computed_in_this_project"),
            "`{id}` must say what does not migrate"
        );
    }
    assert_eq!(
        ok(&["capabilities", "solar.migrate.plan", "--output", "json"])["command"]["effect"],
        "read_only"
    );
    assert_eq!(
        ok(&["capabilities", "solar.migrate.apply", "--output", "json"])["command"]["effect"],
        "global_write"
    );
}

/// Design and Solar migration are two verbs in two domains, never one
/// registry — but they SPEAK alike, so an operator or agent who learned one
/// drives the other (feedback fb90a0a5). The same concept carries the same
/// argument name, the same shape (value / repeated / switch) and the same
/// obligation (a kind is always stated, never defaulted), and both point at
/// the one reference that documents their shared receipt vocabulary.
#[test]
fn design_and_solar_migration_declare_one_argument_vocabulary() {
    let shape = |id: &str, name: &str| {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let input = descriptor["command"]["inputs"]
            .as_array()
            .expect("inputs")
            .iter()
            .find(|input| input["name"] == name)
            .unwrap_or_else(|| panic!("`{id}` must declare --{name}"))
            .clone();
        (
            input["kind"].clone(),
            input["required"].clone(),
            input.get("default").cloned(),
            descriptor["command"]["reference"].clone(),
        )
    };
    for (design, solar) in [
        ("design.migrate.plan", "solar.migrate.plan"),
        ("design.migrate.apply", "solar.migrate.apply"),
    ] {
        for name in ["source-project", "kind", "overwrite", "lane"] {
            let (design_kind, design_required, design_default, design_reference) =
                shape(design, name);
            let (solar_kind, solar_required, _, solar_reference) = shape(solar, name);
            assert_eq!(design_kind, solar_kind, "--{name} shape");
            assert_eq!(
                design_reference, solar_reference,
                "one documented vocabulary"
            );
            if name != "lane" {
                assert_eq!(design_required, solar_required, "--{name} obligation");
            }
            if name == "kind" {
                assert_eq!(design_required, true, "a kind is always stated");
                assert_eq!(design_default, None, "a kind is never defaulted");
            }
        }
        // One object per repeated --item on both. Design requires at least one
        // (it has no "every object" read); Solar's omission means every source
        // object of the kind — the one difference, stated in each summary.
        assert_eq!(shape(design, "item").0, "repeated");
        assert_eq!(shape(solar, "item").0, "repeated");
        for retired in ["from", "items", "city", "portfolio"] {
            for id in [design, solar] {
                let descriptor = ok(&["capabilities", id, "--output", "json"]);
                assert!(
                    !descriptor["command"]["inputs"]
                        .as_array()
                        .expect("inputs")
                        .iter()
                        .any(|input| input["name"] == retired),
                    "`{id}` must not keep the retired --{retired}"
                );
            }
        }
    }
}

#[test]
fn a_design_kind_never_reaches_the_solar_migration_door() {
    // Refused locally, before any credential is touched: the kind vocabulary
    // is the kernel's, and it has exactly two values.
    assert_eq!(
        refusal(&[
            "solar",
            "migrate",
            "plan",
            "--project",
            "chad_test",
            "--source-project",
            "aderm",
            "--kind",
            "transformer",
            "--output",
            "json",
        ]),
        "solar_migrate_kind_invalid"
    );
}

#[test]
fn a_solar_migration_refuses_a_repeated_item_instead_of_collapsing_it() {
    // A plan describing fewer rows than the caller named is the confident
    // empty answer in miniature, so a repeat is refused, locally.
    assert_eq!(
        refusal(&[
            "solar",
            "migrate",
            "plan",
            "--project",
            "chad_test",
            "--source-project",
            "aderm",
            "--kind",
            "portfolio",
            "--item",
            "North",
            "--item",
            "North",
            "--output",
            "json",
        ]),
        "solar_migrate_selection_invalid"
    );
}

#[test]
fn a_solar_migration_refuses_a_project_migrating_into_itself() {
    assert_eq!(
        refusal(&[
            "solar",
            "migrate",
            "plan",
            "--project",
            "chad_test",
            "--source-project",
            "chad_test",
            "--kind",
            "city",
            "--output",
            "json",
        ]),
        "solar_migrate_source_invalid"
    );
}

#[test]
fn native_design_attachments_need_identity_instead_of_desktop_pairing() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(file.path(), b"opaque file").unwrap();
    let path = file.path().to_str().unwrap();
    let commands = [
        vec![
            "design",
            "attachment",
            "list",
            "--project",
            "project_a",
            "--kind",
            "lv_transformer",
            "--object",
            "tr",
        ],
        vec![
            "design",
            "attachment",
            "publish",
            "--project",
            "project_a",
            "--kind",
            "mv_model",
            "--object",
            "line",
            "--path",
            path,
            "--version",
            "rev_native",
            "--yes",
        ],
        vec![
            "design",
            "attachment",
            "download",
            "--project",
            "project_a",
            "--attachment",
            "file_one",
        ],
        vec![
            "design",
            "attachment",
            "retire",
            "--project",
            "project_a",
            "--attachment",
            "file_one",
            "--yes",
        ],
    ];
    for mut arguments in commands {
        arguments.extend(["--output", "json"]);
        let refused = native_ds(&arguments);
        assert_eq!(
            refused.envelope["error"]["code"],
            "design_attachment_refused"
        );
        assert_eq!(
            refused.envelope["error"]["detail"]["cause"],
            "headless_signed_out"
        );
    }
    let refused = native_ds(&[
        "design",
        "attachment",
        "list",
        "--project",
        "project_a",
        "--kind",
        "mv_model",
        "--object",
        "line",
        "--version",
        "v2",
        "--output",
        "json",
    ]);
    assert_eq!(
        refused.envelope["error"]["code"],
        "design_attachment_refused"
    );
    assert_ne!(
        refused.envelope["error"]["detail"]["cause"], "headless_signed_out",
        "invalid governance pin must be refused before identity/transport"
    );
}

/// The retired ds-work orphan script, answered by the shipped executable.
///
/// A backbone with a T-off starting on it has one junction and three tips.
/// Two tips reach a transformer; the backbone's source end does not, and a
/// stray transformer about 111 m from every line is an orphan at 30 m but not
/// at 150 m. A fill-in placeholder is located but never judged.
#[test]
fn mv_lv_orphans_names_the_ends_and_transformers_beyond_tolerance() {
    let root = temp_root("mv-lv-orphans");
    std::fs::create_dir_all(&root).expect("temp root");
    let mv = root.join("model.geojson");
    let transformers = root.join("transformers.geojson");
    let line = |id: &str, coordinates: Value| {
        json!({"type": "Feature", "properties": {"alignment_id": id},
               "geometry": {"type": "LineString", "coordinates": coordinates}})
    };
    std::fs::write(
        &mv,
        json!({"type": "FeatureCollection", "features": [
            line("backbone", json!([[30.0, -2.0], [30.002, -2.0], [30.004, -2.0]])),
            line("t-off", json!([[30.002, -2.0], [30.002, -1.997]])),
        ]})
        .to_string(),
    )
    .expect("mv fixture");
    let point = |name: &str, lon: f64, lat: f64| {
        json!({"type": "Feature", "properties": {"name": name},
               "geometry": {"type": "Point", "coordinates": [lon, lat]}})
    };
    std::fs::write(
        &transformers,
        json!({"type": "FeatureCollection", "features": [
            point("east", 30.004, -1.9999),
            point("north", 30.002, -1.9969),
            point("stray", 30.001, -2.001),
            point("fill_in_far", 30.05, -2.05),
        ]})
        .to_string(),
    )
    .expect("transformer fixture");
    let mv = mv.to_str().expect("utf-8 path");
    let transformers = transformers.to_str().expect("utf-8 path");
    let out = root.join("orphans.geojson");
    let receipt = ok(&[
        "data",
        "mv-lv-orphans",
        "--mv",
        mv,
        "--transformers",
        transformers,
        "--tolerance-m",
        "30",
        "--exclude-prefix",
        "fill_in_",
        "--out",
        out.to_str().expect("utf-8 path"),
        "--output",
        "json",
    ]);
    let counts = &receipt["counts"];
    assert_eq!(counts["junctions"], 1, "{receipt}");
    assert_eq!(counts["tips"], 3, "{receipt}");
    assert_eq!(counts["orphan_tips"], 1, "{receipt}");
    assert_eq!(receipt["orphan_tips"][0]["alignment"], "backbone");
    assert_eq!(receipt["orphan_tips"][0]["end"], "start");
    assert_eq!(counts["transformers_excluded"], 1, "{receipt}");
    assert_eq!(counts["orphan_transformers"], 1, "{receipt}");
    let stray = &receipt["orphan_transformers"][0];
    assert_eq!(stray["name"], "stray");
    let distance = stray["distance_to_mv_m"].as_f64().expect("distance");
    assert!((110.0..112.5).contains(&distance), "{distance}");
    let written: Value =
        serde_json::from_slice(&std::fs::read(&out).expect("orphans written")).expect("geojson");
    assert_eq!(written["features"].as_array().map(Vec::len), Some(2));
    let wide = ok(&[
        "data",
        "mv-lv-orphans",
        "--mv",
        mv,
        "--transformers",
        transformers,
        "--tolerance-m",
        "150",
        "--exclude-prefix",
        "fill_in_",
        "--output",
        "json",
    ]);
    assert_eq!(wide["counts"]["orphan_transformers"], 0, "{wide}");
    assert_eq!(
        refusal(&[
            "data",
            "mv-lv-orphans",
            "--mv",
            transformers,
            "--transformers",
            transformers,
            "--tolerance-m",
            "30",
            "--output",
            "json",
        ]),
        "orphans_mv_empty"
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Search retains exact routing for the supported command surface.
#[test]
fn an_outsiders_word_for_an_operation_finds_it_first() {
    for (query, expected) in [
        ("line", "map.line-difference"),
        // The plural is what a stranger types. `buffers` used to return one
        // row, and it was a project-cache command.
        // The words a person who has never opened a terminal uses for the
        // one sign-in. Before 2026-09-22 `sign in` led with `install.show`
        // and `connect device` with `auth.device.list`.
        ("sign in", "account.connect"),
        ("connect device", "account.connect"),
        ("link device", "account.connect"),
        ("signed out", "account.connect"),
        ("connect me", "account.connect"),
        ("account", "account.connect"),
    ] {
        let found = ok(&["capabilities", "--search", query, "--output", "json"]);
        let first = found["results"][0]["id"].as_str().unwrap_or("<nothing>");
        assert_eq!(
            first, expected,
            "`ds capabilities --search {query}` led with `{first}`; a stranger \
             typing that word means `{expected}`"
        );
    }

    // `Link` is not `line`. A term that only appears inside a longer, unrelated
    // word is noise, and it used to be forty rows of it.
    let lines = ok(&["capabilities", "--search", "line", "--output", "json"]);
    let ids: Vec<&str> = lines["results"]
        .as_array()
        .expect("results")
        .iter()
        .filter_map(|row| row["id"].as_str())
        .collect();
    assert!(
        !ids.contains(&"assets.attach"),
        "searching `line` still returns `assets.attach`: {ids:?}"
    );

    // Nothing found is an answer too. `dissolve` is genuinely absent from this
    // stack, and the worst version of saying so is an empty list with no next
    // step — that is where an outside agent gives up on `ds` and installs its
    // own toolchain instead.
    let absent = ok(&["capabilities", "--search", "dissolve", "--output", "json"]);
    assert_eq!(absent["matched"], 0);
    let next = absent["next"].as_str().unwrap_or_default();
    assert!(
        next.contains("ds capabilities") && next.contains("feedback"),
        "an empty search must still route somewhere; it said `{next}`"
    );
}

/// `ds design pinned preview` answers what a read-only pinned working set
/// costs, on a Server, with no browser in the process.
///
/// The three complaints it exists for are three NUMBERS, so they are asserted
/// as numbers here rather than described:
///
///   1. a room the machine already holds but which is missing ONE design class
///      asks for that class — one layer, not the room;
///   2. fifty pinned transformers over two classes are TWO map sources, not a
///      hundred;
///   3. a feature that arrived carrying thirteen property keys keeps two.
///
/// The keep-set is the part that could not be guessed. It is computed by
/// walking the project's OWN published style documents, so this runs against
/// the real style catalogue `ds-client-core` recorded from the gateway — the
/// same fixture `style_read_answers_what_canonical_data_holds_with_no_map`
/// uses. In that catalogue `ds_grid/structures` reads exactly one field,
/// `label`, through its label layer, and `ds_grid/routes` reads exactly
/// `alignment_id`. Nothing else in either document names a property. A
/// hard-coded keep-set would have had to guess those two names and would have
/// been wrong for every other project.
#[test]
fn pinned_context_is_planned_folded_and_projected_with_no_browser() {
    use ds_command_kernel::pinned_context::{
        self, PINNED_TRANSFORMER_PROPERTY, SCHEMA as PINNED_SCHEMA,
    };

    let catalog: serde_json::Value = serde_json::from_slice(
        &std::fs::read(web_fixture("style-catalog-snapshot.json"))
            .expect("the recorded style catalogue is readable"),
    )
    .expect("the recorded style catalogue is JSON");

    // 1. WHAT TO FETCH. The machine holds `structures` for T1 and nothing for
    //    T2; both classes are required. T1 must ask for the ONE class it lacks.
    let plan = pinned_context::plan(
        serde_json::from_value(serde_json::json!({
            "schema": PINNED_SCHEMA,
            "pins": ["T1", "T2"],
            "require": ["structures", "routes"],
            "held": [{"name": "T1", "layers": {"structures": 3}, "version": 7}],
            "heads": [{"name": "T1", "version": 7}, {"name": "T2", "version": 2}],
        }))
        .expect("the plan request encodes"),
    )
    .expect("the kernel plans the pinned set");
    assert_eq!(plan.counts.rooms_fetched, 1, "only T2 is unheld");
    assert_eq!(plan.counts.layers_fetched, 1, "T1 asks for ONE class");
    let partial = plan
        .fetch
        .iter()
        .find(|item| !item.whole_room)
        .expect("T1 is a layer-level fetch");
    assert_eq!(partial.name, "T1");
    assert_eq!(partial.layers, vec!["routes".to_owned()]);
    assert_eq!(plan.projection, pinned_context::CONTEXT_PROJECTION);

    // A head that has MOVED is still a whole room: a moved revision invalidates
    // every layer of the held copy, so a partial repair would paint a lie.
    let moved = pinned_context::plan(
        serde_json::from_value(serde_json::json!({
            "schema": PINNED_SCHEMA,
            "pins": ["T1"],
            "require": ["structures", "routes"],
            "held": [{"name": "T1", "layers": {"structures": 3}, "version": 7}],
            "heads": [{"name": "T1", "version": 8}],
        }))
        .expect("the plan request encodes"),
    )
    .expect("the kernel plans the pinned set");
    assert_eq!(moved.counts.rooms_fetched, 1);
    assert_eq!(moved.counts.layers_fetched, 0);
    assert_eq!(moved.fetch[0].reason, "head_moved");

    // 2 + 3. WHAT ONE LAYER IS, and WHAT IT CARRIES. The keep-set is resolved
    //        from the recorded catalogue, never listed here.
    let resolved = pinned_context::styles(
        serde_json::from_value(serde_json::json!({
            "schema": PINNED_SCHEMA,
            "catalog": catalog,
            "classes": ["structures", "routes"],
        }))
        .expect("the styles request encodes"),
    )
    .expect("the recorded catalogue resolves both classes");
    let structures = &resolved.classes[0];
    assert_eq!(structures.style_ref.as_deref(), Some("ds_grid/structures"));
    assert!(
        !structures.pinned_variant,
        "this project publishes no _pinned variant, so the design document is used",
    );
    assert_eq!(
        structures.fields,
        vec!["label".to_owned()],
        "the recorded ds_grid/structures document reads exactly `label`",
    );
    assert_eq!(resolved.classes[1].fields, vec!["alignment_id".to_owned()]);

    // The bag an editable design feature carries. Thirteen keys, four of them
    // the nested `*_properties` blobs a preview can do nothing with.
    let bag = serde_json::json!({
        "label": "S1",
        "alignment_id": "A1",
        "station_m": 137.5,
        "type_id": 4,
        "doc_id": "d-1",
        "geohash": "kx7y",
        "existing_status": "new",
        "drafting_status": "draft",
        "construction_status": "planned",
        "network_properties": {"phase": "3"},
        "computed_properties": {"span_m": 41.2},
        "desirable_properties": {"pole": "9m"},
        "surveyed_properties": {"by": "crew-2"},
    });
    assert_eq!(bag.as_object().expect("a bag").len(), 13);

    let rooms: Vec<serde_json::Value> = (0..50)
        .map(|index| {
            serde_json::json!({
                "name": format!("T{index}"),
                "layers": {
                    "structures": {"features": [{
                        "id": "shared-doc-id",
                        "geometry": {"type": "Point", "coordinates": [30.0, -1.9]},
                        "properties": bag,
                    }]},
                    "routes": {"features": [{
                        "id": "shared-doc-id",
                        "geometry": {"type": "LineString", "coordinates": [[30.0, -1.9], [30.1, -1.8]]},
                        "properties": bag,
                    }]},
                },
            })
        })
        .collect();
    let styles: serde_json::Map<String, serde_json::Value> = resolved
        .classes
        .iter()
        .map(|class| (class.class_name.clone(), class.document.clone()))
        .collect();

    let merged = pinned_context::merge(
        serde_json::from_value(serde_json::json!({
            "schema": PINNED_SCHEMA,
            "rooms": rooms,
            "styles": styles,
        }))
        .expect("the merge request encodes"),
    )
    .expect("the kernel folds the pinned set");

    // 2. Fifty transformers over two classes: a hundred map sources become two.
    assert_eq!(merged.totals.sources_before, 100);
    assert_eq!(merged.totals.sources_after, 2);
    assert_eq!(merged.totals.layers_before, 100);
    assert_eq!(merged.totals.layers_after, 2);
    assert_eq!(merged.totals.features, 100);

    // 3. Thirteen property keys arrived; two survive on a structures feature —
    //    the one the style reads, and the transformer the row came from.
    assert_eq!(merged.totals.properties_before, 13);
    let structures_layer = merged
        .layers
        .iter()
        .find(|layer| layer.class_name == "structures")
        .expect("the fold keeps a structures layer");
    assert_eq!(
        structures_layer.fields,
        vec!["label".to_owned(), PINNED_TRANSFORMER_PROPERTY.to_owned()],
    );
    assert_eq!(structures_layer.transformers.len(), 50);
    assert_eq!(structures_layer.geometry_type, "Point");
    assert_eq!(
        merged
            .layers
            .iter()
            .find(|layer| layer.class_name == "routes")
            .expect("the fold keeps a routes layer")
            .geometry_type,
        "LineString",
    );

    // A popup on a merged feature still names its transformer, and fifty rooms
    // that all carry the SAME document id stay fifty features.
    let first = &structures_layer.features[0];
    assert_eq!(
        first.properties[PINNED_TRANSFORMER_PROPERTY], "T0",
        "the identity is a property now, because the layer is no longer one transformer's",
    );
    assert_eq!(structures_layer.features.len(), 50);
    let ids: std::collections::BTreeSet<&String> = structures_layer
        .features
        .iter()
        .map(|feature| &feature.id)
        .collect();
    assert_eq!(ids.len(), 50, "ids are namespaced by the transformer");

    // The nested bags never reach a renderer, whatever else changes.
    for blob in [
        "network_properties",
        "computed_properties",
        "desirable_properties",
        "surveyed_properties",
    ] {
        assert!(
            !first.properties.contains_key(blob),
            "`{blob}` is not read by any pinned style document and must not be carried",
        );
    }
}

/// The command's own receipt is bounded and refuses with a remedy. The three
/// numbers above are the kernel's; what this pins is that `ds` reaches them
/// through `Requires::Server` — no window, no map, no paired application.
#[test]
fn pinned_preview_is_a_server_command_that_refuses_an_empty_pin_set() {
    let command = &ds_cli_design::pinned::COMMAND;
    assert_eq!(command.id, "design.pinned.preview");
    assert!(matches!(
        command.requires,
        ds_cli_contract::spec::Requires::Server
    ));
    assert!(
        command
            .refusals
            .iter()
            .all(|refusal| !refusal.remedy.is_empty()),
        "every refusal names what fixes it",
    );

    let refused = native_ds(&[
        "design",
        "pinned",
        "preview",
        "--project",
        "test-project",
        "--output",
        "json",
    ]);
    assert_eq!(refused.envelope["error"]["code"], "pinned_set_empty");
    assert!(
        refused.envelope["error"]["remedy"]
            .as_str()
            .expect("a remedy")
            .contains("--transformer"),
        "the refusal names the flag that fixes it",
    );
}

// ---------------------------------------------------------------------------
// dsgrid feature-codes / criteria / analyse (program contract 03)
// ---------------------------------------------------------------------------

/// The family over a blank package: import the MV standard as one new
/// package, report it, export it as FEA 15, and see the refusals a caller
/// plans for — the standard's digest gate, the class gate, the delivery
/// gate, the missing clearance criteria — without any working copy or
/// window.













// File-only batching must have the same canonical result as single-command
// apply while avoiding intermediate package writes.




// ---------------------------------------------------------------------------
// Versions and submissions (2026-09-25)
// ---------------------------------------------------------------------------










#[path = "smoke/elevation_compare.rs"]
mod elevation_compare;

#[path = "smoke/terrain_sampling.rs"]
mod terrain_sampling;

#[test]
fn member_form_grants_are_headless_bounded_and_apply_stops_before_unconfirmed_effect() {
    for (id, confirmation) in [
        ("survey.member-grant.read", false),
        ("survey.member-grant.plan", false),
        ("survey.member-grant.apply", true),
    ] {
        let descriptor = ok(&["capabilities", id, "--output", "json"]);
        let command = &descriptor["command"];
        assert_eq!(command["authority"], "headless_project");
        assert_eq!(command["confirmation_required"], confirmation);
        let inputs = command["inputs"].as_array().unwrap();
        for name in ["project", "member"] {
            assert!(
                inputs
                    .iter()
                    .any(|arg| arg["name"] == name && arg["required"] == true)
            );
        }
        assert!(!inputs.iter().any(|arg| matches!(
            arg["name"].as_str(),
            Some("role" | "actor" | "url" | "body" | "token" | "desktop-descriptor")
        )));
        assert!(command["output"].as_str().is_some());
    }
    let refused = ds(&[
        "survey",
        "member-grant",
        "apply",
        "--project",
        "explicit-project",
        "--member",
        "member@example.com",
        "--plan",
        "missing-unconfirmed-plan.json",
        "--output",
        "json",
    ]);
    assert_eq!(refused.envelope["error"]["code"], "confirmation_required");
}

#[test]
fn pm_collaboration_adapters_declare_bounded_reads_and_confirmed_task_comment() {
    let index = ok(&["capabilities", "pm.project.list", "--output", "json"]);
    assert_eq!(index["command"]["authority"], "headless_user");
    assert_eq!(index["command"]["effect"], "read_only");
    let comment = ok(&["capabilities", "pm.task.comment", "--output", "json"]);
    assert_eq!(comment["command"]["authority"], "headless_project");
    assert_eq!(comment["command"]["effect"], "global_write");
    let read = ok(&["capabilities", "pm.task.comments", "--output", "json"]);
    assert_eq!(read["command"]["effect"], "read_only");
}

// ---------------------------------------------------------------------------
// pls structure-translate
// ---------------------------------------------------------------------------

/// The public humble-pole design named into a canonical library written here
/// (its own two structure files under canonical names: a test double, never
/// a library anything is delivered from). Without a staking table the
/// baseline is the design as DS reads it; the dry run translates and compares
/// in memory, the write commits the four artifacts.




#[test]
fn survey_delete_preview_runs_the_real_kernel_and_admits_no_write() {
    let root = temp_root("survey-delete-preview");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("held.json");
    std::fs::write(&path, r#"{"rows":[{"form_slug":"edges","feature":{"id":"e","geometry":{"type":"LineString"},"properties":{"connectivity":{"precedent":"n"}}}}]}"#).unwrap();
    let reply = ok(&[
        "survey",
        "entries",
        "delete-plan",
        "--project",
        "p",
        "--form",
        "nodes",
        "--doc-id",
        "n",
        "--document",
        path.to_str().unwrap(),
        "--idempotency-key",
        "review",
        "--now",
        "2026-10-02T00:00:00Z",
        "--output",
        "json",
    ]);
    assert_eq!(reply["deleted"], 2);
    assert_eq!(reply["cascaded_edges"], 1);
    assert_eq!(reply["authorized"], false);
    assert_eq!(reply["intents"][1]["payload"]["doc_id"], "e");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn survey_delete_declares_native_effect_and_honors_packaged_availability() {
    let descriptor = ok(&["capabilities", "survey.entries.delete", "--output", "json"]);
    assert_eq!(descriptor["command"]["effect"], "global_write");
    assert_eq!(descriptor["command"]["authority"], "headless_project");
    let root = temp_root("survey-delete-invalid");
    std::fs::create_dir_all(&root).unwrap();
    let path = root.join("held.json");
    std::fs::write(&path, r#"{"rows":[]}"#).unwrap();
    let reply = ds(&[
        "survey",
        "entries",
        "delete",
        "--project",
        "p",
        "--form",
        "poles",
        "--doc-id",
        "n",
        "--document",
        path.to_str().unwrap(),
        "--idempotency-key",
        "review",
        "--now",
        "invalid-clock",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(
        reply.envelope["error"]["code"],
        if descriptor["command"]["availability"] == "unavailable" {
            descriptor["command"]["unavailable"]["code"]
                .as_str()
                .unwrap()
        } else {
            "survey_delete_plan_invalid"
        }
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn style_instruction_schema_covers_primary_scale_and_guarded_replay() {
    let schema = ok(&["style", "instruction", "schema", "--output", "json"]);
    let text = serde_json::to_string(&schema).unwrap();
    for word in [
        "categorical",
        "color_range",
        "zoom",
        "guarded",
        "halo_color",
        "expected_digest",
    ] {
        assert!(text.contains(word), "schema missing {word}");
    }
    let descriptor = ok(&["capabilities", "style.zoom.plan", "--output", "json"]);
    let inputs = descriptor["command"]["inputs"].as_array().unwrap();
    assert!(
        inputs
            .iter()
            .any(|input| input["name"] == "stop" && input["kind"] == "repeated")
    );
}

#[test]
fn fixed_a4_seed_is_discoverable_and_refuses_unconfirmed_publication() {
    for (id, effect) in [
        ("style.catalogue.a4.plan", "local_auth_state"),
        ("style.catalogue.a4.create", "global_write"),
    ] {
        let described = native_ds(&["capabilities", id, "--output", "json"]);
        assert_eq!(described.code, 0, "{}", described.envelope);
        let command = &described.envelope["data"]["command"];
        assert_eq!(command["effect"], effect);
        assert_eq!(command["authority"], "headless_project");
        assert_eq!(command["requires"], "server");
        assert!(command["purpose"].as_str().unwrap().contains(
            "No style, binding, layout, manifest head or printing-defaults pointer is changed"
        ));
        assert!(
            !command["inputs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|arg| ["path", "body", "manifest"].contains(&arg["name"].as_str().unwrap()))
        );
    }
    let digest = "a".repeat(64);
    let refused = native_ds(&[
        "style",
        "catalogue",
        "a4",
        "create",
        "--project",
        "project_a",
        "--expected-document",
        &digest,
        "--expected-plan",
        &digest,
        "--output",
        "json",
    ]);
    assert_ne!(refused.code, 0);
    assert_eq!(refused.envelope["error"]["code"], "confirmation_required");
}

#[test]
fn print_binding_is_discoverable_and_refuses_invalid_or_unconfirmed_effects() {
    for (id, effect) in [
        ("style.catalogue.binding.plan", "local_auth_state"),
        ("style.catalogue.binding.create", "global_write"),
    ] {
        let described = native_ds(&["capabilities", id, "--output", "json"]);
        assert_eq!(described.code, 0, "{}", described.envelope);
        let command = &described.envelope["data"]["command"];
        assert_eq!(command["effect"], effect);
        assert_eq!(command["authority"], "headless_project");
        assert_eq!(command["requires"], "server");
        assert!(
            command["purpose"]
                .as_str()
                .unwrap()
                .contains("No style body, layout, printing defaults or global page is written")
        );
    }
    let refused = native_ds(&[
        "style",
        "catalogue",
        "binding",
        "plan",
        "--project",
        "project_a",
        "--entity-class",
        "",
        "--source-kind",
        "live_geojson",
        "--role",
        "project",
        "--ref",
        "lv_poles_as_built_print",
        "--output",
        "json",
    ]);
    assert_eq!(
        refused.envelope["error"]["code"],
        "style_governance_invalid"
    );
    let digest = "a".repeat(64);
    let refused = native_ds(&[
        "style",
        "catalogue",
        "binding",
        "create",
        "--project",
        "project_a",
        "--entity-class",
        "survey_existing_poles",
        "--source-kind",
        "live_geojson",
        "--role",
        "project",
        "--ref",
        "lv_poles_as_built_print",
        "--expected-manifest",
        &digest,
        "--expected-plan",
        &digest,
        "--output",
        "json",
    ]);
    assert_eq!(refused.envelope["error"]["code"], "confirmation_required");
}

#[test]
fn retained_print_previews_are_explicit_and_opening_does_not_render() {
    for id in ["report.preview.read", "report.preview.refresh"] {
        let described = native_ds(&["capabilities", id, "--output", "json"]);
        assert_eq!(described.code, 0, "{}", described.envelope);
        let command = &described.envelope["data"]["command"];
        assert_eq!(command["effect"], "artifact_write");
        assert_eq!(command["authority"], "headless_project");
        let purpose = command["purpose"].as_str().unwrap();
        if id == "report.preview.read" {
            assert!(purpose.contains(
                "does not capture current inputs, render, or wake the publication queue"
            ));
        } else {
            assert!(purpose.contains(
                "Every explicit refresh has a fresh run identity, including unchanged inputs"
            ));
        }
    }
    for action in ["read", "refresh"] {
        let refused = native_ds(&[
            "report",
            "preview",
            action,
            "--project",
            "project_a",
            "--transformer",
            "gashariki",
            "--out",
            "/absolute/fresh-preview.pdf",
            "--output",
            "json",
        ]);
        assert_eq!(refused.envelope["error"]["code"], "confirmation_required");
    }
    let refused = native_ds(&[
        "report",
        "preview",
        "read",
        "--project",
        "project_a",
        "--transformer",
        "UPPERCASE",
        "--out",
        "/absolute/fresh-preview.pdf",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(refused.envelope["error"]["code"], "preview_inputs_invalid");
}

#[test]
fn profile_calculation_discovers_native_tickets_and_refuses_transport_overrides_before_pairing() {
    let described = native_ds(&[
        "capabilities",
        "map.profile.calculation",
        "--output",
        "json",
    ]);
    assert_eq!(described.code, 0, "{}", described.envelope);
    let command = &described.envelope["data"]["command"];
    assert!(discoverable(command));
    assert_eq!(command["contract"], 2);
    assert_eq!(command["requires"], "window");
    assert!(
        command["purpose"]
            .as_str()
            .unwrap()
            .contains("No action saves")
    );
    let result = native_ds(&[
        "map",
        "profile",
        "calculation",
        "--model",
        "model-a",
        "--request",
        r#"{"action":"start","job_id":"a","scope":"other"}"#,
        "--output",
        "json",
    ]);
    assert_ne!(result.code, 0);
    assert_eq!(result.envelope["error"]["code"], "invalid_profile_view");
    assert!(result.envelope["data"].is_null());
}

#[test]
fn solar_portfolio_legacy_uploader_is_absent_and_native_observation_stays_discoverable() {
    let removed = native_ds(&["capabilities", "solar.portfolio.publish", "--output", "json"]);
    assert_ne!(removed.code, 0, "{}", removed.envelope);
    let published = native_ds(&["capabilities", "solar.portfolio.published.read", "--output", "json"]);
    assert_eq!(published.code, 0, "{}", published.envelope);
    let schema = ds_cli_server::solar_application_schema(Some("portfolio_publish")).unwrap();
    assert!(schema.to_string().contains("batch_digest"));
    let client = include_str!("../../../../ds-command-kernel/crates/ds-client-core/src/solar_project.rs");
    assert!(!client.contains("pub(crate) fn publish"));
    let portfolio = include_str!("../../../../ds-command-kernel/crates/ds-client-core/src/solar_portfolio.rs");
    assert!(!portfolio.contains("Command::Publish {"));
}


#[test]
fn task_checklist_commands_are_item_scoped_and_refuse_before_unused_reads() {
    for (id, effect) in [
        ("pm.task.checklist.read", "read_only"),
        ("pm.task.checklist.edit", "global_write"),
        ("pm.task.checklist.promote", "global_write"),
    ] {
        let described = ok(&["capabilities", id, "--output", "json"]);
        let command = &described["command"];
        assert_eq!(command["authority"], "headless_project");
        assert_eq!(command["requires"], "server");
        assert_eq!(command["effect"], effect);
        for name in if effect == "global_write" {
            vec!["task", "project", "id", "base-revision"]
        } else {
            vec!["task", "project"]
        } {
            assert!(
                command["inputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|arg| arg["name"] == name && arg["required"] == true),
                "{id} {name}"
            );
        }
    }
    assert_eq!(
        native_pm_refusal(&[
            "pm",
            "task",
            "checklist",
            "read",
            "--task",
            "T4",
            "--limit",
            "201",
            "--output",
            "json"
        ]),
        "invalid_number"
    );
    assert!(
        NATIVE_AUTH_CODES.contains(
            &native_pm_refusal(&[
                "pm",
                "task",
                "checklist",
                "read",
                "--task",
                "T4",
                "--output",
                "json"
            ])
            .as_str()
        )
    );
    let folder = temp_root("checklist-edits");
    std::fs::create_dir_all(&folder).unwrap();
    let input = folder.join("item-edit.json");
    std::fs::write(
        &input,
        br#"{"action":"edit","item_id":"I2","body":"evidence","attachment_ids":[]}"#,
    )
    .unwrap();
    let path = input.to_str().unwrap();
    let args = [
        "pm",
        "task",
        "checklist",
        "edit",
        "--task",
        "T4",
        "--input",
        path,
        "--id",
        "edit-retry-001",
        "--base-revision",
        "7",
        "--output",
        "json",
    ];
    assert_eq!(native_pm_refusal(&args), "confirmation_required");
    let mut confirmed = args.to_vec();
    confirmed.push("--yes");
    assert!(NATIVE_AUTH_CODES.contains(&native_pm_refusal(&confirmed).as_str()));
    std::fs::write(
        &input,
        br#"{"action":"edit","item_id":"I2","task_id":"other-task"}"#,
    )
    .unwrap();
    assert_eq!(native_pm_refusal(&confirmed), "invalid_checklist");
    let args = [
        "pm",
        "task",
        "checklist",
        "promote",
        "--task",
        "T4",
        "--item",
        "I2",
        "--child",
        "child-2",
        "--id",
        "promote-retry-001",
        "--base-revision",
        "7",
        "--output",
        "json",
    ];
    assert_eq!(native_pm_refusal(&args), "confirmation_required");
    let mut confirmed = args.to_vec();
    confirmed.push("--yes");
    assert!(NATIVE_AUTH_CODES.contains(&native_pm_refusal(&confirmed).as_str()));
    std::fs::remove_dir_all(folder).unwrap();
}

#[test]
fn selected_note_and_document_controls_keep_private_scope_and_explicit_intent() {
    for (id, authority, effect) in [
        ("notes.checklist.read", "headless_user", "read_only"),
        ("notes.checklist.edit", "headless_user", "global_write"),
        ("pm.note.checklist.read", "headless_project", "read_only"),
        ("pm.note.checklist.edit", "headless_project", "global_write"),
        ("pm.document.copy", "headless_project", "global_write"),
        ("pm.mail-thread.ingest", "headless_project", "global_write"),
    ] {
        let described = ok(&["capabilities", id, "--output", "json"]);
        let command = &described["command"];
        assert_eq!(command["authority"], authority);
        assert_eq!(command["effect"], effect);
        assert_eq!(command["requires"], "server");
        let args = command["inputs"].as_array().unwrap();
        assert_eq!(
            args.iter().any(|a| a["name"] == "project"),
            authority == "headless_project",
            "private notes cannot acquire a project"
        );
        if effect == "global_write" {
            assert!(
                args.iter()
                    .any(|a| a["name"] == "id" && a["required"] == true)
            );
        }
    }
    assert_eq!(
        native_refusal(&[
            "notes",
            "checklist",
            "read",
            "--note",
            "n",
            "--limit",
            "201",
            "--output",
            "json"
        ]),
        "invalid_number"
    );
    assert!(
        NATIVE_AUTH_CODES.contains(
            &native_refusal(&[
                "notes",
                "checklist",
                "read",
                "--note",
                "n",
                "--output",
                "json"
            ])
            .as_str()
        )
    );
    assert!(
        NATIVE_AUTH_CODES.contains(
            &native_pm_refusal(&[
                "pm",
                "note",
                "checklist",
                "read",
                "--note",
                "n",
                "--output",
                "json"
            ])
            .as_str()
        )
    );
    let folder = temp_root("selected-note-document");
    std::fs::create_dir_all(&folder).unwrap();
    let input = folder.join("intent.json");
    let path = input.to_str().unwrap();
    std::fs::write(
        &input,
        br#"{"action":"check","item_id":"i","answer":"verified"}"#,
    )
    .unwrap();
    let personal = [
        "notes",
        "checklist",
        "edit",
        "--note",
        "n",
        "--input",
        path,
        "--id",
        "note-edit-001",
        "--version",
        "7",
        "--output",
        "json",
    ];
    assert_eq!(native_refusal(&personal), "confirmation_required");
    let mut confirmed = personal.to_vec();
    confirmed.push("--yes");
    assert!(NATIVE_AUTH_CODES.contains(&native_refusal(&confirmed).as_str()));
    let project = [
        "pm",
        "note",
        "checklist",
        "edit",
        "--note",
        "n",
        "--input",
        path,
        "--id",
        "note-edit-001",
        "--version",
        "7",
        "--output",
        "json",
        "--yes",
    ];
    assert!(NATIVE_AUTH_CODES.contains(&native_pm_refusal(&project).as_str()));
    std::fs::write(
        &input,
        br#"{"action":"check","item_id":"i","owner":"other"}"#,
    )
    .unwrap();
    assert_eq!(native_refusal(&confirmed), "note_checklist_invalid");
    std::fs::write(&input,br#"{"source_kind":"task","source_id":"t","source_version":9,"target_kind":"project_note","target_id":"n","exclude_attachments":true}"#).unwrap();
    let copy = [
        "pm",
        "document",
        "copy",
        "--input",
        path,
        "--id",
        "copy-retry-001",
        "--base-revision",
        "9",
        "--output",
        "json",
    ];
    assert_eq!(native_pm_refusal(&copy), "confirmation_required");
    let mut preview = copy.to_vec();
    preview.push("--dry-run");
    assert!(NATIVE_AUTH_CODES.contains(&native_pm_refusal(&preview).as_str()));
    std::fs::write(&input,br#"{"source_kind":"task","source_id":"t","source_version":9,"target_kind":"project_note","target_id":"n","dry_run":true}"#).unwrap();
    assert_eq!(native_pm_refusal(&preview), "document_request_invalid");
    std::fs::write(&input,serde_json::to_vec(&json!({"record_id":"r","message_count":1,"sources":[{"asset_id":"eml","version":1,"sha256":"a".repeat(64)}]})).unwrap()).unwrap();
    let mail = [
        "pm",
        "mail-thread",
        "ingest",
        "--input",
        path,
        "--id",
        "mail-retry-001",
        "--output",
        "json",
    ];
    assert_eq!(native_pm_refusal(&mail), "confirmation_required");
    let mut preview = mail.to_vec();
    preview.push("--dry-run");
    assert!(NATIVE_AUTH_CODES.contains(&native_pm_refusal(&preview).as_str()));
    std::fs::write(&input,br#"{"record_id":"r","message_count":1,"sources":[],"url":"https://example.invalid/mail.eml"}"#).unwrap();
    assert_eq!(native_pm_refusal(&preview), "document_request_invalid");
    std::fs::remove_dir_all(folder).unwrap();
}





#[test]
fn pm_task_detach_descriptor_confirmation_and_packaging_gate_are_honest() {
    let command = ok(&["capabilities", "pm.task.detach", "--output", "json"])["command"].clone();
    assert_eq!(command["requires"], "server");
    assert_eq!(command["authority"], "headless_project");
    assert_eq!(command["effect"], "global_write");
    assert!(
        command["inputs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["name"] == "expected-version")
    );
    let base = [
        "pm",
        "task",
        "detach",
        "--task",
        "T-1",
        "--attachment",
        "att-1",
        "--expected-version",
        "3",
        "--output",
        "json",
    ];
    assert_eq!(pm_refusal(&base), "confirmation_required");
    let mut invalid = base.to_vec();
    invalid[8] = "0";
    invalid.push("--yes");
    // The development catalogue now admits the compiled profile, so an invalid
    // expected version is refused locally before any credential is read, and
    // a valid confirmed detach stops at the absent native account.
    assert_eq!(native_pm_refusal(&invalid), "attachment_detach_invalid");
    let mut valid = base.to_vec();
    valid.push("--yes");
    let signed_out = native_pm_refusal(&valid);
    assert!(
        NATIVE_AUTH_CODES.contains(&signed_out.as_str()),
        "a valid detach must stop at native auth, got `{signed_out}`"
    );
}

#[test]
fn project_overview_is_discoverable_bounded_and_headless() {
    let command=ok(&["capabilities","pm.overview","--output","json"])["command"].clone();
    assert_eq!(command["effect"],"read_only");assert_eq!(command["authority"],"headless_project");assert_eq!(command["requires"],"server");
    let names:Vec<_>=command["inputs"].as_array().unwrap().iter().map(|i|i["name"].as_str().unwrap()).collect();
    assert_eq!(names,vec!["project","limit","records-cursor","lane"]);
    let search=ok(&["capabilities","--search","project overview","--limit","50","--output","json"]);
    assert!(search["results"].as_array().unwrap().iter().any(|r|r["id"]=="pm.overview"));
    let response=native_pm_refusal(&["pm","overview","--limit","10","--output","json"]);
    assert!(NATIVE_AUTH_CODES.contains(&response.as_str()),"a bounded overview read stops at native auth, got `{response}`");
}

#[test]
fn mv_version_begin_refuses_invalid_source_before_authentication() {
    let root = temp_root("dsgrid-exports-refusals");
    std::fs::create_dir_all(&root).expect("create smoke dir");
    // An MV fence is a revision id or `-`; the shared Rust validation refuses
    // anything else before a credential is read.
    let fenced = native_ds(&[
        "design",
        "version",
        "begin",
        "--project",
        "project-smoke",
        "--kind",
        "mv_model",
        "--object",
        "model-1",
        "--reason",
        "Submitted",
        "--idempotency-key",
        "k-1",
        "--expected-source",
        "Rev ABC",
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(fenced.envelope["error"]["code"], "design_version_refused");
    assert_eq!(
        fenced.envelope["error"]["detail"]["cause"], "invalid_input",
        "{:?}",
        fenced.envelope
    );
    // A batch that names one object twice is refused rather than half-applied.
    let batch = root.join("batch.json");
    std::fs::write(
        &batch,
        serde_json::to_vec(&json!({"reason":"Submission 2","items":[
            {"object":{"kind":"mv_model","id":"model-1"},"idempotency_key":"a"},
            {"object":{"kind":"mv_model","id":"model-1"},"idempotency_key":"b"}]}))
        .unwrap(),
    )
    .unwrap();
    let repeated = native_ds(&[
        "design",
        "version",
        "begin-batch",
        "--project",
        "project-smoke",
        "--file",
        &batch.display().to_string(),
        "--yes",
        "--output",
        "json",
    ]);
    assert_eq!(repeated.envelope["error"]["code"], "design_version_refused");
    std::fs::write(&batch, br#"{"items":[],"extra":1}"#).unwrap();
    assert_eq!(
        native_refusal(&[
            "design",
            "version",
            "begin-batch",
            "--project",
            "project-smoke",
            "--file",
            &batch.display().to_string(),
            "--yes",
            "--output",
            "json",
        ]),
        "invalid_input"
    );
    std::fs::remove_dir_all(&root).ok();
}
