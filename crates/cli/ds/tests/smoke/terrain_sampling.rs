//! Actual headless file jobs: discovery, defaults, side widths and safe refusals.

use super::native_ds;
use serde_json::{Value, json};

#[test]
fn terrain_sampling_is_native_span_aware_and_sides_are_opt_in() {
    let root = tempfile::tempdir().unwrap();
    let route = root.path().join("route.geojson");
    let survey = root.path().join("survey.csv");
    let request = root.path().join("request.json");
    let out = root.path().join("center");
    std::fs::write(
        &route,
        serde_json::to_vec(&json!({"type":"Feature","id":"span-test",
            "properties":{},"geometry":{"type":"LineString",
                "coordinates":[[29.75,-2.62],[29.751,-2.62]]}}))
        .unwrap(),
    )
    .unwrap();
    // Synthetic constant surface, explicitly a mathematical test fixture.
    let original = b"id,x,y,z\na,29.749,-2.621,1000\nb,29.752,-2.621,1000\nc,29.752,-2.619,1000\nd,29.749,-2.619,1000\n";
    std::fs::write(&survey, original).unwrap();
    let mut model = json!({"route_file":"route.geojson","projected_crs":"EPSG:32735",
        "source":{"kind":"survey_tin","csv_file":"survey.csv",
            "source_crs":"EPSG:4326","x_column":"x","y_column":"y","z_column":"z",
            "max_triangle_edge_m":1000},
        "settings":{"seed":42,"maximum_gap_m":100,"height_tolerance_m":0.01,
            "engineering_intervals":[{"start_station_m":0,"end_station_m":65,"weight_span_m":65}]}});
    let write_request = |value: &Value| {
        std::fs::write(&request, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    };
    write_request(&model);
    let invoke = |destination: &std::path::Path, dry_run: bool| {
        let mut args = vec![
            "data",
            "terrain",
            "sample",
            "--request",
            request.to_str().unwrap(),
            "--out",
            destination.to_str().unwrap(),
            "--output",
            "json",
        ];
        if dry_run {
            args.push("--dry-run");
        }
        native_ds(&args)
    };

    let descriptor = native_ds(&["data", "terrain", "describe", "--output", "json"]);
    assert_eq!(descriptor.code, 0, "{}", descriptor.envelope);
    assert_eq!(
        descriptor.envelope["data"]["defaults"]["settings"]["side_offsets_m"],
        json!([])
    );
    let preview = invoke(&out, true);
    assert_eq!(preview.code, 0, "{}", preview.envelope);
    assert_eq!(preview.envelope["data"]["query_count"], 0);
    assert!(
        preview.envelope["data"]["planned_query_count"]
            .as_u64()
            .unwrap()
            > 0
    );
    assert!(preview.envelope["data"]["route_preview"][0]["supported_profile_samples"].is_null());
    assert!(!out.exists());

    let sampled = invoke(&out, false);
    assert_eq!(sampled.code, 0, "{}", sampled.envelope);
    let results: Value =
        serde_json::from_slice(&std::fs::read(out.join("results.json")).unwrap()).unwrap();
    let result = &results["routes"][0]["result"];
    assert_eq!(results["routes"][0]["route_id"], "span-test");
    assert_eq!(result["side_profiles"], json!([]));
    assert_eq!(result["side_observations"], json!([]));
    assert_eq!(result["source_observations_mutated"], false);
    assert_eq!(
        result["engineering_density"]["intervals"][0]["effective_maximum_gap_m"],
        16.25
    );
    assert!(
        result["engineering_density"]["intervals"][0]["centerline_maximum_sample_gap_m"]
            .as_f64()
            .unwrap()
            <= 16.25 + 1e-8
    );
    for sample in result["centerline"]["samples"].as_array().unwrap() {
        assert!((sample["elevation_m"].as_f64().unwrap() - 1000.0).abs() < 1e-8);
        assert_eq!(sample["row_class"], "derived_interpolated");
    }
    assert_eq!(std::fs::read(&survey).unwrap(), original);
    assert_eq!(
        std::fs::read(out.join("declared-survey-input.csv")).unwrap(),
        original
    );

    model["settings"]["side_offsets_m"] = json!([-6, 6]);
    model["settings"]["side_lateral_jitter_m"] = json!(1.5);
    write_request(&model);
    let side_out = root.path().join("sides");
    let sides = invoke(&side_out, false);
    assert_eq!(sides.code, 0, "{}", sides.envelope);
    let side_results: Value =
        serde_json::from_slice(&std::fs::read(side_out.join("results.json")).unwrap()).unwrap();
    let side_result = &side_results["routes"][0]["result"];
    assert_eq!(result["centerline"], side_result["centerline"]);
    assert_eq!(side_result["side_profiles"].as_array().unwrap().len(), 2);
    for (index, nominal) in [-6.0, 6.0].iter().enumerate() {
        let profile = &side_result["side_profiles"][index];
        assert_eq!(profile["nominal_offset_m"].as_f64().unwrap(), *nominal);
        for sample in profile["samples"].as_array().unwrap() {
            assert_eq!(sample["signed_offset_m"].as_f64().unwrap(), *nominal);
            assert!((sample["elevation_m"].as_f64().unwrap() - 1000.0).abs() < 1e-8);
        }
    }
    assert!(
        side_result["side_observations"]
            .as_array()
            .unwrap()
            .iter()
            .any(|sample| sample["signed_offset_m"] != sample["nominal_side_offset_m"])
    );

    let before = std::fs::read(side_out.join("results.json")).unwrap();
    let existing = invoke(&side_out, false);
    assert_eq!(existing.envelope["error"]["code"], "terrain_output_exists");
    assert_eq!(
        std::fs::read(side_out.join("results.json")).unwrap(),
        before
    );

    model["source"]["breaklines"] = json!([{"vertices":[{"x":1,"y":2,"z":3},{"x":4,"y":5,"z":6}]}]);
    write_request(&model);
    let refused_out = root.path().join("refused");
    let refused = invoke(&refused_out, false);
    assert_eq!(
        refused.envelope["error"]["code"],
        "terrain_breaklines_unsupported"
    );
    assert!(!refused_out.exists());
}
