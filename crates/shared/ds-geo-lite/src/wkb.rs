//! Canonical 2D WKB encoding and bounded native decoding.
//!
//! Canonical codec shared by cleaning and columnar output. The byte layout is
//! part of the streaming contract the browser already reads, so it is fixed:
//! byte order flag `1`, u32 type code, then the body. XY only — a third
//! ordinate is carried in a property column, never in the geometry.

use geozero::wkb::{WkbDialect, WkbWriter};
use geozero::{GeomProcessor, error::GeozeroError};
use serde_json::Value;

pub fn geometry_to_wkb(geometry: &Value) -> Option<Vec<u8>> {
    let kind = geometry.get("type")?.as_str()?;
    if kind == "GeometryCollection" {
        return None;
    }
    geometry_to_wkb_with_collections(geometry)
}

/// Encode the same canonical XY bytes, also admitting GeoPackage collections.
/// Extra ordinates remain outside the geometry, as in the columnar profile.
pub fn geometry_to_wkb_with_collections(geometry: &Value) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let mut writer = WkbWriter::new(&mut bytes, WkbDialect::Wkb);
    process_geometry(geometry, &mut writer, 0, 0).ok()?;
    Some(bytes)
}

/// Decode one complete 2D WKB geometry for native columnar consumers.
///
/// Unlike the compatibility envelope reader this refuses partial, non-finite,
/// extended-dimensional and trailing data. `max_nodes` bounds the combined
/// number of geometries, rings and coordinates, including empty parts. CRS
/// and geometric validity remain the source/mask authority's responsibility.
pub fn geometry_from_wkb(wkb: &[u8], max_nodes: usize) -> Result<Value, String> {
    decode(wkb, max_nodes, false, false)
}

/// Decode ISO/EWKB dimensions for source interchange. Every ordinate is checked
/// for finiteness even when the browser profile retains only XY. This keeps the
/// original 64-level native limit; strict columnar decoding retains its limit.
pub fn geometry_from_extended_wkb(
    wkb: &[u8],
    max_nodes: usize,
    preserve_dimensions: bool,
) -> Result<Value, String> {
    decode(wkb, max_nodes, true, preserve_dimensions)
}

