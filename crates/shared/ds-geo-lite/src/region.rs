//! The ONE region predicate: *does this geometry lie in this region?*
//!
//! ## Why this module exists
//!
//! A lasso, a marquee, a saved area filter and `ds design features select
//! --region` all ask the same question, and before this module they answered it
//! from three separate copies of the same ring algebra:
//!
//! | Copy | Coordinate space | Reached from |
//! |---|---|---|
//! | `ds_geo::session` | projected metres | the browser edit-session lasso |
//! | `ds_network::memory_selection` | WGS84 lon/lat | the grid + survey lasso |
//! | `ds-web/src/lib/selection/lasso-run.ts` | *screen pixels* | the survey lasso |
//!
//! The third one was not even a copy of the algebra — it was
//! `map.queryRenderedFeatures(screenBounds)`, which answers "what is painted at
//! these pixels right now, at this zoom, after tile simplification and label
//! collision". That is a different, lossy question, and it cannot be asked at
//! all with the map closed.
//!
//! The predicate is intentionally coordinate-space agnostic: it takes plain
//! `[x, y]` rings, so the projected-metres consumer and the WGS84 consumer share
//! one implementation without either converting to the other's space. Callers
//! that face GeoJSON own the WGS84 validation; see
//! the consumer’s feature selection admission.
//!
//! ## Semantics
//!
//! * `Intersects` — the geometry touches or overlaps the region anywhere,
//!   including the boundary.
//! * `Within` — the geometry lies entirely inside the region. Touching the
//!   boundary is allowed; *crossing* it is not.
//!
//! A geometry with no readable coordinates is in no region under either rule —
//! it is never an error, because an unindexable row is simply not selected.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One position, in whatever coordinate space the caller indexes in.
pub type RegionCoord = [f64; 2];
/// One closed or open ring of positions.
pub type RegionRing = Vec<RegionCoord>;
/// A polygon: exterior ring first, holes after.
pub type RegionRings = Vec<RegionRing>;

/// How a region decides which geometries it selects.
///
/// This is never a stylistic default. A line that crosses the lasso is selected
/// by `Intersects` and rejected by `Within`, so every surface that offers both
/// must carry the operator's choice through to the kernel rather than assuming
/// one.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RegionPredicate {
    /// The geometry touches or overlaps the region anywhere.
    #[default]
    Intersects,
    /// The geometry lies wholly inside the region; crossing the boundary
    /// excludes it.
    Within,
    /// No part of a nonempty, readable geometry touches the selector.
    Outside,
}

/// Read a GeoJSON `Polygon` geometry into rings.
///
/// Returns `None` for any other geometry type, a missing or malformed coordinate
/// array, or an empty ring set. Positions that are not finite numeric pairs are
/// dropped rather than erroring; the caller validates the coordinate space it
/// cares about.
pub fn rings_from_geojson_polygon(value: &Value) -> Option<RegionRings> {
    if value.get("type").and_then(Value::as_str) != Some("Polygon") {
        return None;
    }
    value.get("coordinates").and_then(rings_from_value)
}

/// Does one GeoJSON geometry lie in the region under `predicate`?
///
/// Accepts every GeoJSON geometry type including `GeometryCollection`. A `None`
/// or unreadable geometry selects nothing.
pub fn geojson_geometry_selected(
    geometry: Option<&Value>,
    region: &RegionRings,
    predicate: RegionPredicate,
) -> bool {
    let Some(geometry) = geometry else {
        return false;
    };
    match predicate {
        RegionPredicate::Intersects => geometry_intersects_polygon(geometry, region),
        RegionPredicate::Within => geometry_within_polygon(geometry, region),
        RegionPredicate::Outside => {
            geometry_readable(geometry) && !geometry_intersects_polygon(geometry, region)
        }
    }
}
fn geometry_readable(geometry: &Value) -> bool {
    match geometry.get("type").and_then(Value::as_str) {
        Some("Point") => point_coords(geometry.get("coordinates")).is_some(),
        Some("MultiPoint") => {
            coord_array(geometry.get("coordinates")).is_some_and(|v| !v.is_empty())
        }
        Some("LineString") => {
            coord_array(geometry.get("coordinates")).is_some_and(|v| v.len() >= 2)
        }
        Some("Polygon") => rings_from_geojson_polygon(geometry)
            .is_some_and(|r| !r.is_empty() && r.iter().all(|r| r.len() >= 3)),
        Some("MultiLineString" | "MultiPolygon") => geometry
            .get("coordinates")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                !items.is_empty()
                    && items.iter().all(|v| {
                        if geometry["type"] == "MultiLineString" {
                            coord_array(Some(v)).is_some_and(|v| v.len() >= 2)
                        } else {
                            rings_from_value(v)
                                .is_some_and(|r| !r.is_empty() && r.iter().all(|r| r.len() >= 3))
                        }
                    })
            }),
        Some("GeometryCollection") => geometry
            .get("geometries")
            .and_then(Value::as_array)
            .is_some_and(|v| !v.is_empty() && v.iter().all(geometry_readable)),
        _ => false,
    }
}

