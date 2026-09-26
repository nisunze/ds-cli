//! `--group-by` / `--where` on `ds report project scope|combined`: one
//! Combined Report archive per leaf tag group, each over its own explicit
//! transformer scope.
//!
//! Owner ruling, 2026-09-26: a grouped Combined Report is a custom report the
//! user asks for explicitly — per city, by tags, or by administrative level
//! (an ordinary tag definition once governed enrichment has run) — and the
//! grouping may nest. Nothing is saved on the project: no tag, no consumer
//! grouping. Each leaf goes through the EXISTING combined route with an
//! explicit `--transformer` scope; there is no new service route.
//!
//! This module decides nothing. `ds_command_kernel::combined_grouping` checks
//! the request, names the definitions to export, checks them against the
//! project's listing and plans the leaf groups from the governed tag
//! projection. What is here is the reading — through the same native doors
//! `ds design tag project-list`, `ds report project scope` and
//! `ds design group project-export` use — and the mapping of the kernel's
//! refusals onto declared ones.

use ds_cli_auth::{DesignTagsCommand, TransformerSet};
use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use ds_command_kernel::combined_grouping::{self as kernel, Filter, LeafGroup, Plan, RefusalCode};
use serde_json::{Value, json};

pub const GROUP_BY_ARG: Arg = Arg::repeated(
    "group-by",
    "<definition-id>",
    "Tag definition id: an archive per value; repeat to nest, outer first.",
);
pub const WHERE_ARG: Arg = Arg::repeated(
    "where",
    "<definition-id>=<value>",
    "Keep only this exact tag value; repeat (same id = either).",
);

pub const GROUP_SCOPE_CONFLICT: Refusal = Refusal {
    code: "combined_group_scope_conflict",
    when: "--transformer given with --group-by or --where",
    remedy: "name transformers or select them by tag, not both",
};
pub const GROUP_REQUEST_INVALID: Refusal = Refusal {
    code: "combined_group_request_invalid",
    when: "a --group-by or --where is malformed, repeated or over its bound",
    remedy: "pass each id once; filters as <id>=<value>",
};
pub const GROUP_PROJECTION_INVALID: Refusal = Refusal {
    code: "combined_group_projection_invalid",
    when: "the tag listing or projection is unreadable or not this project's",
    remedy: "retry once, then update ds if it persists",
};
pub const GROUP_KEY_UNKNOWN: Refusal = Refusal {
    code: "combined_group_key_unknown",
    when: "a --group-by or --where id is no active tag definition here",
    remedy: "use an id from `ds design tag project-list`",
};
pub const GROUP_KEY_NOT_SINGLE: Refusal = Refusal {
    code: "combined_group_key_not_single",
    when: "a --group-by definition is multi-valued",
    remedy: "group by a single-valued one; filter this with --where",
};
pub const GROUP_VALUE_UNKNOWN: Refusal = Refusal {
    code: "combined_group_value_unknown",
    when: "a --where value is not in the vocabulary (exact bytes)",
    remedy: "spell it as `ds design tag project-list` does",
};
pub const GROUP_VALUE_RESERVED: Refusal = Refusal {
    code: "combined_group_value_reserved",
    when: "a stored value is `_unassigned`, the untagged bucket",
    remedy: "retag it with `ds design group preview`, then `apply`",
};
pub const GROUPS_EMPTY: Refusal = Refusal {
    code: "combined_groups_empty",
    when: "no active transformer is left after --where",
    remedy: "loosen --where; `ds report project scope` previews groups",
};
pub const GROUPS_TOO_MANY: Refusal = Refusal {
    code: "combined_groups_too_many",
    when: "the grouping yields over 200 archives (count in message)",
    remedy: "group by a coarser definition, or narrow with --where",
};
pub const GROUPS_PARTIAL: Refusal = Refusal {
    code: "combined_groups_partial",
    when: "a group refused or did not run; `detail.groups` has each",
    remedy: "fix each refused cause; re-run those groups with --where",
};

/// Every way the grouping itself refuses, for both commands that take it.
pub const REFUSALS: [Refusal; 9] = [
    GROUP_SCOPE_CONFLICT,
    GROUP_REQUEST_INVALID,
    GROUP_PROJECTION_INVALID,
    GROUP_KEY_UNKNOWN,
    GROUP_KEY_NOT_SINGLE,
    GROUP_VALUE_UNKNOWN,
    GROUP_VALUE_RESERVED,
    GROUPS_EMPTY,
    GROUPS_TOO_MANY,
];

/// What the caller asked for, checked before any credential is restored.
pub struct Requested {
    group_by: Vec<String>,
    filters: Vec<Filter>,
    definitions: Vec<String>,
}

