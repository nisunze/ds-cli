//! Thin live-session transport to the existing native usage and REG retype owners.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "profile_retype_committed_context_changed",
        when: "a retype committed to the captured model but its context changed afterwards",
        remedy: "inspect the named committed model and revision before another edit; do not treat this as an unapplied request",
    },
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
        remedy: "open a model Profile and read map profile view",
    },
    Refusal {
        code: "profile_selection_stale",
        when: "the model, revision, account or project changed",
        remedy: "read map profile view and repeat with its exact model and revision",
    },
    Refusal {
        code: "invalid_profile_model_request",
        when: "an identity or request shape is invalid",
        remedy: "use the declared flags and explicit identities from the current native model",
    },
    Refusal {
        code: "profile_model_refused",
        when: "native usage or retype refuses the request, including REG or read-only restrictions",
        remedy: "inspect the native code and detail; correct the request or model before applying",
    },
    Refusal {
        code: "confirmation_required",
        when: "retype or selection refinement has neither --dry-run nor --yes",
        remedy: "preview with --dry-run; use --yes only for an authorized RAM edit or selection change",
    },
];

const USAGE_REFUSALS: &[Refusal] = &[
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
        remedy: "open a model Profile and read map profile view",
    },
    Refusal {
        code: "profile_selection_stale",
        when: "the captured Profile ownership changed",
        remedy: "read map profile view and retry with its current model and revision",
    },
    Refusal {
        code: "invalid_profile_model_request",
        when: "an identity or request shape is invalid",
        remedy: "use the declared native model and support identities",
    },
    Refusal {
        code: "profile_model_refused",
        when: "the optional read-only library projection refuses",
        remedy: "inspect the native library refusal",
    },
    Refusal {
        code: "invalid_profile_query",
        when: "the native usage query is invalid",
        remedy: "use a placed support from the current Profile",
    },
    Refusal {
        code: "profile_query_stale",
        when: "the native query history or input root changed",
        remedy: "read current Profile history before retrying",
    },
    Refusal {
        code: "profile_calculation_required",
        when: "the support result is absent or affected by pending edits",
        remedy: "explicitly calculate the current Profile; reading and Save do not calculate",
    },
    Refusal {
        code: "profile_results_unavailable",
        when: "no retained native result scope is available",
        remedy: "explicitly initialize/calculate the Profile",
    },
    Refusal {
        code: "profile_query_output_limit",
        when: "the native fact exceeds the bounded response",
        remedy: "inspect the native output-limit detail; use a narrower result query",
    },
    Refusal {
        code: "profile_replay_stale",
        when: "worker authored history has not been observed",
        remedy: "observe the current authored head before querying",
    },
    Refusal {
        code: "profile_replay_model_mismatch",
        when: "the captured native result belongs to another model",
        remedy: "read the current Profile model and retry",
    },
    Refusal {
        code: "profile_publication_pending",
        when: "a calculated candidate awaits admission or discard",
        remedy: "confirm or discard the exact outstanding native result",
    },
];