/// Does a GeoJSON `Feature`'s own top-level geometry lie in the region?
///
/// `properties.geometry`, `x` and `y` are never fallbacks: their CRS is
/// undeclared, and the same rule already governs
/// the consumer’s feature selection admission's samples.
pub fn geojson_feature_selected(
    feature: &Value,
    region: &RegionRings,
    predicate: RegionPredicate,
) -> bool {
    geojson_geometry_selected(feature.get("geometry"), region, predicate)
}

fn geometry_intersects_polygon(geometry: &Value, selector: &RegionRings) -> bool {
    match geometry.get("type").and_then(Value::as_str) {
        Some("Point") => point_coords(geometry.get("coordinates"))
            .is_some_and(|coord| point_in_polygon(coord, selector)),
        Some("MultiPoint") => coord_array(geometry.get("coordinates")).is_some_and(|coords| {
            coords
                .into_iter()
                .any(|coord| point_in_polygon(coord, selector))
        }),
        Some("LineString") => coord_array(geometry.get("coordinates"))
            .is_some_and(|line| line_intersects_polygon(&line, selector)),
        Some("MultiLineString") => geometry
            .get("coordinates")
            .and_then(Value::as_array)
            .is_some_and(|lines| {
                lines.iter().any(|line| {
                    coord_array(Some(line))
                        .is_some_and(|coords| line_intersects_polygon(&coords, selector))
                })
            }),
        Some("Polygon") => rings_from_geojson_polygon(geometry)
            .is_some_and(|rings| polygon_intersects_polygon(&rings, selector)),
        Some("MultiPolygon") => geometry
            .get("coordinates")
            .and_then(Value::as_array)
            .is_some_and(|polygons| {
                polygons.iter().any(|polygon| {
                    rings_from_value(polygon)
                        .is_some_and(|rings| polygon_intersects_polygon(&rings, selector))
                })
            }),
        Some("GeometryCollection") => geometry
            .get("geometries")
            .and_then(Value::as_array)
            .is_some_and(|geometries| {
                geometries
                    .iter()
                    .any(|candidate| geometry_intersects_polygon(candidate, selector))
            }),
        _ => false,
    }
}

fn geometry_within_polygon(geometry: &Value, selector: &RegionRings) -> bool {
    match geometry.get("type").and_then(Value::as_str) {
        Some("Point") => point_coords(geometry.get("coordinates"))
            .is_some_and(|coord| point_in_polygon(coord, selector)),
        Some("MultiPoint") => coord_array(geometry.get("coordinates")).is_some_and(|coords| {
            !coords.is_empty()
                && coords
                    .into_iter()
                    .all(|coord| point_in_polygon(coord, selector))
        }),
        Some("LineString") => coord_array(geometry.get("coordinates"))
            .is_some_and(|line| line_within_polygon(&line, selector)),
        Some("MultiLineString") => geometry
            .get("coordinates")
            .and_then(Value::as_array)
            .is_some_and(|lines| {
                !lines.is_empty()
                    && lines.iter().all(|line| {
                        coord_array(Some(line))
                            .is_some_and(|coords| line_within_polygon(&coords, selector))
                    })
            }),
        Some("Polygon") => rings_from_geojson_polygon(geometry)
            .is_some_and(|rings| polygon_within_polygon(&rings, selector)),
        Some("MultiPolygon") => geometry
            .get("coordinates")
            .and_then(Value::as_array)
            .is_some_and(|polygons| {
                !polygons.is_empty()
                    && polygons.iter().all(|polygon| {
                        rings_from_value(polygon)
                            .is_some_and(|rings| polygon_within_polygon(&rings, selector))
                    })
            }),
        Some("GeometryCollection") => geometry
            .get("geometries")
            .and_then(Value::as_array)
            .is_some_and(|geometries| {
                !geometries.is_empty()
                    && geometries
                        .iter()
                        .all(|candidate| geometry_within_polygon(candidate, selector))
            }),
        _ => false,
    }
}

