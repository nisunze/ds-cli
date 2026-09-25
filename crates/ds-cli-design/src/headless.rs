//! The headless owner of the collaboration commands — a translation of the
//! paired application's adapter (`ds-web/src/lib/desktop/cli-design.ts`).
//!
//! Until 2026-09-20 `tag.*`, `group.*`, `consumer-grouping.*`, `comment.*`,
//! `known-columns.*` and `materials.*` sent their arguments to the paired
//! desktop, whose adapter turned them into ds-brain actions and shaped the
//! answers. Every one of those actions authenticates from the bearer and
//! ds-brain is the only authority, so this module performs the same
//! operations, with the same argument maps the commands already build and the
//! same answer shapes they already document, under the native credential for
//! the project each call names with `--project` (the saved selection is never
//! read). The argument keys are the adapter's own — one door,
//! one spelling — and each function below names the adapter function it
//! translates.
//!
//! Decisions stay where they were: what a tag definition may look like is the
//! kernel's verdict (`ds_command_kernel::design_tags`), what a plan means is
//! ds-brain's, and a material propagation request and receipt are the kernel's
//! (`design_config::material_propagation_{request,receipt}`).

use ds_cli_contract::outcome::Failure;
use ds_client_core::design_annotations::{Command, ObjectRef};
use ds_client_core::known_columns;
use serde_json::{Map, Value, json};

use crate::{
    CONFLICT, DESIGN_RECORD_NOT_FOUND, INVALID_DESIGN_REQUEST, TAG_VALUE_CASE_MISMATCH,
    TAG_VALUE_NOT_IN_VOCABULARY,
};

/// Where one collaboration operation runs: the lane selects the native
/// credential and the project is the one the caller named. The saved selection
/// is never read.
#[derive(Clone, Copy)]
struct Door<'a> {
    lane: &'a str,
    project: &'a str,
}

/// One collaboration operation, by the name the command declared, with the
/// argument map it built, against the project the caller named.
pub fn perform(
    operation: &str,
    arguments: Value,
    lane: &str,
    project: &str,
) -> Result<Value, Failure> {
    // The route's coarse codes are refined the way the paired answer was:
    // a case-mismatched tag value, an archived project and a missing
    // capability each keep their own code, remedy and next step.
    dispatch(operation, arguments, Door { lane, project }).map_err(crate::classify_design_failure)
}

fn dispatch(operation: &str, arguments: Value, door: Door<'_>) -> Result<Value, Failure> {
    let args = arguments.as_object().cloned().unwrap_or_default();
    match operation {
        "design.tag.list" => tag_list(door, &args),
        "design.tag.define" => tag_define(door, &args),
        "design.tag.set" => tag_set(door, &args),
        "design.tag.query" => tag_query(door, &args),
        "design.tag.enrich-preview" => enrichment(door, &args, false),
        "design.tag.enrich-apply" => enrichment(door, &args, true),
        "design.group.list" => group_list(door, &args),
        "design.group.preview" => group_plan(door, &args, GroupPlan::Preview),
        "design.group.apply" => group_plan(door, &args, GroupPlan::Apply),
        "design.group.unassign" => group_plan(door, &args, GroupPlan::Unassign),
        "design.group.export" => group_export(door, &args),
        "design.consumer-grouping.preview" => consumer_grouping(door, &args, false),
        "design.consumer-grouping.apply" => consumer_grouping(door, &args, true),
        "design.consumer-grouping.read" => annotate(
            door,
            &Command::ReadConsumerGrouping {
                purpose: text(&args, "purpose").unwrap_or_else(|| "solar_report".into()),
            },
        ),
        "design.consumer-grouping.archive" => annotate(
            door,
            &Command::ArchiveConsumerGrouping {
                purpose: text(&args, "purpose").unwrap_or_else(|| "solar_report".into()),
            },
        ),
        "design.comment.list" => comment_list(door, &args),
        "design.comment.read" => comment_read(door, &args),
        "design.comment.post" => comment_post(door, &args),
        "design.comment.resolve" => comment_resolve(door, &args),
        "design.comment.promote" => comment_promote(door, &args),
        "design.comment.redact" => comment_redact(door, &args),
        "design.known-columns.list" => known_columns_list(door),
        "design.known-columns.set" => known_columns_set(door, &args),
        "design.materials.preview" => materials(door, &args, "preview"),
        "design.materials.apply" => materials(door, &args, "apply"),
        other => Err(Failure::internal(
            "design_operation_unowned",
            format!("`{other}` has no headless owner"),
        )),
    }
}

// ── the door ────────────────────────────────────────────────────────────

fn annotate(door: Door<'_>, command: &Command) -> Result<Value, Failure> {
    Ok(ds_cli_auth::design_annotations(door.lane, door.project, command)?.into_result())
}

fn annotate_with_project(door: Door<'_>, command: &Command) -> Result<(String, Value), Failure> {
    let report = ds_cli_auth::design_annotations(door.lane, door.project, command)?;
    let project = report.project_id().to_owned();
    Ok((project, report.into_result()))
}

