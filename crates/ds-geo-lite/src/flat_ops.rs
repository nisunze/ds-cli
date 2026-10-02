use crate::{coords_to_flat, flat_to_coords, operations, types::Coord};

/// Classify independently-stationed profile alignments.
///
/// `lines_packed`: `[line_count, vertex_count, lng, lat, ..., vertex_count, ...]`.
/// Returns six-tuples
/// `[line_index, vertex_index_or_-1, role_code, station_m, lng, lat, ...]`,
/// where role codes are `0=start, 1=end, 2=angle_point, 3=tap`. A `-1` vertex
/// index identifies a virtual tap projected into a receiving segment.
#[cfg(feature = "profile")]
pub fn classify_profile_stations(lines_packed: &[f64]) -> Vec<f64> {
    let Some(lines) = unpack_profile_lines(lines_packed) else {
        return Vec::new();
    };
    let stations = operations::profile_stations::classify_profile_stations(&lines);
    let mut out = Vec::with_capacity(stations.len() * 6);
    for station in stations {
        out.extend_from_slice(&[
            station.line_index as f64,
            station.vertex_index.map_or(-1.0, |index| index as f64),
            station.role.code() as f64,
            station.station_m,
            station.coord.lng,
            station.coord.lat,
        ]);
    }
    out
}

#[cfg(feature = "profile")]
fn exact_packet_count(value: f64) -> Option<usize> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 || value > usize::MAX as f64 {
        return None;
    }
    Some(value as usize)
}

#[cfg(feature = "profile")]
fn unpack_profile_lines(packed: &[f64]) -> Option<Vec<Vec<Coord>>> {
    let line_count = exact_packet_count(*packed.first()?)?;
    let mut offset = 1usize;
    let mut lines = Vec::with_capacity(line_count);
    for _ in 0..line_count {
        let vertex_count = exact_packet_count(*packed.get(offset)?)?;
        offset += 1;
        let coordinate_count = vertex_count.checked_mul(2)?;
        let end = offset.checked_add(coordinate_count)?;
        let flat = packed.get(offset..end)?;
        lines.push(flat_to_coords(flat)?);
        offset = end;
    }
    (offset == packed.len()).then_some(lines)
}

/// Triangulate terrain points into a Delaunay TIN.
///
/// `points_flat`: `[lng, lat, z, lng, lat, z, …]`.
/// Returns flat triangle point indexes `[i0, i1, i2, …]`; indexes reference
/// the input triplet order. Empty on malformed or degenerate input.
#[cfg(feature = "tin")]
pub fn terrain_delaunay(points_flat: &[f64]) -> Vec<u32> {
    operations::delaunay::triangulate_terrain(points_flat)
}

/// Sample a terrain TIN at query points.
///
/// `tin_flat`: `[point_count, triangle_count, lng, lat, z, …, i0, i1, i2, …]`.
/// `pts_flat`: `[lng, lat, lng, lat, …]`.
/// Returns one z per query; no-data is `NaN`.
#[cfg(feature = "tin")]
pub fn tin_sample_many(tin_flat: &[f64], pts_flat: &[f64]) -> Vec<f64> {
    operations::tin_sample::sample_many(tin_flat, pts_flat)
}

/// Project points perpendicularly onto a polyline (profile corridor step).
///
/// `line_flat` / `pts_flat`: `[lng, lat, …]` (WGS-84 degrees). For each input
/// point whose perpendicular offset ≤ `max_offset_m`, emits one 5-tuple
/// `[input_index, station_m, offset_m, projected_lng, projected_lat]`,
/// sorted by station. `input_index` references the input pair order so the
/// caller can re-join per-point payloads (labels, elevations) by index.
#[cfg(feature = "editing")]
pub fn project_points_onto_line(
    line_flat: &[f64],
    pts_flat: &[f64],
    max_offset_m: f64,
) -> Vec<f64> {
    let (Some(line), Some(pts)) = (flat_to_coords(line_flat), flat_to_coords(pts_flat)) else {
        return Vec::new();
    };
    let projected = operations::project_onto::project_onto_line(&line, &pts, max_offset_m);
    let mut out = Vec::with_capacity(projected.len() * 5);
    for p in projected {
        out.push(p.input_index as f64);
        out.push(p.station_m);
        out.push(p.offset_m);
        out.push(p.projected.lng);
        out.push(p.projected.lat);
    }
    out
}

/// Extend a line to meet a target polyline (AutoCAD EXTEND).
///
/// Both inputs are flat [lng, lat, …]. Returns modified line coordinates,
/// or empty if no intersection found.
#[cfg(feature = "editing")]
pub fn extend_line(a_flat: &[f64], b_flat: &[f64]) -> Vec<f64> {
    let (Some(a), Some(b)) = (flat_to_coords(a_flat), flat_to_coords(b_flat)) else {
        return Vec::new();
    };
    match operations::extend::extend_to(&a, &b) {
        Some(c) => coords_to_flat(&c),
        None => vec![],
    }
}

