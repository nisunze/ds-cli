//! Small survey decisions shared by browser and native collection hosts.
use serde_json::{json, Value};
fn schema(r: &Value) -> &Value {
    &r["form"]["entry_document_schema"]
}
fn bool_setting(r: &Value, key: &str) -> bool {
    schema(r)[key]
        .as_bool()
        .or_else(|| r["form"]["project_settings"][key].as_bool())
        .unwrap_or(false)
}
fn max_distance(r: &Value) -> Option<f64> {
    if !bool_setting(r, "restrict_max_pick_distance") {
        return None;
    }
    schema(r)["max_pick_distance_m"]
        .as_f64()
        .or_else(|| r["form"]["project_settings"]["max_pick_distance_meters"].as_f64())
        .filter(|v| *v > 0.)
}
pub(super) fn evaluate(r: &Value) -> Result<Value, String> {
    Ok(match r["mode"].as_str() {
        Some("accuracy") => {
            let i = &r["input"];
            json!(
                i["enforceAccuracy"] != true
                    || i["locationPicked"] == true
                    || i["requiredAccuracyM"].as_f64().unwrap_or(0.) == 0.
                    || i["hasGeometryWithoutLocation"] == true
                    || i["accuracyM"]
                        .as_f64()
                        .zip(i["requiredAccuracyM"].as_f64())
                        .is_some_and(|(a, b)| a >= 0. && a <= b)
            )
        }
        Some("allows_pick") => json!(bool_setting(r, "allow_location_picking")),
        Some("pick_reason") => {
            if r["form"].is_null() {
                json!("Form schema is not available offline.")
            } else if !bool_setting(r, "allow_location_picking") {
                json!("Map-picked locations are disabled for this form.")
            } else {
                Value::Null
            }
        }
        Some("max_pick_distance") => json!(max_distance(r)),
        Some("distance_reason") => match max_distance(r).zip(r["distanceM"].as_f64()) {
            Some((max, distance)) if distance > max => json!(format!(
                "Picked point is {} m from your location (max {} m). Move closer or capture your current GPS position.",
                distance.round(),
                max.round()
            )),
            _ => Value::Null,
        },
        Some("derived_edge") => json!(
            schema(r)["connectivity"]["is_network_element"] == true
                && schema(r)["connectivity"]["network_element_type"] == "edge"
        ),
        Some("precedent") => {
            let c = &r["connectivity"];
            let show = r["isChainedEdge"] != true
                && c["is_network_element"] == true
                && c["network_element_type"] == "node"
                && c["is_source_node"] != true;
            json!({"showPicker":show,"required":show && c["require_precedent"]==true})
        }
        Some("direct_edge") => {
            if r["schema"]["connectivity"]["is_network_element"] == true
                && r["schema"]["connectivity"]["network_element_type"] == "edge"
                && !(r["connectivity"]["precedentDocId"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty())
                    && r["connectivity"]["antecedentDocId"]
                        .as_str()
                        .is_some_and(|s| !s.is_empty()))
            {
                return Err("Edge entries are created from connected node workflows; select a node precedent instead.".into());
            }
            Value::Null
        }
        Some("offer_edge") => json!(
            !r["precedent"].is_null()
                && !r["coordinates"].is_null()
                && r["docId"].as_str().is_some_and(|s| !s.is_empty())
                && r["edgeForms"].as_array().is_some_and(|a| !a.is_empty())
        ),
        Some("coordinates") => {
            if r["geometry"]["type"] == "Point" {
                r["geometry"]["coordinates"].clone()
            } else if !r["location"].is_null() {
                json!([r["location"]["longitude"], r["location"]["latitude"]])
            } else {
                Value::Null
            }
        }
        _ => return Err("Unknown survey policy".into()),
    })
}
