//! Exact GeoJSON area predicates shared by local reference queries and other
//! bounded vector consumers. The R-tree is only a candidate accelerator; this
//! module decides whether a candidate actually intersects the requested area.

use geo::{
    BooleanOps, BoundingRect, Centroid, Coord, Geometry, GeometryCollection, Intersects,
    LineString, MultiLineString, MultiPoint, MultiPolygon, Point, Polygon,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const MAX_AREA_COORDINATES: usize = 50_000;

fn coordinate(value: &Value, count: &mut usize) -> Result<Coord<f64>, String> {
    let pair = value
        .as_array()
        .filter(|pair| pair.len() >= 2)
        .ok_or_else(|| "GeoJSON coordinate must contain longitude and latitude".to_string())?;
    let x = pair[0]
        .as_f64()
        .ok_or_else(|| "GeoJSON longitude must be numeric".to_string())?;
    let y = pair[1]
        .as_f64()
        .ok_or_else(|| "GeoJSON latitude must be numeric".to_string())?;
    if !x.is_finite()
        || !y.is_finite()
        || !(-180.0..=180.0).contains(&x)
        || !(-90.0..=90.0).contains(&y)
    {
        return Err("GeoJSON coordinates must be finite WGS84 longitude/latitude".into());
    }
    *count += 1;
    if *count > MAX_AREA_COORDINATES {
        return Err(format!("area exceeds {MAX_AREA_COORDINATES} coordinates"));
    }
    Ok(Coord { x, y })
}

fn coordinates(value: &Value, count: &mut usize) -> Result<Vec<Coord<f64>>, String> {
    value
        .as_array()
        .ok_or_else(|| "GeoJSON coordinate sequence must be an array".to_string())?
        .iter()
        .map(|value| coordinate(value, count))
        .collect()
}

fn ring(value: &Value, count: &mut usize) -> Result<LineString<f64>, String> {
    let coordinates = coordinates(value, count)?;
    if coordinates.len() < 4 || coordinates.first() != coordinates.last() {
        return Err(
            "GeoJSON polygon rings need at least four coordinates and must be closed".into(),
        );
    }
    Ok(LineString::new(coordinates))
}

fn polygon(value: &Value, count: &mut usize) -> Result<Polygon<f64>, String> {
    let rings = value
        .as_array()
        .filter(|rings| !rings.is_empty())
        .ok_or_else(|| "GeoJSON polygon needs an exterior ring".to_string())?;
    let exterior = ring(&rings[0], count)?;
    let interiors = rings[1..]
        .iter()
        .map(|value| ring(value, count))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Polygon::new(exterior, interiors))
}

fn parse_geometry(value: &Value, count: &mut usize) -> Result<Geometry<f64>, String> {
    let kind = value
        .get("type")
        .and_then(Value::as_str)
        .ok_or_else(|| "GeoJSON geometry type is required".to_string())?;
    let values = || {
        value
            .get("coordinates")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("GeoJSON {kind} coordinates must be an array"))
    };
    match kind {
        "Point" => Ok(Point::from(coordinate(
            value
                .get("coordinates")
                .ok_or("GeoJSON Point coordinates are required")?,
            count,
        )?)
        .into()),
        "MultiPoint" => Ok(MultiPoint::new(
            values()?
                .iter()
                .map(|value| coordinate(value, count).map(Point::from))
                .collect::<Result<Vec<_>, _>>()?,
        )
        .into()),
        "LineString" => Ok(LineString::new(coordinates(
            value
                .get("coordinates")
                .ok_or("GeoJSON LineString coordinates are required")?,
            count,
        )?)
        .into()),
        "MultiLineString" => Ok(MultiLineString::new(
            values()?
                .iter()
                .map(|value| coordinates(value, count).map(LineString::new))
                .collect::<Result<Vec<_>, _>>()?,
        )
        .into()),
        "Polygon" => Ok(polygon(
            value
                .get("coordinates")
                .ok_or("GeoJSON Polygon coordinates are required")?,
            count,
        )?
        .into()),
        "MultiPolygon" => Ok(MultiPolygon::new(
            values()?
                .iter()
                .map(|value| polygon(value, count))
                .collect::<Result<Vec<_>, _>>()?,
        )
        .into()),
        "GeometryCollection" => {
            let geometries = value
                .get("geometries")
                .and_then(Value::as_array)
                .ok_or("GeoJSON GeometryCollection geometries must be an array")?;
            Ok(Geometry::GeometryCollection(GeometryCollection::new_from(
                geometries
                    .iter()
                    .map(|value| parse_geometry(value, count))
                    .collect::<Result<Vec<_>, _>>()?,
            )))
        }
        _ => Err(format!("unsupported GeoJSON geometry type `{kind}`")),
    }
}

