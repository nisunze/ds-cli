//! Exact, canonical, locally updatable planar Delaunay triangulation.
//!
//! Vertices carry integer plan coordinates in caller units (terrain callers
//! use millimetres from a local origin) and a caller key. Orientation and
//! in-circle predicates are evaluated exactly in `i128`. Exactly cocircular
//! configurations are resolved by simulation of simplicity on the lifted
//! coordinate, ordered by key, so the triangulation is a pure function of the
//! vertex set: inserting or removing vertices in any order yields the same
//! triangles as building the final set from scratch.
//!
//! Insertion replaces exactly its conflict cavity; removal replaces exactly
//! the removed vertex's star. Both are local. The convex hull is closed by one
//! symbolic infinite vertex, so no artificial super-triangle limits or alters
//! hull triangles. This primitive knows nothing about terrain, corridors or
//! admission: callers decide which triangles represent supported ground.

use std::collections::HashMap;

/// Largest admitted coordinate magnitude. Every predicate term then fits
/// `i128` exactly: in-circle products are bounded by 2^121 per term.
pub const MAX_EXACT_COORDINATE: i64 = 1 << 29;

const INFINITE: u32 = u32::MAX;

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
    x: i64,
    y: i64,
    key: K,
}

/// Work performed by the last insertion or removal.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExactDelaunayWork {
    pub removed_triangles: usize,
    pub created_triangles: usize,
    pub walked_triangles: usize,
    pub rebuilt: bool,
}

