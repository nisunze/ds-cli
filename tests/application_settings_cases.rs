//! The application settings record's named proof.
//!
//! `tests/fixtures/application-settings-cases.json` is a byte-identical copy
//! of ds-web's `src/tests/fixtures/application-settings/cases.json`. Each case
//! carries the op (without the schema wrapper) and the answer the kernel must
//! give: the parsed answer, or `{"error": "<the exact Err string>"}` when the
//! request itself is malformed. ds-web's browser test replays the same file
//! through the vendored WASM, so no default, bound, first-launch rule or
//! retired-key drop can be true in Rust and not in the build the browser loads.
//!
//! Re-record `expected` with `DS_APPLICATION_SETTINGS_CASES_WRITE=1 cargo test
//! --test application_settings_cases` after a deliberate rule change, and say
//! why in the commit. That also refreshes the ds-web copy when `../ds-web` is
//! present.

use ds_command_kernel::application_settings::{evaluate, SCHEMA, SETTING_KEYS};

/// The keys a v1 record carried for the retired processing-lane preference.
const RETIRED_KEYS: [&str; 3] = [
    "processingLane",
    "standardLaneConfirmedAt",
    "fastLaneConfirmedAt",
];
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
    assert!(cases.len() >= 60, "the corpus lost cases");
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
        "performance_profile",
        "interval_label",
    ] {
        assert!(ops.contains(op), "no case exercises the {op} op");
    }
    assert!(errors >= 3, "malformed requests are part of the contract");
    // The retired lane ops are not part of the record any more.
    for op in ["lane", "set_lane"] {
        assert!(
            !ops.contains(op),
            "a case still exercises the retired {op} op"
        );
    }
}

/// The record is the store's record: every answered `settings` carries every
/// key the store produced, in the store's order, and nothing else. The
/// retired lane keys never appear.
#[test]
fn every_answered_record_has_the_store_keys_in_the_store_order() {
    let corpus = corpus();
    let mut checked = 0;
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let Some(settings) = case["expected"]["settings"].as_object() else {
            continue;
        };
        checked += 1;
        let keys: Vec<&str> = settings.keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            SETTING_KEYS.to_vec(),
            "{id}: the record's keys or their order"
        );
        for retired in RETIRED_KEYS {
            assert!(
                !settings.contains_key(retired),
                "{id}: the retired {retired} was written"
            );
        }
        assert!(
            case["expected"].get("lane").is_none(),
            "{id}: the answer still names a lane"
        );
    }
    assert!(checked >= 25, "the corpus records too few records");
}

/// `persist` and `first_launch` are functions of the request alone, so the
/// recorded answers are checked against the requests rather than against
/// prose: a blob is written back exactly when it still carried a retired lane
/// key, and a device default applies only where the blob has nothing.
#[test]
fn normalize_writes_back_only_a_retired_key_drop_and_fills_only_what_the_blob_lacks() {
    let corpus = corpus();
    let mut write_backs = 0;
    let mut first_launches = 0;
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let request = &case["request"];
        if request["kind"] != "normalize" || case["expected"].get("error").is_some() {
            continue;
        }
        let expected = &case["expected"];
        let saved = request["saved"].as_object().cloned().unwrap_or_default();
        assert!(
            request.get("legacy_lane").is_none() && request.get("lane_facts").is_none(),
            "{id}: a retired request member is still recorded"
        );
        let write_back = RETIRED_KEYS.iter().any(|key| saved.contains_key(*key));
        assert_eq!(expected["persist"], json!(write_back), "{id}: persist");
        write_backs += usize::from(write_back);
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
    assert!(write_backs >= 2, "the corpus proves too few write-backs");
    assert!(
        first_launches >= 3,
        "the corpus proves too few first launches"
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
