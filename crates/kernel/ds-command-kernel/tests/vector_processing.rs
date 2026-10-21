use ds_command_kernel::vector_ops::{evaluate, execute, VectorRequest, SCHEMA};
use serde_json::{json, Value};

fn answer(request: &Value) -> Value {
    serde_json::from_str(&evaluate(&serde_json::to_vec(request).unwrap()).unwrap()).unwrap()
}

#[test]
fn native_json_packets_match_installed_four_operation_baseline() {
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/vector_processing_packets.json")).unwrap();
    assert_eq!(fixture["cases"].as_array().unwrap().len(), 7);
    for case in fixture["cases"].as_array().unwrap() {
        let mut request = case["request"].clone();
        request["source"] = fixture["documents"][request["source"].as_str().unwrap()].clone();
        if request.get("against").is_some() {
            request["against"] = fixture["documents"][request["against"].as_str().unwrap()].clone();
        }
        assert_eq!(answer(&request), case["expected"], "{request}");
        let typed: VectorRequest = serde_json::from_value(request).unwrap();
        assert_eq!(execute(&typed).unwrap(), case["expected"]);
    }
}

#[test]
fn json_boundary_refuses_unknown_fields_operations_schemas_and_invalid_parameters() {
    let base = json!({"schema":SCHEMA,"operation":"sample",
        "source":{"type":"LineString","coordinates":[[30.,-2.],[30.001,-2.]]},
        "interval_m":100.});
    for (field, value) in [
        ("unexpected", json!(true)),
        ("radius_m", json!(1)),
        ("operation", json!("clip")),
        ("schema", json!("ds.vector-processing/v2")),
        ("result_projection", json!("pretend_complete")),
    ] {
        let mut request = base.clone();
        request[field] = value;
        assert_eq!(
            evaluate(&serde_json::to_vec(&request).unwrap())
                .unwrap_err()
                .code,
            "vector_request_invalid",
            "{request}"
        );
    }
    for (field, value, code) in [
        ("limit", json!(0), "vector_limit_out_of_range"),
        ("limit", json!(20_001), "vector_limit_out_of_range"),
        ("interval_m", json!(0), "vector_distance_out_of_range"),
        ("source", json!(null), "vector_document_malformed"),
        (
            "source",
            json!({"type":"FeatureCollection","features":[]}),
            "vector_document_empty",
        ),
        (
            "source",
            json!({"type":"Point","coordinates":[30.,-2.]}),
            "vector_no_eligible_feature",
        ),
    ] {
        let mut request = base.clone();
        request[field] = value;
        assert_eq!(
            evaluate(&serde_json::to_vec(&request).unwrap())
                .unwrap_err()
                .code,
            code,
            "{request}"
        );
    }
    let mut typed: VectorRequest = serde_json::from_value(base).unwrap();
    if let VectorRequest::Sample { interval_m, .. } = &mut typed {
        *interval_m = f64::NAN;
    }
    assert_eq!(
        execute(&typed).unwrap_err().code,
        "vector_distance_out_of_range"
    );
}

#[test]
fn complete_produced_projection_preserves_the_input_cut_and_exact_station_numbers() {
    let lines = json!({"type":"FeatureCollection","features":[
        {"type":"Feature","id":"a","geometry":{"type":"LineString","coordinates":[[30.,-2.],[30.002,-2.]]}},
        {"type":"Feature","id":"b","geometry":{"type":"LineString","coordinates":[[30.,-2.01],[30.002,-2.01]]}}
    ]});
    let request = json!({"schema":SCHEMA,"operation":"sample","source":lines,
        "interval_m":50.25,"include_ends":true,"limit":1,"result_projection":"complete_produced"});
    let full = answer(&request);
    assert_eq!(full["processed"], 1);
    assert!(full["produced"].as_u64().unwrap() > 1);
    assert_eq!(
        full["result"]["features"].as_array().unwrap().len(),
        full["produced"].as_u64().unwrap() as usize
    );
    assert_eq!(
        full["result"]["features"][1]["properties"]["distance_m"],
        50.25
    );
    assert!(full["more"].as_str().unwrap().contains("eligible feature"));
    let mut inline_request = request;
    inline_request["result_projection"] = json!("inline");
    let inline = answer(&inline_request);
    assert_eq!(inline["produced"], full["produced"]);
    assert_eq!(inline["result"]["features"].as_array().unwrap().len(), 1);
    assert!(inline["more"].as_str().unwrap().contains(';'));
}

