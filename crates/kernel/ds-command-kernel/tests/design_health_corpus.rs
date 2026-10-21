//! The named parity proof for transformer health.
//!
//! `tests/fixtures/design-health-corpus.json` is a byte-identical copy of
//! ds-web's `src/tests/fixtures/design-health/corpus.json`, and ds-web's
//! `design-health-parity.test.ts` proves both copies are the same bytes and
//! runs these same rows through the vendored WASM. Each case holds a status
//! row, what the deleted `design/health.ts` answered for it (`expected_ts`,
//! recorded by running that code once before deleting it) and what this kernel
//! answers (`expected`). A case where the two differ carries a `why`; the
//! kernel is right in every one of them.
//!
//! `surface_divergences` records the same row as the two OTHER deleted
//! enumerations saw it — the row's permanent-warning scan and the register's
//! project-wide error/warning tables — beside what the one finding list says
//! instead. A recorded divergence that no longer diverges is a proof that has
//! stopped proving anything, so both are asserted.

use ds_command_kernel::design_health::{evaluate, transformer_health_with_run, SCHEMA};
use serde_json::{json, Value};

const CORPUS: &str = include_str!("fixtures/design-health-corpus.json");

/// The fields the deleted TypeScript could express. The kernel's finding also
/// carries `source`, `affected`, `layer` and the bounded detail preview — data
/// the old answer had no room for, not a different answer.
const KERNEL_ONLY: [&str; 5] = [
    "source",
    "affected",
    "layer",
    "detail_parts",
    "detail_total",
];

fn comparable(health: &Value) -> Value {
    let findings: Vec<Value> = health["findings"]
        .as_array()
        .expect("findings")
        .iter()
        .filter(|finding| finding["source"].as_str().unwrap_or("document") == "document")
        .map(|finding| {
            let mut finding = finding.clone();
            let object = finding.as_object_mut().expect("finding is an object");
            for key in KERNEL_ONLY {
                object.remove(key);
            }
            finding
        })
        .collect();
    json!({
        "severity": health["severity"],
        "findings": findings,
        "error_count": health["error_count"],
        "warning_count": health["warning_count"],
    })
}

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CORPUS).expect("corpus parses");
    assert_eq!(corpus["schema"], "ds.design-health-parity/v1");
    corpus
}

#[test]
fn every_recorded_row_reads_the_same_in_rust_as_the_corpus_records() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 56, "the corpus lost rows");
    let mut divergences = 0;
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let health = transformer_health_with_run(&case["row"], case.get("run"));
        let answer = serde_json::to_value(&health).expect("health encodes");
        assert_eq!(answer, case["expected"], "{id}");

        // A case that differs from the deleted TypeScript must say why, and a
        // case that says why must actually differ.
        let same = comparable(&case["expected_ts"]) == comparable(&answer);
        assert_eq!(
            same,
            case.get("why").is_none(),
            "{id}: a divergence needs a why, and a why needs a divergence"
        );
        divergences += usize::from(!same);
    }
    assert_eq!(divergences, 16, "the recorded divergence count changed");
}

#[test]
fn each_deleted_surface_still_disagrees_with_the_one_finding_list() {
    let corpus = corpus();
    let mut recorded = 0;
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let Some(divergences) = case.get("surface_divergences").and_then(Value::as_array) else {
            continue;
        };
        for divergence in divergences {
            let surface = divergence["surface"].as_str().expect("surface");
            assert!(
                matches!(surface, "transformer-info" | "issue-tables"),
                "{id}: unknown surface {surface}"
            );
            assert_ne!(
                divergence["was"], divergence["now"],
                "{id}/{surface}: a recorded divergence that no longer diverges"
            );
            assert!(
                divergence["why"].as_str().is_some_and(|why| why.len() > 40),
                "{id}/{surface}: a divergence needs a why"
            );
            recorded += 1;
        }
    }
    assert_eq!(
        recorded, 28,
        "the recorded surface divergence count changed"
    );
}

#[test]
fn the_fleet_summary_and_its_rows_read_the_same_through_the_closed_door() {
    let corpus = corpus();
    let request = json!({"schema": SCHEMA, "rows": corpus["fleet"]["rows"]});
    let reply: Value =
        serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).expect("fleet"))
            .expect("reply parses");
    assert_eq!(reply["schema"], SCHEMA);
    assert_eq!(reply["rows"], corpus["fleet"]["expected_rows"]);
    assert_eq!(reply["summary"], corpus["fleet"]["expected_summary"]);
}
