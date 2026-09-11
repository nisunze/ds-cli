//! The named parity proof for building acquisition decoding.
//!
//! `tests/fixtures/buildings-decode/corpus.json` is a byte-identical copy of
//! ds-web's `src/tests/fixtures/buildings-decode/corpus.json`, and ds-web's
//! `buildings-decode-parity.test.ts` proves both copies are the same bytes and
//! runs these same cases through the vendored WASM the browser and the desktop
//! actually load.
//!
//! Each case holds the requested area, a complete provider response and the
//! exact answer — an admitted page, or the exact refusal sentence. The
//! sentences are asserted literally on purpose: `decode_buildings`' refusal is
//! persisted verbatim as a dataset room's `last_error` and is the only thing an
//! operator sees, so changing one is changing the product.
//!
//! Until 2026-09-11 four different facts shared one sentence — "building
//! acquisition contains an invalid or out-of-area footprint" — for a malformed
//! row, a row with no bounds, a non-area geometry and a row the provider
//! returned outside the area it was asked for. A real acquisition failed with
//! that sentence and nothing in the room could say which of the four had
//! happened. The cases below are one per distinguishable case, plus the
//! boundary cases that decide whether the area rule itself is at fault.

use ds_command_kernel::project_dataset_cache::{decode_buildings, evaluate, SCHEMA};
use serde_json::{json, Value};

const CORPUS: &str = include_str!("fixtures/buildings-decode/corpus.json");

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CORPUS).expect("corpus parses");
    assert_eq!(corpus["schema"], "ds.buildings-decode-parity/v1");
    corpus
}

#[test]
fn every_decode_case_answers_exactly_what_the_corpus_records() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 19, "the corpus lost cases");
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let result = decode_buildings(&case["response"], &case["area"]);
        let expect = &case["expect"];
        if expect["ok"] == Value::Bool(true) {
            let page = result.unwrap_or_else(|error| panic!("{id} should be admitted: {error}"));
            assert_eq!(
                page["features"].as_array().expect("features").len() as u64,
                expect["features"].as_u64().expect("expected feature count"),
                "{id} admitted a different number of footprints",
            );
            assert_eq!(
                page["sourceVersion"], expect["source_version"],
                "{id} version"
            );
            assert_eq!(
                page["sourceReceipt"]["coverage"], case["response"]["coverage"],
                "{id} must carry the provider's own completeness receipt",
            );
        } else {
            let error = result.expect_err(&format!("{id} should be refused"));
            assert_eq!(
                error,
                expect["error"].as_str().expect("expected error"),
                "{id}"
            );
        }
    }
}

/// The four cases that used to be one sentence are four different sentences,
/// and no two of them are the same string.
#[test]
fn the_four_decode_refusals_are_distinguishable() {
    let corpus = corpus();
    let mut seen: Vec<&str> = Vec::new();
    for id in [
        "properties_not_an_object",
        "polygon_without_coordinates",
        "line_geometry",
        "footprint_outside_area",
    ] {
        let case = corpus["cases"]
            .as_array()
            .expect("cases")
            .iter()
            .find(|case| case["id"] == id)
            .unwrap_or_else(|| panic!("{id} is missing from the corpus"));
        let error = case["expect"]["error"].as_str().expect("error");
        assert!(!seen.contains(&error), "{id} repeats another refusal");
        assert!(
            error.starts_with("building footprint #0"),
            "{id} must name the row"
        );
        seen.push(error);
    }
}

