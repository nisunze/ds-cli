use ds_grid_engine::{DefaultAnalysisRequest, GridSession};
use serde_json::{json, Value};
use std::collections::BTreeMap;
/// Each explicit invocation calls all three owners, including on incomplete models.
/// Refusals become typed blocker evidence; successful independent legs survive.
pub fn analyze(
    session: &GridSession,
    display_case: &ds_grid_engine::profile_display_case::ProfileDisplayCase,
    scene: Option<&ds_grid_engine::ProfileAtlasScene>,
    projection_error: Option<&str>,
) -> Value {
    analyze_with_cache(
        session,
        display_case,
        scene,
        projection_error,
        &mut Default::default(),
    )
}

pub(crate) fn analyze_with_cache(
    session: &GridSession,
    display_case: &ds_grid_engine::profile_display_case::ProfileDisplayCase,
    scene: Option<&ds_grid_engine::ProfileAtlasScene>,
    projection_error: Option<&str>,
    cache: &mut ds_grid_engine::StructureUsageScreeningCache,
) -> Value {
    analyze_impl(session, display_case, scene, projection_error, cache, false).0
}

pub(crate) fn analyze_for_queries(
    session: &GridSession,
    display_case: &ds_grid_engine::profile_display_case::ProfileDisplayCase,
    scene: Option<&ds_grid_engine::ProfileAtlasScene>,
    projection_error: Option<&str>,
    cache: &mut ds_grid_engine::StructureUsageScreeningCache,
) -> (Value, std::sync::Arc<crate::result_queries::Index>) {
    let (value, index) = analyze_impl(session, display_case, scene, projection_error, cache, true);
    (
        value,
        index.expect("explicit native analysis always retains its facts or refusal"),
    )
}

fn analyze_impl(
    session: &GridSession,
    display_case: &ds_grid_engine::profile_display_case::ProfileDisplayCase,
    scene: Option<&ds_grid_engine::ProfileAtlasScene>,
    projection_error: Option<&str>,
    cache: &mut ds_grid_engine::StructureUsageScreeningCache,
    retain: bool,
) -> (Value, Option<std::sync::Arc<crate::result_queries::Index>>) {
    let mut retained = None;
    let revision = session.current_revision();
    let mut blockers = Vec::new();
    if let Some(message) = projection_error {
        blockers.push(json!({"kind":"profile_projection", "message":message}));
    }
    let request = DefaultAnalysisRequest {
        expected_revision: revision.revision_id.clone(),
        expected_engineering_input_root: revision.roots.engineering_input_root.clone(),
        max_rows: 2000,
    };
    let calculated = if retain {
        session
            .analyze_model_defaults_with_facts(&request, cache)
            .map(|(report, facts)| (report, Some(facts)))
    } else {
        session
            .analyze_model_defaults_with_cache(&request, cache)
            .map(|report| (report, None))
    };
    let report = match calculated {
        Ok((report, facts)) => {
            if let Some(facts) = facts {
                retained = Some(std::sync::Arc::new(
                    crate::result_queries::Index::model_analysis(session, &report, facts),
                ));
            }
            for row in &report.demand_unavailable.rows {
                blockers.push(json!({"kind":"section_demand", "entity_id":row.tension_section_id, "message":row.reason}));
            }
            if let Some(reason) = &report.clearance_unavailable {
                blockers.push(
                    json!({"kind":"clearance", "message":reason.message, "refusal":reason.refusal}),
                );
            }
            json!({"status":"ok", "report":report})
        }
        Err(reason) => {
            if retain {
                retained = Some(std::sync::Arc::new(
                    crate::result_queries::Index::unavailable_analysis(
                        session,
                        json!(reason),
                        &reason.to_string(),
                    ),
                ));
            }
            blockers.push(
                json!({"kind":"model_inputs", "message":reason.to_string(), "refusal":reason}),
            );
            json!({"status":"unavailable", "refusal":reason})
        }
    };
    let issues = match ds_grid_engine::engineering_issue_layer::engineering_issue_layer_with_cache(
        session.snapshot(),
        session.current_revision(),
        &ds_grid_engine::engineering_issue_layer::EngineeringIssueLayerRequest {
            derive_structure_screening: true,
            structure_screening: scene.and_then(|scene| scene.engineering.usage_request.clone()),
            // Analyze preserves the current model-bound case envelope. Display
            // weather is presentation; it must not silently narrow engineering.
            clearance: ds_grid_engine::clearance_report::ClearanceReportOptions::default(),
            max_features_per_kind: 2000,
            ..Default::default()
        },
        cache,
    ) {
        Ok(layer) => {
            if let Some(reason) = &layer.clearance_unavailable {
                blockers.push(
                    json!({"kind":"clearance", "message":reason.message, "refusal":reason.refusal}),
                );
            }
            if let Some(reason) = &layer.structure_screening_unavailable {
                blockers.push(json!({"kind":"structure_usage", "message":format!("{:?}", reason), "refusal":reason}));
            }
            Some(layer)
        }
        Err(reason) => {
            blockers.push(json!({"kind":"issues", "message":reason.to_string()}));
            None
        }
    };
    // The native scene evaluates section stringing and clearance independently of
    // default-analysis admission; review details retain exact case and source status.
    if let Some(scene) = scene {
        for marker in &scene.engineering.markers {
            if serde_json::to_value(marker.status).unwrap() == "unknown" {
                for message in &marker.details {
                    blockers.push(
                    json!({"kind":marker.kind, "entity_id":marker.entity_id, "message":message}),
                );
                }
            }
        }
        for message in &scene.engineering.limitations {
            blockers.push(json!({"kind":"review", "message":message}));
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    blockers.retain(|value| seen.insert(value.to_string()));
    let mut blocker_kinds = BTreeMap::<String, usize>::new();
    for blocker in &blockers {
        *blocker_kinds
            .entry(blocker["kind"].as_str().unwrap().into())
            .or_default() += 1;
    }
    let blocker_count = blockers.len();
    blockers.truncate(2000);
    let status = if blockers.is_empty() {
        "qualified"
    } else if report["status"] == "ok" {
        "partial"
    } else {
        "unknown"
    };
    let result = json!({"model_revision":revision.revision_id, "engineering_input_root":revision.roots.engineering_input_root,
        "display_case":display_case, "engineering_case_scope":"model_bound_envelope", "status":status, "report":report, "issues":issues,
        "blockers":blockers, "blocker_count":blocker_count, "blocker_kinds":blocker_kinds, "blockers_truncated":blocker_count > blockers.len(), "section_checks":scene.map(|scene| &scene.evidence), "marker_count":scene.map_or(0, |scene| scene.engineering.markers.len()), "empty_model":session.snapshot().structures.is_empty() });
    (result, retained)
}
