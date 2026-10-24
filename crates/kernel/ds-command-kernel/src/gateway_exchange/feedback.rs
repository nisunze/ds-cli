//! The product feedback backlog as the web client reads and triages it: the
//! backlog, a status change with its resolution, and one human sighting.
//! Brain owns the backlog; these operations name the request, bound the
//! input and admit the typed report. Which statuses exist and which of them
//! count as addressed is decided here, once.

use ds_gateway_client::Operation;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{data, project_reference, Admitted, Exchange, Plan, Refusal, Request};

/// The most reports one backlog read asks for.
pub const BACKLOG_LIMIT: u32 = 200;
/// A backlog of 200 reports with full details.
const BACKLOG_CAP: u64 = 16 * 1024 * 1024;
const TITLE_MAX_CHARS: usize = 200;
const DETAIL_MAX_CHARS: usize = 16_384;
const RESOLUTION_MAX_CHARS: usize = 4_000;
pub const INVALID: &str = "feedback_answer_invalid";
pub const INPUT_INVALID: &str = "feedback_input_invalid";

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum FeedbackStatus {
    Open,
    Triaged,
    InProgress,
    Resolved,
    WontFix,
}

impl FeedbackStatus {
    /// Every status, in the order a triage control offers them.
    pub const ALL: [Self; 5] = [
        Self::Open,
        Self::Triaged,
        Self::InProgress,
        Self::Resolved,
        Self::WontFix,
    ];

    /// Resolved or declined: no longer waiting on anyone.
    pub fn addressed(self) -> bool {
        matches!(self, Self::Resolved | Self::WontFix)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum ReporterKind {
    Human,
    Agent,
}

/// One backlog report, as a triage page shows it.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct FeedbackReport {
    pub id: String,
    pub kind: String,
    pub severity: String,
    pub title: String,
    pub detail: String,
    pub surface: String,
    pub component: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    pub reporter_kind: ReporterKind,
    pub occurrences: u32,
    pub reporters: Vec<String>,
    pub status: FeedbackStatus,
    /// Whether the status counts as addressed.
    pub addressed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    pub version: u64,
    pub created_at: String,
    pub last_seen_at: String,
    pub updated_at: String,
    pub updated_by: String,
}

/// The backlog and the statuses a report may be given.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct FeedbackBacklog {
    pub reports: Vec<FeedbackReport>,
    pub statuses: Vec<FeedbackStatus>,
}

/// What a human sighting answers: the report it became, and whether it
/// merged into one already open.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct FeedbackSubmitted {
    pub report: FeedbackReport,
    pub deduplicated: bool,
}

#[derive(Deserialize)]
struct Row {
    id: String,
    kind: String,
    severity: String,
    title: String,
    detail: String,
    surface: String,
    component: String,
    #[serde(default)]
    project_id: Option<String>,
    reporter_kind: ReporterKind,
    occurrences: u32,
    #[serde(default)]
    reporters: Vec<Value>,
    status: FeedbackStatus,
    #[serde(default)]
    resolution: Option<String>,
    version: u64,
    created_at: String,
    last_seen_at: String,
    updated_at: String,
    updated_by: String,
}

fn invalid() -> Refusal {
    Refusal::new(INVALID, "The feedback service returned an invalid report.")
}

fn filled(text: String) -> Result<String, Refusal> {
    if text.trim().is_empty() {
        Err(invalid())
    } else {
        Ok(text)
    }
}

fn present(text: Option<String>) -> Option<String> {
    text.filter(|text| !text.trim().is_empty())
}

fn report(value: &Value) -> Result<FeedbackReport, Refusal> {
    let row = Row::deserialize(value).map_err(|_| invalid())?;
    if row.occurrences < 1 || row.version < 1 {
        return Err(invalid());
    }
    Ok(FeedbackReport {
        id: filled(row.id)?,
        kind: filled(row.kind)?,
        severity: filled(row.severity)?,
        title: filled(row.title)?,
        detail: filled(row.detail)?,
        surface: filled(row.surface)?,
        component: filled(row.component)?,
        project_id: present(row.project_id),
        reporter_kind: row.reporter_kind,
        occurrences: row.occurrences,
        reporters: row
            .reporters
            .into_iter()
            .filter_map(|reporter| reporter.as_str().map(str::to_owned))
            .filter(|reporter| !reporter.trim().is_empty())
            .collect(),
        addressed: row.status.addressed(),
        status: row.status,
        resolution: present(row.resolution),
        version: row.version,
        created_at: filled(row.created_at)?,
        last_seen_at: filled(row.last_seen_at)?,
        updated_at: filled(row.updated_at)?,
        updated_by: filled(row.updated_by)?,
    })
}

/// Brain answers `{success: false}` on a 200 only when a proxy rewrote the
/// envelope; treat it as the refusal it states.
fn confirmed(body: &Value) -> Result<&Map<String, Value>, Refusal> {
    if body.get("success").and_then(Value::as_bool) == Some(false) {
        let message = body
            .pointer("/error/message")
            .or_else(|| body.get("error"))
            .or_else(|| body.get("message"))
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .unwrap_or("The feedback service refused the request.");
        return Err(Refusal::new(
            INVALID,
            message.chars().take(500).collect::<String>(),
        ));
    }
    data(body).as_object().ok_or_else(invalid)
}

fn bounded(text: &str, max: usize) -> bool {
    !text.trim().is_empty()
        && text.chars().count() <= max
        && !text
            .chars()
            .any(|character| character.is_control() && !matches!(character, '\n' | '\t'))
}

