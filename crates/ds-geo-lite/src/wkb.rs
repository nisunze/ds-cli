//! Canonical 2D WKB encoding and bounded native decoding.
//!
//! Canonical codec shared by cleaning and columnar output. The byte layout is
//! part of the streaming contract the browser already reads, so it is fixed:
//! byte order flag `1`, u32 type code, then the body. XY only — a third
//! ordinate is carried in a property column, never in the geometry.

use serde_json::Value;

pub fn geometry_to_wkb(geometry: &Value) -> Option<Vec<u8>> {
    let kind = geometry.get("type")?.as_str()?;
    let coordinates = geometry.get("coordinates")?;
    let mut out = Vec::new();
    write_geometry(&mut out, kind, coordinates).ok()?;
    Some(out)
}

/// Decode one complete 2D WKB geometry for native columnar consumers.
///
/// Unlike the compatibility envelope reader this refuses partial, non-finite,
/// extended-dimensional and trailing data. `max_nodes` bounds the combined
/// number of geometries, rings and coordinates, including empty parts. CRS
/// and geometric validity remain the source/mask authority's responsibility.
pub fn geometry_from_wkb(wkb: &[u8], max_nodes: usize) -> Result<Value, String> {
    let mut decoder = GeometryDecoder {
        bytes: wkb,
        cursor: 0,
        remaining: max_nodes,
    };
    let geometry = decoder.geometry(0, None)?;
    if decoder.cursor != wkb.len() {
        return Err("WKB has trailing bytes".into());
    }
    Ok(geometry)
}

struct GeometryDecoder<'a> {
    bytes: &'a [u8],
    cursor: usize,
    remaining: usize,
}

impl GeometryDecoder<'_> {
    fn admit(&mut self, count: usize) -> Result<(), String> {
        self.remaining = self
            .remaining
            .checked_sub(count)
            .ok_or("WKB node budget exceeded")?;
        Ok(())
    }

    fn count(&mut self, order: u8, minimum_bytes: usize) -> Result<usize, String> {
        let count =
            read_u32(self.bytes, &mut self.cursor, order).ok_or("truncated WKB count")? as usize;
        if count > self.remaining || count > (self.bytes.len() - self.cursor) / minimum_bytes {
            return Err("WKB count exceeds node budget or remaining bytes".into());
        }
        Ok(count)
    }

    fn point(&mut self, order: u8) -> Result<Value, String> {
        self.admit(1)?;
        let x = read_f64(self.bytes, &mut self.cursor, order).ok_or("truncated WKB coordinate")?;
        let y = read_f64(self.bytes, &mut self.cursor, order).ok_or("truncated WKB coordinate")?;
        if !x.is_finite() || !y.is_finite() {
            return Err("WKB coordinates must be finite".into());
        }
        Ok(serde_json::json!([x, y]))
    }

    fn line(&mut self, order: u8) -> Result<Value, String> {
        let count = self.count(order, 16)?;
        let mut points = Vec::with_capacity(count);
        for _ in 0..count {
            points.push(self.point(order)?);
        }
        Ok(Value::Array(points))
    }

    fn geometry(&mut self, depth: usize, expected: Option<u32>) -> Result<Value, String> {
        if depth > MAX_WKB_DEPTH {
            return Err("WKB nesting limit exceeded".into());
        }
        self.admit(1)?;
        let order = *self.bytes.get(self.cursor).ok_or("truncated WKB header")?;
        self.cursor += 1;
        if order > 1 {
            return Err("invalid WKB byte order".into());
        }
        let code = read_u32(self.bytes, &mut self.cursor, order).ok_or("truncated WKB type")?;
        if expected.is_some_and(|kind| kind != code) {
            return Err("WKB multipart member type mismatch".into());
        }
        let (kind, coordinates) = match code {
            1 => ("Point", self.point(order)?),
            2 => ("LineString", self.line(order)?),
            3 => {
                let count = self.count(order, 4)?;
                self.admit(count)?;
                let mut rings = Vec::with_capacity(count);
                for _ in 0..count {
                    rings.push(self.line(order)?);
                }
                ("Polygon", Value::Array(rings))
            }
            4..=7 => {
                let count = self.count(order, 5)?;
                let mut parts = Vec::with_capacity(count);
                for _ in 0..count {
                    let part = self.geometry(depth + 1, (code != 7).then_some(code - 3))?;
                    parts.push(if code == 7 {
                        part
                    } else {
                        part["coordinates"].clone()
                    });
                }
                if code == 7 {
                    return Ok(
                        serde_json::json!({"type":"GeometryCollection", "geometries":parts}),
                    );
                }
                let kind = match code {
                    4 => "MultiPoint",
                    5 => "MultiLineString",
                    _ => "MultiPolygon",
                };
                (kind, Value::Array(parts))
            }
            _ => return Err("unsupported WKB geometry type or dimensions".into()),
        };
        Ok(serde_json::json!({"type":kind, "coordinates":coordinates}))
    }
}

