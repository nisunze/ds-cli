//! Bounded native calculated-result reads. JSON inputs use the owner's DTO;
//! the lens and paired transport never initialize, observe, solve or save.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "map.profile.results",
    path: &["map", "profile", "results"],
    contract: 1,
    summary: "Page cached native Profile facts without calculating.",
    purpose: "Query admitted calculated usage, failures, negative loading, distinct violating sections, qualification or blockers. Native owns predicates, counts, paging and dependency freshness. Reads never calculate, initialize, observe or save. An affected entity or unconfirmed result refuses; unaffected focused reads can retain engineering at its stated computed revision.",
    chapter: Chapter::MapPresentation,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<model-id>", "Exact open model from map profile view.").required(),
        Arg::value("revision", "<revision-id>", "Expected authored RAM revision from map profile view.").required(),
        Arg::value("history", "<native-json>", "Optional exact native history from map profile view; fences equal-content undo/redo and replaced redo tails."),
        Arg::value("request", "<native-json>", "Native QueryArguments: kind (usage, failures, negative_loading, violating_sections, qualification, blockers), optional entity_ids, limit and cursor. Empty entity_ids means all calculated entities. Limit defaults to 50; native bounds are 1..200 and 256 KiB per page. Pass next_cursor unchanged for the next page.").required(),
        crate::TARGET_ARG, crate::DESCRIPTOR_ARG,
    ],
    output: "Native bounded result: history, authored/computed revisions and roots, freshness, result_id, displayed clearance case, exact usage envelope/basis, kind, total, offset, items, next_cursor, truncated and native query timing. Negative loading retains uplift_failure separately. Qualification is structure screening; blockers cover Profile and usage evidence, not every default-analysis leg. Missing usage envelope refuses instead of reporting false zero. No full scene is transferred.",
    examples: &[Example {
        command: "ds map profile results --model <model-id> --revision <revision-id> --request '{\"kind\":\"negative_loading\",\"limit\":50}' --output json",
        note: "Read the current native identities with map profile view. Explicitly rebuild/analyze if requested inputs are pending.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED, crate::AMBIGUOUS, crate::UNREACHABLE, crate::PAIRING_REJECTED,
        crate::UNSUPPORTED, crate::UNREADABLE, crate::REFUSED,
        Refusal {code:"profile_closed",when:"no model Profile is open",remedy:"open a model Profile"},
        Refusal {code:"profile_selection_stale",when:"the captured project, account or model session changes",remedy:"read map profile view and retry in its current context"},
        Refusal {code:"profile_replay_model_mismatch",when:"query names another native model",remedy:"use the model from map profile view"},
        Refusal {code:"profile_replay_stale",when:"authored edits have not reached the native held history",remedy:"let native authored observation finish and retry; do not calculate to repair transport"},
        Refusal {code:"invalid_profile_query",when:"query JSON, identities, paging offset or bounds are invalid",remedy:"use native QueryArguments and the emitted next_cursor"},
        Refusal {code:"profile_query_stale",when:"history, root, result identity or page selection changes",remedy:"restart paging at the current authored head"},
        Refusal {code:"profile_results_unavailable",when:"no admitted native result scope exists",remedy:"explicitly rebuild or analyze the opened Profile"},
        Refusal {code:"profile_calculation_required",when:"requested dependencies are pending or the matching envelope/displayed clearance index is unavailable",remedy:"inspect blockers or repair inputs, then explicitly rebuild/analyze; Save is independent"},
        Refusal {code:"profile_query_output_limit",when:"one fact or the exact basis exceeds the native byte bound",remedy:"use a focused request; inspect the named owning boundary without exporting a full scene"},
        Refusal {code:"profile_publication_pending",when:"a calculated candidate has not been admitted or discarded",remedy:"wait for native publication admission or discard before querying"},
    ],
    reference: Some("docs/reference/map.md"),
    search: &["usage", "failures", "negative", "uplift", "violations", "qualification", "blockers", "cached"],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = request(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::PROFILE_RESULTS,
        request,
        crate::UI_TIMEOUT,
    )
}
fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("invalid_profile_query", message.into())
}
fn request(inputs: &Inputs) -> Result<Value, Failure> {
    let model = inputs.require("model")?;
    let revision = inputs.require("revision")?;
    if [model, revision]
        .iter()
        .any(|id| id.trim().is_empty() || id.len() > 200 || id.chars().any(char::is_control))
    {
        return Err(invalid(
            "model and revision must be nonblank bounded native identities",
        ));
    }
    let raw = inputs.require("request")?;
    if raw.len() > 64 * 1024 {
        return Err(invalid("query input exceeds 64 KiB transport bound"));
    }
    let query: ds_profile_runtime::QueryArguments =
        serde_json::from_str(raw).map_err(|e| invalid(e.to_string()))?;
    let mut result = json!({"model_id":model,"expected_revision":revision,"request":query});
    if let Some(raw) = inputs.value("history") {
        if raw.len() > 2048 {
            return Err(invalid("history exceeds transport bound"));
        }
        let history: ds_grid_engine::SessionHistoryState =
            serde_json::from_str(raw).map_err(|e| invalid(e.to_string()))?;
        result["expected_history"] = json!(history);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inputs(body: &str) -> Inputs {
        ds_cli_contract::args::parse(
            &COMMAND,
            &[
                "--model",
                "model-a",
                "--revision",
                "rev:a",
                "--request",
                body,
            ]
            .iter()
            .map(|v| (*v).into())
            .collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn every_native_query_kind_uses_the_same_owned_dto() {
        for kind in [
            "usage",
            "failures",
            "negative_loading",
            "violating_sections",
            "qualification",
            "blockers",
        ] {
            let request = request(&inputs(&json!({"kind":kind}).to_string())).unwrap();
            assert_eq!(
                request["request"],
                json!({"kind":kind,"entity_ids":[],"limit":50,"cursor":null})
            );
            assert_eq!(request["model_id"], "model-a");
            assert_eq!(request["expected_revision"], "rev:a");
        }
    }
    #[test]
    fn unknown_fields_free_text_predicates_and_calculation_inputs_refuse_before_pairing() {
        for body in [
            "[]",
            "{}",
            r#"{"kind":"solve"}"#,
            r#"{"kind":"usage","analyze":true}"#,
            r#"{"kind":"usage","query":"bad poles"}"#,
            r#"{"kind":"usage","cursor":{"offset":0}}"#,
        ] {
            assert_eq!(
                request(&inputs(body)).unwrap_err().code(),
                "invalid_profile_query"
            );
        }
    }
    #[test]
    fn local_descriptor_and_read_only_mcp_contract_are_preserved() {
        assert_eq!(COMMAND.effect, Effect::ReadOnly);
        assert_eq!(
            crate::PROFILE_RESULTS.arguments,
            &[
                "model_id",
                "expected_revision",
                "expected_history",
                "request"
            ]
        );
        assert!(
            !COMMAND
                .args
                .iter()
                .any(|arg| arg.name == "yes" || arg.name == "action")
        );
    }
}
