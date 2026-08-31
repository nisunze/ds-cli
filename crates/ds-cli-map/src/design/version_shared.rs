//! Shared validation and receipt shaping for transformer version history.

use ds_cli_contract::outcome::Failure;
use serde_json::{Map, Value, json};

pub const MAX_VERSION_ROWS: usize = 200;
pub const MAX_COMPARE_LAYERS: usize = 200;

pub const REFUSAL_MARKERS: &[(&str, &str)] = &[
    ("transformer_not_found", "transformer_not_found"),
    ("version_not_found", "version_not_found"),
    ("playback_unavailable", "playback_unavailable"),
    ("dirty_room", "dirty_room"),
    ("project_mismatch", "project_mismatch"),
    ("desktop_unreadable", "desktop_unreadable"),
];

pub fn canonical_version(raw: &str, allow_head: bool) -> Result<&str, Failure> {
    if allow_head && raw == "head" {
        return Ok(raw);
    }
    let Some(digits) = raw.strip_prefix('v') else {
        return Err(invalid_version(raw, allow_head));
    };
    if digits.is_empty()
        || digits.starts_with('0')
        || !digits.bytes().all(|byte| byte.is_ascii_digit())
        || digits.parse::<u64>().is_err()
    {
        return Err(invalid_version(raw, allow_head));
    }
    Ok(raw)
}

fn invalid_version(raw: &str, allow_head: bool) -> Failure {
    let expected = if allow_head {
        "v<number> or head"
    } else {
        "v<number>"
    };
    Failure::invalid(
        "invalid_version",
        format!("`{raw}` is not a canonical {expected} version reference"),
    )
    .remedy(format!(
        "pass {expected}; list exact retained versions first"
    ))
}

pub fn classify_failure(failure: Failure) -> Failure {
    let failure = crate::classify_design_failure(failure);
    if failure.code() == "auth_context_mismatch" {
        return Failure::conflict(
            "project_mismatch",
            "the paired application's active project changed during version navigation",
        )
        .remedy("open the intended exact project in DS GridDesign, verify `ds desktop status`, then retry");
    }
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|value| value["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match REFUSAL_MARKERS
        .iter()
        .find_map(|(marker, code)| detail.contains(marker).then_some(*code))
    {
        Some("transformer_not_found") => Failure::invalid(
            "transformer_not_found",
            "the exact transformer does not exist in the active project",
        )
        .remedy("run `ds map design list --output json` and pass one exact transformer name"),
        Some("version_not_found") => Failure::invalid(
            "version_not_found",
            "the exact transformer version does not exist",
        )
        .remedy("run `ds map design version list --transformer <name>` and pass an exact playable v<number>"),
        Some("playback_unavailable") => Failure::invalid(
            "playback_unavailable",
            "the retained version has metadata but no immutable playback snapshot",
        )
        .remedy("choose a version whose list row reports playback_available=true"),
        Some("dirty_room") => Failure::conflict(
            "dirty_room",
            "the active design room has unsaved local work",
        )
        .remedy("keep working there, or explicitly save or discard it before opening version playback"),
        Some("project_mismatch") => Failure::conflict(
            "project_mismatch",
            "the transformer version and active project do not match",
        )
        .remedy("open the intended exact project in DS GridDesign, verify `ds desktop status`, then retry"),
        Some("desktop_unreadable") => unreadable("the application did not publish a complete bounded version receipt"),
        _ => failure,
    }
}

pub fn unreadable(message: impl Into<String>) -> Failure {
    Failure::unavailable("desktop_unreadable", message)
        .remedy("restart DS GridDesign, reopen the exact project, and retry")
}

pub fn text<'a>(value: &'a Value, key: &str) -> Result<&'a str, Failure> {
    value[key]
        .as_str()
        .ok_or_else(|| unreadable(format!("the application omitted `{key}`")))
}

pub fn nonempty_text<'a>(value: &'a Value, key: &str) -> Result<&'a str, Failure> {
    text(value, key).and_then(|text| {
        (!text.is_empty())
            .then_some(text)
            .ok_or_else(|| unreadable(format!("the application returned an empty `{key}`")))
    })
}

pub fn nullable_text(value: &Value, key: &str) -> Result<Value, Failure> {
    match value.get(key) {
        Some(Value::String(text)) => Ok(Value::String(text.clone())),
        Some(Value::Null) => Ok(Value::Null),
        _ => Err(unreadable(format!(
            "the application omitted nullable `{key}`"
        ))),
    }
}

pub fn boolean(value: &Value, key: &str) -> Result<bool, Failure> {
    value[key]
        .as_bool()
        .ok_or_else(|| unreadable(format!("the application omitted `{key}`")))
}

