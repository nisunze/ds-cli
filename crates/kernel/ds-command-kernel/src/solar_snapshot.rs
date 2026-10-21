//! Deterministic Solar input snapshot admission and canonicalisation.
//!
//! Hosts acquire documents and persist accepted bytes. This module owns what
//! those documents mean as a calculator snapshot: canonical JSON, fingerprints,
//! provenance and whether a cached answer may be reused.
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub mod input_base;

const MAX_BYTES: usize = 8 << 20;
const MAX_DOCUMENTS: usize = 512;

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn text(value: &Value) -> Option<&str> {
    value.as_str().filter(|v| !v.is_empty())
}
fn object(value: &Value) -> Result<&Map<String, Value>, String> {
    value
        .as_object()
        .ok_or_else(|| "Solar snapshot requires a JSON object".into())
}

/// Preserve schema 3's deployed browser canonical bytes: sorted UTF-16 keys,
/// JavaScript integer property order/number spelling, and HTML-safe strings.
/// This intentionally differs from the Solar Rust-to-Rust manifest digest.
pub fn canonical(value: &Value) -> Result<String, String> {
    fn index(key: &str) -> Option<u32> {
        key.parse::<u32>()
            .ok()
            .filter(|n| *n != u32::MAX && n.to_string() == key)
    }
    fn write(value: &Value, out: &mut String, depth: usize) -> Result<(), String> {
        if depth > 120 {
            return Err("Solar snapshot nesting limit exceeded".into());
        }
        match value {
            Value::Number(n) => out.push_str(&crate::report_export::jcs::ecmascript_number(
                n.as_f64()
                    .filter(|n| n.is_finite())
                    .ok_or("Solar number is not finite")?,
            )),
            Value::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(item, out, depth + 1)?;
                }
                out.push(']');
            }
            Value::Object(items) => {
                let mut keys: Vec<_> = items.keys().collect();
                keys.sort_by(|a, b| match (index(a), index(b)) {
                    (Some(a), Some(b)) => a.cmp(&b),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    _ => a.encode_utf16().cmp(b.encode_utf16()),
                });
                out.push('{');
                for (i, key) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write(&Value::String(key.clone()), out, depth + 1)?;
                    out.push(':');
                    write(&items[key], out, depth + 1)?;
                }
                out.push('}');
            }
            _ => out.push_str(
                &serde_json::to_string(value)
                    .map_err(|_| "Solar snapshot cannot be encoded")?
                    .replace('&', "\\u0026")
                    .replace('<', "\\u003c")
                    .replace('>', "\\u003e")
                    .replace('\u{2028}', "\\u2028")
                    .replace('\u{2029}', "\\u2029"),
            ),
        }
        if out.len() > MAX_BYTES {
            return Err("Solar snapshot exceeds its byte limit".into());
        }
        Ok(())
    }
    let mut out = String::new();
    write(value, &mut out, 0)?;
    Ok(out)
}

fn validate_sections(base: &Map<String, Value>) -> Result<(), String> {
    for key in ["_root", "_config", "00_templates", "01_city_inputs"] {
        if !base.get(key).is_some_and(Value::is_object) {
            return Err(format!("Solar input is missing {key}"));
        }
    }
    let paths = base
        .get("_config")
        .and_then(Value::as_object)
        .and_then(|c| c.get("paths"))
        .and_then(Value::as_object);
    if paths.is_none_or(Map::is_empty) {
        return Err("Solar input path manifest is missing".into());
    }
    Ok(())
}

fn build(root: &str, city: &str, base: &Value, source_kind: &str) -> Result<Value, String> {
    let base = object(base)?;
    if base.len() != 4 {
        return Err("Solar snapshot base requires exactly four sections".into());
    }
    validate_sections(base)?;
    let base_value = Value::Object(base.clone());
    let fingerprint = input_base::fingerprint(&base_value)?;
    let seed =
        ds_solar_contracts::digest::demand_noise_seed(city).map_err(|_| "Solar seed failed")?;
    let mut snapshot = base.clone();
    snapshot.insert("_provenance".into(), json!({"schema":3,"source_kind":source_kind,"root":root,"context":city,"demand_noise_seed":seed,"input_base_fingerprint":fingerprint}));
    let snapshot_json = input_base::canonical(&Value::Object(snapshot))?;
    let bytes = snapshot_json.len();
    if bytes == 0 || bytes > MAX_BYTES {
        return Err("Solar snapshot exceeds its byte limit".into());
    }
    let documents = 1
        + base["00_templates"].as_object().map_or(0, Map::len)
        + base["01_city_inputs"].as_object().map_or(0, Map::len)
        + base["_config"].as_object().map_or(0, Map::len);
    if documents > MAX_DOCUMENTS {
        return Err("Solar snapshot exceeds its document limit".into());
    }
    Ok(
        json!({"snapshot_json":snapshot_json,"snapshot_sha256":digest(snapshot_json.as_bytes()),"input_base_fingerprint":fingerprint,"snapshot_bytes":bytes,"source_document_count":documents}),
    )
}

