//! The recorded replay of the Design Activities store's policy.
//!
//! `tests/fixtures/design-activities-store-cases.json` holds one request per
//! decision the store makes — admitting a snapshot, naming a ledger's digest,
//! placing a capture, thinning a project's captures, naming the pair the fold
//! diffs and ordering a sweep — and replays each with exact equality. Two
//! hosts write this store (the Linux server under `$XDG_STATE_HOME` and the
//! Governance page in IndexedDB), so a drift here is two histories of the
//! same captures.
//!
//! A case with `refusal` carries the words the refusal must contain instead
//! of an `expected` reply: what the store refuses is as much of its contract
//! as what it admits.
//!
//! Re-record after a deliberate change with `DS_RECORD_CORPUS=1`: each case's
//! `expected` is rewritten with the live reply (2-space indent, keys in reply
//! order, trailing newline) and the replay then asserts against it, so a
//! recording run still proves the file it wrote.

use ds_command_kernel::design_activities_store::{evaluate, SCHEMA, SNAPSHOT_SCHEMA};
use serde_json::Value;

const CASES_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/design-activities-store-cases.json"
);

fn cases() -> Value {
    let raw = std::fs::read_to_string(CASES_PATH).expect("cases read");
    let cases: Value = serde_json::from_str(&raw).expect("cases parse");
    assert_eq!(cases["schema"], "ds.design-activities-store-cases/v1");
    cases
}

fn answer(request: &Value) -> Result<Value, String> {
    let bytes = serde_json::to_vec(request).expect("request encodes");
    evaluate(&bytes).map(|reply| serde_json::from_str(&reply).expect("reply parses"))
}

/// Where two answers first differ, as a path — so a failure names the field
/// instead of printing a store.
fn first_difference(left: &Value, right: &Value, path: &str) -> Option<String> {
    match (left, right) {
        (Value::Object(left_map), Value::Object(right_map)) => {
            for (key, held) in left_map {
                let Some(other) = right_map.get(key) else {
                    return Some(format!("{path}.{key} is missing"));
                };
                if let Some(found) = first_difference(held, other, &format!("{path}.{key}")) {
                    return Some(found);
                }
            }
            right_map
                .keys()
                .find(|key| !left_map.contains_key(*key))
                .map(|key| format!("{path}.{key} is unexpected"))
        }
        (Value::Array(left_items), Value::Array(right_items)) => {
            if left_items.len() != right_items.len() {
                return Some(format!(
                    "{path} holds {} entries, not {}",
                    left_items.len(),
                    right_items.len()
                ));
            }
            left_items
                .iter()
                .zip(right_items)
                .enumerate()
                .find_map(|(index, (held, other))| {
                    first_difference(held, other, &format!("{path}[{index}]"))
                })
        }
        _ if left == right => None,
        _ => Some(format!("{path}: {left} is not {right}")),
    }
}

#[track_caller]
fn assert_same(left: &Value, right: &Value, id: &str) {
    if let Some(found) = first_difference(left, right, id) {
        panic!("{found}");
    }
}

fn recording() -> bool {
    std::env::var("DS_RECORD_CORPUS").is_ok_and(|value| value == "1")
}

#[test]
fn every_recorded_request_replays_exactly() {
    let mut cases = cases();
    let record = recording();
    let entries = cases["cases"].as_array_mut().expect("cases");
    assert!(entries.len() >= 8, "the cases file lost cases");
    for case in entries.iter_mut() {
        let id = case["id"].as_str().expect("case id").to_string();
        match case
            .get("refusal")
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            Some(words) => {
                let refusal = answer(&case["request"]).expect_err(&format!("{id} is admitted"));
                assert!(refusal.contains(&words), "{id}: {refusal}");
                assert!(
                    case.get("expected").is_none(),
                    "{id}: a refusal has no reply"
                );
            }
            None => {
                let reply =
                    answer(&case["request"]).unwrap_or_else(|error| panic!("{id}: {error}"));
                assert_eq!(reply["schema"], SCHEMA, "{id}");
                if record {
                    case["expected"] = reply.clone();
                }
                assert_same(&reply, &case["expected"], &id);
            }
        }
    }
    if record {
        let mut text = serde_json::to_string_pretty(&cases).expect("cases encode");
        text.push('\n');
        std::fs::write(CASES_PATH, text).expect("cases write");
    }
}

