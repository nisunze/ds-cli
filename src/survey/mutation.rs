//! Capture and mutation intent. The host supplies clocks, grants and prior rows;
//! the kernel owns validation, canonical deltas and optimistic projections.
use serde_json::{json, Map, Value};

fn object(value: &Value) -> Map<String, Value> {
    value.as_object().cloned().unwrap_or_default()
}
fn props(value: &Value) -> &Value {
    value.get("properties").unwrap_or(value)
}
fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("").trim()
}
fn email(value: &Value) -> String {
    text(value).to_lowercase()
}
fn cap(r: &Value, name: &str) -> bool {
    r["grants"][name] == true
}
fn first<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    keys.iter()
        .find_map(|key| value.get(*key).filter(|v| !v.is_null()))
}
fn identity(payload: &Value) -> Result<Value, String> {
    let project = text(first(payload, &["project_id", "eds_project_id"]).unwrap_or(&Value::Null));
    let form = text(first(payload, &["form_id", "form_slug"]).unwrap_or(&Value::Null));
    let doc = first(payload, &["doc_id", "entry_id"])
        .or_else(|| first(&payload["data"], &["id", "doc_id", "feature_id"]))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let op = text(&payload["operation"]).to_lowercase();
    let op = if op.is_empty() { "update" } else { &op };
    if !["update", "delete", "restore"].contains(&op) {
        return Err("Unsupported Survey mutation operation".into());
    }
    crate::survey_entry_create::validate_project(project).map_err(|e| e.to_string())?;
    if form.is_empty()
        || form.len() > 256
        || doc.is_empty()
        || doc.len() > 1500
        || doc == "unknown-entry"
        || [form, doc].iter().any(|v| v.chars().any(char::is_control))
    {
        return Err("Entry mutation is missing its shared document identity".into());
    }
    Ok(json!({"project_id":project,"form_id":form,"doc_id":doc,"operation":op}))
}
fn created_by(feature: &Value) -> String {
    let p = props(feature);
    email(
        first(&p["metadata"], &["created_by", "createdBy"])
            .or_else(|| first(p, &["created_by", "createdBy", "user_email", "userEmail"]))
            .unwrap_or(&Value::Null),
    )
}
fn authorize(r: &Value, operation: &str) -> Result<(), String> {
    match operation {
        "create" if cap(r, "entries.create") => Ok(()),
        "delete" if cap(r, "entries.delete") => Ok(()),
        "update" | "restore"
            if cap(r, "entries.edit_all")
                || (cap(r, "entries.edit_own")
                    && !created_by(&r["prior"]).is_empty()
                    && created_by(&r["prior"]) == email(&r["user_email"])) =>
        {
            Ok(())
        }
        "create" => Err("Missing capability: entries.create".into()),
        "delete" => Err("Missing capability: entries.delete".into()),
        _ => Err("Missing capability: entries.edit_all or entries.edit_own".into()),
    }
}
fn normalize(r: &Value) -> Result<Value, String> {
    let row = &r["row"];
    let source = &row["payload"];
    if !source.is_object() {
        return Err("Survey mutation payload must be an object".into());
    }
    let mut candidate = object(source);
    for key in ["project_id", "form_id", "doc_id", "operation"] {
        let aliases: &[&str] = match key {
            "project_id" => &["project_id", "eds_project_id"],
            "form_id" => &["form_id", "form_slug"],
            "doc_id" => &["doc_id", "entry_id"],
            _ => &["operation"],
        };
        if first(source, aliases).is_none() {
            candidate.insert(key.into(), row[key].clone());
        }
    }
    let id = identity(&Value::Object(candidate))?;
    if ["project_id", "form_id", "doc_id", "operation"]
        .iter()
        .any(|key| id[*key] != row[*key])
    {
        return Err("Survey mutation payload does not match its durable identity.".into());
    }
    let operation = text(&id["operation"]);
    let mut output = object(&id);
    for (target, legacy) in [
        ("updated_data", "data"),
        ("updated_geometry", "geometry"),
        ("updated_connectivity", "connectivity"),
        ("updated_detailed_location", "detailed_location"),
    ] {
        let value = source.get(target).filter(|v| !v.is_null()).or_else(|| {
            if operation == "update" {
                source.get(legacy).filter(|v| !v.is_null())
            } else {
                None
            }
        });
        if let Some(value) = value {
            if target == "updated_geometry" {
                crate::survey_entry_create::evaluate_geometry(
                    &serde_json::to_vec(
                        &json!({"schema":"ds.survey-entry-geometry/v1","geometry":value}),
                    )
                    .map_err(|e| e.to_string())?,
                )?;
            } else if !value.is_object() {
                return Err(format!("{target} must be an object"));
            }
            output.insert(target.into(), value.clone());
        }
    }
    for key in [
        "context_key",
        "metadata",
        "expected_version",
        "queued_at",
        "is_deleted",
    ] {
        if let Some(value) = source.get(key) {
            output.insert(key.into(), value.clone());
        }
    }
    let key = source
        .get("idempotency_key")
        .unwrap_or(&row["idempotency_key"]);
    if key != &row["idempotency_key"] || text(key).is_empty() || text(key).len() > 512 {
        return Err("Survey mutation payload does not match its durable identity.".into());
    }
    output.insert("idempotency_key".into(), key.clone());
    if let Some(v) = output.get("expected_version") {
        if !v.as_u64().is_some_and(|v| v <= 9_007_199_254_740_991) {
            return Err("Expected version must be a nonnegative safe integer".into());
        }
    }
    if output.get("metadata").is_some_and(|v| !v.is_object()) {
        return Err("Metadata must be an object".into());
    }
    let result = Value::Object(output);
    if serde_json::to_vec(&result)
        .map_err(|e| e.to_string())?
        .len()
        > crate::survey_entry_create::MAX_PAYLOAD_BYTES
    {
        return Err("Survey mutation exceeds its byte bound".into());
    }
    Ok(result)
}
fn optimistic(r: &Value) -> Result<Value, String> {
    let mut properties = Map::new();
    properties.insert("doc_id".into(), r["doc_id"].clone());
    properties.insert("form_slug".into(), r["form_slug"].clone());
    for key in ["data", "metadata", "connectivity", "detailed_location"] {
        if r["document"][key].is_object() {
            properties.insert(key.into(), r["document"][key].clone());
        }
    }
    Ok(
        json!({"type":"Feature","id":r["doc_id"],"geometry":super::entry::geometry(r)?,"properties":properties}),
    )
}
fn patch(r: &Value) -> Result<Value, String> {
    if r["prior"].is_null() && r["deleted"] != true {
        return Ok(Value::Null);
    }
    let mut feature = if r["prior"].is_null() {
        json!({"type":"Feature","properties":{"id":r["doc_id"]},"geometry":null})
    } else {
        r["prior"].clone()
    };
    let p = if feature.get("properties").is_some() {
        &mut feature["properties"]
    } else {
        &mut feature
    };
    if !p.is_object() {
        return Err("Survey feature properties must be an object".into());
    }
    for (key, input) in [("data", "updatedData"), ("metadata", "updatedMetadata")] {
        if let Some(delta) = r["patch"][input].as_object().filter(|v| !v.is_empty()) {
            let mut merged = object(&p[key]);
            merged.extend(delta.clone());
            p[key] = Value::Object(merged);
        }
    }
    if let Some(value) = r["patch"].get("updatedConnectivity") {
        if value.is_null() {
            p.as_object_mut().unwrap().remove("connectivity");
        } else {
            p["connectivity"] = value.clone();
        }
    }
    if r["deleted"] == true {
        if p["metadata"].is_object() {
            p["metadata"]["is_deleted"] = json!(true);
        } else {
            p["is_deleted"] = json!(true);
        }
        if p["id"].is_null() {
            p["id"] = r["doc_id"].clone();
        }
    }
    Ok(feature)
}
fn capture(r: &Value) -> Result<Value, String> {
    authorize(r, "create")?;
    let p = &r["params"];
    let conn = if !p["connectivity"].is_null() {
        p["connectivity"].clone()
    } else if !text(&p["precedentDocId"]).is_empty() {
        json!({"precedentDocId":p["precedentDocId"]})
    } else {
        Value::Null
    };
    super::policy::evaluate(
        &json!({"mode":"direct_edge","schema":p["entryDocumentSchema"],"connectivity":conn}),
    )?;
    let document = super::entry::encode(
        &json!({"schema":p["entryDocumentSchema"],"userEmail":r["user_email"],"now":r["now"],
        "data":p["data"],"geometry":p["geometry"],"location":p["location"],"connectivity":conn,"detailedLocation":p["detailedLocation"]}),
    )?;
    let id = &r["identity"];
    let mut payload = json!({"project_id":id["project_id"],"form_id":id["form_id"],"doc_id":id["doc_id"],"operation":"create",
        "idempotency_key":r["mutation_id"],"data":document["data"],"metadata":{"created_at":r["now"],"origin":"web"}});
    for key in ["geometry", "connectivity", "detailed_location"] {
        if let Some(v) = document.get(key) {
            payload[key] = v.clone();
        }
    }
    payload["schema"] = json!("ds.survey-entry-create/v1");
    payload["operation"] = json!("plan");
    let plan: Value = serde_json::from_str(&crate::survey_entry_create::evaluate(
        &serde_json::to_vec(&payload).map_err(|e| e.to_string())?,
    )?)
    .map_err(|e| e.to_string())?;
    let feature = optimistic(
        &json!({"doc_id":id["doc_id"],"form_slug":id["form_id"],"document":document,"geometry":p["geometry"],"location":p["location"]}),
    )?;
    Ok(json!({"payload":plan["payload"],"feature":feature}))
}
fn mutate(r: &Value) -> Result<Value, String> {
    let id = identity(&r["payload"])?;
    let op = text(&id["operation"]);
    authorize(r, op)?;
    let mut payload = r["payload"].clone();
    for key in ["project_id", "form_id", "doc_id", "operation"] {
        payload[key] = id[key].clone();
    }
    if payload["idempotency_key"].is_null() {
        payload["idempotency_key"] = r["mutation_id"].clone();
    }
    if payload["expected_version"].is_null() {
        payload["expected_version"] = json!(props(&r["prior"])["metadata"]["client_version"]
            .as_u64()
            .unwrap_or(0));
    }
    let mut meta = object(&payload["metadata"]);
    let clock = if op == "delete" {
        "deleted_at"
    } else {
        "edit_at"
    };
    if !meta.contains_key(clock) {
        meta.insert(clock.into(), r["now"].clone());
    }
    meta.insert("origin".into(), json!("web"));
    payload["metadata"] = Value::Object(meta);
    if op == "delete" {
        payload["is_deleted"] = json!(true);
    }
    let mut row = id.clone();
    row["idempotency_key"] = payload["idempotency_key"].clone();
    row["payload"] = payload;
    let payload = normalize(&json!({"row":row}))?;
    let mut delta = json!({"updatedData":payload["updated_data"]});
    if !payload["updated_connectivity"].is_null() {
        delta["updatedConnectivity"] = payload["updated_connectivity"].clone();
    }
    if op == "restore" {
        delta["updatedMetadata"] = json!({"is_deleted":false});
    }
    let mut feature = patch(
        &json!({"prior":r["prior"],"doc_id":id["doc_id"],"patch":delta,"deleted":op=="delete"}),
    )?;
    if !feature.is_null() {
        let p = if feature.get("properties").is_some() {
            &mut feature["properties"]
        } else {
            &mut feature
        };
        if op == "restore" {
            p.as_object_mut()
                .ok_or("Survey feature properties must be an object")?
                .remove("is_deleted");
        }
        if !payload["updated_detailed_location"].is_null() {
            p["detailed_location"] = payload["updated_detailed_location"].clone();
        }
    }
    if !feature.is_null() && !payload["updated_geometry"].is_null() {
        feature["geometry"] = payload["updated_geometry"].clone();
    }
    Ok(json!({"identity":id,"payload":payload,"feature":feature}))
}
pub(super) fn evaluate(r: &Value) -> Result<Value, String> {
    match r["mode"].as_str() {
        Some("identity") => identity(&r["payload"]),
        Some("normalize") => normalize(r),
        Some("capture") => capture(r),
        Some("mutate") => mutate(r),
        Some("patch") => patch(r),
        Some("authorize") => {
            authorize(r, text(&r["mutation_operation"]))?;
            Ok(Value::Null)
        }
        _ => Err("Unknown Survey mutation command".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalized_identity_is_fenced_and_geometry_is_strict() {
        let row = json!({"project_id":"p","form_id":"form","doc_id":"d","operation":"update","idempotency_key":"key","payload":{"updated_data":{"x":1}}});
        assert_eq!(
            normalize(&json!({"row":row})).unwrap()["updated_data"],
            json!({"x":1})
        );
        let mut bad = row.clone();
        bad["payload"]["project_id"] = json!("other");
        assert!(normalize(&json!({"row":bad})).is_err());
        let mut bad = row;
        bad["payload"]["updated_geometry"] = json!("POINT (1 2)");
        assert!(normalize(&json!({"row":bad})).is_err());
    }
    #[test]
    fn capture_checks_grants_before_producing_a_durable_intent() {
        assert!(capture(&json!({"grants":{}}))
            .unwrap_err()
            .contains("entries.create"));
    }
    #[test]
    fn capture_composes_the_existing_create_wire_contract() {
        let r = json!({"params":{"data":{"name":"Pole"},"entryDocumentSchema":{"top_level_keys":[{"key":"data","required":false},{"key":"metadata","required":false},{"key":"geometry","required":false}]}},"identity":{"project_id":"p","form_id":"form","doc_id":"d"},"grants":{"entries.create":true},"user_email":"field@example.com","mutation_id":"key","now":"2026-09-11T00:00:00Z"});
        let result = capture(&r).unwrap();
        assert_eq!(result["payload"]["operation"], "create");
        assert_eq!(result["payload"]["data"]["name"], "Pole");
        assert_eq!(result["feature"]["properties"]["doc_id"], "d");
    }
    #[test]
    fn the_optimistic_feature_nests_data_and_keeps_line_geometry_and_connectivity() {
        let line = json!({"type":"LineString","coordinates":[[29.87,-1.94],[29.88,-1.95]]});
        let r = json!({"params":{"data":{"name":"Test","count":10},"geometry":line,"precedentDocId":"node-123",
            "entryDocumentSchema":{"top_level_keys":[{"key":"data"},{"key":"metadata"},{"key":"geometry"},{"key":"connectivity"}]}},
            "identity":{"project_id":"p","form_id":"test_form","doc_id":"d"},"grants":{"entries.create":true},
            "user_email":"test@example.com","mutation_id":"key","now":"2026-04-17T12:00:00.000Z"});
        let feature = &capture(&r).unwrap()["feature"];
        let p = &feature["properties"];
        assert_eq!(feature["id"], "d");
        assert_eq!(feature["geometry"], line);
        assert_eq!(p["form_slug"], "test_form");
        assert_eq!(p["data"], json!({"name":"Test","count":10}));
        assert_eq!(p["connectivity"]["precedent"], "node-123");
        assert_eq!(p["metadata"]["created_by"], "test@example.com");
        assert_eq!(p["metadata"]["created_at"], "2026-04-17T12:00:00.000Z");
        for legacy in ["id", "table_name", "name", "count"] {
            assert!(p.get(legacy).is_none(), "{legacy} leaked into properties");
        }
    }
    #[test]
    fn restore_clears_legacy_tombstones_and_retains_location_delta() {
        let r = json!({"payload":{"project_id":"p","form_id":"form","doc_id":"d","operation":"restore","updated_detailed_location":{"accuracy":3}},"prior":{"type":"Feature","properties":{"is_deleted":true,"metadata":{"is_deleted":true}}},"grants":{"entries.edit_all":true},"now":"2026-09-11T00:00:00Z","mutation_id":"key"});
        let result = mutate(&r).unwrap();
        assert!(result["feature"]["properties"]["is_deleted"].is_null());
        assert_eq!(
            result["feature"]["properties"]["metadata"]["is_deleted"],
            false
        );
        assert_eq!(
            result["feature"]["properties"]["detailed_location"]["accuracy"],
            3
        );
    }
    #[test]
    fn update_keeps_identity_and_local_nested_values_and_refuses_create() {
        let r = json!({"payload":{"project_id":"p","form_id":"form","doc_id":"d","operation":"update","updated_data":{"x":2}},"prior":{"type":"Feature","properties":{"data":{"x":1,"y":3},"metadata":{"client_version":4}}},"grants":{"entries.edit_all":true},"now":"2026-09-11T00:00:00Z","mutation_id":"key"});
        let result = mutate(&r).unwrap();
        assert_eq!(result["payload"]["expected_version"], 4);
        assert_eq!(
            result["feature"]["properties"]["data"],
            json!({"x":2,"y":3})
        );
        let mut bad = r;
        bad["payload"]["operation"] = json!("create");
        assert!(mutate(&bad).is_err());
    }
}
