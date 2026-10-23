use super::*;
#[test]
#[ignore = "requires a built compatible Network engine and immutable package fixture"]
fn acceptance_native_mv_projection_local_print_context() {
    let path = std::env::var("DS_MV_PROJECTION_MODEL").unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let digest = report_export::sha256_hex(&bytes);
    let model = load_local(&path, "stable").unwrap();
    assert_eq!(model.identity.id, format!("local-{}", &digest[..16]));
    assert_eq!(model.identity.revision_id, format!("local:sha256:{digest}"));
    assert_eq!(model.identity.digest, digest);
    let expected: Value = serde_json::from_slice(
        &std::fs::read(std::env::var("DS_MV_PROJECTION_GOLDEN").unwrap()).unwrap(),
    )
    .unwrap();
    assert_eq!(model.projection, expected);
    let sources = provenance(std::slice::from_ref(&model));
    assert_eq!(sources[0]["sha256"], digest);
    let layers = overview(std::slice::from_ref(&model)).unwrap();
    assert!(
        !layers["dsgrid_mv_lines"]["features"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
}
#[test]
#[ignore = "requires an explicitly unavailable captured Network engine"]
fn acceptance_native_mv_missing_engine_is_unavailable() {
    let path = std::env::var("DS_MV_PROJECTION_MODEL").unwrap();
    let error = match load_local(&path, "stable") {
        Err(error) => error,
        Ok(_) => panic!("missing engine cannot return a projection"),
    };
    assert_eq!(
        error.class(),
        ds_cli_contract::outcome::ExitClass::Unavailable
    );
    assert_eq!(error.code(), "network_engine_missing");
}
#[test]
fn mv_context_uses_metric_crs_and_records_model_identity_without_changing_geometry_source() {
    let projection = json!({"type":"FeatureCollection","features":[{"type":"Feature","id":"route-1","geometry":{"type":"LineString","coordinates":[[29.999,-2.],[30.001,-2.]]},"properties":{"_layer":"alignments","label":"MV A"}}]});
    let models = [Model {
        identity: map::Model {
            id: "mv-model".into(),
            label: "Proposed MV".into(),
            revision_id: "rev-1".into(),
            digest: "a".repeat(64),
        },
        projection: projection.clone(),
    }];
    let network = json!({"tr":{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[30.,-2.]},"properties":{"transfo":"tx_a"}}]}});
    let attached = attach(None, &models, "tx_a", &network, 200.)
        .unwrap()
        .unwrap();
    let document: Value = serde_json::from_slice(&attached.bytes).unwrap();
    let feature = &document["layers"]["dsgrid_mv_lines"]["features"][0];
    assert_eq!(feature["properties"]["model_revision_id"], "rev-1");
    assert_eq!(feature["geometry"], projection["features"][0]["geometry"]);
    assert_eq!(attached.sha256, report_export::sha256_hex(&attached.bytes));
    assert_eq!(models[0].projection, projection);
}

#[test]
fn individual_sheet_suppresses_only_angle_symbols_at_poles() {
    let projection = json!({"type":"FeatureCollection","features":[
        {"type":"Feature","id":"on-lv","geometry":{"type":"Point","coordinates":[30.000004,-2.0]},"properties":{"_layer":"angle_points"}},
        {"type":"Feature","id":"on-mv","geometry":{"type":"Point","coordinates":[30.002,-2.0]},"properties":{"_layer":"angle_points"}},
        {"type":"Feature","id":"free","geometry":{"type":"Point","coordinates":[30.004,-2.0]},"properties":{"_layer":"angle_points"}},
        {"type":"Feature","id":"pole","geometry":{"type":"Point","coordinates":[30.002,-2.0]},"properties":{"_layer":"structures"}}
    ]});
    let models = [Model {
        identity: map::Model {
            id: "mv-model".into(),
            label: "MV".into(),
            revision_id: "rev-1".into(),
            digest: "a".repeat(64),
        },
        projection,
    }];
    let network = json!({
        "tr":{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"transfo":"tx_a"}}]},
        "lv_poles":{"type":"FeatureCollection","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[30.0,-2.0]},"properties":{"pole_number":"P1"}}]}
    });
    let individual = attach(None, &models, "tx_a", &network, 600.)
        .unwrap()
        .unwrap();
    let document: Value = serde_json::from_slice(&individual.bytes).unwrap();
    let angles = document["layers"]["dsgrid_mv_angle_points"]["features"]
        .as_array()
        .unwrap();
    assert_eq!(angles.len(), 1);
    assert_eq!(angles[0]["id"], "mv-model:\"free\"");
    assert_eq!(
        document["layers"]["dsgrid_mv_structures"]["features"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    let mv_overview = overview(&models).unwrap();
    assert_eq!(
        mv_overview["dsgrid_mv_angle_points"]["features"]
            .as_array()
            .unwrap()
            .len(),
        3
    );
}

#[test]
fn individual_sheet_keeps_angle_symbols_without_matching_poles() {
    let mut mv = BTreeMap::from([(
        "dsgrid_mv_angle_points".to_string(),
        vec![json!({"geometry":{"type":"Point","coordinates":[30.0,-2.0]}})],
    )]);
    suppress_angle_symbols_at_poles(
        &mut mv,
        &json!({"lv_poles":{"type":"FeatureCollection","features":[]}}),
    );
    assert_eq!(mv["dsgrid_mv_angle_points"].len(), 1);
}

#[test]
fn held_projections_round_trip_and_a_changed_copy_reads_as_not_held() {
    let dir = tempfile::tempdir().unwrap();
    let hold = super::super::hold::Hold::at(dir.path().to_path_buf(), "uid-1", "project-1");
    assert!(held(&hold).is_none());
    let model = Model {
        identity: map::Model {
            id: "m1".into(),
            label: "Feeder 1".into(),
            revision_id: "r1".into(),
            digest: "d1".into(),
        },
        projection: json!({"features": [{"properties": {"_layer": "structures"}}]}),
    };
    hold_models(&hold, std::slice::from_ref(&model)).unwrap();
    let back = held(&hold).unwrap();
    assert_eq!(
        (
            back[0].identity.id.as_str(),
            back[0].identity.revision_id.as_str()
        ),
        ("m1", "r1")
    );
    assert_eq!(back[0].projection, model.projection);

    let path = dir.path().join("tampered.json");
    let mut document: Value =
        serde_json::from_slice(&hold.read_file("mv-projections.json").unwrap()).unwrap();
    document["models"][0]["projection"] = json!({"features": []});
    std::fs::write(&path, serde_json::to_vec(&document).unwrap()).unwrap();
    hold.keep_file("mv-projections.json", &std::fs::read(&path).unwrap())
        .unwrap();
    assert!(
        held(&hold).is_none(),
        "a copy whose checksum does not match is not held"
    );
}
