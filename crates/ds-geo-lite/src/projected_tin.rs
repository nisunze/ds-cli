//! Bounded, piecewise-planar terrain interpolation in projected metres.
//!
//! Source XYZ observations are retained verbatim. This API never treats metres
//! as WGS84 degrees, extrapolates outside coverage, or ignores breaklines.
//! Contract: `docs/projected-terrain-tin.md` in this crate.

use std::collections::BTreeMap;
use std::fmt;

use crate::operations::delaunay::triangulate_projected_xy;
use crate::operations::tin_sample::barycentric_weights_xy;

pub const MAX_PROJECTED_TIN_POINTS: usize = 6_000;
pub const MAX_PROJECTED_TIN_QUERIES: usize = 20_000;
/// Additional admission bound for dense queries in one spatial-index cell.
pub const MAX_PROJECTED_TIN_QUERY_TRIANGLE_CHECKS: usize = 10_000_000;
/// This is a local surface, not a global projected-CRS mesh.
pub const MAX_PROJECTED_TIN_EXTENT_M: f64 = 1_000_000.0;
const POINT_TOLERANCE_M: f64 = 1.0e-6;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedTerrainPoint {
    pub x_m: f64,
    pub y_m: f64,
    pub z_m: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProjectedTerrainBreakline {
    pub vertices: Vec<ProjectedTerrainPoint>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProjectedTerrainTinOptions {
    /// A triangle is usable only when all three plan edges are at most this
    /// long. Required explicitly; there is no default that bridges a gap.
    pub max_triangle_edge_m: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProjectedTerrainTinError {
    TooManyPoints {
        count: usize,
        limit: usize,
    },
    TooFewUniquePoints {
        count: usize,
    },
    NonFinitePoint {
        index: usize,
    },
    ConflictingDuplicate {
        first_index: usize,
        second_index: usize,
    },
    InvalidMaximumEdge,
    ExtentTooLarge,
    UnsupportedBreaklines {
        count: usize,
    },
    DegenerateSurface,
    TooManyQueries {
        count: usize,
        limit: usize,
    },
    NonFiniteQuery {
        index: usize,
    },
    QueryWorkLimit {
        triangle_checks: usize,
        limit: usize,
    },
    NonFiniteInterpolation,
}

impl fmt::Display for ProjectedTerrainTinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyPoints { count, limit } => {
                write!(
                    f,
                    "projected TIN has {count} points; local surface limit is {limit}"
                )
            }
            Self::TooFewUniquePoints { count } => {
                write!(
                    f,
                    "projected TIN needs at least three unique XY points; found {count}"
                )
            }
            Self::NonFinitePoint { index } => write!(f, "terrain point {index} has non-finite XYZ"),
            Self::ConflictingDuplicate {
                first_index,
                second_index,
            } => write!(
                f,
                "terrain points {first_index} and {second_index} share XY within one micrometre but disagree in Z"
            ),
            Self::InvalidMaximumEdge => write!(
                f,
                "maximum triangle edge must be finite and greater than one micrometre"
            ),
            Self::ExtentTooLarge => write!(
                f,
                "projected TIN extent must be finite and at most {MAX_PROJECTED_TIN_EXTENT_M} metres per axis"
            ),
            Self::UnsupportedBreaklines { count } => write!(
                f,
                "projected TIN has {count} authored breaklines; constrained triangulation is not supported"
            ),
            Self::DegenerateSurface => {
                write!(f, "terrain points cannot form a two-dimensional surface")
            }
            Self::TooManyQueries { count, limit } => {
                write!(
                    f,
                    "projected TIN query batch has {count} points; limit is {limit}"
                )
            }
            Self::NonFiniteQuery { index } => write!(f, "surface query {index} has non-finite XY"),
            Self::QueryWorkLimit {
                triangle_checks,
                limit,
            } => write!(
                f,
                "projected TIN query batch needs up to {triangle_checks} triangle checks; limit is {limit}; split the batch"
            ),
            Self::NonFiniteInterpolation => {
                write!(f, "terrain interpolation produced a non-finite elevation")
            }
        }
    }
}

