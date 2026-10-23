//! Type inference and column ordering — §3 of the columnar format contract.
//!
//! One ladder serves both output formats. If Arrow IPC and GeoParquet ever
//! inferred types differently, a value would change meaning by being written
//! twice, so the ladder lives here and neither writer has its own.

use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Narrowest kind that holds every non-null value in a column.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[cfg_attr(feature = "bindings", derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum PropertyKind {
    /// Every observed value was null. Emitted as `Utf8`, never a null type —
    /// too many readers reject or silently retype a null-typed column.
    Null,
    Boolean,
    Integer,
    Number,
    Text,
}

pub const FEATURE_ID_COLUMN: &str = "__ds_feature_id";
/// Arrow IPC geometry column. Named by the browser streaming contract, which
/// this crate does not renegotiate.
pub const IPC_GEOMETRY_COLUMN: &str = "__ds_geometry_wkb";
/// GeoParquet primary geometry column, per the GeoParquet convention.
pub const PARQUET_GEOMETRY_COLUMN: &str = "geometry";

/// Property columns in the order an operator uploaded them: the hint first
/// (which carries the source's own column order), then anything discovered in
/// the batch that the hint did not name.
pub fn ordered_columns(features: &[Value], columns_hint: &[String]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut columns = Vec::new();
    for column in columns_hint {
        if !column.is_empty() && seen.insert(column.clone()) {
            columns.push(column.clone());
        }
    }
    for column in columns_for_features(features) {
        if seen.insert(column.clone()) {
            columns.push(column);
        }
    }
    columns
}

/// Every property key present anywhere in the batch, in first-seen order.
pub fn columns_for_features(features: &[Value]) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut columns = Vec::new();
    for feature in features {
        let Some(properties) = properties_of(feature) else {
            continue;
        };
        for key in properties.keys() {
            if seen.insert(key.clone()) {
                columns.push(key.clone());
            }
        }
    }
    columns
}

pub fn properties_of(feature: &Value) -> Option<&Map<String, Value>> {
    feature.get("properties").and_then(Value::as_object)
}

pub fn property<'a>(feature: &'a Value, column: &str) -> Option<&'a Value> {
    properties_of(feature)
        .and_then(|properties| properties.get(column))
        .filter(|value| !value.is_null())
}

pub fn infer_property_kinds(
    features: &[Value],
    columns: &[String],
) -> BTreeMap<String, PropertyKind> {
    let mut out = BTreeMap::new();
    for column in columns {
        let mut kind = PropertyKind::Null;
        for feature in features {
            let Some(value) = property(feature, column) else {
                continue;
            };
            kind = widen(kind, observe(value));
            if kind == PropertyKind::Text {
                break;
            }
        }
        out.insert(column.clone(), kind);
    }
    out
}

/// Apply the same stored-type ladder to table rows, across every batch.
pub fn infer_row_kinds(
    rows: &[Map<String, Value>],
    columns: &[String],
) -> BTreeMap<String, PropertyKind> {
    columns
        .iter()
        .map(|column| {
            let kind = rows
                .iter()
                .filter_map(|row| row.get(column))
                .fold(PropertyKind::Null, |kind, value| {
                    widen(kind, observe(value))
                });
            (column.clone(), kind)
        })
        .collect()
}

fn observe(value: &Value) -> PropertyKind {
    match value {
        Value::Bool(_) => PropertyKind::Boolean,
        Value::Number(number) => {
            if number.is_i64() || number.is_u64() {
                PropertyKind::Integer
            } else {
                PropertyKind::Number
            }
        }
        Value::Null => PropertyKind::Null,
        _ => PropertyKind::Text,
    }
}

/// A column resolves to the narrowest kind holding every value it saw. Mixing
/// integers with reals widens to `Number`; mixing anything else widens to
/// `Text` rather than dropping a value that will not fit.
fn widen(current: PropertyKind, observed: PropertyKind) -> PropertyKind {
    use PropertyKind::{Boolean, Integer, Null, Number, Text};
    match (current, observed) {
        (Null, other) => other,
        (other, Null) => other,
        (a, b) if a == b => a,
        (Integer, Number) | (Number, Integer) => Number,
        (Boolean, _) | (_, Boolean) => Text,
        _ => Text,
    }
}

