//! Solar run presentation belongs to the native owner, shared by CLI and Desktop.
use crate::native::{CityPhase, CityTimings};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Default, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    pub phase: Option<CityPhase>,
    pub stage: Option<String>,
    pub step: Option<u32>,
    #[serde(default)]
    pub charts: bool,
    #[serde(default)]
    pub calculation_complete: bool,
    #[serde(default)]
    pub complete: bool,
    pub error: Option<String>,
    pub timings: Option<CityTimings>,
    pub completed_at_ms: Option<i64>,
    pub now_ms: Option<i64>,
}

// Grouping and ordering are a domain projection, never a second browser plan.
const ROWS: [(&str, &str, &str, &[&str]); 8] = [
    ("network_costs", "Network Costs", "🌐", &["network_costs"]),
    (
        "site_data",
        "Site Data",
        "🌍",
        &[
            "input_normalization",
            "site_load",
            "site_weather",
            "site_pv",
        ],
    ),
    ("plant_design", "Plant Design", "⚡", &["plant_design"]),
    ("plant_metrics", "Plant Metrics", "📊", &["plant_metrics"]),
    ("plant_costs", "Plant Costs", "💰", &["plant_costs"]),
    (
        "financial_analysis",
        "Financial Analysis",
        "📈",
        &["finance"],
    ),
    ("plotting", "Plotting", "📊", &[]),
    ("unification", "Unification", "🔗", &["unification"]),
];

fn duration(ms: u64) -> String {
    if ms < 1000 {
        return format!("{ms}ms");
    }
    let s = (ms + 500) / 1000;
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m {}s", s / 60, s % 60)
    } else {
        format!("{}h {}m", s / 3600, (s % 3600) / 60)
    }
}
fn ago(ms: i64) -> String {
    let s = (ms.max(0) + 500) / 1000;
    if s < 60 {
        format!("{s}s ago")
    } else if s < 3600 {
        format!("{}m ago", s / 60)
    } else if s < 86400 {
        format!("{}h ago", s / 3600)
    } else {
        format!("{}d ago", s / 86400)
    }
}

