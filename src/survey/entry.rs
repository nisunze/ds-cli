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