fn write_geometry(out: &mut Vec<u8>, kind: &str, coords: &Value) -> Result<(), String> {
    match kind {
        "Point" => {
            write_header(out, 1);
            write_point_body(out, point_coords(coords)?);
        }
        "LineString" => {
            write_header(out, 2);
            write_line_body(out, coords)?;
        }
        "Polygon" => {
            write_header(out, 3);
            write_polygon_body(out, coords)?;
        }
        "MultiPoint" => {
            write_header(out, 4);
            let points = as_array(coords, "MultiPoint")?;
            write_u32(out, points.len())?;
            for point in points {
                write_header(out, 1);
                write_point_body(out, point_coords(point)?);
            }
        }
        "MultiLineString" => {
            write_header(out, 5);
            let lines = as_array(coords, "MultiLineString")?;
            write_u32(out, lines.len())?;
            for line in lines {
                write_header(out, 2);
                write_line_body(out, line)?;
            }
        }
        "MultiPolygon" => {
            write_header(out, 6);
            let polygons = as_array(coords, "MultiPolygon")?;
            write_u32(out, polygons.len())?;
            for polygon in polygons {
                write_header(out, 3);
                write_polygon_body(out, polygon)?;
            }
        }
        other => return Err(format!("unsupported geometry type {other}")),
    }
    Ok(())
}

fn as_array<'a>(value: &'a Value, kind: &str) -> Result<&'a Vec<Value>, String> {
    value
        .as_array()
        .ok_or_else(|| format!("{kind} coordinates must be an array"))
}

fn write_header(out: &mut Vec<u8>, kind: u32) {
    out.push(1);
    out.extend_from_slice(&kind.to_le_bytes());
}

fn write_point_body(out: &mut Vec<u8>, point: (f64, f64)) {
    out.extend_from_slice(&point.0.to_le_bytes());
    out.extend_from_slice(&point.1.to_le_bytes());
}

fn write_line_body(out: &mut Vec<u8>, coords: &Value) -> Result<(), String> {
    let points = as_array(coords, "LineString")?;
    write_u32(out, points.len())?;
    for point in points {
        write_point_body(out, point_coords(point)?);
    }
    Ok(())
}

fn write_polygon_body(out: &mut Vec<u8>, coords: &Value) -> Result<(), String> {
    let rings = as_array(coords, "Polygon")?;
    write_u32(out, rings.len())?;
    for ring in rings {
        write_line_body(out, ring)?;
    }
    Ok(())
}

fn write_u32(out: &mut Vec<u8>, value: usize) -> Result<(), String> {
    let value = u32::try_from(value).map_err(|_| "geometry part count exceeds u32".to_string())?;
    out.extend_from_slice(&value.to_le_bytes());
    Ok(())
}

fn point_coords(value: &Value) -> Result<(f64, f64), String> {
    let coords = as_array(value, "Point")?;
    let x = coords
        .first()
        .and_then(Value::as_f64)
        .ok_or("point x is missing or not a number")?;
    let y = coords
        .get(1)
        .and_then(Value::as_f64)
        .ok_or("point y is missing or not a number")?;
    Ok((x, y))
}

/// The coordinate of a WKB point.
///
/// Only `Point` is decoded, and completely: a point has exactly one unambiguous
/// answer. Native compound-geometry consumers use [`geometry_from_wkb`],
/// which checks complete consumption and a caller-supplied resource budget.
pub fn point_xy(wkb: &[u8]) -> Option<(f64, f64)> {
    if wkb.len() < 21 || wkb[0] != 1 {
        return None;
    }
    if u32::from_le_bytes([wkb[1], wkb[2], wkb[3], wkb[4]]) != 1 {
        return None;
    }
    Some((
        f64::from_le_bytes(wkb[5..13].try_into().ok()?),
        f64::from_le_bytes(wkb[13..21].try_into().ok()?),
    ))
}

