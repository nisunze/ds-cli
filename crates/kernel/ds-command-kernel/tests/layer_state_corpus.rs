//! The layer drawer's named parity proof (layer domain, slice 1).
//!
//! `tests/fixtures/layer-state-corpus.json` is a byte-identical copy of
//! ds-web's `src/tests/fixtures/layer-state/corpus.json`. Each case carries
//! the request, what the ds-web TypeScript answered for it (`expected_ts`,
//! recorded by the differential recorder before that TypeScript was removed)
//! and what `layer_state::evaluate` answers (`expected`).
//!
//! Every field the TypeScript answered must be answered identically by the
//! kernel, except at the JSON pointers a case declares as divergences — and
//! there the answers must actually differ, so a declaration cannot go stale.
//! `expected` must match byte-for-byte; ds-web's `layer-state-parity.test.ts`
//! replays the same requests through the vendored WASM against it.
//!
//! Re-record `expected` with `DS_LAYER_STATE_CORPUS_WRITE=1 cargo test --test
//! layer_state_corpus` after a deliberate rule change, and say why in the
//! commit. That also refreshes the ds-web copy when `../ds-web` is present.

use ds_command_kernel::layer_state::evaluate;
use serde_json::{Map, Value};
use std::path::PathBuf;

const CORPUS: &str = include_str!("fixtures/layer-state-corpus.json");

fn own_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/layer-state-corpus.json")
}
fn web_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../../ds-web/src/tests/fixtures/layer-state/corpus.json")
}

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CORPUS).expect("corpus parses");
    assert_eq!(corpus["schema"], "ds.layer-state-parity/v1");
    corpus
}

fn answer(case: &Value) -> Value {
    let id = case["id"].as_str().expect("case id");
    let request = serde_json::to_vec(&case["request"]).expect("request encodes");
    serde_json::from_str(&evaluate(&request).unwrap_or_else(|error| panic!("{id}: {error}")))
        .expect("kernel answers JSON")
}

/// Every key the TypeScript answered is present and equal in the kernel's
/// answer; the kernel may answer more. Maps of layer ids (`preferences`, the
/// isolation snapshot's `visibility`) must match exactly — an extra entry
/// there would be a decision the TypeScript never made.
fn compare(
    expected: &Value,
    actual: &Value,
    path: &str,
    skip: &[String],
    differences: &mut Vec<String>,
    mismatches: &mut Vec<String>,
) {
    if skip.iter().any(|s| s == path) {
        if expected == actual {
            mismatches.push(format!(
                "{path}: declared divergent but the kernel answers the same as the TypeScript"
            ));
        } else {
            differences.push(path.to_owned());
        }
        return;
    }
    match (expected, actual) {
        (Value::Object(want), Value::Object(have)) => {
            let exact = path == "/preferences" || path == "/isolation/visibility";
            if exact {
                let want_keys: std::collections::BTreeSet<_> = want.keys().collect();
                let have_keys: std::collections::BTreeSet<_> = have.keys().collect();
                if want_keys != have_keys {
                    mismatches.push(format!(
                        "{path}: key sets differ\n  ts:     {want_keys:?}\n  kernel: {have_keys:?}"
                    ));
                }
            }
            for (key, value) in want {
                match have.get(key) {
                    Some(actual) => compare(
                        value,
                        actual,
                        &format!("{path}/{key}"),
                        skip,
                        differences,
                        mismatches,
                    ),
                    None => mismatches.push(format!("{path}/{key}: the kernel has no such field")),
                }
            }
        }
        (Value::Array(want), Value::Array(have)) => {
            if want.len() != have.len() {
                mismatches.push(format!(
                    "{path}: {} elements in TypeScript, {} in the kernel",
                    want.len(),
                    have.len()
                ));
                return;
            }
            for (index, (w, h)) in want.iter().zip(have).enumerate() {
                compare(
                    w,
                    h,
                    &format!("{path}/{index}"),
                    skip,
                    differences,
                    mismatches,
                );
            }
        }
        (want, have) => {
            if want != have {
                mismatches.push(format!("{path}:\n  ts:     {want}\n  kernel: {have}"));
            }
        }
    }
}

#[test]
fn every_typescript_answer_is_the_kernel_answer_except_the_declared_divergences() {
    let corpus = corpus();
    let tokens = corpus["divergences"]
        .as_object()
        .expect("divergence vocabulary");
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 25, "the corpus lost cases");
    let mut failures = Vec::new();
    let mut declared = 0;
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let actual = answer(case);
        let mut skip = Vec::new();
        for pointer in case["divergences"].as_array().into_iter().flatten() {
            let text = pointer.as_str().expect("pointer");
            let (path, token) = text.rsplit_once(':').expect("pointer:token");
            assert!(
                tokens.contains_key(token),
                "{id}: divergence {token} is not in the vocabulary"
            );
            skip.push(path.to_owned());
        }
        let mut differences = Vec::new();
        let mut mismatches = Vec::new();
        compare(
            &case["expected_ts"],
            &actual,
            "",
            &skip,
            &mut differences,
            &mut mismatches,
        );
        declared += differences.len();
        if !mismatches.is_empty() {
            failures.push(format!("{id}:\n{}", mismatches.join("\n")));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n\n"));
    assert!(
        declared >= 5,
        "the declared divergences should be exercised; saw {declared}"
    );
}

#[test]
fn the_kernel_answers_the_recorded_answer_byte_for_byte_or_rerecords_it() {
    let mut corpus = corpus();
    let write = std::env::var_os("DS_LAYER_STATE_CORPUS_WRITE").is_some();
    let mut drift = Vec::new();
    let cases = corpus["cases"].as_array_mut().expect("cases");
    for case in cases.iter_mut() {
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
            serde_json::to_string_pretty(&corpus).expect("corpus encodes")
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
fn the_corpus_keeps_the_order_of_keys_the_recorder_wrote() {
    // `preserve_order` is on: a re-recorded corpus must diff minimally.
    let corpus: Map<String, Value> = serde_json::from_str(CORPUS).expect("corpus parses");
    assert_eq!(
        corpus.keys().collect::<Vec<_>>(),
        ["schema", "purpose", "source", "divergences", "cases"]
    );
}
