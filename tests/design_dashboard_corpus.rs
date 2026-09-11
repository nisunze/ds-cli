//! The named parity proof for the Dashboard model (campaign slice 4).
//!
//! `tests/fixtures/design-dashboard-corpus.json` is a byte-identical copy of
//! ds-web's `src/tests/fixtures/design-dashboard/corpus.json`, and ds-web's
//! `design-dashboard-parity.test.ts` proves both copies are the same bytes,
//! runs these same requests through the vendored WASM, and holds the full
//! comparison against the deleted TypeScript (it owns the i18n catalogue that
//! turns the kernel's label keys back into the page's sentences).
//!
//! This file proves the NATIVE build — the one `ds design dashboard` links —
//! answers the same recorded bytes, and that the rules the page can no longer
//! be asked about hold: special rows are excluded, the pile is ranked, and
//! the reply is all keys and no English.

use ds_command_kernel::design_dashboard::{evaluate, SCHEMA};
use serde_json::Value;

const CORPUS: &str = include_str!("fixtures/design-dashboard-corpus.json");

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CORPUS).expect("corpus parses");
    assert_eq!(corpus["schema"], "ds.design-dashboard-parity/v1");
    corpus
}

fn answer(request: &Value) -> Value {
    let bytes = serde_json::to_vec(request).expect("request encodes");
    serde_json::from_str(&evaluate(&bytes).expect("reply")).expect("reply parses")
}

/// JSON has ONE number type; the corpus is written by a reader that has one
/// too, so an exact `1.0` comes back as `1`. Compare numbers as numbers.
fn numeric(value: &Value) -> Value {
    match value {
        Value::Number(number) => serde_json::Number::from_f64(number.as_f64().unwrap_or(0.0))
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Value::Array(items) => Value::Array(items.iter().map(numeric).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(key, held)| (key.clone(), numeric(held)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// Where two answers first differ, as a path — so a failure names the field
/// instead of printing a project.
fn first_difference(left: &Value, right: &Value, path: &str) -> Option<String> {
    match (left, right) {
        (Value::Object(left_map), Value::Object(right_map)) => {
            for (key, held) in left_map {
                let Some(other) = right_map.get(key) else {
                    return Some(format!("{path}.{key} is missing"));
                };
                if let Some(found) = first_difference(held, other, &format!("{path}.{key}")) {
                    return Some(found);
                }
            }
            right_map
                .keys()
                .find(|key| !left_map.contains_key(*key))
                .map(|key| format!("{path}.{key} is unexpected"))
        }
        (Value::Array(left_items), Value::Array(right_items)) => {
            if left_items.len() != right_items.len() {
                return Some(format!(
                    "{path} holds {} entries, not {}",
                    left_items.len(),
                    right_items.len()
                ));
            }
            left_items
                .iter()
                .zip(right_items)
                .enumerate()
                .find_map(|(index, (held, other))| {
                    first_difference(held, other, &format!("{path}[{index}]"))
                })
        }
        _ if left == right => None,
        _ => Some(format!("{path}: {left} is not {right}")),
    }
}

#[track_caller]
fn assert_same(left: &Value, right: &Value, id: &str) {
    if let Some(found) = first_difference(&numeric(left), &numeric(right), id) {
        panic!("{found}");
    }
}

#[test]
fn every_recorded_request_reads_the_same_in_rust_as_the_corpus_records() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 7, "the corpus lost cases");
    let biggest = cases
        .iter()
        .map(|case| case["request"]["rows"].as_array().map_or(0, Vec::len))
        .max()
        .unwrap_or(0);
    assert!(
        biggest >= 80,
        "the corpus lost its real-project-shaped fleet"
    );
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let reply = answer(&case["request"]);
        assert_eq!(reply["schema"], SCHEMA, "{id}");
        assert_same(&reply, &case["expected"], id);
    }
}

#[test]
fn special_rows_travel_with_the_list_and_are_never_counted() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    let specials = cases
        .iter()
        .find(|case| case["id"] == "specials-only")
        .expect("the specials-only case");
    let reply = answer(&specials["request"]);
    assert_eq!(reply["dashboard"]["total"], 0);
    assert_eq!(reply["dashboard"]["health"]["score"], 100);
    assert!(reply["dashboard"]["attention"]
        .as_array()
        .expect("attention")
        .is_empty());
    // And the same names inside a real fleet change nothing but their absence.
    let fleet = cases
        .iter()
        .find(|case| case["id"] == "fleet-standard")
        .expect("the fleet case");
    let rows = fleet["request"]["rows"].as_array().expect("rows");
    let named: Vec<&str> = rows
        .iter()
        .filter_map(|row| row["name"].as_str())
        .filter(|name| {
            matches!(
                *name,
                "combined_transformer" | "mv_data" | "collisions" | "all_transformers"
            )
        })
        .collect();
    assert_eq!(named.len(), 4, "the fleet lost its special rows");
    let reply = answer(&fleet["request"]);
    assert_eq!(
        reply["dashboard"]["total"].as_u64().expect("total") as usize,
        rows.len() - 4
    );
    for note in reply["dashboard"]["attention"]
        .as_array()
        .expect("attention")
    {
        assert!(
            !named.contains(&note["name"].as_str().unwrap_or("")),
            "a special row reached the attention pile"
        );
    }
}

