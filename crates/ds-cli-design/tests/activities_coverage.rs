//! `ds design activities read` states its own coverage.
//!
//! The store is a capture directory on ONE machine. A read that answers "14
//! projects" for an account that can see 63 is telling the truth about the
//! store and nothing about the estate — and an honesty statement that names
//! membership and refusals as the bound, when neither is, turns that into the
//! confident-empty answer. The sweep retains the visible directory precisely
//! so a read can say "14 of 63"; these tests hold the read to reading it.

use std::path::{Path, PathBuf};

use ds_cli_design::activities::read::{Query, answer};
use ds_cli_design::activities::{DIRECTORY_SCHEMA, activities_call, store_call};
use serde_json::{Map, Value, json};

/// A fresh, empty account folder under the OS temp root.
fn fresh_account(tag: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let account = std::env::temp_dir().join(format!(
        "ds-activities-coverage-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&account).expect("account folder");
    account
}

fn retain_directory(account: &Path, projects: &[&str]) {
    let listed: Vec<Value> = projects
        .iter()
        .map(|id| json!({ "ds_project": id, "status": "active", "display_name": id }))
        .collect();
    std::fs::write(
        account.join("directory.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": DIRECTORY_SCHEMA,
            "lane": "stable",
            "captured_at_ms": 1_758_000_000_000i64,
            "captured_by": "uid",
            "projects": listed,
        }))
        .expect("json"),
    )
    .expect("directory retained");
}

/// One admissible capture of `project`, built the way the sweep builds it:
/// the kernel folds the ledger, digests it, and admits the envelope.
fn retain_capture(account: &Path, project: &str, captured_at_ms: i64) {
    let mut ledger_request = Map::new();
    ledger_request.insert("ds_project".into(), json!(project));
    ledger_request.insert("captured_at_ms".into(), json!(captured_at_ms));
    ledger_request.insert("rows".into(), json!([]));
    ledger_request.insert("now_ms".into(), json!(captured_at_ms));
    let ledger = activities_call("ledger", ledger_request).expect("ledger")["ledger"].clone();
    let mut digest_request = Map::new();
    digest_request.insert("ledger".into(), ledger.clone());
    let digest = store_call("digest", digest_request).expect("digest")["digest"].clone();
    let snapshot = json!({
        "schema": ds_command_kernel::design_activities_store::SNAPSHOT_SCHEMA,
        "ds_project": project,
        "display_name": project,
        "status": "active",
        "lane": "stable",
        "captured_at_ms": captured_at_ms,
        "captured_by": "uid",
        "source": { "rows": 0, "fast_lane": false, "diagnostics_source": "none" },
        "ledger": ledger,
        "digest": digest,
    });
    let mut validate = Map::new();
    validate.insert("snapshot".into(), snapshot.clone());
    store_call("validate", validate).expect("the fixture capture is admissible");
    let directory = account.join(project);
    std::fs::create_dir_all(&directory).expect("project folder");
    std::fs::write(
        directory.join(format!("{captured_at_ms:013}.json")),
        serde_json::to_vec_pretty(&snapshot).expect("json"),
    )
    .expect("capture retained");
}

fn read(account: &Path, named: &[&str]) -> Result<Value, ds_cli_contract::outcome::Failure> {
    let named: Vec<String> = named.iter().map(|name| (*name).to_owned()).collect();
    answer(
        account,
        &Query {
            lane: "stable",
            bucket: "active",
            limit: 50,
            tz_offset: 0,
            since: None,
            named: &named,
            user: "",
        },
    )
}