#[test]
fn every_action_is_exercised_and_every_refusal_names_its_field() {
    let cases = cases();
    let entries = cases["cases"].as_array().expect("cases");
    for action in ["validate", "digest", "key", "retain", "select", "plan"] {
        assert!(
            entries
                .iter()
                .any(|case| case["request"]["action"] == action && case.get("refusal").is_none()),
            "no case admits {action}"
        );
        assert!(
            entries
                .iter()
                .any(|case| case["request"]["action"] == action && case.get("refusal").is_some()),
            "no case refuses {action}"
        );
    }
    for case in entries {
        let Some(words) = case.get("refusal").and_then(Value::as_str) else {
            continue;
        };
        let id = case["id"].as_str().expect("case id");
        let refusal = answer(&case["request"]).expect_err("refused");
        assert!(
            refusal.starts_with("Design activities store refuses")
                || refusal.starts_with("Invalid design activities store request")
                || refusal.starts_with("Unsupported design activities store"),
            "{id}: {refusal} is not one of the store's refusals"
        );
        assert!(refusal.contains(words), "{id}: {refusal}");
    }
}

#[test]
fn what_is_retained_is_never_more_than_what_the_store_held() {
    let cases = cases();
    for case in cases["cases"].as_array().expect("cases") {
        if case["request"]["action"] != "retain" || case.get("refusal").is_some() {
            continue;
        }
        let id = case["id"].as_str().expect("case id");
        let mut existing: Vec<i64> =
            serde_json::from_value(case["request"]["existing"].clone()).expect("existing");
        existing.sort_unstable();
        existing.dedup();
        let reply = answer(&case["request"]).expect("retain");
        let keep: Vec<i64> = serde_json::from_value(reply["keep"].clone()).expect("keep");
        let dropped: Vec<i64> = serde_json::from_value(reply["drop"].clone()).expect("drop");
        let mut both: Vec<i64> = keep.iter().chain(dropped.iter()).copied().collect();
        both.sort_unstable();
        assert_eq!(both, existing, "{id}: the store gained or lost a capture");
        assert!(keep.windows(2).all(|pair| pair[0] < pair[1]), "{id}");
        assert!(dropped.windows(2).all(|pair| pair[0] < pair[1]), "{id}");
        if let Some(newest) = existing.last() {
            assert!(
                keep.contains(newest),
                "{id}: retention dropped the newest capture"
            );
        }
        // Idempotent: what survives one retention survives the next.
        let mut again = case["request"].clone();
        again["existing"] = serde_json::to_value(&keep).expect("keep encodes");
        let second = answer(&again).expect("retain again");
        assert_eq!(
            second["keep"], reply["keep"],
            "{id}: retention is not idempotent"
        );
        assert_eq!(second["drop"], serde_json::json!([] as [i64; 0]), "{id}");
    }
}

#[test]
fn a_planned_sweep_is_sequential_paused_and_accounts_for_every_project() {
    let cases = cases();
    for case in cases["cases"].as_array().expect("cases") {
        if case["request"]["action"] != "plan" || case.get("refusal").is_some() {
            continue;
        }
        let id = case["id"].as_str().expect("case id");
        let reply = answer(&case["request"]).expect("plan");
        assert_eq!(reply["sequential"], serde_json::json!(true), "{id}");
        assert!(reply["pause_ms"].as_i64().expect("pause") >= 0, "{id}");
        let visible: Vec<String> = case["request"]["visible"]
            .as_array()
            .expect("visible")
            .iter()
            .map(|project| project["ds_project"].as_str().expect("name").to_string())
            .collect();
        let mut answered: Vec<String> = reply["refresh"]
            .as_array()
            .expect("refresh")
            .iter()
            .chain(reply["skip"].as_array().expect("skip"))
            .map(|entry| entry["ds_project"].as_str().expect("name").to_string())
            .collect();
        answered.sort();
        let mut expected = visible.clone();
        expected.sort();
        assert_eq!(
            answered, expected,
            "{id}: a visible project went unanswered"
        );
    }
}

#[test]
fn a_snapshot_the_cases_admit_is_the_envelope_the_hosts_write() {
    let cases = cases();
    let admitted = cases["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| case["request"]["action"] == "validate" && case.get("refusal").is_none())
        .expect("an admitted snapshot");
    let snapshot = &admitted["request"]["snapshot"];
    assert_eq!(snapshot["schema"], SNAPSHOT_SCHEMA);
    assert_eq!(snapshot["ledger"]["schema"], "ds.design-activity-ledger/v1");
    // The digest in the file is the ledger it carries — the whole point of
    // the envelope — and the `digest` action agrees with it.
    let reply = answer(&serde_json::json!({
        "schema": SCHEMA, "action": "digest", "ledger": snapshot["ledger"].clone(),
    }))
    .expect("digest");
    assert_eq!(reply["digest"], snapshot["digest"]);
}
