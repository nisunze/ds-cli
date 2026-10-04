//! Paired native Profile display commands; viewport and window sizing are local UI.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, TARGET_ARG};

const PROFILE_CLOSED: Refusal = Refusal {
    code: "profile_closed",
    when: "display, analysis, or selection has no open model Profile, or fit/rebuild has no open Profile",
    remedy: "open a model with ds dsgrid profile open, then retry",
};
const PROFILE_STYLES_UNAVAILABLE: Refusal = Refusal {
    code: "profile_styles_unavailable",
    when: "the API has no complete governed Profile pens or review colors",
    remedy: "seed the global DS Grid profile style document, then refresh project map styles",
};
const PROFILE_STYLES_INVALID: Refusal = Refusal {
    code: "profile_styles_invalid",
    when: "a governed Profile pen or review color is outside its typed bounds",
    remedy: "repair the global Profile style document through the style API",
};
const PROFILE_SCENE_UNAVAILABLE: Refusal = Refusal {
    code: "profile_scene_unavailable",
    when: "native profile projection cannot use this model package",
    remedy: "inspect the native refusal and model diagnostics",
};
const PROFILE_CASE_BASE_UNAVAILABLE: Refusal = Refusal {
    code: "profile_case_base_unavailable",
    when: "weather selection has no native base for the held Profile revision and display scale",
    remedy: "draw the current Profile revision and scale, then choose its weather case",
};
const PROFILE_CASE_UNAVAILABLE: Refusal = Refusal {
    code: "profile_case_unavailable",
    when: "the selected weather case or its native labels are unavailable for this model",
    remedy: "read map profile view and choose one of its enabled native cases",
};
const PROFILE_PACKAGE_INVALID: Refusal = Refusal {
    code: "profile_package_invalid",
    when: "the held checkpoint is not a valid DS Grid package",
    remedy: "inspect or reopen the working copy",
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
    contract: 2,
    summary: "Read the paired Profile's exact visual state.",
    purpose: "Reads the paired Profile occupant, dock height in pixels, viewport, selection and edit mode. Selection is transient UI context; the view and engineering model remain unchanged.",
    chapter: Chapter::MapPresentation,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[TARGET_ARG, DESCRIPTOR_ARG],
    output: "Profile occupant, model_id and revision (null without an open model), scale, visibility, height_px, viewport, edit_mode and selection. review contains the bounded native command projection: up to 256 cases with value/label/disabled, selected_case_index, display_case, marker_count and explicit case/label truncation. display contains native state and governed rows; analysis contains the bounded native command receipt. scene_loaded reports a revision-current scene. Selection includes entity_ids, primary, kind and structures [{id,number}].",
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
    contract: 4,
    summary: "Set the paired Profile's visual state through typed CLI inputs.",
    purpose: "Updates the paired Profile display; omitted values stay. Rust validates display-case, visibility and scale against an open model; viewport and dock height may be staged first. Review boxes and usage labels default on. Fit, rebuild and analyze are explicit. Analyze runs native structure, section and clearance checks at the held revision with the model-bound case envelope; missing inputs block Profile and Issues. Weather changes only the curve, not the engineering envelope. No project data changes.",
    chapter: Chapter::MapPresentation,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "terrain",
            "<json-object>",
            "Complete native terrain settings: corridor_half_width_m, gap_tolerance_m, ground_clearance_offset_m, lowest_wire_drop_m. All finite 0..10000; gap tolerance must be positive. Omitted terrain retains native settings.",
        ),
        Arg::value(
            "display-case",
            "<json-object>",
            "Native JSON: {\"mode\":\"greatest_sag\"}, {\"mode\":\"sag_reference\"}, or {\"mode\":\"analysis_case\",\"analysis_case_id\":\"<id>\"}. Read review.cases from map profile view; needs an open model.",
        ),
        Arg::value(
            "vertical-exaggeration",
            "<ratio>",
            "Display-only vertical exaggeration, 0.5..100.",
        ),
        Arg::value(
            "visibility",
            "<json-object>",
            "JSON booleans: review, ground, side_profiles, clearance_line, terrain_points, terrain_ordinates, grid, structures, wire, section_labels, span_distances, structure_labels, structure_numbers, structure_names, structure_comments, embedded, offset_embedment_height, route_deviation.",
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
            "<fit|rebuild|analyze>",
            "Fit, rebuild, or run native checks with blockers in Profile and Issues.",
        )
        .choices(&["fit", "rebuild", "analyze"]),
        TARGET_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The resulting exact visual state from the paired Profile, including height_px (dock height in pixels), with the applied patch and optional action. Analyze also returns ran:true and the bounded native analysis receipt; full evidence stays in Profile and Issues.",
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
        PROFILE_STYLES_UNAVAILABLE,
        PROFILE_STYLES_INVALID,
        PROFILE_SCENE_UNAVAILABLE,
        PROFILE_CASE_BASE_UNAVAILABLE,
        PROFILE_CASE_UNAVAILABLE,
        PROFILE_PACKAGE_INVALID,
        PROFILE_SELECTION_STALE,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::REFUSED,
    ],
    reference: Some("docs/reference/map.md"),
    search: &[
        "labels",
        "fit",
        "rebuild",
        "visibility",
        "usage",
        "clearance",
        "blockers",
    ],
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
        TARGET_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The paired Desktop's native selection receipt with the held model and revision, selected entity IDs and primary focus. The range follows the native scene's family order; no range is calculated by the CLI.",
    examples: &[Example {
        command: "ds map profile select --model <model-id> --revision <revision-id> --from <entity-id> --to <entity-id> --mode replace --desktop-descriptor ~/.local/share/rw.datasolutions.desktop.local-dev/cli-bridge.d/<instance-id>.json --output json",
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
        if inputs.value("action") == Some("analyze") {
            std::time::Duration::from_secs(300)
        } else {
            crate::UI_TIMEOUT
        },
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
    if let Some(raw) = inputs.value("terrain") {
        let value: Value = serde_json::from_str(raw)
            .map_err(|_| invalid("terrain must be a complete native JSON object"))?;
        serde_json::from_value::<ds_command_kernel::profile_display::TerrainSettings>(
            value.clone(),
        )
        .map_err(|_| invalid("terrain must contain all four native numeric fields"))?;
        patch.insert("terrain".into(), value);
    }
    if let Some(raw) = inputs.value("display-case") {
        if raw.len() > 4096 {
            return Err(invalid(
                "display-case exceeds the bounded native request size",
            ));
        }
        let value: Value = serde_json::from_str(raw)
            .map_err(|_| invalid("display-case must be a native JSON object"))?;
        if !value.is_object() {
            return Err(invalid("display-case must be a native JSON object"));
        }
        patch.insert("display_case".to_owned(), value);
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
        if !matches!(action, "fit" | "rebuild" | "analyze") {
            return Err(invalid("action must be fit, rebuild or analyze"));
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
        if serde_json::from_value::<ds_command_kernel::profile_display::DisplayOption>(json!(key))
            .is_err()
            || !value.is_boolean()
        {
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
    fn display_case_is_forwarded_verbatim_for_native_validation() {
        let args = [
            "--display-case",
            r#"{"mode":"analysis_case","analysis_case_id":"cold"}"#,
        ]
        .map(str::to_owned);
        let inputs = ds_cli_contract::args::parse(&SET, &args).unwrap();
        let patch = Value::Object(patch_from_inputs(&inputs).unwrap());
        assert_eq!(
            patch,
            json!({"display_case": {"mode":"analysis_case", "analysis_case_id":"cold"}})
        );
        assert_eq!(
            ds_cli_desktop::ops::undeclared_key(&crate::PROFILE_SET, &patch),
            None
        );
        for raw in ["[]", "null", "broken"] {
            let args = ["--display-case".to_owned(), raw.to_owned()];
            let inputs = ds_cli_contract::args::parse(&SET, &args).unwrap();
            assert_eq!(
                patch_from_inputs(&inputs).unwrap_err().code(),
                "invalid_profile_view"
            );
        }
    }

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
            parse_visibility(r#"{"ground":false,"wire":true,"route_deviation":false,"offset_embedment_height":true}"#)
                .unwrap()
                .len(),
            4
        );
        assert!(parse_visibility(r#"{"ground":"false"}"#).is_err());
        assert!(parse_visibility(r#"{"bogus":true}"#).is_err());
    }

    #[test]
    fn native_review_flags_and_complete_terrain_cross_the_bridge_without_defaults() {
        let args = ["--visibility", r#"{"review":false,"terrain_ordinates":true,"structure_comments":false}"#,
            "--terrain", r#"{"corridor_half_width_m":20,"gap_tolerance_m":80,"ground_clearance_offset_m":6,"lowest_wire_drop_m":2}"#].map(str::to_owned);
        let inputs = ds_cli_contract::args::parse(&SET, &args).unwrap();
        let patch = patch_from_inputs(&inputs).unwrap();
        assert_eq!(
            patch["visibility"],
            json!({"review":false,"terrain_ordinates":true,"structure_comments":false})
        );
        assert_eq!(patch["terrain"]["lowest_wire_drop_m"], 2);
        assert_eq!(patch.len(), 2);
        let inputs =
            ds_cli_contract::args::parse(&SET, &["--terrain".into(), "{}".into()]).unwrap();
        assert_eq!(
            patch_from_inputs(&inputs).unwrap_err().code(),
            "invalid_profile_view"
        );
        let analyze =
            ds_cli_contract::args::parse(&SET, &["--action".into(), "analyze".into()]).unwrap();
        let patch = Value::Object(patch_from_inputs(&analyze).unwrap());
        assert_eq!(patch, json!({"action": "analyze"}));
        assert_eq!(
            ds_cli_desktop::ops::undeclared_key(&crate::PROFILE_SET, &patch),
            None
        );
        assert_eq!(SET.effect, Effect::LocalUi);
        assert_eq!(SET.authority, Authority::DesktopPairing);
        assert!(
            ds_cli_contract::args::parse(&SET, &["--action".into(), "compute-arbitrary".into()])
                .is_err()
        );
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
