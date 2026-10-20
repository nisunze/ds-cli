//! The survey dashboard's numbers as ds-brain keeps them: the cached report
//! (`POST /api/v1/pipeline` `get_reports`) and its regeneration (`POST
//! /api/v1/data` `reports`, which syncs the dashboard and aggregates). A cached
//! report that a later mutation made stale is regenerated once, unless the
//! caller asks only for what is cached.

use serde::Serialize;
use serde_json::{json, Value};

use super::{data, project_reference, Admitted, Exchange, Plan, Refusal, Request};

const READ_PATH: &str = "/api/v1/pipeline";
const RUN_PATH: &str = "/api/v1/data";
/// A large project syncs and aggregates before answering (48 s observed).
const TIMEOUT_MS: u64 = 300_000;
const CAP: u64 = 16 * 1024 * 1024;
pub const INVALID: &str = "survey_reports_answer_invalid";

/// The cached report, as ds-brain answered it.
#[derive(Clone, Debug, PartialEq)]
pub struct Cached {
    pub report: Value,
    /// No report was generated yet.
    pub missing: bool,
    /// A mutation landed after the report was generated.
    pub stale: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReadReports {
    pub project: String,
    pub include_sync_counts: bool,
}

fn report(body: &Value) -> Result<Value, Refusal> {
    let answer = data(body);
    if !answer.is_object() {
        return Err(Refusal::new(INVALID, "The survey report service answered an invalid report."));
    }
    Ok(answer.clone())
}

impl Exchange for ReadReports {
    type Output = Cached;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut body = json!({"action": "get_reports", "eds_project_id": project_reference(&self.project)?});
        if self.include_sync_counts {
            body["include_sync_counts"] = true.into();
        }
        Ok(Request::post(READ_PATH, body).timeout_ms(TIMEOUT_MS).response_cap(CAP).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let report = report(&body)?;
        Ok(Admitted::Done(Cached {
            missing: report.get("status").and_then(Value::as_str) == Some("no_reports"),
            stale: report.get("stale") == Some(&Value::Bool(true)),
            report,
        }))
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// Regenerate the report; `force` rebuilds every enabled form mirror first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunReports {
    pub project: String,
    pub force: bool,
    pub include_sync_counts: bool,
}

impl Exchange for RunReports {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        let mut body = json!({"action": "reports", "eds_project_id": project_reference(&self.project)?});
        if self.force {
            body["force"] = true.into();
        }
        if self.include_sync_counts {
            body["include_sync_counts"] = true.into();
        }
        Ok(Request::post(RUN_PATH, body).timeout_ms(TIMEOUT_MS).response_cap(CAP).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        report(&body).map(Admitted::Done)
    }

    fn project(&self) -> Option<&str> {
        Some(&self.project)
    }
}

/// What the dashboard renders: the report, whether it was just regenerated,
/// and the refusal that kept a stale one on screen.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Shown {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refreshed: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Decide what to show after the cached read: regenerate once when it is
/// stale (or missing and stale) and `refresh_stale` allows it.
pub fn after_read(cached: Cached, refresh_stale: bool) -> Result<Shown, Cached> {
    if cached.missing {
        return if cached.stale && refresh_stale { Err(cached) } else { Ok(Shown::default()) };
    }
    if cached.stale && refresh_stale {
        return Err(cached);
    }
    Ok(Shown { data: Some(cached.report), ..Shown::default() })
}

/// What a regeneration that followed a stale or missing read shows.
pub fn after_run(cached: Cached, run: Result<Value, Refusal>) -> Shown {
    match run {
        Ok(report) => Shown { data: Some(report), refreshed: Some(true), error: None },
        Err(refusal) if cached.missing => Shown { data: None, refreshed: Some(true), error: Some(refusal.message) },
        Err(refusal) => Shown { data: Some(cached.report), refreshed: Some(false), error: Some(refusal.message) },
    }
}

/// The report this device holds for one project, if any: the dashboard paints
/// it at once, offline too. Held per account and project.
#[cfg(feature = "durable-store")]
pub async fn held_report(
    edge: &impl super::retained::Edge,
    project: &str,
) -> Result<Option<Value>, Refusal> {
    project_reference(project)?;
    let kind = crate::retained_reads::Kind::SurveyReport;
    let observed = super::retained::read::<Value>(edge, kind, Some(project), "").await?;
    Ok(observed.held.map(|held| held.value))
}

/// Hold a report Brain answered for one project, without its sync counts
/// (they are read on demand), for the next open. A store that cannot keep it
/// never hides it.
#[cfg(feature = "durable-store")]
pub async fn retain_report(
    edge: &impl super::retained::Edge,
    project: &str,
    report: Value,
) -> Result<(), Refusal> {
    project_reference(project)?;
    let Value::Object(mut report) = report else {
        return Err(Refusal::new(INVALID, "The survey report service answered an invalid report."));
    };
    report.remove("sync_counts");
    let kind = crate::retained_reads::Kind::SurveyReport;
    if let Ok(observed) = super::retained::read::<Value>(edge, kind, Some(project), "").await {
        let owner = observed.owner.as_deref();
        let _ = super::retained::write(edge, owner, kind, Some(project), "", &report, None).await;
    }
    Ok(())
}

/// The dashboard's report from Brain, regenerated once when stale and
/// `refresh_stale` allows it, and held for the next open.
#[cfg(feature = "durable-store")]
pub async fn read_retained(
    edge: &impl super::retained::Edge,
    project: &str,
    include_sync_counts: bool,
    refresh_stale: bool,
) -> Shown {
    let read = ReadReports { project: project.to_owned(), include_sync_counts };
    let cached = match edge.fetch(read).await {
        Ok(cached) => cached,
        Err(refusal) => return Shown { error: Some(refusal.message), ..Shown::default() },
    };
    let shown = match after_read(cached, refresh_stale) {
        Ok(shown) => shown,
        Err(cached) => {
            let run = RunReports { project: project.to_owned(), force: false, include_sync_counts };
            after_run(cached, edge.fetch(run).await)
        }
    };
    retain_shown(edge, project, &shown).await;
    shown
}

/// Regenerate the dashboard's report (`force` rebuilds every form mirror
/// first) and hold it for the next open.
#[cfg(feature = "durable-store")]
pub async fn run_retained(
    edge: &impl super::retained::Edge,
    project: &str,
    force: bool,
    include_sync_counts: bool,
) -> Shown {
    let run = RunReports { project: project.to_owned(), force, include_sync_counts };
    let shown = match edge.fetch(run).await {
        Ok(report) => Shown { data: Some(report), ..Shown::default() },
        Err(refusal) => Shown { error: Some(refusal.message), ..Shown::default() },
    };
    retain_shown(edge, project, &shown).await;
    shown
}

#[cfg(feature = "durable-store")]
async fn retain_shown(edge: &impl super::retained::Edge, project: &str, shown: &Shown) {
    if let (Some(report), None) = (&shown.data, &shown.error) {
        let _ = retain_report(edge, project, report.clone()).await;
    }
}

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{after_read, after_run, ReadReports, RunReports, Shown};
    use crate::gateway_exchange::browser::{attempt, typed, GatewayHost};
    use wasm_bindgen::prelude::*;
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::rejection;
    use crate::gateway_exchange::retained::browser::BrowserEdge;

