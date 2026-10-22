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
    let result = design_features::select(
        DesignFeatureSourceContext::TransformerSnapshot {
            ds_project: "project-a".into(),
            transformer: "T-1".into(),
            version: None,
            content_digest: None,
        },
        &layers,
        FeatureSelector {
            sample: 2,
            ..FeatureSelector::default()
        },
    )
    .unwrap();
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
