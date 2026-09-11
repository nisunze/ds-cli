//! Which transformers may be printed, in what order, and which of them this
//! device holds — clean or dirty — decided once for the paired application and
//! for `ds`.
//!
//! The browser joined two inventories by hand (`prepare.ts`): the project's
//! status rows and this device's cached rooms, fenced to the signed-in
//! account. The join, the kind filter, the order, the version fallback chain
//! and the bound are this module's. It performs no IO: the host reports the
//! rows, the rooms and the account.

use crate::design::{canonical_row_name, is_special_row};
use crate::design_status_query::locale_cmp;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};
use std::collections::HashMap;

/// One room this device holds, as its inventory reports it.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Room {
    pub name: String,
    /// The account that wrote the room; empty or absent = unowned.
    #[serde(default)]
    pub owner_email: Option<String>,
    #[serde(default)]
    pub local_dirty: bool,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
pub struct PrintableRow {
    pub name: String,
    pub kind: String,
    /// This device holds a room for it — clean or dirty.
    pub cached: bool,
    pub dirty: bool,
    /// `metadata.version`, else the client's base server version, else 0 —
    /// read as JavaScript's `Number` read it (`null` for what it could not).
    pub server_version: Value,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
pub struct More {
    pub omitted: usize,
}

#[derive(Debug, Clone, Serialize, JsonSchema, PartialEq)]
pub struct Inventory {
    pub total: usize,
    pub transformers: Vec<PrintableRow>,
    pub more: More,
}

/// Numeric version coercion for JSON status values, matching Number().
fn js_number(value: &Value) -> Value {
    let number = match value {
        Value::Number(n) => n.as_f64().unwrap_or(f64::NAN),
        Value::Bool(b) => {
            if *b {
                1.0
            } else {
                0.0
            }
        }
        Value::String(s) => {
            let trimmed = s.trim();
            if trimmed.is_empty() {
                0.0
            } else {
                if let Some(digits) = trimmed
                    .strip_prefix("0x")
                    .or_else(|| trimmed.strip_prefix("0X"))
                {
                    u64::from_str_radix(digits, 16)
                        .map(|n| n as f64)
                        .unwrap_or(f64::NAN)
                } else if let Some(digits) = trimmed
                    .strip_prefix("0b")
                    .or_else(|| trimmed.strip_prefix("0B"))
                {
                    u64::from_str_radix(digits, 2)
                        .map(|n| n as f64)
                        .unwrap_or(f64::NAN)
                } else if let Some(digits) = trimmed
                    .strip_prefix("0o")
                    .or_else(|| trimmed.strip_prefix("0O"))
                {
                    u64::from_str_radix(digits, 8)
                        .map(|n| n as f64)
                        .unwrap_or(f64::NAN)
                } else {
                    trimmed.parse::<f64>().unwrap_or(f64::NAN)
                }
            }
        }
        Value::Array(values) => {
            fn text(value: &Value) -> String {
                match value {
                    Value::Null => String::new(),
                    Value::String(s) => s.clone(),
                    Value::Array(values) => values.iter().map(text).collect::<Vec<_>>().join(","),
                    Value::Object(_) => "[object Object]".into(),
                    other => other.to_string(),
                }
            }
            return js_number(&Value::String(
                values.iter().map(text).collect::<Vec<_>>().join(","),
            ));
        }
        Value::Null => 0.0,
        _ => f64::NAN,
    };
    Number::from_f64(number)
        .map(Value::Number)
        .unwrap_or(Value::Null)
}

/// `a ?? b`: the first value that is neither absent nor null.
fn first_present(candidates: [Option<&Value>; 2]) -> Option<&Value> {
    candidates
        .into_iter()
        .flatten()
        .find(|value| !value.is_null())
}

fn printable_kind(row: &Map<String, Value>) -> Option<String> {
    let name = row.get("name").and_then(Value::as_str)?;
    if canonical_row_name(name).is_empty() || is_special_row(name) {
        return None;
    }
    match row.get("kind") {
        // Absent means an ordinary transformer; `null` never did.
        None => Some("individual".to_string()),
        Some(Value::String(kind)) if kind == "individual" => Some(kind.clone()),
        Some(_) => None,
    }
}

/// The printable inventory: individual transformers in name order, each with
/// whether this device holds it, bounded to `limit` rows.
pub fn printable(
    rows: &[Value],
    rooms: &[Room],
    current_user: &str,
    limit: usize,
) -> Result<Inventory, String> {
    if !(1..=500).contains(&limit) {
        return Err("printing.transformers accepts limit from 1 through 500.".into());
    }
    let current = current_user.trim().to_lowercase();
    // A room is this account's when it is unowned, or owned by the signed-in
    // account — never when no account is known. A later room for the same
    // canonical name replaces an earlier one, as the browser's Map did.
    let mut local: HashMap<String, &Room> = HashMap::new();
    for room in rooms {
        let owner = room
            .owner_email
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_lowercase();
        if owner.is_empty() || (!current.is_empty() && owner == current) {
            local.insert(canonical_row_name(&room.name), room);
        }
    }
    let mut all: Vec<(&Map<String, Value>, String, String)> = rows
        .iter()
        .filter_map(Value::as_object)
        .filter_map(|row| {
            let kind = printable_kind(row)?;
            let name = row
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Some((row, name, kind))
        })
        .collect();
    all.sort_by(|left, right| locale_cmp(&left.1, &right.1));
    let total = all.len();
    let transformers: Vec<PrintableRow> = all
        .iter()
        .take(limit)
        .map(|(row, name, kind)| {
            let held = local.get(&canonical_row_name(name)).copied();
            let version = first_present([
                row.get("metadata")
                    .and_then(Value::as_object)
                    .and_then(|metadata| metadata.get("version")),
                row.get("client_base_server_version"),
            ])
            .map(js_number)
            .unwrap_or_else(|| Value::Number(0.into()));
            PrintableRow {
                name: name.clone(),
                kind: kind.clone(),
                cached: held.is_some(),
                dirty: held.is_some_and(|room| room.local_dirty),
                server_version: version,
            }
        })
        .collect();
    let omitted = total - transformers.len();
    Ok(Inventory {
        total,
        transformers,
        more: More { omitted },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn room(name: &str, owner: Option<&str>, dirty: bool) -> Room {
        Room {
            name: name.into(),
            owner_email: owner.map(str::to_string),
            local_dirty: dirty,
        }
    }

    fn rows() -> Vec<Value> {
        vec![
            json!({"name": "tx_b", "kind": "individual", "client_cached": false}),
            json!({"name": "tx_a", "kind": "individual", "client_cached": true, "metadata": {"version": 4}}),
            json!({"name": "combined_transformer", "kind": "combined"}),
            json!({"name": "tx_c", "client_base_server_version": "7"}),
            json!({"name": "mv_data", "kind": null}),
            json!({"name": "TX_D", "kind": "individual", "metadata": {"version": null}, "client_base_server_version": "seven"}),
        ]
    }

    #[test]
    fn joins_the_fleet_with_this_account_rooms_in_name_order_bounded() {
        let out = printable(
            &rows(),
            &[
                room("tx_a", Some("Owner@Example.com"), false),
                room("TX_D", None, true),
            ],
            "owner@example.com",
            2,
        )
        .unwrap();
        assert_eq!(out.total, 4);
        assert_eq!(out.more.omitted, 2);
        assert_eq!(out.transformers[0].name, "tx_a");
        assert!(out.transformers[0].cached && !out.transformers[0].dirty);
        assert_eq!(out.transformers[0].server_version, json!(4.0));
        assert_eq!(out.transformers[1].name, "tx_b");
        assert!(!out.transformers[1].cached);
        assert_eq!(out.transformers[1].server_version, json!(0));
        let all = printable(&rows(), &[room("TX_D", None, true)], "", 500).unwrap();
        let names: Vec<&str> = all
            .transformers
            .iter()
            .map(|row| row.name.as_str())
            .collect();
        assert_eq!(names, ["tx_a", "tx_b", "tx_c", "TX_D"]);
        assert_eq!(all.transformers[2].server_version, json!(7.0));
        // An unreadable version is `null`, never a guessed number.
        assert_eq!(all.transformers[3].server_version, Value::Null);
        assert!(all.transformers[3].cached && all.transformers[3].dirty);
    }

    #[test]
    fn another_account_room_and_an_unknown_account_never_count_as_this_device() {
        let other = printable(
            &rows(),
            &[room("tx_a", Some("other@example.com"), true)],
            "owner@example.com",
            1,
        )
        .unwrap();
        assert!(!other.transformers[0].cached && !other.transformers[0].dirty);
        let unknown = printable(
            &rows(),
            &[room("tx_a", Some("owner@example.com"), false)],
            "",
            1,
        )
        .unwrap();
        assert!(!unknown.transformers[0].cached);
    }

    #[test]
    fn the_bound_is_named() {
        assert!(printable(&[], &[], "", 0).is_err());
        assert!(printable(&[], &[], "", 501).is_err());
        assert_eq!(printable(&[], &[], "", 1).unwrap().total, 0);
    }
}
