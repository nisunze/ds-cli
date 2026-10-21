//! Canonical keyed integer triangulation over Spade's public mesh APIs.
//!
//! Spade owns construction, hulls, location and local insertion/removal. DS
//! retains stable handles, bounded integer admission and the lifted key tie
//! rule. A temporary constraint flips an exactly cocircular diagonal; removing
//! it immediately leaves an unconstrained Delaunay mesh. No upstream internals
//! or fork are used. Callers retain terrain support and gap admission.

use spade::{ConstrainedDelaunayTriangulation, HasPosition, Point2, Triangulation};
use std::collections::HashMap;

/// Largest admitted coordinate magnitude. Every predicate term then fits
/// `i128` exactly: in-circle products are bounded by 2^121 per term.
pub const MAX_EXACT_COORDINATE: i64 = 1 << 29;

/// Stable handle of an admitted vertex. Handles of removed vertices may be
/// reused by later insertions.
pub type VertexHandle = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExactDelaunayError {
    CoordinateOutOfRange,
    /// Another live vertex already occupies the exact position.
    DuplicatePosition {
        existing: VertexHandle,
    },
    UnknownVertex,
}

impl std::fmt::Display for ExactDelaunayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CoordinateOutOfRange => write!(
                f,
                "coordinate magnitude exceeds {MAX_EXACT_COORDINATE} exact units"
            ),
            Self::DuplicatePosition { existing } => {
                write!(f, "position already occupied by vertex {existing}")
            }
            Self::UnknownVertex => write!(f, "vertex is not in the triangulation"),
        }
    }
}

impl std::error::Error for ExactDelaunayError {}

#[derive(Debug, Clone)]
struct Vertex<K> {
    position: (i64, i64),
    key: K,
}

#[derive(Debug, Clone)]
struct MeshPoint {
    position: Point2<f64>,
    id: VertexHandle,
}

impl HasPosition for MeshPoint {
    type Scalar = f64;
    fn position(&self) -> Point2<f64> {
        self.position
    }
}

/// Finite cavity changes, including the DS tie flips. `walked_triangles`
/// counts faces examined by DS normalization; Spade's internal location walks
/// are not exposed. No full triangulation is rebuilt by this adapter.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExactDelaunayWork {
    pub removed_triangles: usize,
    pub created_triangles: usize,
    pub walked_triangles: usize,
    pub rebuilt: bool,
}

/// An unconstrained mesh with exact integer/key canonical topology.
#[derive(Debug, Clone)]
pub struct ExactDelaunay<K> {
    mesh: ConstrainedDelaunayTriangulation<MeshPoint>,
    vertices: Vec<Option<Vertex<K>>>,
    free: Vec<VertexHandle>,
    work: ExactDelaunayWork,
}

impl<K: Ord + Clone> Default for ExactDelaunay<K> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Ord + Clone> ExactDelaunay<K> {
    pub fn new() -> Self {
        Self {
            mesh: ConstrainedDelaunayTriangulation::new(),
            vertices: Vec::new(),
            free: Vec::new(),
            work: ExactDelaunayWork::default(),
        }
    }

    pub fn build(
        points: impl IntoIterator<Item = (i64, i64, K)>,
    ) -> Result<Self, ExactDelaunayError> {
        let mut mesh = Self::new();
        for (x, y, key) in points {
            mesh.insert(x, y, key)?;
        }
        Ok(mesh)
    }

    pub fn vertex_count(&self) -> usize {
        self.mesh.num_vertices()
    }

    pub fn finite_triangle_count(&self) -> usize {
        self.mesh.num_inner_faces()
    }

    pub fn last_work(&self) -> ExactDelaunayWork {
        self.work
    }

    pub fn position(&self, vertex: VertexHandle) -> Option<(i64, i64)> {
        self.vertex(vertex).map(|v| v.position)
    }

    pub fn key(&self, vertex: VertexHandle) -> Option<&K> {
        self.vertex(vertex).map(|v| &v.key)
    }

    pub fn vertex_at(&self, x: i64, y: i64) -> Option<VertexHandle> {
        self.mesh
            .locate_vertex(Point2::new(x as f64, y as f64))
            .map(|v| v.data().id)
    }

