//! The survey media address's named parity proof.
//!
//! `tests/fixtures/survey-media-address-cases.json` is a byte-identical copy
//! of ds-web's `src/tests/fixtures/survey-media-address/cases.json`. Each case
//! carries the op (without the schema wrapper) and the answer the kernel must
//! give: the resolved address or wire body, or `{"error": "<the exact Err
//! string>"}` when the reference or the request is refused. ds-web's browser
//! test replays the same file through the vendored WASM, so a media reference
//! cannot resolve one way in the browser and another in `ds`, the desktop
//! shell or the gateway.
//!
//! Re-record `expected` with `DS_SURVEY_MEDIA_ADDRESS_CASES_WRITE=1 cargo test
//! --test survey_media_address_cases` after a deliberate rule change, and say
//! why in the commit. That also refreshes the ds-web copy when `../ds-web` is
//! present.

use ds_command_kernel::survey_media_address::{evaluate, REFUSAL, SCHEMA};
use ds_command_kernel::survey_photo_outbox::thumbnail_path_for;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

const CASES: &str = include_str!("fixtures/survey-media-address-cases.json");

fn own_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/survey-media-address-cases.json")
}
fn web_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../ds-web/src/tests/fixtures/survey-media-address/cases.json")
}

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CASES).expect("cases parse");
    assert_eq!(corpus["schema"], "ds.survey-media-address-cases/v1");
    corpus
}

/// What the kernel answers for one case: its answer parsed, or its refusal as
/// `{"error": ...}`.
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
    let write = std::env::var_os("DS_SURVEY_MEDIA_ADDRESS_CASES_WRITE").is_some();
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
        "the ds-web copy has drifted; re-record with DS_SURVEY_MEDIA_ADDRESS_CASES_WRITE=1"
    );
}

#[test]
fn the_corpus_covers_every_layout_op_and_refusal_the_contract_names() {
    let corpus = corpus();
    let vocabulary = corpus["refusals"].as_object().expect("refusal vocabulary");
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 35, "the corpus lost cases");
    let mut ids = BTreeSet::new();
    let mut parsed_layouts = BTreeSet::new();
    let mut wired_layouts = BTreeSet::new();
    let mut refused = BTreeSet::new();
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        assert!(ids.insert(id), "{id} is recorded twice");
        assert!(
            case["why"].as_str().is_some_and(|why| why.len() > 20),
            "{id} does not say why it exists"
        );
        let expected = &case["expected"];
        match case["refuses"].as_str() {
            Some(token) => {
                assert!(
                    vocabulary.contains_key(token),
                    "{id}: {token} is not in the refusal vocabulary"
                );
                refused.insert(token.to_owned());
                let error = expected["error"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{id}: a refusal case must record an error string"));
                if token == "malformed_request" {
                    assert_ne!(
                        error, REFUSAL,
                        "{id}: a malformed request is not a path refusal"
                    );
                } else {
                    assert_eq!(error, REFUSAL, "{id}: every path refusal is one sentence");
                }
            }
            None => {
                assert!(
                    expected.get("error").is_none(),
                    "{id} refuses without naming why"
                );
                match case["request"]["kind"].as_str() {
                    Some("parse") => {
                        let layout = expected["layout"].as_str().expect("a parsed layout");
                        parsed_layouts.insert(layout.to_owned());
                        // The companion is never hand-written into the corpus:
                        // it is whatever the outbox's one rule derives.
                        let object = expected["object_path"].as_str().expect("an object path");
                        assert_eq!(
                            expected["thumbnail_object_path"].as_str(),
                            thumbnail_path_for(object).ok().as_deref(),
                            "{id}: the companion is not thumbnail_path_for's answer"
                        );
                        assert_eq!(
                            expected.get("field").is_some(),
                            layout == "field_app",
                            "{id}: only the field app layout carries a media field"
                        );
                    }
                    Some("wire") => {
                        let layout = expected["layout"].as_str().unwrap_or("web");
                        wired_layouts.insert(layout.to_owned());
                        for required in ["project_id", "form_slug", "entry_id", "file_name"] {
                            assert!(expected[required].is_string(), "{id} omits {required}");
                        }
                        assert_eq!(
                            expected.get("media_field").is_some(),
                            layout == "field_app",
                            "{id}: only the field app layout sends a media field"
                        );
                    }
                    other => panic!("{id}: unknown op {other:?}"),
                }
            }
        }
    }
    let layouts: BTreeSet<String> = ["web", "projects", "field_app"]
        .iter()
        .map(|layout| (*layout).to_owned())
        .collect();
    assert_eq!(parsed_layouts, layouts, "every layout must be parsed");
    assert_eq!(wired_layouts, layouts, "every layout must be addressed");
    let named: BTreeSet<String> = vocabulary.keys().cloned().collect();
    assert_eq!(refused, named, "every named refusal must be exercised");
}

#[test]
fn the_web_layouts_wire_body_stays_byte_identical_to_todays_request() {
    // The released gateway accepts exactly these four fields. If this ever
    // changes, ds-brain must be released first.
    let corpus = corpus();
    let mut proven = 0;
    for case in corpus["cases"].as_array().expect("cases") {
        if case["request"]["kind"] == "wire" && case["expected"].get("error").is_none() {
            let body = case["expected"].as_object().expect("a wire body");
            if !body.contains_key("layout") {
                assert_eq!(
                    body.keys().collect::<Vec<_>>(),
                    ["project_id", "form_slug", "entry_id", "file_name"],
                    "{}: the web layout's body is today's body, in today's order",
                    case["id"]
                );
                proven += 1;
            }
        }
    }
    assert!(proven >= 2, "the web layout's body must be pinned");
}

#[test]
fn the_corpus_keeps_the_order_of_keys_the_recorder_wrote() {
    // `preserve_order` is on: a re-recorded corpus must diff minimally.
    let corpus: Map<String, Value> = serde_json::from_str(CASES).expect("cases parse");
    assert_eq!(
        corpus.keys().collect::<Vec<_>>(),
        ["schema", "purpose", "source", "note", "refusals", "cases"]
    );
}
