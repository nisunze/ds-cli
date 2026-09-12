//! The application settings record's named proof.
//!
//! `tests/fixtures/application-settings-cases.json` is a byte-identical copy
//! of ds-web's `src/tests/fixtures/application-settings/cases.json`. Each case
//! carries the op (without the schema wrapper) and the answer the kernel must
//! give: the parsed answer, or `{"error": "<the exact Err string>"}` when the
//! request itself is malformed. ds-web's browser test replays the same file
//! through the vendored WASM, so no default, bound, migration or lane rule can
//! be true in Rust and not in the build the browser loads.
//!
//! Re-record `expected` with `DS_APPLICATION_SETTINGS_CASES_WRITE=1 cargo test
//! --test application_settings_cases` after a deliberate rule change, and say
//! why in the commit. That also refreshes the ds-web copy when `../ds-web` is
//! present.

use ds_command_kernel::application_settings::{evaluate, SCHEMA, SETTING_KEYS};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

const CASES: &str = include_str!("fixtures/application-settings-cases.json");

fn own_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/application-settings-cases.json")
}
fn web_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-web/src/tests/fixtures/application-settings/cases.json")
}

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CASES).expect("cases parse");
    assert_eq!(corpus["schema"], "ds.application-settings-cases/v1");
    corpus
}

/// What the kernel answers for one case: its answer parsed, or its refusal of
/// the request itself as `{"error": ...}`.
fn answer(case: &Value) -> Value {
    let id = case["id"].as_str().expect("case id");
    let schema = case.get("schema").and_then(Value::as_str).unwrap_or(SCHEMA);
    let request = serde_json::to_vec(&json!({"schema": schema, "op": case["request"]}))
        .expect("request encodes");
    match evaluate(&request) {
        Ok(answer) => {
            serde_json::from_str(&answer).unwrap_or_else(|_| panic!("{id}: answer is not JSON"))
        }
        Err(error) => json!({ "error": error }),
    }
}

#[test]
fn the_kernel_answers_every_recorded_case_or_rerecords_it() {
    let mut corpus = corpus();
    let write = std::env::var_os("DS_APPLICATION_SETTINGS_CASES_WRITE").is_some();
    let mut drift = Vec::new();
    for case in corpus["cases"].as_array_mut().expect("cases") {
        let id = case["id"].as_str().expect("case id").to_owned();
        let actual = answer(case);
        if write {
            case["expected"] = actual;
        } else if case["expected"] != actual {
            drift.push(format!(
                "{id}:\n  recorded: {}\n  kernel:   {actual}",
                case["expected"]
            ));
        }
    }
    if write {
        let text = format!(
            "{}\n",
            serde_json::to_string_pretty(&corpus).expect("cases encode")
        );
        std::fs::write(own_copy(), &text).expect("write the kernel copy");
        if web_copy().parent().is_some_and(|dir| dir.is_dir()) {
            std::fs::write(web_copy(), &text).expect("write the ds-web copy");
        }
        return;
    }
    assert!(drift.is_empty(), "\n{}", drift.join("\n\n"));
}

#[test]
fn the_ds_web_copy_is_the_same_file() {
    let Ok(shared) = std::fs::read_to_string(web_copy()) else {
        // A kernel checkout without its ds-web sibling still proves itself.
        return;
    };
    assert_eq!(
        shared, CASES,
        "the ds-web copy has drifted; re-record with DS_APPLICATION_SETTINGS_CASES_WRITE=1"
    );
}

/// Every op the contract names is exercised, every case says why it exists,
/// and the malformed-request path is part of the record.
#[test]
fn the_corpus_covers_every_op_the_contract_names() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 40, "the corpus lost cases");
    let mut ids = BTreeSet::new();
    let mut ops = BTreeSet::new();
    let mut errors = 0;
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        assert!(ids.insert(id), "{id} is recorded twice");
        assert!(
            case["why"].as_str().is_some_and(|why| why.len() > 40),
            "{id} does not say why it exists"
        );
        ops.insert(
            case["request"]["kind"]
                .as_str()
                .expect("an op names its kind")
                .to_owned(),
        );
        if case["expected"].get("error").is_some() {
            errors += 1;
        }
    }
    for op in [
        "normalize",
        "patch",
        "reset",
        "lane",
        "set_lane",
        "performance_profile",
        "interval_label",
    ] {
        assert!(ops.contains(op), "no case exercises the {op} op");
    }
    assert!(errors >= 3, "malformed requests are part of the contract");
}

