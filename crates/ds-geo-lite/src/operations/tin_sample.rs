//! Barycentric z sampling over a flat WGS-84 terrain TIN packet.

use crate::types::EARTH_RADIUS_M;

const HEADER_LEN: usize = 2;
const POINT_STRIDE: usize = 3;
const TRIANGLE_STRIDE: usize = 3;
const BARY_EPS: f64 = 1e-10;

#[derive(Clone, Copy)]
struct TerrainPoint {
    lng: f64,
    lat: f64,
    z: f64,
}

#[derive(Clone, Copy)]
struct ProjectedPoint {
    x: f64,
    y: f64,
    z: f64,
}

#[derive(Clone, Copy)]
struct Triangle {
    a: usize,
    b: usize,
    c: usize,
}

struct Tin {
    points: Vec<ProjectedPoint>,
    triangles: Vec<Triangle>,
    origin_lng: f64,
    origin_lat: f64,
    cos_lat: f64,
}

/// Sample z at flat query points `[lng, lat, ...]`.
///
/// `tin_flat` packet:
/// `[point_count, triangle_count, lng, lat, z, ..., i0, i1, i2, ...]`.
/// Returns one z per query, with `NaN` when outside the TIN or malformed.
pub fn sample_many(tin_flat: &[f64], pts_flat: &[f64]) -> Vec<f64> {
    if !pts_flat.len().is_multiple_of(2) {
        return Vec::new();
    }
    let query_count = pts_flat.len() / 2;
    let Some(tin) = parse_tin(tin_flat) else {
        return vec![f64::NAN; query_count];
    };

    let mut out = Vec::with_capacity(query_count);
    for chunk in pts_flat.chunks_exact(2) {
        let lng = chunk[0];
        let lat = chunk[1];
        if !lng.is_finite() || !lat.is_finite() {
            out.push(f64::NAN);
            continue;
        }
        let p = tin.project(lng, lat);
        out.push(sample_projected(&tin, p).unwrap_or(f64::NAN));
    }
    out
}

fn parse_tin(flat: &[f64]) -> Option<Tin> {
    if flat.len() < HEADER_LEN {
        return None;
    }
    let point_count = parse_count(flat[0])?;
    let triangle_count = parse_count(flat[1])?;
    let point_values = point_count.checked_mul(POINT_STRIDE)?;
    let triangle_values = triangle_count.checked_mul(TRIANGLE_STRIDE)?;
    let expected = HEADER_LEN
        .checked_add(point_values)?
        .checked_add(triangle_values)?;
    if flat.len() != expected || point_count < 3 || triangle_count == 0 {
        return None;
    }

    let points_start = HEADER_LEN;
    let triangles_start = points_start + point_values;
    let raw_points = parse_points(&flat[points_start..triangles_start])?;
    let origin_lng = raw_points.iter().map(|p| p.lng).sum::<f64>() / point_count as f64;
    let origin_lat = raw_points.iter().map(|p| p.lat).sum::<f64>() / point_count as f64;
    let cos_lat = origin_lat.to_radians().cos();
    if !cos_lat.is_finite() || cos_lat.abs() < 1e-9 {
        return None;
    }

    let points = raw_points
        .into_iter()
        .map(|p| project_point(p, origin_lng, origin_lat, cos_lat))
        .collect();
    let triangles = parse_triangles(&flat[triangles_start..], point_count)?;
    Some(Tin {
        points,
        triangles,
        origin_lng,
        origin_lat,
        cos_lat,
    })
}

fn parse_count(value: f64) -> Option<usize> {
    if !value.is_finite() || value < 0.0 || value.fract() != 0.0 {
        return None;
    }
    Some(value as usize)
}

fn parse_points(values: &[f64]) -> Option<Vec<TerrainPoint>> {
    let mut points = Vec::with_capacity(values.len() / POINT_STRIDE);
    for chunk in values.chunks_exact(POINT_STRIDE) {
        let p = TerrainPoint {
            lng: chunk[0],
            lat: chunk[1],
            z: chunk[2],
        };
        if !p.lng.is_finite() || !p.lat.is_finite() || !p.z.is_finite() {
            return None;
        }
        points.push(p);
    }
    Some(points)
}

