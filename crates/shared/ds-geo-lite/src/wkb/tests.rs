use super::*;
use serde_json::json;

#[test]
fn native_decoder_preserves_holes_and_multipart_geometry() {
    let fixtures = [
        json!({"type":"Point","coordinates":[30.0,-2.0]}),
        json!({"type":"LineString","coordinates":[[1.0,2.0],[3.0,4.0]]}),
        json!({"type":"Polygon","coordinates":[[[0.0,0.0],[9.0,0.0],[9.0,9.0],[0.0,0.0]],[[1.0,1.0],[2.0,1.0],[2.0,2.0],[1.0,1.0]]]}),
        json!({"type":"MultiPoint","coordinates":[[1.0,2.0],[3.0,4.0]]}),
        json!({"type":"MultiLineString","coordinates":[[[1.0,2.0],[3.0,4.0]],[]]}),
        json!({"type":"MultiPolygon","coordinates":[[[[0.0,0.0],[2.0,0.0],[2.0,2.0],[0.0,0.0]]],[]]}),
    ];
    for geometry in fixtures {
        let bytes = geometry_to_wkb(&geometry).unwrap();
        assert_eq!(geometry_from_wkb(&bytes, 100).unwrap(), geometry);
        for end in 0..bytes.len() {
            assert!(geometry_from_wkb(&bytes[..end], 100).is_err());
        }
        assert!(geometry_from_wkb(&bytes, 1).is_err());
        let mut trailing = bytes;
        trailing.push(0);
        assert!(geometry_from_wkb(&trailing, 100).is_err());
    }
}

#[test]
fn native_decoder_rejects_nonfinite_wrong_types_and_hostile_counts() {
    let mut point = vec![0];
    point.extend_from_slice(&1_u32.to_be_bytes());
    point.extend_from_slice(&30_f64.to_be_bytes());
    point.extend_from_slice(&(-2_f64).to_be_bytes());
    assert_eq!(
        geometry_from_wkb(&point, 2).unwrap()["coordinates"],
        json!([30.0, -2.0])
    );
    for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        point[5..13].copy_from_slice(&bad.to_be_bytes());
        assert!(geometry_from_wkb(&point, 2).is_err());
    }
    let valid = geometry_to_wkb(&json!({"type":"Point","coordinates":[1.0,2.0]})).unwrap();
    let mut multi = vec![1];
    multi.extend_from_slice(&5_u32.to_le_bytes()); // MultiLineString cannot contain Point.
    multi.extend_from_slice(&1_u32.to_le_bytes());
    multi.extend_from_slice(&valid);
    assert!(geometry_from_wkb(&multi, 100).is_err());
    multi[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(geometry_from_wkb(&multi, usize::MAX).is_err());
    let mut invalid_order = valid;
    invalid_order[0] = 2;
    assert!(geometry_from_wkb(&invalid_order, 100).is_err());
}

#[test]
fn native_decoder_bounds_nested_empty_collections() {
    let mut bytes = vec![1];
    bytes.extend_from_slice(&7_u32.to_le_bytes());
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    assert_eq!(
        geometry_from_wkb(&bytes, 1).unwrap(),
        json!({"type":"GeometryCollection","geometries":[]})
    );
    for _ in 0..40 {
        let mut outer = vec![1];
        outer.extend_from_slice(&7_u32.to_le_bytes());
        outer.extend_from_slice(&1_u32.to_le_bytes());
        outer.extend(bytes);
        bytes = outer;
    }
    assert!(
        geometry_from_wkb(&bytes, 100)
            .unwrap_err()
            .contains("nesting")
    );
}

#[test]
fn truncated_blob_retains_its_read_prefix_during_relocation() {
    let mut truncated = vec![1];
    truncated.extend_from_slice(&2_u32.to_le_bytes());
    truncated.extend_from_slice(&2_u32.to_le_bytes());
    truncated.extend_from_slice(&100_f64.to_le_bytes());
    truncated.extend_from_slice(&100_f64.to_le_bytes());
    let valid = geometry_to_wkb(&json!({"type":"Point","coordinates":[1.,2.]}));
    assert_eq!(
        bbox_of(&[Some(truncated), valid]),
        Some([1., 2., 100., 100.])
    );
}

