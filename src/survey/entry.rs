use serde_json::{json, Map, Value};

pub(super) fn point(value: &Value) -> Result<(f64, f64), String> {
    let coordinates = value
        .as_array()
        .ok_or("Point coordinates must be an array")?;
    let x = coordinates
        .first()
        .and_then(Value::as_f64)
        .ok_or("Longitude is required")?;
    let y = coordinates
        .get(1)
        .and_then(Value::as_f64)
        .ok_or("Latitude is required")?;
    if !(-180.0..=180.0).contains(&x) || !(-90.0..=90.0).contains(&y) {
        return Err("Coordinates must be WGS84 longitude/latitude".into());
    }
    Ok((x, y))
}
pub(super) fn geometry(request: &Value) -> Result<Value, String> {
    if !request["geometry"].is_null() {
        let geometry = &request["geometry"];
        if !geometry.is_object() || geometry["type"].as_str().is_none() {
            return Err("Survey geometry must be GeoJSON".into());
        }
        if geometry["type"] == "Point" {
            point(&geometry["coordinates"])?;
        }
        return Ok(geometry.clone());
    }
    if request["location"].is_null() {
        return Ok(Value::Null);
    }
    let coordinates = json!([
        request["location"]["longitude"],
        request["location"]["latitude"]
    ]);
    point(&coordinates)?;
    Ok(json!({"type":"Point","coordinates":coordinates}))
}
pub(super) fn length(geometry: &Value) -> Result<f64, String> {
    let coordinates = geometry["coordinates"]
        .as_array()
        .filter(|_| geometry["type"] == "LineString")
        .ok_or("Line length requires a LineString")?;
    if coordinates.len() > 10_000 {
        return Err("Survey line exceeds 10000 vertices".into());
    }
    let mut total = 0.0;
    for pair in coordinates.windows(2) {
        let a = point(&pair[0])?;
        let b = point(&pair[1])?;
        total += ds_geo_lite::haversine_m(a.0, a.1, b.0, b.1);
    }
    Ok(total)
}
pub(super) fn encode(request: &Value) -> Result<Value, String> {
    let schema = &request["schema"];
    let keys = schema["top_level_keys"]
        .as_array()
        .ok_or("Resolved entry schema is required offline")?;
    let allows = |key: &str| keys.iter().any(|k| k["key"] == key);
    let mut doc = Map::new();
    if allows("data") {
        if !request["data"].is_object() {
            return Err("Survey data must be an object".into());
        }
        doc.insert("data".into(), request["data"].clone());
    }
    if allows("metadata") {
        let now = request["now"]
            .as_str()
            .ok_or("Device creation time is required")?;
        let email = request["userEmail"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown")
            .to_lowercase();
        doc.insert(
            "metadata".into(),
            json!({"created_by":email,"created_at":now,"is_deleted":false}),
        );
    }
    let geom = geometry(request)?;
    if allows("geometry") && !geom.is_null() {
        doc.insert("geometry".into(), geom.clone());
    }
    if allows("connectivity") && !request["connectivity"].is_null() {
        let input = &request["connectivity"];
        let mut output = Map::new();
        for (from, to) in [
            ("precedentDocId", "precedent"),
            ("precedentFormId", "precedent_form_id"),
            ("antecedentDocId", "antecedent"),
            ("antecedentFormId", "antecedent_form_id"),
        ] {
            if let Some(s) = input[from].as_str().filter(|s| !s.is_empty()) {
                output.insert(to.into(), json!(s));
            }
        }
        if geom["type"] == "LineString" {
            output.insert(
                "length_m".into(),
                json!(match input["lengthM"].as_f64() {
                    Some(n) => n,
                    None => length(&geom)?,
                }),
            );
        } else if geom["type"] == "Point" {
            if let Some(n) = input["distanceM"].as_f64() {
                output.insert("distance_m".into(), json!(n));
            } else if !input["precedentCoordinates"].is_null() {
                let a = point(&input["precedentCoordinates"])?;
                let b = point(&geom["coordinates"])?;
                output.insert(
                    "distance_m".into(),
                    json!(ds_geo_lite::haversine_m(a.0, a.1, b.0, b.1)),
                );
            }
        }
        if !output.is_empty() {
            doc.insert("connectivity".into(), Value::Object(output));
        }
    }
    if allows("detailed_location") {
        let mut output = Map::new();
        for key in [
            "device_lat",
            "device_lng",
            "accuracy_m",
            "altitude_m",
            "speed_mps",
        ] {
            if let Some(n) = request["detailedLocation"][key].as_f64() {
                output.insert(key.into(), json!(n));
            }
        }
        if !output.is_empty() {
            doc.insert("detailed_location".into(), Value::Object(output));
        }
    }
    Ok(Value::Object(doc))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn schema(keys: &[&str]) -> Value {
        json!({"top_level_keys": keys.iter().map(|k| json!({"key":k})).collect::<Vec<_>>()})
    }
    fn node() -> Value {
        schema(&[
            "data",
            "metadata",
            "geometry",
            "connectivity",
            "detailed_location",
        ])
    }
    fn doc(request: Value) -> Value {
        let mut r = json!({"schema":node(),"userEmail":"A@B.c","now":"2026-09-23T00:00:00Z","data":{"name":"Test"}});
        r.as_object_mut()
            .unwrap()
            .extend(request.as_object().unwrap().clone());
        encode(&r).unwrap()
    }
    const LINE: &str = r#"{"type":"LineString","coordinates":[[29.87,-1.94],[29.88,-1.95]]}"#;
    #[test]
    fn only_canonical_top_level_keys_and_metadata_are_emitted() {
        let d = doc(json!({"location":{"latitude":-1.94,"longitude":29.87}}));
        let mut keys = d.as_object().unwrap().keys().cloned().collect::<Vec<_>>();
        keys.sort();
        assert_eq!(keys, ["data", "geometry", "metadata"]);
        assert_eq!(
            d["metadata"],
            json!({"created_by":"a@b.c","created_at":"2026-09-23T00:00:00Z","is_deleted":false})
        );
        assert_eq!(d["geometry"]["type"], "Point");
    }
    #[test]
    fn a_node_carries_distance_and_an_edge_carries_only_its_own_length() {
        let n = doc(
            json!({"location":{"latitude":-1.94,"longitude":29.87},"connectivity":{"precedentDocId":"p1","precedentFormId":"lv_poles","precedentCoordinates":[29.86,-1.93]}}),
        );
        assert_eq!(n["connectivity"]["precedent"], "p1");
        assert_eq!(n["connectivity"]["precedent_form_id"], "lv_poles");
        assert!(n["connectivity"]["distance_m"].as_f64().unwrap() > 0.);
        assert!(n["connectivity"].get("length_m").is_none());
        let line: Value = serde_json::from_str(LINE).unwrap();
        let e = doc(
            json!({"geometry":line,"connectivity":{"precedentDocId":"node-a","antecedentDocId":"node-b","antecedentFormId":"transformers","precedentCoordinates":[29.87,-1.94],"distanceM":42}}),
        );
        assert_eq!(e["geometry"], line);
        assert_eq!(e["connectivity"]["antecedent"], "node-b");
        assert!(e["connectivity"]["length_m"].as_f64().unwrap() > 100.);
        assert!(e["connectivity"].get("distance_m").is_none());
        let a = point(&json!([29.87, -1.94])).unwrap();
        let b = point(&json!([29.88, -1.95])).unwrap();
        let direct = ds_geo_lite::haversine_m(a.0, a.1, b.0, b.1);
        assert!((direct - length(&line).unwrap()).abs() < 0.001);
    }
    #[test]
    fn detailed_location_is_flat_and_a_leaf_form_drops_connectivity() {
        let d = doc(
            json!({"location":{"latitude":-1.941,"longitude":29.871},"detailedLocation":{"device_lat":-1.941,"device_lng":29.871,"accuracy_m":4.2,"altitude_m":1463.5,"speed_mps":1.7}}),
        );
        assert_eq!(
            d["detailed_location"],
            json!({"device_lat":-1.941,"device_lng":29.871,"accuracy_m":4.2,"altitude_m":1463.5,"speed_mps":1.7})
        );
        let leaf = doc(
            json!({"schema":schema(&["data","metadata","geometry"]),"location":{"latitude":-1.94,"longitude":29.87},"connectivity":{"precedentDocId":"p1"}}),
        );
        assert!(leaf.get("connectivity").is_none());
    }
}
