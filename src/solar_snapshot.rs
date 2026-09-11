//! Deterministic Solar input snapshot admission and canonicalisation.
//!
//! Hosts acquire documents and persist accepted bytes. This module owns what
//! those documents mean as a calculator snapshot: canonical JSON, fingerprints,
//! provenance and whether a cached answer may be reused.
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};

const MAX_BYTES: usize = 8 << 20;
const MAX_DOCUMENTS: usize = 512;

fn digest(bytes: &[u8]) -> String { format!("{:x}", Sha256::digest(bytes)) }
fn hex(value: &str) -> bool { value.len() == 64 && value.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) }
fn text(value: &Value) -> Option<&str> { value.as_str().filter(|v| !v.is_empty()) }
fn object(value: &Value) -> Result<&Map<String, Value>, String> { value.as_object().ok_or_else(|| "Solar snapshot requires a JSON object".into()) }

/// The exact canonical JSON authority already used by ds-solar contracts.
pub fn canonical(value: &Value) -> Result<String, String> {
    String::from_utf8(ds_solar_contracts::digest::canonical_json_bytes(value)
        .map_err(|_| "Solar snapshot cannot be encoded")?).map_err(|_| "Solar snapshot cannot be encoded".into())
}

fn build(root: &str, city: &str, base: &Value, source_kind: &str) -> Result<Value, String> {
    let base = object(base)?;
    for key in ["_root", "_config", "00_templates", "01_city_inputs"] {
        if !base.get(key).is_some_and(Value::is_object) { return Err(format!("Solar input is missing {key}")); }
    }
    let paths = base.get("_config").and_then(Value::as_object).and_then(|c| c.get("paths")).and_then(Value::as_object);
    if paths.is_none_or(Map::is_empty) { return Err("Solar input path manifest is missing".into()); }
    let base_value = Value::Object(base.clone());
    let fingerprint = digest(canonical(&base_value)?.as_bytes());
    let seed = ds_solar_contracts::digest::demand_noise_seed(city).map_err(|_| "Solar seed failed")?;
    let mut snapshot = base.clone();
    snapshot.insert("_provenance".into(), json!({"schema":3,"source_kind":source_kind,"root":root,"context":city,"demand_noise_seed":seed,"input_base_fingerprint":fingerprint}));
    let snapshot_json = canonical(&Value::Object(snapshot))?;
    let bytes = snapshot_json.len();
    if bytes == 0 || bytes > MAX_BYTES { return Err("Solar snapshot exceeds its byte limit".into()); }
    let documents = 1 + base["00_templates"].as_object().map_or(0, Map::len) + base["01_city_inputs"].as_object().map_or(0, Map::len) + base["_config"].as_object().map_or(0, Map::len);
    if documents > MAX_DOCUMENTS { return Err("Solar snapshot exceeds its document limit".into()); }
    Ok(json!({"snapshot_json":snapshot_json,"snapshot_sha256":digest(snapshot_json.as_bytes()),"input_base_fingerprint":fingerprint,"snapshot_bytes":bytes,"source_document_count":documents}))
}

fn validate(root: &str, city: &str, snapshot: &Value, pending_unsynced: bool) -> Result<Value, String> {
    if pending_unsynced { return Ok(json!({"eligible":false,"reason":"pending_unsynced"})); }
    let input = object(snapshot)?;
    let encoded = text(input.get("snapshot_json").unwrap_or(&Value::Null)).ok_or("Solar snapshot body is missing")?;
    let snapshot_hash = text(input.get("snapshot_sha256").unwrap_or(&Value::Null)).unwrap_or("");
    if encoded.len() > MAX_BYTES || !hex(snapshot_hash) || digest(encoded.as_bytes()) != snapshot_hash { return Err("Solar snapshot digest is invalid".into()); }
    let value: Value = serde_json::from_str(encoded).map_err(|_| "Solar snapshot is not JSON")?;
    let body = object(&value)?;
    let provenance = body.get("_provenance").and_then(Value::as_object).ok_or("Solar snapshot provenance is missing")?;
    if provenance.get("schema").and_then(Value::as_u64) != Some(3) || provenance.get("root").and_then(Value::as_str) != Some(root) || provenance.get("context").and_then(Value::as_str) != Some(city) || provenance.get("input_base_fingerprint").and_then(Value::as_str) != text(input.get("input_base_fingerprint").unwrap_or(&Value::Null)) { return Err("Solar snapshot provenance does not match this city".into()); }
    let base = json!({"_root":body.get("_root").ok_or("Solar root missing")?,"_config":body.get("_config").ok_or("Solar config missing")?,"00_templates":body.get("00_templates").ok_or("Solar templates missing")?,"01_city_inputs":body.get("01_city_inputs").ok_or("Solar city inputs missing")?});
    let expected = digest(canonical(&base)?.as_bytes());
    if expected != text(input.get("input_base_fingerprint").unwrap_or(&Value::Null)).unwrap_or("") { return Err("Solar snapshot fingerprint is invalid".into()); }
    Ok(json!({"eligible":true,"snapshot":snapshot}))
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    let request: Value = serde_json::from_slice(input).map_err(|_| "Solar snapshot request is not JSON")?;
    let object = object(&request)?;
    let root = text(object.get("root").unwrap_or(&Value::Null)).ok_or("Solar root is required")?;
    let city = text(object.get("city_id").unwrap_or(&Value::Null)).ok_or("Solar city is required")?;
    let output = match text(object.get("operation").unwrap_or(&Value::Null)) {
        Some("build") => build(root, city, object.get("base").ok_or("Solar base is required")?, "local solar input workspace")?,
        Some("validate") => validate(root, city, object.get("snapshot").ok_or("Solar snapshot is required")?, object.get("pending_unsynced").and_then(Value::as_bool).unwrap_or(false))?,
        _ => return Err("Unknown Solar snapshot operation".into()),
    };
    serde_json::to_string(&output).map_err(|_| "Solar snapshot result cannot be encoded".into())
}

#[cfg(test)]
mod tests { use super::*;
 fn base() -> Value { json!({"_root":{"a":1},"_config":{"paths":{"x":"y"}},"00_templates":{"z":2},"01_city_inputs":{"n":1.0}}) }
 #[test] fn canonical_numbers_and_order_are_owned_by_solar_contracts() { let value=json!({"b":1.0,"a":-0.0,"e":1e21,"nested":[{"z":9007199254740991u64,"a":1e-7}]}); assert_eq!(canonical(&value).unwrap().as_bytes(), ds_solar_contracts::digest::canonical_json_bytes(&value).unwrap()); }
 #[test] fn rejects_bad_provenance_and_pending_cache() { let built: Value=serde_json::from_str(&evaluate(&serde_json::to_vec(&json!({"operation":"build","root":"r","city_id":"c","base":base()})).unwrap()).unwrap()).unwrap(); assert!(validate("r","c",&built,true).unwrap()["eligible"]==false); let mut bad=built; bad["snapshot_json"]=json!("{}"); assert!(validate("r","c",&bad,false).is_err()); }
}