fn decode(
    wkb: &[u8],
    max_nodes: usize,
    extended: bool,
    preserve_dimensions: bool,
) -> Result<Value, String> {
    let mut decoder = GeometryDecoder {
        bytes: wkb,
        cursor: 0,
        remaining: max_nodes,
        extended,
        preserve_dimensions,
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
    extended: bool,
    preserve_dimensions: bool,
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
            return Err("WKB count exceeds the remaining geometry bytes or node budget".into());
        }
        Ok(count)
    }

    fn point(&mut self, order: u8, dimensions: usize) -> Result<Value, String> {
        self.admit(1)?;
        let mut position = Vec::with_capacity(dimensions);
        for dimension in 0..dimensions {
            let value =
                read_f64(self.bytes, &mut self.cursor, order).ok_or("truncated WKB coordinate")?;
            if !value.is_finite() {
                return Err("WKB coordinates must be finite".into());
            }
            if self.preserve_dimensions || dimension < 2 {
                position.push(Value::from(value));
            }
        }
        Ok(Value::Array(position))
    }

    fn line(&mut self, order: u8, dimensions: usize) -> Result<Value, String> {
        let count = self.count(order, dimensions * 8)?;
        let mut points = Vec::with_capacity(count);
        for _ in 0..count {
            points.push(self.point(order, dimensions)?);
        }
        Ok(Value::Array(points))
    }

    fn geometry(&mut self, depth: usize, expected: Option<u32>) -> Result<Value, String> {
        if depth > if self.extended { 64 } else { MAX_WKB_DEPTH } {
            return Err("WKB nesting limit exceeded".into());
        }
        self.admit(1)?;
        let order = *self.bytes.get(self.cursor).ok_or("truncated WKB header")?;
        self.cursor += 1;
        if order > 1 {
            return Err("invalid WKB byte order".into());
        }
        let raw = read_u32(self.bytes, &mut self.cursor, order).ok_or("truncated WKB type")?;
        let (code, dimensions) = if self.extended {
            if raw & 0x2000_0000 != 0 {
                read_u32(self.bytes, &mut self.cursor, order).ok_or("truncated WKB SRID")?;
            }
            let family = ((raw & 0xffff) % 4000) / 1000;
            let dimensions = 2
                + usize::from(raw & 0x8000_0000 != 0 || matches!(family, 1 | 3))
                + usize::from(raw & 0x4000_0000 != 0 || matches!(family, 2 | 3));
            ((raw & 0xffff) % 1000, dimensions)
        } else {
            (raw, 2)
        };
        if expected.is_some_and(|kind| kind != code) {
            return Err("WKB multipart member type mismatch".into());
        }
        let (kind, coordinates) = match code {
            1 => ("Point", self.point(order, dimensions)?),
            2 => ("LineString", self.line(order, dimensions)?),
            3 => {
                let count = self.count(order, 4)?;
                self.admit(count)?;
                let mut rings = Vec::with_capacity(count);
                for _ in 0..count {
                    rings.push(self.line(order, dimensions)?);
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

// Stream admitted JSON coordinates directly into the upstream writer. A text
// round trip would reparse floats and would reject otherwise ignored ordinates.
fn process_geometry<P: GeomProcessor>(
    geometry: &Value,
    writer: &mut P,
    depth: usize,
    idx: usize,
) -> geozero::error::Result<()> {
    if depth > 64 {
        return Err(GeozeroError::Geometry("WKB nesting limit exceeded".into()));
    }
    let kind = geometry.get("type").and_then(Value::as_str).unwrap_or("");
    if kind == "GeometryCollection" {
        let parts = parts(&geometry["geometries"])?;
        writer.geometrycollection_begin(parts.len(), idx)?;
        for (idx, part) in parts.iter().enumerate() {
            process_geometry(part, writer, depth + 1, idx)?;
        }
        return writer.geometrycollection_end(idx);
    }
    process_coordinates(kind, &geometry["coordinates"], writer, idx)
}

fn process_coordinates<P: GeomProcessor>(
    kind: &str,
    coordinates: &Value,
    writer: &mut P,
    idx: usize,
) -> geozero::error::Result<()> {
    match kind {
        "Point" => {
            writer.point_begin(idx)?;
            process_position(coordinates, writer, 0)?;
            writer.point_end(idx)
        }
        "LineString" => process_line(coordinates, writer, true, idx),
        "Polygon" => process_polygon(coordinates, writer, true, idx),
        "MultiPoint" | "MultiLineString" | "MultiPolygon" => {
            let parts = parts(coordinates)?;
            match kind {
                "MultiPoint" => writer.multipoint_begin(parts.len(), idx)?,
                "MultiLineString" => writer.multilinestring_begin(parts.len(), idx)?,
                _ => writer.multipolygon_begin(parts.len(), idx)?,
            }
            for (part_idx, part) in parts.iter().enumerate() {
                match kind {
                    "MultiPoint" => process_position(part, writer, part_idx)?,
                    "MultiLineString" => process_line(part, writer, false, part_idx)?,
                    _ => process_polygon(part, writer, false, part_idx)?,
                }
            }
            match kind {
                "MultiPoint" => writer.multipoint_end(idx),
                "MultiLineString" => writer.multilinestring_end(idx),
                _ => writer.multipolygon_end(idx),
            }
        }
        _ => Err(GeozeroError::Geometry("unsupported geometry type".into())),
    }
}

fn parts(value: &Value) -> geozero::error::Result<&[Value]> {
    value
        .as_array()
        .filter(|parts| u32::try_from(parts.len()).is_ok())
        .map(Vec::as_slice)
        .ok_or_else(|| GeozeroError::Geometry("invalid geometry coordinates or part count".into()))
}

fn process_position<P: GeomProcessor>(
    value: &Value,
    writer: &mut P,
    idx: usize,
) -> geozero::error::Result<()> {
    let position = parts(value)?;
    let ordinate = |idx| {
        position
            .get(idx)
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite())
            .ok_or_else(|| GeozeroError::Geometry("invalid XY coordinate".into()))
    };
    writer.xy(ordinate(0)?, ordinate(1)?, idx)
}

fn process_line<P: GeomProcessor>(
    value: &Value,
    writer: &mut P,
    tagged: bool,
    idx: usize,
) -> geozero::error::Result<()> {
    let positions = parts(value)?;
    writer.linestring_begin(tagged, positions.len(), idx)?;
    for (idx, position) in positions.iter().enumerate() {
        process_position(position, writer, idx)?;
    }
    writer.linestring_end(tagged, idx)
}

fn process_polygon<P: GeomProcessor>(
    value: &Value,
    writer: &mut P,
    tagged: bool,
    idx: usize,
) -> geozero::error::Result<()> {
    let rings = parts(value)?;
    writer.polygon_begin(tagged, rings.len(), idx)?;
    for (idx, ring) in rings.iter().enumerate() {
        process_line(ring, writer, false, idx)?;
    }
    writer.polygon_end(tagged, idx)
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
mod tests;
