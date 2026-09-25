//! The parity corpus of the project assets index (ds-brain
//! `docs/contracts/assets-index.md` §7.1).
//!
//! ds-brain builds, caches and serves each project's assets index (owner
//! ruling, 2026-09-25). Until parity holds, the kernel's cloud-source
//! projection in `src/assets/tree.rs` is the spec the Go builder must answer
//! byte for byte as JSON values, and this corpus is how that is checked: every
//! `tests/fixtures/assets-tree/<case>.request.json` is a `TreeRequest` exactly
//! as `build_value` accepts it, and `<case>.response.json` is its answer. A
//! refused request records `{"refusal": "<message>"}`.
//!
//! `<case>.decoded.json` is the same request after the kernel decoded it and
//! encoded it again: the serde defaults filled in, the skip rules applied.
//! ds-brain builds its requests in Go, so its types must round-trip exactly
//! this way before the kernel and the Go builder can be said to read the same
//! request. The row-bound case alone has none: its 20,001 empty rows prove a
//! refusal, not an encoding.
//!
//! The requests carry only what the index is built from: the cloud sources
//! ds-brain owns, catalogued assets and declared folders. The edge-only
//! sources (`dataset_rooms`, `national_datasets`, `report_rooms`,
//! `local_prints`) never reach ds-brain, `user_data` belongs to one person and
//! is never stored in a shared index, and `query` / `link` / `folder` /
//! `depth` / `kind` are filters the index does not serve — the second test
//! keeps all of them out.
//!
//! `DS_ASSETS_TREE_CORPUS_WRITE=1 cargo test --test assets_tree_corpus`
//! rewrites every response from the current kernel. A rewrite is a change to
//! the contract ds-brain is held to, so the Go copy under
//! `ds-brain/internal/services/testdata/assets-index/` moves with it.

use std::{
    fs,
    path::{Path, PathBuf},
};

use ds_command_kernel::assets::{tree::build_value, TreeRequest, TREE_SOURCE_NAMES};
use serde_json::{json, Value};

const CORPUS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/assets-tree");
const WRITE_ENV: &str = "DS_ASSETS_TREE_CORPUS_WRITE";

/// The sources ds-brain reads for the shared index (contract §2).
const CLOUD_SOURCES: [&str; 15] = [
    "transformers",
    "transformer_versions",
    "design_attachments",
    "grid_models",
    "grid_exports",
    "pm_attachments",
    "pm_tasks",
    "pm_records",
    "pm_parties",
    "compounded",
    "compute_artifacts",
    "solar",
    "survey_media",
    "project_data",
    "print_setups",
];

/// Sources that never enter a shared index: the edge-only four and the
/// per-person `user_data`.
const NOT_INDEXED: [&str; 5] = [
    "dataset_rooms",
    "national_datasets",
    "report_rooms",
    "local_prints",
    "user_data",
];

/// `TreeRequest` members the index does not serve.
const FILTERS: [&str; 5] = ["query", "link", "folder", "depth", "kind"];

/// The case that must hold every cloud source, catalogue rows and folders.
const WHOLE_PROJECT: &str = "whole-project-mixed";

/// The case whose rows are too many to be worth a decoded form.
const ROW_BOUND: &str = "source-row-bound-refused";

struct Case {
    name: String,
    request_path: PathBuf,
    response_path: PathBuf,
    decoded_path: PathBuf,
}

fn cases() -> Vec<Case> {
    let mut cases: Vec<Case> = fs::read_dir(CORPUS_DIR)
        .expect("the corpus directory exists")
        .map(|entry| entry.expect("corpus entry").path())
        .filter_map(|path| {
            let file = path.file_name()?.to_str()?.to_string();
            let name = file.strip_suffix(".request.json")?.to_string();
            let response_path = path.with_file_name(format!("{name}.response.json"));
            let decoded_path = path.with_file_name(format!("{name}.decoded.json"));
            Some(Case {
                name,
                request_path: path,
                response_path,
                decoded_path,
            })
        })
        .collect();
    cases.sort_by(|left, right| left.name.cmp(&right.name));
    cases
}

fn read_json(path: &Path) -> Value {
    let text =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("{}: {error}", path.display()))
}