/// Trim a line at its intersection with a cutting edge (AutoCAD TRIM).
///
/// `keep_lng/keep_lat` is the point on the source line whose side to keep.
/// Returns modified line coordinates, or empty if no intersection.
#[cfg(feature = "editing")]
pub fn trim_line(a_flat: &[f64], b_flat: &[f64], keep_lng: f64, keep_lat: f64) -> Vec<f64> {
    if !keep_lng.is_finite() || !keep_lat.is_finite() {
        return Vec::new();
    }
    let (Some(a), Some(b)) = (flat_to_coords(a_flat), flat_to_coords(b_flat)) else {
        return Vec::new();
    };
    let keep = Coord::new(keep_lng, keep_lat);
    match operations::trim::trim_at(&a, &b, keep) {
        Some(c) => coords_to_flat(&c),
        None => vec![],
    }
}

/// Split a line at the point nearest to `split_lng/split_lat`.
///
/// Returns `[n1, …first_coords, …second_coords]` where `n1` is the number
/// of f64 values in the first line. Empty on failure.
#[cfg(feature = "editing")]
pub fn split_line_at_point(coords_flat: &[f64], split_lng: f64, split_lat: f64) -> Vec<f64> {
    if !split_lng.is_finite() || !split_lat.is_finite() {
        return Vec::new();
    }
    let Some(coords) = flat_to_coords(coords_flat) else {
        return Vec::new();
    };
    let at = Coord::new(split_lng, split_lat);
    match operations::split::split_at_point(&coords, at) {
        Some((first, second)) => pack_segments(&[first, second]),
        None => vec![],
    }
}

/// Split a line at every intersection with a cutting line.
///
/// Returns `[count, n1, n2, …, …seg1, …seg2, …]` where `count` is the
/// number of segments, then `count` length values, then all coords.
/// Empty on failure (no intersections).
#[cfg(feature = "editing")]
pub fn split_line_with_cutter(line_flat: &[f64], cutter_flat: &[f64]) -> Vec<f64> {
    let (Some(line), Some(cutter)) = (flat_to_coords(line_flat), flat_to_coords(cutter_flat))
    else {
        return Vec::new();
    };
    match operations::split::split_with_line(&line, &cutter) {
        Some(segments) => {
            let segs: Vec<Vec<Coord>> = segments;
            pack_segments(&segs)
        }
        None => vec![],
    }
}

/// Scatter randomly-spaced points inside an area (survey-mimicking).
///
/// `kind`: 0 = Point, 1 = LineString — both are buffered by `buffer_m` metres
/// into an area first (buffer is INTERNAL; the polygon is never returned).
/// 2 = Polygon — sampled directly, with holes honoured. `rings_packed` is the
/// `pack_segments` format `[ring_count, len0, …, ring0_coords, ring1_coords, …]`
/// (each `len` = coord_count × 2); for kind 0/1 pass a single ring (the point or
/// line coordinates), for kind 2 pass the outer ring followed by any holes.
///
/// `min_spacing_m` is a hard floor between points; `max_spacing_m` (≥ min) sets
/// the coverage/density ceiling. `seed` makes the output reproducible. Returns a
/// flat `[lng, lat, …]`, empty on invalid input or zero coverage.
#[cfg(feature = "sampling")]
pub fn random_points_in_area(
    kind: u32,
    rings_packed: &[f64],
    buffer_m: f64,
    min_spacing_m: f64,
    max_spacing_m: f64,
    seed: f64,
) -> Vec<f64> {
    let Some(rings) = unpack_segments(rings_packed) else {
        return Vec::new();
    };
    if rings.is_empty() {
        return Vec::new();
    }

    let poly_rings: Vec<Vec<Coord>> = match kind {
        2 => rings,
        0 | 1 => {
            if !(buffer_m.is_finite() && buffer_m > 0.0) {
                return Vec::new();
            }
            let seq = &rings[0];
            let ring = match kind {
                0 if seq.len() == 1 => operations::buffer::buffer_point(seq[0], buffer_m, 8),
                1 => operations::buffer::buffer_line(seq, buffer_m, 8),
                _ => Vec::new(),
            };
            if ring.is_empty() {
                return Vec::new();
            }
            vec![ring]
        }
        _ => return Vec::new(),
    };

    coords_to_flat(&operations::random_points::sample(
        &poly_rings,
        min_spacing_m,
        max_spacing_m,
        seed,
    ))
}

