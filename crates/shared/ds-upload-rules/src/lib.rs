//! Neutral upload rules shared by the page Kernel and Network cleaning.
//!
//! A value domain (categorical string/int/number or boolean, with aliases and
//! a unit) decides how one cell value is cleaned, confirmed, coerced, parsed
//! and formatted. The page's editors answer these synchronously while tables
//! render; upload cleaning applies the same rules to every uploaded feature.
//! Upload grouping plans temporary layers from parsed preview metadata and
//! stamps each feature's source file and layer. Pure functions over JSON
//! values: no IO, clock, engine or application state.
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use std::collections::BTreeMap;

pub mod domain_value;
pub mod upload_groups;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[cfg_attr(feature = "bindings", tsify(hashmap_as_object))]
pub struct ColumnPlanDomain {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub values: Vec<Value>,
    #[serde(default)]
    pub aliases: BTreeMap<String, Value>,
    #[serde(default)]
    pub unit: Option<String>,
}

pub fn clean_domain_value(domain: &ColumnPlanDomain, value: &Value) -> Value {
    let canonical = canonical_domain_value(domain, value).unwrap_or_else(|| value.clone());
    coerce_domain_value(domain, &canonical)
}

pub fn domain_confirms_value(domain: &ColumnPlanDomain, value: &Value) -> bool {
    !value.is_null() && !domain.values.is_empty() && canonical_domain_value(domain, value).is_some()
}

pub fn canonical_domain_value(domain: &ColumnPlanDomain, value: &Value) -> Option<Value> {
    if let Some(alias) = canonical_domain_alias_value(domain, value) {
        return Some(alias);
    }
    if domain.values.is_empty() {
        return None;
    }
    if domain.kind == "categorical_number"
        && let Some(wanted) = parse_numeric_domain_value(value)
    {
        for option in &domain.values {
            if parse_numeric_domain_value(option) == Some(wanted) {
                return Some(option.clone());
            }
        }
    }
    let wanted = compact_domain_value(value);
    if wanted.is_empty() {
        return None;
    }
    domain
        .values
        .iter()
        .find(|option| compact_domain_value(option) == wanted)
        .cloned()
}

pub fn canonical_domain_alias_value(domain: &ColumnPlanDomain, value: &Value) -> Option<Value> {
    let wanted = compact_domain_value(value);
    if wanted.is_empty() {
        return None;
    }
    domain
        .aliases
        .iter()
        .find(|(alias, _)| compact_text(alias) == wanted)
        .map(|(_, canonical)| canonical.clone())
}

pub fn coerce_domain_value(domain: &ColumnPlanDomain, value: &Value) -> Value {
    let text = value_as_text(value).trim().to_string();
    if text.is_empty() {
        return Value::String(String::new());
    }
    match domain.kind.as_str() {
        "categorical_int" => text
            .parse::<i64>()
            .map(|value| Value::Number(Number::from(value)))
            .unwrap_or(Value::String(text)),
        "categorical_number" => parse_numeric_domain_value(&Value::String(text.clone()))
            .and_then(number_value)
            .unwrap_or(Value::String(text)),
        "boolean" => match text.to_ascii_lowercase().as_str() {
            "true" | "yes" | "y" | "1" => Value::Bool(true),
            "false" | "no" | "n" | "0" => Value::Bool(false),
            _ => Value::String(text),
        },
        _ => Value::String(text),
    }
}

pub fn parse_numeric_domain_value(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        _ => {
            let text = value_as_text(value).trim().to_string();
            if text.is_empty() {
                return None;
            }
            if let Ok(parsed) = text.parse::<f64>()
                && parsed.is_finite()
            {
                return Some(parsed);
            }
            let mut chars = text.chars().peekable();
            let mut number = String::new();
            if matches!(chars.peek(), Some('-')) {
                number.push('-');
                chars.next();
            }
            let mut saw_digit = false;
            let mut saw_dot = false;
            while let Some(ch) = chars.peek().copied() {
                if ch.is_ascii_digit() {
                    saw_digit = true;
                    number.push(ch);
                    chars.next();
                } else if ch == '.' && !saw_dot {
                    saw_dot = true;
                    number.push(ch);
                    chars.next();
                } else {
                    break;
                }
            }
            let suffix = chars.collect::<String>().trim().to_string();
            if !saw_digit || suffix.is_empty() || !suffix.chars().all(|ch| ch.is_ascii_alphabetic())
            {
                return None;
            }
            number.parse::<f64>().ok().filter(|value| value.is_finite())
        }
    }
}

pub fn number_value(value: f64) -> Option<Value> {
    Number::from_f64(value).map(Value::Number)
}

pub fn normalize_column(value: &str) -> String {
    let mut out = String::new();
    let mut last_was_underscore = false;
    for ch in value.trim().to_lowercase().chars() {
        if ch.is_alphanumeric() || ch == '_' {
            out.push(ch);
            last_was_underscore = false;
        } else if !last_was_underscore {
            out.push('_');
            last_was_underscore = true;
        }
    }
    out.trim_matches('_').to_string()
}

pub fn value_as_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        Value::Bool(value) => value.to_string(),
        _ => value.to_string(),
    }
}

pub fn compact_domain_value(value: &Value) -> String {
    compact_text(&value_as_text(value))
}

pub fn compact_text(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .chars()
        .filter(|ch| ch.is_alphanumeric())
        .collect()
}
