//! WGS84 admission/projection around Spade's Delaunay triangulation.

use crate::types::EARTH_RADIUS_M;
use spade::{DelaunayTriangulation, HasPosition, Point2, Triangulation};

const POINT_EPS_M: f64 = 1e-6;
const AREA_EPS_M2: f64 = 1e-12;

#[derive(Clone, Copy, Debug)]
struct Vertex {
    x: f64,
    y: f64,
    source_index: usize,
}

impl HasPosition for Vertex {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        spade::mitigate_underflow(Point2::new(self.x, self.y))
    }
}

/// Counterclockwise source-triplet indices. Malformed, sparse or collinear
/// input produces an empty mesh; admission retains the original row identity.
pub fn triangulate_terrain(points_flat: &[f64]) -> Vec<u32> {
    terrain_vertices(points_flat)
        .map(triangulate_vertices)
        .unwrap_or_default()
}

/// Caller-admitted projected metres use the same upstream mesh engine.
pub(crate) fn triangulate_projected_xy(points: &[[f64; 2]]) -> Vec<u32> {
    triangulate_vertices(
        points
            .iter()
            .enumerate()
            .map(|(source_index, xy)| Vertex {
                x: xy[0],
                y: xy[1],
                source_index,
            })
            .collect(),
    )
}

fn triangulate_vertices(vertices: Vec<Vertex>) -> Vec<u32> {
    if vertices.len() < 3 || vertices.iter().any(|v| v.source_index > u32::MAX as usize) {
        return Vec::new();
    }
    let mut mesh = DelaunayTriangulation::<Vertex>::new();
    for point in vertices {
        if mesh.insert(point).is_err() {
            return Vec::new();
        }
    }
    mesh.inner_faces()
        .filter(|f| {
            let [a, b, c] = f.vertices().map(|v| v.position());
            ((b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)).abs() > AREA_EPS_M2
        })
        .flat_map(|f| f.vertices().map(|v| v.data().source_index as u32))
        .collect()
}

fn terrain_vertices(points_flat: &[f64]) -> Option<Vec<Vertex>> {
    if !points_flat.len().is_multiple_of(3) {
        return None;
    }

    let mut raw: Vec<(usize, f64, f64)> = Vec::with_capacity(points_flat.len() / 3);
    for (idx, chunk) in points_flat.chunks_exact(3).enumerate() {
        let lng = chunk[0];
        let lat = chunk[1];
        let z = chunk[2];
        if lng.is_finite() && lat.is_finite() && z.is_finite() {
            raw.push((idx, lng, lat));
        }
    }
    if raw.len() < 3 {
        return Some(Vec::new());
    }

    let origin_lng = raw.iter().map(|(_, lng, _)| *lng).sum::<f64>() / raw.len() as f64;
    let origin_lat = raw.iter().map(|(_, _, lat)| *lat).sum::<f64>() / raw.len() as f64;
    let cos_lat = origin_lat.to_radians().cos();
    if !cos_lat.is_finite() || cos_lat.abs() < 1e-9 {
        return Some(Vec::new());
    }

    let mut projected: Vec<Vertex> = raw
        .into_iter()
        .map(|(source_index, lng, lat)| Vertex {
            x: (lng - origin_lng).to_radians() * EARTH_RADIUS_M * cos_lat,
            y: (lat - origin_lat).to_radians() * EARTH_RADIUS_M,
            source_index,
        })
        .collect();
    projected.sort_by(|a, b| {
        a.x.total_cmp(&b.x)
            .then_with(|| a.y.total_cmp(&b.y))
            .then_with(|| a.source_index.cmp(&b.source_index))
    });

    let mut unique: Vec<Vertex> = Vec::with_capacity(projected.len());
    for point in projected {
        let duplicate = unique
            .last()
            .map(|last| last.position().distance_2(point.position()) <= POINT_EPS_M * POINT_EPS_M)
            .unwrap_or(false);
        if !duplicate {
            unique.push(point);
        }
    }
    Some(unique)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn triangulates_single_triangle() {
        let out = triangulate_terrain(&[30.0, -2.0, 10.0, 30.001, -2.0, 11.0, 30.0, -1.999, 12.0]);
        assert_eq!(out.len(), 3);
        let mut tri = [out[0], out[1], out[2]];
        tri.sort();
        assert_eq!(tri, [0, 1, 2]);
    }

    #[test]
    fn triangulates_square_as_two_triangles() {
        let out = triangulate_terrain(&[
            30.0, -2.0, 10.0, 30.001, -2.0, 11.0, 30.001, -1.999, 12.0, 30.0, -1.999, 13.0,
        ]);
        assert_eq!(out.len(), 6);
        for tri in out.chunks_exact(3) {
            assert!(tri.iter().all(|idx| *idx < 4));
            assert_ne!(tri[0], tri[1]);
            assert_ne!(tri[1], tri[2]);
            assert_ne!(tri[0], tri[2]);
        }
    }

    #[test]
    fn skips_duplicate_xy_points() {
        let out = triangulate_terrain(&[
            30.0, -2.0, 10.0, 30.001, -2.0, 11.0, 30.0, -2.0, 99.0, 30.0, -1.999, 12.0,
        ]);
        assert_eq!(out.len(), 3);
        assert!(!out.contains(&2));
    }

    #[test]
    fn subnormal_projected_coordinates_do_not_discard_the_surface() {
        let out = triangulate_projected_xy(&[[-1.0, 0.0], [1.0, 0.0], [1e-150, 1.0], [0.0, -1.0]]);
        assert_eq!(out.len(), 6);
        assert!((0..4).all(|index| out.contains(&index)));
    }

    #[test]
    fn returns_empty_for_collinear_or_malformed_input() {
        assert!(
            triangulate_terrain(&[30.0, -2.0, 10.0, 30.001, -2.0, 11.0, 30.002, -2.0, 12.0])
                .is_empty()
        );
        assert!(triangulate_terrain(&[30.0, -2.0]).is_empty());
    }
}
