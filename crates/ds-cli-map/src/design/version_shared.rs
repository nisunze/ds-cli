//! Shared validation and receipt shaping for transformer version history.

use ds_cli_contract::outcome::Failure;
use serde_json::{Value, json};

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
    if !ds_command_kernel::design_versions::valid_target(raw) {
        return Err(invalid_version(raw, allow_head));
    }
    Ok(raw)
}

fn invalid_version(raw: &str, allow_head: bool) -> Failure {
    let expected = if allow_head {
        "local-<digest>, v<number> or head"
    } else {
        "local-<digest> or v<number>"
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
        .remedy("run `ds design version list --project <project-id> --transformer <name>` and pass an exact playable v<number>"),
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

pub fn descriptor(raw: &Value, side: &str) -> Result<Value, Failure> {
    let kind = nonempty_text(raw, "kind")?;
    match kind {
        "version" => {
            let version_id = nonempty_text(raw, "versionId")?;
            canonical_version(version_id, false)?;
            Ok(json!({ "kind": "version", "version_id": version_id }))
        }
        "local_head" if side == "to" => {
            let revision = nonempty_text(raw, "revision")?;
            if !revision.starts_with("local-")
                || !ds_command_kernel::design_versions::valid_target(revision)
            {
                return Err(unreadable(
                    "local head requires an immutable content revision",
                ));
            }
            Ok(json!({"kind":"local_head","revision":revision}))
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

#[cfg(test)]
mod tests {
    use super::*;

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
