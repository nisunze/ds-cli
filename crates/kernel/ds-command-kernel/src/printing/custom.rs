//! Exact local custom-print area preparation. Hosts fetch the named boundaries;
//! the kernel checks identity, connected coverage and reproducible composition.
use ds_geo_lite::spatial_predicate::{area_bounds, intersects_area};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Boundary {
    pub level: String,
    pub code: String,
    pub name: String,
    pub geometry: Value,
}

pub fn plan_area(project: &str, id: &str, codes: &[String]) -> Result<Value, String> {
    let valid_id = |value: &str| {
        !value.is_empty()
            && value.len() <= 128
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    if !valid_id(project) || !valid_id(id) {
        return Err("Custom printing requires exact project and map identifiers".into());
    }
    let selected: BTreeSet<_> = codes.iter().cloned().collect();
    if codes.is_empty()
        || codes.len() > 8
        || selected.len() != codes.len()
        || codes
            .iter()
            .any(|code| code.len() != 4 || !code.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("Custom printing requires 1..8 unique exact Rwanda sector codes".into());
    }
    Ok(json!({"project":project,"id":id,"codes":selected}))
}

pub fn prepare_area(
    project: &str,
    id: &str,
    codes: &[String],
    boundaries: Vec<Boundary>,
) -> Result<Value, String> {
    plan_area(project, id, codes)?;
    let selected: BTreeSet<_> = codes.iter().cloned().collect();
    if boundaries.len() != selected.len() {
        return Err("Custom print boundary capture is incomplete".into());
    }
    let mut captured = BTreeMap::new();
    for boundary in boundaries {
        if boundary.level != "sector"
            || !selected.contains(&boundary.code)
            || captured.contains_key(&boundary.code)
            || boundary.name.is_empty()
            || boundary.name.trim() != boundary.name
            || boundary.name.len() > 200
            || boundary.name.chars().any(char::is_control)
        {
            return Err(
                "Custom print boundary identity does not match the selected sectors".into(),
            );
        }
        crate::reference_cache::validate_area(&boundary.geometry)?;
        captured.insert(boundary.code.clone(), boundary);
    }
    let ordered = captured.values().collect::<Vec<_>>();
    let mut connected = BTreeSet::from([0]);
    loop {
        let before = connected.len();
        for (index, boundary) in ordered.iter().enumerate() {
            if connected.contains(&index) {
                continue;
            }
            for neighbour in connected.clone() {
                if intersects_area(&boundary.geometry, &ordered[neighbour].geometry)? {
                    connected.insert(index);
                    break;
                }
            }
        }
        if connected.len() == before {
            break;
        }
    }
    if connected.len() != ordered.len() {
        return Err("Selected sectors do not form one connected custom print area".into());
    }
    // Retain each polygon, including all interior rings. A bounding box is a
    // viewport hint only; it must never substitute for the selected area.
    let mut polygons = Vec::new();
    let mut features = Vec::new();
    for boundary in &ordered {
        match boundary.geometry["type"].as_str() {
            Some("Polygon") => polygons.push(boundary.geometry["coordinates"].clone()),
            Some("MultiPolygon") => polygons.extend(
                boundary.geometry["coordinates"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .cloned(),
            ),
            _ => return Err("Custom print boundaries must be polygonal".into()),
        }
        features.push(json!({"type":"Feature","id":boundary.code,
            "properties":{"code":boundary.code,"name":boundary.name},"geometry":boundary.geometry}));
    }
    let area = json!({"type":"MultiPolygon","coordinates":polygons});
    let bounds = area_bounds(&area)?;
    let bytes = serde_json::to_vec(&area).map_err(|e| e.to_string())?;
    Ok(
        json!({"schema":"ds.custom-print-area/v1","project":project,"id":id,
        "sector_codes":selected,"area":area,"bounds":bounds,
        "area_sha256":format!("{:x}",Sha256::digest(bytes)),
        "sectors":{"type":"FeatureCollection","features":features},
        "connected":true,"publication":"local_only"}),
    )
}

/// Compact default projection; complete coordinates are explicitly requested.
pub fn area_result(mut area: Value, include_geometry: bool) -> Value {
    if !include_geometry {
        area.as_object_mut().unwrap().remove("area");
        area["sectors"] = json!(area["sectors"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| f["properties"].clone())
            .collect::<Vec<_>>());
    }
    area["geometry_included"] = json!(include_geometry);
    area
}

#[cfg(test)]
mod tests {
    use super::*;
    fn boundary(code: &str, west: f64) -> Boundary {
        Boundary {
            level: "sector".into(),
            code: code.into(),
            name: code.into(),
            geometry: json!({"type":"Polygon",
            "coordinates":[[[west,-2.0],[west+1.0,-2.0],[west+1.0,-1.0],[west,-1.0],[west,-2.0]]]}),
        }
    }
    #[test]
    fn connected_capture_is_order_independent_and_keeps_holes() {
        let mut first = boundary("2401", 29.0);
        first.geometry["coordinates"]
            .as_array_mut()
            .unwrap()
            .push(json!([
                [29.1, -1.9],
                [29.2, -1.9],
                [29.2, -1.8],
                [29.1, -1.8],
                [29.1, -1.9]
            ]));
        let a = prepare_area(
            "project",
            "map",
            &["2401".into(), "2402".into()],
            vec![first, boundary("2402", 30.0)],
        )
        .unwrap();
        let mut reversed: Vec<Boundary> = a["sectors"]["features"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| Boundary {
                level: "sector".into(),
                code: f["id"].as_str().unwrap().into(),
                name: f["properties"]["name"].as_str().unwrap().into(),
                geometry: f["geometry"].clone(),
            })
            .collect();
        reversed.reverse();
        let b = prepare_area("project", "map", &["2402".into(), "2401".into()], reversed).unwrap();
        assert_eq!(a, b);
        assert_eq!(a["area"]["coordinates"][0].as_array().unwrap().len(), 2);
        assert!(!intersects_area(
            &json!({"type":"Point","coordinates":[29.15,-1.85]}),
            &a["area"]
        )
        .unwrap());
    }
    #[test]
    fn command_validates_before_fetch_and_geometry_is_explicit() {
        let invoke = |value: Value| -> Result<Value, String> {
            serde_json::from_str(&crate::report::evaluate(
                &serde_json::to_vec(&value).unwrap(),
            )?)
            .map_err(|error| error.to_string())
        };
        assert!(invoke(json!({"command":"custom_area_plan","project":"project","id":"huye","codes":["2401","2401"]})).is_err());
        assert!(invoke(json!({"command":"custom_area_plan","project":"project","id":"../huye","codes":["2401"]})).is_err());
        let mut request = json!({"command":"custom_area","project":"project","id":"huye",
            "codes":["2401"],"boundaries":[boundary("2401",29.0)]});
        let compact = invoke(request.clone()).unwrap();
        assert!(compact["result"].get("area").is_none());
        assert_eq!(compact["result"]["sectors"][0]["code"], "2401");
        assert_eq!(compact["result"]["geometry_included"], false);
        request["include_geometry"] = json!(true);
        let full = invoke(request.clone()).unwrap();
        assert_eq!(full["result"]["area"]["type"], "MultiPolygon");
        assert_eq!(
            compact["result"]["area_sha256"],
            full["result"]["area_sha256"]
        );
        request["boundaries"][0]["level"] = json!("district");
        assert!(invoke(request).is_err());
    }

    #[test]
    fn missing_wrong_duplicate_and_disconnected_sectors_are_refused() {
        let codes = vec!["2401".into(), "2402".into()];
        for boundaries in [
            vec![boundary("2401", 29.0)],
            vec![boundary("2401", 29.0), boundary("2403", 30.0)],
            vec![boundary("2401", 29.0), boundary("2401", 30.0)],
            vec![boundary("2401", 29.0), boundary("2402", 31.0)],
        ] {
            assert!(prepare_area("project", "map", &codes, boundaries).is_err());
        }
        assert!(prepare_area(
            "project",
            "../map",
            &["2401".into()],
            vec![boundary("2401", 29.0)]
        )
        .is_err());
    }
}
