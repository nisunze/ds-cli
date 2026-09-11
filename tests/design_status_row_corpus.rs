//! The named parity proof for status row truth (campaign slice 3).
//!
//! `tests/fixtures/design-status-row-corpus.json` is a byte-identical copy of
//! ds-web's `src/tests/fixtures/design-status-row/corpus.json`, and ds-web's
//! `design-status-row-parity.test.ts` proves both copies are the same bytes,
//! runs these same requests through the vendored WASM, and holds the full
//! comparison against the deleted TypeScript (it owns the rename tables that
//! make English labels and i18n keys comparable).
//!
//! This file proves the NATIVE build — the one `ds design status` links —
//! answers the same recorded bytes, and that the row-identity table
//! reproduces every membership of the three name sets it replaced.

use ds_command_kernel::design_status_row::{evaluate, SCHEMA};
use serde_json::Value;

const CORPUS: &str = include_str!("fixtures/design-status-row-corpus.json");

/// The cases whose answer deliberately differs from the deleted TypeScript.
/// A case that starts or stops diverging has to say so here and in the
/// fixture both readers share.
const DIVERGENT: [&str; 3] = [
    "gov-open-local-lock",
    "retry-refused-with-saved-layers",
    "special-cased-spelling",
];

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CORPUS).expect("corpus parses");
    assert_eq!(corpus["schema"], "ds.design-status-row-parity/v1");
    corpus
}

#[test]
fn every_recorded_request_reads_the_same_in_rust_as_the_corpus_records() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 65, "the corpus lost cases");
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
}

#[test]
fn each_declared_divergence_is_declared_and_nothing_else_diverges() {
    let corpus = corpus();
    let declared: Vec<&str> = corpus["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .filter(|case| case.get("why").is_some())
        .map(|case| case["id"].as_str().expect("case id"))
        .collect();
    assert_eq!(declared, DIVERGENT, "the declared divergence list changed");
    for case in corpus["cases"].as_array().expect("cases") {
        let Some(why) = case.get("why").and_then(Value::as_str) else {
            continue;
        };
        assert!(
            why.len() > 80,
            "{}: a divergence needs a why that says why",
            case["id"]
        );
    }
}

/// `design::row_kind` + `row_allows` replaced `SPECIAL_ROW_NAMES`,
/// `COMBINED_AGGREGATE_NAMES`, `UNVERSIONABLE_NAMES`,
/// `project_control::is_special_status_row` and
/// `report::is_reserved_report_identity`. Every membership of all five is
/// reproduced, or the row says why it moved.
#[test]
fn the_row_identity_table_reproduces_every_membership_it_replaces() {
    let corpus = corpus();
    let specials = corpus["specials"].as_array().expect("specials");
    assert!(specials.len() >= 12, "the name table lost names");
    let mut moved = 0;
    for entry in specials {
        let name = entry["name"].as_str().expect("name");
        let request = serde_json::json!({
            "schema": "ds.design-status-row/v1",
            "server_rows": [{"name": name}],
        });
        let answer: Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).expect("reply"))
                .expect("reply parses");
        let row = &answer["rows"][0];
        assert_eq!(row["kind"], entry["expected"]["kind"], "{name} kind");
        assert_eq!(row["allows"], entry["expected"]["allows"], "{name} allows");

        // The shape the three deleted sets could express.
        let as_the_sets_saw_it = serde_json::json!({
            "special": row["kind"] != "individual",
            "combinable": row["allows"]["combinable"],
            "versionable": row["allows"]["versionable"],
        });
        let same = as_the_sets_saw_it == entry["expected_ts"];
        assert_eq!(
            same,
            entry.get("why").is_none(),
            "{name}: a divergence needs a why, and a why needs a divergence"
        );
        moved += usize::from(!same);
        // `is_reserved_report_identity` is the same vocabulary now.
        assert_eq!(
            ds_command_kernel::report::is_reserved_report_identity(name),
            !row["allows"]["reportable"].as_bool().expect("reportable"),
            "{name}: the report lane reads the one vocabulary"
        );
    }
    assert_eq!(moved, 5, "the recorded name-table divergence count changed");
}

#[test]
fn the_door_refuses_what_it_should() {
    // Wrong schema, unknown field, missing rows, and the byte bound.
    assert!(evaluate(br#"{"schema":"other","server_rows":[]}"#).is_err());
    assert!(
        evaluate(br#"{"schema":"ds.design-status-row/v1","server_rows":[],"nope":1}"#).is_err()
    );
    assert!(evaluate(br#"{"schema":"ds.design-status-row/v1"}"#).is_err());
    assert!(evaluate(&vec![b' '; 32 * 1024 * 1024 + 1]).is_err());
    // A row that is not an object is a row with no name, never a panic.
    let reply: Value = serde_json::from_str(
        &evaluate(br#"{"schema":"ds.design-status-row/v1","server_rows":[null,3,"x"]}"#)
            .expect("reply"),
    )
    .expect("parses");
    assert_eq!(reply["rows"].as_array().expect("rows").len(), 1);
}