fn geometry_value(value: &Value) -> Result<&Value, String> {
    if value.get("type").and_then(Value::as_str) == Some("Feature") {
        value
            .get("geometry")
            .filter(|value| !value.is_null())
            .ok_or_else(|| "GeoJSON Feature has no geometry".to_string())
    } else {
        Ok(value)
    }
}

pub(crate) fn parsed_area(area: &Value) -> Result<Geometry<f64>, String> {
    let mut count = 0;
    let area = parse_geometry(geometry_value(area)?, &mut count)?;
    match area {
        Geometry::Polygon(_) | Geometry::MultiPolygon(_) => Ok(area),
        _ => Err("query area must be a GeoJSON Polygon or MultiPolygon".into()),
    }
}

pub fn validate_area(area: &Value) -> Result<(), String> {
    parsed_area(area).map(|_| ())
}

pub fn area_bounds(area: &Value) -> Result<[f64; 4], String> {
    let area = parsed_area(area)?;
    let bounds = area
        .bounding_rect()
        .ok_or_else(|| "query area has no spatial extent".to_string())?;
    Ok([
        bounds.min().x,
        bounds.min().y,
        bounds.max().x,
        bounds.max().y,
    ])
}

pub fn intersects_area(feature: &Value, area: &Value) -> Result<bool, String> {
    selects_area(feature, area, AreaSelection::Intersects)
}

/// How a provider decides which features one requested area selects.
///
/// This is not a stylistic choice. A footprint that straddles the edge of an
/// area is selected by `Intersects` and rejected by `CentroidInside`, so any
/// consumer that *removes* rows for an area must remove them under the same
/// rule the provider *fetched* them under. Removing more than the provider
/// will return loses a real building; removing less leaves a duplicate of one.
/// Both sides therefore name this value instead of hard-coding a predicate.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AreaSelection {
    /// The feature's geometry touches or overlaps the area anywhere.
    Intersects,
    /// The feature's centroid lies in the area — one footprint belongs to
    /// exactly one area, at the cost of dropping a crossing footprint whose
    /// centre is outside.
    CentroidInside,
}

/// Centroid of a feature's own geometry, in the feature's coordinates.
///
/// Derived from the geometry rather than read from a provider property: the
/// deletion side of a refresh sees only what it stored, and a property-supplied
/// centre could disagree with the footprint it was stored beside.
fn centroid_of(geometry: &Geometry<f64>) -> Option<Point<f64>> {
    geometry.centroid()
}

/// The ONE area predicate. Acquisition admission and refresh deletion both call
/// this with the same `selection`, so the two can never drift apart.
///
/// A feature with no readable geometry selects nothing rather than erroring:
/// an unindexable row is not in any area.
pub fn selects_area(
    feature: &Value,
    area: &Value,
    selection: AreaSelection,
) -> Result<bool, String> {
    PreparedArea::new(area, selection)?.selects(feature)
}

/// One immutable parsed area shared by all rows of a local query. Preparation
/// retains the same area admission and selection semantics as `selects_area`;
/// it establishes neither source coverage nor freshness.
pub struct PreparedArea {
    area: Geometry<f64>,
    selection: AreaSelection,
}