pub fn count(value: &Value, key: &str) -> Result<u64, Failure> {
    value[key]
        .as_u64()
        .ok_or_else(|| unreadable(format!("the application omitted bounded count `{key}`")))
}

pub fn version_row(raw: &Value) -> Result<Value, Failure> {
    let version_id = nonempty_text(raw, "versionId")?;
    canonical_version(version_id, false)?;
    let version = count(raw, "version")?;
    if version == 0 || version_id != format!("v{version}") {
        return Err(unreadable(
            "the version id and assigned ordinal do not describe the same version",
        ));
    }
    Ok(json!({
        "version_id": version_id,
        "version": version,
        "reason": nullable_text(raw, "reason")?,
        "created_at": nullable_text(raw, "createdAt")?,
        "created_by": nullable_text(raw, "createdBy")?,
        "playback_available": boolean(raw, "playbackAvailable")?,
    }))
}

pub fn descriptor(raw: &Value, side: &str) -> Result<Value, Failure> {
    let kind = nonempty_text(raw, "kind")?;
    match kind {
        "version" => {
            let version_id = nonempty_text(raw, "versionId")?;
            canonical_version(version_id, false)?;
            Ok(json!({ "kind": "version", "version_id": version_id }))
        }
        "saved_head" if side == "to" => {
            let generation = count(raw, "generation")?;
            if generation == 0 {
                return Err(unreadable(
                    "the saved-head comparison generation must be positive",
                ));
            }
            Ok(json!({
                "kind": "saved_head",
                "generation": generation,
            }))
        }
        _ => Err(unreadable(format!(
            "the application returned an unsupported {side} comparison descriptor"
        ))),
    }
}

pub fn change_counts(raw: &Value) -> Result<Value, Failure> {
    Ok(json!({
        "unchanged": count(raw, "unchanged")?,
        "local_only": count(raw, "localOnly")?,
        "cloud_only": count(raw, "cloudOnly")?,
        "attribute_only_changed": count(raw, "attributeOnlyChanged")?,
        "geometry_only_changed": count(raw, "geometryOnlyChanged")?,
        "attribute_and_geometry_changed": count(raw, "attributeAndGeometryChanged")?,
        "ambiguous_unmatchable": count(raw, "ambiguousUnmatchable")?,
    }))
}

pub fn comparison_layers(raw: &Value) -> Result<Value, Failure> {
    let rows = raw
        .as_array()
        .ok_or_else(|| unreadable("the application omitted bounded comparison layers"))?;
    if rows.len() > MAX_COMPARE_LAYERS {
        return Err(unreadable(format!(
            "the application returned more than {MAX_COMPARE_LAYERS} comparison layers"
        )));
    }
    let shaped = rows
        .iter()
        .map(|row| {
            let counts = change_counts(&row["counts"])?;
            Ok(json!({
                "layer_name": nonempty_text(row, "layerName")?,
                "counts": counts,
            }))
        })
        .collect::<Result<Vec<Value>, Failure>>()?;
    Ok(Value::Array(shaped))
}

pub fn require_false(result: &Value, key: &str) -> Result<(), Failure> {
    if boolean(result, key)? {
        return Err(unreadable(format!(
            "the application reported `{key}=true` for a read-only version operation"
        )));
    }
    Ok(())
}

pub fn require_true(result: &Value, key: &str) -> Result<(), Failure> {
    if !boolean(result, key)? {
        return Err(unreadable(format!(
            "the application did not confirm `{key}=true`"
        )));
    }
    Ok(())
}

pub fn one_argument(key: &str, value: &str) -> Value {
    let mut args = Map::new();
    args.insert(key.into(), json!(value));
    Value::Object(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_version_metadata_stays_readable_when_optional_fields_are_null() {
        let row = version_row(&json!({
            "versionId":"v1","version":1,"reason":null,"createdAt":null,
            "createdBy":null,"playbackAvailable":false,
        }))
        .expect("legacy metadata row");
        assert_eq!(row["reason"], Value::Null);
        assert_eq!(row["created_at"], Value::Null);
        assert_eq!(row["created_by"], Value::Null);
        assert_eq!(row["playback_available"], false);
    }

    #[test]
    fn version_tokens_and_saved_head_generations_are_bounded() {
        assert_eq!(
            canonical_version("v18446744073709551616", false)
                .unwrap_err()
                .code(),
            "invalid_version"
        );
        assert_eq!(
            descriptor(&json!({"kind":"saved_head","generation":0}), "to")
                .unwrap_err()
                .code(),
            "desktop_unreadable"
        );
    }
}