/// `None` when the caller did not ask for grouping at all.
pub fn requested(inputs: &Inputs) -> Result<Option<Requested>, Failure> {
    let group_by = inputs.repeated("group-by").to_vec();
    let wheres = inputs.repeated("where");
    if group_by.is_empty() && wheres.is_empty() {
        return Ok(None);
    }
    if !inputs.repeated("transformer").is_empty() {
        return Err(Failure::invalid(
            GROUP_SCOPE_CONFLICT.code,
            "--transformer names the scope; --group-by and --where select it from tags",
        )
        .remedy(GROUP_SCOPE_CONFLICT.remedy));
    }
    let filters = wheres
        .iter()
        .map(|text| Filter::parse(text))
        .collect::<Result<Vec<_>, _>>()
        .map_err(refusal)?;
    let definitions = kernel::projection_definitions(&group_by, &filters).map_err(refusal)?;
    Ok(Some(Requested {
        group_by,
        filters,
        definitions,
    }))
}

/// The resolved grouping: the inventory's receipt and scope, the kernel's
/// plan, and the digest of the tag projection it was planned from.
pub struct Grouped {
    pub receipt: Value,
    pub scope: Value,
    pub plan: Plan,
    pub projection_sha256: Option<String>,
}

/// Read the three inputs and ask the kernel for the leaf groups.
pub fn resolve(lane: &str, project: &str, requested: &Requested) -> Result<Grouped, Failure> {
    // An unknown id is named here, against the project's own listing — the
    // projection route answers it with a not-found that reads like a
    // permission problem.
    let listing = ds_cli_auth::design_tags(lane, project, &DesignTagsCommand::Definitions)?;
    kernel::check_definitions(
        &requested.group_by,
        &requested.filters,
        &listing.result()["definitions"],
    )
    .map_err(refusal)?;

    let everyone = TransformerSet::new(Vec::new()).map_err(|error| {
        Failure::invalid(super::INVALID_SCOPE.code, error.to_string())
            .remedy(super::INVALID_SCOPE.remedy)
    })?;
    let inventory = ds_cli_auth::transformer_inventory_for_project(lane, project, &everyone)?;
    let receipt = super::project_receipt(&inventory);
    let scope = super::scope_json(&everyone, inventory.result());
    let active: Vec<String> = scope["participating"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect();
    let project_id = inventory.project_id().to_owned();

    // No active transformer means no projection to ask for; the kernel
    // refuses an empty scope before it reads one.
    let (document, projection_sha256) = if active.is_empty() {
        (String::new(), None)
    } else {
        let projection = ds_cli_auth::design_tags(
            lane,
            project,
            &DesignTagsCommand::Projection {
                transformers: active.clone(),
                definitions: requested.definitions.clone(),
            },
        )
        .map_err(definition_gone)?
        .into_result();
        (
            projection["document"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            projection["sha256"].as_str().map(str::to_owned),
        )
    };
    let plan = kernel::plan(&kernel::Request {
        project_id: &project_id,
        document: &document,
        transformers: &active,
        group_by: &requested.group_by,
        filters: &requested.filters,
    })
    .map_err(refusal)?;
    Ok(Grouped {
        receipt,
        scope,
        plan,
        projection_sha256,
    })
}

/// Each leaf group's explicit scope, validated before anything is published
/// so a group the combined route would refuse never leaves the others half
/// done.
pub fn group_scopes(plan: &Plan) -> Result<Vec<TransformerSet>, Failure> {
    plan.groups
        .iter()
        .map(|group| {
            TransformerSet::new(group.transformers.iter().cloned()).map_err(|error| {
                Failure::invalid(
                    super::INVALID_SCOPE.code,
                    format!("group {}: {error}", path_text(group)),
                )
                .remedy(super::INVALID_SCOPE.remedy)
            })
        })
        .collect()
}

/// `district=gasabo / city=kigali`; a filter-only request has one group with
/// no path.
pub fn path_text(group: &LeafGroup) -> String {
    if group.path.is_empty() {
        return "(filtered scope)".to_string();
    }
    group
        .path
        .iter()
        .map(|step| format!("{}={}", step.key, step.value))
        .collect::<Vec<_>>()
        .join(" / ")
}

fn path_json(group: &LeafGroup) -> Value {
    json!(
        group
            .path
            .iter()
            .map(|step| json!({"key": step.key, "value": step.value}))
            .collect::<Vec<_>>()
    )
}

/// The grouping, summarised; with `members` each group carries its names.
pub fn grouping_json(grouped: &Grouped, members: bool) -> Value {
    let plan = &grouped.plan;
    let groups: Vec<Value> = plan
        .groups
        .iter()
        .map(|group| {
            let mut row = json!({
                "path": path_json(group),
                "transformer_count": group.transformers.len(),
            });
            if members {
                row["transformers"] = json!(group.transformers);
            }
            row
        })
        .collect();
    let mut value = json!({
        "schema": plan.schema,
        "group_by": plan.group_by,
        "where": plan.filters,
        "projection_sha256": grouped.projection_sha256,
        "scope_count": plan.scope_count,
        "transformer_count": plan.transformer_count,
        "filtered_out_count": plan.filtered_out_count,
        "unassigned_count": plan.unassigned_count,
        "group_count": plan.group_count,
    });
    if members {
        value["groups"] = json!(groups);
    }
    value
}

/// How many service errors one group receipt carries before it counts them.
const ERRORS_SHOWN: usize = 4;

/// One group's receipt from a published archive.
pub fn published(group: &LeafGroup, output: &Value) -> Value {
    let errors: Vec<Value> = output["errors"].as_array().cloned().unwrap_or_default();
    json!({
        "path": path_json(group),
        "transformer_count": group.transformers.len(),
        "status": output["status"],
        "prefix": output["prefix"],
        "archives": output["archives"],
        "individual_artifact_transformer_count": output["individual_artifact_transformer_count"],
        "missing_individual_artifact_count": output["missing_individual_artifact_count"],
        "errors": errors.iter().take(ERRORS_SHOWN).collect::<Vec<_>>(),
        "error_count": errors.len(),
        "registry_write_failed": output["registry_write_failed"],
    })
}

/// One group's receipt from a refusal: its code, message and remedy, and the
/// archive it DID publish when the refusal says so.
pub fn refused(group: &LeafGroup, failure: &Failure) -> Value {
    let published = failure
        .detail_value()
        .map(|detail| detail["published"].clone())
        .unwrap_or(Value::Null);
    json!({
        "path": path_json(group),
        "transformer_count": group.transformers.len(),
        "status": "refused",
        "prefix": published["prefix"],
        "archives": published["archives"],
        "error": {
            "code": failure.code(),
            "message": failure.message(),
            "remedy": failure.remedy_text(),
            "next": failure.next_commands(),
        },
    })
}

/// A group left unrun after a refusal that would refuse every group alike.
pub fn not_run(group: &LeafGroup) -> Value {
    json!({
        "path": path_json(group),
        "transformer_count": group.transformers.len(),
        "status": "not_run",
    })
}

/// Whether a refusal is about THIS group's rooms, so the next group may still
/// publish. Anything else — identity, credential, state, permission, an
/// unreachable service — would refuse every remaining group the same way.
pub fn group_scoped(failure: &Failure) -> bool {
    matches!(
        failure.code(),
        "combined_inputs_publication_pending"
            | "combined_inputs_not_current"
            | "combined_inputs_empty"
            | "combined_no_inputs"
            | "report_no_individual_artifacts"
            | "report_grouping_incomplete"
            | "auth_input_invalid"
    )
}

/// The refusal for a grouped run that did not publish every group.
pub fn partial(grouping: Value, groups: Vec<Value>) -> Failure {
    let count = |status: &str| {
        groups
            .iter()
            .filter(|group| group["status"] == status)
            .count()
    };
    let (refused, not_run) = (count("refused"), count("not_run"));
    Failure::conflict(
        GROUPS_PARTIAL.code,
        format!(
            "{} of {} group archive(s) published; {refused} refused, {not_run} not run",
            groups.len() - refused - not_run,
            groups.len(),
        ),
    )
    .remedy(GROUPS_PARTIAL.remedy)
    .next("ds report project archives")
    .detail(json!({ "grouping": grouping, "groups": groups }))
}

/// The kernel decides WHICH refusal applies; each is constructed here from
/// its own declared constant so `ds capabilities` lists every code.
fn refusal(refusal: kernel::Refusal) -> Failure {
    let message = refusal.detail.clone();
    let failure = match refusal.code {
        RefusalCode::RequestInvalid => Failure::invalid(GROUP_REQUEST_INVALID.code, message)
            .remedy(GROUP_REQUEST_INVALID.remedy),
        RefusalCode::ProjectionInvalid => {
            Failure::unavailable(GROUP_PROJECTION_INVALID.code, message)
                .remedy(GROUP_PROJECTION_INVALID.remedy)
        }
        RefusalCode::KeyUnknown => Failure::invalid(GROUP_KEY_UNKNOWN.code, message)
            .remedy(GROUP_KEY_UNKNOWN.remedy)
            .next("ds design tag project-list"),
        RefusalCode::KeyNotSingle => Failure::invalid(GROUP_KEY_NOT_SINGLE.code, message)
            .remedy(GROUP_KEY_NOT_SINGLE.remedy)
            .next("ds design tag project-list"),
        RefusalCode::ValueUnknown => Failure::invalid(GROUP_VALUE_UNKNOWN.code, message)
            .remedy(GROUP_VALUE_UNKNOWN.remedy)
            .next("ds design tag project-list"),
        RefusalCode::ValueReserved => Failure::conflict(GROUP_VALUE_RESERVED.code, message)
            .remedy(GROUP_VALUE_RESERVED.remedy)
            .next("ds design group list"),
        RefusalCode::Empty => Failure::conflict(GROUPS_EMPTY.code, message)
            .remedy(GROUPS_EMPTY.remedy)
            .next("ds report project scope"),
        RefusalCode::TooMany => Failure::invalid(GROUPS_TOO_MANY.code, message)
            .remedy(GROUPS_TOO_MANY.remedy)
            .next("ds report project scope"),
    };
    failure.detail(serde_json::to_value(&refusal).unwrap_or(Value::Null))
}

/// The listing named a key active and the projection then refused it as not
/// active (archived between the two reads). That is the fact
/// `check_definitions` refuses as `combined_group_key_unknown`, so it keeps
/// this command's code for it, with the tag service's own words.
fn definition_gone(failure: Failure) -> Failure {
    if failure.code() != ds_cli_auth::TAG_DEFINITION_UNKNOWN_REFUSAL.code {
        return failure;
    }
    let mut mapped = Failure::invalid(GROUP_KEY_UNKNOWN.code, failure.message())
        .remedy(GROUP_KEY_UNKNOWN.remedy);
    if let Some(detail) = failure.detail_value() {
        mapped = mapped.detail(detail.clone());
    }
    for next in failure.next_commands() {
        mapped = mapped.next(next.clone());
    }
    mapped
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every kernel refusal has a declared door here, and no two share one.
    #[test]
    fn every_kernel_refusal_maps_to_its_own_declared_code() {
        let declared: Vec<&str> = REFUSALS.iter().map(|refusal| refusal.code).collect();
        for code in kernel::RefusalCode::ALL {
            let failure = refusal(kernel::Refusal {
                code,
                token: code.token(),
                detail: "d".into(),
                key: None,
                subjects: Vec::new(),
                subject_count: 0,
                count: None,
                bound: None,
            });
            assert_eq!(failure.code(), code.token(), "{code:?}");
            assert!(declared.contains(&failure.code()), "{code:?} undeclared");
        }
    }

    #[test]
    fn a_refusal_about_one_group_lets_the_next_group_run() {
        assert!(group_scoped(&Failure::conflict(
            "combined_inputs_not_current",
            "m"
        )));
        assert!(!group_scoped(&Failure::unauthorized("auth_rejected", "m")));
        assert!(!group_scoped(&Failure::unavailable(
            "headless_signed_out",
            "m"
        )));
    }

    #[test]
    fn a_partial_run_names_every_group_and_does_not_exit_zero() {
        let failure = partial(
            json!({"group_count": 3}),
            vec![
                json!({"status": "success"}),
                json!({"status": "refused"}),
                json!({"status": "not_run"}),
            ],
        );
        assert_eq!(failure.code(), "combined_groups_partial");
        assert!(
            failure.message().starts_with("1 of 3"),
            "{}",
            failure.message()
        );
        let detail = failure.detail_value().expect("detail");
        assert_eq!(detail["groups"].as_array().map(Vec::len), Some(3));
    }

    #[test]
    fn a_definition_archived_after_the_listing_keeps_the_key_unknown_code() {
        let gone = ds_cli_auth::tag_definition_unknown("stable", "p-1", &["city"], None);
        assert_eq!(
            gone.code(),
            ds_cli_auth::TAG_DEFINITION_UNKNOWN_REFUSAL.code
        );
        let mapped = definition_gone(gone);
        assert_eq!(mapped.code(), GROUP_KEY_UNKNOWN.code);
        assert!(mapped.message().contains("`city`"));
        assert_eq!(mapped.remedy_text(), Some(GROUP_KEY_UNKNOWN.remedy));
        assert_eq!(mapped.detail_value().unwrap()["definition_ids"][0], "city");
        assert_eq!(mapped.next_commands().len(), 1);
        let other = definition_gone(Failure::unauthorized("auth_rejected", "refused"));
        assert_eq!(other.code(), "auth_rejected");
    }
}