fn text(args: &Map<String, Value>, key: &str) -> Option<String> {
    args.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn texts(args: &Map<String, Value>, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// `objectRef(args)`: the anchor the command's `crate::anchor` built.
fn object_ref(args: &Map<String, Value>) -> Result<ObjectRef, Failure> {
    let kind = text(args, "kind").unwrap_or_default();
    let id = text(args, "object").unwrap_or_default();
    if !matches!(kind.as_str(), "lv_transformer" | "mv_model") || id.is_empty() {
        return Err(Failure::invalid(
            "invalid_design_anchor",
            "the anchor needs --kind lv_transformer|mv_model and --object",
        )
        .remedy(crate::INVALID_ANCHOR.remedy));
    }
    Ok(ObjectRef {
        kind,
        id,
        version_id: text(args, "version"),
    })
}

fn null_or(value: Option<&Value>) -> Value {
    value.cloned().unwrap_or(Value::Null)
}

// ── tags ────────────────────────────────────────────────────────────────

/// `listCliDesignTags`.
fn tag_list(door: Door<'_>, args: &Map<String, Value>) -> Result<Value, Failure> {
    let object = object_ref(args)?;
    let (project, definitions) = annotate_with_project(
        door,
        &Command::ListTagDefinitions {
            include_archived: false,
        },
    )?;
    let assignments = annotate(
        door,
        &Command::ListTags {
            object: object.clone(),
        },
    )?;
    let applied: Map<String, Value> = assignments
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|row| {
            row["definition_id"]
                .as_str()
                .map(|id| (id.to_owned(), row.clone()))
        })
        .collect();
    let tags: Vec<Value> = definitions
        .as_array()
        .into_iter()
        .flatten()
        .map(|definition| {
            let id = definition["definition_id"].as_str().unwrap_or_default();
            let row = applied.get(id);
            let values = definition["values"].as_array().cloned().unwrap_or_default();
            json!({
                "definition": id,
                "name": definition["name"],
                "valueType": definition["value_type"],
                "inputControl": definition["input_control"],
                "constraints": null_or(definition.get("constraints")),
                "cardinality": definition["cardinality"],
                "state": definition["state"],
                "management": definition.get("management").cloned().unwrap_or(json!("project")),
                "writable": definition["management"] != "system",
                "managedBy": null_or(definition.get("managed_by")),
                "semanticNamespace": null_or(definition.get("semantic_namespace")),
                "semanticKey": null_or(definition.get("semantic_key")),
                "parentDefinition": null_or(definition.get("parent_definition_id")),
                "jurisdiction": null_or(definition.get("jurisdiction")),
                "source": null_or(definition.get("source")),
                "allowed": values.iter().map(|option| option["value"].clone()).collect::<Vec<_>>(),
                "allowedIds": values.iter().map(|option| null_or(option.get("value_id"))).collect::<Vec<_>>(),
                "values": row.map(|r| r["values"].clone()).filter(|v| !v.is_null()).unwrap_or(json!([])),
                "valueIds": row.map(|r| r["value_ids"].clone()).filter(|v| !v.is_null()).unwrap_or(json!([])),
                "typedValues": row.map(|r| r["typed_values"].clone()).filter(|v| !v.is_null()).unwrap_or(json!([])),
                "template": match (definition["template_id"].as_str(), definition["template_version_id"].as_str()) {
                    (Some(template), Some(version)) => json!(format!("{template}@{version}")),
                    _ => Value::Null,
                },
            })
        })
        .collect();
    Ok(json!({
        "project": project,
        "object": object.id,
        "kind": object.kind,
        "version": object.version_id,
        "tags": tags,
    }))
}

/// `defineCliDesignTag`. The definition's admissibility was the kernel's
/// verdict in the command (`admissible`); here the definition is read for
/// its current version, refused when a governed authority owns it, and saved.
fn tag_define(door: Door<'_>, args: &Map<String, Value>) -> Result<Value, Failure> {
    let definition_id = text(args, "definition").unwrap_or_default();
    let (project, definitions) = annotate_with_project(
        door,
        &Command::ListTagDefinitions {
            include_archived: true,
        },
    )?;
    let current = definitions
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| row["definition_id"].as_str() == Some(definition_id.as_str()))
        .cloned();
    if let Some(current) = &current
        && current["management"] == "system"
    {
        let remedy = match current["managed_by"].as_str() {
            Some("location_enrichment") => {
                "Maintained by location enrichment. Re-run enrichment to change it.".to_owned()
            }
            other => format!(
                "Maintained by {}. Ask that authority to re-resolve it.",
                other.unwrap_or("a governed authority")
            ),
        };
        return Err(Failure::unauthorized(
            "design_not_permitted",
            format!("`{definition_id}` is a system-managed definition"),
        )
        .remedy(remedy));
    }
    let values: Vec<Value> = texts(args, "values")
        .into_iter()
        .enumerate()
        .map(|(order, value)| json!({ "value": value, "order": order }))
        .collect();
    let mut definition = json!({
        "definition_id": definition_id,
        "name": text(args, "name").unwrap_or_default(),
        "cardinality": text(args, "cardinality").unwrap_or_else(|| "single".into()),
        "values": values,
    });
    for (flag, key) in [
        ("value_type", "value_type"),
        ("input_control", "input_control"),
        ("description", "description"),
        ("semantic-namespace", "semantic_namespace"),
        ("semantic-key", "semantic_key"),
        ("parent-definition", "parent_definition_id"),
        ("jurisdiction", "jurisdiction"),
    ] {
        if let Some(value) = text(args, flag) {
            definition[key] = json!(value);
        }
    }
    if let Some(constraints) = args
        .get("constraints")
        .filter(|c| c.as_object().is_some_and(|m| !m.is_empty()))
    {
        definition["constraints"] = constraints.clone();
    }
    if let Some(version) = current.as_ref().and_then(|c| c["version"].as_i64()) {
        definition["expected_version"] = json!(version);
    }
    let saved = annotate(door, &Command::SaveTagDefinition { definition })?;
    Ok(json!({
        "project": project,
        "definition": saved["definition_id"],
        "name": saved["name"],
        "valueType": saved["value_type"],
        "inputControl": saved["input_control"],
        "constraints": null_or(saved.get("constraints")),
        "cardinality": saved["cardinality"],
        "version": saved["version"],
        "management": saved.get("management").cloned().unwrap_or(json!("project")),
        "semanticNamespace": null_or(saved.get("semantic_namespace")),
        "semanticKey": null_or(saved.get("semantic_key")),
        "parentDefinition": null_or(saved.get("parent_definition_id")),
        "jurisdiction": null_or(saved.get("jurisdiction")),
        "values": saved["values"].as_array().into_iter().flatten().map(|option| option["value"].clone()).collect::<Vec<_>>(),
    }))
}