#[test]
fn the_answer_states_captured_against_visible_from_the_retained_directory() {
    let account = fresh_account("coverage");
    retain_directory(&account, &["p_one", "p_two", "p_three"]);
    retain_capture(&account, "p_one", 1_758_000_000_000);

    let data = read(&account, &[]).expect("one capture answers");
    assert_eq!(data["captured"]["project_count"], 1, "{data}");
    assert_eq!(data["coverage"]["visible"], 3, "{data}");
    assert_eq!(data["coverage"]["captured"], 1, "{data}");
    assert_eq!(
        data["coverage"]["never_captured"],
        json!(["p_two", "p_three"]),
        "{data}"
    );
    assert_eq!(data["coverage"]["more_never_captured"], false);

    // The honesty statement names the TRUE bound with the numbers, and keeps
    // the membership and refusal sentences, which are true.
    let not_claimed = data["not_claimed"].as_array().expect("not_claimed");
    let bound = not_claimed
        .iter()
        .filter_map(Value::as_str)
        .find(|line| line.contains("1 of 3 visible projects have been captured on this machine"))
        .unwrap_or_else(|| panic!("no coverage line in {not_claimed:?}"));
    assert!(
        bound.contains("ds design activities sweep --yes"),
        "{bound}"
    );
    assert!(
        bound.contains("Only projects this account is a member of"),
        "{bound}"
    );
    assert!(
        bound.contains("recorded as a refusal, not as an absence"),
        "{bound}"
    );

    let _ = std::fs::remove_dir_all(&account);
}

#[test]
fn a_store_without_a_directory_says_coverage_is_unknown_rather_than_a_number() {
    let account = fresh_account("no-directory");
    retain_capture(&account, "p_one", 1_758_000_000_000);

    let data = read(&account, &[]).expect("one capture answers");
    assert_eq!(data["coverage"]["visible"], Value::Null, "{data}");
    assert_eq!(data["coverage"]["never_captured"], Value::Null, "{data}");
    assert_eq!(data["coverage"]["captured"], 1, "{data}");
    assert!(
        data["coverage"]["unknown"]
            .as_str()
            .is_some_and(|reason| reason.contains("no project directory is retained")),
        "{data}"
    );
    let lines = data["not_claimed"].to_string();
    assert!(lines.contains("is unknown"), "{lines}");
    assert!(!lines.contains("of null visible"), "{lines}");

    let _ = std::fs::remove_dir_all(&account);
}

#[test]
fn a_named_project_the_directory_does_not_carry_is_refused_as_not_visible() {
    let account = fresh_account("not-visible");
    retain_directory(&account, &["p_seen"]);

    // Fabricated: no directory entry, no capture. `store_empty` for it would
    // send the operator to a sweep that can never succeed.
    let failure = read(&account, &["p_made_up"]).expect_err("an invisible project is refused");
    assert_eq!(
        failure.code(),
        "project_not_visible",
        "{}",
        failure.message()
    );
    assert!(
        failure.message().contains("p_made_up"),
        "{}",
        failure.message()
    );
    assert!(
        failure.message().contains("1 visible projects"),
        "{}",
        failure.message()
    );
    assert!(
        failure
            .remedy_text()
            .is_some_and(|remedy| remedy.contains("ds auth project list")),
        "{:?}",
        failure.remedy_text()
    );

    // Visible but never captured: THAT is `store_empty`, and its sweep remedy
    // can succeed — and the refusal says how much of the estate is held.
    let failure = read(&account, &["p_seen"]).expect_err("an uncaptured project is empty");
    assert_eq!(failure.code(), "store_empty", "{}", failure.message());
    assert!(
        failure.message().contains("0 of 1 visible projects"),
        "{}",
        failure.message()
    );

    let _ = std::fs::remove_dir_all(&account);
}

#[test]
fn a_captured_project_the_account_has_since_lost_still_answers() {
    // The directory is rewritten by every sweep; a project captured before
    // access was lost is still evidence, and is not refused as invisible.
    let account = fresh_account("lost");
    retain_directory(&account, &["p_other"]);
    retain_capture(&account, "p_lost", 1_758_000_000_000);

    let data = read(&account, &["p_lost"]).expect("the retained capture answers");
    assert_eq!(data["captured"]["project_count"], 1, "{data}");
    assert_eq!(data["coverage"]["visible"], 1, "{data}");
    assert_eq!(data["coverage"]["captured"], 0, "{data}");

    let _ = std::fs::remove_dir_all(&account);
}

#[test]
fn the_read_declares_the_not_visible_refusal_a_caller_can_plan_for() {
    let codes: Vec<&str> = ds_cli_design::activities::read::COMMAND
        .refusals
        .iter()
        .map(|refusal| refusal.code)
        .collect();
    assert!(codes.contains(&"project_not_visible"), "{codes:?}");
    assert!(
        ds_cli_design::activities::read::COMMAND
            .output
            .contains("`coverage`"),
        "{}",
        ds_cli_design::activities::read::COMMAND.output
    );
}