/// Do two polygons overlap, share a boundary, or contain one another?
pub fn polygon_intersects_polygon(a: &[RegionRing], b: &[RegionRing]) -> bool {
    rings_have_segment_intersection(a, b)
        || a.iter().flatten().any(|coord| point_in_polygon(*coord, b))
        || b.iter().flatten().any(|coord| point_in_polygon(*coord, a))
}

/// Does `inner` lie wholly inside `outer` without crossing its boundary?
pub fn polygon_within_polygon(inner: &[RegionRing], outer: &[RegionRing]) -> bool {
    !inner.is_empty()
        && inner.iter().all(|ring| {
            !ring.is_empty() && ring.iter().all(|point| point_in_polygon(*point, outer))
        })
        && !rings_cross_polygon_boundary(inner, outer)
}

/// Does a polyline touch the polygon anywhere, including only crossing it?
pub fn line_intersects_polygon(line: &[RegionCoord], polygon: &[RegionRing]) -> bool {
    if line.is_empty() {
        return false;
    }
    line.iter().any(|coord| point_in_polygon(*coord, polygon))
        || line.windows(2).any(|segment| {
            polygon.iter().any(|ring| {
                ring_segments(ring)
                    .any(|edge| segments_intersect(segment[0], segment[1], edge.0, edge.1))
            })
        })
}

/// Does a polyline lie wholly inside the polygon without crossing out of it?
pub fn line_within_polygon(line: &[RegionCoord], polygon: &[RegionRing]) -> bool {
    !line.is_empty()
        && line.iter().all(|coord| point_in_polygon(*coord, polygon))
        && !polyline_crosses_polygon_boundary(line, polygon)
}

fn rings_cross_polygon_boundary(a: &[RegionRing], b: &[RegionRing]) -> bool {
    a.iter()
        .any(|ring| polyline_crosses_polygon_boundary(ring, b))
}

fn polyline_crosses_polygon_boundary(line: &[RegionCoord], polygon: &[RegionRing]) -> bool {
    line.windows(2).any(|segment| {
        polygon.iter().any(|ring| {
            ring_segments(ring).any(|edge| {
                segment_has_disallowed_boundary_intersection(segment[0], segment[1], edge.0, edge.1)
            })
        })
    })
}

fn rings_have_segment_intersection(a: &[RegionRing], b: &[RegionRing]) -> bool {
    a.iter().any(|ring_a| {
        ring_segments(ring_a).any(|edge_a| {
            b.iter().any(|ring_b| {
                ring_segments(ring_b)
                    .any(|edge_b| segments_intersect(edge_a.0, edge_a.1, edge_b.0, edge_b.1))
            })
        })
    })
}

/// Is a position inside the polygon, counting the boundary and honouring holes?
pub fn point_in_polygon(point: RegionCoord, rings: &[RegionRing]) -> bool {
    let Some(outer) = rings.first() else {
        return false;
    };
    if point_on_ring(point, outer) {
        return true;
    }
    if !point_in_ring(point, outer) {
        return false;
    }
    for hole in rings.iter().skip(1) {
        if point_on_ring(point, hole) {
            return true;
        }
        if point_in_ring(point, hole) {
            return false;
        }
    }
    true
}

/// Ray-cast containment against one ring, boundary excluded.
///
/// Exposed because ranking a pick by distance needs the same containment answer
/// the region predicate uses — a click inside a polygon scores zero distance.
pub fn point_in_ring(point: RegionCoord, ring: &[RegionCoord]) -> bool {
    if ring.len() < 3 {
        return false;
    }
    crate::predicates::ring_contains_by(
        ring,
        point,
        |p| *p,
        crate::predicates::RingEdges::Previous,
        crate::predicates::RayArithmetic::ProductFirst,
    )
}