fn validate(root: &str, city: &str, snapshot: &Value) -> Result<Value, String> {
    let input = object(snapshot)?;
    let encoded = text(input.get("snapshot_json").unwrap_or(&Value::Null))
        .ok_or("Solar snapshot body is missing")?;
    let snapshot_hash = text(input.get("snapshot_sha256").unwrap_or(&Value::Null)).unwrap_or("");
    if encoded.len() > MAX_BYTES
        || !hex(snapshot_hash)
        || digest(encoded.as_bytes()) != snapshot_hash
    {
        return Err("Solar snapshot digest is invalid".into());
    }
    if input.get("snapshot_bytes").and_then(Value::as_u64) != Some(encoded.len() as u64)
        || !input
            .get("source_document_count")
            .and_then(Value::as_u64)
            .is_some_and(|n| (1..=MAX_DOCUMENTS as u64).contains(&n))
        || !input
            .get("media_downloads")
            .and_then(Value::as_array)
            .is_some_and(|v| v.len() <= MAX_DOCUMENTS)
    {
        return Err("Solar snapshot metadata is invalid".into());
    }
    let value: Value = serde_json::from_str(encoded).map_err(|_| "Solar snapshot is not JSON")?;
    let body = object(&value)?;
    validate_sections(body)?;
    let provenance = body
        .get("_provenance")
        .and_then(Value::as_object)
        .ok_or("Solar snapshot provenance is missing")?;
    if provenance.get("schema").and_then(Value::as_u64) != Some(3)
        || provenance.get("root").and_then(Value::as_str) != Some(root)
        || provenance.get("context").and_then(Value::as_str) != Some(city)
        || provenance
            .get("input_base_fingerprint")
            .and_then(Value::as_str)
            != text(input.get("input_base_fingerprint").unwrap_or(&Value::Null))
    {
        return Err("Solar snapshot provenance does not match this city".into());
    }
    let base = json!({"_root":body.get("_root").ok_or("Solar root missing")?,"_config":body.get("_config").ok_or("Solar config missing")?,"00_templates":body.get("00_templates").ok_or("Solar templates missing")?,"01_city_inputs":body.get("01_city_inputs").ok_or("Solar city inputs missing")?});
    let expected = input_base::fingerprint(&base)?;
    let declared = text(input.get("input_base_fingerprint").unwrap_or(&Value::Null)).unwrap_or("");
    if expected != declared && digest(canonical(&base)?.as_bytes()) != declared {
        // Older ds-brain snapshots use Go's encoding/json ordering and exact
        // numeric spellings. Preserve their original section bytes instead of
        // reserializing them through a JavaScript-number interpretation.
        let raw: std::collections::BTreeMap<String, &serde_json::value::RawValue> =
            serde_json::from_str(encoded).map_err(|_| "Solar snapshot is not JSON")?;
        let mut original_base = String::from("{");
        for (i, key) in ["00_templates", "01_city_inputs", "_config", "_root"]
            .iter()
            .enumerate()
        {
            if i > 0 {
                original_base.push(',');
            }
            original_base.push_str(&format!(
                "\"{key}\":{}",
                raw.get(*key).ok_or("Solar input section missing")?.get()
            ));
        }
        original_base.push('}');
        if provenance.get("source_kind").and_then(Value::as_str)
            != Some("authenticated ds-brain desktop snapshot")
            || digest(original_base.as_bytes()) != declared
        {
            return Err("Solar snapshot fingerprint is invalid".into());
        }
    }
    Ok(json!({"eligible":true}))
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    // An encoded snapshot can expand sixfold inside the request's JSON string.
    if input.len() > MAX_BYTES * 6 + 65536 {
        return Err("Solar snapshot request exceeds its byte limit".into());
    }
    let request: Value =
        serde_json::from_slice(input).map_err(|_| "Solar snapshot request is not JSON")?;
    let object = object(&request)?;
    let root = text(object.get("root").unwrap_or(&Value::Null)).ok_or("Solar root is required")?;
    let city =
        text(object.get("city_id").unwrap_or(&Value::Null)).ok_or("Solar city is required")?;
    if root.len() > 4096 || city.len() > 1024 {
        return Err("Solar snapshot identity exceeds its limit".into());
    }
    let output = match text(object.get("operation").unwrap_or(&Value::Null)) {
        Some("select") => {
            let fact = |key: &str| {
                object
                    .get(key)
                    .and_then(Value::as_bool)
                    .ok_or_else(|| format!("Solar selection fact {key} is required"))
            };
            let pending = fact("pending_unsynced")?;
            let present = fact("has_workspace")?;
            let refresh = fact("refresh")?;
            json!({"use_workspace": present && (!refresh || pending), "allow_fetch": !pending})
        }
        Some("build") => build(
            root,
            city,
            object.get("base").ok_or("Solar base is required")?,
            "local solar input workspace",
        )?,
        Some("validate") => validate(
            root,
            city,
            object.get("snapshot").ok_or("Solar snapshot is required")?,
        )?,
        _ => return Err("Unknown Solar snapshot operation".into()),
    };
    serde_json::to_string(&output).map_err(|_| "Solar snapshot result cannot be encoded".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn base() -> Value {
        json!({"_root":{"a":1},"_config":{"paths":{"x":"y"}},"00_templates":{"z":2},"01_city_inputs":{"n":1.0}})
    }
    fn snapshot() -> Value {
        let mut value = build("r", "c", &base(), "local solar input workspace").unwrap();
        value["media_downloads"] = json!([]);
        value
    }
    #[test]
    fn pending_edits_always_win_over_refresh_and_block_cloud_replacement() {
        for present in [false, true] {
            let output: Value = serde_json::from_str(
                &evaluate(
                    &serde_json::to_vec(&json!({
                        "operation":"select", "root":"r", "city_id":"c", "pending_unsynced":true,
                        "has_workspace":present, "refresh":true
                    }))
                    .unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            assert_eq!(output["use_workspace"], present);
            assert_eq!(output["allow_fetch"], false);
        }
    }
    #[test]
    fn deployed_browser_canonical_bytes_are_preserved() {
        let value = json!({"b":1.0,"a":-0.0,"e":1e21,"nested":[{"z":9007199254740991u64,"a":1e-7}], "html":"<&>\u{2028}\u{2029}", "10":10,"2":2});
        assert_eq!(
            canonical(&value).unwrap(),
            r#"{"2":2,"10":10,"a":0,"b":1,"e":1e+21,"html":"\u003c\u0026\u003e\u2028\u2029","nested":[{"a":1e-7,"z":9007199254740991}]}"#
        );
    }
    #[test]
    fn legacy_local_snapshot_remains_readable_without_rewriting_its_bytes() {
        let mut base = base();
        base["01_city_inputs"] = json!({"10":10,"2":2,"number":-0.0});
        let fingerprint = digest(canonical(&base).unwrap().as_bytes());
        let mut body = base;
        body["_provenance"] = json!({"schema":3,"source_kind":"local solar input workspace","root":"r","context":"c","input_base_fingerprint":fingerprint});
        let encoded = canonical(&body).unwrap();
        let snapshot = json!({"snapshot_json":encoded,"snapshot_sha256":digest(encoded.as_bytes()),"snapshot_bytes":encoded.len(),"source_document_count":1,"media_downloads":[],"input_base_fingerprint":fingerprint});
        assert_eq!(validate("r", "c", &snapshot).unwrap()["eligible"], true);
        assert_eq!(snapshot["snapshot_json"], encoded);
    }
    #[test]
    fn governed_encoding_keeps_exact_integers_and_negative_zero_in_snapshot_body() {
        let mut base = base();
        base["01_city_inputs"] = json!({"integer":9007199254740993u64,"zero":-0.0});
        let mut snapshot = build("r", "c", &base, "local solar input workspace").unwrap();
        snapshot["media_downloads"] = json!([]);
        let body = snapshot["snapshot_json"].as_str().unwrap();
        assert!(body.contains("9007199254740993"));
        assert!(body.contains("\"zero\":-0"));
        assert_eq!(validate("r", "c", &snapshot).unwrap()["eligible"], true);
        assert_ne!(
            input_base::fingerprint(&base).unwrap(),
            digest(canonical(&base).unwrap().as_bytes())
        );
    }
    #[test]
    fn validates_exact_bytes_and_city() {
        let mut value = snapshot();
        assert_eq!(validate("r", "c", &value).unwrap()["eligible"], true);
        assert!(validate("r", "other", &value).is_err());
        value["snapshot_bytes"] = json!(1);
        assert!(validate("r", "c", &value).is_err());
        value = snapshot();
        value["snapshot_json"] = json!("{}");
        assert!(validate("r", "c", &value).is_err());
    }
    #[test]
    fn refuses_changed_fingerprint_even_when_body_digest_is_recomputed() {
        let mut value = snapshot();
        let mut body: Value =
            serde_json::from_str(value["snapshot_json"].as_str().unwrap()).unwrap();
        body["01_city_inputs"]["n"] = json!(2);
        let encoded = canonical(&body).unwrap();
        value["snapshot_sha256"] = json!(digest(encoded.as_bytes()));
        value["snapshot_bytes"] = json!(encoded.len());
        value["snapshot_json"] = json!(encoded);
        assert!(validate("r", "c", &value).is_err());
    }
}