/// Envelope of a WKB batch, for the GeoParquet `bbox` field. Returns `None`
/// when nothing in the batch carried a geometry.
pub fn bbox_of(geometries: &[Option<Vec<u8>>]) -> Option<[f64; 4]> {
    let mut bbox = [f64::MAX, f64::MAX, f64::MIN, f64::MIN];
    let mut seen = false;
    for wkb in geometries.iter().flatten() {
        // Compatibility: successfully read positions remain in the accumulator
        // even if a later part of this blob is truncated.
        let _ = fold_bbox(wkb, &mut bbox, &mut seen);
    }
    seen.then_some(bbox)
}

/// Fold every XY pair of a WKB blob into `bbox`.
///
/// A real structural parse, not a scan. An earlier heuristic walked the bytes
/// skipping anything that looked like a header and reading the rest as
/// coordinate pairs; it survived Point and a two-vertex LineString and then
/// read ring length prefixes as f64 on the first real polygon, producing a
/// bbox of ±1.8e308. Length prefixes and coordinates are not distinguishable
/// by inspection, so the structure has to be followed exactly.
fn fold_bbox(wkb: &[u8], bbox: &mut [f64; 4], seen: &mut bool) -> Option<()> {
    let mut cursor = 0usize;
    read_geometry(wkb, &mut cursor, bbox, seen, 0)
}

const MAX_WKB_DEPTH: usize = 32;

fn read_geometry(
    wkb: &[u8],
    cursor: &mut usize,
    bbox: &mut [f64; 4],
    seen: &mut bool,
    depth: usize,
) -> Option<()> {
    if depth > MAX_WKB_DEPTH {
        return None;
    }
    let order = *wkb.get(*cursor)?;
    *cursor += 1;
    let code = read_u32(wkb, cursor, order)?;
    match code {
        1 => read_point(wkb, cursor, bbox, seen, order),
        2 => read_line(wkb, cursor, bbox, seen, order),
        3 => {
            let rings = read_u32(wkb, cursor, order)?;
            for _ in 0..rings {
                read_line(wkb, cursor, bbox, seen, order)?;
            }
            Some(())
        }
        4..=7 => {
            let parts = read_u32(wkb, cursor, order)?;
            for _ in 0..parts {
                // Each member carries its own byte order and type code.
                read_geometry(wkb, cursor, bbox, seen, depth + 1)?;
            }
            Some(())
        }
        _ => None,
    }
}

fn read_line(
    wkb: &[u8],
    cursor: &mut usize,
    bbox: &mut [f64; 4],
    seen: &mut bool,
    order: u8,
) -> Option<()> {
    let points = read_u32(wkb, cursor, order)?;
    for _ in 0..points {
        read_point_body(wkb, cursor, bbox, seen, order)?;
    }
    Some(())
}

fn read_point(
    wkb: &[u8],
    cursor: &mut usize,
    bbox: &mut [f64; 4],
    seen: &mut bool,
    order: u8,
) -> Option<()> {
    read_point_body(wkb, cursor, bbox, seen, order)
}

fn read_point_body(
    wkb: &[u8],
    cursor: &mut usize,
    bbox: &mut [f64; 4],
    seen: &mut bool,
    order: u8,
) -> Option<()> {
    let x = read_f64(wkb, cursor, order)?;
    let y = read_f64(wkb, cursor, order)?;
    if x.is_finite() && y.is_finite() {
        *seen = true;
        bbox[0] = bbox[0].min(x);
        bbox[1] = bbox[1].min(y);
        bbox[2] = bbox[2].max(x);
        bbox[3] = bbox[3].max(y);
    }
    Some(())
}

fn read_u32(wkb: &[u8], cursor: &mut usize, order: u8) -> Option<u32> {
    let raw: [u8; 4] = wkb.get(*cursor..*cursor + 4)?.try_into().ok()?;
    *cursor += 4;
    Some(if order == 1 {
        u32::from_le_bytes(raw)
    } else {
        u32::from_be_bytes(raw)
    })
}

fn read_f64(wkb: &[u8], cursor: &mut usize, order: u8) -> Option<f64> {
    let raw: [u8; 8] = wkb.get(*cursor..*cursor + 8)?.try_into().ok()?;
    *cursor += 8;
    Some(if order == 1 {
        f64::from_le_bytes(raw)
    } else {
        f64::from_be_bytes(raw)
    })
}

#[cfg(test)]
mod tests {
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
