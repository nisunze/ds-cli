//! Paired Profile presentation controls. The profile must already be open in
//! the local Desktop; these commands never alter the engineering model.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

const LABEL_FIELDS: &[&str] = &[
    "number",
    "station",
    "type",
    "height",
    "alignment",
    "comment1",
    "comment2",
    "comment3",
];
const ORIENTATIONS: &[&str] = &["auto", "right", "left", "above", "below", "vertical"];
const VISIBILITY_KEYS: &[&str] = &[
    "ground",
    "side_profiles",
    "clearance_line",
    "terrain_points",
    "terrain_ordinates",
    "grid",
    "structures",
    "wire",
    "section_labels",
    "span_distances",
    "structure_labels",
];

const PROFILE_CLOSED: Refusal = Refusal {
    code: "profile_closed",
    when: "the paired Desktop has no open Profile",
    remedy: "open a model with ds dsgrid profile open, then retry",
};
const INVALID_PROFILE_VIEW: Refusal = Refusal {
    code: "invalid_profile_view",
    when: "a label, orientation, visibility, scale, viewport, or action value is invalid",
    remedy: "use the exact fields and ranges in ds map profile set --help",
};

pub static VIEW: Command = Command {
    id: "map.profile.view",
    path: &["map", "profile", "view"],
    contract: 1,
    summary: "Read the paired Profile's exact visual state.",
    purpose: "Returns the Profile occupant and current labels, orientation, vertical exaggeration, visibility, zoom and pan from the running Desktop. Reads no engineering model and does not change the view.",
    chapter: Chapter::MapPresentation,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[DESCRIPTOR_ARG],
    output: "The paired Profile's exact visual state, including occupant, labels, orientation, scale, visibility and viewport.",
    examples: &[Example {
        command: "ds map profile view --output json",
        note: "Read the live Profile's visual state before changing it.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        PROFILE_CLOSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
    ],
    reference: Some("docs/reference/map.md"),
    search: &["profile", "visual", "labels", "viewport"],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub static SET: Command = Command {
    id: "map.profile.set",
    path: &["map", "profile", "set"],
    contract: 1,
    summary: "Set the paired Profile's visual state through typed CLI inputs.",
    purpose: "Patches only the named Profile display settings in the running Desktop; omitted settings stay unchanged. Labels are an ordered list. The visibility object uses the documented concise keys and boolean values. Fit and rebuild are explicit actions, and the receipt returns the resulting live state. No engineering model is changed.",
    chapter: Chapter::MapPresentation,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "labels",
            "<comma-separated-fields>",
            "Ordered profile structure label fields: number,station,type,height,alignment,comment1,comment2,comment3.",
        ),
        Arg::value(
            "orientation",
            "<orientation>",
            "Profile structure label placement.",
        )
        .choices(ORIENTATIONS),
        Arg::value(
            "vertical-exaggeration",
            "<ratio>",
            "Display-only vertical exaggeration, 0.5..100.",
        ),
        Arg::value(
            "visibility",
            "<json-object>",
            "JSON object with ground, wire, grid, structures and the other documented profile layer keys and boolean values.",
        ),
        Arg::value("zoom", "<ratio>", "Absolute Profile zoom, 0.2..1000000."),
        Arg::value(
            "pan-x",
            "<pixels>",
            "Absolute horizontal viewport pan, -1000000..1000000.",
        ),
        Arg::value(
            "pan-y",
            "<pixels>",
            "Absolute vertical viewport pan, -1000000..1000000.",
        ),
        Arg::value(
            "action",
            "<fit|rebuild>",
            "Fit the complete Profile or rebuild its projected scene.",
        )
        .choices(&["fit", "rebuild"]),
        DESCRIPTOR_ARG,
    ],
    output: "The resulting exact visual state from the paired Profile, with the applied patch and optional action.",
    examples: &[
        Example {
            command: "ds map profile set --labels number,type,comment1,comment2,comment3 --output json",
            note: "Compose the requested structure labels without chainage.",
            runnable: false,
        },
        Example {
            command: "ds map profile set --vertical-exaggeration 5 --visibility '{\"ground\":true,\"wire\":false}' --action fit --output json",
            note: "Set scale and visibility, then fit the complete Profile.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        PROFILE_CLOSED,
        INVALID_PROFILE_VIEW,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
    ],
    reference: Some("docs/reference/map.md"),
    search: &[
        "profile",
        "visual",
        "labels",
        "fit",
        "rebuild",
        "visibility",
    ],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub fn view(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::PROFILE_VIEW,
        json!({}),
        crate::UI_TIMEOUT,
    )
}

pub fn set(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let patch = patch_from_inputs(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::PROFILE_SET,
        Value::Object(patch),
        crate::UI_TIMEOUT,
    )
}

fn patch_from_inputs(inputs: &Inputs) -> Result<Map<String, Value>, Failure> {
    let mut patch = Map::new();
    if let Some(raw) = inputs.value("labels") {
        patch.insert("labels".to_owned(), json!(parse_labels(raw)?));
    }
    if let Some(orientation) = inputs.value("orientation") {
        if !ORIENTATIONS.contains(&orientation) {
            return Err(invalid(
                "orientation must be auto, right, left, above, below, or vertical",
            ));
        }
        patch.insert("orientation".to_owned(), json!(orientation));
    }
    if let Some(raw) = inputs.value("vertical-exaggeration") {
        patch.insert(
            "vertical_exaggeration".to_owned(),
            json!(bounded(raw, "vertical-exaggeration", 0.5, 100.0)?),
        );
    }
    if let Some(raw) = inputs.value("visibility") {
        patch.insert(
            "visibility".to_owned(),
            Value::Object(parse_visibility(raw)?),
        );
    }
    if let Some(raw) = inputs.value("zoom") {
        patch.insert(
            "zoom".to_owned(),
            json!(bounded(raw, "zoom", 0.2, 1_000_000.0)?),
        );
    }
    for (flag, key) in [("pan-x", "pan_x"), ("pan-y", "pan_y")] {
        if let Some(raw) = inputs.value(flag) {
            patch.insert(
                key.to_owned(),
                json!(bounded(raw, flag, -1_000_000.0, 1_000_000.0)?),
            );
        }
    }
    if let Some(action) = inputs.value("action") {
        if !matches!(action, "fit" | "rebuild") {
            return Err(invalid("action must be fit or rebuild"));
        }
        patch.insert("action".to_owned(), json!(action));
    }
    if patch.is_empty() {
        return Err(invalid("provide at least one Profile setting or --action"));
    }
    Ok(patch)
}

fn parse_labels(raw: &str) -> Result<Vec<&str>, Failure> {
    if raw.is_empty() {
        return Ok(Vec::new());
    }
    let fields: Vec<&str> = raw.split(',').map(str::trim).collect();
    if fields.iter().any(|field| !LABEL_FIELDS.contains(field)) {
        return Err(invalid("labels contains an unknown or empty field"));
    }
    let mut seen = std::collections::BTreeSet::new();
    if fields.iter().any(|field| !seen.insert(*field)) {
        return Err(invalid("labels must not repeat a field"));
    }
    Ok(fields)
}

fn parse_visibility(raw: &str) -> Result<Map<String, Value>, Failure> {
    let value: Value = serde_json::from_str(raw)
        .map_err(|_| invalid("visibility must be a JSON object of booleans"))?;
    let object = value
        .as_object()
        .ok_or_else(|| invalid("visibility must be a JSON object of booleans"))?;
    if object.is_empty() {
        return Err(invalid("visibility must name at least one setting"));
    }
    for (key, value) in object {
        if !VISIBILITY_KEYS.contains(&key.as_str()) || !value.is_boolean() {
            return Err(invalid(
                "visibility accepts only documented profile layer boolean settings",
            ));
        }
    }
    Ok(object.clone())
}

fn bounded(raw: &str, field: &str, min: f64, max: f64) -> Result<f64, Failure> {
    let value: f64 = raw
        .parse()
        .map_err(|_| invalid(format!("{field} must be a number")))?;
    if !value.is_finite() || !(min..=max).contains(&value) {
        return Err(invalid(format!("{field} must be between {min} and {max}")));
    }
    Ok(value)
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("invalid_profile_view", message.into())
}

pub fn render(data: &Value) -> String {
    format!("profile visual state {}\n", data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordered_labels_accept_comments_and_reject_drift() {
        assert_eq!(
            parse_labels("number,type,comment1,comment2,comment3").unwrap(),
            vec!["number", "type", "comment1", "comment2", "comment3"]
        );
        assert!(parse_labels("number,number").is_err());
        assert!(parse_labels("number,comment4").is_err());
    }

    #[test]
    fn visibility_requires_exact_boolean_keys() {
        assert_eq!(
            parse_visibility(r#"{"ground":false,"wire":true}"#)
                .unwrap()
                .len(),
            2
        );
        assert!(parse_visibility(r#"{"ground":"false"}"#).is_err());
        assert!(parse_visibility(r#"{"bogus":true}"#).is_err());
    }
}