fn parse_triangles(values: &[f64], point_count: usize) -> Option<Vec<Triangle>> {
    let mut triangles = Vec::with_capacity(values.len() / TRIANGLE_STRIDE);
    for chunk in values.chunks_exact(TRIANGLE_STRIDE) {
        let a = parse_index(chunk[0], point_count)?;
        let b = parse_index(chunk[1], point_count)?;
        let c = parse_index(chunk[2], point_count)?;
        if a == b || b == c || a == c {
            return None;
        }
        triangles.push(Triangle { a, b, c });
    }
    Some(triangles)
}

fn parse_index(value: f64, point_count: usize) -> Option<usize> {
    let idx = parse_count(value)?;
    (idx < point_count).then_some(idx)
}

fn project_point(
    point: TerrainPoint,
    origin_lng: f64,
    origin_lat: f64,
    cos_lat: f64,
) -> ProjectedPoint {
    ProjectedPoint {
        x: (point.lng - origin_lng).to_radians() * EARTH_RADIUS_M * cos_lat,
        y: (point.lat - origin_lat).to_radians() * EARTH_RADIUS_M,
        z: point.z,
    }
}

impl Tin {
    fn project(&self, lng: f64, lat: f64) -> ProjectedPoint {
        project_point(
            TerrainPoint { lng, lat, z: 0.0 },
            self.origin_lng,
            self.origin_lat,
            self.cos_lat,
        )
    }
}

fn sample_projected(tin: &Tin, p: ProjectedPoint) -> Option<f64> {
    for tri in &tin.triangles {
        let a = tin.points[tri.a];
        let b = tin.points[tri.b];
        let c = tin.points[tri.c];
        if let Some((wa, wb, wc)) = barycentric(a, b, c, p) {
            return Some(wa * a.z + wb * b.z + wc * c.z);
        }
    }
    None
}

fn barycentric(
    a: ProjectedPoint,
    b: ProjectedPoint,
    c: ProjectedPoint,
    p: ProjectedPoint,
) -> Option<(f64, f64, f64)> {
    let den = (b.y - c.y) * (a.x - c.x) + (c.x - b.x) * (a.y - c.y);
    if den.abs() <= f64::EPSILON {
        return None;
    }
    let wa = ((b.y - c.y) * (p.x - c.x) + (c.x - b.x) * (p.y - c.y)) / den;
    let wb = ((c.y - a.y) * (p.x - c.x) + (a.x - c.x) * (p.y - c.y)) / den;
    let wc = 1.0 - wa - wb;
    if wa >= -BARY_EPS && wb >= -BARY_EPS && wc >= -BARY_EPS {
        Some((wa, wb, wc))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple_tin() -> Vec<f64> {
        vec![
            3.0, 1.0, 30.0, -2.0, 100.0, 30.001, -2.0, 110.0, 30.0, -1.999, 130.0, 0.0, 1.0, 2.0,
        ]
    }

    #[test]
    fn samples_inside_triangle_with_barycentric_z() {
        let z = sample_many(&simple_tin(), &[30.00025, -1.99975]);
        assert_eq!(z.len(), 1);
        assert!((z[0] - 110.0).abs() < 0.2, "z={}", z[0]);
    }

    #[test]
    fn returns_nan_outside_tin() {
        let z = sample_many(&simple_tin(), &[30.01, -2.01]);
        assert_eq!(z.len(), 1);
        assert!(z[0].is_nan());
    }

    #[test]
    fn malformed_tin_returns_nan_per_query() {
        let z = sample_many(&[3.0, 1.0, 30.0], &[30.0, -2.0, 30.1, -2.1]);
        assert_eq!(z.len(), 2);
        assert!(z.iter().all(|v| v.is_nan()));
    }

    #[test]
    fn malformed_queries_return_empty() {
        assert!(sample_many(&simple_tin(), &[30.0]).is_empty());
    }
}
