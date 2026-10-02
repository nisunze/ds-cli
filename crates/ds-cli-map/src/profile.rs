//! Paired Profile presentation controls. View settings can be staged before
//! the Profile opens; fit/rebuild require an open surface. Model-authored label
//! composition is owned by dsgrid profile labels, not this UI bridge.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

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
    when: "fit or rebuild has no open Profile, or selection has no open model Profile and scene",
    remedy: "open a model with ds dsgrid profile open, then retry",
};
const INVALID_PROFILE_VIEW: Refusal = Refusal {
    code: "invalid_profile_view",
    when: "a visibility, scale, dock height, viewport, or action value is invalid",
    remedy: "use the exact fields and ranges in ds map profile set --help",
};
const INVALID_PROFILE_SELECTION: Refusal = Refusal {
    code: "invalid_profile_selection",
    when: "an endpoint ID or mode is invalid, the endpoints are unknown or cross families, or the native scene refuses the range",
    remedy: "read map profile view for the current model and use IDs from one scene family with replace, add, remove or intersect",
};
const PROFILE_SELECTION_STALE: Refusal = Refusal {
    code: "profile_selection_stale",
    when: "the named model or revision differs from the open Profile or its held scene changed during selection",
    remedy: "read map profile view again, use its model_id and revision, and retry against the current scene",
};

