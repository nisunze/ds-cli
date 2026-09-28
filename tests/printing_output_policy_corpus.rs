//! The named parity proof for the printing output policy.
//!
//! `tests/fixtures/printing-output-policy-corpus.json` is a byte-identical copy
//! of ds-web's `src/tests/fixtures/printing-output-policy/corpus.json`, and
//! ds-web's `printing-output-policy-parity.test.ts` proves both copies are the
//! same bytes and runs these same rows through the vendored WASM. The corpus
//! holds every distinct shape a project has stored in its design export row,
//! what the deleted TypeScript answered for it (`expected_ts`, recorded by
//! running that code once), and what this kernel answers (`expected`). A row
//! where they differ carries `why`; the kernel is right in every one of them.

use ds_command_kernel::report_formats::{
    inspect_project_settings, stored_output_selection, DesignOutputSelection,
};
use serde_json::{json, Value};

const CORPUS: &str = include_str!("fixtures/printing-output-policy-corpus.json");

#[test]
fn every_stored_output_shape_reads_the_same_in_rust_as_the_corpus_records() {
    let corpus: Value = serde_json::from_str(CORPUS).expect("corpus parses");
    assert_eq!(corpus["schema"], "ds.printing-output-policy-parity/v1");
    let cases = corpus["selection_cases"]
        .as_array()
        .expect("selection cases");
    assert!(cases.len() >= 16, "the corpus lost rows");
    let mut divergences = 0;
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let selection = stored_output_selection(&case["stored"])
            .unwrap_or_else(|error| panic!("{id} must read: {error}"));
        assert_eq!(
            serde_json::to_value(&selection).unwrap(),
            case["expected"],
            "{id} selection"
        );
        assert_eq!(
            serde_json::to_value(selection.placement_map().unwrap()).unwrap(),
            case["placements"],
            "{id} placements"
        );
        assert_eq!(
            serde_json::to_value(selection.tokens().unwrap()).unwrap(),
            case["tokens"],
            "{id} tokens"
        );
        // A row that differs from the deleted TypeScript must say why, and a
        // row that says why must actually differ.
        let same = case["expected_ts"] == case["expected"]
            || strip_empty_execution(&case["expected_ts"]) == case["expected"];
        assert_eq!(
            same,
            case.get("why").is_none(),
            "{id}: a divergence needs a why, and a why needs a divergence"
        );
        divergences += usize::from(!same);
        // Whatever the shape, a stored value round-trips through the selection
        // the host will save back.
        let restored: DesignOutputSelection =
            serde_json::from_value(serde_json::to_value(&selection).unwrap()).unwrap();
        assert_eq!(restored, selection, "{id} round trip");
    }
    assert_eq!(divergences, 6, "the recorded divergence count changed");
}

#[test]
fn the_export_row_is_found_under_every_alias_the_corpus_names() {
    let corpus: Value = serde_json::from_str(CORPUS).expect("corpus parses");
    let value = json!({"schema":"ds.design-output-selection/v1",
        "prints":[{"layout_id":"detail","enabled":true,"formats":["pdf"]}],
        "geospatial":["gpkg"],"tabular":["xlsx"],"execution":{}});
    let layout = ds_command_kernel::printing::named_layout(
        Some("detail".into()),
        Some("Detail".into()),
        ds_command_kernel::printing::Paper::A3,
        ds_command_kernel::printing::Orientation::Landscape,
    )
    .unwrap();
    for case in corpus["sheet_cases"].as_array().expect("sheet cases") {
        let id = case["id"].as_str().expect("case id");
        let rows = match case["parameter"].as_str() {
            Some(parameter) => {
                json!([{"parameter":"unrelated","value":1},{"parameter":parameter,"value":value}])
            }
            None => json!([{"parameter":"unrelated","value":1}]),
        };
        let sheets = json!({"project_settings":rows,
            "printing_setups":[{"id":"detail","revision":"a".repeat(64),"layout":layout}]});
        let inspected = inspect_project_settings(&sheets, None).expect("inspection");
        assert_eq!(inspected["source"], case["expected_source"], "{id} source");
        assert_eq!(inspected["ready"], case["expected_ready"], "{id} ready");
        assert_eq!(
            inspected["outputs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|output| output["outputId"].clone())
                .collect::<Vec<_>>(),
            case["expected_outputs"].as_array().unwrap().clone(),
            "{id} outputs"
        );
        assert!(inspected["refusal"].is_null(), "{id} refusal");
    }
}

/// The deleted TypeScript always wrote an `execution` key, empty when it had
/// nothing to say; the kernel omits an empty matrix. That alone is not a
/// divergence in the answer.
fn strip_empty_execution(value: &Value) -> Value {
    let mut value = value.clone();
    if value["execution"] == json!({}) {
        value.as_object_mut().expect("object").remove("execution");
    }
    value
}