    /// Finite triangles are counterclockwise stable DS handles.
    pub fn triangles(&self) -> impl Iterator<Item = [VertexHandle; 3]> + '_ {
        self.mesh
            .inner_faces()
            .map(|f| f.vertices().map(|v| v.data().id))
    }

    pub fn canonical_triangles(&self) -> Vec<[K; 3]> {
        let mut triangles: Vec<_> = self
            .triangles()
            .map(|t| {
                let keys = t.map(|v| self.key(v).unwrap().clone());
                let first = (0..3).min_by_key(|i| &keys[*i]).unwrap();
                std::array::from_fn(|i| keys[(first + i) % 3].clone())
            })
            .collect();
        triangles.sort();
        triangles
    }

    /// Finite neighbors in counterclockwise order, with unspecified start.
    pub fn neighbors(&self, vertex: VertexHandle) -> Vec<VertexHandle> {
        if self.vertex(vertex).is_none() || self.finite_triangle_count() == 0 {
            return Vec::new();
        }
        self.mesh
            .vertex(self.handle(vertex))
            .out_edges()
            .map(|e| e.to().data().id)
            .collect()
    }

    pub fn insert(&mut self, x: i64, y: i64, key: K) -> Result<VertexHandle, ExactDelaunayError> {
        if x.unsigned_abs() > MAX_EXACT_COORDINATE as u64
            || y.unsigned_abs() > MAX_EXACT_COORDINATE as u64
        {
            return Err(ExactDelaunayError::CoordinateOutOfRange);
        }
        if let Some(existing) = self.vertex_at(x, y) {
            return Err(ExactDelaunayError::DuplicatePosition { existing });
        }
        let before = self.finite_triangle_count();
        let id = self.free.pop().unwrap_or(self.vertices.len() as u32);
        if id as usize == self.vertices.len() {
            self.vertices.push(None);
        }
        self.vertices[id as usize] = Some(Vertex {
            position: (x, y),
            key,
        });
        let inserted = self
            .mesh
            .insert(MeshPoint {
                position: Point2::new(x as f64, y as f64),
                id,
            })
            .expect("distinct bounded integer coordinates are admitted by Spade");
        let created = self
            .mesh
            .vertex(inserted)
            .out_edges()
            .filter(|e| !e.is_outer_edge())
            .count();
        self.work = ExactDelaunayWork {
            created_triangles: created,
            removed_triangles: before + created - self.finite_triangle_count(),
            ..Default::default()
        };
        self.normalize(self.frontier(id));
        Ok(id)
    }

    pub fn remove(&mut self, vertex: VertexHandle) -> Result<K, ExactDelaunayError> {
        self.vertex(vertex)
            .ok_or(ExactDelaunayError::UnknownVertex)?;
        let handle = self.handle(vertex);
        let neighbors: Vec<_> = self
            .mesh
            .vertex(handle)
            .out_edges()
            .map(|e| e.to().data().id)
            .collect();
        let before = self.finite_triangle_count();
        let removed = self
            .mesh
            .vertex(handle)
            .out_edges()
            .filter(|e| !e.is_outer_edge())
            .count();
        self.mesh.remove(handle);
        let key = self.vertices[vertex as usize].take().unwrap().key;
        self.free.push(vertex);
        self.work = ExactDelaunayWork {
            removed_triangles: removed,
            created_triangles: self.finite_triangle_count() + removed - before,
            ..Default::default()
        };
        // Re-locate every stable ID: upstream removal invalidates its handles.
        let frontier = neighbors
            .into_iter()
            .flat_map(|v| self.frontier(v))
            .collect();
        self.normalize(frontier);
        Ok(key)
    }

    /// Independent O(vertices x triangles) audit; never an interactive path.
    pub fn verify(&self) -> Result<(), String> {
        if self.mesh.num_constraints() != 0 {
            return Err("temporary tie constraint escaped normalization".into());
        }
        let live: Vec<_> = (0..self.vertices.len() as u32)
            .filter(|v| self.vertex(*v).is_some())
            .collect();
        if live.len() != self.vertex_count()
            || live.iter().any(|v| {
                let (x, y) = self.position(*v).unwrap();
                self.vertex_at(x, y) != Some(*v)
            })
        {
            return Err("stable vertex identity mismatch".into());
        }
        if self.finite_triangle_count() == 0 {
            if live.len() > 2
                && live
                    .iter()
                    .any(|v| orient(self.coord(live[0]), self.coord(live[1]), self.coord(*v)) != 0)
            {
                return Err("noncollinear points without triangles".into());
            }
            return Ok(());
        }
        let mut edges = HashMap::new();
        for tri in self.triangles() {
            if orient(self.coord(tri[0]), self.coord(tri[1]), self.coord(tri[2])) <= 0 {
                return Err(format!("triangle {tri:?} is not counterclockwise"));
            }
            for v in &live {
                if !tri.contains(v) && self.in_circle(tri, *v) > 0 {
                    return Err(format!("vertex {v} lies inside triangle {tri:?}"));
                }
            }
            for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                if edges.insert((a, b), ()).is_some() {
                    return Err("duplicate directed triangle edge".into());
                }
            }
        }
        let hull = edges
            .keys()
            .filter(|(a, b)| !edges.contains_key(&(*b, *a)))
            .count();
        if hull != self.mesh.convex_hull_size()
            || self.finite_triangle_count() != 2 * live.len() - hull - 2
        {
            return Err("nonmanifold hull or incorrect Euler triangle count".into());
        }
        Ok(())
    }

    fn vertex(&self, vertex: VertexHandle) -> Option<&Vertex<K>> {
        self.vertices.get(vertex as usize).and_then(Option::as_ref)
    }

    fn coord(&self, vertex: VertexHandle) -> (i64, i64) {
        self.position(vertex).expect("live vertex")
    }

    fn handle(&self, vertex: VertexHandle) -> spade::handles::FixedVertexHandle {
        let (x, y) = self.coord(vertex);
        self.mesh
            .locate_vertex(Point2::new(x as f64, y as f64))
            .expect("live mesh vertex")
            .fix()
    }

    fn frontier(&self, vertex: VertexHandle) -> Vec<(VertexHandle, VertexHandle)> {
        self.mesh
            .vertex(self.handle(vertex))
            .out_edges()
            .flat_map(|e| {
                [
                    (e.from().data().id, e.to().data().id),
                    (e.next().from().data().id, e.next().to().data().id),
                ]
            })
            .collect()
    }

    fn normalize(&mut self, mut pending: Vec<(VertexHandle, VertexHandle)>) {
        while let Some((a, b)) = pending.pop() {
            let Some(edge) = self
                .mesh
                .get_edge_from_neighbors(self.handle(a), self.handle(b))
            else {
                continue;
            };
            let (Some(c), Some(d)) = (edge.opposite_vertex(), edge.rev().opposite_vertex()) else {
                continue;
            };
            let (c, d) = (c.data().id, d.data().id);
            self.work.walked_triangles += 2;
            if in_circle_exact(self.coord(a), self.coord(b), self.coord(c), self.coord(d)) != 0
                || self.in_circle([a, b, c], d) <= 0
            {
                continue;
            }
            let (c_handle, d_handle) = (self.handle(c), self.handle(d));
            assert!(self.mesh.add_constraint(c_handle, d_handle));
            let diagonal = self
                .mesh
                .get_edge_from_neighbors(c_handle, d_handle)
                .expect("convex cocircular diagonal")
                .as_undirected()
                .fix();
            assert!(self.mesh.remove_constraint_edge(diagonal));
            self.work.created_triangles += 2;
            self.work.removed_triangles += 2;
            pending.extend([(a, c), (c, b), (b, d), (d, a)]);
        }
    }

    /// Sign of the exact determinant with lifted perturbations in key order.
    fn in_circle(&self, tri: [u32; 3], d: u32) -> i32 {
        let points = [tri[0], tri[1], tri[2], d];
        let c = points.map(|v| self.coord(v));
        let exact = in_circle_exact(c[0], c[1], c[2], c[3]);
        if exact != 0 {
            return exact.signum() as i32;
        }
        let mut order = [0usize, 1, 2, 3];
        order.sort_by_key(|i| self.key(points[*i]).unwrap());
        for row in order {
            let other: Vec<_> = (0..4).filter(|i| *i != row).map(|i| c[i]).collect();
            let minor = orient(other[0], other[1], other[2]);
            if minor != 0 {
                return (if row % 2 == 0 { 1 } else { -1 }) * minor.signum() as i32;
            }
        }
        unreachable!("four distinct points cannot have every triple collinear")
    }
}

/// Twice the signed area of `abc`; positive when counterclockwise.
pub fn orient(a: (i64, i64), b: (i64, i64), c: (i64, i64)) -> i128 {
    let (ax, ay) = (a.0 as i128, a.1 as i128);
    let (bx, by) = (b.0 as i128, b.1 as i128);
    let (cx, cy) = (c.0 as i128, c.1 as i128);
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

/// Exact in-circle determinant: positive when `d` lies strictly inside the
/// circle through counterclockwise `a`, `b`, `c`.
pub fn in_circle_exact(a: (i64, i64), b: (i64, i64), c: (i64, i64), d: (i64, i64)) -> i128 {
    let lift = |p: (i64, i64)| {
        let (x, y) = ((p.0 - d.0) as i128, (p.1 - d.1) as i128);
        (x, y, x * x + y * y)
    };
    let (adx, ady, al) = lift(a);
    let (bdx, bdy, bl) = lift(b);
    let (cdx, cdy, cl) = lift(c);
    adx * (bdy * cl - cdy * bl) - ady * (bdx * cl - cdx * bl) + al * (bdx * cdy - cdx * bdy)
}