pub fn project(snapshot: Snapshot) -> Value {
    let phase = snapshot.phase;
    let engine = ds_solar_contracts::PLANNED_STAGES;
    let position = |stage: &str| {
        engine
            .iter()
            .position(|s| s.as_str() == stage)
            .map(|n| n as u32 + 1)
    };
    let current = snapshot
        .step
        .or_else(|| snapshot.stage.as_deref().and_then(position));
    let current_row = ROWS
        .iter()
        .position(|(_, _, _, stages)| stages.contains(&snapshot.stage.as_deref().unwrap_or("")));
    let finished = snapshot.complete
        || snapshot.calculation_complete
        || matches!(
            phase,
            Some(
                CityPhase::RenderingPlots
                    | CityPhase::ComposingReport
                    | CityPhase::Committing
                    | CityPhase::LocalReady
            )
        );
    let finalizing = !snapshot.complete
        && matches!(
            phase,
            Some(CityPhase::ComposingReport | CityPhase::Committing | CityPhase::LocalReady)
        );
    let now = snapshot
        .now_ms
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis());
    let mut success = 0;
    let mut running = 0;
    let mut failed = 0;
    let steps: Vec<Value> = ROWS.iter().enumerate().map(|(order, (id, name, icon, stages))| {
        let mut status = "pending";
        let skipped = *id == "plotting" && !snapshot.charts && finished;
        if snapshot.complete { status = "success"; }
        else if *id == "plotting" {
            if phase == Some(CityPhase::RenderingPlots) && snapshot.charts { status = "running"; }
            else if finished { status = "success"; }
        } else if finalizing && *id == "unification" { status = "running"; }
        else if phase == Some(CityPhase::Failed) && current_row == Some(order) { status = "failed"; }
        else if finished { status = "success"; }
        else if phase == Some(CityPhase::Calculating) && current_row == Some(order) { status = "running"; }
        else if let Some(current) = current {
            let positions: Vec<u32> = stages.iter().filter_map(|stage| position(stage)).collect();
            if !positions.is_empty() && positions.iter().all(|p| *p < current) { status = "success"; }
        }
        match status { "success" => success += 1, "running" => running += 1, "failed" => failed += 1, _ => () }
        let settled = status == "success" || (status == "failed" && snapshot.calculation_complete);
        let mut row = json!({"step_id":id,"name":name,"icon":icon,"status":status,"order":order,
            "display": if skipped {"Skipped"} else {match status {"success"=>"Completed","running"=>"Running","failed"=>"Failed",_=>"Pending"}},
            "can_run":false,"reason":"Runs as part of the complete prepared pipeline","time_ago":""});
        if settled {
            if let Some(t) = &snapshot.timings {
                let measured: u64 = stages.iter().map(|s| t.stage_ms.get(*s).copied().unwrap_or(0)).sum();
                // Aggregate timings cannot be allocated to every stage: doing so
                // multiplied the displayed total by seven on older receipts.
                let ms = if *id == "plotting" { Some(t.charts_ms) }
                    else if *id == "unification" { Some(measured + t.documents_ms + t.commit_ms + t.docx_ms) }
                    else if stages.iter().any(|s| t.stage_ms.contains_key(*s)) { Some(measured) }
                    else { None };
                if skipped { row["duration"] = json!(""); }
                else if let Some(ms) = ms { row["duration"] = json!(duration(ms)); }
            }
            if let Some(completed) = snapshot.completed_at_ms { row["time_ago"] = json!(ago(now.saturating_sub(completed))); }
        }
        if status == "failed" && let Some(error) = &snapshot.error { row["error"] = json!(error); }
        row
    }).collect();
    let active = !snapshot.complete && !matches!(phase, None | Some(CityPhase::Failed));
    let state = if active {
        "running"
    } else if snapshot.error.is_some() || phase == Some(CityPhase::Failed) {
        "issue"
    } else if snapshot.complete {
        "ready"
    } else {
        "idle"
    };
    let total_duration = snapshot
        .timings
        .as_ref()
        .filter(|_| snapshot.complete || snapshot.calculation_complete)
        .map(|t| duration(t.pipeline_ms + t.charts_ms + t.documents_ms + t.commit_ms + t.docx_ms))
        .unwrap_or_else(|| "—".into());
    let last = steps
        .iter()
        .rev()
        .find(|row| row["status"] == "success")
        .and_then(|row| row["name"].as_str())
        .unwrap_or("")
        .to_owned();
    let mut result = json!({"steps":steps,"summary":{"success":success,"running":running,"failed":failed,
        "pending":ROWS.len()-success-running-failed,"total":ROWS.len()},"any_running":running>0,
        "presentation":{"state":state,"label":match state {"ready"=>"Complete","running" if phase==Some(CityPhase::Queued)=>"Queued",
        "running"=>"Running","issue"=>"Needs attention",_=>"Not run"},"percent":success*100/ROWS.len(),
        "last":last,"duration":total_duration,"completed":if active {"In progress".into()} else {snapshot.completed_at_ms.map(|t| ago(now.saturating_sub(t))).unwrap_or_else(||"—".into())}}});
    if let Some(error) = snapshot.error {
        result["critical_failure"] = json!(error);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view(value: Value) -> Value {
        project(serde_json::from_value(value).unwrap())
    }
    #[test]
    fn admission_failure_is_a_run_issue_without_blame_on_network_costs() {
        let v = view(json!({"phase":"failed","charts":true,"error":"not prepared"}));
        assert_eq!(v["summary"]["failed"], 0);
        assert!(
            v["steps"]
                .as_array()
                .unwrap()
                .iter()
                .all(|s| s["status"] == "pending")
        );
        assert_eq!(v["presentation"]["state"], "issue");
        assert_eq!(v["critical_failure"], "not prepared");
    }
    #[test]
    fn site_and_handoff_progress_use_the_rust_plan() {
        let v = view(json!({"phase":"calculating","stage":"site_weather","charts":true}));
        assert_eq!(v["steps"][0]["status"], "success");
        assert_eq!(v["steps"][1]["status"], "running");
        let v = view(
            json!({"phase":"failed","stage":"unification","calculationComplete":true,"charts":true,"error":"catalog"}),
        );
        assert_eq!(v["summary"]["success"], 7);
        assert_eq!(v["summary"]["failed"], 1);
    }
    #[test]
    fn aggregate_receipt_duration_is_counted_once_and_zero_stage_timings_stay_zero() {
        let v = view(
            json!({"phase":"local_ready","complete":true,"charts":false,"timings":{
            "pipeline_ms":340,"charts_ms":0,"documents_ms":400,"commit_ms":80,"docx_ms":0,
            "stage_ms":{"network_costs":0}},"completedAtMs":0,"nowMs":95000}),
        );
        assert_eq!(v["summary"]["success"], 8);
        assert_eq!(v["steps"][0]["duration"], "0ms");
        assert_eq!(v["steps"][6]["display"], "Skipped");
        assert_eq!(v["presentation"]["duration"], "820ms");
        assert_eq!(v["presentation"]["completed"], "1m ago");
    }
}
