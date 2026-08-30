//! `ds map design open` — enter one transformer's visible edit context.
//!
//! The running application owns navigation, room loading, dirty-room safety,
//! and the session snapshot reported by `ds desktop status`. The CLI sends
//! only the exact transformer name and shapes the application's bounded
//! readiness receipt; it never reads or reconstructs room features.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;
use crate::design::TRANSFORMER_ARG;

pub static COMMAND: Command = Command {
    id: "map.design.open",
    path: &["map", "design", "open"],
    contract: 1,
    summary: "Open a transformer's visible edit context.",
    purpose: "Navigate the paired DS GridDesign application to one exact transformer's real edit/map context and wait until it is fully loaded. This is the canonical command for open transformer, transformer edit context, and activate design intents. Reopening the active transformer is idempotent. A different transformer is never opened over a dirty room: the application refuses instead of discarding, saving, or replacing work.",
    chapter: Chapter::Design,
    effect: Effect::LocalUi,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, DESCRIPTOR_ARG],
    output: "A bounded context receipt: project, transformer, context type, previous context, whether it changed, editor and map readiness, dirty state, and explicit staged=false and persisted=false mutation facts.",
    examples: &[Example {
        command: "ds map design open --transformer agasharu --output json",
        note: "Makes the paired application visibly enter agasharu's loaded editor without changing project data.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::SIGNED_OUT,
        Refusal {
            code: "desktop_refused",
            when: "the application answered but could not complete navigation",
            remedy: "read detail.detail for the application's precise message, then retry only after resolving it",
        },
        Refusal {
            code: "transformer_not_found",
            when: "the exact transformer name does not exist in the active project",
            remedy: "run `ds map design list --output json` and pass one exact transformer name",
        },
        Refusal {
            code: "project_mismatch",
            when: "the requested or previously active design context belongs to another project",
            remedy: "open the intended exact project in DS GridDesign, verify `ds desktop status`, then retry",
        },
        Refusal {
            code: "dirty_room",
            when: "another transformer's active room has unsaved local work",
            remedy: "keep working in that room, or explicitly save or discard it before opening a different transformer",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

/// Stable application error markers for context safety and readiness failures
/// that must not collapse into generic `desktop_refused` prose. The desktop
/// parity suite pins these exact tokens to the owning adapter.
pub const REFUSAL_MARKERS: &[(&str, &str)] = &[
    ("transformer_not_found", "transformer_not_found"),
    ("project_mismatch", "project_mismatch"),
    ("dirty_room", "dirty_room"),
    ("desktop_unreadable", "desktop_unreadable"),
];

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_OPEN,
        json!({ "transformer": transformer }),
        crate::DESIGN_STAGE_TIMEOUT,
    )
    .map_err(classify_open_failure)?;

    receipt(transformer, &result)
}

fn receipt(transformer: &str, result: &Value) -> Result<Value, Failure> {
    let project = required_text(result, "project")?;
    let returned_transformer = required_text(result, "transformer")?;
    let context_type = required_text(result, "contextType")?;
    let context_changed = required_bool(result, "contextChanged")?;
    let editor_ready = required_bool(result, "editorReady")?;
    let map_ready = required_bool(result, "mapReady")?;
    let dirty = required_bool(result, "dirty")?;
    if returned_transformer != transformer || context_type != "edit" || !editor_ready || !map_ready
    {
        return Err(unreadable_reply(
            "the application did not confirm the requested ready edit/map context",
        ));
    }
    let previous_context = previous_context(result.get("previousContext"))?;

    Ok(json!({
        "project": project,
        "transformer": returned_transformer,
        "context_type": context_type,
        "previous_context": previous_context,
        "context_changed": context_changed,
        "editor_ready": editor_ready,
        "map_ready": map_ready,
        "dirty": dirty,
        "staged": false,
        "persisted": false,
    }))
}

fn required_text<'a>(result: &'a Value, key: &str) -> Result<&'a str, Failure> {
    result[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| unreadable_reply(format!("the application omitted `{key}`")))
}

fn required_bool(result: &Value, key: &str) -> Result<bool, Failure> {
    result[key]
        .as_bool()
        .ok_or_else(|| unreadable_reply(format!("the application omitted `{key}`")))
}

fn previous_context(value: Option<&Value>) -> Result<Value, Failure> {
    match value {
        Some(Value::Null) => Ok(Value::Null),
        Some(Value::Object(context)) => {
            let mode = context
                .get("mode")
                .and_then(Value::as_str)
                .filter(|mode| *mode == "edit")
                .ok_or_else(|| unreadable_reply("the previous context has no supported mode"))?;
            let transformer = context
                .get("transformer")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .ok_or_else(|| unreadable_reply("the previous context has no transformer"))?;
            Ok(json!({ "mode": mode, "transformer": transformer }))
        }
        _ => Err(unreadable_reply(
            "the application omitted `previousContext`",
        )),
    }
}

