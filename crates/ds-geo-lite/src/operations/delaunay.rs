//! Dependency-free Delaunay triangulation for WGS-84 `[lng, lat, z]` terrain.

use crate::types::EARTH_RADIUS_M;

const POINT_EPS_M: f64 = 1e-6;
const AREA_EPS_M2: f64 = 1e-12;

#[derive(Clone, Copy, Debug)]
struct Vertex {
    x: f64,
    y: f64,
    source_index: Option<usize>,
}

#[derive(Clone, Copy, Debug)]
struct Edge {
    a: usize,
    b: usize,
}

#[derive(Clone, Copy, Debug)]
struct Triangle {
    a: usize,
    b: usize,
    c: usize,
}

/// Returns triangle indexes into the input triplets: `[i0, i1, i2, ...]`.
/// Empty means malformed, too sparse, or degenerate/collinear input.
pub fn triangulate_terrain(points_flat: &[f64]) -> Vec<u32> {
    let Some(vertices) = terrain_vertices(points_flat) else {
        return Vec::new();
    };
    triangulate_vertices(vertices)
}

/// The same triangulation core for already admitted local Cartesian points.
/// The caller owns units, finite-value admission and duplicate handling.
pub(crate) fn triangulate_projected_xy(points: &[[f64; 2]]) -> Vec<u32> {
    triangulate_vertices(
        points
            .iter()
            .enumerate()
            .map(|(source_index, xy)| Vertex {
                x: xy[0],
                y: xy[1],
                source_index: Some(source_index),
            })
            .collect(),
    )
}

fn triangulate_vertices(mut vertices: Vec<Vertex>) -> Vec<u32> {
    if vertices.len() < 3 {
        return Vec::new();
    }
    if vertices
        .iter()
        .filter_map(|p| p.source_index)
        .any(|idx| idx > u32::MAX as usize)
    {
        return Vec::new();
    }

    let point_count = vertices.len();
    let Some(super_triangle) = append_super_triangle(&mut vertices) else {
        return Vec::new();
    };
    let mut triangles = vec![super_triangle];

    for point_idx in 0..point_count {
        let point = vertices[point_idx];
        let mut bad = vec![false; triangles.len()];
        let mut boundary: Vec<Edge> = Vec::new();

        for (tri_idx, tri) in triangles.iter().copied().enumerate() {
            if circumcircle_contains(&vertices, tri, point) {
                bad[tri_idx] = true;
                push_boundary_edge(&mut boundary, Edge { a: tri.a, b: tri.b });
                push_boundary_edge(&mut boundary, Edge { a: tri.b, b: tri.c });
                push_boundary_edge(&mut boundary, Edge { a: tri.c, b: tri.a });
            }
        }

        let mut next = Vec::with_capacity(triangles.len() + boundary.len());
        for (idx, tri) in triangles.iter().copied().enumerate() {
            if !bad[idx] {
                next.push(tri);
            }
        }
        for edge in boundary {
            if let Some(tri) = oriented_triangle(&vertices, edge.a, edge.b, point_idx) {
                next.push(tri);
            }
        }
        triangles = next;
    }

    let mut out = Vec::with_capacity(triangles.len() * 3);
    for tri in triangles {
        if tri.a >= point_count || tri.b >= point_count || tri.c >= point_count {
            continue;
        }
        if oriented_area2(&vertices, tri.a, tri.b, tri.c).abs() <= AREA_EPS_M2 {
            continue;
        }
        let Some(a) = vertices[tri.a].source_index else {
            continue;
        };
        let Some(b) = vertices[tri.b].source_index else {
            continue;
        };
        let Some(c) = vertices[tri.c].source_index else {
            continue;
        };
        out.extend([a as u32, b as u32, c as u32]);
    }
    out
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
            source_index: Some(source_index),
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
            .map(|last| distance2(*last, point) <= POINT_EPS_M * POINT_EPS_M)
            .unwrap_or(false);
        if !duplicate {
            unique.push(point);
        }
    }
    Some(unique)
}

