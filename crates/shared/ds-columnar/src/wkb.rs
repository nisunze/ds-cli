//! Canonical geometry codec shared with cleaning.
pub use ds_geo_lite::wkb::{bbox_of, geometry_to_wkb, point_xy};

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        let wkb =
            geometry_to_wkb(&json!({"type":"Point","coordinates":[30.0,-2.0,1500.0]})).unwrap();
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
        let b =
            geometry_to_wkb(&json!({"type":"LineString","coordinates":[[30.0,-1.0],[30.9,-1.2]]}));
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
}
