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
    when: "a visibility, style, scale, dock height, viewport, or action value is invalid",
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

macro_rules! native_profile_refusal {
    ($code:literal, $when:literal, $remedy:literal) => {
        Refusal {
            code: $code,
            when: $when,
            remedy: $remedy,
        }
    };
}

pub static VIEW: Command = Command {
    id: "map.profile.view",
    path: &["map", "profile", "view"],
    contract: 5,
    summary: "Read the paired Profile's exact visual state.",
    purpose: "Read the Profile occupant, dock height, viewport, selection and edit mode. This read changes neither view nor model.",
    chapter: Chapter::MapPresentation,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[TARGET_ARG, DESCRIPTOR_ARG],
    output: "occupant, model_id/model_name/revision, persisted, history, scale, visibility, height_px, viewport, surface {width,height,scale} (CSS pixels, native px/scene-unit; null unmounted), edit_mode, selection {entity_ids,primary,kind,structures [{id,number}]}. review: up to 256 cases, selected_case_index, display_case, marker counts/truncation. display: native state/rows; analysis: bounded receipt; analysis_state: due/current/off/not_applicable. scene_loaded: current scene; scene_revision: retained head. calculation: native policy,model_revision,computed_revision,required,scope; null before observation.",
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
    contract: 9,
    summary: "Set the paired Profile's visual state through typed CLI inputs.",
    purpose: "Set Profile appearance; omitted values stay. An open model is required except staged height/camera. Explicit rebuild/analyze runs a native background job while edits and observation continue. Only current history/display context admits results. Save is independent. First initialization is synchronous. Display changes needing calculation refuse until rebuild/analyze. Report visibility schedules no analysis. Case selection requires prepared results. Combine --display-case with --action rebuild/analyze to explicitly calculate the requested case.",
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
            "JSON booleans: review, ground, side_profiles, clearance_line, terrain_points, terrain_ordinates, grid, structures, wire, section_labels, span_distances, structure_labels, structure_numbers, structure_names, structure_comments, embedded, offset_embedment_height, route_deviation, structure_usage, section_state, section_usage, analysis.",
        ),
        Arg::value(
            "cable-colors",
            "<json-object>",
            "Per-cable wire colour {\"NAME\":\"#rrggbb\"}; null resets.",
        ),
        Arg::value(
            "styles",
            "<json-object>",
            "Style map: {\"ground\":{\"color\":\"#rrggbb\",\"width_mm\":0.5,\"line_type\":\"dashed\"},\"strain\":{\"symbol\":\"▲\"}}. Width 0.05..3 mm; solid/dashed/dotted/dash_dot. One visible symbol for strain/suspension/junction/unknown/structures/terrain_points. Other targets are visibility keys. Omitted fields retain; null target resets. Schema/targets: map profile view display.style_controls.",
        ),
        Arg::value(
            "height-px",
            "<pixels>",
            "Profile dock height, finite 1..10000 pixels; Rust requires both panes to remain visible.",
        ),
        Arg::value("zoom", "<ratio>", "Absolute Profile zoom, 0.2..1000000."),
        Arg::value(
            "zoom-window",
            "<native-json>",
            "Fit a CSS-pixel rectangle: {\"from\":{\"x\":100,\"y\":50},\"to\":{\"x\":600,\"y\":300}}. Read surface dimensions with map profile view. Use alone.",
        ),
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
    output: "Resulting view. Rebuild awaits native results; Analyze adds ran:true and evidence. Full results stay in Profile/Issues. Calculation never saves.",
    examples: &[Example {
        command: "ds map profile set --height-px 480 --vertical-exaggeration 5 --visibility '{\"ground\":true,\"wire\":false}' --action fit --output json",
        note: "Set controls after map profile view, then fit; model edits require explicit model/revision.",
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
        native_profile_refusal!(
            "profile_replay_model_mismatch",
            "the captured native case belongs to another model",
            "read the active Profile and retry with its native model/session context"
        ),
        native_profile_refusal!(
            "profile_calculation_required",
            "the requested weather case has no current prepared native result",
            "explicitly calculate it using --display-case with --action rebuild or analyze; selection and Save do not calculate"
        ),
        native_profile_refusal!(
            "profile_replay_stale",
            "authored history differs from the observed native base",
            "wait for authored observation, read map profile view, and explicitly retry calculation"
        ),
        native_profile_refusal!(
            "profile_calculation_busy",
            "a native calculation worker is still running, including cancelled noninterruptible work",
            "wait for that worker to finish; edits and observation remain available"
        ),
        native_profile_refusal!(
            "profile_calculation_capacity",
            "the native result or job identity capacity is reached",
            "read the native capacity detail and admit or cancel unused results"
        ),
        native_profile_refusal!(
            "profile_calculation_pending",
            "calculation has not produced a ready result",
            "poll the captured native job before admission"
        ),
        native_profile_refusal!(
            "profile_calculation_stale",
            "model history, display context or calculation identity changed",
            "retain authored edits and explicitly request calculation for the current Profile"
        ),
        native_profile_refusal!(
            "profile_calculation_unavailable",
            "the captured native job no longer exists",
            "read the current Profile and explicitly request a new calculation"
        ),
        native_profile_refusal!(
            "profile_calculation_cancelled",
            "the native calculation result was cancelled",
            "continue editing or explicitly request another calculation"
        ),
        native_profile_refusal!(
            "profile_calculation_failed",
            "the native calculation worker failed",
            "inspect the native failure and current Profile before retrying"
        ),
        native_profile_refusal!(
            "profile_publication_pending",
            "a native result still awaits confirmation or discard",
            "admit or discard that exact native publication"
        ),
        native_profile_refusal!(
            "profile_publication_stale",
            "publication history, calculated revision, axis or identity differs",
            "discard the captured result and use the current Profile context"
        ),
        native_profile_refusal!(
            "profile_publication_unavailable",
            "no captured native publication exists",
            "read the current Profile before another explicit action"
        ),
        native_profile_refusal!(
            "profile_publication_failed",
            "worker or native publication rollback refused",
            "inspect the native refusal; preserve edits and resolve the named publication before retrying"
        ),
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
        "colour",
        "color",
        "width",
        "line type",
        "symbol",
        "appearance",
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
    if let Some(raw) = inputs.value("styles") {
        if raw.len() > 65536 {
            return Err(invalid("styles exceeds 64 KiB"));
        }
        let value: Value = serde_json::from_str(raw)
            .map_err(|_| invalid("styles must be a native JSON object"))?;
        let check: ds_command_kernel::profile_display::DisplayPatch =
            serde_json::from_value(json!({"styles":value})).map_err(|e| invalid(e.to_string()))?;
        ds_command_kernel::profile_display::DisplayState::default()
            .patched(&check)
            .map_err(invalid)?;
        patch.insert("styles".into(), value);
    }
    if let Some(raw) = inputs.value("terrain") {
        let value: Value = serde_json::from_str(raw)
            .map_err(|_| invalid("terrain must be a complete native JSON object"))?;
        serde_json::from_value::<ds_command_kernel::profile_display::TerrainSettings>(
            value.clone(),
        )
        .map_err(|_| invalid("terrain must contain all four native numeric fields"))?;
        patch.insert("terrain".into(), value);
    }
    if let Some(raw) = inputs.value("cable-colors") {
        let value: Value = serde_json::from_str(raw)
            .map_err(|_| invalid("cable-colors must be a JSON object of cable names"))?;
        let colours: std::collections::BTreeMap<String, Option<String>> =
            serde_json::from_value(value.clone())
                .map_err(|_| invalid("cable-colors values must be #rrggbb or null"))?;
        if colours.is_empty() {
            return Err(invalid("cable-colors must name at least one cable"));
        }
        let check = ds_command_kernel::profile_display::DisplayPatch {
            cable_colors: colours,
            ..Default::default()
        };
        ds_command_kernel::profile_display::DisplayState::default()
            .patched(&check)
            .map_err(invalid)?;
        patch.insert("cable_colors".into(), value);
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
            json!(bounded(raw, "height-px", 1.0, 10_000.0)?),
        );
    }
    if let Some(raw) = inputs.value("zoom") {
        patch.insert(
            "zoom".to_owned(),
            json!(bounded(raw, "zoom", 0.2, 1_000_000.0)?),
        );
    }
    if let Some(raw) = inputs.value("zoom-window") {
        if raw.len() > 4096 {
            return Err(invalid(
                "zoom-window exceeds the bounded native request size",
            ));
        }
        let window: ds_canvas::viewport::ZoomWindow =
            serde_json::from_str(raw).map_err(|e| invalid(format!("zoom-window: {e}")))?;
        window.validate().map_err(invalid)?;
        patch.insert(
            "zoom_window".into(),
            serde_json::to_value(window).map_err(|e| invalid(e.to_string()))?,
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
    if patch.contains_key("zoom_window") && patch.len() != 1 {
        return Err(invalid(
            "zoom-window must be used alone without other Profile settings",
        ));
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
    fn styles_use_native_validation_and_forward_exact_patch() {
        let style = json!({"ground":{"color":"#ABCDEF","width_mm":0.8,"line_type":"dotted"},"strain":{"symbol":"▲"},"wire":null});
        let args = ["--styles".to_owned(), style.to_string()];
        let inputs = ds_cli_contract::args::parse(&SET, &args).unwrap();
        let patch = Value::Object(patch_from_inputs(&inputs).unwrap());
        assert_eq!(patch, json!({"styles":style}));
        assert_eq!(
            ds_cli_desktop::ops::undeclared_key(&crate::PROFILE_SET, &patch),
            None
        );
        for raw in [
            r#"{"ground":{"width_mm":0}}"#,
            r#"{"ground":{"symbol":"x"}}"#,
            r#"{"strain":{"line_type":"invented"}}"#,
            r#"{"bogus":null}"#,
        ] {
            let args = ["--styles".to_owned(), raw.to_owned()];
            let inputs = ds_cli_contract::args::parse(&SET, &args).unwrap();
            assert_eq!(
                patch_from_inputs(&inputs).unwrap_err().code(),
                "invalid_profile_view"
            );
        }
    }

    #[test]
    fn height_parser_accepts_inclusive_bounds_and_fractional_pixels() {
        for height in ["1", "480.5", "10000"] {
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
        for height in ["0", "10000.01", "NaN", "inf", "-inf", "1e309", "pixels", ""] {
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
    fn report_visibility_is_independent_of_explicit_calculation() {
        use ds_command_kernel::profile_display::{DisplayOption, DisplayState};
        // Owner, 2026-10-05: the analysis report is on by default; the kernel owns
        // the switch and the CLI forwards it unchanged for the Desktop to apply.
        assert!(DisplayState::default().visibility[&DisplayOption::Analysis]);
        for on in [false, true] {
            let args = ["--visibility".to_owned(), format!(r#"{{"analysis":{on}}}"#)];
            let inputs = ds_cli_contract::args::parse(&SET, &args).unwrap();
            let patch = Value::Object(patch_from_inputs(&inputs).unwrap());
            assert_eq!(patch, json!({"visibility": {"analysis": on}}));
            assert_eq!(
                ds_cli_desktop::ops::undeclared_key(&crate::PROFILE_SET, &patch),
                None
            );
        }
        assert!(parse_visibility(r#"{"analysis":"off"}"#).is_err());
        assert!(parse_visibility(r#"{"auto_analysis":false}"#).is_err());
        let visibility = SET.args.iter().find(|arg| arg.name == "visibility");
        assert!(visibility.unwrap().summary.contains("analysis."));
        assert!(SET.purpose.contains("Report visibility schedules no analysis"));
        assert!(
            VIEW.output
                .contains("analysis_state: due/current/off/not_applicable")
        );
        assert!(VIEW.output.contains("computed_revision,required,scope"));
    }

    #[test]
    fn background_calculation_and_publication_refusals_are_declared() {
        for code in [
            "profile_calculation_required",
            "profile_replay_stale",
            "profile_calculation_busy",
            "profile_calculation_capacity",
            "profile_calculation_pending",
            "profile_calculation_stale",
            "profile_calculation_unavailable",
            "profile_calculation_cancelled",
            "profile_calculation_failed",
            "profile_publication_pending",
            "profile_publication_stale",
            "profile_publication_unavailable",
            "profile_publication_failed",
        ] {
            assert!(
                SET.refusals.iter().any(|refusal| refusal.code == code),
                "{code}"
            );
        }
        assert!(SET.purpose.contains("while edits and observation continue"));
        assert!(SET.purpose.contains("Save is independent"));
    }

    #[test]
    fn cable_colours_cross_the_bridge_validated_by_the_kernel() {
        let args = [
            "--cable-colors",
            r##"{"ASTER 54.6":"#AABBCC","AAAC 34":null}"##,
        ]
        .map(str::to_owned);
        let inputs = ds_cli_contract::args::parse(&SET, &args).unwrap();
        let patch = patch_from_inputs(&inputs).unwrap();
        assert_eq!(
            patch["cable_colors"],
            json!({"ASTER 54.6":"#AABBCC","AAAC 34":null})
        );
        for bad in [r#"{"ASTER":"red"}"#, r#"{}"#, r##"{" ":"#aabbcc"}"##, "[]"] {
            let inputs =
                ds_cli_contract::args::parse(&SET, &["--cable-colors".into(), bad.into()]).unwrap();
            assert_eq!(
                patch_from_inputs(&inputs).unwrap_err().code(),
                "invalid_profile_view",
                "{bad}"
            );
        }
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
    #[test]
    fn window_camera_request_uses_native_validation_before_pairing() {
        let window = r#"{"from":{"x":100,"y":50},"to":{"x":600,"y":300}}"#;
        let args = ["--zoom-window", window].map(str::to_owned);
        let inputs = ds_cli_contract::args::parse(&SET, &args).unwrap();
        assert_eq!(
            patch_from_inputs(&inputs).unwrap()["zoom_window"],
            json!({"from":{"x":100.0,"y":50.0},"to":{"x":600.0,"y":300.0}})
        );
        for raw in [
            r#"{"from":{"x":0,"y":0},"to":{"x":0,"y":100}}"#,
            r#"{"from":{"x":0,"y":0},"to":{"x":100,"y":100},"extra":true}"#,
        ] {
            let args = ["--zoom-window", raw].map(str::to_owned);
            let inputs = ds_cli_contract::args::parse(&SET, &args).unwrap();
            assert_eq!(
                patch_from_inputs(&inputs).unwrap_err().code(),
                "invalid_profile_view"
            );
        }
        let args = ["--zoom-window", window, "--action", "fit"].map(str::to_owned);
        let inputs = ds_cli_contract::args::parse(&SET, &args).unwrap();
        assert_eq!(
            patch_from_inputs(&inputs).unwrap_err().code(),
            "invalid_profile_view"
        );
    }
}