pub static VIEW: Command = Command {
    id: "map.profile.view",
    path: &["map", "profile", "view"],
    contract: 1,
    summary: "Read the paired Profile's exact visual state.",
    purpose: "Returns the Profile occupant, dock height in pixels, viewport, selected entities and edit mode from the running Desktop. Selection is transient UI context, not an engineering model read; this command does not change the view or the model.",
    chapter: Chapter::MapPresentation,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[DESCRIPTOR_ARG],
    output: "The paired Profile's occupant, model_id and revision (null without an open model), scale, visibility, height_px (dock height in pixels), viewport, edit_mode boolean and selection {entity_ids, primary, kind, structures:[{id,number}]}. The legacy model field also remains. Selection IDs follow engine order for one family; mixed selections retain the selected IDs. kind is none, structures, tension_sections, alignments, terrain_points, attachment_points or mixed. structures is populated only for a structures selection; number is the displayed structure number or null when unknown.",
    examples: &[Example {
        command: "ds map profile view --output json",
        note: "Read the live Profile's viewport, selection and edit mode before a scoped model command.",
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
    search: &["viewport"],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub static SET: Command = Command {
    id: "map.profile.set",
    path: &["map", "profile", "set"],
    contract: 1,
    summary: "Set the paired Profile's visual state through typed CLI inputs.",
    purpose: "Patches only the named Profile display settings in the running Desktop, even before the Profile opens; omitted settings stay unchanged. The visibility object uses the documented concise keys and boolean values. Fit and rebuild are explicit actions, and the receipt returns the resulting live state including height_px (dock height in pixels). Read map profile view, set the dock height, then use map profile select with the returned model_id, revision and entity IDs. No engineering model is changed.",
    chapter: Chapter::MapPresentation,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
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
        Arg::value(
            "height-px",
            "<pixels>",
            "Profile dock height, finite 220..10000 pixels.",
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
    output: "The resulting exact visual state from the paired Profile, including height_px (dock height in pixels), with the applied patch and optional action.",
    examples: &[Example {
        command: "ds map profile set --height-px 480 --vertical-exaggeration 5 --visibility '{\"ground\":true,\"wire\":false}' --action fit --output json",
        note: "After map profile view, set dock height, scale and visibility, then fit; use map profile select with the view receipt's model_id, revision and entity IDs.",
        runnable: false,
    }],
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
    search: &["labels", "fit", "rebuild", "visibility"],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub static SELECT: Command = Command {
    id: "map.profile.select",
    path: &["map", "profile", "select"],
    contract: 1,
    summary: "Select one entity or a same-family range in the paired model Profile.",
    purpose: "Changes only the paired Desktop's transient Profile selection. The Desktop checks the explicitly named model and revision against its held scene, asks the native scene for the inclusive same-family range, and applies the named set mode. Use the same ID for --from and --to to select one entity. No engineering model is changed.",
    chapter: Chapter::MapPresentation,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<model-id>", "Exact ID of the open model Profile.").required(),
        Arg::value(
            "revision",
            "<revision-id>",
            "Expected held model revision from map profile view.",
        )
        .required(),
        Arg::value(
            "from",
            "<entity-id>",
            "First Profile entity ID; an inclusive range endpoint.",
        )
        .required(),
        Arg::value(
            "to",
            "<entity-id>",
            "Last Profile entity ID; use --from's ID for one entity.",
        )
        .required(),
        Arg::value(
            "mode",
            "<replace|add|remove|intersect>",
            "How to combine the native range with the current selection.",
        )
        .choices(&["replace", "add", "remove", "intersect"])
        .default("replace"),
        DESCRIPTOR_ARG,
    ],
    output: "The paired Desktop's native selection receipt with the held model and revision, selected entity IDs and primary focus. The range follows the native scene's family order; no range is calculated by the CLI.",
    examples: &[Example {
        command: "ds map profile select --model <model-id> --revision <revision-id> --from <entity-id> --to <entity-id> --mode replace --desktop-descriptor ~/.local/share/rw.datasolutions.desktop.local-dev/cli-bridge.json --output json",
        note: "Select one entity by using the same ID twice; the explicit descriptor addresses the local development Desktop without discovery.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        PROFILE_CLOSED,
        PROFILE_SELECTION_STALE,
        INVALID_PROFILE_SELECTION,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::REFUSED,
    ],
    reference: Some("docs/reference/map.md"),
    search: &[
        "selection",
        "structures",
        "tension sections",
        "terrain points",
        "attachment points",
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

pub fn select(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = selection_request(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::PROFILE_SELECT,
        request,
        crate::UI_TIMEOUT,
    )
}

fn selection_request(inputs: &Inputs) -> Result<Value, Failure> {
    let model = selection_id(inputs.require("model")?, "model")?;
    let revision = selection_id(inputs.require("revision")?, "revision")?;
    let from = selection_id(inputs.require("from")?, "from")?;
    let to = selection_id(inputs.require("to")?, "to")?;
    let mode = inputs.value("mode").unwrap_or("replace");
    if !matches!(mode, "replace" | "add" | "remove" | "intersect") {
        return Err(invalid_selection(
            "mode must be replace, add, remove or intersect",
        ));
    }
    Ok(json!({
        "model_id": model,
        "expected_revision": revision,
        "from_entity_id": from,
        "to_entity_id": to,
        "mode": mode,
    }))
}

fn selection_id<'a>(raw: &'a str, flag: &str) -> Result<&'a str, Failure> {
    if raw.trim().is_empty() || raw.len() > 200 || raw.chars().any(char::is_control) {
        return Err(invalid_selection(format!(
            "--{flag} must be a nonblank ID of at most 200 bytes without control characters"
        )));
    }
    Ok(raw)
}

fn invalid_selection(message: impl Into<String>) -> Failure {
    Failure::invalid("invalid_profile_selection", message.into())
}

fn patch_from_inputs(inputs: &Inputs) -> Result<Map<String, Value>, Failure> {
    let mut patch = Map::new();
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
    if let Some(raw) = inputs.value("height-px") {
        patch.insert(
            "height_px".to_owned(),
            json!(bounded(raw, "height-px", 220.0, 10_000.0)?),
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

pub fn render_selection(data: &Value) -> String {
    format!("profile selection {}\n", data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_parser_accepts_inclusive_bounds_and_fractional_pixels() {
        for height in ["220", "480.5", "10000"] {
            let args = ["--height-px", height].map(str::to_owned);
            let inputs = ds_cli_contract::args::parse(&SET, &args).expect("declared height");
            let patch = patch_from_inputs(&inputs).expect("valid height");
            assert_eq!(
                ds_cli_desktop::ops::undeclared_key(
                    &crate::PROFILE_SET,
                    &Value::Object(patch.clone())
                ),
                None,
                "height payload must be admitted by the desktop bridge"
            );
            assert_eq!(
                patch,
                json!({"height_px": height.parse::<f64>().unwrap()})
                    .as_object()
                    .unwrap()
                    .clone()
            );
        }
    }

    #[test]
    fn height_parser_rejects_invalid_values_before_pairing() {
        for height in [
            "219.99", "10000.01", "NaN", "inf", "-inf", "1e309", "pixels", "",
        ] {
            let args = [format!("--height-px={height}")];
            let inputs = ds_cli_contract::args::parse(&SET, &args).expect("declared height");
            assert_eq!(
                patch_from_inputs(&inputs).unwrap_err().code(),
                "invalid_profile_view",
                "height {height:?}"
            );
        }
        let args = ["--height-px".to_owned()];
        assert!(ds_cli_contract::args::parse(&SET, &args).is_err());
    }

    #[test]
    fn height_is_omitted_when_not_requested() {
        let args = ["--zoom", "2"].map(str::to_owned);
        let inputs = ds_cli_contract::args::parse(&SET, &args).expect("declared zoom");
        assert_eq!(
            patch_from_inputs(&inputs).unwrap(),
            json!({"zoom": 2.0}).as_object().unwrap().clone()
        );
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

    #[test]
    fn selection_request_preserves_opaque_ids_and_defaults_to_replace() {
        let from = r#"["attachment-1","pole-2"]"#;
        let args = [
            "--model",
            "model-a",
            "--revision",
            "revision-b",
            "--from",
            from,
            "--to",
            from,
        ]
        .map(str::to_owned);
        let inputs = ds_cli_contract::args::parse(&SELECT, &args).expect("declared inputs");
        assert_eq!(
            selection_request(&inputs).expect("selection request"),
            json!({
                "model_id": "model-a",
                "expected_revision": "revision-b",
                "from_entity_id": from,
                "to_entity_id": from,
                "mode": "replace",
            })
        );
        let override_args = [
            "--model",
            "model-a",
            "--revision",
            "revision-b",
            "--from",
            from,
            "--to",
            from,
            "--mode",
            "intersect",
        ]
        .map(str::to_owned);
        let override_inputs =
            ds_cli_contract::args::parse(&SELECT, &override_args).expect("declared mode");
        assert_eq!(
            selection_request(&override_inputs).expect("selection request")["mode"],
            "intersect"
        );
    }

    #[test]
    fn selection_request_rejects_blank_or_oversized_ids_before_pairing() {
        let oversized = "x".repeat(201);
        for id in [" ", "\n", oversized.as_str()] {
            let args = [
                "--model",
                "model-a",
                "--revision",
                "revision-b",
                "--from",
                id,
                "--to",
                "pole-2",
            ]
            .map(str::to_owned);
            let inputs = ds_cli_contract::args::parse(&SELECT, &args).expect("declared inputs");
            assert_eq!(
                selection_request(&inputs).unwrap_err().code(),
                "invalid_profile_selection"
            );
        }
    }
}
