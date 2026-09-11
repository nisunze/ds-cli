//! The named parity proof for the deliberate-version prompt (slice 14a).
//!
//! `tests/fixtures/design-version-corpus.json` is a byte-identical copy of
//! ds-web's `src/tests/fixtures/design-version/corpus.json`; ds-web's
//! `design-version-parity.test.ts` proves the copies match and runs the same
//! requests through the vendored WASM against what `transformer-version.ts`
//! answered before it was deleted. This file proves the NATIVE build — the one
//! `ds design version status` links — answers the same recorded bytes.

use ds_command_kernel::design_version::{evaluate, SCHEMA};
use serde_json::Value;

const CORPUS: &str = include_str!("fixtures/design-version-corpus.json");

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CORPUS).expect("corpus parses");
    assert_eq!(corpus["schema"], "ds.design-version-parity/v1");
    corpus
}

fn answer(request: &Value, id: &str) -> Value {
    let bytes = serde_json::to_vec(request).expect("request encodes");
    serde_json::from_str(&evaluate(&bytes).unwrap_or_else(|error| panic!("{id}: {error}")))
        .expect("reply parses")
}

#[test]
fn every_recorded_request_reads_the_same_in_rust_as_the_corpus_records() {
    let corpus = corpus();
    assert_eq!(
        answer(&corpus["facts"]["request"], "facts"),
        corpus["facts"]["expected"]
    );
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 56, "the corpus lost cases");
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let reply = answer(&case["request"], id);
        assert_eq!(reply["schema"], SCHEMA, "{id}");
        assert_eq!(reply, case["expected"], "{id}: {}", case["note"]);
    }
    assert_eq!(
        answer(&corpus["partition"]["request"], "partition"),
        corpus["partition"]["expected"]
    );
    for overwrite in corpus["upload_overwrite"].as_array().expect("overwrites") {
        let id = overwrite["id"].as_str().expect("id");
        assert_eq!(
            answer(&overwrite["request"], id),
            overwrite["expected"],
            "{id}"
        );
    }
}

#[test]
fn the_five_clauses_keep_their_order_across_every_row() {
    let corpus = corpus();
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let (row, gesture) = id.split_once('/').expect("id is row/gesture");
        let reason = case["expected"]["reason"].as_str().unwrap_or_default();
        match gesture {
            "exiting" => assert_eq!(reason, "exiting", "{id}"),
            "blocked" => assert_eq!(reason, "blocked", "{id}"),
            "locally-dirty" => assert_eq!(reason, "locally-dirty", "{id}"),
            _ => {
                // Only the row itself decides once no gesture is in the way.
                let unversionable = ["collisions", "combined"].contains(&row);
                if unversionable {
                    assert_eq!(reason, "unversionable", "{id}");
                }
            }
        }
    }
}

#[test]
fn nothing_diverges_from_the_deleted_typescript() {
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
}