#[test]
fn a_point_is_twenty_one_bytes_little_endian() {
    let wkb = geometry_to_wkb(&json!({"type":"Point","coordinates":[30.0,-2.0]})).unwrap();
    assert_eq!(wkb.len(), 21, "1 byte order + 4 type + 2 x f64");
    assert_eq!(wkb[0], 1, "little endian flag");
    assert_eq!(u32::from_le_bytes(wkb[1..5].try_into().unwrap()), 1);
    assert_eq!(f64::from_le_bytes(wkb[5..13].try_into().unwrap()), 30.0);
    assert_eq!(f64::from_le_bytes(wkb[13..21].try_into().unwrap()), -2.0);
}

#[test]
fn a_third_ordinate_is_dropped_from_the_geometry() {
    let wkb = geometry_to_wkb(&json!({"type":"Point","coordinates":[30.0,-2.0,1500.0]})).unwrap();
    assert_eq!(wkb.len(), 21, "z belongs in a property column, not in WKB");
}

#[test]
fn an_unsupported_geometry_is_refused_rather_than_written_wrong() {
    assert!(geometry_to_wkb(&json!({"type":"Triangle","coordinates":[]})).is_none());
    assert!(geometry_to_wkb(&json!({"type":"Point","coordinates":["a","b"]})).is_none());
}

#[test]
fn a_point_decodes_back_to_its_coordinate() {
    let wkb = geometry_to_wkb(&json!({"type":"Point","coordinates":[30.06,-1.94]})).unwrap();
    assert_eq!(point_xy(&wkb), Some((30.06, -1.94)));
}

#[test]
fn a_compound_geometry_is_refused_rather_than_partially_decoded() {
    let wkb =
        geometry_to_wkb(&json!({"type":"LineString","coordinates":[[30.0,-1.0],[30.9,-1.2]]}))
            .unwrap();
    assert_eq!(
        point_xy(&wkb),
        None,
        "returning the first vertex would be a lie"
    );
}

#[test]
fn bbox_spans_every_geometry_in_the_batch() {
    let a = geometry_to_wkb(&json!({"type":"Point","coordinates":[29.0,-2.5]}));
    let b = geometry_to_wkb(&json!({"type":"LineString","coordinates":[[30.0,-1.0],[30.9,-1.2]]}));
    let bbox = bbox_of(&[a, b, None]).unwrap();
    assert_eq!(bbox, [29.0, -2.5, 30.9, -1.0]);
}

#[test]
fn bbox_of_a_multi_ring_polygon_reads_coordinates_not_length_prefixes() {
    // The regression that shipped a bbox of ±1.8e308: a polygon with a
    // hole has a second ring whose point count sits mid-blob, and a
    // scanner that does not follow the structure reads it as an f64.
    let polygon = geometry_to_wkb(&json!({
        "type": "Polygon",
        "coordinates": [
            [[29.0, -2.0], [30.0, -2.0], [30.0, -1.0], [29.0, -1.0], [29.0, -2.0]],
            [[29.4, -1.8], [29.6, -1.8], [29.6, -1.6], [29.4, -1.6], [29.4, -1.8]]
        ]
    }))
    .unwrap();
    assert_eq!(bbox_of(&[Some(polygon)]).unwrap(), [29.0, -2.0, 30.0, -1.0]);
}

#[test]
fn bbox_of_a_multipolygon_follows_every_member() {
    let multi = geometry_to_wkb(&json!({
        "type": "MultiPolygon",
        "coordinates": [
            [[[29.0, -2.0], [29.5, -2.0], [29.5, -1.5], [29.0, -2.0]]],
            [[[30.0, -1.0], [30.9, -1.0], [30.9, -0.5], [30.0, -1.0]]]
        ]
    }))
    .unwrap();
    assert_eq!(bbox_of(&[Some(multi)]).unwrap(), [29.0, -2.0, 30.9, -0.5]);
}

#[test]
fn a_truncated_blob_contributes_nothing_rather_than_garbage() {
    let point = geometry_to_wkb(&json!({"type":"Point","coordinates":[30.0,-2.0]})).unwrap();
    let good = point.clone();
    let truncated = point[..12].to_vec();
    let bbox = bbox_of(&[Some(good), Some(truncated)]).unwrap();
    assert_eq!(bbox, [30.0, -2.0, 30.0, -2.0]);
}