pub static USAGE: Command = Command {
    id: "map.profile.usage",
    path: &["map", "profile", "usage"],
    contract: 2,
    summary: "Read cached native usage for one support in the live dirty Profile.",
    purpose: "Reads one support through the retained native Profile usage query without calculation, exporting a package or saving. A missing or affected result refuses until explicit calculation. The native page carries exact authored/computed history and inputs, qualification, governing cases and signed loading. --types adds the existing read-only native library choices. The model, history, account and project are fenced across the read.",
    chapter: Chapter::MapPresentation,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "model",
            "<model-id>",
            "Exact open model identity from map profile view.",
        )
        .required(),
        Arg::value(
            "revision",
            "<revision-id>",
            "Exact RAM revision from map profile view.",
        )
        .required(),
        Arg::value(
            "structure",
            "<structure-id>",
            "Native support identity; not its displayed number.",
        )
        .required(),
        Arg::switch(
            "types",
            "Also read the native model library's structure type choices for retype preview.",
        ),
        crate::TARGET_ARG,
        crate::DESCRIPTOR_ARG,
    ],
    output: "model_id, revision, structure_id, structure_number and usage: the unchanged bounded native QueryResults page for kind usage. It carries authored/computed revisions and roots, exact history, result identity, basis, items and freshness. Row evidence text arrays are represented by counts; read blockers with map profile results. --types adds existing native structure_types choices. No calculation or Save occurs.",
    examples: &[],
    refusals: USAGE_REFUSALS,
    reference: Some("docs/reference/map.md"),
    search: &["capacity", "percentage", "uplift", "governing", "blockers"],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub static RETYPE: Command = Command {
    id: "map.profile.retype",
    path: &["map", "profile", "retype"],
    contract: 2,
    summary: "Preview or apply a native REG-checked retype to the live RAM session.",
    purpose: "Uses the native REG planner and revision-pinned transaction. --dry-run previews; --yes edits the named supports in RAM. Returns native affected scopes without calculating Profile or saving. Accumulate edits, then explicitly run map profile set --action rebuild or analyze. Timing covers preview, commit and history observation.",
    chapter: Chapter::MapPresentation,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "model",
            "<model-id>",
            "Exact open model identity from map profile view.",
        )
        .required(),
        Arg::value(
            "revision",
            "<revision-id>",
            "Expected RAM revision from map profile view.",
        )
        .required(),
        Arg::repeated(
            "structure",
            "<structure-id>",
            "Explicit support identity; repeat for a native atomic batch.",
        )
        .required(),
        Arg::value(
            "type",
            "<structure-type-id>",
            "Exact native type identity from map profile usage --types.",
        )
        .required(),
        Arg::switch("dry-run", "Read the native REG preview without editing."),
        crate::TARGET_ARG,
        crate::DESCRIPTOR_ARG,
    ],
    output: "model_id, revision, applied, native preview. Apply adds outcome, persisted:false, profile {calculation {policy,model_revision,computed_revision,required,scope},ran:false}, profile_observation {completed,error}, timing_ms {preview,commit_and_session_refresh,profile_observation,total}. Total excludes calculation and network transport. An observation refusal retains the committed edit. Invalid attachment drafts stay in RAM; Save still validates the package.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/map.md"),
    search: &["assembly", "pole", "batch", "elapsed"],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub static ISSUES: Command = Command {
    id: "map.profile.issues", path: &["map", "profile", "issues"], contract: 2,
    summary: "Page cached native engineering findings without calculating.",
    purpose: "Uses the same native QueryArguments and admitted result index as map profile results. Use kind findings with source model_analysis for complete model-bound design points; failures, negative_loading, qualification and blockers retain their native scope. Missing or affected results require explicit calculation; this read never invokes issue calculation, projection or Save. Native source defaults to profile. Counts, root/history/source/cursor fences, basis and bounded transfer remain native-owned.",
    chapter: Chapter::MapPresentation, effect: Effect::ReadOnly, authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<model-id>", "Exact open model identity from map profile view.").required(),
        Arg::value("revision", "<revision-id>", "Expected RAM revision from map profile view.").required(),
        Arg::value("request", "<native-json>", "Native QueryArguments, at most 64 KiB: kind, source, entity_ids, limit, cursor. Design example: {\"kind\":\"findings\",\"source\":\"model_analysis\"}. Native limit defaults to 50; pages admit at most 200 items and 256 KiB. Pass next_cursor unchanged. The former calculation/filter request is refused. Use map profile results --history for an explicit caller history fence.").required(),
        crate::TARGET_ARG, crate::DESCRIPTOR_ARG,
    ],
    output: "model_id, revision and issues: the unchanged native QueryResults page, with source, exact authored/computed history and roots, actual basis/qualification, complete totals, native facts, next_cursor and truncation. No full scene or implicitly calculated issue layer is transferred. For model_analysis findings the display case is null; top-level point section/support references describe the native vertical governing wire; optional horizontal_span retains independent horizontal section/support/attachment references.",
    examples: &[], refusals: crate::profile_results::QUERY_REFUSALS, reference: Some("docs/reference/map.md"),
    search: &["failing", "violations", "uplift", "blocked", "selected", "cached", "design findings"],
    requires: Requires::Window, availability: crate::paired_availability,
};

