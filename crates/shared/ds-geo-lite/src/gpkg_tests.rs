use super::*;

#[test]
fn native_source_preserves_z_and_m_without_changing_xy_imports() {
    let mut point = vec![1_u8];
    point.extend_from_slice(&3001_u32.to_le_bytes());
    for value in [1.0_f64, 2.0, 99.0, 42.0] {
        point.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(
        gpkg_geometry_to_geojson(&point).unwrap().geometry,
        Some(serde_json::json!({"type":"Point","coordinates":[1.0,2.0]}))
    );
    assert_eq!(
        gpkg_geometry_to_geojson_preserving_dimensions(&point)
            .unwrap()
            .geometry,
        Some(serde_json::json!({"type":"Point","coordinates":[1.0,2.0,99.0,42.0]}))
    );
}

#[test]
fn geojson_round_trips_through_the_shared_gpkg_geometry_contract() {
    for geometry in [
        serde_json::json!({"type":"Point","coordinates":[1.5,2.5]}),
        serde_json::json!({"type":"LineString","coordinates":[[1.0,2.0],[3.0,4.0]]}),
        serde_json::json!({"type":"Polygon","coordinates":[[[0.0,0.0],[1.0,0.0],[1.0,1.0],[0.0,0.0]]]}),
    ] {
        let bytes = geojson_geometry_to_gpkg_blob(&geometry, 4326).unwrap();
        let decoded = gpkg_geometry_to_geojson(&bytes).unwrap();
        assert_eq!(decoded.geometry, Some(geometry));
        assert_eq!(decoded.header.unwrap().srs_id, 4326);
    }
}

#[test]
fn decoder_honours_envelopes_and_ewkb_dimensions() {
    let point = serde_json::json!({"type":"Point","coordinates":[12.0,13.0]});
    let original = geojson_geometry_to_gpkg_blob(&point, 4326).unwrap();
    let mut enveloped = Vec::new();
    enveloped.extend_from_slice(&[0x47, 0x50, 0, 0b11]);
    enveloped.extend_from_slice(&4326_i32.to_le_bytes());
    for value in [10.0_f64, 11.0, 20.0, 21.0] {
        enveloped.extend_from_slice(&value.to_le_bytes());
    }
    enveloped.extend_from_slice(&original[8..]);
    let decoded = gpkg_geometry_to_geojson(&enveloped).unwrap();
    assert_eq!(decoded.geometry, Some(point));
    assert_eq!(decoded.header.unwrap().wkb_offset, 40);

    let mut ewkb = vec![1];
    ewkb.extend_from_slice(&(1_u32 | 0x8000_0000 | 0x2000_0000).to_le_bytes());
    ewkb.extend_from_slice(&4326_u32.to_le_bytes());
    ewkb.extend_from_slice(&1.0_f64.to_le_bytes());
    ewkb.extend_from_slice(&2.0_f64.to_le_bytes());
    ewkb.extend_from_slice(&99.0_f64.to_le_bytes());
    assert_eq!(
        gpkg_geometry_to_geojson(&ewkb).unwrap().geometry,
        Some(serde_json::json!({"type":"Point","coordinates":[1.0,2.0]}))
    );
}

#[test]
fn layer_policy_is_shared_and_fail_closed() {
    let base = GpkgLayerPolicyInput {
        data_type: "FEATURES".to_string(),
        geometry_column: Some("geom".to_string()),
        geometry_type: Some("MULTI_POLYGON".to_string()),
        srs_id: Some(4326),
        srs_organization: Some("EPSG".to_string()),
        srs_organization_coord_sys_id: Some(4326),
        row_count: Some(10),
        max_rows: 100,
    };
    assert!(gpkg_layer_policy(&base).readable);
    let refused = gpkg_layer_policy(&GpkgLayerPolicyInput {
        row_count: Some(101),
        ..base
    });
    assert_eq!(
        refused.fallback_reason.as_deref(),
        Some("row_count_too_large")
    );
}

#[test]
fn hostile_counts_and_trailing_bytes_are_refused_before_allocation() {
    let mut hostile = vec![1];
    hostile.extend_from_slice(&2_u32.to_le_bytes());
    hostile.extend_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        gpkg_geometry_to_geojson(&hostile)
            .unwrap_err()
            .contains("exceeds the remaining geometry bytes")
    );

    let mut trailing = geojson_geometry_to_gpkg_blob(
        &serde_json::json!({"type":"Point","coordinates":[1.0,2.0]}),
        4326,
    )
    .unwrap();
    trailing.push(0);
    assert!(
        gpkg_geometry_to_geojson(&trailing)
            .unwrap_err()
            .contains("trailing bytes")
    );
}

#[test]
fn header_refusals_and_empty_dimension_flags_stay_at_the_gpkg_boundary() {
    let point = serde_json::json!({"type":"Point","coordinates":[1.0,2.0]});
    let original = geojson_geometry_to_gpkg_blob(&point, 4326).unwrap();
    for end in 2..8 {
        assert!(gpkg_geometry_to_geojson(&original[..end]).is_err());
    }
    for (field, value) in [(2, 1), (3, 0x40), (3, 0x80), (3, 0b1011)] {
        let mut invalid = original.clone();
        invalid[field] = value;
        assert!(gpkg_geometry_to_geojson(&invalid).is_err());
    }
    for envelope_code in 1..=4 {
        let mut truncated = original[..8].to_vec();
        truncated[3] = (envelope_code << 1) | 1;
        assert!(gpkg_geometry_to_geojson(&truncated).is_err());
    }
    let mut empty = original[..8].to_vec();
    empty[3] |= 0b1_0000;
    let decoded = gpkg_geometry_to_geojson(&empty).unwrap();
    assert!(decoded.geometry.is_none());
    assert!(decoded.header.unwrap().empty);

    // The admitted extended flag is metadata, not permission to reinterpret
    // SRS/header endian as the separate WKB payload's endian.
    let mut extended = original;
    extended[3] = 0b10_0000;
    extended[4..8].copy_from_slice(&4326_i32.to_be_bytes());
    let decoded = gpkg_geometry_to_geojson(&extended).unwrap();
    assert_eq!(decoded.geometry, Some(point));
    let header = decoded.header.unwrap();
    assert!(header.extended);
    assert!(!header.little_endian);
    assert_eq!(header.srs_id, 4326);
}
