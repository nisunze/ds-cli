//! `ds` records its own failures and refusals without changing them
//! (ds-command-kernel `docs/contracts/ds-cli-reliability-and-feedback.md`
//! §3.4, §7.4, §7.5).
//!
//! Each case runs the real binary against its own state root under Cargo's
//! on-disk test directory and no packaged profile, so every send is
//! unreachable and the event stays in the outbox where it can be read.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use serde_json::Value;

fn state_root(name: &str) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("reliability")
        .join(name);
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("test state root");
    root
}

fn ds(state: &Path, reporting: &str, extra_env: &[(&str, &str)], args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ds"));
    command
        .args(args)
        .env("XDG_STATE_HOME", state)
        .env("DS_SRE_REPORTING", reporting)
        .env_remove("DS_NATIVE_CLIENT_PROFILE_BUNDLE")
        .env_remove("DS_CLI_SURFACE")
        .env_remove("DS_CLI_AGENT");
    for (name, value) in extra_env {
        command.env(name, value);
    }
    command.output().expect("ds runs")
}

fn stored(state: &Path) -> (String, Value) {
    let text = fs::read_to_string(state.join("ds").join("reliability.json"))
        .expect("the reporter state document");
    let document = serde_json::from_str(&text).expect("the state is JSON");
    (text, document)
}

const REFUSED: &[&str] = &[
    "feedback",
    "submit",
    "--title",
    "SENTINEL-TITLE-4711",
    "--detail",
    "SENTINEL detail at /home/sentinel-user/private/parcels.geojson",
    "--component",
    "ds-cli",
    "--agent",
    "sentinel-agent",
    "--output",
    "json",
];

#[test]
fn cargo_runs_every_test_with_reporting_off() {
    assert_eq!(
        std::env::var("DS_SRE_REPORTING").as_deref(),
        Ok("off"),
        "ds-cli/.cargo/config.toml keeps test runs out of SRE"
    );
}

/// §7.5 and §3.4: the answer, its stream bytes and its exit code are
/// identical with reporting on and off; off creates nothing; on, the refusal
/// is one recorded `refused` event that does not count as a failure.
#[test]
fn a_refusal_is_recorded_without_changing_the_answer() {
    let off = state_root("refusal-off");
    let on = state_root("refusal-on");
    let quiet = ds(&off, "off", &[], REFUSED);
    let reported = ds(&on, "on", &[], REFUSED);
    assert_eq!(
        quiet.status.code(),
        Some(2),
        "confirmation_required is invalid input"
    );
    assert_eq!(reported.status.code(), quiet.status.code());
    assert_eq!(reported.stdout, quiet.stdout);
    assert_eq!(reported.stderr, quiet.stderr);
    assert!(
        !off.join("ds").exists(),
        "reporting off creates no state at all"
    );

    let (_, document) = stored(&on);
    let outbox = document["outbox"].as_array().expect("outbox");
    assert_eq!(outbox.len(), 1, "one event for one invocation");
    let event = &outbox[0]["event"];
    assert_eq!(event["source"], "cli_command");
    assert_eq!(event["platform"], "cli");
    assert_eq!(event["action"], "feedback.submit");
    assert_eq!(event["result_class"], "refused");
    assert_eq!(event["outcome"], "success");
    assert_eq!(event["error_code"], "confirmation_required");
    assert_eq!(
        event["lane"], "local",
        "a development build reports on lane local"
    );
    assert_eq!(
        event["flags"],
        serde_json::json!(["--title", "--detail", "--component", "--agent", "--output"])
    );
    assert!(
        event["user_action_id"]
            .as_str()
            .unwrap()
            .starts_with("inv_")
    );
    assert_eq!(
        document["last_flush"], "offline",
        "the send was tried and kept"
    );
}

/// §7.4: sentinel values in every argument appear nowhere in what is stored
/// or would be sent.
#[test]
fn no_argument_value_reaches_the_outbox() {
    let on = state_root("privacy");
    let output = ds(&on, "on", &[], REFUSED);
    assert_eq!(output.status.code(), Some(2));
    let (text, _) = stored(&on);
    for sentinel in [
        "SENTINEL",
        "sentinel",
        "4711",
        "parcels.geojson",
        "/home/",
        "private",
    ] {
        assert!(!text.contains(sentinel), "`{sentinel}` was stored: {text}");
    }
}

/// A tool call's process records its event as `mcp` and leaves the send to the
/// adapter's long-lived process, off the request path.
#[test]
fn a_tool_call_process_records_and_never_sends() {
    let on = state_root("adapter");
    let output = ds(
        &on,
        "on",
        &[("DS_CLI_SURFACE", "mcp"), ("DS_CLI_AGENT", "claude-code")],
        REFUSED,
    );
    assert_eq!(output.status.code(), Some(2));
    let (_, document) = stored(&on);
    let event = &document["outbox"][0]["event"];
    assert_eq!(event["platform"], "mcp");
    assert_eq!(event["agent"], "claude-code");
    assert!(
        document["last_flush"].is_null(),
        "no send was attempted from the tool call"
    );
}

