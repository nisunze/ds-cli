use super::*;
use serde_json::{json, Value};

/// The writer owner's own styled-KML fixture: points, lines and polygons with
/// Google Earth styles, plus a second plain point layer.
fn fixture_layers() -> Vec<Value> {
    let envelope = ds_io::kml_text_to_layer(ds_io::GeoTextImportInput {
        name: "earth.kml".into(),
        text: include_str!(
            "../tests/fixtures/google-earth-styles.kml"
        )
        .into(),
    })
    .unwrap();
    let mut layers: Vec<Value> = envelope
        .layers
        .iter()
        .map(|layer| {
            json!({"name": layer.name, "geojson": {"type": "FeatureCollection", "features": layer.features}})
        })
        .collect();
    layers.push(json!({"name": "poles", "geojson": {"type": "FeatureCollection", "features": [
        {"type": "Feature", "geometry": {"type": "Point", "coordinates": [30.06, -1.95]},
         "properties": {"pole_number": 1, "struct_type": "9m 150daN", "remarks": "Kayonza, \"east\"; ü"}},
        {"type": "Feature", "geometry": {"type": "Point", "coordinates": [30.07, -1.96]},
         "properties": {"pole_number": 2, "existing": true, "remarks": null}}
    ]}}));
    layers
}

fn dashboard() -> Value {
    json!({
        "summary": {"title": "LV dashboard", "project": "Rwanda template", "generated_at": "2026-10-22T08:00:00Z",
                    "kpis": [{"label": "Transformers", "value": "12"}, {"label": "Customers", "value": "1,204"}]},
        "sheets": [
            {"name": "Transformers", "headers": ["name", "kva", "approved"], "rows": [["TR-1", 100, true], ["TR-2", 50.5, false]]},
            {"name": "Poles/Lines: long name over thirty-one characters", "headers": ["layer", "count"], "rows": [["lv_poles", 812], ["lv_lines", null]]}
        ]
    })
}

/// The writer owner on the engine's view of a request: every object
/// key-sorted (BTreeMap), its GeoJSON objects emitted key-sorted.
fn engine_bytes(request: &Value) -> Vec<u8> {
    let mut request = request.clone();
    request.sort_all_objects();
    let request = &request;
    let layers = || GeoLayerExportInput {
        layers: serde_json::from_value(request["layers"].clone()).unwrap(),
    };
    match request["format"].as_str().unwrap() {
        "geojson" => {
            let mut value: Value =
                serde_json::from_str(&ds_io::layers_to_geojson_string(layers()).unwrap()).unwrap();
            value.sort_all_objects();
            serde_json::to_string_pretty(&value).unwrap().into_bytes()
        }
        "xlsx" => ds_io::layers_to_xlsx(layers()).unwrap(),
        "delimited" => ds_io::layers_to_delimited(layers(), request["separator"].as_str().unwrap())
            .unwrap()
            .into_bytes(),
        "kmz" => ds_io::layers_to_kmz(layers()).unwrap(),
        "shp_zip" => ds_io::layers_to_shp_zip(layers()).unwrap(),
        "dashboard" => {
            let mut input = request.clone();
            input.as_object_mut().unwrap().remove("format");
            ds_io::dashboard_to_xlsx(serde_json::from_value(input).unwrap()).unwrap()
        }
        other => panic!("unknown format {other}"),
    }
}

fn requests() -> Vec<Value> {
    let layers = fixture_layers();
    let mut dashboard = dashboard();
    dashboard["format"] = json!("dashboard");
    vec![
        json!({"format": "geojson", "layers": layers}),
        json!({"format": "xlsx", "layers": layers}),
        json!({"format": "delimited", "layers": layers, "separator": ","}),
        json!({"format": "delimited", "layers": layers, "separator": "\t"}),
        json!({"format": "kmz", "layers": layers}),
        json!({"format": "shp_zip", "layers": layers}),
        dashboard,
    ]
}

#[test]
fn every_format_returns_the_writer_owner_bytes_and_decodes() {
    for request in requests() {
        let wire = serde_json::to_vec(&request).unwrap();
        let bytes = execute_bytes(&wire).unwrap();
        assert_eq!(bytes, engine_bytes(&request), "{}", request["format"]);
        // Writers are deterministic: no clock, no randomness.
        assert_eq!(execute_bytes(&wire).unwrap(), bytes);
        let format = decode_request(&wire).unwrap().format();
        let receipt = decode_reply(format, &bytes).unwrap();
        assert_eq!(receipt.byte_length, bytes.len());
        assert_eq!(
            receipt.text,
            matches!(
                format,
                LayerExportFormat::Geojson | LayerExportFormat::Delimited
            )
        );
    }
}