fn point_on_ring(point: RegionCoord, ring: &[RegionCoord]) -> bool {
    ring_segments(ring).any(|(a, b)| point_on_segment(point, a, b))
}

fn ring_segments(ring: &[RegionCoord]) -> impl Iterator<Item = (RegionCoord, RegionCoord)> + '_ {
    ring.iter()
        .copied()
        .zip(ring.iter().copied().cycle().skip(1))
        .take(
            ring.len()
                .saturating_sub(usize::from(ring.first() == ring.last())),
        )
}

fn segments_intersect(a1: RegionCoord, a2: RegionCoord, b1: RegionCoord, b2: RegionCoord) -> bool {
    crate::predicates::segments_intersect(
        a1,
        a2,
        b1,
        b2,
        crate::predicates::SegmentTolerance::REGION,
        crate::predicates::Crossing::Quantized,
    )
}

fn segment_has_disallowed_boundary_intersection(
    a: RegionCoord,
    b: RegionCoord,
    c: RegionCoord,
    d: RegionCoord,
) -> bool {
    let o1 = orient(a, b, c);
    let o2 = orient(a, b, d);
    let o3 = orient(c, d, a);
    let o4 = orient(c, d, b);

    if o1 == 0 && o2 == 0 && o3 == 0 && o4 == 0 {
        return false;
    }

    if (o1 > 0) != (o2 > 0) && (o3 > 0) != (o4 > 0) {
        return true;
    }

    let feature_endpoint_on_boundary = point_on_segment(a, c, d) || point_on_segment(b, c, d);
    let boundary_vertex_inside_feature_segment =
        point_on_segment(c, a, b) || point_on_segment(d, a, b);
    boundary_vertex_inside_feature_segment && !feature_endpoint_on_boundary
}

fn orient(a: RegionCoord, b: RegionCoord, c: RegionCoord) -> i8 {
    crate::predicates::orientation_sign(a, b, c, crate::tolerance::REGION_DETERMINANT)
}

fn point_on_segment(point: RegionCoord, a: RegionCoord, b: RegionCoord) -> bool {
    crate::predicates::point_on_segment(point, a, b, crate::predicates::SegmentTolerance::REGION)
}

fn rings_from_value(value: &Value) -> Option<RegionRings> {
    let rings = value.as_array()?;
    let out = rings
        .iter()
        .filter_map(|ring| coord_array(Some(ring)))
        .collect::<Vec<_>>();
    (!out.is_empty()).then_some(out)
}

fn coord_array(value: Option<&Value>) -> Option<Vec<RegionCoord>> {
    let out = value?
        .as_array()?
        .iter()
        .filter_map(|coord| point_coords(Some(coord)))
        .collect::<Vec<_>>();
    Some(out)
}

