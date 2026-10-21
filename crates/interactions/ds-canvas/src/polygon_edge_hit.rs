//! Screen-space polygon edge hit testing, mechanically extracted from ds-web.
//!
//! The host supplies projection effects. Filtering, implicit ring closure,
//! numeric edge distance and early-exit ordering remain one UI-owned algorithm.

use crate::gantt_hit_index::js_hypot;
use serde_json::Value;
use std::convert::Infallible;

/// Read-only polygon transport. Readers expose values lazily; all geometry
/// filtering, traversal, closure and hit decisions remain below in this core.
pub trait PolygonEdgeValue: Sized {
    type Error;
    fn field(&self, name: &str) -> Result<Self, Self::Error>;
    fn array_len(&self) -> Result<Option<usize>, Self::Error>;
    fn item(&self, index: usize) -> Result<Self, Self::Error>;
    fn number(&self) -> Option<f64>;
    fn text(&self) -> Option<String>;
}

#[derive(Debug)]
pub enum PolygonEdgeHitError<R, P> {
    InvalidCoordinates,
    InvalidRing,
    Read(R),
    Project(P),
}

struct NativeValue<'a>(&'a Value);
impl PolygonEdgeValue for NativeValue<'_> {
    type Error = Infallible;
    fn field(&self, name: &str) -> Result<Self, Self::Error> {
        Ok(Self(self.0.get(name).unwrap_or(&Value::Null)))
    }
    fn array_len(&self) -> Result<Option<usize>, Self::Error> {
        Ok(self.0.as_array().map(Vec::len))
    }
    fn item(&self, index: usize) -> Result<Self, Self::Error> {
        Ok(Self(self.0.get(index).unwrap_or(&Value::Null)))
    }
    fn number(&self) -> Option<f64> {
        self.0.as_f64()
    }
    fn text(&self) -> Option<String> {
        self.0.as_str().map(str::to_owned)
    }
}

pub fn polygon_edge_hit<F, E>(
    geometry: &Value,
    point: [f64; 2],
    tolerance_px: f64,
    project: F,
) -> Result<bool, PolygonEdgeHitError<Infallible, E>>
where
    F: FnMut([f64; 2]) -> Result<[f64; 2], E>,
{
    polygon_edge_hit_source(&NativeValue(geometry), point, tolerance_px, project)
}