impl std::error::Error for ProjectedTerrainTinError {}

/// One immutable local surface. Triangle indices refer to `points()` in its
/// original input order, including unchanged duplicate observations.
#[derive(Debug, Clone)]
pub struct ProjectedTerrainTin {
    points: Vec<ProjectedTerrainPoint>,
    xy: Vec<[f64; 2]>,
    triangles: Vec<[usize; 3]>,
    cells: BTreeMap<(i64, i64), Vec<usize>>,
    origin: [f64; 2],
    bounds: [[f64; 2]; 2],
    options: ProjectedTerrainTinOptions,
    unique_point_count: usize,
    discarded_long_edge_triangle_count: usize,
}

impl ProjectedTerrainTin {
    pub fn build(
        points: &[ProjectedTerrainPoint],
        options: ProjectedTerrainTinOptions,
        breaklines: &[ProjectedTerrainBreakline],
    ) -> Result<Self, ProjectedTerrainTinError> {
        if points.len() > MAX_PROJECTED_TIN_POINTS {
            return Err(ProjectedTerrainTinError::TooManyPoints {
                count: points.len(),
                limit: MAX_PROJECTED_TIN_POINTS,
            });
        }
        if !breaklines.is_empty() {
            return Err(ProjectedTerrainTinError::UnsupportedBreaklines {
                count: breaklines.len(),
            });
        }
        if !options.max_triangle_edge_m.is_finite()
            || options.max_triangle_edge_m <= POINT_TOLERANCE_M
        {
            return Err(ProjectedTerrainTinError::InvalidMaximumEdge);
        }
        for (index, point) in points.iter().enumerate() {
            if !point.x_m.is_finite() || !point.y_m.is_finite() || !point.z_m.is_finite() {
                return Err(ProjectedTerrainTinError::NonFinitePoint { index });
            }
        }

        let mut ordered: Vec<usize> = (0..points.len()).collect();
        ordered.sort_by(|&a, &b| {
            points[a]
                .x_m
                .total_cmp(&points[b].x_m)
                .then_with(|| points[a].y_m.total_cmp(&points[b].y_m))
                .then_with(|| a.cmp(&b))
        });
        let mut unique: Vec<usize> = Vec::with_capacity(ordered.len());
        for (position, &index) in ordered.iter().enumerate() {
            let point = points[index];
            // Compare the entire tiny X window: adjacent-only duplicate
            // checks miss (x,y), (x,y+10), (x+epsilon,y).
            // Check every original observation for conflicting heights, not
            // only retained vertices: near-coincidence is not transitive.
            let conflicting = ordered[..position]
                .iter()
                .rev()
                .copied()
                .take_while(|&other| point.x_m - points[other].x_m <= POINT_TOLERANCE_M)
                .find(|&other| {
                    point.z_m != points[other].z_m
                        && (point.x_m - points[other].x_m).hypot(point.y_m - points[other].y_m)
                            <= POINT_TOLERANCE_M
                });
            if let Some(other) = conflicting {
                return Err(ProjectedTerrainTinError::ConflictingDuplicate {
                    first_index: other,
                    second_index: index,
                });
            }
            let duplicate = unique
                .iter()
                .rev()
                .copied()
                .take_while(|&other| point.x_m - points[other].x_m <= POINT_TOLERANCE_M)
                .find(|&other| {
                    (point.x_m - points[other].x_m).hypot(point.y_m - points[other].y_m)
                        <= POINT_TOLERANCE_M
                });
            if duplicate.is_none() {
                unique.push(index);
            }
        }
        if unique.len() < 3 {
            return Err(ProjectedTerrainTinError::TooFewUniquePoints {
                count: unique.len(),
            });
        }

        let min_x = points.iter().map(|p| p.x_m).fold(f64::INFINITY, f64::min);
        let min_y = points.iter().map(|p| p.y_m).fold(f64::INFINITY, f64::min);
        let max_x = points
            .iter()
            .map(|p| p.x_m)
            .fold(f64::NEG_INFINITY, f64::max);
        let max_y = points
            .iter()
            .map(|p| p.y_m)
            .fold(f64::NEG_INFINITY, f64::max);
        let extent_x = max_x - min_x;
        let extent_y = max_y - min_y;
        if !extent_x.is_finite()
            || !extent_y.is_finite()
            || extent_x > MAX_PROJECTED_TIN_EXTENT_M
            || extent_y > MAX_PROJECTED_TIN_EXTENT_M
        {
            return Err(ProjectedTerrainTinError::ExtentTooLarge);
        }
        // Translation alone improves predicates for UTM-sized coordinates;
        // no angular conversion, scale adjustment or source edit occurs.
        let origin = [min_x + extent_x * 0.5, min_y + extent_y * 0.5];
        let xy: Vec<_> = points
            .iter()
            .map(|point| [point.x_m - origin[0], point.y_m - origin[1]])
            .collect();
        let unique_xy: Vec<_> = unique.iter().map(|&index| xy[index]).collect();
        let triangle_indices = triangulate_projected_xy(&unique_xy);
        if triangle_indices.is_empty() {
            return Err(ProjectedTerrainTinError::DegenerateSurface);
        }
        let mut triangles = Vec::with_capacity(triangle_indices.len() / 3);
        let mut discarded_long_edge_triangle_count = 0;
        for indices in triangle_indices.chunks_exact(3) {
            let triangle = [
                unique[indices[0] as usize],
                unique[indices[1] as usize],
                unique[indices[2] as usize],
            ];
            if [(0, 1), (1, 2), (2, 0)].iter().any(|&(a, b)| {
                let a = xy[triangle[a]];
                let b = xy[triangle[b]];
                (a[0] - b[0]).hypot(a[1] - b[1]) > options.max_triangle_edge_m
            }) {
                discarded_long_edge_triangle_count += 1;
            } else {
                triangles.push(triangle);
            }
        }
        let mut cells: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
        for (triangle_index, triangle) in triangles.iter().enumerate() {
            let min = [
                triangle
                    .iter()
                    .map(|&p| xy[p][0])
                    .fold(f64::INFINITY, f64::min),
                triangle
                    .iter()
                    .map(|&p| xy[p][1])
                    .fold(f64::INFINITY, f64::min),
            ];
            let max = [
                triangle
                    .iter()
                    .map(|&p| xy[p][0])
                    .fold(f64::NEG_INFINITY, f64::max),
                triangle
                    .iter()
                    .map(|&p| xy[p][1])
                    .fold(f64::NEG_INFINITY, f64::max),
            ];
            // Every admitted triangle fits in at most two cells per axis
            // (three allowing rounding exactly on a cell boundary).
            let low = cell(min, options.max_triangle_edge_m);
            let high = cell(max, options.max_triangle_edge_m);
            debug_assert!(high.0 - low.0 <= 2 && high.1 - low.1 <= 2);
            for x in low.0..=high.0 {
                for y in low.1..=high.1 {
                    cells.entry((x, y)).or_default().push(triangle_index);
                }
            }
        }
        Ok(Self {
            points: points.to_vec(),
            xy,
            triangles,
            cells,
            origin,
            bounds: [[min_x, min_y], [max_x, max_y]],
            options,
            unique_point_count: unique.len(),
            discarded_long_edge_triangle_count,
        })
    }