fn input_invalid() -> Refusal {
    Refusal::new(INPUT_INVALID, "The feedback request is outside its bounds.")
}

/// The backlog, newest first as Brain orders it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Backlog;

impl Exchange for Backlog {
    type Output = FeedbackBacklog;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(Request::generated(
            Operation::FeedbackAction,
            Some(json!({"action": "list", "limit": BACKLOG_LIMIT})),
        )?
        .response_cap(BACKLOG_CAP)
        .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let reports = confirmed(&body)?
            .get("reports")
            .and_then(Value::as_array)
            .ok_or_else(invalid)?
            .iter()
            .map(report)
            .collect::<Result<_, _>>()?;
        Ok(Admitted::Done(FeedbackBacklog {
            reports,
            statuses: FeedbackStatus::ALL.to_vec(),
        }))
    }
}

/// Give one report a status and resolution, fenced by the version read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpdateStatus {
    pub id: String,
    pub expected_version: u64,
    pub status: FeedbackStatus,
    pub resolution: String,
}

impl Exchange for UpdateStatus {
    type Output = FeedbackReport;

    fn plan(&self) -> Result<Plan, Refusal> {
        let resolution = self.resolution.trim();
        if !bounded(&self.id, TITLE_MAX_CHARS)
            || self.expected_version < 1
            || resolution.chars().count() > RESOLUTION_MAX_CHARS
        {
            return Err(input_invalid());
        }
        let body = json!({"action": "update_status", "id": self.id,
            "expected_version": self.expected_version, "status": self.status, "resolution": resolution});
        Ok(Request::generated(Operation::FeedbackAction, Some(body))?.into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        report(confirmed(&body)?.get("report").ok_or_else(invalid)?).map(Admitted::Done)
    }
}

/// One person's sighting, filed under the project open when it was sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubmitHuman {
    pub title: String,
    pub detail: String,
    pub component: String,
    pub kind: String,
    pub severity: String,
    pub project: Option<String>,
}

impl Exchange for SubmitHuman {
    type Output = FeedbackSubmitted;

    fn plan(&self) -> Result<Plan, Refusal> {
        let (title, detail) = (self.title.trim(), self.detail.trim());
        if !bounded(title, TITLE_MAX_CHARS)
            || !bounded(detail, DETAIL_MAX_CHARS)
            || !bounded(&self.component, TITLE_MAX_CHARS)
            || !["obstacle", "bug", "friction", "idea", "question"].contains(&self.kind.as_str())
            || !["blocker", "major", "minor", "trivial"].contains(&self.severity.as_str())
        {
            return Err(input_invalid());
        }
        let mut report = json!({"reporter_kind": "human", "surface": "ds-web", "title": title,
            "detail": detail, "component": self.component, "kind": self.kind, "severity": self.severity});
        if let Some(project) = &self.project {
            report["project_id"] = json!(project_reference(project)?);
        }
        Ok(Request::generated(
            Operation::FeedbackAction,
            Some(json!({"action": "submit", "report": report})),
        )?
        .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let data = confirmed(&body)?;
        Ok(Admitted::Done(FeedbackSubmitted {
            report: report(data.get("report").ok_or_else(invalid)?)?,
            deduplicated: data.get("deduplicated").and_then(Value::as_bool) == Some(true),
        }))
    }

    fn project(&self) -> Option<&str> {
        self.project.as_deref()
    }
}

/// The typed browser exports Svelte calls; each runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) mod exports {
    use super::{Backlog, FeedbackStatus, SubmitHuman, UpdateStatus};
    use crate::gateway_exchange::browser::{run, typed, GatewayHost};
    use wasm_bindgen::prelude::*;

    /// The feedback backlog and the statuses a report may be given.
    #[wasm_bindgen(js_name = feedbackBacklog, unchecked_return_type = "FeedbackBacklog")]
    pub async fn feedback_backlog(host: GatewayHost) -> Result<JsValue, JsValue> {
        typed(&run(&host, Backlog).await?)
    }

    /// Give one report a status and resolution, fenced by its version.
    #[wasm_bindgen(js_name = feedbackUpdateStatus, unchecked_return_type = "FeedbackReport")]
    pub async fn feedback_update_status(
        host: GatewayHost,
        id: String,
        expected_version: u32,
        #[wasm_bindgen(unchecked_param_type = "FeedbackStatus")] status: JsValue,
        resolution: String,
    ) -> Result<JsValue, JsValue> {
        let status: FeedbackStatus = tsify::serde_wasm_bindgen::from_value(status)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        let call = UpdateStatus {
            id,
            expected_version: expected_version.into(),
            status,
            resolution,
        };
        typed(&run(&host, call).await?)
    }

    /// File one person's sighting under the project open when it was sent.
    #[wasm_bindgen(js_name = feedbackSubmitHuman, unchecked_return_type = "FeedbackSubmitted")]
    pub async fn feedback_submit_human(
        host: GatewayHost,
        title: String,
        detail: String,
        component: String,
        kind: String,
        severity: String,
        project: Option<String>,
    ) -> Result<JsValue, JsValue> {
        let call = SubmitHuman {
            title,
            detail,
            component,
            kind,
            severity,
            project,
        };
        typed(&run(&host, call).await?)
    }
}

#[cfg(test)]
#[path = "feedback_tests.rs"]
mod tests;