pub static FILTER: Command = Command {
    id: "map.profile.filter", path: &["map", "profile", "filter"], contract: 1,
    summary: "Refine current Plan selection with native field predicates.",
    purpose: "Queries only captured selected candidates from retained native Arrow arrays and uses native ordered selection composition. --mode intersect keeps matches; remove subtracts them. --dry-run previews without changing selection; --yes applies only if account, project, model, revision and selection are still exact. No package export, model edit or Save.",
    chapter: Chapter::MapPresentation, effect: Effect::LocalUi, authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("model", "<model-id>", "Exact open model identity from map profile view.").required(),
        Arg::value("revision", "<revision-id>", "Expected RAM revision from map profile view.").required(),
        Arg::value("table", "<table-kind>", "Native selection table: structures, tension_sections, terrain_points or alignments.").required(),
        Arg::value("query", "<native-json>", "Native TableIdFilterQuery, at most 64 KiB: filters [{column,op,value,value2?}], stats_columns?, candidate_ids?. Predicates use AND. Native op: contains, equals, not_equals, starts_with, ends_with, gt, gte, lt, lte, between, is_empty, not_empty. Captured current selection supplies candidate_ids; caller input cannot widen it. Empty filters match the selected table rows.").required(),
        Arg::value("mode", "<intersect|remove>", "Explicit refinement of the captured selection.").required(),
        Arg::switch("dry-run", "Preview matches and resulting selection without applying."),
        crate::TARGET_ARG, crate::DESCRIPTOR_ARG,
    ],
    output: "model_id, expected_revision, table, mode, applied, native query result {revision, table_kind, total_rows, matched_count, entity_ids, columns, value_stats} and composed selection {entity_ids,primary}. Native statistics and matching are scoped to selected candidates. Preview does not change selection; apply changes only Plan selection.",
    examples: &[], refusals: REFUSALS, reference: Some("docs/reference/map.md"),
    search: &["selected", "subset", "predicate", "arrow"],
    requires: Requires::Window, availability: crate::paired_availability,
};
pub fn filter(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let request = filter_request(inputs, context.confirmed)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::PROFILE_FILTER,
        request,
        std::time::Duration::from_secs(300),
    )
}
fn filter_request(inputs: &Inputs, confirmed: bool) -> Result<Value, Failure> {
    if !inputs.switch("dry-run") && !confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            "Use --dry-run to preview, or --yes for an authorized selection refinement.",
        ));
    }
    let raw = inputs.require("query")?;
    if raw.len() > 65_536 {
        return Err(Failure::invalid(
            "invalid_profile_model_request",
            "Native table query exceeds 64 KiB.",
        ));
    }
    let query: ds_geo::table::TableIdFilterQuery = serde_json::from_str(raw).map_err(|error| {
        Failure::invalid(
            "invalid_profile_model_request",
            format!("Native table query: {error}"),
        )
    })?;
    let mut request = base(inputs)?;
    request["query"] = serde_json::to_value(query)
        .map_err(|error| Failure::invalid("invalid_profile_model_request", error.to_string()))?;
    request["table"] = json!(id(inputs.require("table")?, "table")?);
    request["mode"] = json!(id(inputs.require("mode")?, "mode")?);
    request["action"] = json!(if inputs.switch("dry-run") {
        "preview"
    } else {
        "apply"
    });
    Ok(request)
}

pub fn issues(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = issues_request(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::PROFILE_ISSUES,
        request,
        std::time::Duration::from_secs(300),
    )
}
fn issues_request(inputs: &Inputs) -> Result<Value, Failure> {
    crate::profile_results::native_query_request(inputs)
}

