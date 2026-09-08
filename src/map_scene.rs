//! Portable display scenes. No transport, credentials, renderer or project mutation.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_BYTES: usize = 32 * 1024 * 1024;
pub const REQUEST_SCHEMA: &str = "ds.map.scene.request/v1";
pub const SCENE_SCHEMA: &str = "ds.map.scene/v1";
pub const LAYER_FEATURE_LIMIT: usize = 20_000;
pub const SCENE_FEATURE_LIMIT: usize = 50_000;

/// Both prepared artifacts and the live renderer use this projection. The host
/// supplies a style snapshot and camera zoom; it does not choose active layers.
pub fn render_plan(input: &[u8]) -> Result<Value, String> {
    if input.len() > MAX_BYTES {
        return Err("scene plan input exceeds 32 MiB".into());
    }
    let input: Value = serde_json::from_slice(input).map_err(|_| invalid())?;
    let zoom = input["zoom"]
        .as_f64()
        .filter(|z| z.is_finite())
        .ok_or_else(invalid)?;
    let layers = input["layers"].as_array().ok_or_else(invalid)?;
    if layers.len() > 4096 {
        return Err("scene plan exceeds layer budget".into());
    }
    let mut ids = BTreeSet::new();
    let mut active = Vec::new();
    for layer in layers {
        let id = layer["id"]
            .as_str()
            .filter(|s| name(s))
            .ok_or_else(invalid)?;
        if !ids.insert(id) {
            return Err("scene layer identifiers must be unique".into());
        }
        if layer["layout"]["visibility"] == "none"
            || zoom < layer["minzoom"].as_f64().unwrap_or(0.0)
            || zoom >= layer["maxzoom"].as_f64().unwrap_or(25.0)
        {
            continue;
        }
        active.push(id);
    }
    Ok(
        json!({"active_layer_ids":active,"layer_feature_limit":LAYER_FEATURE_LIMIT,"scene_feature_limit":SCENE_FEATURE_LIMIT}),
    )
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct View {
    pub center: [f64; 2],
    pub zoom: f64,
    pub bearing: f64,
    pub pitch: f64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub name: String,
    pub view: View,
    pub sources: BTreeMap<String, Value>,
    pub layers: Vec<Value>,
}

fn invalid() -> String {
    "scene must have bounded names, a finite geographic view, supported sources and unique layers referencing those sources".into()
}

fn safe_url(raw: &str, pmtiles: bool) -> bool {
    let raw = if pmtiles {
        raw.strip_prefix("pmtiles://").unwrap_or(raw)
    } else {
        raw
    };
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    // Portable files never carry provider keys, signed capabilities or local paths.
    // Runtime provider bindings (Google) use the caller's existing authenticated API.
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
}

fn name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn geometry(value: &Value, positions: &mut usize, depth: usize) -> bool {
    if depth > 16 {
        return false;
    }
    let position = |value: &Value, count: &mut usize| {
        *count += 1;
        value.as_array().is_some_and(|p| {
            (2..=3).contains(&p.len())
                && *count <= 500_000
                && p.iter().all(|n| n.as_f64().is_some_and(f64::is_finite))
                && p[0].as_f64().is_some_and(|n| (-180.0..=180.0).contains(&n))
                && p[1].as_f64().is_some_and(|n| (-90.0..=90.0).contains(&n))
        })
    };
    fn coordinates(
        value: &Value,
        nesting: usize,
        count: &mut usize,
        position: &impl Fn(&Value, &mut usize) -> bool,
    ) -> bool {
        if nesting == 0 {
            return position(value, count);
        }
        value.as_array().is_some_and(|items| {
            !items.is_empty()
                && items
                    .iter()
                    .all(|v| coordinates(v, nesting - 1, count, position))
        })
    }
    match value["type"].as_str().unwrap_or("") {
        "Point" => position(&value["coordinates"], positions),
        "MultiPoint" | "LineString" => coordinates(&value["coordinates"], 1, positions, &position),
        "MultiLineString" | "Polygon" => {
            coordinates(&value["coordinates"], 2, positions, &position)
        }
        "MultiPolygon" => coordinates(&value["coordinates"], 3, positions, &position),
        "GeometryCollection" => value["geometries"]
            .as_array()
            .is_some_and(|items| items.iter().all(|v| geometry(v, positions, depth + 1))),
        _ => false,
    }
}

fn canonicalize(value: Value) -> Value {
    match value {
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(k, v)| (k, canonicalize(v)))
                .collect::<BTreeMap<_, _>>()
                .into_iter()
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.into_iter().map(canonicalize).collect()),
        other => other,
    }
}

