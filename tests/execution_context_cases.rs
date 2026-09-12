//! The execution context's named proof (project/instance isolation, slice 1).
//!
//! `tests/fixtures/execution-context-cases.json` is a byte-identical copy of
//! ds-web's `src/tests/fixtures/execution-context/cases.json`. Each case
//! carries the op (without the schema wrapper) and the answer the kernel must
//! give: the parsed answer, or `{"error": "<the exact Err string>"}` when the
//! request itself is malformed. ds-web replays the same file through the
//! vendored WASM, so no rule about who may see, admit, keep, queue or recover
//! a project's work can be true on one host and not another.
//!
//! Re-record `expected` with `DS_EXECUTION_CONTEXT_CASES_WRITE=1 cargo test
//! --test execution_context_cases` after a deliberate rule change, and say why
//! in the commit. That also refreshes the ds-web copy when `../ds-web` is
//! present.

use ds_command_kernel::execution_context::{
    evaluate, NOT_FOUND, NOT_VISIBLE, NOT_VISIBLE_MESSAGE, OPERATIONS, PROJECT_NOT_VISIBLE,
    REFUSALS, SCHEMA,
};
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;
use std::path::PathBuf;

const CASES: &str = include_str!("fixtures/execution-context-cases.json");

fn own_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/execution-context-cases.json")
}
fn web_copy() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../ds-web/src/tests/fixtures/execution-context/cases.json")
}

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CASES).expect("cases parse");
    assert_eq!(corpus["schema"], "ds.execution-context-cases/v1");
    corpus
}

/// What the kernel answers for one case: its answer parsed, or its refusal of
/// the request itself as `{"error": ...}`.
fn answer(case: &Value) -> Value {
    let id = case["id"].as_str().expect("case id");
    let schema = case.get("schema").and_then(Value::as_str).unwrap_or(SCHEMA);
    let request = serde_json::to_vec(&json!({"schema": schema, "op": case["request"]}))
        .expect("request encodes");
    match evaluate(&request) {
        Ok(answer) => {
            serde_json::from_str(&answer).unwrap_or_else(|_| panic!("{id}: answer is not JSON"))
        }
        Err(error) => json!({ "error": error }),
    }
}

#[test]
fn the_kernel_answers_every_recorded_case_or_rerecords_it() {
    let mut corpus = corpus();
    let write = std::env::var_os("DS_EXECUTION_CONTEXT_CASES_WRITE").is_some();
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
        // A kernel checkout without its ds-web sibling still proves itself.
        return;
    };
    assert_eq!(
        shared, CASES,
        "the ds-web copy has drifted; re-record with DS_EXECUTION_CONTEXT_CASES_WRITE=1"
    );
}

/// Isolation is a closed vocabulary: every op, every refusal code, every
/// provenance and every outcome the contract names is exercised, and nothing
/// it does not name is answered.
#[test]
fn the_corpus_covers_every_op_refusal_provenance_and_operation() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 55, "the corpus lost cases");
    let mut ids = BTreeSet::new();
    let mut ops = BTreeSet::new();
    let mut refused = BTreeSet::new();
    let mut outcomes = BTreeSet::new();
    let mut provenance = BTreeSet::new();
    let mut operations = BTreeSet::new();
    let mut errors = 0;
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        assert!(ids.insert(id), "{id} is recorded twice");
        assert!(
            case["why"].as_str().is_some_and(|why| why.len() > 40),
            "{id} does not say why it exists"
        );
        let expected = &case["expected"];
        let kind = case["request"]["kind"]
            .as_str()
            .expect("an op names its kind")
            .to_owned();
        // A case that deliberately names no op at all proves the wire is
        // closed; it does not enlarge the vocabulary.
        if expected.get("error").is_none() {
            ops.insert(kind);
        }
        if let Some(code) = expected["refusal"]["code"].as_str() {
            refused.insert(code.to_owned());
        }
        if let Some(outcome) = expected["outcome"].as_str() {
            outcomes.insert(outcome.to_owned());
        }
        if let Some(source) = expected["provenance"].as_str() {
            provenance.insert(source.to_owned());
        }
        if let Some(operation) = expected["context"]["operation"].as_str() {
            operations.insert(operation.to_owned());
        }
        if expected.get("error").is_some() {
            errors += 1;
        }
    }
    assert_eq!(
        ops,
        ["admit", "capacity", "recover", "retain", "visible"]
            .iter()
            .map(|op| (*op).to_owned())
            .collect::<BTreeSet<String>>(),
        "every op is exercised and no other exists"
    );
    assert_eq!(
        refused,
        REFUSALS
            .iter()
            .map(|code| (*code).to_owned())
            .collect::<BTreeSet<String>>(),
        "the refusal vocabulary is closed and every code is proven"
    );
    assert_eq!(
        outcomes,
        ["admitted", "existing", "recovered", "refused", "retained"]
            .iter()
            .map(|outcome| (*outcome).to_owned())
            .collect::<BTreeSet<String>>()
    );
    assert_eq!(
        provenance,
        ["saved_selection", "sealed_input", "stored"]
            .iter()
            .map(|source| (*source).to_owned())
            .collect::<BTreeSet<String>>(),
        "every recovery provenance is proven"
    );
    assert_eq!(
        operations,
        OPERATIONS
            .iter()
            .map(|operation| (*operation).to_owned())
            .collect::<BTreeSet<String>>(),
        "every operation the contract names is admitted at least once"
    );
    assert!(errors >= 4, "malformed requests are part of the contract");
}

