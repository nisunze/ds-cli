//! Solar's governed input-base encoding, shared with Brain's encoding/json.
//! This is deliberately distinct from legacy browser snapshots and report JCS.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

/// Sorted UTF-8 keys, exact integers, shortest binary64 floats, HTML escaping.
/// Go retains negative zero. JSON inputs must retain their integer/float type.
pub fn canonical(value: &Value) -> Result<String, String> {
    fn write(value: &Value, out: &mut String, depth: usize) -> Result<(), String> {
        if depth > 120 {
            return Err("Solar snapshot nesting limit exceeded".into());
        }
        match value {
            Value::Number(n) => {
                if n.is_i64() || n.is_u64() {
                    out.push_str(&n.to_string());
                } else {
                    let f = n
                        .as_f64()
                        .filter(|f| f.is_finite())
                        .ok_or("Solar number is not finite")?;
                    if f == 0.0 && f.is_sign_negative() {
                        out.push_str("-0");
                    } else {
                        out.push_str(&crate::report_export::jcs::ecmascript_number(f));
                    }
                }
            }
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
                keys.sort();
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
                    .map_err(|_| "Solar input cannot be encoded")?
                    .replace('&', "\\u0026")
                    .replace('<', "\\u003c")
                    .replace('>', "\\u003e")
                    .replace('\u{2028}', "\\u2028")
                    .replace('\u{2029}', "\\u2029"),
            ),
        }
        if out.len() > super::MAX_BYTES {
            return Err("Solar snapshot exceeds its byte limit".into());
        }
        Ok(())
    }
    let mut out = String::new();
    write(value, &mut out, 0)?;
    Ok(out)
}

/// Brain captures four sections and only the governed config path manifest.
/// Client transport hashes remain hashes of the exact transmitted body.
pub fn encode(base: &Value) -> Result<String, String> {
    let sections = super::object(base)?;
    super::validate_sections(sections)?;
    canonical(&json!({
        "_root":sections["_root"], "_config":{"paths":sections["_config"]["paths"]},
        "00_templates":sections["00_templates"], "01_city_inputs":sections["01_city_inputs"]
    }))
}

pub fn fingerprint(base: &Value) -> Result<String, String> {
    Ok(format!("{:x}", Sha256::digest(encode(base)?.as_bytes())))
}