/// The whole diagnostic question this corpus exists to answer: a footprint that
/// legitimately straddles the edge of a thin area is ADMITTED. `Intersects` is
/// an exact polygon-polygon test, not a bounding-box test, so a straddling row
/// cannot be the cause of a failed acquisition — which is what leaves provider
/// overfetch as the remaining explanation for an out-of-area refusal.
#[test]
fn the_area_rule_is_exact_and_keeps_straddling_footprints() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    let find = |id: &str| {
        cases
            .iter()
            .find(|case| case["id"] == id)
            .unwrap_or_else(|| panic!("{id} is missing"))
            .clone()
    };
    for id in [
        "sliver_overlapping_footprint",
        "footprint_straddles_north_edge",
        "footprint_covers_area_corner",
        "multipolygon_with_holes",
    ] {
        let case = find(id);
        assert!(
            decode_buildings(&case["response"], &case["area"]).is_ok(),
            "{id} must be admitted under the intersects rule",
        );
    }
    // And the exactness has a price the bounding box would not pay: a footprint
    // inside a hole of the requested area shares its bounds and is refused.
    let hole = find("footprint_inside_area_hole");
    let error = decode_buildings(&hole["response"], &hole["area"]).expect_err("in a hole");
    assert!(error.contains("lies outside the requested area"), "{error}");
}

/// A refusal names one row; it never carries the row.
#[test]
fn a_refusal_stays_a_bounded_sentence() {
    let long: String = "x".repeat(4_000);
    let feature = json!({
        "type": "Feature",
        "id": long,
        "properties": {"note": long},
        "geometry": {"type": "Polygon", "coordinates": [[[40., 40.], [40.001, 40.], [40.001, 40.001], [40., 40.001], [40., 40.]]]},
    });
    let area = json!({"type":"Polygon","coordinates":[[[30.,-1.],[30.1,-1.],[30.1,-0.9],[30.,-0.9],[30.,-1.]]]});
    let response = json!({
        "status": "success", "provider": "google",
        "coverage": {"status": "features_returned", "truncated": false},
        "layers": {"buildings_context": {"type": "FeatureCollection", "features": [feature]}},
        "counts": {"buildings_context": 1},
        "source_receipt": {"provider": "google", "dataset_version": "v3"},
    });
    let error = decode_buildings(&response, &area).expect_err("out of area");
    assert!(
        error.len() < 400,
        "a refusal must stay readable: {} bytes",
        error.len()
    );
    assert!(error.contains(&"x".repeat(64)) && !error.contains(&"x".repeat(65)));
}

/// Control characters in a provider identifier never reach the room's status.
#[test]
fn a_quoted_identifier_cannot_break_the_status_line() {
    let feature = json!({
        "type": "Feature",
        "id": "up\nhere\ttoo",
        "properties": {},
        "geometry": {"type": "Polygon", "coordinates": [[[40., 40.], [40.001, 40.], [40.001, 40.001], [40., 40.001], [40., 40.]]]},
    });
    let area = json!({"type":"Polygon","coordinates":[[[30.,-1.],[30.1,-1.],[30.1,-0.9],[30.,-0.9],[30.,-1.]]]});
    let response = json!({
        "status": "success", "provider": "google",
        "coverage": {"status": "features_returned", "truncated": false},
        "layers": {"buildings_context": {"type": "FeatureCollection", "features": [feature]}},
        "counts": {"buildings_context": 1},
        "source_receipt": {"provider": "google", "dataset_version": "v3"},
    });
    let error = decode_buildings(&response, &area).expect_err("out of area");
    assert!(!error.contains('\n') && !error.contains('\t'), "{error}");
    assert!(error.contains("`upheretoo`"), "{error}");
}

/// The same answers through the module's own request door, which is what the
/// WASM export and the CLI both reach.
#[test]
fn the_decode_action_answers_the_corpus_through_the_request_door() {
    let corpus = corpus();
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        let request = json!({
            "action": "decode_buildings",
            "schema": SCHEMA,
            "response": case["response"],
            "area": case["area"],
        });
        let answer = evaluate(&serde_json::to_vec(&request).expect("request"));
        if case["expect"]["ok"] == Value::Bool(true) {
            let value: Value =
                serde_json::from_str(&answer.unwrap_or_else(|error| panic!("{id}: {error}")))
                    .expect("json");
            assert_eq!(
                value["features"].as_array().expect("features").len() as u64,
                case["expect"]["features"].as_u64().expect("count"),
                "{id}",
            );
        } else {
            assert_eq!(
                answer.expect_err(&format!("{id} refuses")),
                case["expect"]["error"].as_str().expect("error"),
                "{id}",
            );
        }
    }
}