    pub fn points(&self) -> &[ProjectedTerrainPoint] {
        &self.points
    }
    pub fn triangles(&self) -> &[[usize; 3]] {
        &self.triangles
    }
    pub fn options(&self) -> ProjectedTerrainTinOptions {
        self.options
    }
    pub fn unique_point_count(&self) -> usize {
        self.unique_point_count
    }
    pub fn discarded_long_edge_triangle_count(&self) -> usize {
        self.discarded_long_edge_triangle_count
    }

    pub fn sample(&self, x_m: f64, y_m: f64) -> Result<Option<f64>, ProjectedTerrainTinError> {
        if !x_m.is_finite() || !y_m.is_finite() {
            return Err(ProjectedTerrainTinError::NonFiniteQuery { index: 0 });
        }
        self.sample_admitted([x_m, y_m])
    }

    pub fn sample_many(
        &self,
        queries_xy_m: &[[f64; 2]],
    ) -> Result<Vec<Option<f64>>, ProjectedTerrainTinError> {
        if queries_xy_m.len() > MAX_PROJECTED_TIN_QUERIES {
            return Err(ProjectedTerrainTinError::TooManyQueries {
                count: queries_xy_m.len(),
                limit: MAX_PROJECTED_TIN_QUERIES,
            });
        }
        let mut triangle_checks = 0usize;
        for (index, &query) in queries_xy_m.iter().enumerate() {
            if !query[0].is_finite() || !query[1].is_finite() {
                return Err(ProjectedTerrainTinError::NonFiniteQuery { index });
            }
            triangle_checks += self.candidates(query).map_or(0, <[usize]>::len);
            if triangle_checks > MAX_PROJECTED_TIN_QUERY_TRIANGLE_CHECKS {
                return Err(ProjectedTerrainTinError::QueryWorkLimit {
                    triangle_checks,
                    limit: MAX_PROJECTED_TIN_QUERY_TRIANGLE_CHECKS,
                });
            }
        }
        queries_xy_m
            .iter()
            .map(|&query| self.sample_admitted(query))
            .collect()
    }