impl PreparedArea {
    pub fn new(area: &Value, selection: AreaSelection) -> Result<Self, String> {
        Ok(Self {
            area: parsed_area(area)?,
            selection,
        })
    }

    pub fn selects(&self, feature: &Value) -> Result<bool, String> {
        let geometry = match geometry_value(feature) {
            Ok(value) => {
                let mut count = 0;
                parse_geometry(value, &mut count)?
            }
            Err(_) => return Ok(false),
        };
        Ok(match self.selection {
            AreaSelection::Intersects => geometry.intersects(&self.area),
            AreaSelection::CentroidInside => {
                centroid_of(&geometry).is_some_and(|centre| centre.intersects(&self.area))
            }
        })
    }
}

/// True only when the entire query polygon is held. Holes and disconnected
/// coverage remain gaps; touching an edge does not make an outside area held.
pub fn covers_area(coverage: &Value, area: &Value) -> Result<bool, String> {
    let polygons = |value: &Value| -> Result<MultiPolygon<f64>, String> {
        match parsed_area(value)? {
            Geometry::Polygon(polygon) => Ok(MultiPolygon::new(vec![polygon])),
            Geometry::MultiPolygon(polygons) => Ok(polygons),
            _ => unreachable!("parsed_area admits only polygons"),
        }
    };
    let held = polygons(coverage)?;
    let wanted = polygons(area)?;
    if wanted.0.is_empty() {
        return Err("query area is empty".into());
    }
    Ok(wanted.difference(&held).0.is_empty())
}

#[cfg(test)]
mod tests {
    #[test]
    fn complete_area_coverage_respects_holes_disconnected_regions_and_boundaries() {
        let ring = |w: f64, s: f64, e: f64, n: f64| {
            serde_json::json!([[w, s], [e, s], [e, n], [w, n], [w, s]])
        };
        let polygon = |r: Value| serde_json::json!({"type":"Polygon","coordinates":[r]});
        let held = serde_json::json!({"type":"MultiPolygon","coordinates":[[ring(0.,0.,2.,2.)],[ring(4.,0.,6.,2.)]]});
        assert!(super::covers_area(&held, &held).unwrap());
        assert!(!super::covers_area(&held, &polygon(ring(0., 0., 6., 2.))).unwrap());
        let hole = serde_json::json!({"type":"Polygon","coordinates":[ring(0.,0.,6.,6.),ring(2.,2.,4.,4.)]});
        assert!(!super::covers_area(&hole, &polygon(ring(1., 1., 5., 5.))).unwrap());
        assert!(super::covers_area(&hole, &polygon(ring(0., 0., 2., 2.))).unwrap());
    }

    use super::*;
    use serde_json::json;

    #[test]
    fn prepared_area_keeps_holes_boundaries_selection_and_captured_geometry() {
        let mut area = json!({"type":"Polygon","coordinates":[
            [[0.,0.],[4.,0.],[4.,4.],[0.,4.],[0.,0.]],
            [[1.,1.],[3.,1.],[3.,3.],[1.,3.],[1.,1.]]
        ]});
        let intersects = PreparedArea::new(&area, AreaSelection::Intersects).unwrap();
        let centroid = PreparedArea::new(&area, AreaSelection::CentroidInside).unwrap();
        let inside = json!({"type":"Point","coordinates":[0.5,0.5]});
        let hole = json!({"type":"Point","coordinates":[2.,2.]});
        let edge = json!({"type":"Point","coordinates":[0.,0.5]});
        let crossing = json!({"type":"Polygon","coordinates":[
            [[-1.,0.2],[0.1,0.2],[0.1,0.4],[-1.,0.4],[-1.,0.2]]
        ]});
        for prepared in [&intersects, &centroid] {
            assert!(prepared.selects(&inside).unwrap());
            assert!(!prepared.selects(&hole).unwrap());
            assert!(prepared.selects(&edge).unwrap());
            assert!(!prepared
                .selects(&json!({"type":"Feature","geometry":null}))
                .unwrap());
            assert!(prepared
                .selects(&json!({"type":"Point","coordinates":[]}))
                .is_err());
        }
        assert!(intersects.selects(&crossing).unwrap());
        assert!(!centroid.selects(&crossing).unwrap());
        area["coordinates"] = json!([[[10., 10.], [11., 10.], [11., 11.], [10., 11.], [10., 10.]]]);
        assert!(intersects.selects(&inside).unwrap());
        assert!(!selects_area(&inside, &area, AreaSelection::Intersects).unwrap());
        assert!(PreparedArea::new(&inside, AreaSelection::Intersects).is_err());
    }

