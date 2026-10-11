//! Live edit transport; native session owns commands, history and validation.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_grid_engine::session::SessionControlRequest;
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "map.profile.edit", path: &["map", "profile", "edit"], contract: 1,
    summary: "Preview/apply native live edits or undo/redo exact Profile history.",
    purpose: "Use the UI native session control on the open RAM model. Preview requires --dry-run; apply/undo/redo require --yes. Native fences full history and stages package bindings atomically. Applied edits await observation, reported separately with elapsed timings. Save stays explicit. Read command schemas with dsgrid describe --kind commands and history with map profile view.",
    chapter: Chapter::MapPresentation, effect: Effect::LocalUi, authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<model-id>", "Exact open model from map profile view.").required(),
        Arg::value("history", "<native-json>", "Complete native history from map profile view, including cursor, redo depth and journal pin.").required(),
        Arg::value("request", "<native-json>", "Native operation (64 KiB): action preview/apply with envelope, undo/redo, or preview_transaction/apply_transaction with commands:[{command_id,command}] (1..4096). Envelope: command_id, command_schema_version:1, expected_revision, command. Schemas: dsgrid describe --kind commands.").required(),
        Arg::switch("dry-run", "Required for native preview/preview_transaction; writes and observes nothing."),
        crate::TARGET_ARG, crate::DESCRIPTOR_ARG,
    ],
    output: "model_id, revision, applied, persisted:false and unchanged native control result/history. Preview returns simulation without observation. Applied edits return separate profile_observation, native calculation freshness and command-to-canvas timings. Failed observation does not undo or hide an applied edit. Native refusals retain their code/detail in profile_model_refused.",
    examples: &[],
    refusals: &[
        crate::NOT_PAIRED, crate::AMBIGUOUS, crate::UNREACHABLE, crate::PAIRING_REJECTED,
        crate::UNSUPPORTED, crate::UNREADABLE, crate::REFUSED,
        Refusal { code:"invalid_profile_model_request", when:"model, history or native operation JSON is invalid", remedy:"use exact map profile view history and the native operation/command schema" },
        Refusal { code:"confirmation_required", when:"apply or history navigation lacks --yes", remedy:"preview the native command first; use --yes only for an authorized RAM edit" },
        Refusal { code:"profile_closed", when:"no model Profile is open", remedy:"open a model Profile first" },
        Refusal { code:"profile_selection_stale", when:"model, account, project or session changed", remedy:"read the current Profile before retrying" },
        Refusal { code:"profile_model_refused", when:"native command, full history, bindings or read-only admission refuses", remedy:"inspect the original native code/detail; correct inputs without changing the captured fence" },
        Refusal { code:"profile_edit_committed_context_changed", when:"an edit committed to its captured model but context changed afterwards", remedy:"inspect the named committed model/revision; do not replay as an unapplied request" },
    ],
    reference: Some("docs/reference/map.md"), search: &["move", "insert", "create", "delete", "alignment"],
    requires: Requires::Window, availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let request = native_request(inputs, context.confirmed)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::PROFILE_EDIT,
        request,
        crate::UI_TIMEOUT,
    )
}

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("invalid_profile_model_request", message.into())
}

fn native_request(inputs: &Inputs, confirmed: bool) -> Result<Value, Failure> {
    let model = inputs.require("model")?;
    if model.trim().is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
        return Err(invalid("model must be a bounded native identity"));
    }
    let raw = inputs.require("request")?;
    let history = inputs.require("history")?;
    if raw.len() > 65536 || history.len() > 2048 {
        return Err(invalid("native operation/history exceeds transport bounds"));
    }
    let request = SessionControlRequest {
        expected_history: serde_json::from_str(history)
            .map_err(|error| invalid(error.to_string()))?,
        operation: serde_json::from_str(raw).map_err(|error| invalid(error.to_string()))?,
    };
    if inputs.switch("dry-run") == request.operation.is_mutating() {
        return Err(invalid(
            "Use --dry-run for native preview/preview_transaction only; omit it for apply/undo/redo.",
        ));
    }
    if request.operation.is_mutating() && !confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            "Preview first; --yes admits an authorized RAM edit/history action.",
        ));
    }
    Ok(json!({"model_id":model,"request":request}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inputs(request: &str) -> Inputs {
        let mut args: Vec<String> = ["--model", "local-test", "--history", r#"{"model_revision":"rev:head","initial_revision":"rev:head","undo_depth":0,"redo_depth":0,"history_pin":"pin"}"#, "--request", request].iter().map(|value| (*value).into()).collect();
        if matches!(
            serde_json::from_str::<Value>(request).unwrap()["action"].as_str(),
            Some("preview" | "preview_transaction")
        ) {
            args.push("--dry-run".into());
        }
        ds_cli_contract::args::parse(&COMMAND, &args).unwrap()
    }
    #[test]
    fn native_controls_are_validated_before_pairing_and_mutations_require_confirmation() {
        for action in ["undo", "redo"] {
            let raw = json!({"action":action}).to_string();
            assert!(native_request(&inputs(&raw), false).is_err());
            let value = native_request(&inputs(&raw), true).unwrap();
            let native: SessionControlRequest =
                serde_json::from_value(value["request"].clone()).unwrap();
            assert_eq!(json!(native.operation), json!({"action":action}));
        }
        for raw in [
            r#"{"action":"save"}"#,
            r#"{"action":"undo","scope":"other"}"#,
            r#"{"action":"apply"}"#,
        ] {
            assert!(native_request(&inputs(raw), true).is_err());
        }
        assert_eq!(crate::PROFILE_EDIT.arguments, ["model_id", "request"]);
    }

    #[test]
    fn transaction_preview_and_apply_share_the_native_command_items_and_confirmation_policy() {
        let commands = json!([{"command_id":"clear-alignment","command":{"command_kind":"delete_alignment","id":"al-test"}}]);
        for action in ["preview_transaction", "apply_transaction"] {
            let raw = json!({"action":action,"commands":commands}).to_string();
            assert_eq!(
                native_request(&inputs(&raw), false).is_ok(),
                action == "preview_transaction"
            );
            let value = native_request(&inputs(&raw), true).unwrap();
            assert_eq!(
                value["request"]["operation"],
                json!({"action":action,"commands":commands})
            );
        }
    }
}
