//! The catalogue aliases every report reader applies: a catalogue row's
//! canonical name, its declared misspellings, and the unambiguous alias map
//! (Dirty Categories). The Kernel's design configuration re-exports them.

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub fn js_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .map(|v| {
                if v.is_null() {
                    String::new()
                } else {
                    js_string(v)
                }
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
        _ => value.to_string(),
    }
}
pub fn valid_alias(key: &str) -> bool {
    !matches!(key, "" | "nan" | "none" | "null" | "undefined" | "\\")
}
pub fn alias_list(value: &Value) -> Vec<String> {
    let values: Vec<Value> = match value {
        Value::Array(a) => a.clone(),
        Value::String(s) => s.split("##").map(|s| json!(s)).collect(),
        _ => vec![],
    };
    let mut seen = BTreeSet::new();
    values
        .iter()
        .filter_map(|v| {
            let text = js_string(v).trim().to_owned();
            let key = text.to_lowercase();
            (valid_alias(&key) && seen.insert(key)).then_some(text)
        })
        .collect()
}

/// Unambiguous reporting aliases, normalized once for all catalogue readers.
/// An alias claimed by different canonical names is omitted, regardless of order.
pub fn alias_map(rows: &Value) -> BTreeMap<String, String> {
    let mut aliases = BTreeMap::new();
    let mut ambiguous = BTreeSet::new();
    for row in rows.as_array().into_iter().flatten() {
        let Some(clean) = canonical(row) else {
            continue;
        };
        for name in alias_list(field(row, "misspelled_names", "misspellednames"))
            .into_iter()
            .chain(std::iter::once(clean.clone()))
        {
            let key = name.trim().to_lowercase();
            if aliases
                .insert(key.clone(), clean.clone())
                .is_some_and(|previous| previous != clean)
            {
                ambiguous.insert(key);
            }
        }
    }
    for key in ambiguous {
        aliases.remove(&key);
    }
    aliases
}

pub fn field<'a>(row: &'a Value, canonical: &str, sanitized: &str) -> &'a Value {
    row.get(canonical)
        .or_else(|| {
            row.as_object()?.iter().find_map(|(key, value)| {
                (key.to_ascii_lowercase()
                    .chars()
                    .filter(char::is_ascii_alphanumeric)
                    .collect::<String>()
                    == sanitized)
                    .then_some(value)
            })
        })
        .unwrap_or(&Value::Null)
}

/// One catalogue row's canonical name, or `None` when the row declares none.
/// A row with no canonical name groups nothing and validates nothing.
pub fn canonical(row: &Value) -> Option<String> {
    let name = field(row, "clean_name", "cleanname").as_str()?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}