    #[test]
    fn polygon_and_multipolygon_are_exact_area_selectors() {
        let area = json!({"type":"Polygon","coordinates":[[[0.,0.],[2.,0.],[2.,2.],[0.,2.],[0.,0.]],[[0.5,0.5],[1.5,0.5],[1.5,1.5],[0.5,1.5],[0.5,0.5]]]});
        assert_eq!(area_bounds(&area).unwrap(), [0., 0., 2., 2.]);
        assert!(
            intersects_area(&json!({"type":"Point","coordinates":[0.25,0.25]}), &area).unwrap()
        );
        assert!(!intersects_area(&json!({"type":"Point","coordinates":[1.,1.]}), &area).unwrap());
        let multi = json!({"type":"MultiPolygon","coordinates":[[[[3.,3.],[4.,3.],[4.,4.],[3.,4.],[3.,3.]]]]});
        assert!(validate_area(&multi).is_ok());
    }

    #[test]
    fn a_crossing_footprint_separates_the_two_area_selections() {
        // The footprint straddles the eastern edge: half of it is inside the
        // area and its centre is outside. This single feature is the whole
        // reason the predicate has to be named — one rule keeps it, the other
        // does not, and a refresh that deletes under the wrong one loses it.
        let area =
            json!({"type":"Polygon","coordinates":[[[0.,0.],[1.,0.],[1.,1.],[0.,1.],[0.,0.]]]});
        let crossing = json!({"type":"Feature","properties":{},
            "geometry":{"type":"Polygon","coordinates":[[[0.9,0.4],[1.3,0.4],[1.3,0.6],[0.9,0.6],[0.9,0.4]]]}});
        assert!(selects_area(&crossing, &area, AreaSelection::Intersects).unwrap());
        assert!(!selects_area(&crossing, &area, AreaSelection::CentroidInside).unwrap());
        // A footprint wholly inside is selected by both, so the two rules only
        // ever differ on the boundary.
        let inside = json!({"type":"Feature","properties":{},
            "geometry":{"type":"Polygon","coordinates":[[[0.2,0.2],[0.3,0.2],[0.3,0.3],[0.2,0.3],[0.2,0.2]]]}});
        for selection in [AreaSelection::Intersects, AreaSelection::CentroidInside] {
            assert!(selects_area(&inside, &area, selection).unwrap());
        }
        // `intersects_area` is exactly the intersecting selection, not a second
        // implementation of it.
        assert_eq!(
            intersects_area(&crossing, &area).unwrap(),
            selects_area(&crossing, &area, AreaSelection::Intersects).unwrap()
        );
        // A feature with no geometry is in no area under either rule.
        let empty = json!({"type":"Feature","properties":{},"geometry":null});
        for selection in [AreaSelection::Intersects, AreaSelection::CentroidInside] {
            assert!(!selects_area(&empty, &area, selection).unwrap());
        }
    }

    #[test]
    fn open_and_non_area_selectors_are_refused() {
        assert!(validate_area(&json!({"type":"Point","coordinates":[0.,0.]})).is_err());
        assert!(validate_area(
            &json!({"type":"Polygon","coordinates":[[[0.,0.],[1.,0.],[1.,1.],[0.,1.]]]})
        )
        .is_err());
    }
}
