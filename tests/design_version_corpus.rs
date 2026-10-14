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
fn accepted_baseline_migration_accounts_for_every_historical_difference() {
    let corpus = corpus();
    let migration = &corpus["baseline_migration"];
    assert_eq!(
        migration["source_commit"],
        "a8f638621689d8abecf02914d7d4322a0915fb9d"
    );
    assert_eq!(
        migration["added_fact_fields"],
        serde_json::json!(["baseline_ready"])
    );
    let fields = [
        "version",
        "latest",
        "next",
        "count",
        "label",
        "restored",
        "unversioned",
        "reason",
        "changed_since_version",
        "versionable",
    ];
    let facts = corpus["facts"]["expected"]["versions"].as_array().unwrap();
    assert_eq!(migration["facts"].as_array().unwrap().len(), facts.len());
    for (index, now) in facts.iter().enumerate() {
        let was = &corpus["facts"]["expected_ts"][index];
        let changed: Vec<&str> = fields
            .into_iter()
            .filter(|field| now[field] != was[field])
            .collect();
        assert_eq!(migration["facts"][index]["index"], index);
        assert_eq!(
            migration["facts"][index]["fields"],
            serde_json::json!(changed)
        );
        assert_eq!(now["baseline_ready"], false);
        assert!(now["changed_since_version"].is_null());
        let label = if now["version"] == 0 {
            "baseline_required".into()
        } else {
            format!("v{}", now["version"])
        };
        assert_eq!(now["label"], label);
    }
    let declared: Vec<&str> = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case.get("why").is_some())
        .map(|case| case["id"].as_str().unwrap())
        .collect();
    assert_eq!(declared, ["unchanged-since-lead/plain"]);
    assert_eq!(migration["prompt_divergences"], serde_json::json!(declared));
}

#[test]
fn pinned_zero_and_each_missing_baseline_fact_have_exact_native_answers() {
    let corpus = corpus();
    let cases = corpus["baseline_cases"].as_array().unwrap();
    assert_eq!(cases.len(), 9);
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let facts = answer(
            &serde_json::json!({"schema":SCHEMA,"op":"facts","rows":[case["row"]]}),
            id,
        );
        assert_eq!(
            facts,
            serde_json::json!({"schema":SCHEMA,"op":"facts","versions":[case["expected_facts"]]}),
            "{id}"
        );
        let prompt = answer(
            &serde_json::json!({"schema":SCHEMA,"op":"prompt","name":"tx_baseline","row":case["row"]}),
            id,
        );
        assert_eq!(prompt, case["expected_prompt"], "{id}");
    }
}