/// `ds doctor` shows the switch, the outbox and the last flush, and probes no
/// network to do it.
#[test]
fn doctor_shows_the_reporting_state() {
    let on = state_root("doctor");
    ds(&on, "on", &[], REFUSED);
    let doctor = ds(&on, "on", &[], &["doctor", "--output", "json"]);
    assert!(doctor.status.success());
    let answer: Value = serde_json::from_slice(&doctor.stdout).expect("doctor JSON");
    let reporting = &answer["data"]["reporting"];
    assert_eq!(reporting["enabled"], true);
    assert_eq!(reporting["state"], "ready");
    assert_eq!(reporting["outbox_events"], 1);
    assert_eq!(reporting["last_flush"], "offline");
    assert_eq!(reporting["lane"], "local");

    let quiet = state_root("doctor-off");
    let off = ds(&quiet, "off", &[], &["doctor", "--output", "json"]);
    let answer: Value = serde_json::from_slice(&off.stdout).expect("doctor JSON");
    assert_eq!(answer["data"]["reporting"]["enabled"], false);
    assert!(!quiet.join("ds").exists());
}

/// A successful command is counted into the day's rollup, never sent per call.
#[test]
fn an_answer_is_counted_never_sent() {
    let on = state_root("rollup");
    let output = ds(&on, "on", &[], &["capabilities", "--output", "json"]);
    assert!(output.status.success());
    let (_, document) = stored(&on);
    assert_eq!(document["outbox"].as_array().map_or(0, Vec::len), 0);
    assert_eq!(document["rollup"]["capabilities"], 1);
}

/// §7.3 with the development-only panic hook: a crash is one `crashed` event
/// and one automatic `bug` report waiting for a signed-in send (this test has
/// no packaged profile, so it waits); a second crash raises that report's
/// count and queues nothing new. The panic still ends the process as before.
#[test]
fn a_crash_is_one_event_and_one_automatic_report_and_a_repeat_raises_its_count() {
    let on = state_root("crash");
    let crash = &[("DS_CLI_TEST_PANIC", "feedback.list")];
    let args = &["feedback", "list", "--output", "json"];
    let first = ds(&on, "on", crash, args);
    let quiet = ds(&state_root("crash-off"), "off", crash, args);
    assert_eq!(first.status.code(), Some(101), "a panic exits as a panic");
    assert_eq!(first.status.code(), quiet.status.code());
    assert_eq!(first.stdout, quiet.stdout);

    let (text, document) = stored(&on);
    let event = &document["outbox"][0]["event"];
    assert_eq!(event["action"], "feedback.list");
    assert_eq!(event["result_class"], "crashed");
    assert_eq!(event["outcome"], "failure");
    let automatic = document["automatic"].as_array().expect("automatic reports");
    assert_eq!(automatic.len(), 1, "one report for one crash");
    let report = &automatic[0]["payload"];
    assert_eq!(report["agent"], "ds (automatic)");
    assert_eq!(report["kind"], "bug");
    assert_eq!(report["title"], "ds feedback.list crashed: panic");
    assert_eq!(report["context"]["sre_event"], event["user_action_id"]);
    assert!(
        !automatic[0]
            .to_string()
            .contains("asked feedback.list to crash"),
        "the panic message is never in a report: {text}"
    );

    ds(&on, "on", crash, args);
    let (_, document) = stored(&on);
    let automatic = document["automatic"].as_array().expect("automatic reports");
    assert_eq!(automatic.len(), 1, "a repeat files nothing new");
    assert_eq!(automatic[0]["occurrences"], 2);
}

/// §5.4: a failed envelope names `ds feedback draft --last` as its last next
/// command, the same with reporting on or off, in JSON and human output; the
/// SRE event id is in the journal the draft reads. A refusal names no draft.
#[test]
fn a_failure_envelope_points_to_its_feedback_draft() {
    let draft = serde_json::json!("ds feedback draft --last");
    let on = state_root("pointer");
    let failed = ds(&on, "on", &[], &["feedback", "list", "--output", "json"]);
    assert_eq!(
        failed.status.code(),
        Some(3),
        "no packaged profile is unavailable"
    );
    let answer: Value = serde_json::from_slice(&failed.stdout).expect("failure JSON");
    let next = answer["error"]["next"].as_array().expect("next commands");
    assert_eq!(next.last(), Some(&draft), "{answer}");
    let quiet = ds(
        &state_root("pointer-off"),
        "off",
        &[],
        &["feedback", "list", "--output", "json"],
    );
    assert_eq!(
        quiet.stdout, failed.stdout,
        "the answer never depends on the switch"
    );

    let human = ds(
        &state_root("pointer-human"),
        "off",
        &[],
        &["feedback", "list"],
    );
    let stderr = String::from_utf8_lossy(&human.stderr);
    assert!(
        stderr.contains("  next: ds feedback draft --last"),
        "{stderr}"
    );

    let refused = ds(&on, "on", &[], REFUSED);
    let answer: Value = serde_json::from_slice(&refused.stdout).expect("refusal JSON");
    let next = answer["error"]["next"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    assert!(!next.contains(&draft), "{answer}");
}