    fn candidates(&self, query: [f64; 2]) -> Option<&[usize]> {
        if query[0] < self.bounds[0][0]
            || query[0] > self.bounds[1][0]
            || query[1] < self.bounds[0][1]
            || query[1] > self.bounds[1][1]
        {
            return None;
        }
        let xy = [query[0] - self.origin[0], query[1] - self.origin[1]];
        self.cells
            .get(&cell(xy, self.options.max_triangle_edge_m))
            .map(Vec::as_slice)
    }

    fn sample_admitted(&self, query: [f64; 2]) -> Result<Option<f64>, ProjectedTerrainTinError> {
        let Some(candidates) = self.candidates(query) else {
            return Ok(None);
        };
        let xy = [query[0] - self.origin[0], query[1] - self.origin[1]];
        for &triangle_index in candidates {
            let [a, b, c] = self.triangles[triangle_index];
            // An original vertex of a supported triangle is exact terrain.
            // Avoid cancellation in barycentric arithmetic at that vertex.
            if let Some(index) = [a, b, c].into_iter().find(|&index| {
                query[0] == self.points[index].x_m && query[1] == self.points[index].y_m
            }) {
                return Ok(Some(self.points[index].z_m));
            }
            let Some((wa, wb, wc)) = barycentric_weights_xy(self.xy[a], self.xy[b], self.xy[c], xy)
            else {
                continue;
            };
            // Legacy geographic sampling admits tiny negative weights. This
            // metric contract accepts only convex interpolation: no even-tiny
            // extrapolation beyond the hull or into a filtered triangle gap.
            if wa < 0.0 || wb < 0.0 || wc < 0.0 {
                continue;
            }
            let elevation =
                wa * self.points[a].z_m + wb * self.points[b].z_m + wc * self.points[c].z_m;
            if !elevation.is_finite() {
                return Err(ProjectedTerrainTinError::NonFiniteInterpolation);
            }
            return Ok(Some(elevation));
        }
        Ok(None)
    }
}

fn cell(xy: [f64; 2], size_m: f64) -> (i64, i64) {
    // Admitted local extent <= 1e6 m and size > 1e-6 m keep these quotients
    // within +/-1e12, safely inside i64 without saturating casts.
    (
        (xy[0] / size_m).floor() as i64,
        (xy[1] / size_m).floor() as i64,
    )
}