fn point_coords(value: Option<&Value>) -> Option<RegionCoord> {
    let coords = value?.as_array()?;
    let x = coords.first()?.as_f64()?;
    let y = coords.get(1)?.as_f64()?;
    (x.is_finite() && y.is_finite()).then_some([x, y])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn square() -> RegionRings {
        rings_from_geojson_polygon(&json!({
            "type": "Polygon",
            "coordinates": [[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0], [0.0, 0.0]]]
        }))
        .unwrap()
    }

    fn selected(geometry: Value, predicate: RegionPredicate) -> bool {
        geojson_geometry_selected(Some(&geometry), &square(), predicate)
    }

    #[test]
    fn boundary_positions_are_inside_and_a_crossing_line_separates_the_two_rules() {
        assert!(selected(
            json!({"type":"Point","coordinates":[5,5]}),
            RegionPredicate::Intersects
        ));
        assert!(selected(
            json!({"type":"Point","coordinates":[10,5]}),
            RegionPredicate::Intersects
        ));
        assert!(!selected(
            json!({"type":"Point","coordinates":[20,5]}),
            RegionPredicate::Intersects
        ));

        let crossing = json!({"type":"LineString","coordinates":[[-1,5],[11,5]]});
        assert!(selected(crossing.clone(), RegionPredicate::Intersects));
        assert!(!selected(crossing, RegionPredicate::Within));

        let inside = json!({"type":"LineString","coordinates":[[1,1],[9,9]]});
        assert!(selected(inside.clone(), RegionPredicate::Intersects));
        assert!(selected(inside, RegionPredicate::Within));
    }

    #[test]
    fn a_hole_excludes_its_interior_but_not_its_edge() {
        let donut = rings_from_geojson_polygon(&json!({
            "type": "Polygon",
            "coordinates": [
                [[0.0,0.0],[10.0,0.0],[10.0,10.0],[0.0,10.0],[0.0,0.0]],
                [[4.0,4.0],[6.0,4.0],[6.0,6.0],[4.0,6.0],[4.0,4.0]]
            ]
        }))
        .unwrap();
        assert!(!point_in_polygon([5.0, 5.0], &donut));
        assert!(point_in_polygon([4.0, 5.0], &donut));
        assert!(point_in_polygon([1.0, 1.0], &donut));
    }

    #[test]
    fn unreadable_and_absent_geometry_is_in_no_region_under_either_rule() {
        for predicate in [RegionPredicate::Intersects, RegionPredicate::Within] {
            assert!(!geojson_geometry_selected(None, &square(), predicate));
            assert!(!selected(Value::Null, predicate));
            assert!(!selected(
                json!({"type":"Bogus","coordinates":[1,1]}),
                predicate
            ));
            assert!(!geojson_feature_selected(
                &json!({"type":"Feature","properties":{},"geometry":null}),
                &square(),
                predicate
            ));
        }
    }

    #[test]
    fn multi_geometries_are_any_for_intersects_and_all_for_within() {
        let straddling = json!({
            "type":"MultiPoint",
            "coordinates":[[5,5],[50,50]]
        });
        assert!(selected(straddling.clone(), RegionPredicate::Intersects));
        assert!(!selected(straddling, RegionPredicate::Within));

        let empty = json!({"type":"MultiPoint","coordinates":[]});
        assert!(!selected(empty.clone(), RegionPredicate::Intersects));
        assert!(!selected(empty, RegionPredicate::Within));
    }

    #[test]
    fn only_a_geojson_polygon_reads_as_a_region() {
        assert!(rings_from_geojson_polygon(&json!({"type":"Point","coordinates":[0,0]})).is_none());
        assert!(rings_from_geojson_polygon(&json!({"type":"Polygon"})).is_none());
        assert!(
            rings_from_geojson_polygon(&json!({"type":"MultiPolygon","coordinates":[]})).is_none()
        );
    }

    #[test]
    fn the_predicate_wire_name_is_snake_case_on_both_directions() {
        assert_eq!(
            serde_json::to_value(RegionPredicate::Within).unwrap(),
            json!("within")
        );
        assert_eq!(
            serde_json::from_value::<RegionPredicate>(json!("intersects")).unwrap(),
            RegionPredicate::Intersects
        );
        assert_eq!(RegionPredicate::default(), RegionPredicate::Intersects);
    }
}

#[cfg(test)]
mod outside_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn outside_is_disjoint_and_never_selects_missing_or_empty_geometry() {
        let region = vec![vec![[0., 0.], [10., 0.], [10., 10.], [0., 10.], [0., 0.]]];
        for (geometry, expected) in [
            (json!({"type":"Point","coordinates":[20,20]}), true),
            (json!({"type":"Point","coordinates":[10,5]}), false),
            (
                json!({"type":"LineString","coordinates":[[-1,5],[11,5]]}),
                false,
            ),
            (
                json!({"type":"LineString","coordinates":[[11,5],[20,5]]}),
                true,
            ),
            (json!({"type":"GeometryCollection","geometries":[]}), false),
            (json!({"type":"MultiPoint","coordinates":[]}), false),
            (Value::Null, false),
        ] {
            assert_eq!(
                geojson_geometry_selected(Some(&geometry), &region, RegionPredicate::Outside),
                expected,
                "{geometry}"
            );
        }
        assert!(!geojson_geometry_selected(
            None,
            &region,
            RegionPredicate::Outside
        ));
    }
}
