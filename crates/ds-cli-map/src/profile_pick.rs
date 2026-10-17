//! Read-only picking through the mounted Profile's existing native index.
//! The lens transports CSS coordinates; the canvas owns its camera inverse
//! and Rust owns candidate geometry, attachment occurrences and ordering.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "map.profile.pick",
    path: &["map", "profile", "pick"],
    contract: 1,
    summary: "Read native hits at a point in the mounted model Profile.",
    purpose: "Inspect ground points, attachment occurrences, shafts, wires and alignments at CSS coordinates relative to the Profile canvas. Use the model, exact native history and opaque pick_context from map profile view. The mounted canvas supplies its actual camera, visibility and existing pointer tolerance; Rust supplies complete overlap ordering. This reads the held index, without selection, calculation, observation, Save or acquisition. A changed model, history, scene, viewport or mounted context refuses.",
    chapter: Chapter::MapPresentation,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "model",
            "<model-id>",
            "Exact open model from map profile view.",
        )
        .required(),
        Arg::value(
            "history",
            "<native-json>",
            "Complete native history from map profile view; maximum 2 KiB.",
        )
        .required(),
        Arg::value(
            "context",
            "<pick-context>",
            "Opaque mounted pick_context from the same map profile view.",
        )
        .required(),
        Arg::value(
            "x",
            "<css-px>",
            "Finite horizontal coordinate relative to the canvas left edge.",
        )
        .required(),
        Arg::value(
            "y",
            "<css-px>",
            "Finite vertical coordinate relative to the canvas top edge.",
        )
        .required(),
        crate::TARGET_ARG,
        crate::DESCRIPTOR_ARG,
    ],
    output: "Exact model/history/pick_context and native model_revision, axis_pin_digest, indexed_segment_count and ordered candidates. Each candidate has entity_id, kind, distance_scene_units, distance_px and optional attachment_ref (structure_id + attachment_point_id). Complete finite ordering, no silent truncation; the existing 8 MiB Desktop response transport ceiling returns desktop_unreadable if exceeded. No selection or model state changes.",
    examples: &[Example {
        command: "ds map profile pick --model <model-id> --history '<native-history-json>' --context <pick-context> --x 420 --y 180 --output json",
        note: "Read map profile view first; coordinates use its mounted CSS canvas, without inventing a scene transform.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::REFUSED,
        Refusal {
            code: "profile_closed",
            when: "no model Profile is open",
            remedy: "open a model Profile",
        },
        Refusal {
            code: "profile_scene_unavailable",
            when: "the actual model canvas or held native pick index is unavailable",
            remedy: "wait for the admitted Profile canvas, then read map profile view",
        },
        Refusal {
            code: "profile_selection_stale",
            when: "account, project, model, native history, scene, camera, visibility or mounted context changes",
            remedy: "read map profile view again and use its exact current context",
        },
        Refusal {
            code: "profile_pick_scene_stale",
            when: "the native revision or cached axis no longer matches",
            remedy: "wait for native Profile observation, then read map profile view again",
        },
        Refusal {
            code: "profile_pick_invalid_request",
            when: "native point, camera scale, tolerance or axis is invalid",
            remedy: "use the current mounted context and finite CSS coordinates",
        },
        Refusal {
            code: "invalid_profile_pick",
            when: "transport fields, model, history, context or CSS coordinates are invalid",
            remedy: "use the exact fields declared by map profile pick and history from map profile view",
        },
    ],
    reference: Some("docs/reference/map.md"),
    search: &[
        "hit",
        "query",
        "selectable",
        "ground point",
        "attachment point",
        "shaft",
        "overlap",
    ],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = pick_request(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::PROFILE_PICK,
        request,
        crate::UI_TIMEOUT,
    )
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("invalid_profile_pick", message.into())
}

fn identity<'a>(value: &'a str, name: &str, maximum: usize) -> Result<&'a str, Failure> {
    if value.trim().is_empty() || value.len() > maximum || value.chars().any(char::is_control) {
        return Err(invalid(format!(
            "{name} must be a nonblank bounded identity"
        )));
    }
    Ok(value)
}

fn coordinate(inputs: &Inputs, name: &str) -> Result<f64, Failure> {
    inputs
        .require(name)?
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .ok_or_else(|| invalid(format!("{name} must be a finite CSS coordinate")))
}

fn pick_request(inputs: &Inputs) -> Result<Value, Failure> {
    let model = identity(inputs.require("model")?, "model", 200)?;
    let context = identity(inputs.require("context")?, "context", 200)?;
    let raw = inputs.require("history")?;
    if raw.len() > 2048 {
        return Err(invalid("history exceeds the 2 KiB transport bound"));
    }
    let history: ds_network_contracts::profile::ProfileHistory =
        serde_json::from_str(raw).map_err(|error| invalid(error.to_string()))?;
    Ok(
        json!({"model_id": model, "expected_history": history, "pick_context": context,
        "screen_x": coordinate(inputs, "x")?, "screen_y": coordinate(inputs, "y")?}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inputs(x: &str, history: &str) -> Inputs {
        ds_cli_contract::args::parse(
            &COMMAND,
            &[
                "--model",
                "model-a",
                "--history",
                history,
                "--context",
                "mounted-context",
                "--x",
                x,
                "--y",
                "180",
            ]
            .iter()
            .map(|value| (*value).into())
            .collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn transports_native_history_and_css_point_without_scene_or_visibility_overrides() {
        let history = json!({"model_revision":"head","initial_revision":"initial","undo_depth":2,"redo_depth":1,"history_pin":"exact-prefix"});
        let wire = pick_request(&inputs("420.5", &history.to_string())).unwrap();
        assert_eq!(
            wire,
            json!({"model_id":"model-a","expected_history":history,"pick_context":"mounted-context","screen_x":420.5,"screen_y":180.0})
        );
        assert_eq!(
            ds_cli_desktop::ops::undeclared_key(&crate::PROFILE_PICK, &wire),
            None
        );
        assert_eq!(COMMAND.effect, Effect::ReadOnly);
    }
    #[test]
    fn malformed_history_and_nonfinite_points_refuse_before_pairing() {
        let history = r#"{"model_revision":"head","initial_revision":"initial","undo_depth":2,"redo_depth":1,"history_pin":"prefix"}"#;
        for x in ["NaN", "inf", "-inf", "not-a-coordinate"] {
            assert_eq!(
                pick_request(&inputs(x, history)).unwrap_err().code(),
                "invalid_profile_pick"
            );
        }
        for raw in [
            "{}",
            r#"{"model_revision":"head","initial_revision":"initial","undo_depth":2,"redo_depth":1,"history_pin":"prefix","scene_x":3}"#,
        ] {
            assert_eq!(
                pick_request(&inputs("1", raw)).unwrap_err().code(),
                "invalid_profile_pick"
            );
        }
    }
}
