//! The named parity proof for the download scope and plan (slice 14a).
//!
//! `tests/fixtures/design-download-corpus.json` is a byte-identical copy of
//! ds-web's `src/tests/fixtures/design-download/corpus.json`; ds-web's
//! `design-download-parity.test.ts` proves the copies match and compares each
//! plan with what `status-table.ts` / `status-downloads.ts` answered before
//! they were deleted. This file proves the NATIVE build — the one `ds design
//! download plan` links — and that the placement rule is an explicit output.

use ds_command_kernel::plan_project_control_artifact_download as plan;
use ds_command_kernel::PROJECT_CONTROL_ARTIFACT_DOWNLOAD_SCHEMA as SCHEMA;
use serde_json::Value;

const CORPUS: &str = include_str!("fixtures/design-download-corpus.json");

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CORPUS).expect("corpus parses");
    assert_eq!(corpus["schema"], "ds.design-download-parity/v1");
    corpus
}

#[test]
fn every_recorded_request_reads_the_same_in_rust_as_the_corpus_records() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 4, "the corpus lost cases");
    for case in cases {
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
fn the_local_copy_wins_its_artifact_name_by_placement_in_either_merge_order() {
    let corpus = corpus();
    let whole = corpus["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| case["id"] == "whole-project")
        .expect("the whole-project case");
    let chosen = |row: &str, artifact: &str| -> (String, String) {
        let entry = whole["expected"]["placement"]
            .as_array()
            .expect("placement")
            .iter()
            .find(|entry| entry["row"] == row && entry["name"] == artifact)
            .unwrap_or_else(|| panic!("{row}/{artifact} has no placement"));
        (
            entry["chosen"].as_str().expect("chosen").to_string(),
            entry["why"].as_str().expect("why").to_string(),
        )
    };
    // The local copy arrived AFTER the record pointer.
    assert_eq!(
        chosen("batikoti", "batikoti_info_tables.xlsx"),
        (
            "file:///local/exports/batikoti_info_tables.xlsx".to_string(),
            "local_wins".to_string()
        )
    );
    // The regression: a record pointer re-listed AFTER the local copy must not
    // steal the row back from the file on disk.
    assert_eq!(
        chosen("local_first", "local_first.kmz"),
        (
            "file:///local/exports/local_first.kmz".to_string(),
            "local_wins".to_string()
        )
    );
    // Two cloud references: the later, refreshed pointer wins.
    assert_eq!(chosen("pointer_race", "pointer_race.xlsx").1, "later_wins");
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
