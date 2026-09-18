//! The named parity proof for contour acquisition decoding.
//!
//! `tests/fixtures/contours-decode/corpus.json` records every case
//! `decode_contours` decides over the tiler's `/v1/contours/fetch` answer: an
//! admitted page (its feature count, source version and the receipt the
//! decoder synthesises, because the tiler sends none), or the exact refusal
//! sentence. The sentences are asserted literally on purpose: a refusal is
//! persisted verbatim as a dataset room's `last_error` and is the only thing
//! an operator sees, so changing one is changing the product.
//!
//! The corpus is the twin of `buildings-decode/corpus.json`. ds-web holds no
//! copy yet: the desktop still retains contours in IndexedDB rather than in a
//! room, and its parity test gains a contours twin when that moves (slice 4
//! of the headless seeding design).

use ds_command_kernel::project_dataset_cache::{
    decode_contours, evaluate, CONTOURS_INDEX_LAYER, CONTOURS_INTERMEDIATE_LAYER, SCHEMA,
};
use serde_json::{json, Value};

const CORPUS: &str = include_str!("fixtures/contours-decode/corpus.json");

fn corpus() -> Value {
    let corpus: Value = serde_json::from_str(CORPUS).expect("corpus parses");
    assert_eq!(corpus["schema"], "ds.contours-decode-parity/v1");
    corpus
}