/// The proof the whole boundary rests on, asserted over the recorded answers
/// rather than over prose: nothing a caller may not see is ever named back to
/// them, and no refusal discloses another project's existence.
#[test]
fn no_recorded_answer_discloses_what_the_caller_may_not_see() {
    let corpus = corpus();
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let expected = &case["expected"];
        let Some(code) = expected["refusal"]["code"].as_str() else {
            continue;
        };
        let message = expected["refusal"]["message"]
            .as_str()
            .unwrap_or_else(|| panic!("{id}: a refusal says something"));
        if code == NOT_VISIBLE {
            assert_eq!(
                message, NOT_FOUND,
                "{id}: an invisible job answers exactly what an unknown id answers"
            );
            assert_eq!(
                expected,
                &json!({"visible": false, "refusal": {"code": NOT_VISIBLE, "message": NOT_FOUND}}),
                "{id}: an invisible job answers nothing else at all"
            );
        }
        if code == PROJECT_NOT_VISIBLE {
            assert_eq!(
                message, NOT_VISIBLE_MESSAGE,
                "{id}: a stale snapshot and an unauthorized project read alike"
            );
        }
        // A capacity refusal reports counts and limits; naming a project would
        // tell one tenant what another is running.
        if expected["refusal"]["scope"].is_string() {
            for project in ["project-a", "project-b", "project-c"] {
                assert!(
                    !message.contains(project),
                    "{id}: a capacity refusal named {project}"
                );
            }
            assert!(
                expected["refusal"]["retry_after_ms"]
                    .as_u64()
                    .is_some_and(|after| (1_000..=60_000).contains(&after)),
                "{id}: saturation must carry bounded retry guidance"
            );
        }
    }
}

/// A context is immutable, so every answer that returns one returns the same
/// record the case handed in — never a re-stamped or re-scoped copy.
#[test]
fn every_returned_context_is_the_one_that_went_in() {
    let corpus = corpus();
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let request = &case["request"];
        let expected = &case["expected"];
        let (Some(kind), Some(answered)) = (request["kind"].as_str(), expected.get("context"))
        else {
            continue;
        };
        let handed_in = match kind {
            "retain" => request.get("context"),
            "admit" if expected["outcome"] == "existing" => request.get("existing"),
            "recover" if expected["provenance"] == "stored" => request["row"].get("context"),
            _ => None,
        };
        if let Some(handed_in) = handed_in {
            assert_eq!(handed_in, answered, "{id}: the context came back changed");
        }
    }
}

#[test]
fn the_corpus_keeps_the_order_of_keys_the_recorder_wrote() {
    // `preserve_order` is on: a re-recorded corpus must diff minimally.
    let corpus: Map<String, Value> = serde_json::from_str(CASES).expect("cases parse");
    assert_eq!(
        corpus.keys().collect::<Vec<_>>(),
        ["schema", "purpose", "source", "note", "cases"]
    );
}
