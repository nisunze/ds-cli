//! The prepared local layer lifecycle's named proof (layer domain, slice 2).
//!
//! `tests/fixtures/local-layers-cases.json` is a byte-identical copy of
//! ds-web's `src/tests/fixtures/local-layers/cases.json`. Each case carries the
//! op (without the schema wrapper) and the answer the kernel must give: the
//! parsed answer, or `{"error": "<the exact Err string>"}` when the request
//! itself is malformed. ds-web's browser test replays the same file through the
//! vendored WASM and the native store replays it over a real catalogue file, so
//! no rule in this contract can be true on one host and not another.
//!
//! Re-record `expected` with `DS_LOCAL_LAYERS_CASES_WRITE=1 cargo test --test
//! local_layers_cases` after a deliberate rule change, and say why in the
//! commit. That also refreshes the ds-web copy when `../ds-web` is present.

use ds_command_kernel::local_layers::{evaluate, SCHEMA};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

const CASES: &str = include_str!("fixtures/local-layers-cases.json");

fn own_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/local-layers-cases.json")
}
fn web_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../ds-web/src/tests/fixtures/local-layers/cases.json")
}

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CASES).expect("cases parse");
    assert_eq!(corpus["schema"], "ds.local-layers-cases/v1");
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
    let write = std::env::var_os("DS_LOCAL_LAYERS_CASES_WRITE").is_some();
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
        "the ds-web copy has drifted; re-record with DS_LOCAL_LAYERS_CASES_WRITE=1"
    );
}

/// A refusal is a decision the whole lifecycle shares, so the corpus must
/// exercise every op, every change and every refusal code the contract names —
/// and nothing it does not.
#[test]
fn the_corpus_covers_every_op_change_and_refusal_the_contract_names() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 50, "the corpus lost cases");
    let mut ids = BTreeSet::new();
    let mut ops = BTreeSet::new();
    let mut changes = BTreeSet::new();
    let mut refused = BTreeSet::new();
    let mut applied = BTreeSet::new();
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
        if let Some(change) = case["request"]["change"]["kind"].as_str() {
            changes.insert(change.to_owned());
        }
        if let Some(code) = case["expected"]["refusal"]["code"].as_str() {
            refused.insert(code.to_owned());
        }
        if let Some(change) = case["expected"]["receipt"]["applied"].as_str() {
            applied.insert(change.to_owned());
        }
        if case["expected"].get("error").is_some() {
            errors += 1;
        }
    }
    for op in ["list", "register", "restore", "update", "remove"] {
        assert!(ops.contains(op), "no case exercises the {op} op");
    }
    let vocabulary: BTreeSet<String> = [
        "payload",
        "source_metadata",
        "rename",
        "rename_source",
        "style",
        "visible",
    ]
    .iter()
    .map(|word| (*word).to_owned())
    .collect();
    assert_eq!(changes, vocabulary, "every change kind must be exercised");
    assert_eq!(
        applied, vocabulary,
        "every change kind must have a case that is actually applied"
    );
    assert_eq!(
        refused,
        [
            "duplicate_layer",
            "malformed_descriptor",
            "scope_mismatch",
            "unknown_layer",
            "unsupported_source_kind",
        ]
        .iter()
        .map(|code| (*code).to_owned())
        .collect::<BTreeSet<String>>(),
        "the refusal vocabulary is closed and every code is proven"
    );
    assert!(errors >= 5, "malformed requests are part of the contract");
}

/// The two proofs the whole boundary rests on, asserted over the recorded
/// answers rather than over prose: a refusal never rewrites the caller's
/// catalogue, and a release never names anything the layer does not own.
#[test]
fn a_refusal_changes_nothing_and_a_release_names_only_owned_records() {
    let corpus = corpus();
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let expected = &case["expected"];
        if expected["outcome"] == "refused" || expected.get("error").is_some() {
            assert!(
                expected.get("registry").is_none() && expected.get("receipt").is_none(),
                "{id}: a refusal answers no registry, so nothing can be written from it"
            );
            continue;
        }
        assert!(
            expected["registry"]["layers"].is_array(),
            "{id}: an admitted answer carries the next registry"
        );
        for item in expected["receipt"]["release"]
            .as_array()
            .into_iter()
            .flatten()
        {
            let kind = item["kind"].as_str().unwrap_or_default();
            assert!(
                matches!(kind, "features_payload" | "upload_chunks"),
                "{id}: {kind} is not a record a local layer owns"
            );
            let keys: Vec<&str> = item
                .as_object()
                .expect("a release item is an object")
                .keys()
                .map(String::as_str)
                .collect();
            assert!(
                keys == ["kind", "id"]
                    || keys == ["kind", "workspaceKey", "fileName", "parsedLayer"],
                "{id}: a release item names exactly one owned record, saw {keys:?}"
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
