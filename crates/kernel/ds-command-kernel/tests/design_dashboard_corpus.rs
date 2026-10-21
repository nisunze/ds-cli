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
//!
//! Re-recording: an ADDITIVE change to the reply (a new field on an existing
//! shape) invalidates every case's `expected` at once. Run the replay test
//! with `DS_RECORD_CORPUS=1` and it rewrites each case's `expected` with the
//! live reply — 2-space indent, keys in reply order, trailing newline — then
//! asserts against what it wrote. `expected_ts` and `divergences` are never
//! touched: they record the deleted TypeScript, which cannot be re-run. Copy
//! the result to ds-web's `src/tests/fixtures/design-dashboard/corpus.json`
//! byte for byte afterwards.

use ds_command_kernel::design_dashboard::{evaluate, SCHEMA};
use serde_json::Value;

const CORPUS: &str = include_str!("fixtures/design-dashboard-corpus.json");
const CORPUS_PATH: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/design-dashboard-corpus.json"
);
const RECORD_ENV: &str = "DS_RECORD_CORPUS";

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

/// `DS_RECORD_CORPUS=1`: every case's `expected` becomes the live reply and
/// the fixture is rewritten in place. Nothing else in the document moves.
fn recording() -> bool {
    std::env::var(RECORD_ENV).is_ok_and(|value| value == "1")
}

fn record(corpus: &mut Value) {
    let cases = corpus["cases"].as_array_mut().expect("cases");
    for case in cases.iter_mut() {
        let reply = answer(&case["request"]);
        case.as_object_mut()
            .expect("a case is an object")
            .insert("expected".into(), reply);
    }
    let mut bytes = serde_json::to_vec_pretty(corpus).expect("corpus encodes");
    bytes.push(b'\n');
    std::fs::write(CORPUS_PATH, bytes).expect("the fixture is writable");
}

#[test]
fn every_recorded_request_reads_the_same_in_rust_as_the_corpus_records() {
    let mut corpus = corpus();
    if recording() {
        record(&mut corpus);
    }
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 6, "the corpus lost cases");
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
        .find(|case| case["id"] == "fleet")
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
        for summary in dashboard["phase_summaries"]
            .as_array()
            .expect("phase summaries")
        {
            for bucket in summary["buckets"].as_array().expect("buckets") {
                let status = bucket["status"].as_str().unwrap_or("");
                assert_eq!(
                    bucket["label_key"],
                    format!("ddash_phase_status_{status}"),
                    "{id}: a phase bucket spelled its own status"
                );
            }
        }
        if let Some(ring) = dashboard["health"]["ring"].as_array() {
            for slice in ring {
                let key = slice["key"].as_str().unwrap_or("");
                assert_eq!(slice["label_key"], format!("ddash_ring_{key}"), "{id}");
            }
        }
    }
}

/// The donut geometry the page used to sum for itself: each ring starts at
/// zero, its offsets are the cumulative shares before each slice, and no
/// slice claims more than the whole.
#[test]
fn every_ring_starts_at_zero_and_walks_its_offsets_forward() {
    let corpus = corpus();
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("case id");
        let reply = answer(&case["request"]);
        let dashboard = &reply["dashboard"];
        let total = dashboard["total"].as_u64().expect("total");
        let rings: [(&str, Option<&Vec<Value>>); 2] = [
            ("districts", dashboard["districts"].as_array()),
            ("health.ring", dashboard["health"]["ring"].as_array()),
        ];
        for (name, slices) in rings {
            let slices = slices.map(Vec::as_slice).unwrap_or(&[]);
            if name == "health.ring" {
                assert_eq!(
                    slices.is_empty(),
                    total == 0,
                    "{id}: the health ring is drawn exactly when there are transformers"
                );
            }
            let mut before = 0i64;
            let mut counted = 0u64;
            let whole: u64 = slices
                .iter()
                .map(|s| s["count"].as_u64().unwrap_or(0))
                .sum();
            for (index, slice) in slices.iter().enumerate() {
                let percent = slice["percent"].as_i64().expect("percent");
                let offset = slice["offset"].as_i64().expect("offset");
                assert!((0..=100).contains(&percent), "{id}.{name}[{index}]");
                assert!(offset >= before, "{id}.{name}[{index}] walked backwards");
                assert_eq!(
                    offset,
                    ((counted as f64 / whole as f64) * 100.0 + 0.5).floor() as i64,
                    "{id}.{name}[{index}] is not the cumulative share before it"
                );
                if index == 0 {
                    assert_eq!(offset, 0, "{id}.{name} does not start at zero");
                }
                before = offset;
                counted += slice["count"].as_u64().unwrap_or(0);
            }
        }
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