/// `setCliDesignTags`: read the current assignment for its version, then set.
fn tag_set(door: Door<'_>, args: &Map<String, Value>) -> Result<Value, Failure> {
    let object = object_ref(args)?;
    let definition_id = text(args, "definition").unwrap_or_default();
    let (project, assignments) = annotate_with_project(
        door,
        &Command::ListTags {
            object: object.clone(),
        },
    )?;
    let expected_version = assignments
        .as_array()
        .into_iter()
        .flatten()
        .find(|row| row["definition_id"].as_str() == Some(definition_id.as_str()))
        .and_then(|row| row["version"].as_i64());
    let typed_values = args.get("typed_values").cloned();
    let values = if typed_values.is_some() {
        None
    } else {
        Some(texts(args, "values"))
    };
    let saved = annotate(
        door,
        &Command::SetTags {
            object: object.clone(),
            definition_id: definition_id.clone(),
            values,
            typed_values,
            expected_version,
        },
    )?;
    Ok(json!({
        "project": project,
        "object": object.id,
        "definition": definition_id,
        "values": saved["values"],
        "typedValues": saved.get("typed_values").cloned().filter(|v| !v.is_null()).unwrap_or(json!([])),
        "version": saved["version"],
    }))
}

/// `assertChoiceValuesAreStored` + `queryCliDesignTags`.
fn tag_query(door: Door<'_>, args: &Map<String, Value>) -> Result<Value, Failure> {
    let filters = args.get("filters").cloned().unwrap_or(json!([]));
    let match_all = text(args, "match").as_deref() != Some("any");
    let limit = args["limit"].as_i64().unwrap_or(2_000);
    // A choice predicate is matched against the stored vocabulary, never
    // corrected: `equals` on an unauthored spelling returns nothing and
    // `not_equals` every row, which is the same lie inverted.
    let choice_filters: Vec<&Value> = filters
        .as_array()
        .into_iter()
        .flatten()
        .filter(|filter| filter.get("values").is_some())
        .collect();
    let (project, definitions) = if choice_filters.is_empty() {
        let report = ds_cli_auth::design_annotations(
            door.lane,
            door.project,
            &Command::QueryTags {
                filters: filters.clone(),
                match_all,
                limit,
            },
        )?;
        let project = report.project_id().to_owned();
        return Ok(query_result(&project, &report.into_result()));
    } else {
        annotate_with_project(
            door,
            &Command::ListTagDefinitions {
                include_archived: false,
            },
        )?
    };
    for filter in choice_filters {
        let Some(definition) = definitions
            .as_array()
            .into_iter()
            .flatten()
            .find(|row| row["definition_id"] == filter["definition_id"])
        else {
            continue;
        };
        let stored: Vec<&str> = definition["values"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|option| option["value"].as_str())
            .collect();
        if definition["value_type"] != "choice" || stored.is_empty() {
            continue;
        }
        for value in filter["values"].as_array().into_iter().flatten() {
            let Some(value) = value.as_str() else {
                continue;
            };
            if stored.contains(&value) {
                continue;
            }
            let id = filter["definition_id"].as_str().unwrap_or_default();
            if let Some(authored) = stored
                .iter()
                .find(|token| token.eq_ignore_ascii_case(value))
            {
                return Err(Failure::invalid(
                    "tag_value_case_mismatch",
                    format!("{id} stores \"{authored}\", not \"{value}\"."),
                )
                .remedy(TAG_VALUE_CASE_MISMATCH.remedy));
            }
            return Err(Failure::invalid(
                "tag_value_not_in_vocabulary",
                format!(
                    "{id} allows {}; \"{value}\" is not one of them.",
                    stored.join(",")
                ),
            )
            .remedy(TAG_VALUE_NOT_IN_VOCABULARY.remedy));
        }
    }
    let result = annotate(
        door,
        &Command::QueryTags {
            filters,
            match_all,
            limit,
        },
    )?;
    Ok(query_result(&project, &result))
}