/// Offset a polyline by `distance_m` meters (AutoCAD OFFSET) → a parallel
/// polyline. `right_side` = right of travel direction, else left. Miter
/// corners with a bevel fallback. Returns flat `[lng, lat, …]`, empty on
/// failure / `distance_m <= 0`.
#[cfg(feature = "editing")]
pub fn offset_line(coords_flat: &[f64], distance_m: f64, right_side: bool) -> Vec<f64> {
    let Some(coords) = flat_to_coords(coords_flat) else {
        return Vec::new();
    };
    coords_to_flat(&operations::offset::offset_line(
        &coords, distance_m, right_side,
    ))
}

/// Pack multiple coordinate sequences into a flat array.
/// Format: [segment_count, len1, len2, …, …coords1, …coords2, …]
/// where each len is the number of f64 values (coord_count × 2).
#[cfg(any(feature = "editing", feature = "sampling"))]
fn pack_segments(segments: &[Vec<Coord>]) -> Vec<f64> {
    let flats: Vec<Vec<f64>> = segments.iter().map(|s| coords_to_flat(s)).collect();
    let total: usize = flats.iter().map(|f| f.len()).sum();
    let mut out = Vec::with_capacity(1 + segments.len() + total);
    out.push(segments.len() as f64);
    for f in &flats {
        out.push(f.len() as f64);
    }
    for f in &flats {
        out.extend(f);
    }
    out
}

/// Decode the `pack_segments` format back into coordinate rings.
/// `[segment_count, len1, len2, …, …coords1, …coords2, …]`, each `len` an even
/// f64 count. Rejects a truncated packet, an odd segment length, or any
/// non-finite value (the same boundary hygiene as `flat_to_coords`).
#[cfg(feature = "sampling")]
fn unpack_segments(packed: &[f64]) -> Option<Vec<Vec<Coord>>> {
    let &count = packed.first()?;
    if !count.is_finite() || count < 0.0 {
        return None;
    }
    let count = count as usize;
    let header = 1 + count;
    if packed.len() < header {
        return None;
    }

    let mut lens = Vec::with_capacity(count);
    for &len in &packed[1..header] {
        if !len.is_finite() || len < 0.0 {
            return None;
        }
        let len = len as usize;
        if !len.is_multiple_of(2) {
            return None;
        }
        lens.push(len);
    }

    let mut offset = header;
    let mut rings = Vec::with_capacity(count);
    for len in lens {
        let end = offset + len;
        if end > packed.len() {
            return None;
        }
        let seg = &packed[offset..end];
        if seg.iter().any(|value| !value.is_finite()) {
            return None;
        }
        rings.push(
            seg.chunks_exact(2)
                .map(|c| Coord::new(c[0], c[1]))
                .collect(),
        );
        offset = end;
    }
    Some(rings)
}

#[cfg(all(test, feature = "editing", feature = "profile", feature = "sampling"))]
mod tests {
    use super::*;
    use crate::{
        bearing_deg, buffer_geometry, destination_point, find_intersections, haversine_m,
        points_along_line, spherical_polygon_area_m2,
    };

    #[test]
    fn geometry_exports_reject_partial_and_non_finite_coordinate_packets() {
        assert!(points_along_line(&[30.0, -2.0, 30.001], 10.0, true).is_empty());
        assert!(
            project_points_onto_line(&[30.0, -2.0, 30.001, -2.0], &[30.0, f64::NAN], 10.0,)
                .is_empty()
        );
        assert!(
            find_intersections(
                &[30.0, -2.0, 30.001, -2.0],
                &[30.0005, -2.001, f64::INFINITY, -1.999],
            )
            .is_empty()
        );
        assert!(buffer_geometry(0, &[30.0, -2.0, 30.001, -2.0], 10.0, 4).is_empty());
        assert!(buffer_geometry(0, &[30.0, -2.0], f64::INFINITY, 4).is_empty());
        assert!(offset_line(&[30.0, -2.0, 30.001, -2.0], f64::INFINITY, true).is_empty());
        assert_eq!(
            spherical_polygon_area_m2(&[30.0, -2.0, f64::NAN, -2.0]),
            0.0
        );
        assert!(classify_profile_stations(&[1.0, 2.5, 30.0, -2.0]).is_empty());
        assert!(classify_profile_stations(&[1.0, 2.0, 30.0, -2.0]).is_empty());
        assert!(classify_profile_stations(&[1.0, 2.0, 30.0, -2.0, f64::NAN, -2.0]).is_empty());
        assert!(classify_profile_stations(&[1.0, 2.0, 30.0, -2.0, 30.001, -2.0, 999.0]).is_empty());
    }

    #[test]
    fn scalar_geometry_exports_do_not_produce_finite_results_for_non_finite_input() {
        assert!(haversine_m(30.0, -2.0, f64::NAN, -2.0).is_nan());
        assert!(bearing_deg(30.0, -2.0, f64::INFINITY, -2.0).is_nan());
        assert!(destination_point(30.0, -2.0, 90.0, f64::NAN).is_empty());
    }
}