pub fn build(input: &[u8]) -> Result<Value, String> {
    if input.is_empty() || input.len() > MAX_BYTES {
        return Err("scene request exceeds 32 MiB".into());
    }
    let request: Request = serde_json::from_slice(input).map_err(|_| invalid())?;
    if request.schema != REQUEST_SCHEMA
        || !name(&request.name)
        || request.sources.len() > 256
        || request.layers.len() > 256
    {
        return Err(invalid());
    }
    let view = &request.view;
    if ![
        view.center[0],
        view.center[1],
        view.zoom,
        view.bearing,
        view.pitch,
    ]
    .iter()
    .all(|v| v.is_finite())
        || !(-180.0..=180.0).contains(&view.center[0])
        || !(-85.0..=85.0).contains(&view.center[1])
        || !(0.0..=24.0).contains(&view.zoom)
        || !(0.0..=85.0).contains(&view.pitch)
        || !(-360.0..=360.0).contains(&view.bearing)
    {
        return Err(invalid());
    }
    let mut feature_count = 0;
    let mut positions = 0;
    for (id, source) in &request.sources {
        if !name(id) {
            return Err(invalid());
        }
        let source = source.as_object().ok_or_else(invalid)?;
        if source.keys().any(|key| {
            ![
                "type",
                "url",
                "tiles",
                "data",
                "minzoom",
                "maxzoom",
                "bounds",
                "attribution",
            ]
            .contains(&key.as_str())
        }) {
            return Err(
                "scene sources cannot contain headers, credentials or undeclared fields".into(),
            );
        }
        match source.get("type").and_then(Value::as_str).unwrap_or("") {
            "geojson" => {
                let data = source.get("data").ok_or_else(invalid)?;
                if data["type"] != "FeatureCollection" {
                    return Err("scene GeoJSON must be an embedded FeatureCollection".into());
                }
                let features = data["features"].as_array().ok_or_else(invalid)?;
                feature_count += features.len();
                if features.len() > LAYER_FEATURE_LIMIT || feature_count > SCENE_FEATURE_LIMIT {
                    return Err("scene exceeds its GeoJSON feature budget".into());
                }
                for feature in features {
                    if feature["type"] != "Feature"
                        || !geometry(&feature["geometry"], &mut positions, 0)
                        || !(feature["properties"].is_object() || feature["properties"].is_null())
                    {
                        return Err(invalid());
                    }
                }
            }
            "vector" => {
                let url = source
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or_else(invalid)?;
                if !url.starts_with("pmtiles://") || !safe_url(url, true) {
                    return Err(
                        "scene PMTiles URL must be HTTPS without credentials or query parameters"
                            .into(),
                    );
                }
            }
            "3d-tiles" => {
                if !source
                    .get("url")
                    .and_then(Value::as_str)
                    .is_some_and(|u| safe_url(u, false))
                {
                    return Err(
                        "scene 3D Tiles URL must be HTTPS without credentials or query parameters"
                            .into(),
                    );
                }
            }
            "google-3d" => {
                if source.len() != 1 {
                    return Err("Google scene bindings contain only their type; credentials resolve at runtime".into());
                }
            }
            "raster" => {
                if let Some(url) = source.get("url").and_then(Value::as_str) {
                    if !url.starts_with("pmtiles://")
                        || !safe_url(url, true)
                        || source.contains_key("tiles")
                    {
                        return Err(invalid());
                    }
                } else {
                    let tiles = source
                        .get("tiles")
                        .and_then(Value::as_array)
                        .ok_or_else(invalid)?;
                    if tiles.is_empty()
                        || tiles.len() > 4
                        || !tiles
                            .iter()
                            .all(|t| t.as_str().is_some_and(|u| safe_url(u, false)))
                    {
                        return Err(invalid());
                    }
                }
            }
            _ => return Err("unsupported scene source type".into()),
        }
        // URLs are forbidden on embedded data and provider bindings too.
        if let Some(raw) = source.get("url") {
            if !raw.as_str().is_some_and(|u| safe_url(u, true)) {
                return Err("scene URLs cannot contain credentials".into());
            }
        }
    }
    let mut ids = BTreeSet::new();
    for layer in &request.layers {
        let fields = layer.as_object().ok_or_else(invalid)?;
        if fields.keys().any(|k| {
            ![
                "id",
                "source",
                "type",
                "source-layer",
                "minzoom",
                "maxzoom",
                "filter",
                "paint",
                "layout",
                "metadata",
            ]
            .contains(&k.as_str())
        }) {
            return Err(invalid());
        }
        let id = layer["id"].as_str().ok_or_else(invalid)?;
        let source_id = layer["source"].as_str().ok_or_else(invalid)?;
        if !name(id) || !ids.insert(id) || !request.sources.contains_key(source_id) {
            return Err(invalid());
        }
        let kind = layer["type"].as_str().ok_or_else(invalid)?;
        if ![
            "circle",
            "line",
            "fill",
            "fill-extrusion",
            "symbol",
            "raster",
            "3d-tiles",
        ]
        .contains(&kind)
        {
            return Err(invalid());
        }
        let source_kind = request.sources[source_id]["type"].as_str().unwrap_or("");
        if match kind {
            "raster" => source_kind != "raster",
            "3d-tiles" => !["3d-tiles", "google-3d"].contains(&source_kind),
            _ => !["vector", "geojson"].contains(&source_kind),
        } {
            return Err(invalid());
        }
        if request.sources[source_id]["type"] == "vector"
            && !layer["source-layer"].as_str().is_some_and(name)
        {
            return Err(invalid());
        }
    }
    let canonical = canonicalize(serde_json::to_value(&request).map_err(|_| invalid())?);
    let digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&canonical).map_err(|_| invalid())?)
    );
    let plan = render_plan(
        &serde_json::to_vec(&json!({"zoom":request.view.zoom,"layers":request.layers}))
            .map_err(|_| invalid())?,
    )?;
    let artifact = json!({ "schema": SCENE_SCHEMA, "sha256": digest, "request": canonical, "render_plan":plan,
        "summary": { "layers": request.layers.len(), "sources": request.sources.len(), "embedded_features": feature_count },
        "precision": "display_only", "network_fetched": false });
    if serde_json::to_vec(&artifact).map_err(|_| invalid())?.len() > MAX_BYTES {
        return Err("scene artifact exceeds 32 MiB".into());
    }
    Ok(artifact)
}

