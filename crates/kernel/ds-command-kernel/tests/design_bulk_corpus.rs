//! The named parity proof for bulk admissibility (campaign slice 14a).
//!
//! `tests/fixtures/design-bulk-corpus.json` is a byte-identical copy of
//! ds-web's `src/tests/fixtures/design-bulk/corpus.json`; ds-web's
//! `design-bulk-parity.test.ts` proves the copies match and compares each plan
//! with the three `$derived` folds of `StatusTable.svelte` before they were
//! deleted. This file proves the NATIVE build — the one `ds design bulk plan`
//! links — answers the same recorded bytes.

use ds_command_kernel::plan_project_control_bulk_action as plan;
use ds_command_kernel::PROJECT_CONTROL_BULK_PLAN_SCHEMA as SCHEMA;
use serde_json::Value;

const CORPUS: &str = include_str!("fixtures/design-bulk-corpus.json");

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CORPUS).expect("corpus parses");
    assert_eq!(corpus["schema"], "ds.design-bulk-parity/v1");
    corpus
}

fn all_cases(corpus: &Value) -> Vec<&Value> {
    corpus["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .chain(
            corpus["verbs_without_a_predecessor"]
                .as_array()
                .expect("extra verbs")
                .iter(),
        )
        .collect()
}

#[test]
fn every_recorded_request_reads_the_same_in_rust_as_the_corpus_records() {
    let corpus = corpus();
    assert!(
        corpus["cases"].as_array().expect("cases").len() >= 18,
        "the corpus lost cases"
    );
    for case in all_cases(&corpus) {
        let id = case["id"].as_str().expect("case id");
        let request = serde_json::to_vec(&case["request"]).expect("request encodes");
        let answer: Value =
            serde_json::from_str(&plan(&request).unwrap_or_else(|error| panic!("{id}: {error}")))
                .expect("plan parses");
        assert_eq!(answer["schema"], SCHEMA, "{id}");
        assert_eq!(answer, case["expected"], "{id}: {}", case["note"]);
    }
}

#[test]
fn every_row_is_accounted_for_as_a_target_or_a_named_skip() {
    let corpus = corpus();
    for case in all_cases(&corpus) {
        let id = case["id"].as_str().expect("case id");
        let mut rows: Vec<&str> = case["request"]["rows"]
            .as_array()
            .expect("rows")
            .iter()
            .map(|row| row["name"].as_str().expect("name"))
            .collect();
        let expected = &case["expected"];
        let mut named: Vec<&str> = expected["targets"]
            .as_array()
            .expect("targets")
            .iter()
            .chain(expected["skipped"].as_array().expect("skipped").iter())
            .map(|row| row["name"].as_str().expect("name"))
            .collect();
        rows.sort_unstable();
        named.sort_unstable();
        assert_eq!(rows, named, "{id}");
        let fresh = expected["fresh"].as_u64().expect("fresh");
        let stale = expected["stale"].as_u64().expect("stale");
        assert_eq!(
            fresh + stale,
            expected["targets"].as_array().expect("targets").len() as u64,
            "{id}"
        );
    }
}

#[test]
fn a_capability_the_operator_lacks_refuses_the_whole_verb_by_name() {
    let corpus = corpus();
    let ungated = corpus["verbs_without_a_predecessor"]
        .as_array()
        .expect("extra verbs")
        .iter()
        .find(|case| case["id"] == "generate_reports/no-capability")
        .expect("the ungated case");
    assert_eq!(ungated["expected"]["unavailable"], "capability_missing");
    assert!(ungated["expected"]["targets"]
        .as_array()
        .expect("targets")
        .is_empty());
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