pub fn polygon_edge_hit_source<V, F, E>(
    geometry: &V,
    point: [f64; 2],
    tolerance_px: f64,
    mut project: F,
) -> Result<bool, PolygonEdgeHitError<V::Error, E>>
where
    V: PolygonEdgeValue,
    F: FnMut([f64; 2]) -> Result<[f64; 2], E>,
{
    if !point.into_iter().all(f64::is_finite) || !tolerance_px.is_finite() || tolerance_px < 0.0 {
        return Ok(false);
    }
    // Preserve the original type reads as well as first-ring/polygon exit.
    if geometry
        .field("type")
        .map_err(PolygonEdgeHitError::Read)?
        .text()
        .as_deref()
        == Some("Polygon")
    {
        let rings = geometry
            .field("coordinates")
            .map_err(PolygonEdgeHitError::Read)?;
        return polygon_hit(&rings, point, tolerance_px, &mut project);
    }
    if geometry
        .field("type")
        .map_err(PolygonEdgeHitError::Read)?
        .text()
        .as_deref()
        != Some("MultiPolygon")
    {
        return Ok(false);
    }
    let polygons = geometry
        .field("coordinates")
        .map_err(PolygonEdgeHitError::Read)?;
    let count = polygons
        .array_len()
        .map_err(PolygonEdgeHitError::Read)?
        .ok_or(PolygonEdgeHitError::InvalidCoordinates)?;
    for index in 0..count {
        let rings = polygons.item(index).map_err(PolygonEdgeHitError::Read)?;
        if polygon_hit(&rings, point, tolerance_px, &mut project)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn polygon_hit<V, F, E>(
    rings: &V,
    point: [f64; 2],
    tolerance_px: f64,
    project: &mut F,
) -> Result<bool, PolygonEdgeHitError<V::Error, E>>
where
    V: PolygonEdgeValue,
    F: FnMut([f64; 2]) -> Result<[f64; 2], E>,
{
    let count = rings
        .array_len()
        .map_err(PolygonEdgeHitError::Read)?
        .ok_or(PolygonEdgeHitError::InvalidCoordinates)?;
    for index in 0..count {
        let ring = rings.item(index).map_err(PolygonEdgeHitError::Read)?;
        let count = ring
            .array_len()
            .map_err(PolygonEdgeHitError::Read)?
            .ok_or(PolygonEdgeHitError::InvalidRing)?;
        let mut coords = Vec::new();
        for index in 0..count {
            let coord = ring.item(index).map_err(PolygonEdgeHitError::Read)?;
            if let Some(coord) = finite_coord(&coord).map_err(PolygonEdgeHitError::Read)? {
                coords.push(coord);
            }
        }
        if coords.len() < 2 {
            continue;
        }
        for pair in coords.windows(2) {
            if edge_hit(point, pair[0], pair[1], tolerance_px, project)
                .map_err(PolygonEdgeHitError::Project)?
            {
                return Ok(true);
            }
        }
        let first = coords[0];
        let last = coords[coords.len() - 1];
        if first != last
            && edge_hit(point, last, first, tolerance_px, project)
                .map_err(PolygonEdgeHitError::Project)?
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn finite_coord<V: PolygonEdgeValue>(coord: &V) -> Result<Option<[f64; 2]>, V::Error> {
    let Some(length) = coord.array_len()? else {
        return Ok(None);
    };
    if length < 2 {
        return Ok(None);
    }
    let Some(x) = coord.item(0)?.number().filter(|x| x.is_finite()) else {
        return Ok(None);
    };
    let Some(y) = coord.item(1)?.number().filter(|y| y.is_finite()) else {
        return Ok(None);
    };
    Ok(Some([x, y]))
}

fn edge_hit<F, E>(
    point: [f64; 2],
    start: [f64; 2],
    end: [f64; 2],
    tolerance_px: f64,
    project: &mut F,
) -> Result<bool, E>
where
    F: FnMut([f64; 2]) -> Result<[f64; 2], E>,
{
    // Project both endpoints before rejecting either, exactly as the old host
    // implementation did. Do not cache or reorder callback effects.
    let start = project(start)?;
    let end = project(end)?;
    if !start.into_iter().all(f64::is_finite) || !end.into_iter().all(f64::is_finite) {
        return Ok(false);
    }
    Ok(distance_to_segment(point, start, end) <= tolerance_px)
}

fn distance_to_segment(point: [f64; 2], start: [f64; 2], end: [f64; 2]) -> f64 {
    let dx = end[0] - start[0];
    let dy = end[1] - start[1];
    if dx == 0.0 && dy == 0.0 {
        return js_hypot(point[0] - start[0], point[1] - start[1]);
    }
    let t = (((point[0] - start[0]) * dx + (point[1] - start[1]) * dy) / (dx * dx + dy * dy))
        .clamp(0.0, 1.0);
    let nearest_x = start[0] + t * dx;
    let nearest_y = start[1] + t * dy;
    js_hypot(point[0] - nearest_x, point[1] - nearest_y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hit(geometry: Value, point: [f64; 2], tolerance: f64) -> bool {
        polygon_edge_hit(&geometry, point, tolerance, Ok::<_, ()>).unwrap()
    }
    fn square() -> Value {
        json!({"type":"Polygon","coordinates":[[[0,0],[100,0],[100,100],[0,100],[0,0]]]})
    }

    #[test]
    fn existing_edge_fill_multipolygon_and_implicit_closure_behavior() {
        assert!(hit(square(), [52.0, 4.0], 8.0));
        assert!(!hit(square(), [50.0, 50.0], 8.0));
        assert!(hit(
            json!({"type":"MultiPolygon","coordinates":[
                [[[0,0],[20,0],[20,20],[0,20],[0,0]]],
                [[[100,100],[140,100],[140,140],[100,140],[100,100]]]
            ]}),
            [120.0, 104.0],
            8.0
        ));
        assert!(hit(
            json!({"type":"Polygon","coordinates":[[[0,0],[100,0],[100,100],[0,100]]]}),
            [3.0, 50.0],
            8.0
        ));
        assert!(!hit(
            json!({"type":"LineString","coordinates":[[0,0],[100,0]]}),
            [50.0, 50.0],
            8.0
        ));
    }

    #[test]
    fn holes_invalid_coords_and_zero_tolerance_keep_existing_behavior() {
        assert!(hit(
            json!({"type":"Polygon","coordinates":[
                [[0,0],[100,0],[100,100],[0,100]], [[40,40],[60,40],[60,60],[40,60]]
            ]}),
            [50.0, 42.0],
            3.0
        ));
        assert!(hit(
            json!({"type":"Polygon","coordinates":[[[null,0],[0,0],[100,0],[100,null],[100,100],[0,100]]]}),
            [3.0, 50.0],
            4.0
        ));
        assert!(hit(square(), [50.0, 0.0], 0.0));
        assert!(hit(
            json!({"type":"Polygon","coordinates":[[[5,6],[5,6]]]}),
            [5.0, 6.0],
            0.0
        ));
        assert!(!hit(square(), [50.0, 0.0], -1.0));
        assert!(!hit(square(), [f64::NAN, 0.0], 8.0));
        assert!(!hit(square(), [50.0, 0.0], f64::INFINITY));
    }

    #[test]
    fn projection_effects_keep_pair_order_early_exit_and_errors() {
        let mut calls = Vec::new();
        assert!(
            polygon_edge_hit(&square(), [52.0, 4.0], 8.0, |point| {
                calls.push(point);
                Ok::<_, ()>(point)
            })
            .unwrap()
        );
        assert_eq!(calls, vec![[0.0, 0.0], [100.0, 0.0]]);
        calls.clear();
        assert!(
            !polygon_edge_hit(&square(), [52.0, 4.0], 8.0, |point| {
                calls.push(point);
                Ok::<_, ()>(if point == [0.0, 0.0] {
                    [f64::NAN, f64::INFINITY]
                } else {
                    point
                })
            })
            .unwrap()
        );
        assert_eq!(calls.len(), 8);
        assert_eq!(calls[0..2], [[0.0, 0.0], [100.0, 0.0]]);
        let error = polygon_edge_hit(&square(), [0.0, 0.0], 8.0, |_| Err("projection failure"));
        assert!(matches!(
            error,
            Err(PolygonEdgeHitError::Project("projection failure"))
        ));
    }
}
