//! Pure, bounded physical-sheet fitting. No acquisition, projection or UI policy.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub type Rect = [f64; 4];
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct NetworkFit {
    pub source_layers: Vec<String>,
    pub clearance_mm: f64,
}
impl NetworkFit {
    pub fn validate(&self) -> Result<(), String> {
        if self.source_layers.is_empty()
            || self.source_layers.len() > 16
            || self.source_layers.iter().any(|id| !super::identifier(id))
            || self.source_layers.iter().collect::<BTreeSet<_>>().len() != self.source_layers.len()
            || !super::bounded(self.clearance_mm, 1., 30.)
        {
            return Err(
                "network fit needs 1..16 unique engineering source layers and 1..30 mm clearance"
                    .into(),
            );
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ScaleLimits {
    pub min_denominator: f64,
    pub max_denominator: f64,
}
impl ScaleLimits {
    pub fn validate(&self) -> Result<(), String> {
        if !super::bounded(self.min_denominator, 10., 100_000_000.)
            || !super::bounded(self.max_denominator, self.min_denominator, 100_000_000.)
        {
            return Err("print scale limits need ordered denominators within 10..100000000".into());
        }
        Ok(())
    }
}

/// Automatic rounding may only zoom out. Exact cameras are never rounded or clamped.
pub fn denominator(
    required: f64,
    rounding: Option<f64>,
    limits: Option<&ScaleLimits>,
    camera: bool,
) -> Result<f64, String> {
    if !required.is_finite() || required <= 0. {
        return Err("invalid fitted print scale".into());
    }
    if let Some(limits) = limits {
        limits.validate()?;
        if camera && !(limits.min_denominator..=limits.max_denominator).contains(&required) {
            return Err("explicit camera is outside the project print scale limits".into());
        }
    }
    let mut result = required;
    if !camera {
        if let Some(limits) = limits {
            result = result.max(limits.min_denominator);
        }
        if let Some(step) = rounding {
            if !super::bounded(step, 1., 1_000_000.) {
                return Err("invalid print scale rounding increment".into());
            }
            result = (result / step).ceil() * step;
        }
    }
    if limits.is_some_and(|limits| result > limits.max_denominator + 1e-7) {
        return Err("complete network cannot fit within the project print scale limits; increase max_denominator or use larger paper".into());
    }
    Ok(result)
}

pub struct Path {
    pub vertices: Vec<[f64; 2]>,
    pub filled: bool,
}
const GRID: usize = 16;
fn bounds(points: impl Iterator<Item = [f64; 2]>) -> Option<Rect> {
    let mut r = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in points {
        r[0] = r[0].min(p[0]);
        r[1] = r[1].min(p[1]);
        r[2] = r[2].max(p[0]);
        r[3] = r[3].max(p[1]);
    }
    r.iter()
        .all(|v| v.is_finite())
        .then_some([r[0], r[1], r[2] - r[0], r[3] - r[1]])
}
fn segment_hits(a: [f64; 2], b: [f64; 2], r: Rect) -> bool {
    let mut lo: f64 = 0.;
    let mut hi: f64 = 1.;
    for axis in 0..2 {
        let delta = b[axis] - a[axis];
        if delta.abs() < 1e-12 {
            if a[axis] < r[axis] - 1e-9 || a[axis] > r[axis] + r[axis + 2] + 1e-9 {
                return false;
            }
        } else {
            let t0 = (r[axis] - a[axis]) / delta;
            let t1 = (r[axis] + r[axis + 2] - a[axis]) / delta;
            lo = lo.max(t0.min(t1));
            hi = hi.min(t0.max(t1));
            if lo > hi + 1e-9 {
                return false;
            }
        }
    }
    true
}

/// Conservative grid: segments, not just vertices; filled polygons reserve
/// their envelope, so a table cannot occupy an unobserved polygon interior.
pub fn footprint(paths: &[Path]) -> Result<Vec<Rect>, String> {
    let count: usize = paths.iter().map(|p| p.vertices.len()).sum();
    if count == 0
        || count > 1_000_000
        || paths
            .iter()
            .flat_map(|p| &p.vertices)
            .flatten()
            .any(|v| !v.is_finite())
    {
        return Err(
            "network footprint needs finite held geometry within one million vertices".into(),
        );
    }
    let mut extent = bounds(paths.iter().flat_map(|p| p.vertices.iter().copied())).unwrap();
    // Point-only and straight-line designs still have a nonzero conservative envelope.
    extent[2] = extent[2].max(1e-6);
    extent[3] = extent[3].max(1e-6);
    let dx = extent[2] / GRID as f64;
    let dy = extent[3] / GRID as f64;
    let mut cells = [[false; GRID]; GRID];
    let col =
        |x: f64| (((x - extent[0]) / dx).floor() as isize).clamp(0, GRID as isize - 1) as usize;
    let row =
        |y: f64| (((y - extent[1]) / dy).floor() as isize).clamp(0, GRID as isize - 1) as usize;
    let mut mark = |a: [f64; 2], b: [f64; 2], filled: bool| {
        for (y, cells) in cells
            .iter_mut()
            .enumerate()
            .take(row(a[1].max(b[1])) + 1)
            .skip(row(a[1].min(b[1])))
        {
            for (x, cell) in cells
                .iter_mut()
                .enumerate()
                .take(col(a[0].max(b[0])) + 1)
                .skip(col(a[0].min(b[0])))
            {
                if *cell {
                    continue;
                }
                let r = [extent[0] + x as f64 * dx, extent[1] + y as f64 * dy, dx, dy];
                *cell = filled || segment_hits(a, b, r);
            }
        }
    };
    for path in paths {
        if path.filled {
            if let Some(b) = bounds(path.vertices.iter().copied()) {
                mark([b[0], b[1]], [b[0] + b[2], b[1] + b[3]], true);
            }
        } else if path.vertices.len() == 1 {
            mark(path.vertices[0], path.vertices[0], false);
        } else {
            for segment in path.vertices.windows(2) {
                mark(segment[0], segment[1], false);
            }
        }
    }
    let mut rectangles: Vec<Rect> = Vec::new();
    for (y, row) in cells.iter().enumerate() {
        let mut x = 0;
        while x < GRID {
            if !row[x] {
                x += 1;
                continue;
            }
            let start = x;
            while x < GRID && row[x] {
                x += 1;
            }
            let r = [
                extent[0] + start as f64 * dx,
                extent[1] + y as f64 * dy,
                (x - start) as f64 * dx,
                dy,
            ];
            if let Some(prior) = rectangles.iter_mut().find(|p| {
                (p[0] - r[0]).abs() < 1e-9
                    && (p[2] - r[2]).abs() < 1e-9
                    && (p[1] + p[3] - r[1]).abs() < 1e-9
            }) {
                prior[3] += dy;
            } else {
                rectangles.push(r);
            }
        }
    }
    Ok(rectangles)
}

#[derive(Debug, Clone)]
pub struct Fit {
    /// Millimetres per footprint coordinate unit.
    pub scale: f64,
    /// Paper point under the footprint's coordinate origin.
    pub origin_mm: [f64; 2],
    pub parts_mm: Vec<Rect>,
}
pub struct FitOptions {
    pub clearance_mm: f64,
    pub max_scale: f64,
    /// A previously feasible rectangular fit, independently validated below.
    pub baseline: Option<Fit>,
}
fn intersects(a: Rect, b: Rect) -> bool {
    a[0] < b[0] + b[2] - 1e-7
        && a[0] + a[2] > b[0] + 1e-7
        && a[1] < b[1] + b[3] - 1e-7
        && a[1] + a[3] > b[1] + 1e-7
}
pub fn paper_parts(parts: &[Rect], scale: f64, origin: [f64; 2], clearance: f64) -> Vec<Rect> {
    parts
        .iter()
        .map(|p| {
            [
                origin[0] + p[0] * scale - clearance,
                origin[1] + p[1] * scale - clearance,
                p[2] * scale + 2. * clearance,
                p[3] * scale + 2. * clearance,
            ]
        })
        .collect()
}

/// Descending scale trials and bounded local refinement avoid assuming that
/// disconnected/concave occupancy is monotone under enlargement. All candidate
/// origins keep the complete footprint inside its parent, including clearance.
pub fn fit(
    parts: &[Rect],
    frame: Rect,
    obstacles: &[Rect],
    clearance: f64,
    max_scale: f64,
) -> Result<Fit, String> {
    fit_with_scale_policy(
        parts,
        frame,
        obstacles,
        FitOptions {
            clearance_mm: clearance,
            max_scale,
            baseline: None,
        },
        Some,
    )
}

/// Resolve each candidate's physical scale before testing it against furniture.
/// Returning None excludes a scale (for example beyond the farthest allowed view).
pub fn fit_with_scale_policy(
    parts: &[Rect],
    frame: Rect,
    obstacles: &[Rect],
    options: FitOptions,
    resolve_scale: impl Fn(f64) -> Option<f64>,
) -> Result<Fit, String> {
    let clearance = options.clearance_mm;
    let max_scale = options.max_scale;
    if parts.is_empty()
        || parts.len() > GRID * GRID
        || obstacles.len() > 256
        || !super::bounded(clearance, 1., 30.)
        || !max_scale.is_finite()
        || max_scale <= 0.
        || parts
            .iter()
            .chain(std::iter::once(&frame))
            .chain(obstacles)
            .any(|r| r.iter().any(|v| !v.is_finite()) || r[2] <= 0. || r[3] <= 0.)
    {
        return Err("invalid bounded network fit geometry".into());
    }
    let b = bounds(
        parts
            .iter()
            .flat_map(|r| [[r[0], r[1]], [r[0] + r[2], r[1] + r[3]]]),
    )
    .unwrap();
    let maximum = ((frame[2] - 2. * clearance) / b[2])
        .min((frame[3] - 2. * clearance) / b[3])
        .min(max_scale);
    if maximum <= 0. {
        return Err("print border leaves no network footprint space".into());
    }
    let trial = |scale: f64| -> Option<Fit> {
        let requested_scale = scale;
        let scale = resolve_scale(scale)?;
        if !scale.is_finite()
            || scale <= 0.
            || scale > requested_scale + 1e-7
            || scale > maximum + 1e-7
        {
            return None;
        }
        let w = frame[2] - 2. * clearance - b[2] * scale;
        let h = frame[3] - 2. * clearance - b[3] * scale;
        let left = frame[0] + clearance - b[0] * scale;
        let top = frame[1] + clearance - b[1] * scale;
        // Centre first, then deterministic perimeter/grid positions.
        let mut origins = vec![[left + w / 2., top + h / 2.]];
        for y in 0..=4 {
            for x in 0..=4 {
                origins.push([left + w * x as f64 / 4., top + h * y as f64 / 4.]);
            }
        }
        // At the same resolved scale, prefer the footprint's centre nearest
        // its printable parent. Keep the proven rectangular origin available.
        if let Some(baseline) = &options.baseline {
            origins.push(baseline.origin_mm);
        }
        let target = [left + w / 2., top + h / 2.];
        origins.sort_by(|a, b| {
            let distance = |p: &[f64; 2]| (p[0] - target[0]).hypot(p[1] - target[1]);
            distance(a).total_cmp(&distance(b))
        });
        for origin in origins {
            let parts_mm = paper_parts(parts, scale, origin, clearance);
            if !parts_mm.iter().any(|p| {
                p[0] < frame[0] - 1e-7
                    || p[1] < frame[1] - 1e-7
                    || p[0] + p[2] > frame[0] + frame[2] + 1e-7
                    || p[1] + p[3] > frame[1] + frame[3] + 1e-7
                    || obstacles.iter().any(|o| intersects(*p, *o))
            }) {
                return Some(Fit {
                    scale,
                    origin_mm: origin,
                    parts_mm,
                });
            }
        }
        None
    };
    let mut best = None;
    let mut upper = maximum;
    for n in (1..=32).rev() {
        let scale = maximum * n as f64 / 32.;
        if let Some(result) = trial(scale) {
            best = Some(result);
            break;
        }
        upper = scale;
    }
    let baseline = options.baseline.clone().and_then(|mut candidate| {
        let resolved = resolve_scale(candidate.scale)?;
        if !candidate.scale.is_finite()
            || candidate.scale <= 0.
            || candidate.scale > maximum + 1e-7
            || (candidate.scale - resolved).abs() > 1e-7
            || candidate.origin_mm.iter().any(|v| !v.is_finite())
        {
            return None;
        }
        candidate.parts_mm = paper_parts(parts, candidate.scale, candidate.origin_mm, clearance);
        if candidate.parts_mm.iter().any(|p| {
            p[0] < frame[0] - 1e-7
                || p[1] < frame[1] - 1e-7
                || p[0] + p[2] > frame[0] + frame[2] + 1e-7
                || p[1] + p[3] > frame[1] + frame[3] + 1e-7
                || obstacles.iter().any(|o| intersects(*p, *o))
        }) {
            return None;
        }
        Some(candidate)
    });
    if let Some(candidate) = baseline {
        if best
            .as_ref()
            .is_none_or(|best| candidate.scale > best.scale)
        {
            best = Some(candidate);
        }
    }
    let mut best = best.ok_or("Template furniture leaves no usable stepped network footprint")?;
    upper = upper.max(best.scale);
    for _ in 0..8 {
        let scale = (best.scale + upper) / 2.;
        if let Some(result) = trial(scale) {
            if result.scale > best.scale {
                best = result;
            } else {
                upper = scale;
            }
        } else {
            upper = scale;
        }
    }
    // A larger rectangular baseline must still obey the same centring
    // preference; refinement must not leave it stranded at an earlier origin.
    if let Some(centred) = trial(best.scale) {
        best = centred;
    }
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocked_centre_prefers_nearest_feasible_origin_at_the_same_scale() {
        let result = fit(
            &[[-5., -10., 10., 20.]],
            [0., 0., 100., 100.],
            &[[43., 38., 14., 24.]],
            1.,
            1.,
        )
        .unwrap();
        assert_eq!(result.scale, 1.);
        assert_eq!(result.origin_mm, [28., 50.]);
        assert!(!result
            .parts_mm
            .iter()
            .any(|p| intersects(*p, [43., 38., 14., 24.])));
    }

    #[test]
    fn segments_and_polygon_interiors_are_reserved() {
        let paths = [Path {
            vertices: vec![[0., 0.], [100., 100.]],
            filled: false,
        }];
        let parts = footprint(&paths).unwrap();
        assert!(parts
            .iter()
            .any(|r| r[0] <= 50. && r[0] + r[2] >= 50. && r[1] <= 50. && r[1] + r[3] >= 50.));
        assert!(parts.iter().map(|r| r[2] * r[3]).sum::<f64>() < 4000.);
        let polygon = footprint(&[Path {
            vertices: vec![[0., 0.], [100., 0.], [100., 100.], [0., 100.], [0., 0.]],
            filled: true,
        }])
        .unwrap();
        assert_eq!(polygon, vec![[0., 0., 100., 100.]]);
    }
    #[test]
    fn stepped_fit_returns_unused_corner_without_collision() {
        let parts = footprint(&[Path {
            vertices: vec![[0., 0.], [100., 100.]],
            filled: false,
        }])
        .unwrap();
        let furniture = [[0., 65., 35., 35.], [65., 0., 35., 35.]];
        let result = fit(&parts, [0., 0., 100., 100.], &furniture, 2., 10.).unwrap();
        assert!(result.scale > 0.85);
        assert!(result
            .parts_mm
            .iter()
            .all(|p| !furniture.iter().any(|o| intersects(*p, *o))));
    }
    #[test]
    fn tiny_design_has_closest_scale_cap_and_cameras_remain_exact() {
        let limits = ScaleLimits {
            min_denominator: 1000.,
            max_denominator: 5000.,
        };
        assert_eq!(
            denominator(100., Some(100.), Some(&limits), false).unwrap(),
            1000.
        );
        assert!(denominator(100., None, Some(&limits), true).is_err());
        assert_eq!(
            denominator(1123., Some(100.), Some(&limits), true).unwrap(),
            1123.
        );
        assert!(denominator(4999., Some(300.), Some(&limits), false).is_err());
        assert!(denominator(6000., None, Some(&limits), false).is_err());
    }
    #[test]
    fn requests_author_the_same_policy_and_prepare_only_selected_draft_context() {
        use crate::printing::{context, default_layout, evaluate};
        use serde_json::{json, Value};
        let layout = default_layout();
        let run = |request: Value| -> Result<Value, String> {
            serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap())?)
                .map_err(|e| e.to_string())
        };
        let result = run(json!({"op":"set_fit_policy","layout":layout,"network_fit":{"source_layers":["poles","lv_lines"],"clearance_mm":3},"scale_limits":{"min_denominator":1000,"max_denominator":10000}})).unwrap();
        assert_eq!(
            result["layout"]["network_fit"]["clearance_mm"].as_f64(),
            Some(3.)
        );
        assert!(run(json!({"op":"set_fit_policy","layout":layout,"network_fit":null,"scale_limits":{"min_denominator":10000,"max_denominator":1000}})).is_err());
        let selected =
            context::toggle(layout, context::derived_layers().remove(2), true, None).unwrap();
        for online in [false, true] {
            let result =
                run(json!({"op":"preview_context_preparation","layout":selected,"online":online}))
                    .unwrap();
            assert_eq!(result["acquire_missing"], online);
            assert_eq!(result["contexts"].as_array().unwrap().len(), 1);
            assert_eq!(
                result["contexts"][0]["source"]["kind"],
                "elevation_contours"
            );
        }
    }
}
