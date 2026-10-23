//! Canonical identities the print contract compares: row names and the
//! key-ordered JSON digest. The Kernel's `design` module re-exports both.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

pub fn digest(value: &impl Serialize) -> Result<String, String> {
    fn ordered(v: Value) -> Value {
        match v {
            Value::Object(m) => Value::Object(
                m.into_iter()
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .map(|(k, v)| (k, ordered(v)))
                    .collect(),
            ),
            Value::Array(a) => Value::Array(a.into_iter().map(ordered).collect()),
            v => v,
        }
    }
    let bytes = serde_json::to_vec(&ordered(
        serde_json::to_value(value).map_err(|e| e.to_string())?,
    ))
    .map_err(|e| e.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

/// The browser's `canonicalName` (`api/design.ts`) and the canonicalisation
/// `project_control`/`report` already applied, which are the same rule: trim,
/// lowercase, and every other byte becomes `_`.
pub fn canonical_row_name(name: &str) -> String {
    name.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}