#[test]
fn measure_keeps_whole_document_totals_and_holes_when_the_rows_are_bounded() {
    let source = json!({"type":"FeatureCollection","features":[
        {"type":"Feature","geometry":{"type":"Polygon","coordinates":[
            [[30.,-2.],[30.01,-2.],[30.01,-1.99],[30.,-1.99],[30.,-2.]],
            [[30.002,-1.998],[30.004,-1.998],[30.004,-1.996],[30.002,-1.996],[30.002,-1.998]]]}},
        {"type":"Feature","geometry":{"type":"Point","coordinates":[30.,-2.]}}
    ]});
    let measured =
        answer(&json!({"schema":SCHEMA,"operation":"measure","source":source,"limit":1}));
    assert_eq!(measured["totals"]["features"], 2);
    assert_eq!(measured["features"].as_array().unwrap().len(), 1);
    assert_eq!(measured["features"][0]["holes"], 1);
    let polygon = &source["features"][0]["geometry"];
    let mut outer = polygon.clone();
    outer["coordinates"].as_array_mut().unwrap().truncate(1);
    let outer_measure = answer(&json!({"schema":SCHEMA,"operation":"measure","source":outer}));
    assert!(
        measured["totals"]["area_m2"].as_f64().unwrap()
            < outer_measure["totals"]["area_m2"].as_f64().unwrap()
    );
    assert_eq!(
        measured["totals"]["length_m"],
        outer_measure["totals"]["length_m"]
    );
    assert!(measured["more"].as_str().is_some());
}

#[test]
fn historical_finite_coordinate_and_flat_packet_semantics_are_unchanged() {
    let measured = answer(&json!({"schema":SCHEMA,"operation":"measure",
        "source":{"type":"Point","coordinates":[521000.,9770000.]}}));
    assert_eq!(measured["totals"]["features"], 1);
    assert_eq!(measured["totals"]["length_m"], 0.0);
    assert_eq!(measured["totals"]["area_m2"], 0.0);

    for bad in [vec![30.], vec![30., f64::NAN], vec![f64::INFINITY, -2.]] {
        assert!(ds_geo_lite::points_along_line(&bad, 1., true).is_empty());
        assert!(ds_geo_lite::find_intersections(&bad, &[0., 0., 1., 1.]).is_empty());
        assert!(ds_geo_lite::buffer_geometry(1, &bad, 1., 8).is_empty());
        assert_eq!(ds_geo_lite::spherical_polygon_area_m2(&bad), 0.0);
    }
    assert!(ds_geo_lite::haversine_m(f64::NAN, 0., 0., 0.).is_nan());
    assert!(ds_geo_lite::points_along_line(&[0., 0., 1., 1.], f64::NAN, true).is_empty());
    let signed = ds_geo_lite::points_along_line(&[-0.0, -0.0, 0.0, 0.0], 1., true);
    assert_eq!(signed.len(), 3);
    assert_eq!(signed[0].to_bits(), (-0.0_f64).to_bits());
    assert_eq!(signed[1].to_bits(), (-0.0_f64).to_bits());
    assert!(ds_geo_lite::flat_to_coords(&[300., 200.]).is_some());
    assert_eq!(
        ds_geo_lite::find_intersections(&[0., 0., 1., 1.], &[0., 1., 1., 0.]),
        vec![0.5, 0.5]
    );
    assert!(ds_geo_lite::find_intersections(&[0., 0., 1., 1.], &[0., 0., 1., 1.]).is_empty());
}
