//! Deliberate local Save through the same native session lease as the UI.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_network_contracts::profile::ProfileHistory;
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "map.profile.save",
    path: &["map", "profile", "save"],
    contract: 1,
    summary: "Save the live RAM model locally at its exact native history.",
    purpose: "An explicit local checkpoint only. Native full-history admission captures the open model and blocks edits, undo, redo and discard while storage completes. The UI shows Saving. A refusal releases the lock and leaves RAM edits unsaved. This operation neither calculates nor publishes to a project or checks a cloud head. Read the history with map profile view first.",
    chapter: Chapter::MapPresentation,
    effect: Effect::ArtifactWrite,
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
            "Complete native history including undo/redo cursor and journal pin.",
        )
        .required(),
        crate::TARGET_ARG,
        crate::DESCRIPTOR_ARG,
    ],
    output: "model_id, revision, unchanged history, persisted:true and timing_ms.total after acknowledged local persistence. No result is recomputed and no cloud request is made. Native failure remains profile_model_refused with its original code/detail.",
    examples: &[],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::REFUSED,
        Refusal {
            code: "confirmation_required",
            when: "local Save lacks --yes",
            remedy: "read map profile view and use --yes for the authorized local checkpoint",
        },
        Refusal {
            code: "invalid_profile_model_request",
            when: "model or history JSON is missing, malformed or unbounded",
            remedy: "use the exact model and complete native history from map profile view",
        },
        Refusal {
            code: "profile_closed",
            when: "no open model Profile is available",
            remedy: "open the intended model Profile first",
        },
        Refusal {
            code: "profile_selection_stale",
            when: "model, project, account or native history changed",
            remedy: "read map profile view again; never substitute an older revision",
        },
        Refusal {
            code: "profile_model_refused",
            when: "native Save admission, checkpoint validation or local persistence fails",
            remedy: "inspect the original native refusal; RAM edits remain unsaved and need recovery or an explicit retry",
        },
    ],
    reference: Some("docs/reference/map.md"),
    search: &["checkpoint", "persist", "unsaved"],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let request = native_request(inputs, context.confirmed)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::PROFILE_SAVE,
        request,
        crate::UI_TIMEOUT,
    )
}
fn native_request(inputs: &Inputs, confirmed: bool) -> Result<Value, Failure> {
    let invalid = |message| Failure::invalid("invalid_profile_model_request", message);
    let model = inputs.require("model")?;
    let history = inputs.require("history")?;
    if model.is_empty()
        || model.trim() != model
        || model.len() > 200
        || model.chars().any(char::is_control)
        || history.len() > 2048
    {
        return Err(invalid("Use a bounded native model identity and history."));
    }
    let history: ProfileHistory = serde_json::from_str(history)
        .map_err(|_| invalid("Use the complete native history JSON."))?;
    if !confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            "--yes admits the intentional local Save.",
        ));
    }
    Ok(json!({"model_id":model,"expected_history":history}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_save_requires_confirmation_and_forwards_exact_native_history() {
        assert!(COMMAND.requires_confirmation());
        let history = r#"{"model_revision":"head","initial_revision":"initial","undo_depth":2,"redo_depth":1,"history_pin":"pin"}"#;
        let args = ["--model", "local-probe", "--history", history].map(String::from);
        let inputs = ds_cli_contract::args::parse(&COMMAND, &args).unwrap();
        assert!(native_request(&inputs, false).is_err());
        let value = native_request(&inputs, true).unwrap();
        assert_eq!(
            value["expected_history"],
            serde_json::from_str::<Value>(history).unwrap()
        );
        assert_eq!(
            crate::PROFILE_SAVE.arguments,
            ["model_id", "expected_history"]
        );
        assert_eq!(value.as_object().unwrap().len(), 2);
    }
}
