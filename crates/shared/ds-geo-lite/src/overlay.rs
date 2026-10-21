//! Polygon Boolean operations using the same engine as area predicates.
//! Inputs are bounded, valid, closed WGS84 Polygon/MultiPolygon geometries.
//! Calculations use planar longitude/latitude and the engine's even-odd rule;
//! no caller epsilon, healing, projection choice or attribute policy is added.

use geo::{BooleanOps, Geometry, MultiPolygon, Validation};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Intersection,
    Union,
    Difference,
    SymmetricDifference,
}

fn area(value: &Value) -> Result<MultiPolygon<f64>, String> {
    let area = match crate::spatial_predicate::parsed_area(value)? {
        Geometry::Polygon(polygon) => MultiPolygon::new(vec![polygon]),
        Geometry::MultiPolygon(polygons) => polygons,
        _ => unreachable!("area admission accepts polygons only"),
    };
    area.check_validation()
        .map_err(|error| format!("invalid overlay polygon: {error:?}"))?;
    Ok(area)
}

/// Return a MultiPolygon, including an empty coordinates array for empty output.
/// Each input has the shared 50,000-coordinate admission bound. Features are
/// accepted, but this geometry operation does not copy their properties.
pub fn polygons(left: &Value, right: &Value, operation: Operation) -> Result<Value, String> {
    let left = area(left)?;
    let right = area(right)?;
    let result = match operation {
        Operation::Intersection => left.intersection(&right),
        Operation::Union => left.union(&right),
        Operation::Difference => left.difference(&right),
        Operation::SymmetricDifference => left.xor(&right),
    };
    let coordinates: Vec<Vec<Vec<[f64; 2]>>> = result
        .0
        .iter()
        .map(|polygon| {
            std::iter::once(polygon.exterior())
                .chain(polygon.interiors())
                .map(|ring| ring.0.iter().map(|c| [c.x, c.y]).collect())
                .collect()
        })
        .collect();
    Ok(json!({"type":"MultiPolygon", "coordinates":coordinates}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use geo::Area;

    fn square(x: f64, y: f64, size: f64) -> Value {
        json!({"type":"Polygon","coordinates":[[[x,y],[x+size,y],[x+size,y+size],[x,y+size],[x,y]]]})
    }

    #[test]
    fn all_operations_share_the_engine_and_orientation_rule() {
        let left = square(0., 0., 2.);
        let mut right = square(1., 1., 2.);
        right["coordinates"][0].as_array_mut().unwrap().reverse();
        for (operation, expected) in [
            (Operation::Intersection, 1.),
            (Operation::Union, 7.),
            (Operation::Difference, 3.),
            (Operation::SymmetricDifference, 6.),
        ] {
            let output = polygons(&left, &right, operation).unwrap();
            assert!((area(&output).unwrap().unsigned_area() - expected).abs() < 1e-8);
        }
    }

    #[test]
    fn holes_empty_results_and_invalid_inputs_have_explicit_contracts() {
        let output = polygons(
            &square(0., 0., 4.),
            &square(1., 1., 2.),
            Operation::Difference,
        )
        .unwrap();
        assert_eq!(output["coordinates"][0].as_array().unwrap().len(), 2);
        assert_eq!(area(&output).unwrap().unsigned_area(), 12.);
        let empty = polygons(
            &square(0., 0., 1.),
            &square(2., 2., 1.),
            Operation::Intersection,
        )
        .unwrap();
        assert_eq!(empty, json!({"type":"MultiPolygon","coordinates":[]}));
        let bowtie = json!({"type":"Polygon","coordinates":[[[0,0],[2,2],[0,2],[2,0],[0,0]]]});
        assert!(polygons(&bowtie, &square(0., 0., 1.), Operation::Union).is_err());
        assert!(
            polygons(
                &json!({"type":"Point","coordinates":[0,0]}),
                &square(0., 0., 1.),
                Operation::Union
            )
            .is_err()
        );
    }
}
