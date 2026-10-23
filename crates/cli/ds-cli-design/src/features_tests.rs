use super::*;

#[test]
fn every_kernel_error_has_its_exact_public_code() {
    let errors = [
        FeatureSelectionError::TooManyLayers { count: 65 },
        FeatureSelectionError::InvalidLayerName,
        FeatureSelectionError::DuplicateLayerFilter { layer: "a".into() },
        FeatureSelectionError::TooManyWhereFilters { count: 65 },
        FeatureSelectionError::InvalidWhereKey,
        FeatureSelectionError::InvalidWhereValue { key: "a".into() },
        FeatureSelectionError::InvalidBbox,
        FeatureSelectionError::TooManyIds { count: 5001 },
        FeatureSelectionError::InvalidId,
        FeatureSelectionError::DuplicateId { id: "a".into() },
        FeatureSelectionError::SampleTooLarge { count: 201 },
        FeatureSelectionError::InvalidFeatureCollection { layer: "a".into() },
        FeatureSelectionError::InvalidFeature {
            layer: "a".into(),
            index: 0,
        },
        FeatureSelectionError::ScanLimitExceeded { count: 100001 },
        FeatureSelectionError::MatchLimitExceeded,
    ];
    for error in errors {
        let expected = error.code();
        assert_eq!(map_kernel(error).code(), expected);
    }
}

#[test]
fn bbox_parser_uses_shared_finite_wgs84_admission() {
    for raw in ["1,2,3", "NaN,2,3,4", "181,0,182,1", "3,2,1,4"] {
        assert_eq!(parse_bbox(raw).unwrap_err().code(), "invalid_bbox");
    }
    assert_eq!(parse_bbox(" 1, 2, 3, 4 ").unwrap(), [1., 2., 3., 4.]);
}

#[test]
fn cli_projection_keeps_authoritative_geometry_and_omission_receipts() {
    let point = json!({"type":"Point","coordinates":[30.0,-2.0]});
    let layers = BTreeMap::from([(
        "poles".to_string(),
        json!({
            "type":"FeatureCollection",
            "features":[
                {
                    "type":"Feature",
                    "id":"pole-1",
                    "properties":{"geometry":{"type":"Point","coordinates":[0,0]},"x":0,"y":0},
                    "geometry":point.clone()
                },
                {
                    "type":"Feature",
                    "id":"pole-2",
                    "properties":{"geometry":{"type":"Point","coordinates":[31,-3]},"x":31,"y":-3}
                }
            ]
        }),
    )]);
    let source = DesignFeatureSourceContext::TransformerSnapshot {
        ds_project: "project-a".into(),
        transformer: "T-1".into(),
        version: None,
        content_digest: None,
    };
    let selector = FeatureSelector {
        sample: 2,
        ..FeatureSelector::default()
    };
    let result = select_in_engine(source.clone(), &layers, selector.clone()).unwrap();
    // The engine's answer, as Core admits it, is the owner's exact bytes.
    let owner = ds_geo::feature_selection::select_geojson_features(&layers, selector).unwrap();
    assert_eq!(
        design_features::encode_response(result.clone()),
        design_features::encode_response(design_features::DesignFeaturesSelectResponse {
            source,
            selection: owner,
        })
    );
    let (projection, omitted_ids) = selection_projection(&result.selection, 2);

    assert_eq!(omitted_ids, 0);
    assert_eq!(projection["sample"][0]["geometry"], point);
    assert_eq!(projection["sample"][0]["geometryState"], "included");
    assert_eq!(projection["sample"][1]["geometryState"], "omitted");
    assert_eq!(projection["sample"][1]["geometryOmissionReason"], "missing");
    assert_eq!(projection["sample_with_geometry"], 1);
    assert_eq!(projection["sample_without_geometry"], 1);
    assert_eq!(projection["sample_geometry_oversize_omissions"], 0);
}

#[test]
fn command_contract_names_geometry_authority_and_legacy_non_fallbacks() {
    assert_eq!(COMMAND.contract, 2);
    assert!(COMMAND.output.contains("top-level WGS84 GeoJSON geometry"));
    assert!(COMMAND.output.contains("properties.geometry/x/y"));
    assert!(COMMAND.output.contains("undeclared CRS"));
}

#[test]
fn engine_refusals_keep_the_selector_code_message_and_detail() {
    let layers = BTreeMap::from([(
        "poles".to_string(),
        json!({"type":"FeatureCollection","features":[]}),
    )]);
    let source = DesignFeatureSourceContext::TransformerSnapshot {
        ds_project: "project-a".into(),
        transformer: "T-1".into(),
        version: None,
        content_digest: None,
    };
    for selector in [
        FeatureSelector {
            ids: vec!["a".into(), "a".into()],
            ..FeatureSelector::default()
        },
        FeatureSelector {
            sample: 201,
            ..FeatureSelector::default()
        },
        FeatureSelector {
            layers: vec!["absent layer\n".into()],
            ..FeatureSelector::default()
        },
    ] {
        let refused = select_in_engine(source.clone(), &layers, selector.clone()).unwrap_err();
        let owner = map_kernel(
            ds_geo::feature_selection::select_geojson_features(&layers, selector).unwrap_err(),
        );
        assert_eq!(refused.code(), owner.code());
        assert_eq!(refused.message(), owner.message());
        assert_eq!(refused.class(), owner.class());
        assert_eq!(refused.remedy_text(), owner.remedy_text());
        assert_eq!(refused.detail_value(), owner.detail_value());
    }
}

/// Run in a child of this test binary whose environment names an absent
/// engine, so no other test in this process sees a changed engine selection.
#[test]
fn a_missing_engine_is_a_typed_unavailable_refusal() {
    if std::env::var_os("DS_FEATURES_MISSING_ENGINE_CHILD").is_some() {
        let source = DesignFeatureSourceContext::TransformerSnapshot {
            ds_project: "project-a".into(),
            transformer: "T-1".into(),
            version: None,
            content_digest: None,
        };
        let refused =
            select_in_engine(source, &BTreeMap::new(), FeatureSelector::default()).unwrap_err();
        assert_eq!(refused.code(), "network_engine_missing");
        assert_eq!(refused.class(), ds_cli_contract::ExitClass::Unavailable);
        assert_eq!(refused.remedy_text(), Some(ENGINE_MISSING.remedy));
        return;
    }
    let absent = tempfile::tempdir().unwrap();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "features::tests::a_missing_engine_is_a_typed_unavailable_refusal",
        ])
        .env("DS_FEATURES_MISSING_ENGINE_CHILD", "1")
        .env("DS_NETWORK_ENGINE_BIN", absent.path().join("ds-network"))
        .output()
        .unwrap();
    assert!(child.status.success(), "{child:?}");
    assert!(String::from_utf8_lossy(&child.stdout).contains("1 passed"));
    for code in [
        "network_engine_missing",
        "network_engine_incompatible",
        "network_serialization_incompatible",
    ] {
        assert!(COMMAND.refusals.iter().any(|refusal| refusal.code == code));
    }
}