/// Whether a column already pinned to `pinned` can still hold a batch whose
/// values observe `observed`.
///
/// A streaming conversion pins one schema for the whole file, so this is the
/// question that decides whether a later batch can be written or the artifact
/// has to be given up. Text is the universal sink — [`value_to_text`] renders
/// every scalar — so a text column accepts anything. A narrower column accepts
/// only what does not widen it; an `Int64` receiving `1.5` genuinely cannot
/// hold the value, and writing a null there is the silent loss this exists to
/// prevent.
pub fn accepts(pinned: PropertyKind, observed: PropertyKind) -> bool {
    match pinned {
        PropertyKind::Null | PropertyKind::Text => true,
        narrower => widen(narrower, observed) == narrower,
    }
}

/// JSON scalar to its stored text form. Strings keep their exact bytes; other
/// scalars are rendered compactly rather than re-serialized with JSON quoting.
pub fn value_to_text(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        other => other.to_string(),
    }
}

pub fn value_as_i64(value: &Value) -> Option<i64> {
    value
        .as_i64()
        .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn features() -> Vec<Value> {
        vec![
            json!({"properties": {"a": 1, "b": 1.5, "c": "x", "d": true, "e": null}}),
            json!({"properties": {"a": 2, "b": 2,   "c": 7,   "d": false}}),
        ]
    }

    #[test]
    fn the_ladder_widens_to_the_narrowest_kind_that_holds_every_value() {
        let f = features();
        let columns = ordered_columns(&f, &[]);
        let kinds = infer_property_kinds(&f, &columns);
        assert_eq!(kinds["a"], PropertyKind::Integer);
        assert_eq!(
            kinds["b"],
            PropertyKind::Number,
            "int + real widens to real"
        );
        assert_eq!(
            kinds["c"],
            PropertyKind::Text,
            "text + number widens to text"
        );
        assert_eq!(kinds["d"], PropertyKind::Boolean);
        assert_eq!(
            kinds["e"],
            PropertyKind::Null,
            "all-null stays Null, written as Utf8"
        );
    }

    #[test]
    fn the_hint_fixes_column_order_and_extras_follow() {
        let f = features();
        let hint = vec!["c".to_string(), "a".to_string()];
        // Hint first, in hint order; then whatever the hint did not name.
        assert_eq!(ordered_columns(&f, &hint), vec!["c", "a", "b", "d", "e"]);
    }

    #[test]
    fn an_empty_hint_entry_is_not_a_column() {
        let f = features();
        let hint = vec![String::new(), "a".to_string()];
        assert_eq!(ordered_columns(&f, &hint)[0], "a");
    }

    #[test]
    fn an_explicit_json_null_is_not_a_value() {
        let f = features();
        assert!(property(&f[0], "e").is_none(), "null must read as absent");
    }
}

#[cfg(test)]
mod row_tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn rows_and_features_use_one_ladder_and_never_guess_text_types() {
        let rows: Vec<Map<String, Value>> = serde_json::from_value(
            json!([{"n":1,"b":true,"date":"2026-10-02","number":"42"},{"n":1.5,"b":false}]),
        )
        .unwrap();
        let columns = vec![
            "n".into(),
            "b".into(),
            "date".into(),
            "number".into(),
            "missing".into(),
        ];
        let features = rows
            .iter()
            .map(|row| json!({"properties":row}))
            .collect::<Vec<_>>();
        let kinds = infer_row_kinds(&rows, &columns);
        assert_eq!(kinds, infer_property_kinds(&features, &columns));
        assert_eq!(kinds["number"], PropertyKind::Text);
        assert_eq!(kinds["date"], PropertyKind::Text);
        assert_eq!(kinds["missing"], PropertyKind::Null);
    }
}