pub fn usage(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = usage_request(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::PROFILE_USAGE,
        request,
        std::time::Duration::from_secs(300),
    )
}
pub fn retype(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    let request = retype_request(inputs, context.confirmed)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::PROFILE_RETYPE,
        request,
        std::time::Duration::from_secs(300),
    )
}
fn id<'a>(raw: &'a str, name: &str) -> Result<&'a str, Failure> {
    if raw.trim().is_empty() || raw.len() > 200 || raw.chars().any(char::is_control) {
        return Err(Failure::invalid(
            "invalid_profile_model_request",
            format!("--{name} must be a nonblank identity of at most 200 bytes without controls"),
        ));
    }
    Ok(raw)
}
fn base(inputs: &Inputs) -> Result<Value, Failure> {
    Ok(
        json!({"model_id": id(inputs.require("model")?, "model")?, "expected_revision": id(inputs.require("revision")?, "revision")?}),
    )
}
fn usage_request(inputs: &Inputs) -> Result<Value, Failure> {
    let mut request = base(inputs)?;
    request["structure_id"] = json!(id(inputs.require("structure")?, "structure")?);
    request["types"] = json!(inputs.switch("types"));
    Ok(request)
}
fn retype_request(inputs: &Inputs, confirmed: bool) -> Result<Value, Failure> {
    if !inputs.switch("dry-run") && !confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            "Use --dry-run to preview, or --yes for an authorized RAM edit.",
        ));
    }
    let mut request = base(inputs)?;
    let structures = inputs
        .repeated("structure")
        .iter()
        .map(|raw| id(raw, "structure"))
        .collect::<Result<Vec<_>, _>>()?;
    if structures.is_empty() || structures.len() > 4096 {
        return Err(Failure::invalid(
            "invalid_profile_model_request",
            "Pass 1..4096 explicit --structure identities.",
        ));
    }
    request["structure_ids"] = json!(structures);
    request["structure_type_id"] = json!(id(inputs.require("type")?, "type")?);
    request["apply"] = json!(!inputs.switch("dry-run"));
    Ok(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inputs(command: &Command, tail: &[&str]) -> Inputs {
        let mut args = vec![
            "--model",
            "model-a",
            "--revision",
            "rev:a",
            "--structure",
            "pole-40",
        ];
        args.extend_from_slice(tail);
        ds_cli_contract::args::parse(
            command,
            &args.iter().map(|v| (*v).into()).collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn usage_is_a_cached_native_query_with_explicit_calculation_refusals() {
        assert_eq!(USAGE.contract, 2);
        assert_eq!(USAGE.effect, Effect::ReadOnly);
        assert!(USAGE.purpose.contains("without calculation"));
        assert!(
            USAGE
                .output
                .contains("unchanged bounded native QueryResults")
        );
        for code in [
            "profile_calculation_required",
            "profile_query_stale",
            "profile_results_unavailable",
            "profile_query_output_limit",
            "profile_publication_pending",
        ] {
            assert!(
                USAGE.refusals.iter().any(|refusal| refusal.code == code),
                "{code}"
            );
        }
    }
    #[test]
    fn usage_passes_exact_native_support_and_live_revision() {
        assert_eq!(
            usage_request(&inputs(&USAGE, &["--types"])).unwrap(),
            json!({"model_id":"model-a","expected_revision":"rev:a","structure_id":"pole-40","types":true})
        );
    }
    #[test]
    fn preview_is_read_only_even_with_confirmation_and_apply_needs_it() {
        let preview = inputs(&RETYPE, &["--type", "type-b", "--dry-run"]);
        assert_eq!(retype_request(&preview, true).unwrap()["apply"], false);
        let apply = inputs(&RETYPE, &["--type", "type-b", "--structure", "pole-41"]);
        assert_eq!(
            retype_request(&apply, false).unwrap_err().code(),
            "confirmation_required"
        );
        assert_eq!(
            retype_request(&apply, true).unwrap(),
            json!({"model_id":"model-a","expected_revision":"rev:a","structure_ids":["pole-40","pole-41"],"structure_type_id":"type-b","apply":true})
        );
    }
    #[test]
    fn issue_query_uses_the_native_cached_source_schema_and_refuses_legacy_calculation() {
        let args = [
            "--model",
            "a",
            "--revision",
            "rev:a",
            "--request",
            r#"{"kind":"findings","source":"model_analysis","entity_ids":["pole-40"]}"#,
        ]
        .map(str::to_owned);
        let parsed = ds_cli_contract::args::parse(&ISSUES, &args).unwrap();
        let request = issues_request(&parsed).unwrap();
        assert_eq!(
            request["request"],
            json!({"kind":"findings","source":"model_analysis","entity_ids":["pole-40"],"limit":50,"cursor":null})
        );
        assert_eq!(ISSUES.contract, 2);
        assert_eq!(ISSUES.effect, Effect::ReadOnly);
        assert!(
            ISSUES
                .refusals
                .iter()
                .any(|refusal| refusal.code == "profile_calculation_required")
        );
        for body in [
            r#"{"derive_structure_screening":true,"filter":{"nature":"uplift_case"}}"#,
            r#"{"kind":"findings","source":"legacy"}"#,
            r#"{"kind":"findings","analyze":true}"#,
        ] {
            let args =
                ["--model", "a", "--revision", "rev:a", "--request", body].map(str::to_owned);
            let parsed = ds_cli_contract::args::parse(&ISSUES, &args).unwrap();
            assert_eq!(
                issues_request(&parsed).unwrap_err().code(),
                "invalid_profile_query"
            );
        }
    }
    #[test]
    fn malformed_ids_cannot_reach_pairing() {
        assert!(id(" ", "model").is_err());
        assert!(id("pole\n40", "structure").is_err());
    }
}