#[test]
fn bbox_is_absent_when_nothing_carried_a_geometry() {
    assert!(bbox_of(&[None, None]).is_none());
}

// Frozen pre-consolidation GeoPackage encoder: an independent byte oracle for
// the upstream writer, never linked into production or used to bless drift.
fn write_wkb_geometry(out: &mut Vec<u8>, geometry: &Value) -> Option<()> {
    match geometry.get("type").and_then(Value::as_str)? {
        "Point" => {
            write_header(out, 1);
            write_position(out, geometry.get("coordinates")?)
        }
        "LineString" => write_sequence(out, 2, geometry.get("coordinates")?),
        "Polygon" => write_polygon(out, geometry.get("coordinates")?),
        "MultiPoint" => write_collection(out, 4, geometry, "Point"),
        "MultiLineString" => write_collection(out, 5, geometry, "LineString"),
        "MultiPolygon" => write_collection(out, 6, geometry, "Polygon"),
        "GeometryCollection" => {
            let geometries = geometry.get("geometries")?.as_array()?;
            write_header(out, 7);
            write_count(out, geometries.len())?;
            geometries
                .iter()
                .try_for_each(|child| write_wkb_geometry(out, child))
        }
        _ => None,
    }
}

fn write_collection(out: &mut Vec<u8>, kind: u32, geometry: &Value, child: &str) -> Option<()> {
    let coordinates = geometry.get("coordinates")?.as_array()?;
    write_header(out, kind);
    write_count(out, coordinates.len())?;
    for value in coordinates {
        write_wkb_geometry(
            out,
            &Value::Object(serde_json::Map::from_iter([
                ("type".to_string(), Value::String(child.to_string())),
                ("coordinates".to_string(), value.clone()),
            ])),
        )?;
    }
    Some(())
}

fn write_header(out: &mut Vec<u8>, kind: u32) {
    out.push(1);
    out.extend_from_slice(&kind.to_le_bytes());
}

fn write_position(out: &mut Vec<u8>, value: &Value) -> Option<()> {
    let position = value.as_array()?;
    let x = position.first()?.as_f64()?;
    let y = position.get(1)?.as_f64()?;
    if !x.is_finite() || !y.is_finite() {
        return None;
    }
    out.extend_from_slice(&x.to_le_bytes());
    out.extend_from_slice(&y.to_le_bytes());
    Some(())
}

fn write_sequence(out: &mut Vec<u8>, kind: u32, value: &Value) -> Option<()> {
    let positions = value.as_array()?;
    write_header(out, kind);
    write_count(out, positions.len())?;
    positions
        .iter()
        .try_for_each(|position| write_position(out, position))
}

fn write_polygon(out: &mut Vec<u8>, value: &Value) -> Option<()> {
    let rings = value.as_array()?;
    write_header(out, 3);
    write_count(out, rings.len())?;
    for ring in rings {
        let positions = ring.as_array()?;
        write_count(out, positions.len())?;
        positions
            .iter()
            .try_for_each(|position| write_position(out, position))?;
    }
    Some(())
}

fn write_count(out: &mut Vec<u8>, value: usize) -> Option<()> {
    out.extend_from_slice(&u32::try_from(value).ok()?.to_le_bytes());
    Some(())
}

#[test]
fn upstream_writer_preserves_every_legacy_xy_byte_and_ignored_ordinate() {
    let next = f64::from_bits(1.0_f64.to_bits() + 1);
    let fixtures = [
        json!({"type":"Point","coordinates":[-0.0,next,"ignored"]}),
        json!({"type":"LineString","coordinates":[[1.0,2.0,99.0],[next,-0.0]]}),
        json!({"type":"Polygon","coordinates":[[[0.0,0.0],[9.0,0.0],[0.0,0.0]],[],[[1.0,1.0],[2.0,1.0],[1.0,1.0]]]}),
        json!({"type":"MultiPoint","coordinates":[[1.0,2.0],[next,-0.0]]}),
        json!({"type":"MultiLineString","coordinates":[[],[[1.0,2.0],[3.0,4.0]]]}),
        json!({"type":"MultiPolygon","coordinates":[[],[[[1.0,2.0],[3.0,4.0],[1.0,2.0]]]]}),
        json!({"type":"GeometryCollection","geometries":[{"type":"GeometryCollection","geometries":[]},{"type":"Point","coordinates":[next,-0.0]},{"type":"MultiPoint","coordinates":[[1.0,2.0]]}]}),
    ];
    for geometry in fixtures {
        let mut expected = Vec::new();
        write_wkb_geometry(&mut expected, &geometry).unwrap();
        assert_eq!(
            geometry_to_wkb_with_collections(&geometry),
            Some(expected.clone())
        );
        if geometry["type"] == "GeometryCollection" {
            assert!(geometry_to_wkb(&geometry).is_none());
        } else {
            assert_eq!(geometry_to_wkb(&geometry), Some(expected));
        }
    }
    for kind in [
        "LineString",
        "Polygon",
        "MultiPoint",
        "MultiLineString",
        "MultiPolygon",
    ] {
        let geometry = json!({"type":kind,"coordinates":[]});
        let mut expected = Vec::new();
        write_wkb_geometry(&mut expected, &geometry).unwrap();
        assert_eq!(geometry_to_wkb(&geometry), Some(expected));
    }
}