fn query_result(project: &str, result: &Value) -> Value {
    let matches: Vec<Value> = result["matches"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            json!({
                "object": row["object"]["id"],
                "version": null_or(row["object"].get("version_id")),
                "assignments": row["assignments"].as_array().into_iter().flatten().map(|assignment| json!({
                    "definition": assignment["definition_id"],
                    "values": assignment["values"],
                    "typedValues": assignment.get("typed_values").cloned().filter(|v| !v.is_null()).unwrap_or(json!([])),
                    "version": assignment["version"],
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "project": project,
        "kind": result["object_kind"],
        "match": result["match"],
        "total": matches.len(),
        "scannedObjects": result["scanned_objects"],
        "scannedAssignments": result["scanned_assignments"],
        "assignmentReads": result["assignment_reads"],
        "matches": matches,
    })
}

/// `previewCliLocationEnrichment` / `applyCliLocationEnrichment`.
fn enrichment(door: Door<'_>, args: &Map<String, Value>, apply: bool) -> Result<Value, Failure> {
    let transformers = texts(args, "transformers");
    let reference_revision = text(args, "reference-revision");
    let command = if apply {
        Command::ApplyLocationEnrichment {
            transformers,
            reference_revision,
            plan_digest: text(args, "digest").unwrap_or_default(),
        }
    } else {
        Command::PreviewLocationEnrichment {
            transformers,
            reference_revision,
        }
    };
    let plan = annotate(door, &command)?;
    Ok(json!({
        "project": plan["project_id"],
        "authority": plan["authority"],
        "jurisdiction": null_or(plan.get("jurisdiction")),
        "country": null_or(plan.get("country")),
        "referenceRevision": plan["reference_revision"],
        "digest": plan["plan_digest"],
        "applied": plan["applied"],
        "counts": plan["counts"],
        "definitions": plan["definitions"],
        "outcomes": plan["outcomes"],
    }))
}

// ── groups ──────────────────────────────────────────────────────────────

/// `listCliDesignTagGroups`.
fn group_list(door: Door<'_>, args: &Map<String, Value>) -> Result<Value, Failure> {
    let (project, summaries) = annotate_with_project(
        door,
        &Command::ListTagGroups {
            transformers: texts(args, "transformers"),
        },
    )?;
    let groups: Vec<Value> = summaries
        .as_array()
        .into_iter()
        .flatten()
        .map(|summary| {
            let mut options: Vec<Value> = summary["definition"]["values"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            options.sort_by_key(|option| option["order"].as_i64().unwrap_or(0));
            let values = summary["values"].as_array().cloned().unwrap_or_default();
            json!({
                "group": summary["group"],
                "defined": summary["defined"],
                "cardinality": null_or(summary["definition"].get("cardinality")),
                "allowed": options.iter().map(|option| option["value"].clone()).collect::<Vec<_>>(),
                "needsModel": values.iter().any(|row| row.get("grid_sync").is_some()),
                "values": values.iter().map(|row| json!({
                    "transformer": row["transformer"],
                    "value": null_or(row.get("value")),
                    "modelState": null_or(row["grid_sync"].get("state")),
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(json!({ "project": project, "groups": groups }))
}

enum GroupPlan {
    Preview,
    Apply,
    Unassign,
}

/// `previewCliDesignTagGroup` / `applyCliDesignTagGroup` /
/// `unassignCliDesignTagGroup`, shaped by `planResult`.
fn group_plan(
    door: Door<'_>,
    args: &Map<String, Value>,
    kind: GroupPlan,
) -> Result<Value, Failure> {
    let group = text(args, "group").unwrap_or_default();
    let transformers = texts(args, "transformers");
    // The value is sent EXACTLY as given (trimmed, as the entry builder
    // trims it); an omitted value previews the unassign.
    let value = text(args, "value");
    let digest = text(args, "digest").unwrap_or_default();
    let entries = |value: Option<String>| -> Vec<(String, Option<String>)> {
        transformers
            .iter()
            .map(|t| (t.clone(), value.clone()))
            .collect()
    };
    let command = match kind {
        GroupPlan::Preview => Command::PreviewTagGroup {
            group,
            entries: entries(value),
        },
        GroupPlan::Apply => Command::ApplyTagGroup {
            group,
            entries: entries(value),
            plan_digest: digest,
        },
        GroupPlan::Unassign => Command::UnassignTagGroup {
            group,
            transformers,
            plan_digest: digest,
        },
    };
    let (project, plan) = annotate_with_project(door, &command)?;
    Ok(plan_result(&project, &plan))
}

/// `planResult` + `summarizeTagGroupPlan`.
fn plan_result(project: &str, plan: &Value) -> Value {
    let outcomes = plan["outcomes"].as_array().cloned().unwrap_or_default();
    let (mut changed, mut unchanged, mut refused) = (0usize, 0usize, 0usize);
    for outcome in &outcomes {
        match outcome["action"].as_str() {
            Some("refused") => refused += 1,
            Some("assign" | "reassign" | "unassign") => changed += 1,
            _ => unchanged += 1,
        }
    }
    json!({
        "project": project,
        "group": plan["group"],
        "operation": plan["operation"],
        "digest": plan["plan_digest"],
        "state": plan["state"],
        "finished": plan["state"] == "applied",
        "changed": changed,
        "unchanged": unchanged,
        "refused": refused,
        "outstanding": plan.get("grid_sync_pending").cloned().filter(|v| !v.is_null()).unwrap_or(json!([])),
        "outcomes": outcomes.iter().map(|outcome| json!({
            "transformer": outcome["transformer"],
            "action": outcome["action"],
            "from": null_or(outcome.get("from")),
            "to": null_or(outcome.get("to")),
            "reason": null_or(outcome.get("reason")),
            "detail": null_or(outcome.get("detail")),
            "modelState": null_or(outcome["grid_sync"].get("state")),
            "modelRefusal": null_or(outcome["grid_sync"].get("refusal")),
        })).collect::<Vec<_>>(),
    })
}

/// `exportCliDesignTagProjection`.
fn group_export(door: Door<'_>, args: &Map<String, Value>) -> Result<Value, Failure> {
    let (project, projection) = annotate_with_project(
        door,
        &Command::ExportTagProjection {
            transformers: texts(args, "transformers"),
            definition_ids: texts(args, "definition-ids"),
        },
    )?;
    Ok(json!({
        "project": project,
        "schema": projection["schema_version"],
        "definitionIds": projection["definition_ids"],
        "sha256": projection["sha256"],
        "bytes": projection["bytes"],
        "transformers": projection["transformers"],
        "groups": projection["groups"],
        "assignments": projection["assignments"],
        "excluded": projection["excluded"].as_array().into_iter().flatten().map(|row| json!({
            "transformer": row["transformer"],
            "definition": row["definition_id"],
            "reason": row["reason"],
        })).collect::<Vec<_>>(),
        "document": projection["document"],
    }))
}

/// `previewCliConsumerGrouping` / `applyCliConsumerGrouping`. `bindings`
/// arrive as the JSON text the command was given.
fn consumer_grouping(
    door: Door<'_>,
    args: &Map<String, Value>,
    apply: bool,
) -> Result<Value, Failure> {
    let purpose = text(args, "purpose").unwrap_or_else(|| "solar_report".into());
    let bindings: Value = match args.get("bindings") {
        Some(Value::String(raw)) => serde_json::from_str(raw).map_err(|_| {
            Failure::invalid("design_request_invalid", "bindings must be a JSON array")
                .remedy(INVALID_DESIGN_REQUEST.remedy)
        })?,
        Some(value) => value.clone(),
        None => json!([]),
    };
    if !bindings.is_array() {
        return Err(
            Failure::invalid("design_request_invalid", "bindings must be a JSON array")
                .remedy(INVALID_DESIGN_REQUEST.remedy),
        );
    }
    if purpose == "report_archive" && bindings.as_array().is_some_and(|b| !b.is_empty()) {
        return Err(Failure::invalid(
            "design_request_invalid",
            "report_archive grouping binds no external source; drop --bindings",
        )
        .remedy(INVALID_DESIGN_REQUEST.remedy));
    }
    let transformers = texts(args, "transformers");
    let definition_ids = texts(args, "definition-ids");
    let command = if apply {
        Command::ApplyConsumerGrouping {
            purpose,
            transformers,
            definition_ids,
            bindings,
            plan_digest: text(args, "digest").unwrap_or_default(),
        }
    } else {
        Command::PreviewConsumerGrouping {
            purpose,
            transformers,
            definition_ids,
            bindings,
        }
    };
    annotate(door, &command)
}

// ── comments ────────────────────────────────────────────────────────────

/// `listCliDesignThreads`.
fn comment_list(door: Door<'_>, args: &Map<String, Value>) -> Result<Value, Failure> {
    let object = object_ref(args)?;
    let (project, threads) = annotate_with_project(
        door,
        &Command::ListThreads {
            object: object.clone(),
            include_resolved: args.get("resolved") == Some(&Value::Bool(true)),
        },
    )?;
    let rows: Vec<Value> = threads
        .as_array()
        .into_iter()
        .flatten()
        .map(|thread| {
            json!({
                "thread": thread["thread_id"],
                "title": thread["title"],
                "state": thread["state"],
                "comments": thread["comment_count"],
                "version": thread["version"],
                "anchor": null_or(thread["anchor"].get("key")),
                "task": null_or(thread.get("task_id")),
            })
        })
        .collect();
    Ok(json!({
        "project": project,
        "object": object.id,
        "kind": object.kind,
        "version": object.version_id,
        "total": rows.len(),
        "threads": rows,
    }))
}

fn thread_view(project: &str, resolved: &Value) -> Value {
    let thread = &resolved["thread"];
    json!({
        "project": project,
        "thread": thread["thread_id"],
        "title": thread["title"],
        "state": thread["state"],
        "version": thread["version"],
        "task": null_or(thread.get("task_id")),
        "more": resolved["truncated"] == Value::Bool(true),
        "comments": resolved["comments"].as_array().into_iter().flatten().map(|comment| {
            let redacted = comment["redacted"] == Value::Bool(true);
            json!({
                "comment": comment["comment_id"],
                "sequence": comment["sequence"],
                "author": comment["author_email"],
                "roles": comment.get("author_roles").cloned().filter(|v| !v.is_null()).unwrap_or(json!([])),
                "redacted": redacted,
                "body": if redacted { Value::Null } else { comment["body"].clone() },
                "at": null_or(comment.get("created_at")),
            })
        }).collect::<Vec<_>>(),
    })
}

/// `readCliDesignThread`.
fn comment_read(door: Door<'_>, args: &Map<String, Value>) -> Result<Value, Failure> {
    let thread_id = text(args, "thread").unwrap_or_default();
    let (project, resolved) = annotate_with_project(door, &Command::GetThread { thread_id })?;
    Ok(thread_view(&project, &resolved))
}

/// `mintDesignId(prefix, humanName)`: a slug of the name and a random
/// suffix, bounded at 128, never trailing a dash.
fn mint_id(prefix: &str, human_name: &str) -> Result<String, Failure> {
    let mut slug = String::new();
    let mut dash = false;
    for character in human_name.to_lowercase().chars() {
        if character.is_ascii_alphanumeric() {
            slug.push(character);
            dash = false;
        } else if !dash && !slug.is_empty() {
            slug.push('-');
            dash = true;
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(96).collect();
    let suffix: String = ds_cli_auth::device::mint_command_id()?
        .chars()
        .take(10)
        .collect();
    let base = if slug.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}-{slug}")
    };
    let minted: String = format!("{base}-{suffix}").chars().take(128).collect();
    Ok(minted.trim_end_matches('-').to_owned())
}

/// `postCliDesignComment`: append to a thread, or open one.
fn comment_post(door: Door<'_>, args: &Map<String, Value>) -> Result<Value, Failure> {
    let body = text(args, "body").unwrap_or_default();
    if let Some(thread_id) = text(args, "thread") {
        let comment_id = mint_id("c", &body.chars().take(24).collect::<String>())?;
        let (project, resolved) = annotate_with_project(
            door,
            &Command::AddComment {
                thread_id: thread_id.clone(),
                comment_id,
                body,
            },
        )?;
        return Ok(json!({
            "project": project,
            "thread": thread_id,
            "comments": resolved["thread"]["comment_count"],
            "version": resolved["thread"]["version"],
        }));
    }
    let object = object_ref(args)?;
    let title = text(args, "title").unwrap_or_default();
    let thread_id = mint_id("thread", &title)?;
    let (project, resolved) = annotate_with_project(
        door,
        &Command::CreateThread {
            thread_id,
            object,
            title,
            body,
        },
    )?;
    Ok(json!({
        "project": project,
        "thread": resolved["thread"]["thread_id"],
        "title": resolved["thread"]["title"],
        "comments": resolved["thread"]["comment_count"],
        "version": resolved["thread"]["version"],
    }))
}

/// `resolveCliDesignThread`: read the thread for its version, then resolve
/// or reopen it.
fn comment_resolve(door: Door<'_>, args: &Map<String, Value>) -> Result<Value, Failure> {
    let thread_id = text(args, "thread").unwrap_or_default();
    let (project, current) = annotate_with_project(
        door,
        &Command::GetThread {
            thread_id: thread_id.clone(),
        },
    )?;
    let expected_version = current["thread"]["version"].as_i64().ok_or_else(|| {
        Failure::invalid(
            "design_record_not_found",
            format!("thread {thread_id} carries no version"),
        )
        .remedy(DESIGN_RECORD_NOT_FOUND.remedy)
    })?;
    let resolved = annotate(
        door,
        &Command::ResolveThread {
            thread_id: thread_id.clone(),
            expected_version,
            reopen: args.get("reopen") == Some(&Value::Bool(true)),
        },
    )?;
    Ok(json!({
        "project": project,
        "thread": thread_id,
        "state": resolved["thread"]["state"],
        "version": resolved["thread"]["version"],
    }))
}

/// `redactDesignComment`, behind a read: the thread must still be at the
/// version the moderator read the comment at, and the comment must be in it,
/// before its text is cleared under that same version. The text is not
/// retained, so nothing here redacts blind.
fn comment_redact(door: Door<'_>, args: &Map<String, Value>) -> Result<Value, Failure> {
    let thread_id = text(args, "thread").unwrap_or_default();
    let comment_id = text(args, "comment").unwrap_or_default();
    let expected_version = args
        .get("expected_version")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let (project, current) = annotate_with_project(
        door,
        &Command::GetThread {
            thread_id: thread_id.clone(),
        },
    )?;
    let now = current["thread"]["version"].as_i64();
    if now != Some(expected_version) {
        return Err(Failure::conflict(
            CONFLICT.code,
            format!(
                "thread {thread_id} is at version {}, not {expected_version}; it moved since you read it",
                now.map_or("unknown".to_owned(), |v| v.to_string())
            ),
        )
        .remedy(CONFLICT.remedy)
        .next(format!("ds design comment read --thread {thread_id}")));
    }
    let comment = current["comments"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|comment| comment["comment_id"] == comment_id.as_str());
    // A thread longer than one read may hold the comment past the page;
    // ds-brain is then the one that answers whether it exists.
    if comment.is_none() && current["truncated"] != Value::Bool(true) {
        return Err(Failure::invalid(
            "design_record_not_found",
            format!("comment {comment_id} is not in thread {thread_id}"),
        )
        .remedy(DESIGN_RECORD_NOT_FOUND.remedy));
    }
    let already = comment.is_some_and(|comment| comment["redacted"] == Value::Bool(true));
    let resolved = annotate(
        door,
        &Command::RedactComment {
            thread_id: thread_id.clone(),
            comment_id: comment_id.clone(),
            expected_version,
            reason: text(args, "reason").unwrap_or_default(),
        },
    )?;
    Ok(json!({
        "project": project,
        "thread": thread_id,
        "comment": comment_id,
        "author": comment.map_or(Value::Null, |comment| comment["author_email"].clone()),
        "sequence": comment.map_or(Value::Null, |comment| comment["sequence"].clone()),
        "already_redacted": already,
        "version": resolved["thread"]["version"],
    }))
}

/// `promoteCliDesignThread`: the thread's version and the plan's revision
/// pin the promotion.
fn comment_promote(door: Door<'_>, args: &Map<String, Value>) -> Result<Value, Failure> {
    let thread_id = text(args, "thread").unwrap_or_default();
    let (project, current) = annotate_with_project(
        door,
        &Command::GetThread {
            thread_id: thread_id.clone(),
        },
    )?;
    let expected_version = current["thread"]["version"].as_i64().ok_or_else(|| {
        Failure::conflict(
            CONFLICT.code,
            format!("thread {thread_id} carries no version"),
        )
        .remedy(CONFLICT.remedy)
    })?;
    let graph = ds_cli_auth::project_management_for_project(
        door.lane,
        door.project,
        &ds_client_core::project_management::Command::Graph,
    )?
    .into_result();
    let base_revision = graph["revision"]
        .as_i64()
        .or_else(|| graph["graph_revision"].as_i64())
        .unwrap_or(0);
    let title = text(args, "title")
        .or_else(|| current["thread"]["title"].as_str().map(str::to_owned))
        .unwrap_or_default();
    let resolved = annotate(
        door,
        &Command::PromoteThread {
            thread_id: thread_id.clone(),
            expected_version,
            title,
            command_id: format!("dscmd-{}", ds_cli_auth::device::mint_command_id()?),
            base_revision,
        },
    )?;
    Ok(json!({
        "project": project,
        "thread": thread_id,
        "task": null_or(resolved["thread"].get("task_id")),
        "version": resolved["thread"]["version"],
    }))
}

// ── known columns ───────────────────────────────────────────────────────

/// `listCliDesignKnownColumns`.
fn known_columns_list(door: Door<'_>) -> Result<Value, Failure> {
    let report =
        ds_cli_auth::known_columns(door.lane, door.project, &known_columns::Command::List)?;
    let project = report.project_id().to_owned();
    let document = report.into_result();
    Ok(json!({
        "project": project,
        "authority": document["authority"],
        "revision": document["revision"],
        "columns": document["columns"],
    }))
}

/// `setCliDesignKnownColumn`: read for the revision, then patch against it.
fn known_columns_set(door: Door<'_>, args: &Map<String, Value>) -> Result<Value, Failure> {
    let current =
        ds_cli_auth::known_columns(door.lane, door.project, &known_columns::Command::List)?
            .into_result();
    let expected_revision = current["revision"].as_i64().unwrap_or(0);
    let report = ds_cli_auth::known_columns(
        door.lane,
        door.project,
        &known_columns::Command::Set {
            layer: text(args, "layer").unwrap_or_default(),
            field: text(args, "field").unwrap_or_default(),
            visible: args.get("visible") == Some(&Value::Bool(true)),
            expected_revision,
        },
    )?;
    let project = report.project_id().to_owned();
    let saved = report.into_result();
    Ok(json!({
        "project": project,
        "authority": saved["authority"],
        "layer": saved["layer"],
        "field": saved["field"],
        "visible": saved["visible"],
        "revision": saved["revision"],
        "changed": saved["changed"],
    }))
}

// ── materials ───────────────────────────────────────────────────────────

/// `propagateCliDesignMaterials`: the kernel builds the fenced request and
/// judges the receipt; the report gate carries it.
fn materials(door: Door<'_>, args: &Map<String, Value>, mode: &str) -> Result<Value, Failure> {
    let build = |input: Value| -> Result<Value, Failure> {
        let bytes = serde_json::to_vec(&input)
            .map_err(|error| Failure::internal("design_request_invalid", error.to_string()))?;
        let built = ds_command_kernel::design_config::material_propagation_request(&bytes)
            .map_err(|message| {
                Failure::invalid("design_request_invalid", message)
                    .remedy(INVALID_DESIGN_REQUEST.remedy)
            })?;
        serde_json::from_str(&built)
            .map_err(|error| Failure::internal("design_request_invalid", error.to_string()))
    };
    // The source project is the selected one; the door hands it over once
    // the credential and its selection are restored, so the request is built
    // once and for the right project.
    let (_, request, receipt) =
        ds_cli_auth::material_propagation(door.lane, door.project, |project| {
            build(json!({
                "schema": "ds.design.material-propagation/v1",
                "source_project": project,
                "template": args.get("template").cloned().unwrap_or(Value::Null),
                "rule_set": args.get("rule-set").cloned().unwrap_or(Value::Null),
                "rows": args.get("rows").cloned().unwrap_or(json!([])),
                "mode": mode,
                "expected_digest": text(args, "digest").unwrap_or_default(),
            }))
        })?;
    let judged = ds_command_kernel::design_config::material_propagation_receipt(
        &serde_json::to_vec(&json!({ "request": request, "receipt": receipt }))
            .map_err(|error| Failure::internal("design_request_invalid", error.to_string()))?,
    )
    .map_err(|message| {
        Failure::unavailable("design_service_failed", message)
            .remedy(crate::DESIGN_SERVICE_FAILED.remedy)
    })?;
    let verdict: Value = serde_json::from_str(&judged)
        .map_err(|error| Failure::internal("design_service_failed", error.to_string()))?;
    if verdict["ok"] != Value::Bool(true) {
        return Err(Failure::unavailable(
            "design_service_failed",
            format!(
                "invalid material propagation receipt: {} does not match the request",
                verdict["mismatch"].as_str().unwrap_or("unknown")
            ),
        )
        .remedy(crate::DESIGN_SERVICE_FAILED.remedy));
    }
    Ok(receipt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_is_summarised_as_the_dialog_summarises_it() {
        let plan = json!({
            "group": "city", "operation": "assign", "plan_digest": "d", "state": "applied",
            "grid_sync_pending": ["T-3"],
            "outcomes": [
                {"transformer": "T-1", "action": "assign", "to": "gagal"},
                {"transformer": "T-2", "action": "unchanged", "from": "gagal", "to": "gagal"},
                {"transformer": "T-3", "action": "refused", "reason": "unknown"},
                {"transformer": "T-4", "action": "reassign", "from": "x", "to": "y", "grid_sync": {"state": "pending"}},
            ],
        });
        let result = plan_result("p", &plan);
        assert_eq!(result["changed"], 2);
        assert_eq!(result["unchanged"], 1);
        assert_eq!(result["refused"], 1);
        assert_eq!(result["finished"], true);
        assert_eq!(result["outstanding"], json!(["T-3"]));
        assert_eq!(result["outcomes"][3]["modelState"], "pending");
        assert_eq!(result["outcomes"][0]["from"], Value::Null);
    }

    #[test]
    fn a_thread_view_hides_a_redacted_body_but_keeps_its_place() {
        let view = thread_view(
            "p",
            &json!({
                "thread": {"thread_id": "t", "title": "T", "state": "open", "version": 2},
                "comments": [
                    {"comment_id": "c1", "sequence": 1, "author_email": "a@ds.rw", "body": "hi"},
                    {"comment_id": "c2", "sequence": 2, "author_email": "b@ds.rw", "body": "gone", "redacted": true},
                ],
                "truncated": true,
            }),
        );
        assert_eq!(view["more"], true);
        assert_eq!(view["comments"][0]["body"], "hi");
        assert_eq!(view["comments"][1]["body"], Value::Null);
        assert_eq!(view["comments"][1]["redacted"], true);
        assert_eq!(view["task"], Value::Null);
    }

    #[test]
    fn a_minted_id_is_a_slug_of_the_name_and_a_suffix() {
        let id = mint_id("thread", "Week 32 — Review!").expect("minted");
        assert!(id.starts_with("thread-week-32-review-"), "{id}");
        assert!(id.len() <= 128 && !id.ends_with('-'));
        let bare = mint_id("c", "").expect("minted");
        assert!(bare.starts_with("c-") && bare.len() == 12, "{bare}");
    }

    #[test]
    fn a_query_result_is_the_shape_the_command_documents() {
        let result = query_result(
            "p",
            &json!({
                "object_kind": "lv_transformer", "match": "all",
                "scanned_objects": 5, "scanned_assignments": 9, "assignment_reads": 2,
                "matches": [{"object": {"kind": "lv_transformer", "id": "T-1"},
                             "assignments": [{"definition_id": "city", "values": ["gagal"], "version": 3}]}],
            }),
        );
        assert_eq!(result["total"], 1);
        assert_eq!(result["matches"][0]["object"], "T-1");
        assert_eq!(result["matches"][0]["version"], Value::Null);
        assert_eq!(
            result["matches"][0]["assignments"][0]["typedValues"],
            json!([])
        );
    }

    #[test]
    fn an_unowned_operation_is_a_defect_not_a_refusal() {
        assert_eq!(
            perform("design.nothing", json!({}), "stable", "test-project")
                .expect_err("unowned")
                .code(),
            "design_operation_unowned"
        );
    }
}
