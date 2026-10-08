//! Lens over the native retained Profile job DTO. No calculation orchestration.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_profile_runtime::CalculationArguments;
use serde_json::{Value, json};

macro_rules! refusal {
    ($code:literal, $when:literal, $remedy:literal) => {
        Refusal {
            code: $code,
            when: $when,
            remedy: $remedy,
        }
    };
}
pub static COMMAND: Command = Command {
    id:"map.profile.calculation", path:&["map","profile","calculation"], contract:1,
    summary:"Start, inspect, cancel or admit a native Profile calculation.",
    purpose:"Return a native job ticket immediately for an already initialized Profile. Edits, selection and cached reads remain available. Native owns captured inputs, capacity, cancellation and history/display fences. Status never calculates or installs results; Admit explicitly installs a ready current result through native publication. Cancellation discards results; noninterruptible work holds its slot until it exits. No action saves. Cold initialization and browser/peer worker migration remain separate boundaries.",
    chapter:Chapter::MapPresentation, effect:Effect::LocalUi, authority:Authority::DesktopPairing, execution:Execution::Sync,
    args:&[
        Arg::value("model","<model-id>","Exact open model from map profile view.").required(),
        Arg::value("history","<native-json>","Exact authored history from map profile view. Required by native Start; optional for Status, Cancel and Admit. Any supplied history must still match."),
        Arg::value("request","<native-json>","Native CalculationArguments: action start/status/cancel/admit and job_id (single-use, 1..128 bytes). Start additionally accepts display_case and analyze (default false). Native captures admitted styles, options and journal. Read the ticket with status, then explicitly admit a ready result; cancel discards it. No caller scope, principal, publication pin or journal is accepted.").required(),
        crate::TARGET_ARG,crate::DESCRIPTOR_ARG,
    ],
    output:"Start/Status/Cancel: bounded native ticket with job_id, scope, model_id, exact history, status, timing, solver_interruptible:false, runtime_worker_active and error. Admit: bounded Profile receipt with authored/computed identities, native work/timing, review/analysis counts after worker/render publication. Full calculated scene stays in Profile; it is not printed by this command. Save remains independent.",
    examples:&[Example{command:"ds map profile calculation --model <model-id> --history '<native-history>' --request '{\"action\":\"start\",\"job_id\":\"review-1\",\"analyze\":true}' --output json",note:"Read exact model/history with map profile view; poll status and explicitly admit the current ready result.",runnable:false}],
    refusals:&[
        crate::NOT_PAIRED,crate::AMBIGUOUS,crate::UNREACHABLE,crate::PAIRING_REJECTED,crate::UNSUPPORTED,crate::UNREADABLE,crate::REFUSED,
        refusal!("invalid_profile_view","job JSON, history, identity or display case is invalid","use native CalculationArguments and the exact observed history"),
        refusal!("profile_closed","no model Profile is open","open a model Profile first"),
        refusal!("profile_selection_stale","project, account or model session changes","read map profile view in the current captured context"),
        refusal!("profile_results_unavailable","the Profile has no retained native scope","explicitly initialize the Profile before job controls"),
        refusal!("profile_replay_base_unavailable","the native model scope is absent","explicitly initialize the Profile before starting a job"),
        refusal!("profile_replay_model_mismatch","control names another native model","use the model from map profile view"),
        refusal!("profile_replay_stale","Start lacks exact observed history or supplied history differs","let authored observation finish, read map profile view and retry with its history"),
        refusal!("profile_replay_invalid","retained native journal exceeds its bounds","inspect the named journal boundary without replacing its history"),
        refusal!("profile_calculation_busy","a physical native worker still runs","wait for it to exit; edits and reads remain available"),
        refusal!("profile_calculation_capacity","native result or single-use identity capacity is reached","admit or cancel unused results; inspect the named capacity"),
        refusal!("profile_calculation_pending","the job is still running","poll status before admission"),
        refusal!("profile_calculation_stale","history, display context or job identity changed","retain edits and explicitly calculate the current history with a new job ID"),
        refusal!("profile_calculation_unavailable","the job does not exist in this native scope","inspect the current Profile and use its job ticket"),
        refusal!("profile_calculation_cancelled","the result was cancelled","continue editing or explicitly start a new job"),
        refusal!("profile_calculation_failed","the native worker failed","inspect the native failure before another explicit job"),
        refusal!("profile_publication_pending","a native result awaits confirmation/discard","finish that publication before a new job"),
        refusal!("profile_publication_stale","publication identity or history differs","discard the captured result and use current Profile context"),
        refusal!("profile_publication_unavailable","no native publication exists","read the current Profile before another explicit action"),
        refusal!("profile_publication_failed","worker/render publication rollback refused","preserve edits and resolve the named publication"),
    ],
    reference:Some("docs/reference/map.md"),search:&["job","ticket","async","calculate","analyze","cancel","status"],
    requires:Requires::Window,availability:crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = native_request(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::PROFILE_CALCULATION,
        request,
        crate::UI_TIMEOUT,
    )
}
fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("invalid_profile_view", message.into())
}
pub(crate) fn native_request(inputs: &Inputs) -> Result<Value, Failure> {
    let model = inputs.require("model")?;
    if model.trim().is_empty() || model.len() > 200 || model.chars().any(char::is_control) {
        return Err(invalid("model must be a bounded native identity"));
    }
    let raw = inputs.require("request")?;
    if raw.len() > 64 * 1024 {
        return Err(invalid("calculation request exceeds 64 KiB"));
    }
    let request: CalculationArguments =
        serde_json::from_str(raw).map_err(|e| invalid(e.to_string()))?;
    request.validate().map_err(invalid)?;
    let mut value = json!({"model_id":model,"request":request});
    if let Some(raw) = inputs.value("history") {
        if raw.len() > 2048 {
            return Err(invalid("history exceeds transport bound"));
        }
        let history: ds_grid_engine::SessionHistoryState =
            serde_json::from_str(raw).map_err(|e| invalid(e.to_string()))?;
        value["expected_history"] = json!(history);
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inputs(raw: &str) -> Inputs {
        ds_cli_contract::args::parse(
            &COMMAND,
            &["--model", "model-a", "--request", raw]
                .iter()
                .map(|s| (*s).into())
                .collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn native_actions_transport_the_shared_closed_dto_without_engineering() {
        for action in ["start", "status", "cancel", "admit"] {
            let body = format!(r#"{{"action":"{action}","job_id":"job-a"}}"#);
            let wire = native_request(&inputs(&body)).unwrap();
            assert_eq!(wire["model_id"], "model-a");
            assert_eq!(wire["request"]["action"], action);
            assert!(wire["scope"].is_null());
            assert!(wire["publication_id"].is_null());
        }
        assert_eq!(
            crate::PROFILE_CALCULATION.arguments,
            ["model_id", "expected_history", "request"]
        );
    }
    #[test]
    fn malformed_controls_refuse_before_pairing_and_never_accept_transport_overrides() {
        for raw in [
            r#"{"action":"save","job_id":"a"}"#,
            r#"{"action":"start","job_id":""}"#,
            r#"{"action":"start","job_id":"a","scope":"other"}"#,
            r#"{"action":"cancel","job_id":"a","publication_id":"forged"}"#,
            r#"{"action":"start","job_id":"a","analyze":"yes"}"#,
        ] {
            assert_eq!(
                native_request(&inputs(raw)).unwrap_err().code(),
                "invalid_profile_view"
            );
        }
        assert_eq!(
            native_request(&inputs(&"x".repeat(65537)))
                .unwrap_err()
                .code(),
            "invalid_profile_view"
        );
    }
}