fn append_super_triangle(vertices: &mut Vec<Vertex>) -> Option<Triangle> {
    let mut min_x = f64::INFINITY;
    let mut min_y = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_y = f64::NEG_INFINITY;
    for p in vertices.iter() {
        min_x = min_x.min(p.x);
        min_y = min_y.min(p.y);
        max_x = max_x.max(p.x);
        max_y = max_y.max(p.y);
    }

    let delta = (max_x - min_x).max(max_y - min_y);
    if !delta.is_finite() || delta <= POINT_EPS_M {
        return None;
    }
    let mid_x = (min_x + max_x) * 0.5;
    let mid_y = (min_y + max_y) * 0.5;
    let base = vertices.len();
    vertices.push(Vertex {
        x: mid_x - 20.0 * delta,
        y: mid_y - delta,
        source_index: None,
    });
    vertices.push(Vertex {
        x: mid_x,
        y: mid_y + 20.0 * delta,
        source_index: None,
    });
    vertices.push(Vertex {
        x: mid_x + 20.0 * delta,
        y: mid_y - delta,
        source_index: None,
    });
    oriented_triangle(vertices, base, base + 1, base + 2)
}

fn push_boundary_edge(edges: &mut Vec<Edge>, edge: Edge) {
    if let Some(pos) = edges.iter().position(|other| {
        (other.a == edge.a && other.b == edge.b) || (other.a == edge.b && other.b == edge.a)
    }) {
        edges.remove(pos);
    } else {
        edges.push(edge);
    }
}

fn oriented_triangle(vertices: &[Vertex], a: usize, b: usize, c: usize) -> Option<Triangle> {
    let area2 = oriented_area2(vertices, a, b, c);
    if area2.abs() <= AREA_EPS_M2 {
        return None;
    }
    if area2 > 0.0 {
        Some(Triangle { a, b, c })
    } else {
        Some(Triangle { a, b: c, c: b })
    }
}

fn oriented_area2(vertices: &[Vertex], a: usize, b: usize, c: usize) -> f64 {
    let pa = vertices[a];
    let pb = vertices[b];
    let pc = vertices[c];
    (pb.x - pa.x) * (pc.y - pa.y) - (pb.y - pa.y) * (pc.x - pa.x)
}

fn circumcircle_contains(vertices: &[Vertex], tri: Triangle, point: Vertex) -> bool {
    let a = vertices[tri.a];
    let b = vertices[tri.b];
    let c = vertices[tri.c];
    let d = 2.0 * (a.x * (b.y - c.y) + b.x * (c.y - a.y) + c.x * (a.y - b.y));
    if d.abs() <= AREA_EPS_M2 {
        return false;
    }

    let a2 = a.x * a.x + a.y * a.y;
    let b2 = b.x * b.x + b.y * b.y;
    let c2 = c.x * c.x + c.y * c.y;
    let ux = (a2 * (b.y - c.y) + b2 * (c.y - a.y) + c2 * (a.y - b.y)) / d;
    let uy = (a2 * (c.x - b.x) + b2 * (a.x - c.x) + c2 * (b.x - a.x)) / d;
    let center = Vertex {
        x: ux,
        y: uy,
        source_index: None,
    };
    let r2 = distance2(center, a);
    let p2 = distance2(center, point);
    p2 <= r2 + r2.max(1.0) * 1e-12
}

fn distance2(a: Vertex, b: Vertex) -> f64 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
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
    fn returns_empty_for_collinear_or_malformed_input() {
        assert!(
            triangulate_terrain(&[30.0, -2.0, 10.0, 30.001, -2.0, 11.0, 30.002, -2.0, 12.0])
                .is_empty()
        );
        assert!(triangulate_terrain(&[30.0, -2.0]).is_empty());
    }
}
