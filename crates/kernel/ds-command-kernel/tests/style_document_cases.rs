//! The style family's named parity proof (layer domain, style-document slice).
//!
//! `tests/fixtures/style-document-cases.json` is a byte-identical copy of
//! ds-web's `src/tests/fixtures/style-document/cases.json`. Each case carries
//! the op (without the schema wrapper) and the answer the kernel must give:
//! the parsed answer, or `{"error": "<the exact Err string>"}` when the
//! request itself is malformed. ds-web's browser test replays the same file
//! through the vendored WASM, so no rule in this contract can be true on one
//! host and not the other.
//!
//! Re-record `expected` with `DS_STYLE_DOCUMENT_CASES_WRITE=1 cargo test
//! --test style_document_cases` after a deliberate rule change, and say why in
//! the commit. That also refreshes the ds-web copy when `../ds-web` is present.

use ds_command_kernel::style_document::{evaluate, SCHEMA};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

const CASES: &str = include_str!("fixtures/style-document-cases.json");

fn own_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/style-document-cases.json")
}
fn web_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../ds-web/src/tests/fixtures/style-document/cases.json")
}

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CASES).expect("cases parse");
    assert_eq!(corpus["schema"], "ds.style-document-cases/v1");
    corpus
}

/// What the kernel answers for one case: its answer parsed, or its refusal
/// of the request itself as `{"error": ...}`.
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
    let write = std::env::var_os("DS_STYLE_DOCUMENT_CASES_WRITE").is_some();
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
        "the ds-web copy has drifted; re-record with DS_STYLE_DOCUMENT_CASES_WRITE=1"
    );
}

#[test]
fn the_corpus_covers_every_op_action_and_outcome_the_contract_names() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 30, "the corpus lost cases");
    let mut ids = BTreeSet::new();
    let mut ops = BTreeSet::new();
    let mut admitted = BTreeSet::new();
    let mut refused = BTreeSet::new();
    let mut errors = 0;
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        assert!(ids.insert(id), "{id} is recorded twice");
        assert!(
            case["why"].as_str().is_some_and(|why| why.len() > 20),
            "{id} does not say why it exists"
        );
        let kind = case["request"]["kind"].as_str().unwrap_or_default();
        let action = case["request"]["action"].as_str().unwrap_or_default();
        ops.insert(format!("{kind}/{action}"));
        match case["expected"]["outcome"].as_str() {
            Some("admitted") => {
                admitted.insert(action.to_owned());
            }
            Some("refused") => {
                refused.insert(action.to_owned());
            }
            _ => {
                if case["expected"].get("error").is_some() {
                    errors += 1;
                }
            }
        }
    }
    for wanted in [
        "save_payload/",
        "request/publish",
        "request/seed",
        "request/read_shared",
        "request/assemble_runtime",
        "admit/publish",
        "admit/seed",
        "admit/read_shared",
        "admit/assemble_runtime",
    ] {
        assert!(ops.contains(wanted), "no case exercises {wanted}");
    }
    let family: BTreeSet<String> = ["publish", "seed", "read_shared", "assemble_runtime"]
        .iter()
        .map(|action| (*action).to_owned())
        .collect();
    assert_eq!(admitted, family, "every action must have an admitted case");
    assert_eq!(refused, family, "every action must have a refused case");
    assert!(errors >= 5, "malformed requests are part of the contract");
    // The refusal vocabulary is closed.
    for case in cases {
        if let Some(code) = case["expected"]["refusal"]["code"].as_str() {
            assert!(
                [
                    "backend_refused",
                    "unreadable_response",
                    "contract_violation"
                ]
                .contains(&code),
                "{} names an unknown refusal code {code}",
                case["id"]
            );
        }
    }
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
