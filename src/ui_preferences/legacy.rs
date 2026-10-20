//! The `persistent_cache` legacy source: the `persist:` keys and prefixes of
//! the retired `ds_web_store` `cache` object store that move into UI
//! preferences, and one reader per key. Deleted whole with the importer once
//! the recovery window closes.
use super::Key;
use serde_json::Value;

/// The exact legacy keys and key prefixes the host reads, read-only.
pub(super) fn keys() -> (Vec<&'static str>, Vec<&'static str>) {
    (Vec::new(), Vec::new())
}

/// The rows one legacy entry yields: key, project and raw value. Readers fill
/// a missing timestamp with `now`; an entry no reader owns yields none.
pub(super) fn read(_legacy_key: &str, _value: &Value, _now: u64) -> Vec<(Key, Option<String>, Value)> {
    Vec::new()
}
