//! End-to-end invocation: exit codes, envelope stability, build identity, and
//! the vertical command against real domain data.
//!
//! The semantic assertions here bind to the actual `.dsgrid` fixture in
//! `ds-network`, not to a mock. A mock would prove only that this crate's
//! adapter talks to itself; the point of the vertical slice is that `ds`
//! reaches the authoritative engine and reports what the engine says.

use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ds_cli_contract::{ExitClass, Failure};
use serde_json::Value;

mod common;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn temp_file(label: &str) -> std::path::PathBuf {
    let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "ds-cli-{label}-{}-{sequence}.json",
        std::process::id()
    ))
}

struct Run {
    envelope: Value,
    stdout: String,
    stderr: String,
    code: i32,
}

fn ds(args: &[&str]) -> Run {
    let output = Command::new(env!("CARGO_BIN_EXE_ds"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .expect("ds binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    Run {
        envelope: serde_json::from_str(&stdout).unwrap_or(Value::Null),
        stdout,
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        code: output.status.code().unwrap_or(-1),
    }
}

// ---------------------------------------------------------------------------
// Exit-code mapping
// ---------------------------------------------------------------------------

#[test]
fn exit_codes_map_to_their_classes() {
    let model = common::fixture();
    let cases: &[(&str, i32, &[&str])] = &[
        ("success", 0, &["dsgrid", "inspect", "--model", "MODEL"]),
        ("unknown domain", 2, &["nosuchdomain"]),
        ("unknown command", 2, &["dsgrid", "nosuchcommand"]),
        ("unknown flag", 2, &["dsgrid", "inspect", "--nope", "x"]),
        ("missing required input", 2, &["dsgrid", "inspect"]),
        ("missing value", 2, &["dsgrid", "inspect", "--model"]),
        (
            "invalid choice",
            2,
            &["dsgrid", "inspect", "--model", "MODEL", "--include", "nope"],
        ),
        (
            "file not found",
            2,
            &["dsgrid", "inspect", "--model", "/no/such/file.dsgrid"],
        ),
    ];

    for (label, expected, args) in cases {
        let args: Vec<String> = args
            .iter()
            .map(|arg| {
                if *arg == "MODEL" {
                    model.clone()
                } else {
                    (*arg).to_string()
                }
            })
            .chain(["--output".into(), "json".into()])
            .collect();
        let borrowed: Vec<&str> = args.iter().map(String::as_str).collect();
        let run = ds(&borrowed);
        assert_eq!(
            run.code, *expected,
            "{label}: expected exit {expected}, got {}: {}{}",
            run.code, run.stdout, run.stderr
        );
    }
}

#[test]
fn exit_code_and_envelope_class_always_agree() {
    // A caller branching on the exit code and a caller branching on
    // `error.class` must never reach different conclusions.
    let cases: &[&[&str]] = &[
        &["nosuchdomain"],
        &["dsgrid", "inspect"],
        &["dsgrid", "inspect", "--model", "/no/such/file.dsgrid"],
        &["capabilities", "nosuchselector"],
    ];
    let expected = [
        (2, "invalid_input"),
        (3, "unavailable"),
        (4, "unauthorized"),
        (5, "conflict"),
        (6, "failed"),
        (1, "internal"),
    ];

    for args in cases {
        let mut args = args.to_vec();
        args.extend(["--output", "json"]);
        let run = ds(&args);
        let class = run.envelope["error"]["class"].as_str().unwrap_or("");
        let mapped = expected
            .iter()
            .find(|(_, token)| *token == class)
            .unwrap_or_else(|| panic!("unrecognized error class `{class}` in {}", run.stdout));
        assert_eq!(
            run.code,
            mapped.0,
            "`ds {}` reported class `{class}` but exited {}",
            args.join(" "),
            run.code
        );
    }
}

#[test]
fn every_failure_class_has_its_exact_exit_and_envelope_token() {
    // Integration calls above exercise real invalid-input refusals. The other
    // classes require intentionally paired/authenticated or failing owners,
    // so pin the complete shared envelope contract directly instead of
    // pretending one parser refusal covers all six.
    for (class, code, token) in [
        (ExitClass::InvalidInput, 2, "invalid_input"),
        (ExitClass::Unavailable, 3, "unavailable"),
        (ExitClass::Unauthorized, 4, "unauthorized"),
        (ExitClass::Conflict, 5, "conflict"),
        (ExitClass::Failed, 6, "failed"),
        (ExitClass::Internal, 1, "internal"),
    ] {
        let refusal = Failure::new(class, "test_refusal", "test refusal");
        assert_eq!(refusal.class(), class);
        assert_eq!(refusal.class().token(), token);
        assert_eq!(ds_cli_contract::output::exit_code(refusal.class()), code);
    }
}

// ---------------------------------------------------------------------------
// Envelope stability
// ---------------------------------------------------------------------------

#[test]
fn success_envelope_is_stable() {
    let model = common::fixture();
    let run = ds(&["dsgrid", "inspect", "--model", &model, "--output", "json"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);

    assert_eq!(run.envelope["v"], 1, "envelope version changed");
    assert_eq!(run.envelope["status"], "ok");
    assert_eq!(run.envelope["command"], "dsgrid.inspect");
    assert_eq!(run.envelope["contract"], 1);
    assert!(run.envelope["data"].is_object());
    // `more` is absent when there is nothing more to say. Absence is the
    // signal; an empty object would be a value a caller has to interpret.
    assert!(run.envelope.get("more").is_none());
}

#[test]
fn error_envelope_is_stable() {
    let run = ds(&[
        "dsgrid",
        "inspect",
        "--model",
        "/no/such/file.dsgrid",
        "--output",
        "json",
    ]);
    assert_eq!(run.envelope["v"], 1);
    assert_eq!(run.envelope["status"], "error");
    assert_eq!(run.envelope["command"], "dsgrid.inspect");
    let error = &run.envelope["error"];
    assert_eq!(error["class"], "invalid_input");
    assert_eq!(error["code"], "model_not_found");
    assert_eq!(error["retryable"], false);
    assert!(
        error["remedy"].is_string(),
        "a refusal without a remedy is a dead end"
    );
}

#[test]
fn machine_output_is_stdout_and_diagnostics_are_stderr() {
    let model = common::fixture();
    let run = ds(&["dsgrid", "inspect", "--model", &model, "--output", "json"]);
    assert!(
        run.stderr.is_empty(),
        "success wrote to stderr: {}",
        run.stderr
    );
    serde_json::from_str::<Value>(&run.stdout).expect("stdout is exactly one JSON document");

    // A refusal in JSON mode is a result a caller must parse, so it goes to
    // stdout too — stdout stays parseable in every outcome.
    let failed = ds(&["dsgrid", "inspect", "--model", "/nope", "--output", "json"]);
    serde_json::from_str::<Value>(&failed.stdout).expect("refusal is on stdout as JSON");

    // In human mode the same refusal goes to stderr, so a person piping
    // stdout gets only answers.
    let human = ds(&["dsgrid", "inspect", "--model", "/nope"]);
    assert!(
        human.stdout.is_empty(),
        "human-mode refusal polluted stdout"
    );
    assert!(!human.stderr.is_empty(), "human-mode refusal said nothing");
}

// ---------------------------------------------------------------------------
// Build identity
// ---------------------------------------------------------------------------

#[test]
fn build_identity_is_verifiable() {
    // Packaging runs exactly this and asserts the source SHA against its pin.
    let run = ds(&["version", "--output", "json"]);
    assert_eq!(run.code, 0);
    let data = &run.envelope["data"];
    assert_eq!(data["product"], "ds");
    assert!(
        data["version"]
            .as_str()
            .expect("version")
            .starts_with(char::is_numeric)
    );
    assert!(data["target"].as_str().expect("target").contains('-'));
    assert!(
        data["dirty"].is_boolean(),
        "dirty state must be stated, not omitted"
    );
    assert_eq!(data["envelope"], 1);

    let sha = data["source_sha"].as_str().expect("source_sha");
    assert!(
        sha == "unknown" || sha.len() == 40,
        "source_sha is neither a git SHA nor an honest `unknown`: {sha}"
    );

    // `--version` and `ds version` are the same fact.
    let flag = ds(&["--version", "--output", "json"]);
    assert_eq!(flag.envelope["data"], *data);
}

// ---------------------------------------------------------------------------
// The vertical command, against real domain data
// ---------------------------------------------------------------------------

#[test]
fn inspect_reports_the_engine_s_own_identity() {
    let model = common::fixture();
    let run = ds(&["dsgrid", "inspect", "--model", &model, "--output", "json"]);
    assert_eq!(run.code, 0, "{}{}", run.stdout, run.stderr);
    let data = &run.envelope["data"];

    // These are the fixture's real values, produced by ds-grid-exchange. They
    // are asserted verbatim so a change in the engine's answer is a visible
    // test failure rather than a silent change in what `ds` reports.
    assert_eq!(data["format"], "dsgrid");
    assert_eq!(data["model"]["crs"], "EPSG:32735");
    assert_eq!(data["model"]["schema_version"], 1);
    assert_eq!(data["model"]["format_version"], 1);
    assert_eq!(data["model"]["fingerprint"], "fnv1a64:6f9e4a421fccf238");
    assert_eq!(data["model"]["id"], "pls-import-fnv1a64:6f9e4a42");

    // The default answer decodes nothing. This is the cost contract, not a
    // detail: it is why calling inspect first is cheap.
    assert_eq!(data["decoded"], false);
    assert!(
        data.get("tables").is_none(),
        "default answer carried a projection"
    );
    assert!(
        data.get("library").is_none(),
        "default answer carried a projection"
    );
}

#[test]
fn projections_are_opt_in_and_report_their_cost() {
    let model = common::fixture();

    let manifest_only = ds(&[
        "dsgrid",
        "inspect",
        "--model",
        &model,
        "--include",
        "tables",
        "--output",
        "json",
    ]);
    assert_eq!(manifest_only.code, 0);
    assert_eq!(
        manifest_only.envelope["data"]["decoded"], false,
        "the tables projection is answerable from the manifest and must not decode"
    );
    assert!(manifest_only.envelope["data"]["tables"].is_object());

    let decoded = ds(&[
        "dsgrid",
        "inspect",
        "--model",
        &model,
        "--include",
        "library",
        "--output",
        "json",
    ]);
    assert_eq!(decoded.code, 0);
    assert_eq!(
        decoded.envelope["data"]["decoded"], true,
        "the library projection needs the tables and must say so"
    );
    let library = &decoded.envelope["data"]["library"];
    assert!(
        !library["structure_types"]
            .as_array()
            .expect("structure types")
            .is_empty()
    );
    assert!(!library["cables"].as_array().expect("cables").is_empty());
}

#[test]
fn truncation_is_always_visible() {
    // A silently shortened list reads as a complete one. Ask for a limit
    // below the real count and the response must say what it withheld.
    let model = common::fixture();
    let run = ds(&[
        "dsgrid",
        "inspect",
        "--model",
        &model,
        "--include",
        "tables",
        "--limit",
        "2",
        "--output",
        "json",
    ]);
    assert_eq!(run.code, 0);
    let tables = run.envelope["data"]["tables"].as_object().expect("tables");
    assert_eq!(tables.len(), 2, "limit was not applied");

    let truncated = run.envelope["data"]["more"]["truncated"]
        .as_array()
        .expect("truncation must be reported");
    let entry = truncated.iter().find(|entry| entry["field"] == "tables");
    let entry = entry.expect("the truncated tables projection must be named");
    assert!(entry["withheld"].as_u64().expect("withheld count") > 0);
}

#[test]
fn continuation_names_what_else_exists() {
    // The response tells a caller what it did not ask for, so discovering the
    // rest never costs another round-trip through help.
    let model = common::fixture();
    let run = ds(&["dsgrid", "inspect", "--model", &model, "--output", "json"]);
    let available = run.envelope["data"]["more"]["available_projections"]
        .as_array()
        .expect("unrequested projections are named");
    let names: Vec<&str> = available.iter().filter_map(Value::as_str).collect();
    assert!(names.contains(&"tables"));
    assert!(names.contains(&"library"));
}

#[test]
fn bounds_are_enforced_not_merely_documented() {
    let model = common::fixture();
    for (limit, code) in [("0", 2), ("abc", 2), ("999999", 2)] {
        let run = ds(&[
            "dsgrid", "inspect", "--model", &model, "--limit", limit, "--output", "json",
        ]);
        assert_eq!(
            run.code, code,
            "--limit {limit} was not rejected: {}",
            run.stdout
        );
        assert_eq!(run.envelope["error"]["code"], "invalid_limit");
    }
}

// ---------------------------------------------------------------------------
// Refusal behaviour
// ---------------------------------------------------------------------------

#[test]
fn unavailable_refusals_carry_a_remedy_and_a_next_step() {
    // `desktop status` reaches the real world. Whatever this machine's state,
    // the answer must be actionable: either a report, or a refusal naming a
    // remedy.
    let run = ds(&["desktop", "status", "--output", "json"]);

    // Checked in every state, not only the happy one. A credential is most
    // likely to escape through an error path, which is exactly where an
    // upstream message gets echoed.
    for needle in ["token", "Bearer", "authorization", "refresh"] {
        assert!(
            !run.stdout.contains(needle),
            "`desktop status` output contains `{needle}`: {}",
            run.stdout
        );
        assert!(
            !run.stderr.contains(needle),
            "`desktop status` diagnostics contain `{needle}`: {}",
            run.stderr
        );
    }

    if run.envelope["status"] == "ok" {
        let data = &run.envelope["data"];
        assert!(data["paired"].is_boolean());
        assert!(data["signed_in"].is_boolean());
        return;
    }
    let error = &run.envelope["error"];
    assert_eq!(error["class"], "unavailable");
    assert!(
        error["remedy"].is_string(),
        "an unavailable refusal must say what would fix it"
    );
    assert_eq!(
        error["retryable"], true,
        "unavailable is retryable once the world changes"
    );
}

#[test]
fn an_explicit_stale_descriptor_never_leaks_its_secret() {
    // Point `ds` at a descriptor whose port nothing is listening on. The
    // upstream transport error is echoed into `detail`, so this is the path
    // where a naive implementation would carry a URL, a header, or whatever
    // the HTTP client happened to include.
    let descriptor = temp_file("test-stale-bridge");
    std::fs::write(
        &descriptor,
        br#"{"version":1,"url":"http://127.0.0.1:1","token":"s3cr3t-pairing-value","pid":1}"#,
    )
    .expect("write descriptor");

    let run = ds(&[
        "desktop",
        "status",
        "--desktop-descriptor",
        descriptor.to_str().expect("path"),
        "--output",
        "json",
    ]);
    let _ = std::fs::remove_file(&descriptor);

    assert_eq!(
        run.code, 3,
        "a stale descriptor is an unavailable, not a crash"
    );
    assert_eq!(run.envelope["error"]["code"], "desktop_unreachable");
    assert!(
        !run.stdout.contains("s3cr3t-pairing-value")
            && !run.stderr.contains("s3cr3t-pairing-value"),
        "the pairing secret reached the output"
    );
}

#[test]
fn near_misses_are_suggested() {
    // An agent that guessed a name should be corrected, not sent back to
    // help. This is the cheapest possible recovery.
    let run = ds(&["dsgrd", "--output", "json"]);
    assert_eq!(run.envelope["error"]["code"], "unknown_domain");
    assert!(
        run.envelope["error"]["remedy"]
            .as_str()
            .expect("remedy")
            .contains("dsgrid"),
        "no suggestion offered for a one-character typo"
    );

    let run = ds(&["dsgrid", "inspct", "--output", "json"]);
    assert_eq!(run.envelope["error"]["code"], "unknown_command");
    assert!(
        run.envelope["error"]["remedy"]
            .as_str()
            .expect("remedy")
            .contains("inspect")
    );
}

// ---------------------------------------------------------------------------
// Help routing
// ---------------------------------------------------------------------------

#[test]
fn help_is_reachable_by_every_spelling_an_agent_will_try() {
    let spellings: &[&[&str]] = &[
        &["--help"],
        &["help"],
        &["dsgrid", "--help"],
        &["help", "dsgrid"],
        &["dsgrid", "inspect", "--help"],
        &["help", "dsgrid", "inspect"],
        &["dsgrid"],
        &[],
    ];
    for args in spellings {
        let run = ds(args);
        assert_eq!(
            run.code,
            0,
            "`ds {}` did not answer: {}",
            args.join(" "),
            run.stderr
        );
        assert!(
            !run.stdout.is_empty(),
            "`ds {}` printed nothing",
            args.join(" ")
        );
    }
}

#[test]
fn command_help_in_json_is_the_machine_descriptor() {
    // A caller wanting a schema must never have to parse a help screen.
    let run = ds(&["dsgrid", "inspect", "--help", "--output", "json"]);
    assert_eq!(run.code, 0);
    assert_eq!(run.envelope["status"], "ok");
    assert_eq!(run.envelope["data"]["id"], "dsgrid.inspect");
    assert!(run.envelope["data"]["inputs"].is_array());
    assert!(run.envelope["data"]["refusals"].is_array());
}

#[test]
fn output_format_is_validated() {
    let run = ds(&["--output", "yaml", "dsgrid", "inspect"]);
    assert_eq!(run.code, 2);
    assert!(run.stderr.contains("human") && run.stderr.contains("json"));
}

#[test]
fn output_is_byte_identical_across_runs() {
    // Determinism is part of the contract: a caller may hash a result, diff
    // two runs, or cache on the bytes. Map keys are ordered because
    // `serde_json` uses a sorted map by default, and every collection this
    // CLI emits is either in the domain's canonical order or lexical.
    let model = common::fixture();
    let args = [
        "dsgrid",
        "inspect",
        "--model",
        &model,
        "--include",
        "tables",
        "--include",
        "library",
        "--output",
        "json",
    ];
    let first = ds(&args);
    let second = ds(&args);
    assert_eq!(first.code, 0, "{}{}", first.stdout, first.stderr);
    assert_eq!(
        first.stdout, second.stdout,
        "two runs over the same input produced different bytes"
    );
}

#[test]
fn confirmation_policy_is_enforced_for_every_effectful_command() {
    // No command that writes a durable artifact or mutates shared state may
    // run without `--yes`. The check lives in dispatch, in one place, so this
    // holds for commands that do not exist yet — it walks the live surface
    // rather than a list someone maintains.
    let (index, code) = ds_json(&["capabilities", "--output", "json"]);
    assert_eq!(code, 0);

    let mut checked = 0;
    for domain in index["data"]["domains"].as_array().expect("domains") {
        let id = domain["id"].as_str().expect("domain id");
        let (commands, _) = ds_json(&["capabilities", id, "--output", "json"]);
        for command in commands["data"]["commands"].as_array().expect("commands") {
            let command_id = command["id"].as_str().expect("id");
            let (descriptor, _) = ds_json(&["capabilities", command_id, "--output", "json"]);
            let descriptor = &descriptor["data"]["command"];
            if !descriptor["confirmation_required"]
                .as_bool()
                .unwrap_or(false)
            {
                continue;
            }
            checked += 1;

            // Invoke with no inputs at all and no `--yes`. Whatever else is
            // wrong with the call, it must never be allowed to take effect.
            let path: Vec<&str> = descriptor["path"]
                .as_array()
                .expect("path")
                .iter()
                .map(|part| part.as_str().expect("part"))
                .collect();
            let mut args = path.clone();
            args.extend(["--output", "json"]);
            let run = ds(&args);
            assert_ne!(
                run.code, 0,
                "`{command_id}` requires confirmation but ran without --yes"
            );
        }
    }

    // This assertion used to read `checked == 0`, with a note that it would
    // start protecting the invariant the moment the first effectful command
    // landed. `ds map design save` is that command — it pushes a
    // transformer's staged edits to the project — so the tripwire has been
    // turned around: the claim now is that the gate was actually exercised,
    // and a surface that loses its last effectful command fails here rather
    // than passing vacuously.
    assert!(
        checked > 0,
        "no command declares an effect that needs confirmation, so the \
         dispatch gate went untested"
    );
}

fn ds_json(args: &[&str]) -> (Value, i32) {
    let run = ds(args);
    (run.envelope, run.code)
}

#[test]
fn an_environment_descriptor_is_used_and_the_flag_still_wins() {
    // `DS_DESKTOP_DESCRIPTOR` is how a terminal opened by the desktop's own
    // `cl` launcher stays pinned to the window that opened it. It is a
    // default, not an override: the flag names a descriptor verbatim.
    let stale = temp_file("test-env-bridge");
    std::fs::write(
        &stale,
        br#"{"version":1,"url":"http://127.0.0.1:1","token":"env-s3cr3t-value","pid":1}"#,
    )
    .expect("write descriptor");
    let missing = temp_file("test-env-missing");
    let _ = std::fs::remove_file(&missing);

    let run = |args: &[&str]| {
        let output = Command::new(env!("CARGO_BIN_EXE_ds"))
            .args(["desktop", "status", "--output", "json"])
            .args(args)
            .env("NO_COLOR", "1")
            .env("DS_DESKTOP_DESCRIPTOR", &stale)
            .output()
            .expect("ds runs");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        Run {
            envelope: serde_json::from_str(&stdout).unwrap_or(Value::Null),
            stdout,
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            code: output.status.code().unwrap_or(-1),
        }
    };

    let by_environment = run(&[]);
    assert_eq!(
        by_environment.code, 3,
        "the environment descriptor was used: {}",
        by_environment.stdout
    );
    assert_eq!(
        by_environment.envelope["error"]["code"],
        "desktop_unreachable"
    );
    assert!(
        !by_environment.stdout.contains("env-s3cr3t-value")
            && !by_environment.stderr.contains("env-s3cr3t-value"),
        "the pairing secret reached the output"
    );

    let by_flag = run(&["--desktop-descriptor", missing.to_str().expect("path")]);
    assert_eq!(
        by_flag.envelope["error"]["code"], "descriptor_unusable",
        "the flag names a descriptor verbatim, even over a usable environment one"
    );
    let _ = std::fs::remove_file(&stale);
}