/// The record is the store's record: every answered `settings` carries every
/// key the store produced, in the store's order, and nothing else — the
/// confirmation markers only while they apply.
#[test]
fn every_answered_record_has_the_store_keys_in_the_store_order() {
    let corpus = corpus();
    let markers = ["standardLaneConfirmedAt", "fastLaneConfirmedAt"];
    let mut checked = 0;
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let Some(settings) = case["expected"]["settings"].as_object() else {
            continue;
        };
        checked += 1;
        let keys: Vec<&str> = settings.keys().map(String::as_str).collect();
        let expected: Vec<&str> = SETTING_KEYS
            .iter()
            .copied()
            .filter(|key| !markers.contains(key) || settings.contains_key(*key))
            .collect();
        assert_eq!(keys, expected, "{id}: the record's keys or their order");
        let lane = settings["processingLane"].as_str().expect("a lane word");
        assert!(
            settings.contains_key("standardLaneConfirmedAt") == (lane == "standard"),
            "{id}: the Standard marker is present exactly while Standard is the preference"
        );
        assert!(
            !(settings.contains_key("fastLaneConfirmedAt") && lane != "fast"),
            "{id}: a fast marker never rides on a Standard preference"
        );
    }
    assert!(checked >= 25, "the corpus records too few records");
}

/// `persist` and `first_launch` are functions of the request alone, so the
/// recorded answers are checked against the requests rather than against
/// prose: a legacy lane migrates only into a blob without one, and a device
/// default applies only where the blob has nothing.
#[test]
fn normalize_writes_back_only_a_migration_and_fills_only_what_the_blob_lacks() {
    let corpus = corpus();
    let truthy = |value: &Value| match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    };
    let mut migrations = 0;
    let mut first_launches = 0;
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let request = &case["request"];
        if request["kind"] != "normalize" || case["expected"].get("error").is_some() {
            continue;
        }
        let expected = &case["expected"];
        let saved = request["saved"].as_object().cloned().unwrap_or_default();
        let legacy = match &request["legacy_lane"] {
            lane @ Value::String(_) => Some(lane.clone()),
            Value::Object(record) => record.get("lane").cloned(),
            _ => None,
        };
        let migrate =
            !saved.get("processingLane").is_some_and(truthy) && legacy.as_ref().is_some_and(truthy);
        assert_eq!(expected["persist"], json!(migrate), "{id}: persist");
        migrations += usize::from(migrate);
        let absent = |key: &str| matches!(saved.get(key), None | Some(Value::Null));
        assert_eq!(
            expected["first_launch"],
            json!({
                "popup_mode": absent("popupMode"),
                "my_location": absent("showMyLocationControl"),
            }),
            "{id}: first_launch"
        );
        first_launches += usize::from(absent("popupMode"));
        let device = &request["device"];
        if absent("popupMode") {
            let popup = if device["phone"] == true {
                "minimal"
            } else {
                "normal"
            };
            assert_eq!(
                expected["settings"]["popupMode"],
                json!(popup),
                "{id}: device popup"
            );
        }
        if absent("showMyLocationControl") {
            assert_eq!(
                expected["settings"]["showMyLocationControl"], device["my_location_default"],
                "{id}: device my-location"
            );
        }
    }
    assert!(migrations >= 3, "the corpus proves too few migrations");
    assert!(
        first_launches >= 3,
        "the corpus proves too few first launches"
    );
}

/// The lane rule, asserted over every recorded answer: Standard is never the
/// lane where the installed shell or the project withholds it, and a record's
/// preference is never rewritten by the facts.
#[test]
fn standard_is_never_answered_where_it_is_unreachable() {
    let corpus = corpus();
    let mut withheld = 0;
    let mut kept = 0;
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let facts = &case["request"]["lane_facts"];
        let Some(lane) = case["expected"]["lane"].as_str() else {
            continue;
        };
        let available = facts["desktop_surface"] == false && facts["project_standard_lane"] == true;
        if !available {
            assert_eq!(
                lane, "fast",
                "{id}: Standard answered where it is unreachable"
            );
            withheld += 1;
            if case["expected"]["settings"]["processingLane"] == "standard" {
                kept += 1;
            }
        }
    }
    assert!(withheld >= 4, "the corpus proves too few withheld lanes");
    assert!(
        kept >= 2,
        "the corpus never shows a preference surviving a fast-only visit"
    );
}

#[test]
fn the_corpus_keeps_the_order_of_keys_the_recorder_wrote() {
    // `preserve_order` is on: a re-recorded corpus must diff minimally.
    let corpus: Map<String, Value> = serde_json::from_str(CASES).expect("cases parse");
    assert_eq!(
        corpus.keys().collect::<Vec<_>>(),
        ["schema", "purpose", "source", "note", "cases"]
    );
}
