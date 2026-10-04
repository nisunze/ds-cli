//! Bounded operator receipts. Full drawing and engineering evidence stay in the UI projection.
use ds_command_kernel::profile_display::DisplayState;
use ds_command_kernel::profile_encoding::DisplayRow;
use ds_grid_engine::ProfileAtlasScene;
use serde_json::{json, Value};
const BLOCKER_LIMIT: usize = 200;
const CASE_LIMIT: usize = 256;
fn text(value: &Value) -> Value {
    value
        .as_str()
        .map(|value| Value::String(value.chars().take(512).collect()))
        .unwrap_or(Value::Null)
}
pub fn project(
    state: &DisplayState,
    rows: Option<&[DisplayRow]>,
    scene: Option<&ProfileAtlasScene>,
    analysis: Option<&Value>,
) -> Value {
    let review = scene.map(|scene| {
        let mut marker_counts = std::collections::BTreeMap::from([("pass".to_string(), 0usize), ("fail".to_string(), 0), ("unknown".to_string(), 0)]);
        for marker in &scene.engineering.markers {
            let status = serde_json::to_value(marker.status).unwrap();
            *marker_counts.get_mut(status.as_str().unwrap()).unwrap() += 1;
        }
        let choices = scene.engineering.cases.iter().take(CASE_LIMIT).map(|choice| {
            let mut choice = serde_json::to_value(choice).unwrap();
            let label = choice["label"].clone();
            choice["label"] = text(&label);
            choice["label_truncated"] = json!(label.as_str().is_some_and(|text| text.chars().count() > 512));
            choice
        }).collect::<Vec<_>>();
        json!({"display_case":scene.engineering.display_case, "selected_case_index":scene.engineering.selected_case_index,
            "cases":choices, "cases_total":scene.engineering.cases.len(), "cases_truncated":scene.engineering.cases.len() > choices.len(), "marker_count":scene.engineering.markers.len(), "marker_counts":marker_counts})
    });
    let analysis = analysis.map(|source| {
        let mut result = json!({});
        for key in ["model_revision", "engineering_input_root", "display_case", "engineering_case_scope", "status", "marker_count", "empty_model", "blocker_count", "blocker_kinds"] {
            result[key] = source[key].clone();
        }
        let blockers = source["blockers"].as_array().unwrap().iter().take(BLOCKER_LIMIT).map(|blocker| {
            json!({"kind":blocker["kind"], "entity_id":blocker["entity_id"], "message":text(&blocker["message"]),
                "message_truncated":blocker["message"].as_str().is_some_and(|text| text.chars().count() > 512)})
        }).collect::<Vec<_>>();
        result["blockers_truncated"] = json!(source["blocker_count"].as_u64().unwrap() > blockers.len() as u64);
        result["blockers"] = json!(blockers);
        result["blocker_detail_limit"] = json!(BLOCKER_LIMIT);
        result["report_status"] = source["report"]["status"].clone();
        result["issues"] = json!({"clearance":source["issues"]["clearance"],
            "structure_screening": {"classification":source["issues"]["structure_screening"]["classification"], "summary":source["issues"]["structure_screening"]["summary"]},
            "clearance_features_total":source["issues"]["clearance_features_total"], "structure_features_total":source["issues"]["structure_features_total"],
            "clearance_features_truncated":source["issues"]["clearance_features_truncated"], "structure_features_truncated":source["issues"]["structure_features_truncated"]});
        result
    });
    json!({"display":rows.map(|rows| json!({"state":state, "rows":rows})), "review":review, "analysis":analysis})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn large_engine_reports_keep_bounded_receipts_and_explicit_truncation() {
        let analysis = json!({"model_revision":"head", "status":"partial", "blocker_count":2603,
            "blocker_kinds":{"structure_usage":2603}, "blockers":(0..2000).map(|_| json!({"kind":"structure_usage", "message":"é".repeat(1024)})).collect::<Vec<_>>(),
            "report":{"status":"ok", "report":{"full_evidence":"not a command receipt"}}});
        let view = project(&DisplayState::default(), None, None, Some(&analysis));
        assert_eq!(view["analysis"]["blockers"].as_array().unwrap().len(), 200);
        assert_eq!(view["analysis"]["blocker_count"], 2603);
        assert_eq!(view["analysis"]["blockers_truncated"], true);
        assert_eq!(view["analysis"]["blockers"][0]["message_truncated"], true);
        assert!(view["analysis"].get("report").is_none());
        assert!(serde_json::to_vec(&view).unwrap().len() < 1024 * 1024);
    }
}