/// Both native CLI artifacts and the browser use the exact same validation.
pub fn validate_artifact(input: &[u8]) -> Result<Value, String> {
    if input.len() > MAX_BYTES {
        return Err("scene artifact exceeds 32 MiB".into());
    }
    let artifact: Value = serde_json::from_slice(input).map_err(|_| invalid())?;
    if artifact["schema"] != SCENE_SCHEMA {
        return Err("unsupported scene artifact schema".into());
    }
    let rebuilt = build(&serde_json::to_vec(&artifact["request"]).map_err(|_| invalid())?)?;
    if rebuilt["sha256"] != artifact["sha256"] {
        return Err("scene artifact digest mismatch".into());
    }
    Ok(rebuilt)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Value {
        json!({"schema":REQUEST_SCHEMA,"name":"streetlights","view":{"center":[30.1,-2.1],"zoom":15,"pitch":50,"bearing":0},
        "sources":{"survey":{"type":"geojson","data":{"type":"FeatureCollection","features":[{"type":"Feature","id":"pole-1","geometry":{"type":"Point","coordinates":[30.1,-2.1,1450]},"properties":{"height":9}}]}}},
        "layers":[{"id":"poles","source":"survey","type":"circle","paint":{"circle-color":"#ffee00","circle-radius":5}}]})
    }
    #[test]
    fn scene_is_headless_deterministic_and_preserves_precision() {
        let input = serde_json::to_vec(&request()).unwrap();
        let scene = build(&input).unwrap();
        assert_eq!(scene, build(&input).unwrap());
        assert_eq!(scene["network_fetched"], false);
        assert_eq!(
            scene["request"]["sources"]["survey"]["data"]["features"][0]["geometry"]["coordinates"]
                [2],
            1450
        );
        assert_eq!(
            validate_artifact(&serde_json::to_vec(&scene).unwrap()).unwrap(),
            scene
        );
    }
    #[test]
    fn rejects_tampering_and_missing_sources() {
        let mut scene = build(&serde_json::to_vec(&request()).unwrap()).unwrap();
        scene["request"]["view"]["zoom"] = json!(9);
        assert!(validate_artifact(&serde_json::to_vec(&scene).unwrap()).is_err());
        let mut req = request();
        req["layers"][0]["source"] = json!("missing");
        assert!(build(&serde_json::to_vec(&req).unwrap()).is_err());
    }
    #[test]
    fn provider_credentials_never_enter_portable_scenes() {
        for raw in [
            "https://example.test/root.json?key=private",
            "file:///etc/passwd",
            "https://user:pass@example.test/root.json",
        ] {
            let mut req = request();
            req["sources"]["tiles"] = json!({"type":"3d-tiles","url":raw});
            assert!(build(&serde_json::to_vec(&req).unwrap()).is_err());
        }
        let mut req = request();
        req["sources"]["google"] = json!({"type":"google-3d"});
        assert!(build(&serde_json::to_vec(&req).unwrap()).is_ok());
    }
    #[test]
    fn rejects_bad_geometry_and_source_layer_mismatch() {
        let mut req = request();
        req["layers"][0]["type"] = json!("3d-tiles");
        assert!(build(&serde_json::to_vec(&req).unwrap()).is_err());
        let mut req = request();
        req["sources"]["survey"]["data"]["features"][0]["geometry"]["coordinates"] =
            json!([800, 0]);
        assert!(build(&serde_json::to_vec(&req).unwrap()).is_err());
    }
    #[test]
    fn object_key_order_does_not_change_the_digest() {
        let req = request();
        let compact = serde_json::to_vec(&req).unwrap();
        let pretty = serde_json::to_vec_pretty(&req).unwrap();
        assert_eq!(
            build(&compact).unwrap()["sha256"],
            build(&pretty).unwrap()["sha256"]
        );
        assert_eq!(
            canonicalize(json!({"z":{"b":1,"a":2},"a":0})),
            canonicalize(json!({"a":0,"z":{"a":2,"b":1}}))
        );
    }
    #[test]
    fn live_and_portable_scenes_share_order_visibility_and_budgets() {
        let mut req = request();
        req["layers"] = json!([
            {"id":"first","type":"circle","source":"survey"},
            {"id":"hidden","type":"circle","source":"survey","layout":{"visibility":"none"}},
            {"id":"too-close","type":"circle","source":"survey","minzoom":17},
            {"id":"last","type":"circle","source":"survey","maxzoom":16}
        ]);
        let scene = build(&serde_json::to_vec(&req).unwrap()).unwrap();
        let plan =
            render_plan(&serde_json::to_vec(&json!({"zoom":15,"layers":req["layers"]})).unwrap())
                .unwrap();
        assert_eq!(plan, scene["render_plan"]);
        assert_eq!(plan["active_layer_ids"], json!(["first", "last"]));
    }
}
