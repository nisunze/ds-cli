//! Bounded native calculated-result reads. JSON inputs use the owner's DTO;
//! the lens and paired transport never initialize, observe, solve or save.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

pub(crate) const QUERY_REFUSALS: &[Refusal] = &[
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
        code: "profile_selection_stale",
        when: "the captured project, account or model session changes",
        remedy: "read map profile view and retry in its current context",
    },
    Refusal {
        code: "profile_replay_model_mismatch",
        when: "query names another native model",
        remedy: "use the model from map profile view",
    },
    Refusal {
        code: "profile_replay_stale",
        when: "authored edits have not reached the native held history",
        remedy: "let native authored observation finish and retry; do not calculate to repair transport",
    },
    Refusal {
        code: "invalid_profile_query",
        when: "query JSON, source/kind combination, identities, paging offset or bounds are invalid",
        remedy: "use native QueryArguments and the emitted next_cursor",
    },
    Refusal {
        code: "profile_query_stale",
        when: "history, root, result identity or page selection changes",
        remedy: "restart paging at the current authored head",
    },
    Refusal {
        code: "profile_results_unavailable",
        when: "no admitted native result scope exists",
        remedy: "explicitly rebuild or analyze the opened Profile",
    },
    Refusal {
        code: "profile_calculation_required",
        when: "requested dependencies are pending or the requested source/envelope/clearance index is unavailable",
        remedy: "inspect blockers or repair inputs, then explicitly rebuild/analyze; Save is independent",
    },
    Refusal {
        code: "profile_query_output_limit",
        when: "one fact or the exact basis exceeds the native byte bound",
        remedy: "use a focused request; inspect the named owning boundary without exporting a full scene",
    },
    Refusal {
        code: "profile_publication_pending",
        when: "a calculated candidate has not been admitted or discarded",
        remedy: "wait for native publication admission or discard before querying",
    },
];

pub static COMMAND: Command = Command {
    id: "map.profile.results",
    path: &["map", "profile", "results"],
    contract: 2,
    summary: "Page cached native Profile facts without calculating.",
    purpose: "Read cached usage, failures, signed loading, violating sections, qualification, blockers or design findings. Rust owns predicates, counts, paging and freshness. Default source profile; model_analysis reads independent Analyze facts. Pending/global/unproved analysis requires Analyze; unaffected supports retain their computed revision. Unconfirmed results refuse. Reads never calculate, initialize, observe or save.",
    chapter: Chapter::MapPresentation,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<model-id>", "Exact open model from map profile view.").required(),
        Arg::value("revision", "<revision-id>", "Expected authored RAM revision from map profile view.").required(),
        Arg::value("history", "<native-json>", "Optional exact native history from map profile view; fences equal-content undo/redo and replaced redo tails."),
        Arg::value("request", "<native-json>", "Native QueryArguments: kind=usage|failures|negative_loading|violating_sections|qualification|blockers|findings; source=profile(default)|model_analysis; entity_ids(empty=all), limit(default 50, 1..200), cursor. Findings requires model_analysis and Analyze. Pages <=256 KiB. Reuse next_cursor unchanged.").required(),
        crate::TARGET_ARG, crate::DESCRIPTOR_ARG,
    ],
    output: "Page: history, authored/computed revisions/roots, freshness, result_id, source, display case (null for model_analysis), envelope/basis, kind, total, offset, items, next_cursor, truncated and timing. Negative loading != uplift_failure. Profile qualifies usage; model_analysis adds resistance cases, design findings and demand/clearance/resistance blockers. Counts are facts, not distinct supports; basis declares scope. Horizontal refs never borrow vertical refs. Missing envelopes refuse. No full scene.",
    examples: &[Example {
        command: "ds map profile results --model <model-id> --revision <revision-id> --request '{\"kind\":\"negative_loading\",\"limit\":50}' --output json",
        note: "Read map profile view first; pending inputs need rebuild/analyze.",
        runnable: false,
    }],
    refusals: QUERY_REFUSALS,
    reference: Some("docs/reference/map.md"),
    search: &["usage", "failures", "negative", "uplift", "violations", "qualification", "blockers", "findings", "model analysis", "cached"],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = native_query_request(inputs)?;
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
pub(crate) fn native_query_request(inputs: &Inputs) -> Result<Value, Failure> {
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
            "findings",
        ] {
            let source = if kind == "findings" {
                "model_analysis"
            } else {
                "profile"
            };
            let request =
                native_query_request(&inputs(&json!({"kind":kind,"source":source}).to_string()))
                    .unwrap();
            assert_eq!(
                request["request"],
                json!({"kind":kind,"source":source,"entity_ids":[],"limit":50,"cursor":null})
            );
            assert_eq!(request["model_id"], "model-a");
            assert_eq!(request["expected_revision"], "rev:a");
        }
    }
    #[test]
    fn source_default_and_model_analysis_cursor_are_owned_and_transferred_without_changes() {
        let default = native_query_request(&inputs(r#"{"kind":"usage"}"#)).unwrap();
        assert_eq!(default["request"]["source"], "profile");
        let history = json!({"model_revision":"rev:a","initial_revision":"rev:initial","undo_depth":1,"redo_depth":0,"history_pin":"native-history"});
        let cursor = json!({"result_id":"native-result","scope":"one","model_id":"model-a","history":history,
            "kind":"findings","source":"model_analysis","entity_ids":["point-a"],"offset":50});
        let body = json!({"kind":"findings","source":"model_analysis","entity_ids":["point-a"],"limit":20,"cursor":cursor});
        let value = native_query_request(&inputs(&body.to_string())).unwrap();
        assert_eq!(value["request"], body);
        assert_eq!(COMMAND.contract, 2);
    }
    #[test]
    fn unknown_fields_free_text_predicates_and_calculation_inputs_refuse_before_pairing() {
        for body in [
            "[]",
            "{}",
            r#"{"kind":"solve"}"#,
            r#"{"kind":"findings","source":"legacy"}"#,
            r#"{"kind":"findings","derive_structure_screening":true}"#,
            r#"{"kind":"usage","analyze":true}"#,
            r#"{"kind":"usage","query":"bad poles"}"#,
            r#"{"kind":"usage","cursor":{"offset":0}}"#,
        ] {
            assert_eq!(
                native_query_request(&inputs(body)).unwrap_err().code(),
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
