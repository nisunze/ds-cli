//! Stable GeoJSON feature identity.
//!
//! A feature may carry more than one durable identity while it moves through
//! import, persistence, and editing lanes.  The ordered candidates here are
//! the one canonical rule used by session mounting, version stamping, and
//! public layer comparison:
//!
//! 1. top-level GeoJSON `id`
//! 2. `properties.id`
//! 3. `properties.doc_id`
//! 4. `properties.feature_id`
//!
//! Strings are trimmed, numeric ids use their JSON representation, and empty
//! or session-scoped (`layer/id`) values are not stable layer-local ids.

use serde_json::Value;
use std::collections::HashSet;

/// Resolve the first stable identity carried by a GeoJSON feature.
pub fn geojson_feature_id(feature: &Value) -> Option<String> {
    geojson_feature_identity_candidates(feature)
        .into_iter()
        .next()
}

/// Return every stable identity alias carried by a GeoJSON feature.
///
/// Candidates retain canonical priority order and are de-duplicated.  The
/// caller decides whether aliases identify exactly one peer or form an
/// ambiguous component; this helper never guesses by row order or geometry.
pub fn geojson_feature_identity_candidates(feature: &Value) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut candidates = Vec::new();
    let values = std::iter::once(feature.get("id")).chain(
        ["id", "doc_id", "feature_id"]
            .into_iter()
            .map(|key| feature.get("properties").and_then(|props| props.get(key))),
    );
    for value in values {
        let Some(id) = value_as_feature_id(value).filter(|id| !id.contains('/')) else {
            continue;
        };
        if seen.insert(id.clone()) {
            candidates.push(id);
        }
    }
    candidates
}

fn value_as_feature_id(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(id)) if !id.trim().is_empty() => Some(id.trim().to_string()),
        Some(Value::Number(number)) => Some(number.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn candidates_follow_priority_dedupe_and_accept_numbers() {
        let feature = json!({
            "id": 42,
            "properties": {
                "id": " 42 ",
                "doc_id": "persisted",
                "feature_id": "fallback"
            }
        });
        assert_eq!(
            geojson_feature_identity_candidates(&feature),
            vec!["42", "persisted", "fallback"]
        );
        assert_eq!(geojson_feature_id(&feature).as_deref(), Some("42"));
    }

    #[test]
    fn invalid_higher_priority_value_falls_through() {
        let feature = json!({
            "id": "session/layer-id",
            "properties": { "doc_id": " durable " }
        });
        assert_eq!(geojson_feature_id(&feature).as_deref(), Some("durable"));
    }
}