/// One recorded request decoded exactly as the wire decodes a `TreeRequest`.
fn decode(case: &Case) -> TreeRequest {
    let text = fs::read_to_string(&case.request_path)
        .unwrap_or_else(|error| panic!("{}: {error}", case.name));
    serde_json::from_str(&text)
        .unwrap_or_else(|error| panic!("{}: the request does not decode: {error}", case.name))
}

/// The kernel's answer to one decoded request: `build_value`, or the refusal
/// it names.
fn answer(request: &TreeRequest) -> Value {
    match build_value(request) {
        Ok(value) => value,
        Err(refusal) => json!({ "refusal": refusal }),
    }
}

fn write_json(path: &Path, value: &Value) {
    let mut text = serde_json::to_string_pretty(value).expect("value encodes");
    text.push('\n');
    fs::write(path, text).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
}

fn check_recorded(case: &Case, path: &Path, actual: &Value, what: &str) {
    assert!(
        path.exists(),
        "{} has no recorded {what}; run with {WRITE_ENV}=1",
        case.name
    );
    assert_eq!(
        actual,
        &read_json(path),
        "{} differs from its recorded {what}",
        case.name
    );
}

#[test]
fn every_recorded_request_answers_its_recorded_response() {
    let cases = cases();
    assert!(cases.len() >= 40, "the corpus lost cases: {}", cases.len());
    let write = std::env::var(WRITE_ENV).is_ok_and(|value| value == "1");
    for case in &cases {
        let request = decode(case);
        let answer = answer(&request);
        let decoded = (case.name != ROW_BOUND)
            .then(|| serde_json::to_value(&request).expect("the request encodes"));
        if write {
            write_json(&case.response_path, &answer);
            if let Some(decoded) = &decoded {
                write_json(&case.decoded_path, decoded);
            }
            continue;
        }
        check_recorded(case, &case.response_path, &answer, "response");
        if let Some(decoded) = &decoded {
            check_recorded(case, &case.decoded_path, decoded, "decoded form");
        }
    }
    // No stray response or decoded form without its request.
    for entry in fs::read_dir(CORPUS_DIR).expect("corpus directory") {
        let path = entry.expect("entry").path();
        let file = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let name = file
            .strip_suffix(".response.json")
            .or_else(|| file.strip_suffix(".decoded.json"));
        if let Some(name) = name {
            assert!(
                cases.iter().any(|case| case.name == name),
                "{file} has no request"
            );
        }
    }
}

#[test]
fn the_corpus_carries_only_what_the_index_is_built_from() {
    let mut covered: Vec<&str> = Vec::new();
    for case in cases() {
        let request = read_json(&case.request_path);
        let object = request.as_object().expect("a request is an object");
        for filter in FILTERS {
            assert!(
                !object.contains_key(filter),
                "{} carries `{filter}`, which the index does not serve",
                case.name
            );
        }
        if let Some(sources) = request.get("sources").and_then(Value::as_object) {
            for name in sources.keys() {
                assert!(
                    !NOT_INDEXED.contains(&name.as_str()),
                    "{} carries `{name}`, which never enters a shared index",
                    case.name
                );
                if let Some(known) = CLOUD_SOURCES.iter().find(|source| **source == name) {
                    if !covered.contains(known) {
                        covered.push(known);
                    }
                }
            }
        }
    }
    for source in CLOUD_SOURCES {
        assert!(covered.contains(&source), "no case exercises `{source}`");
        assert!(
            TREE_SOURCE_NAMES.contains(&source),
            "`{source}` is not a tree source"
        );
    }

    // The whole-project case holds every cloud source, catalogue rows and
    // declared folders at once.
    let whole = read_json(&Path::new(CORPUS_DIR).join(format!("{WHOLE_PROJECT}.request.json")));
    for source in CLOUD_SOURCES {
        let rows = whole["sources"][source].as_array();
        assert!(
            rows.is_some_and(|rows| !rows.is_empty()),
            "{WHOLE_PROJECT} lacks rows for `{source}`"
        );
    }
    assert!(whole["assets"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty()));
    assert!(whole["folders"]
        .as_array()
        .is_some_and(|rows| !rows.is_empty()));
}
