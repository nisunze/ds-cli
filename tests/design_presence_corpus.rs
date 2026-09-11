//! The named parity proof for lease reconciliation (campaign slice 14a).
//!
//! `tests/fixtures/design-presence-corpus.json` is a byte-identical copy of
//! ds-web's `src/tests/fixtures/design-presence/corpus.json`; ds-web's
//! `design-presence-parity.test.ts` proves the copies match and compares each
//! plan with the recorded four-step pass of `transformer-presence.ts`. This
//! file proves the NATIVE build — the one `ds design presence status` links —
//! plans the same calls, and that the shared budget survived the move.

use ds_command_kernel::design_presence::{evaluate, MAX_LOCK_CALLS_PER_PASS, SCHEMA};
use serde_json::Value;

const CORPUS: &str = include_str!("fixtures/design-presence-corpus.json");

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CORPUS).expect("corpus parses");
    assert_eq!(corpus["schema"], "ds.design-presence-parity/v1");
    corpus
}

fn case<'a>(corpus: &'a Value, id: &str) -> &'a Value {
    corpus["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| case["id"] == id)
        .unwrap_or_else(|| panic!("{id} is missing from the corpus"))
}

#[test]
fn every_recorded_pass_reads_the_same_in_rust_as_the_corpus_records() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 15, "the corpus lost cases");
    for entry in cases {
        let id = entry["id"].as_str().expect("case id");
        let request = serde_json::to_vec(&entry["request"]).expect("request encodes");
        let plan: Value = serde_json::from_str(
            &evaluate(&request).unwrap_or_else(|error| panic!("{id}: {error}")),
        )
        .expect("plan parses");
        assert_eq!(plan["schema"], SCHEMA, "{id}");
        assert_eq!(plan, entry["expected"], "{id}: {}", entry["note"]);
    }
}

#[test]
fn one_budget_is_shared_and_the_holds_spend_it_first() {
    let corpus = corpus();
    let shared = &case(&corpus, "shared-budget")["expected"];
    assert_eq!(
        shared["hold"].as_array().expect("holds").len(),
        MAX_LOCK_CALLS_PER_PASS
    );
    assert!(shared["release"].as_array().expect("releases").is_empty());
    assert_eq!(shared["deferred_count"], 2);
    assert_eq!(shared["budget"]["left"], 0);
    let partial = &case(&corpus, "releases-after-holds")["expected"];
    assert_eq!(partial["hold"].as_array().expect("holds").len(), 19);
    assert_eq!(partial["release"].as_array().expect("releases").len(), 1);
    assert_eq!(partial["deferred_count"], 1);
}

#[test]
fn a_partial_pass_never_releases_a_lease_it_has_no_evidence_about() {
    let corpus = corpus();
    assert!(case(&corpus, "vanished-on-partial")["expected"]["release"]
        .as_array()
        .expect("releases")
        .is_empty());
    assert_eq!(
        case(&corpus, "vanished-on-complete")["expected"]["release"][0]["name"],
        "tx_9"
    );
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
