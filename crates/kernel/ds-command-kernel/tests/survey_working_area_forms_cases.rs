//! The working-area form selection's named proof.
//!
//! `tests/fixtures/survey-working-area-forms-cases.json` is a byte-identical
//! copy of ds-web's `src/tests/fixtures/survey-working-area-forms/cases.json`.
//! Each case carries the request (the schema defaults to the module's) and the
//! answer the kernel must give: the parsed answer, or `{"error": "<the exact
//! Err string>"}` when the request itself is refused. ds-web replays the same
//! file through the vendored WASM, so the browser's working area cannot load a
//! form `ds` would not.
//!
//! Re-record `expected` with `DS_SURVEY_WORKING_AREA_FORMS_CASES_WRITE=1 cargo
//! test --test survey_working_area_forms_cases` after a deliberate rule change,
//! and say why in the commit. That also refreshes the ds-web copy when
//! `../ds-web` is present.

use ds_command_kernel::survey_working_area_forms::{evaluate, SCHEMA};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

const CASES: &str = include_str!("fixtures/survey-working-area-forms-cases.json");

fn own_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/survey-working-area-forms-cases.json")
}
fn web_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-web/src/tests/fixtures/survey-working-area-forms/cases.json")
}

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CASES).expect("cases parse");
    assert_eq!(corpus["schema"], "ds.survey-working-area-forms-cases/v1");
    corpus
}

fn answer(case: &Value) -> Value {
    let id = case["id"].as_str().expect("case id");
    let mut request = case["request"].clone();
    if request.get("schema").is_none() {
        request["schema"] = json!(SCHEMA);
    }
    let bytes = serde_json::to_vec(&request).expect("request encodes");
    match evaluate(&bytes) {
        Ok(answer) => {
            serde_json::from_str(&answer).unwrap_or_else(|_| panic!("{id}: answer is not JSON"))
        }
        Err(error) => json!({ "error": error }),
    }
}

#[test]
fn the_kernel_answers_every_recorded_case_or_rerecords_it() {
    let mut corpus = corpus();
    let write = std::env::var_os("DS_SURVEY_WORKING_AREA_FORMS_CASES_WRITE").is_some();
    let mut drift = Vec::new();
    for case in corpus["cases"].as_array_mut().expect("cases") {
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
            serde_json::to_string_pretty(&corpus).expect("cases encode")
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
fn the_ds_web_copy_is_the_same_file() {
    let Ok(shared) = std::fs::read_to_string(web_copy()) else {
        return;
    };
    assert_eq!(
        shared, CASES,
        "the ds-web copy has drifted; re-record with DS_SURVEY_WORKING_AREA_FORMS_CASES_WRITE=1"
    );
}

/// The corpus exercises every op and every refusal the contract names, and
/// the two rules the defect is about: never-chosen loads nothing, and a choice
/// never silently becomes "all".
#[test]
fn the_corpus_covers_every_op_and_refusal_the_contract_names() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    let mut ids = BTreeSet::new();
    let mut ops = BTreeSet::new();
    let mut refusals = BTreeSet::new();
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        assert!(ids.insert(id), "{id} is recorded twice");
        assert!(
            case["why"].as_str().is_some_and(|why| why.len() > 40),
            "{id} does not say why it exists"
        );
        if let Some(kind) = case["request"]["op"]["kind"].as_str() {
            ops.insert(kind.to_owned());
        }
        if let Some(error) = case["expected"]["error"].as_str() {
            if let Some((code, _)) = error.split_once(": ") {
                if !code.contains(' ') {
                    refusals.insert(code.to_owned());
                }
            }
        } else {
            let expected = &case["expected"];
            let loads = expected["loads"].as_array().expect("loads");
            if expected["chosen"] == false {
                assert!(loads.is_empty(), "{id}: never chosen must load nothing");
            }
            if loads.is_empty() {
                assert!(
                    expected["remedy"].as_str().is_some(),
                    "{id}: an empty load names its remedy"
                );
            }
            let forms = expected["forms"].as_array().expect("forms");
            let selected = forms.iter().filter(|row| row["selected"] == true).count();
            assert_eq!(
                selected,
                loads.len(),
                "{id}: loads is exactly the selected rows"
            );
        }
    }
    for op in ["read", "select", "all", "none", "clear"] {
        assert!(ops.contains(op), "no case exercises the {op} op");
    }
    for code in ["unknown_form", "no_forms_named", "project_context_changed"] {
        assert!(
            refusals.contains(code),
            "no case records the {code} refusal"
        );
    }
}
