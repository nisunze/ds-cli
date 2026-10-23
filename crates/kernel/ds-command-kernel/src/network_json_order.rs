//! Object key order for operations moved from the Network engine.
//!
//! The Network engine is built without serde_json's `preserve_order`, so it
//! parses and emits every JSON object as a `BTreeMap`: keys in alphanumerical
//! order. This crate keeps insertion order. Operations that moved here from
//! the engine (upload cleaning, import-key merge, file export) sort every
//! object at their boundary, so callers receive the same key order, column
//! order and bytes the engine produced.
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;

/// Parse request bytes the way the engine did: every object key-sorted.
pub(crate) fn decode_sorted<T: DeserializeOwned>(bytes: &[u8]) -> serde_json::Result<T> {
    let mut value: Value = serde_json::from_slice(bytes)?;
    value.sort_all_objects();
    serde_json::from_value(value)
}

/// Re-order every JSON object inside a typed value (typed fields keep their
/// declaration order, as the engine's serializer did).
pub(crate) fn sorted<T: Serialize + DeserializeOwned>(typed: T) -> serde_json::Result<T> {
    let mut value = serde_json::to_value(&typed)?;
    value.sort_all_objects();
    serde_json::from_value(value)
}

/// Re-serialize pretty JSON text with key-sorted objects, as the engine
/// emitted it. Numbers round-trip exactly (`float_roundtrip`).
pub(crate) fn sorted_pretty_text(text: &str) -> serde_json::Result<String> {
    let mut value: Value = serde_json::from_str(text)?;
    value.sort_all_objects();
    serde_json::to_string_pretty(&value)
}