#[test]
fn an_absent_separator_is_a_comma() {
    let layers = fixture_layers();
    let absent = execute_bytes(
        &serde_json::to_vec(&json!({"format": "delimited", "layers": layers})).unwrap(),
    )
    .unwrap();
    let comma = execute_bytes(
        &serde_json::to_vec(&json!({"format": "delimited", "layers": layers, "separator": ","}))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(absent, comma);
}

#[test]
fn malformed_requests_are_refused_by_name() {
    for request in [
        json!({"format": "pdf", "layers": []}),
        json!({"layers": []}),
        json!({"format": "xlsx", "layers": [{"name": "a"}]}),
        json!({"format": "dashboard", "summary": {}}),
        json!({"format": "kmz", "layers": "poles"}),
    ] {
        let error = execute_bytes(&serde_json::to_vec(&request).unwrap()).unwrap_err();
        assert!(
            error.to_string().starts_with("export_request_invalid: "),
            "{request}: {error}"
        );
    }
    assert!(execute_bytes(b"{").is_err());
}

#[test]
fn incompatible_replies_are_refused_by_the_decoder() {
    let layers = fixture_layers();
    let kmz =
        execute_bytes(&serde_json::to_vec(&json!({"format": "kmz", "layers": layers})).unwrap())
            .unwrap();
    let xlsx =
        execute_bytes(&serde_json::to_vec(&json!({"format": "xlsx", "layers": layers})).unwrap())
            .unwrap();
    for (format, bytes) in [
        (LayerExportFormat::Geojson, b"[1, 2]".to_vec()),
        (LayerExportFormat::Geojson, vec![0xff, 0xfe]),
        (LayerExportFormat::Delimited, Vec::new()),
        (LayerExportFormat::Xlsx, kmz.clone()),
        (LayerExportFormat::Kmz, xlsx.clone()),
        (LayerExportFormat::ShpZip, kmz),
        (
            LayerExportFormat::Dashboard,
            b"PK\x03\x04 not an archive".to_vec(),
        ),
    ] {
        let error = decode_reply(format, &bytes).unwrap_err();
        assert!(
            error.to_string().starts_with("export_response_invalid: "),
            "{format:?}: {error}"
        );
    }
}

#[test]
fn compatible_additive_request_fields_are_accepted() {
    let layers = fixture_layers();
    let plain =
        execute_bytes(&serde_json::to_vec(&json!({"format": "xlsx", "layers": layers})).unwrap())
            .unwrap();
    let extended = execute_bytes(
        &serde_json::to_vec(
            &json!({"format": "xlsx", "layers": layers, "futureStyle": {"theme": "dark"}}),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(plain, extended);
    let receipt = json!({"format": "xlsx", "byte_length": 10, "text": false, "future": 1});
    assert!(serde_json::from_value::<LayerExportReceipt>(receipt).is_ok());
}

/// The engine parsed requests into BTreeMap objects. Keys deliberately out of
/// order must export exactly like the same layers with sorted keys (control),
/// and GeoJSON objects the writer adds itself are emitted key-sorted too.
#[test]
fn unsorted_object_keys_export_in_the_engine_key_order() {
    let unsorted = json!({"layers": [{"name": "poles", "geojson": {"type": "FeatureCollection", "features": [
        {"type": "Feature", "properties": {"zeta": 1, "alpha": "a", "Mid": true, "nested": {"z": 1, "a": 2}},
         "geometry": {"type": "Point", "coordinates": [30.0, -2.0]}}
    ]}}]});
    let mut control = unsorted.clone();
    control.sort_all_objects();
    assert_ne!(
        serde_json::to_string(&unsorted).unwrap(),
        serde_json::to_string(&control).unwrap(),
        "the fixture must really be unsorted under preserve_order"
    );
    for format in ["geojson", "xlsx", "delimited", "kmz", "shp_zip"] {
        let request = |layers: &Value| {
            let mut request = layers.clone();
            request["format"] = json!(format);
            serde_json::to_vec(&request).unwrap()
        };
        assert_eq!(
            execute_bytes(&request(&unsorted)).unwrap(),
            execute_bytes(&request(&control)).unwrap(),
            "{format}"
        );
    }
    let mut geojson = unsorted.clone();
    geojson["format"] = json!("geojson");
    let text =
        String::from_utf8(execute_bytes(&serde_json::to_vec(&geojson).unwrap()).unwrap()).unwrap();
    assert!(
        text.starts_with("{\n  \"features\": [\n    {\n      \"geometry\""),
        "{text}"
    );
    assert!(text.contains("\"properties\": {\n        \"Mid\": true,\n        \"_layer\": \"poles\",\n        \"alpha\""));
    let mut csv = unsorted;
    csv["format"] = json!("delimited");
    let text =
        String::from_utf8(execute_bytes(&serde_json::to_vec(&csv).unwrap()).unwrap()).unwrap();
    assert!(
        text.starts_with("Mid,alpha,nested,zeta,geometry_wkt\r\n"),
        "{text}"
    );
    assert!(text.contains("\"{\"\"a\"\":2,\"\"z\"\":1}\""), "{text}");
}