fn unreadable_reply(message: impl Into<String>) -> Failure {
    Failure::unavailable("desktop_unreadable", message).remedy("restart DS GridDesign and retry")
}

fn classify_open_failure(failure: Failure) -> Failure {
    let failure = crate::classify_design_failure(failure);
    if failure.code() == "auth_context_mismatch" {
        return Failure::conflict(
            "project_mismatch",
            "the paired application's active project changed before context entry",
        )
        .remedy(
            "open the intended exact project in DS GridDesign, verify `ds desktop status`, then retry",
        );
    }
    if failure.code() != "desktop_refused" {
        return failure;
    }
    let detail = failure
        .detail_value()
        .and_then(|value| value["detail"].as_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let code = REFUSAL_MARKERS
        .iter()
        .find_map(|(marker, code)| detail.contains(marker).then_some(*code));
    match code {
        Some("transformer_not_found") => Failure::invalid(
            "transformer_not_found",
            "the exact transformer does not exist in the active project",
        )
        .remedy("run `ds map design list --output json` and pass one exact transformer name"),
        Some("project_mismatch") => Failure::conflict(
            "project_mismatch",
            "the design context and active project do not match",
        )
        .remedy(
            "open the intended exact project in DS GridDesign, verify `ds desktop status`, then retry",
        ),
        Some("dirty_room") => Failure::conflict(
            "dirty_room",
            "another transformer's active room has unsaved local work",
        )
        .remedy(
            "keep working in that room, or explicitly save or discard it before opening a different transformer",
        ),
        Some("desktop_unreadable") => Failure::unavailable(
            "desktop_unreadable",
            "the transformer editor or map did not confirm readiness within its bound",
        )
        .remedy("keep DS GridDesign open, resolve any map loading error, then retry"),
        _ => failure,
    }
}

pub fn render(data: &Value) -> String {
    format!(
        "opened  {}  in {}  {}{}\n",
        data["transformer"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        if data["editor_ready"].as_bool().unwrap_or(false)
            && data["map_ready"].as_bool().unwrap_or(false)
        {
            "editor/map ready"
        } else {
            "context not ready"
        },
        if data["context_changed"].as_bool().unwrap_or(false) {
            ""
        } else {
            " (already active)"
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn receipt_is_bounded_and_preserves_idempotence() {
        let raw = json!({
            "project": "arjgpydw_survey_test",
            "transformer": "agasharu",
            "contextType": "edit",
            "previousContext": { "mode": "edit", "transformer": "agasharu" },
            "contextChanged": false,
            "editorReady": true,
            "mapReady": true,
            "dirty": false,
            "features": [{ "must": "not cross the CLI receipt" }],
        });
        let shaped = receipt("agasharu", &raw).expect("valid receipt");
        assert_eq!(
            shaped,
            json!({
                "project": "arjgpydw_survey_test",
                "transformer": "agasharu",
                "context_type": "edit",
                "previous_context": { "mode": "edit", "transformer": "agasharu" },
                "context_changed": false,
                "editor_ready": true,
                "map_ready": true,
                "dirty": false,
                "staged": false,
                "persisted": false,
            })
        );
        assert!(shaped.get("features").is_none());
    }

    #[test]
    fn receipt_refuses_before_claiming_a_context_is_ready() {
        let raw = json!({
            "project": "arjgpydw_survey_test",
            "transformer": "agasharu",
            "contextType": "edit",
            "previousContext": null,
            "contextChanged": true,
            "editorReady": true,
            "mapReady": false,
            "dirty": false,
        });
        assert_eq!(
            receipt("agasharu", &raw).expect_err("not ready").code(),
            "desktop_unreadable"
        );
    }

    #[test]
    fn semantic_open_refusals_keep_their_public_codes() {
        for code in [
            "transformer_not_found",
            "project_mismatch",
            "dirty_room",
            "desktop_unreadable",
        ] {
            let failure = Failure::failed("desktop_refused", "desktop declined")
                .detail(json!({ "detail": format!("{code}: bounded owner detail") }));
            assert_eq!(classify_open_failure(failure).code(), code);
        }
        let identity_race = Failure::conflict("auth_context_mismatch", "project changed");
        assert_eq!(
            classify_open_failure(identity_race).code(),
            "project_mismatch"
        );
    }
}