/// One canonical planar Delaunay triangulation over keyed integer points.
#[derive(Debug, Clone)]
pub struct ExactDelaunay<K> {
    vertices: Vec<Option<Vertex<K>>>,
    free_vertices: Vec<u32>,
    positions: HashMap<(i64, i64), u32>,
    triangles: Vec<Option<[u32; 3]>>,
    free_triangles: Vec<u32>,
    /// Directed edge to the one triangle that contains it counterclockwise.
    edges: HashMap<(u32, u32), u32>,
    /// One live incident triangle per vertex.
    incident: Vec<u32>,
    /// Vertices awaiting a non-collinear triple; empty once triangulated.
    pending: Vec<u32>,
    live: usize,
    finite: usize,
    /// Most recently created finite triangle: the walk starts near the last
    /// change, so spatially ordered insertion stays local.
    recent: u32,
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
            vertices: Vec::new(),
            free_vertices: Vec::new(),
            positions: HashMap::new(),
            triangles: Vec::new(),
            free_triangles: Vec::new(),
            edges: HashMap::new(),
            incident: Vec::new(),
            pending: Vec::new(),
            live: 0,
            finite: 0,
            recent: INFINITE,
            work: ExactDelaunayWork::default(),
        }
    }

    /// Build from scratch. Duplicate positions refuse with the first holder.
    pub fn build(
        points: impl IntoIterator<Item = (i64, i64, K)>,
    ) -> Result<Self, ExactDelaunayError> {
        let mut triangulation = Self::new();
        for (x, y, key) in points {
            triangulation.insert(x, y, key)?;
        }
        Ok(triangulation)
    }

    pub fn vertex_count(&self) -> usize {
        self.live
    }

    pub fn finite_triangle_count(&self) -> usize {
        self.finite
    }

    pub fn last_work(&self) -> ExactDelaunayWork {
        self.work
    }

    pub fn position(&self, vertex: VertexHandle) -> Option<(i64, i64)> {
        self.vertex(vertex).map(|v| (v.x, v.y))
    }

    pub fn key(&self, vertex: VertexHandle) -> Option<&K> {
        self.vertex(vertex).map(|v| &v.key)
    }

    pub fn vertex_at(&self, x: i64, y: i64) -> Option<VertexHandle> {
        self.positions.get(&(x, y)).copied()
    }

    /// Every finite triangle as counterclockwise vertex handles.
    pub fn triangles(&self) -> impl Iterator<Item = [VertexHandle; 3]> + '_ {
        self.triangles
            .iter()
            .flatten()
            .filter(|t| !t.contains(&INFINITE))
            .copied()
    }

    /// Finite triangles as key triples, each rotated to start at its least
    /// key, sorted: the canonical value two equal triangulations share.
    pub fn canonical_triangles(&self) -> Vec<[K; 3]> {
        let mut out: Vec<[K; 3]> = self
            .triangles()
            .map(|t| {
                let keys = t.map(|v| self.vertex(v).expect("live vertex").key.clone());
                let first = (0..3).min_by(|a, b| keys[*a].cmp(&keys[*b])).unwrap();
                [
                    keys[first].clone(),
                    keys[(first + 1) % 3].clone(),
                    keys[(first + 2) % 3].clone(),
                ]
            })
            .collect();
        out.sort();
        out
    }

    /// Vertices adjacent to `vertex` by a finite edge, in counterclockwise
    /// order around it (unspecified start).
    pub fn neighbors(&self, vertex: VertexHandle) -> Vec<VertexHandle> {
        if self.vertex(vertex).is_none() || !self.pending.is_empty() || self.finite == 0 {
            return Vec::new();
        }
        self.star(vertex)
            .into_iter()
            .map(|t| {
                let tri = self.triangle(t);
                let at = tri.iter().position(|v| *v == vertex).unwrap();
                tri[(at + 1) % 3]
            })
            .filter(|v| *v != INFINITE)
            .collect()
    }

    /// Admit one vertex. The insertion replaces only its conflict cavity.
    pub fn insert(&mut self, x: i64, y: i64, key: K) -> Result<VertexHandle, ExactDelaunayError> {
        if x.abs() > MAX_EXACT_COORDINATE || y.abs() > MAX_EXACT_COORDINATE {
            return Err(ExactDelaunayError::CoordinateOutOfRange);
        }
        if let Some(existing) = self.positions.get(&(x, y)) {
            return Err(ExactDelaunayError::DuplicatePosition {
                existing: *existing,
            });
        }
        self.work = ExactDelaunayWork::default();
        let handle = self.allocate_vertex(Vertex { x, y, key });
        if self.finite == 0 {
            self.pending.push(handle);
            self.try_bootstrap();
            return Ok(handle);
        }
        self.insert_into_triangulation(handle);
        Ok(handle)
    }

    /// Remove one vertex. The removal replaces only that vertex's star.
    pub fn remove(&mut self, vertex: VertexHandle) -> Result<K, ExactDelaunayError> {
        let removed = self
            .vertex(vertex)
            .cloned()
            .ok_or(ExactDelaunayError::UnknownVertex)?;
        self.work = ExactDelaunayWork::default();
        if self.finite == 0 {
            self.pending.retain(|v| *v != vertex);
            self.release_vertex(vertex);
            return Ok(removed.key);
        }
        let star = self.star(vertex);
        let star_triangles: Vec<[u32; 3]> = star.iter().map(|t| self.triangle(*t)).collect();
        let mut link: Vec<u32> = Vec::new();
        for tri in &star_triangles {
            for v in tri {
                if *v != vertex && *v != INFINITE && !link.contains(v) {
                    link.push(*v);
                }
            }
        }
        let outside_star = self.finite
            - star_triangles
                .iter()
                .filter(|t| !t.contains(&INFINITE))
                .count();
        // Retriangulate the star from the Delaunay triangulation of its link.
        let mut scratch: ExactDelaunay<K> = ExactDelaunay::new();
        let mut scratch_of = HashMap::new();
        for v in &link {
            let p = self.vertex(*v).expect("live link vertex");
            let handle = scratch
                .insert(p.x, p.y, p.key.clone())
                .expect("link vertices are distinct admitted positions");
            scratch_of.insert(handle, *v);
        }
        if outside_star == 0 && scratch.finite == 0 {
            // Every remaining vertex is collinear: no planar triangle remains.
            self.release_vertex(vertex);
            self.reset_to_pending();
            self.work.rebuilt = true;
            return Ok(removed.key);
        }
        let star_finite: Vec<[u32; 3]> = star_triangles
            .iter()
            .filter(|t| !t.contains(&INFINITE))
            .copied()
            .collect();
        let kept: Vec<[u32; 3]> = scratch
            .triangles()
            .map(|t| t.map(|v| scratch_of[&v]))
            .filter(|t| {
                let (cx, cy) = (
                    t.iter().map(|v| self.coord(*v).0).sum::<i64>(),
                    t.iter().map(|v| self.coord(*v).1).sum::<i64>(),
                );
                star_finite.iter().any(|s| self.contains_scaled(s, cx, cy))
            })
            .collect();
        let link_edges: Vec<(u32, u32)> = star_triangles
            .iter()
            .flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])])
            .filter(|(a, b)| *a != vertex && *b != vertex)
            .collect();
        for t in &star {
            self.remove_triangle(*t);
        }
        self.release_vertex(vertex);
        for t in &kept {
            self.add_triangle(*t);
        }
        // Edges left without a counterclockwise owner are new hull edges.
        let mut open = Vec::new();
        for t in &kept {
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                if !self.edges.contains_key(&(b, a)) {
                    open.push((b, a));
                }
            }
        }
        for (a, b) in link_edges {
            if a != INFINITE && b != INFINITE && !self.edges.contains_key(&(a, b)) {
                open.push((a, b));
            }
        }
        for (a, b) in open {
            if !self.edges.contains_key(&(a, b)) {
                self.add_triangle([a, b, INFINITE]);
            }
        }
        self.work.removed_triangles = star.len();
        Ok(removed.key)
    }

    /// Exhaustive invariant audit for tests and receipts, O(vertices x
    /// triangles): every finite triangle is counterclockwise and empty under
    /// the perturbed in-circle predicate, every directed edge has its twin,
    /// and the finite triangle count equals `2n - h - 2` for `h` hull
    /// vertices. Never call it on an interactive path.
    pub fn verify(&self) -> Result<(), String> {
        if self.finite == 0 {
            return if self.triangles.iter().flatten().count() == 0 {
                Ok(())
            } else {
                Err("ghost triangles without a finite triangle".into())
            };
        }
        if !self.pending.is_empty() {
            return Err("pending vertices beside a triangulation".into());
        }
        let live: Vec<u32> = (0..self.vertices.len() as u32)
            .filter(|v| self.vertex(*v).is_some())
            .collect();
        let mut hull = std::collections::BTreeSet::new();
        for tri in self.triangles.iter().flatten() {
            for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                if !self.edges.contains_key(&(b, a)) {
                    return Err(format!("edge {a}->{b} has no twin"));
                }
            }
            if let Some(at) = tri.iter().position(|v| *v == INFINITE) {
                hull.insert(tri[(at + 1) % 3]);
                hull.insert(tri[(at + 2) % 3]);
                continue;
            }
            if orient(self.coord(tri[0]), self.coord(tri[1]), self.coord(tri[2])) <= 0 {
                return Err(format!("triangle {tri:?} is not counterclockwise"));
            }
            for v in &live {
                if !tri.contains(v) && self.in_circle(*tri, *v) > 0 {
                    return Err(format!("vertex {v} lies inside triangle {tri:?}"));
                }
            }
        }
        let expected = 2 * live.len() - hull.len() - 2;
        if self.finite != expected {
            return Err(format!(
                "{} finite triangles for {} vertices and {} hull vertices; expected {expected}",
                self.finite,
                live.len(),
                hull.len()
            ));
        }
        Ok(())
    }

    // --- internals ---------------------------------------------------------

    fn vertex(&self, v: u32) -> Option<&Vertex<K>> {
        self.vertices.get(v as usize).and_then(Option::as_ref)
    }

    fn coord(&self, v: u32) -> (i64, i64) {
        let p = self.vertex(v).expect("live vertex");
        (p.x, p.y)
    }

    fn triangle(&self, t: u32) -> [u32; 3] {
        self.triangles[t as usize].expect("live triangle")
    }

    fn allocate_vertex(&mut self, vertex: Vertex<K>) -> u32 {
        let position = (vertex.x, vertex.y);
        let handle = match self.free_vertices.pop() {
            Some(slot) => {
                self.vertices[slot as usize] = Some(vertex);
                slot
            }
            None => {
                self.vertices.push(Some(vertex));
                self.incident.push(INFINITE);
                (self.vertices.len() - 1) as u32
            }
        };
        self.positions.insert(position, handle);
        self.live += 1;
        handle
    }

    fn release_vertex(&mut self, v: u32) {
        if let Some(vertex) = self.vertices[v as usize].take() {
            self.positions.remove(&(vertex.x, vertex.y));
            self.free_vertices.push(v);
            self.incident[v as usize] = INFINITE;
            self.live -= 1;
        }
    }

    fn add_triangle(&mut self, tri: [u32; 3]) -> u32 {
        let slot = match self.free_triangles.pop() {
            Some(slot) => {
                self.triangles[slot as usize] = Some(tri);
                slot
            }
            None => {
                self.triangles.push(Some(tri));
                (self.triangles.len() - 1) as u32
            }
        };
        for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            let previous = self.edges.insert((a, b), slot);
            debug_assert!(previous.is_none(), "directed edge owned twice");
        }
        for v in tri {
            if v != INFINITE {
                self.incident[v as usize] = slot;
            }
        }
        if !tri.contains(&INFINITE) {
            self.finite += 1;
            self.recent = slot;
        }
        self.work.created_triangles += 1;
        slot
    }

    fn remove_triangle(&mut self, t: u32) {
        let tri = self.triangles[t as usize].take().expect("live triangle");
        for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            self.edges.remove(&(a, b));
        }
        if !tri.contains(&INFINITE) {
            self.finite -= 1;
        }
        self.free_triangles.push(t);
    }

    fn reset_to_pending(&mut self) {
        self.triangles.clear();
        self.free_triangles.clear();
        self.edges.clear();
        self.finite = 0;
        self.recent = INFINITE;
        self.pending = (0..self.vertices.len() as u32)
            .filter(|v| self.vertex(*v).is_some())
            .collect();
        for slot in self.incident.iter_mut() {
            *slot = INFINITE;
        }
    }

    /// Start triangulating once a non-collinear triple exists, then insert
    /// every other pending vertex. The result does not depend on which
    /// triple starts it.
    fn try_bootstrap(&mut self) {
        if self.pending.len() < 3 {
            return;
        }
        let (a, b) = (self.pending[0], self.pending[1]);
        let Some(c) = self.pending[2..]
            .iter()
            .copied()
            .find(|c| orient(self.coord(a), self.coord(b), self.coord(*c)) != 0)
        else {
            return;
        };
        let pending = std::mem::take(&mut self.pending);
        let (a, b, c) = if orient(self.coord(a), self.coord(b), self.coord(c)) > 0 {
            (a, b, c)
        } else {
            (a, c, b)
        };
        self.add_triangle([a, b, c]);
        self.add_triangle([b, a, INFINITE]);
        self.add_triangle([c, b, INFINITE]);
        self.add_triangle([a, c, INFINITE]);
        for v in pending {
            if v != a && v != b && v != c {
                self.insert_into_triangulation(v);
            }
        }
        self.work.rebuilt = true;
    }

    fn insert_into_triangulation(&mut self, p: u32) {
        let seed = self.locate_conflict(p);
        let mut cavity = vec![seed];
        let mut in_cavity = std::collections::HashSet::from([seed]);
        let mut cursor = 0;
        while cursor < cavity.len() {
            let tri = self.triangle(cavity[cursor]);
            cursor += 1;
            for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                let Some(&neighbor) = self.edges.get(&(b, a)) else {
                    continue;
                };
                if !in_cavity.contains(&neighbor) && self.conflicts(neighbor, p) {
                    in_cavity.insert(neighbor);
                    cavity.push(neighbor);
                }
            }
        }
        let mut boundary = Vec::new();
        for t in &cavity {
            let tri = self.triangle(*t);
            for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                let outside = self
                    .edges
                    .get(&(b, a))
                    .is_none_or(|neighbor| !in_cavity.contains(neighbor));
                if outside {
                    boundary.push((a, b));
                }
            }
        }
        for t in &cavity {
            self.remove_triangle(*t);
        }
        for (a, b) in boundary {
            self.add_triangle(rotate_infinite([a, b, p]));
        }
        self.work.removed_triangles = cavity.len();
    }

    /// A triangle in conflict with `p`, found by a visibility walk.
    fn locate_conflict(&mut self, p: u32) -> u32 {
        let target = self.coord(p);
        let recent_live = self
            .triangles
            .get(self.recent as usize)
            .copied()
            .flatten()
            .is_some_and(|t| !t.contains(&INFINITE));
        let mut current = if recent_live {
            self.recent
        } else {
            self.triangles
                .iter()
                .position(|t| t.is_some_and(|t| !t.contains(&INFINITE)))
                .expect("a triangulated set has a finite triangle") as u32
        };
        let limit = 4 * self.triangles.len() + 8;
        for _ in 0..limit {
            self.work.walked_triangles += 1;
            let tri = self.triangle(current);
            if tri.contains(&INFINITE) {
                if self.conflicts(current, p) {
                    return current;
                }
                break;
            }
            let mut moved = false;
            for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
                if orient(self.coord(a), self.coord(b), target) < 0 {
                    current = self.edges[&(b, a)];
                    moved = true;
                    break;
                }
            }
            if !moved {
                return current;
            }
        }
        // A visibility walk terminates on Delaunay triangulations; keep an
        // exhaustive fallback for robustness rather than a wrong cavity.
        (0..self.triangles.len() as u32)
            .find(|t| self.triangles[*t as usize].is_some() && self.conflicts(*t, p))
            .expect("some triangle conflicts with a new distinct point")
    }

    fn conflicts(&self, t: u32, p: u32) -> bool {
        let tri = self.triangle(t);
        let q = self.coord(p);
        if let Some(at) = tri.iter().position(|v| *v == INFINITE) {
            // Ghost triangle [a, b, inf]: its hull edge is b -> a, the
            // outside lies to the left of a -> b.
            let a = self.coord(tri[(at + 1) % 3]);
            let b = self.coord(tri[(at + 2) % 3]);
            return match orient(a, b, q) {
                o if o > 0 => true,
                0 => strictly_between(a, b, q),
                _ => false,
            };
        }
        self.in_circle(tri, p) > 0
    }

    /// Sign of the perturbed in-circle test of `d` against the
    /// counterclockwise triangle; never zero for distinct points.
    fn in_circle(&self, tri: [u32; 3], d: u32) -> i32 {
        let points = [tri[0], tri[1], tri[2], d];
        let c = points.map(|v| self.coord(v));
        let exact = in_circle_exact(c[0], c[1], c[2], c[3]);
        if exact != 0 {
            return exact.signum() as i32;
        }
        // Lifted perturbation eps^rank, highest key priority first. The
        // cofactor of row r in the 4x4 lifted determinant is
        // (-1)^(r) * orient(other three rows in order).
        let mut order = [0usize, 1, 2, 3];
        order.sort_by(|i, j| {
            self.vertex(points[*i])
                .unwrap()
                .key
                .cmp(&self.vertex(points[*j]).unwrap().key)
        });
        for row in order {
            let [p, q, r] = match row {
                0 => [c[1], c[2], c[3]],
                1 => [c[0], c[2], c[3]],
                2 => [c[0], c[1], c[3]],
                _ => [c[0], c[1], c[2]],
            };
            let minor = orient(p, q, r);
            if minor != 0 {
                let sign = if row % 2 == 0 { 1 } else { -1 };
                return sign * minor.signum() as i32;
            }
        }
        unreachable!("four distinct points cannot all be mutually collinear triples")
    }

    /// Triangles incident to `vertex`, in counterclockwise rotation order.
    fn star(&self, vertex: u32) -> Vec<u32> {
        let start = self.incident[vertex as usize];
        let mut out = vec![start];
        let mut current = start;
        loop {
            let tri = self.triangle(current);
            let at = tri.iter().position(|v| *v == vertex).expect("incident");
            let next_vertex = tri[(at + 2) % 3];
            current = self.edges[&(vertex, next_vertex)];
            if current == start {
                return out;
            }
            out.push(current);
        }
    }

    /// Exact closed containment of the point `(3x, 3y)` scaled by three in a
    /// finite counterclockwise triangle (a centroid needs no division).
    fn contains_scaled(&self, tri: &[u32; 3], x3: i64, y3: i64) -> bool {
        let scale = |v: u32| {
            let (x, y) = self.coord(v);
            (3 * x, 3 * y)
        };
        let (a, b, c) = (scale(tri[0]), scale(tri[1]), scale(tri[2]));
        let q = (x3, y3);
        orient(a, b, q) >= 0 && orient(b, c, q) >= 0 && orient(c, a, q) >= 0
    }
}

fn rotate_infinite(tri: [u32; 3]) -> [u32; 3] {
    match tri.iter().position(|v| *v == INFINITE) {
        Some(0) => [tri[1], tri[2], tri[0]],
        Some(1) => [tri[2], tri[0], tri[1]],
        _ => tri,
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

fn strictly_between(a: (i64, i64), b: (i64, i64), q: (i64, i64)) -> bool {
    let dot = (q.0 as i128 - a.0 as i128) * (b.0 as i128 - a.0 as i128)
        + (q.1 as i128 - a.1 as i128) * (b.1 as i128 - a.1 as i128);
    let length = (b.0 as i128 - a.0 as i128).pow(2) + (b.1 as i128 - a.1 as i128).pow(2);
    dot > 0 && dot < length
}
