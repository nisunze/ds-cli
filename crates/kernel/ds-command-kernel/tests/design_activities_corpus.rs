//! The recorded replay of the Design Activities owner.
//!
//! `tests/fixtures/design-activities-corpus.json` holds ledger requests over
//! the Dashboard corpus's real-shaped fleet rows and fold requests over the
//! ledgers those produce. Every case replays with exact equality, so the
//! ledger a host retains and the fold the Governance page, the CLI and MCP
//! read cannot drift without this file saying so.
//!
//! Re-record after a deliberate change with `DS_RECORD_CORPUS=1`: each case's
//! `expected` is rewritten with the live reply (2-space indent, keys in reply
//! order, trailing newline) and the replay then asserts against it, so a
//! recording run still proves the file it wrote.

use ds_command_kernel::design_activities::{evaluate, LEDGER_SCHEMA, SCHEMA};
use serde_json::Value;

const CORPUS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/design-activities-corpus.json"
);

fn corpus() -> Value {
    let raw = std::fs::read_to_string(CORPUS_PATH).expect("corpus reads");
    let corpus: Value = serde_json::from_str(&raw).expect("corpus parses");
    assert_eq!(corpus["schema"], "ds.design-activities-corpus/v1");
    corpus
}

fn answer(request: &Value) -> Value {
    let bytes = serde_json::to_vec(request).expect("request encodes");
    serde_json::from_str(&evaluate(&bytes).expect("reply")).expect("reply parses")
}

/// Where two answers first differ, as a path — so a failure names the field
/// instead of printing a fleet.
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
    let mut corpus = corpus();
    let record = recording();
    let cases = corpus["cases"].as_array_mut().expect("cases");
    assert!(cases.len() >= 8, "the corpus lost cases");
    for case in cases.iter_mut() {
        let id = case["id"].as_str().expect("case id").to_string();
        let reply = answer(&case["request"]);
        assert_eq!(reply["schema"], SCHEMA, "{id}");
        if record {
            case["expected"] = reply.clone();
        }
        assert_same(&reply, &case["expected"], &id);
    }
    if record {
        let mut text = serde_json::to_string_pretty(&corpus).expect("corpus encodes");
        text.push('\n');
        std::fs::write(CORPUS_PATH, text).expect("corpus writes");
    }
}