#[test]
fn the_pile_is_ranked_red_then_amber_then_blue_and_then_by_name() {
    let corpus = corpus();
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let reply = answer(&case["request"]);
        let notes = reply["dashboard"]["attention"]
            .as_array()
            .expect("attention");
        let rank = |tone: &str| match tone {
            "red" => 0,
            "amber" => 1,
            _ => 2,
        };
        for pair in notes.windows(2) {
            let before = rank(pair[0]["tone"].as_str().unwrap_or(""));
            let here = rank(pair[1]["tone"].as_str().unwrap_or(""));
            assert!(before <= here, "{id}: the pile lost its rank");
        }
    }
}

#[test]
fn every_label_is_a_key_and_the_only_english_is_the_backend_s_own_sentence() {
    let corpus = corpus();
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let reply = answer(&case["request"]);
        let dashboard = &reply["dashboard"];
        for stage in dashboard["pipeline"].as_array().expect("pipeline") {
            assert!(
                stage["label_key"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("ddash_"),
                "{id}"
            );
        }
        for note in dashboard["attention"].as_array().expect("attention") {
            assert!(
                note["label_key"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("ddash_"),
                "{id}"
            );
            assert!(
                note["detail_key"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("ddash_")
                    || note["detail_key"]
                        .as_str()
                        .unwrap_or("")
                        .starts_with("dsr_"),
                "{id}: an attention note invented a detail"
            );
        }
        for entry in dashboard["governance"].as_array().expect("governance") {
            assert!(
                entry["label_key"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("ddash_"),
                "{id}"
            );
        }
        for fact in dashboard["facts"].as_array().expect("facts") {
            assert!(
                fact["key"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("ddash_fact_"),
                "{id}"
            );
        }
        assert!(
            dashboard["health"]["label_key"]
                .as_str()
                .unwrap_or("")
                .starts_with("ddash_health_"),
            "{id}"
        );
    }
}

#[test]
fn the_declared_divergences_are_the_whole_list_and_each_one_says_why() {
    let corpus = corpus();
    let notes = corpus["divergence_notes"].as_object().expect("notes");
    let mut declared: Vec<&str> = Vec::new();
    for case in corpus["cases"].as_array().expect("cases") {
        let Some(fields) = case["divergences"]["fields"].as_array() else {
            continue;
        };
        for field in fields {
            let field = field.as_str().expect("field name");
            let why = notes[field].as_str().unwrap_or("");
            assert!(
                why.len() > 80,
                "{field} declares a divergence with no why worth reading"
            );
            if !declared.contains(&field) {
                declared.push(field);
            }
        }
    }
    declared.sort_unstable();
    assert_eq!(
        declared,
        [
            "attention",
            "crew",
            "districts",
            "errorsByUser",
            "health",
            "latest"
        ],
        "a divergence appeared or vanished without being declared"
    );
}

#[test]
fn a_request_over_the_bound_or_with_the_wrong_schema_is_refused() {
    assert!(evaluate(b"{}").is_err());
    assert!(evaluate(br#"{"schema":"nope","rows":[]}"#).is_err());
    // An unknown field is a contract break, not a silent pass.
    assert!(evaluate(br#"{"schema":"ds.design-dashboard/v1","rows":[],"nope":1}"#).is_err());
}
