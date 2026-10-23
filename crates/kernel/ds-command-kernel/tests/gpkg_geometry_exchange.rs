//! The Kernel's GeoPackage operations answer exactly what the retired Network
//! exports answered (one shared codec) and refuse oversized or malformed
//! input by name.
use ds_command_kernel::gpkg_geometry::{
    decode_geometry, layer_policy, GpkgLayerPolicyInput, MAX_GPKG_GEOMETRY_BYTES,
};
use serde_json::json;

fn blob(geometry: serde_json::Value) -> Vec<u8> {
    ds_geo_lite::gpkg::geojson_geometry_to_gpkg_blob(&geometry, 4326).unwrap()
}

#[test]
fn geometry_cells_decode_through_the_shared_codec() {
    for geometry in [
        json!({"type":"Point","coordinates":[30.06,-1.95]}),
        json!({"type":"LineString","coordinates":[[30.0,-2.0],[30.001,-2.0]]}),
        json!({"type":"Polygon","coordinates":[[[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,0.0]]]}),
    ] {
        let bytes = blob(geometry.clone());
        let decoded = decode_geometry(&bytes).unwrap();
        assert_eq!(
            decoded,
            ds_geo_lite::gpkg::gpkg_geometry_to_geojson(&bytes).unwrap()
        );
        assert_eq!(decoded.geometry, Some(geometry));
        assert_eq!(decoded.header.as_ref().unwrap().srs_id, 4326);
        assert!(!decoded.bare_wkb);
        assert_eq!(
            serde_json::to_value(&decoded)
                .unwrap()
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            ["geometry", "header", "bare_wkb"]
        );
    }
    // Bare WKB keeps XY only, exactly as the browser import always did.
    let mut point = vec![1_u8];
    point.extend_from_slice(&3001_u32.to_le_bytes());
    for value in [1.0_f64, 2.0, 99.0, 42.0] {
        point.extend_from_slice(&value.to_le_bytes());
    }
    let decoded = decode_geometry(&point).unwrap();
    assert!(decoded.bare_wkb);
    assert_eq!(
        decoded.geometry,
        Some(json!({"type":"Point","coordinates":[1.0,2.0]}))
    );
}

#[test]
fn oversized_and_malformed_cells_are_refused_by_name() {
    let oversized = vec![0_u8; MAX_GPKG_GEOMETRY_BYTES + 1];
    assert!(decode_geometry(&oversized)
        .unwrap_err()
        .to_string()
        .starts_with("gpkg_request_invalid: GeoPackage geometry is"));
    for malformed in [vec![3_u8], b"GP".to_vec(), {
        let mut trailing = blob(json!({"type":"Point","coordinates":[1.0,2.0]}));
        trailing.push(0);
        trailing
    }] {
        let refused = decode_geometry(&malformed).unwrap_err().to_string();
        assert!(refused.starts_with("gpkg_request_invalid:"), "{refused}");
    }
}

#[test]
fn layer_policy_is_the_shared_fail_closed_classification() {
    let base = GpkgLayerPolicyInput {
        data_type: "FEATURES".into(),
        geometry_column: Some("geom".into()),
        geometry_type: Some("MULTI_POLYGON".into()),
        srs_id: Some(4326),
        srs_organization: Some("EPSG".into()),
        srs_organization_coord_sys_id: Some(4326),
        row_count: Some(10),
        max_rows: 100,
    };
    assert_eq!(
        layer_policy(&base).unwrap(),
        ds_geo_lite::gpkg::gpkg_layer_policy(&base)
    );
    assert!(layer_policy(&base).unwrap().readable);
    let refused = layer_policy(&GpkgLayerPolicyInput {
        row_count: Some(101),
        ..base.clone()
    })
    .unwrap();
    assert_eq!(
        refused.fallback_reason.as_deref(),
        Some("row_count_too_large")
    );
    // The browser omits absent catalog fields; they read as absent.
    let sparse: GpkgLayerPolicyInput =
        serde_json::from_value(json!({"data_type": "features", "max_rows": 12, "future": 1}))
            .unwrap();
    assert_eq!(
        layer_policy(&sparse).unwrap().fallback_reason.as_deref(),
        Some("invalid_database")
    );
    let huge = GpkgLayerPolicyInput {
        geometry_type: Some("x".repeat(70_000)),
        ..base
    };
    assert!(layer_policy(&huge)
        .unwrap_err()
        .to_string()
        .starts_with("gpkg_request_invalid:"));
}