#[test]
fn extended_decoder_keeps_dimension_profiles_and_finite_admission_separate() {
    for code in [1001_u32, 2001, 3001, 0xa000_0001, 0x6000_0001, 0xe000_0001] {
        let dimensions = if code == 3001 || code == 0xe000_0001 {
            4
        } else {
            3
        };
        for order in [0_u8, 1] {
            let mut bytes = vec![order];
            bytes.extend(if order == 1 {
                code.to_le_bytes()
            } else {
                code.to_be_bytes()
            });
            if code & 0x2000_0000 != 0 {
                bytes.extend(if order == 1 {
                    4326_u32.to_le_bytes()
                } else {
                    4326_u32.to_be_bytes()
                });
            }
            let values = [1.0_f64, -0.0, 99.0, 42.0];
            for value in &values[..dimensions] {
                bytes.extend(if order == 1 {
                    value.to_le_bytes()
                } else {
                    value.to_be_bytes()
                });
            }
            assert!(geometry_from_wkb(&bytes, 10).is_err());
            assert_eq!(
                geometry_from_extended_wkb(&bytes, 2, true).unwrap()["coordinates"],
                json!(values[..dimensions])
            );
            assert_eq!(
                geometry_from_extended_wkb(&bytes, 2, false).unwrap()["coordinates"],
                json!([1.0, -0.0])
            );
            assert!(geometry_from_extended_wkb(&bytes, 1, true).is_err());
            for end in 0..bytes.len() {
                assert!(geometry_from_extended_wkb(&bytes[..end], 10, true).is_err());
            }
            let mut trailing = bytes.clone();
            trailing.push(0);
            assert!(geometry_from_extended_wkb(&trailing, 10, true).is_err());
            let length = bytes.len();
            bytes[length - 8..].copy_from_slice(&if order == 1 {
                f64::NAN.to_le_bytes()
            } else {
                f64::NAN.to_be_bytes()
            });
            assert!(
                geometry_from_extended_wkb(&bytes, 10, false).is_err(),
                "dropped Z/M must still be finite"
            );
        }
    }
}

#[test]
fn extended_decoder_rejects_wrong_members_orders_counts_and_depth_before_allocation() {
    let point = geometry_to_wkb(&json!({"type":"Point","coordinates":[1.0,2.0]})).unwrap();
    let mut wrong = vec![1];
    wrong.extend(5_u32.to_le_bytes());
    wrong.extend(1_u32.to_le_bytes());
    wrong.extend(&point);
    assert!(geometry_from_extended_wkb(&wrong, usize::MAX, true).is_err());
    wrong[5..9].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(geometry_from_extended_wkb(&wrong, usize::MAX, true).is_err());
    let mut wrong = point;
    wrong[0] = 2;
    assert!(geometry_from_extended_wkb(&wrong, usize::MAX, true).is_err());
    let mut nested = vec![1];
    nested.extend(7_u32.to_le_bytes());
    nested.extend(0_u32.to_le_bytes());
    for depth in 1..=65 {
        let mut outer = vec![1];
        outer.extend(7_u32.to_le_bytes());
        outer.extend(1_u32.to_le_bytes());
        outer.extend(nested);
        nested = outer;
        assert_eq!(
            geometry_from_extended_wkb(&nested, nested.len(), true).is_ok(),
            depth <= 64
        );
    }
}