fn find(corpus: &Value, id: &str) -> Value {
    corpus["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .find(|case| case["id"] == id)
        .unwrap_or_else(|| panic!("{id} is missing from the corpus"))
        .clone()
}

#[test]
fn every_decode_case_answers_exactly_what_the_corpus_records() {
    let corpus = corpus();
    let cases = corpus["cases"].as_array().expect("cases");
    assert!(cases.len() >= 27, "the corpus lost cases");
    for case in cases {
        let id = case["id"].as_str().expect("case id");
        let result = decode_contours(&case["response"], &case["area"]);
        let expect = &case["expect"];
        if expect["ok"] == Value::Bool(true) {
            let page = result.unwrap_or_else(|error| panic!("{id} should be admitted: {error}"));
            assert_eq!(
                page["features"].as_array().expect("features").len() as u64,
                expect["features"].as_u64().expect("expected feature count"),
                "{id} admitted a different number of lines",
            );
            assert_eq!(
                page["sourceVersion"], expect["source_version"],
                "{id} version"
            );
            assert_eq!(
                page["sourceReceipt"], expect["receipt"],
                "{id} must synthesise exactly the recorded receipt",
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

/// Every admitted line keeps the `contour_kind` the print-context assembler
/// splits on, and the split reproduces the tiler's own two layers exactly.
#[test]
fn an_admitted_page_splits_back_into_the_provider_layers() {
    let corpus = corpus();
    for id in [
        "two_kinds_admitted",
        "complete_empty_response",
        "terrarium_fallback_source",
    ] {
        let case = find(&corpus, id);
        let page = decode_contours(&case["response"], &case["area"]).expect(id);
        let features = page["features"].as_array().expect("features");
        let of = |kind: &str| {
            features
                .iter()
                .filter(|feature| feature["properties"]["contour_kind"] == kind)
                .count()
        };
        let provided = |layer: &str| {
            case["response"]["layers"][layer]["features"]
                .as_array()
                .map_or(0, Vec::len)
        };
        assert_eq!(of("index"), provided(CONTOURS_INDEX_LAYER), "{id}");
        assert_eq!(
            of("intermediate"),
            provided(CONTOURS_INTERMEDIATE_LAYER),
            "{id}"
        );
        assert_eq!(of("index") + of("intermediate"), features.len(), "{id}");
    }
}

/// Each distinguishable row fault is its own sentence, no two the same, and
/// every one names the layer and the row.
#[test]
fn the_row_refusals_are_distinguishable_and_name_the_row() {
    let corpus = corpus();
    let mut seen: Vec<String> = Vec::new();
    for id in [
        "properties_not_an_object",
        "polygon_geometry",
        "null_geometry",
        "line_without_coordinates",
        "line_outside_area",
        "unknown_contour_kind",
        "kind_in_the_wrong_layer",
        "missing_contour_kind",
    ] {
        let case = find(&corpus, id);
        let error = case["expect"]["error"].as_str().expect("error").to_owned();
        assert!(!seen.contains(&error), "{id} repeats another refusal");
        assert!(
            error.starts_with("contour line #0"),
            "{id} must name the row: {error}"
        );
        assert!(
            error.contains("in `contours_index`") || error.contains("in `contours_intermediate`"),
            "{id} must name the layer: {error}"
        );
        seen.push(error);
    }
}

/// The area rule is the exact line-polygon test, not a bounding-box test: a
/// contour that straddles the edge of the requested area is admitted, so a
/// straddling row cannot be the cause of a failed acquisition.
#[test]
fn the_area_rule_is_exact_and_keeps_straddling_lines() {
    let corpus = corpus();
    for id in ["line_straddles_east_edge", "multilinestring_admitted"] {
        let case = find(&corpus, id);
        assert!(
            decode_contours(&case["response"], &case["area"]).is_ok(),
            "{id} must be admitted under the intersects rule",
        );
    }
    let outside = find(&corpus, "line_outside_area");
    let error = decode_contours(&outside["response"], &outside["area"]).expect_err("outside");
    assert!(error.contains("rule: intersects"), "{error}");
}

/// A refusal names one row; it never carries the row, and a provider string
/// in it is bounded in characters and stripped of control characters.
#[test]
fn a_refusal_stays_a_bounded_sentence() {
    let long: String = "x".repeat(4_000);
    let area = json!({"type":"Polygon","coordinates":[[[30.,-1.],[30.01,-1.],[30.01,-0.99],[30.,-0.99],[30.,-1.]]]});
    let response = |feature: Value| {
        json!({
            "status": "success",
            "layers": {
                "contours_intermediate": {"type": "FeatureCollection", "features": [feature]},
                "contours_index": {"type": "FeatureCollection", "features": []},
            },
            "source": "rwanda_dem_10m", "source_version": "v1",
            "minor_interval_m": 10, "index_interval_m": 50,
            "requested_spacing_m": 10.0, "effective_spacing_m": 10.0,
            "grid": [4, 4], "sampled_points": 16, "unresolved_points": 0,
            "intermediate_features": 1, "index_features": 0,
        })
    };
    let far = json!({
        "type": "Feature", "id": long,
        "properties": {"contour_kind": "intermediate", "note": long},
        "geometry": {"type": "LineString", "coordinates": [[40., 40.], [40.001, 40.]]},
    });
    let error = decode_contours(&response(far), &area).expect_err("out of area");
    assert!(error.len() < 400, "{} bytes", error.len());
    assert!(error.contains(&"x".repeat(64)) && !error.contains(&"x".repeat(65)));
    let broken = json!({
        "type": "Feature", "id": "up\nhere\ttoo",
        "properties": {"contour_kind": "ma\njor"},
        "geometry": {"type": "LineString", "coordinates": [[30.002, -0.995], [30.004, -0.995]]},
    });
    let error = decode_contours(&response(broken), &area).expect_err("unknown kind");
    assert!(!error.contains('\n') && !error.contains('\t'), "{error}");
    assert!(
        error.contains("`upheretoo`") && error.contains("`major`"),
        "{error}"
    );
}

/// The same answers through the module's own request door, which is what the
/// WASM export and the CLI both reach.
#[test]
fn the_decode_action_answers_the_corpus_through_the_request_door() {
    let corpus = corpus();
    for case in corpus["cases"].as_array().expect("cases") {
        let id = case["id"].as_str().expect("id");
        let request = json!({
            "action": "decode_contours",
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
            assert_eq!(value["sourceReceipt"], case["expect"]["receipt"], "{id}");
        } else {
            assert_eq!(
                answer.expect_err(&format!("{id} refuses")),
                case["expect"]["error"].as_str().expect("error"),
                "{id}",
            );
        }
    }
}

/// A contour is one line, and the decoder admits it as one.
///
/// The tiler joins every marching-squares crossing of one level into
/// continuous polylines, so a line arrives with hundreds of vertices and a
/// closed contour arrives as a ring whose last vertex is its first. Both used
/// to arrive as piles of two-vertex fragments, and a decoder that only ever saw
/// fragments could have been reading the first two coordinates of each. This
/// asserts it reads the whole line, keeps every vertex, and closes the ring.
#[test]
fn a_long_polyline_and_a_closed_ring_are_admitted_whole() {
    let area = json!({"type":"Polygon","coordinates":[[
        [30.0,-2.0],[30.02,-2.0],[30.02,-1.98],[30.0,-1.98],[30.0,-2.0]]]});
    let polyline: Vec<Value> = (0..400)
        .map(|step| {
            json!([
                30.001 + step as f64 * 0.000_04,
                -1.999 - (step % 7) as f64 * 0.000_01
            ])
        })
        .collect();
    let ring: Vec<Value> = (0..=24)
        .map(|step| {
            let angle = std::f64::consts::TAU * step as f64 / 24.;
            json!([30.01 + 0.002 * angle.cos(), -1.99 + 0.002 * angle.sin()])
        })
        .collect();
    let line = |kind: &str, elevation: i64, coordinates: &[Value]| {
        json!({"type":"Feature",
            "properties":{"elevation_m":elevation,"contour_kind":kind,
                "contour_label":if kind == "index" {format!("{elevation} m")} else {String::new()},
                "interval_m":10,"source":"rwanda_dem_10m"},
            "geometry":{"type":"LineString","coordinates":coordinates}})
    };
    let response = json!({
        "status": "success",
        "layers": {
            "contours_intermediate": {"type":"FeatureCollection",
                "features":[line("intermediate", 1_640, &polyline)]},
            "contours_index": {"type":"FeatureCollection",
                "features":[line("index", 1_650, &ring)]},
        },
        "source": "rwanda_dem_10m",
        "source_version": "https://storage.googleapis.com/edcl/reference/rwanda_dem/2026-08-28",
        "minor_interval_m": 10, "index_interval_m": 50,
        "requested_spacing_m": 10.0, "effective_spacing_m": 10.0,
        "grid": [113, 113], "sampled_points": 12769, "unresolved_points": 0,
        "intermediate_features": 1, "index_features": 1,
    });
    let page = decode_contours(&response, &area).expect("both lines are admitted");
    let features = page["features"].as_array().expect("features");
    assert_eq!(features.len(), 2, "two lines, not two piles of fragments");
    let coordinates = |index: usize| {
        features[index]["geometry"]["coordinates"]
            .as_array()
            .expect("coordinates")
            .clone()
    };
    assert_eq!(
        coordinates(0).len(),
        400,
        "every vertex of the line is kept"
    );
    let closed = coordinates(1);
    assert_eq!(closed.len(), 25);
    assert_eq!(
        closed.first(),
        closed.last(),
        "a closed contour stays closed through admission",
    );
}