#[test]
fn the_fleet_ledger_is_complete_and_the_special_rows_are_only_counted() {
    let corpus = corpus();
    let case = corpus["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| case["id"] == "ledger-fleet")
        .expect("the fleet ledger case");
    let rows = case["request"]["rows"].as_array().expect("rows");
    assert!(
        rows.len() >= 80,
        "the corpus lost its real-project-shaped fleet"
    );
    let specials = [
        "combined_transformer",
        "mv_data",
        "collisions",
        "all_transformers",
    ];
    let ledger = &answer(&case["request"])["ledger"];
    assert_eq!(ledger["schema"], LEDGER_SCHEMA);
    assert_eq!(ledger["row_count"].as_u64().unwrap() as usize, rows.len());
    let transformers = ledger["transformers"].as_array().expect("transformers");
    assert_eq!(transformers.len(), rows.len() - specials.len());
    for name in specials {
        assert!(
            !transformers.iter().any(|t| t["name"] == name),
            "{name} reached the ledger"
        );
        assert!(
            rows.iter().any(|row| row["name"] == name),
            "the fleet lost {name}"
        );
    }
    for row in rows {
        let name = row["name"].as_str().unwrap_or("");
        if specials.contains(&name) {
            continue;
        }
        assert!(
            transformers.iter().any(|t| t["name"] == name),
            "{name} is missing from the ledger"
        );
    }
    // Every phase blob a row carries is a stamp in the ledger.
    for row in rows {
        let name = row["name"].as_str().unwrap_or("");
        let Some(entry) = transformers.iter().find(|t| t["name"] == name) else {
            continue;
        };
        for (slot, phase) in [
            ("sketch_metadata", "sketch"),
            ("draft_metadata", "draft"),
            ("process_metadata", "process"),
            ("report_metadata", "report"),
            ("combined_inclusion_metadata", "combined"),
        ] {
            assert_eq!(
                row.get(slot).is_some_and(Value::is_object),
                entry["phases"].get(phase).is_some(),
                "{name}: the {phase} slot and its stamp disagree"
            );
        }
    }
}

#[test]
fn every_label_is_a_key_of_this_module_or_of_the_status_row() {
    let corpus = corpus();
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let reply = answer(&case["request"]);
        let Some(activities) = reply.get("activities") else {
            for transformer in reply["ledger"]["transformers"]
                .as_array()
                .expect("transformers")
            {
                if let Some(latest) = transformer.get("latest") {
                    assert!(
                        latest["label_key"]
                            .as_str()
                            .unwrap_or("")
                            .starts_with("dsr_action_"),
                        "{id}: a ledger invented an action"
                    );
                }
            }
            continue;
        };
        for anomaly in activities["anomalies"].as_array().expect("anomalies") {
            let key = anomaly["label_key"].as_str().unwrap_or("");
            assert!(key.starts_with("gda_anomaly_"), "{id}: {key}");
            assert_eq!(
                key,
                format!("gda_anomaly_{}", anomaly["kind"].as_str().unwrap_or("")),
                "{id}"
            );
        }
        for fact in activities["facts"].as_array().expect("facts") {
            let key = fact["key"].as_str().unwrap_or("");
            assert!(key.starts_with("gda_fact_"), "{id}: {key}");
            for (_, pair) in fact["plural_params"].as_object().into_iter().flatten() {
                assert!(
                    pair.as_str().unwrap_or("").starts_with("gda_"),
                    "{id}: a plural pair is not a key"
                );
            }
        }
        for entry in activities["timeline"].as_array().expect("timeline") {
            assert!(
                entry["label_key"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("dsr_action_"),
                "{id}: the timeline invented an action"
            );
        }
        for user in activities["users"].as_array().expect("users") {
            if let Some(key) = user.get("last_label_key") {
                assert!(
                    key.as_str().unwrap_or("").starts_with("dsr_action_"),
                    "{id}"
                );
            }
        }
        let facts = activities["facts"].as_array().expect("facts");
        assert!(facts.len() <= 5, "{id}: too many facts");
    }
}

#[test]
fn the_fold_lists_are_ordered_as_the_contract_says() {
    let corpus = corpus();
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let reply = answer(&case["request"]);
        let Some(activities) = reply.get("activities") else {
            continue;
        };
        let timeline = activities["timeline"].as_array().expect("timeline");
        for pair in timeline.windows(2) {
            assert!(
                pair[0]["when_ms"].as_i64() >= pair[1]["when_ms"].as_i64(),
                "{id}: the timeline lost its order"
            );
        }
        let mut seen = std::collections::BTreeSet::new();
        for entry in timeline {
            assert!(
                seen.insert((
                    entry["ds_project"].to_string(),
                    entry["name"].to_string(),
                    entry["phase"].to_string(),
                    entry["when_ms"].to_string(),
                )),
                "{id}: a timeline stamp is duplicated"
            );
        }
        let users = activities["users"].as_array().expect("users");
        for pair in users.windows(2) {
            let left = pair[0]["last_seen_ms"].as_i64();
            let right = pair[1]["last_seen_ms"].as_i64();
            assert!(
                right.is_none() || left >= right,
                "{id}: the users lost their order"
            );
        }
        let changes = activities["changes"].as_array().expect("changes");
        for pair in changes.windows(2) {
            let left = pair[0]["when_ms"].as_i64();
            let right = pair[1]["when_ms"].as_i64();
            assert!(
                right.is_none() || left >= right,
                "{id}: the changes lost their order"
            );
        }
        for user in users {
            let per: usize = user["projects"]
                .as_array()
                .expect("projects")
                .iter()
                .map(|p| p["designs"].as_u64().unwrap_or(0) as usize)
                .sum();
            assert_eq!(per, user["designs"].as_u64().unwrap_or(0) as usize, "{id}");
        }
    }
}

#[test]
fn a_request_over_the_bound_or_with_the_wrong_schema_is_refused() {
    assert!(evaluate(b"{}").is_err());
    assert!(evaluate(br#"{"schema":"nope","action":"ledger"}"#).is_err());
    assert!(evaluate(
        br#"{"schema":"ds.design-activities/v1","action":"ledger","ds_project":"p","captured_at_ms":1,"rows":[],"nope":1}"#
    )
    .is_err());
}
