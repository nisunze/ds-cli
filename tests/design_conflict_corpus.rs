//! The named parity proof for overwrite admissibility (campaign slice 14a).
//!
//! `tests/fixtures/design-conflict-corpus.json` is a byte-identical copy of
//! ds-web's `src/tests/fixtures/design-conflict/corpus.json`, and ds-web's
//! `design-conflict-parity.test.ts` proves both copies are the same bytes,
//! runs these same requests through the vendored WASM, and owns the rename
//! table that makes the kernel's refusal CODES comparable with the English
//! sentences the browser carried.
//!
//! This file proves the NATIVE build — the one `ds design conflict` links —
//! answers the same recorded bytes, and that the nine refusals keep their
//! order.

use ds_command_kernel::design_conflict::{evaluate, SCHEMA};
use serde_json::Value;

const CORPUS: &str = include_str!("fixtures/design-conflict-corpus.json");

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CORPUS).expect("corpus parses");
    assert_eq!(corpus["schema"], "ds.design-conflict-parity/v1");
    corpus
}

#[test]
fn every_recorded_request_reads_the_same_in_rust_as_the_corpus_records() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 14, "the corpus lost cases");
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let request = serde_json::to_vec(&case["request"]).expect("request encodes");
        let answer: Value = serde_json::from_str(
            &evaluate(&request).unwrap_or_else(|error| panic!("{id}: {error}")),
        )
        .expect("reply parses");
        assert_eq!(answer["schema"], SCHEMA, "{id}");
        assert_eq!(answer, case["expected"], "{id}: {}", case["note"]);
    }
    let detect = &corpus["detect"];
    let request = serde_json::to_vec(&detect["request"]).expect("request encodes");
    let answer: Value = serde_json::from_str(&evaluate(&request).expect("detect answers")).unwrap();
    assert_eq!(answer, detect["expected"]);
}

#[test]
fn the_refusals_keep_the_order_the_review_numbered_them_in() {
    let corpus = corpus();
    let mut seen: Vec<u64> = Vec::new();
    for case in corpus["cases"].as_array().expect("cases") {
        if let Some(ordinal) = case["expected"]["refusal"]["ordinal"].as_u64() {
            seen.push(ordinal);
        }
    }
    // Every one of the nine send refusals is exercised, and the tenth is the
    // tick gate's alone.
    let mut ordinals: Vec<u64> = seen.clone();
    ordinals.sort_unstable();
    ordinals.dedup();
    assert_eq!(ordinals, (1..=9).collect::<Vec<u64>>());
    let tick_only: Vec<&str> = corpus["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .filter(|case| case["expected"]["tick_refusal"]["code"] == "review_not_finished")
        .map(|case| case["id"].as_str().expect("case id"))
        .collect();
    assert_eq!(tick_only, ["review-not-finished"]);
}

#[test]
fn the_only_declared_divergence_is_the_detection_of_special_rows() {
    let corpus = corpus();
    let declared: Vec<&str> = corpus["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .filter(|case| case.get("why").is_some())
        .map(|case| case["id"].as_str().expect("case id"))
        .collect();
    assert!(
        declared.is_empty(),
        "{declared:?} newly declare a divergence"
    );
    let why = corpus["detect"]["why"]
        .as_str()
        .expect("the detect divergence is declared");
    assert!(why.len() > 80, "a divergence has to say why");
    let names: Vec<&str> = corpus["detect"]["expected"]["conflicts"]
        .as_array()
        .expect("conflicts")
        .iter()
        .map(|row| row["name"].as_str().expect("name"))
        .collect();
    assert!(
        !names.contains(&"collisions"),
        "the analysis document is never conflicted"
    );
}