    /// The dashboard's report from Brain, held for the next open; regenerated
    /// once when stale and `refreshStale` allows it.
    #[wasm_bindgen(js_name = surveyReportsRead, unchecked_return_type = "SurveyReportsShown")]
    pub async fn survey_reports_read(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        include_sync_counts: bool,
        refresh_stale: bool,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        typed(&super::read_retained(&edge, &project, include_sync_counts, refresh_stale).await)
    }

    /// Regenerate the dashboard's report and hold it; `force` rebuilds every
    /// form mirror first.
    #[wasm_bindgen(js_name = surveyReportsRegenerate, unchecked_return_type = "SurveyReportsShown")]
    pub async fn survey_reports_regenerate(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        force: bool,
        include_sync_counts: bool,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        typed(&super::run_retained(&edge, &project, force, include_sync_counts).await)
    }

    /// The report this device holds for a project, or none.
    #[wasm_bindgen(js_name = surveyReportHeld, unchecked_return_type = "Record<string, unknown> | null")]
    pub async fn survey_report_held(host: GatewayHost, store: StoreHost, project: String) -> Result<JsValue, JsValue> {
        let held = super::held_report(&BrowserEdge::new(host, store), &project).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// Hold a report Brain answered, without its sync counts, for the next open.
    #[wasm_bindgen(js_name = surveyReportRetain)]
    pub async fn survey_report_retain(
        host: GatewayHost,
        store: StoreHost,
        project: String,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] report: JsValue,
    ) -> Result<(), JsValue> {
        let report = tsify::serde_wasm_bindgen::from_value(report)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        let edge = BrowserEdge::new(host, store);
        super::retain_report(&edge, &project, report)
            .await
            .map_err(|refusal| rejection(&refusal))
    }

    #[wasm_bindgen(typescript_custom_section)]
    const TYPES: &'static str = r#"
/** The survey dashboard's report: `error` names a refusal; `refreshed` says a stale one was regenerated. */
export interface SurveyReportsShown { data?: Record<string, unknown>; refreshed?: boolean; error?: string }
"#;

    /// The cached survey report, regenerated once when a later mutation made it stale.
    #[wasm_bindgen(js_name = surveyReports, unchecked_return_type = "SurveyReportsShown")]
    pub async fn survey_reports(
        host: GatewayHost,
        project: String,
        include_sync_counts: bool,
        refresh_stale: bool,
    ) -> Result<JsValue, JsValue> {
        let read = ReadReports { project: project.clone(), include_sync_counts };
        let cached = match attempt(&host, read).await? {
            Ok(cached) => cached,
            Err(refusal) => return typed(&Shown { error: Some(refusal.message), ..Shown::default() }),
        };
        let shown = match after_read(cached, refresh_stale) {
            Ok(shown) => shown,
            Err(cached) => {
                let run = attempt(&host, RunReports { project, force: false, include_sync_counts }).await?;
                after_run(cached, run)
            }
        };
        typed(&shown)
    }

    /// Regenerate the survey report; `force` rebuilds every form mirror first.
    #[wasm_bindgen(js_name = surveyReportsRun, unchecked_return_type = "SurveyReportsShown")]
    pub async fn survey_reports_run(
        host: GatewayHost,
        project: String,
        force: bool,
        include_sync_counts: bool,
    ) -> Result<JsValue, JsValue> {
        let shown = match attempt(&host, RunReports { project, force, include_sync_counts }).await? {
            Ok(report) => Shown { data: Some(report), ..Shown::default() },
            Err(refusal) => Shown { error: Some(refusal.message), ..Shown::default() },
        };
        typed(&shown)
    }
}

#[cfg(test)]
#[path = "survey_reports_tests.rs"]
mod tests;
