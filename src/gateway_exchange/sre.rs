//! Reliability: the platform's reliability overview, one window of request
//! activity, the box's published status snapshot, and one client event
//! reported to the request-event store. Brain reads Cloud Monitoring, the
//! activity store and the published snapshot; these operations name each
//! request and admit its typed answer.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{data, Admitted, Exchange, Method, Plan, Refusal, Request};
use crate::server_status;
#[cfg(feature = "durable-store")]
use {
    super::retained::{self, Edge},
    crate::retained_reads::Kind,
};

const OVERVIEW_PATH: &str = "/api/v1/sre/overview";
const EVENTS_PATH: &str = "/api/v1/sre/requests";
const DATA_PATH: &str = "/api/v1/data";
/// Cloud Monitoring answers within a minute.
const OVERVIEW_TIMEOUT_MS: u64 = 60_000;
/// A window is streamed from BigQuery after a freshness sync.
const ACTIVITY_TIMEOUT_MS: u64 = 300_000;
/// The most rows one activity window asks for.
pub const ACTIVITY_MAX_ROWS: u32 = 10_000;
pub const ACTIVITY_DEFAULT_ROWS: u32 = 5_000;
pub const ACTIVITY_MAX_DAYS: u32 = 365;
/// 10,000 rows of a few hundred bytes each, with room to spare.
const ACTIVITY_CAP: u64 = 64 * 1024 * 1024;
/// Brain reads at most 64 KiB of an event.
const EVENT_MESSAGE_MAX_CHARS: usize = 2_000;
const EVENT_TEXT_MAX_CHARS: usize = 512;
pub const INVALID: &str = "sre_answer_invalid";
pub const INPUT_INVALID: &str = "sre_input_invalid";
/// The box has never published a snapshot.
pub const SNAPSHOT_MISSING: &str = "not_published";
/// The snapshot answered is not the box's.
pub const SNAPSHOT_MALFORMED: &str = "contract_violation";

fn get(path: &str) -> Request {
    Request {
        method: Method::Get,
        body: None,
        ..Request::post(path, Value::Null)
    }
}

fn invalid(what: &str) -> Refusal {
    Refusal::new(
        INVALID,
        format!("The reliability service answered an invalid {what}."),
    )
}

/// One SLO's current reading; `None` where Cloud Monitoring has no data.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(default)]
pub struct SloMetric {
    pub goal: f64,
    pub sli: Option<f64>,
    pub budget_remaining_pct: Option<f64>,
    pub burn_state: String,
    pub burn_rate_5m: Option<f64>,
    pub burn_rate_1h: Option<f64>,
    pub burn_rate_30m: Option<f64>,
    pub burn_rate_6h: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(default)]
pub struct SreService {
    pub key: String,
    pub display_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub availability: Option<SloMetric>,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub latency: Option<SloMetric>,
}

/// One Cloud Run service's request rate, latency, errors and memory.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(default)]
pub struct SreServiceOps {
    pub service_name: String,
    pub request_rate: Option<f64>,
    pub p99_latency_ms: Option<f64>,
    pub error_rate_5xx: Option<f64>,
    pub error_ratio_pct: Option<f64>,
    pub memory_utilization_p95: Option<f64>,
    pub memory_overflow: bool,
    pub oom_kills_1h: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(default)]
pub struct SreFleet {
    pub window_minutes: i64,
    pub services: i64,
    pub request_rate: Option<f64>,
    pub error_rate_5xx: Option<f64>,
    pub error_ratio_pct: Option<f64>,
    pub oom_kills_1h: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(default)]
pub struct SreCombinedReports {
    pub window_minutes: i64,
    pub p95_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    pub attempts: Option<f64>,
    pub failures: Option<f64>,
    pub failure_ratio_pct: Option<f64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(default)]
pub struct SreStaleItem {
    pub kind: String,
    pub ds_project: String,
    pub name: String,
    pub since: String,
}

/// One error category and its operator runbook; Brain owns the taxonomy.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(default)]
pub struct SreErrorCategory {
    pub category: String,
    pub title: String,
    pub summary: String,
    pub runbook_url: String,
}

/// The platform's reliability at a glance.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(default)]
pub struct SreOverview {
    pub generated_at: String,
    pub services: Vec<SreService>,
    pub service_ops: Vec<SreServiceOps>,
    pub fleet: Option<SreFleet>,
    pub combined_reports: Option<SreCombinedReports>,
    pub stale: Vec<SreStaleItem>,
    #[cfg_attr(
        all(feature = "wasm", target_arch = "wasm32"),
        tsify(type = "unknown[]")
    )]
    pub incidents: Vec<Value>,
    pub error_catalog: Vec<SreErrorCategory>,
}

/// Brain answers `null` for an empty list; read it as empty.
fn lists_or_empty(mut value: Value) -> Value {
    if let Some(record) = value.as_object_mut() {
        for key in [
            "services",
            "service_ops",
            "stale",
            "incidents",
            "error_catalog",
        ] {
            if record.get(key).is_some_and(Value::is_null) {
                record.insert(key.into(), json!([]));
            }
        }
    }
    value
}

/// The reliability overview: platform-wide, so it names no project.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Overview;

impl Exchange for Overview {
    type Output = SreOverview;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(get(OVERVIEW_PATH).timeout_ms(OVERVIEW_TIMEOUT_MS).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        SreOverview::deserialize(lists_or_empty(data(&body).clone()))
            .map(Admitted::Done)
            .map_err(|_| invalid("overview"))
    }
}

/// One request event of the activity store: a client or forwarded-compute
/// request and how it ended.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(default)]
pub struct SreActivityEvent {
    pub event_id: String,
    pub ts: Option<String>,
    pub trace_id: String,
    pub container_kind: Option<String>,
    pub source: Option<String>,
    pub platform: Option<String>,
    pub user_visible: Option<bool>,
    pub request_id: Option<String>,
    pub user_action_id: Option<String>,
    pub page_route: Option<String>,
    pub release: Option<String>,
    pub service: String,
    pub profile: String,
    pub lane: String,
    pub revision: Option<String>,
    pub action_class: String,
    pub action: String,
    pub method: String,
    pub path: String,
    pub outcome: String,
    pub status: Option<f64>,
    pub latency_ms: f64,
    pub deadline_s: f64,
    pub timed_out: bool,
    pub error_message: Option<String>,
    pub error_category: Option<String>,
    pub error_code: Option<String>,
    pub retry_budget: Option<String>,
    pub retry_budget_max: Option<f64>,
    pub retry_attempts: Option<f64>,
    pub retry_budget_exhausted: Option<bool>,
    pub ds_project: Option<String>,
    pub user_email: Option<String>,
    pub capture_mode: String,
}

/// One activity window, and the count its summary line confirmed.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct SreActivity {
    pub rows: Vec<SreActivityEvent>,
    pub count: u64,
}

/// One window of request activity, newest first, bounded by days and rows.
/// The stream ends with a summary line whose count must match the rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Activity {
    pub days: u32,
    pub limit: u32,
}

impl Exchange for Activity {
    type Output = SreActivity;

    fn plan(&self) -> Result<Plan, Refusal> {
        if !(1..=ACTIVITY_MAX_DAYS).contains(&self.days)
            || !(1..=ACTIVITY_MAX_ROWS).contains(&self.limit)
        {
            return Err(Refusal::new(
                INPUT_INVALID,
                "An activity window spans 1 to 365 days and 1 to 10,000 rows.",
            ));
        }
        let body = json!({"action": "query_table", "table": "sre_requests",
            "days": self.days, "limit": self.limit});
        Ok(Request::post(DATA_PATH, body)
            .timeout_ms(ACTIVITY_TIMEOUT_MS)
            .response_cap(ACTIVITY_CAP)
            .ndjson(self.limit as usize + 1)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        let lines = body.as_array().ok_or_else(|| invalid("activity window"))?;
        let Some((summary, rows)) = lines.split_last() else {
            return Err(invalid("activity window without its summary"));
        };
        if summary.get("__type").and_then(Value::as_str) != Some("summary") {
            return Err(invalid("activity window without its summary"));
        }
        if summary.get("status").and_then(Value::as_str) != Some("ok") {
            return Err(Refusal::new(
                INVALID,
                format!("The activity stream failed after {} rows.", rows.len()),
            ));
        }
        let count = summary
            .get("count")
            .and_then(Value::as_u64)
            .filter(|count| *count == rows.len() as u64)
            .ok_or_else(|| invalid("activity count"))?;
        let rows = rows
            .iter()
            .map(|row| {
                if row.get("__type").is_some() {
                    return Err(invalid("activity window"));
                }
                SreActivityEvent::deserialize(row)
                    .ok()
                    .filter(|event| !event.event_id.trim().is_empty())
                    .ok_or_else(|| invalid("activity row"))
            })
            .collect::<Result<_, _>>()?;
        Ok(Admitted::Done(SreActivity { rows, count }))
    }
}

/// How the monitor reads the box's status snapshot.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum ServerSnapshotRead {
    Ok {
        #[cfg_attr(
            all(feature = "wasm", target_arch = "wasm32"),
            tsify(type = "Record<string, unknown>")
        )]
        snapshot: Value,
        /// Seconds between the reader's clock and the snapshot's stamp.
        age_seconds: u64,
        liveness: Liveness,
    },
    /// The box has never published one.
    Missing,
    /// The caller may not read it.
    Unauthorized,
    /// What came back is not the box's snapshot.
    Malformed,
    /// No answer, or a refusal that says nothing about the snapshot.
    Transport {
        #[serde(skip_serializing_if = "Option::is_none")]
        #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
        http_status: Option<u16>,
    },
}

/// How a snapshot of a given age may be drawn: the publisher writes every
/// ten seconds, so a minute is fresh and five are the most a stale one may be.
#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "snake_case")]
pub enum Liveness {
    Fresh,
    Stale,
    Offline,
}

/// The age of a snapshot stamped `published_unix` at `now_ms`, and what it
/// means for how the snapshot is drawn.
pub fn liveness(published_unix: i64, now_ms: u64) -> (u64, Liveness) {
    let age = ((now_ms / 1000) as i64 - published_unix).unsigned_abs();
    let liveness = match age {
        0..=60 => Liveness::Fresh,
        61..=300 => Liveness::Stale,
        _ => Liveness::Offline,
    };
    (age, liveness)
}

/// The box's status snapshot, admitted by [`server_status`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ServerSnapshot;

impl Exchange for ServerSnapshot {
    type Output = Value;

    fn plan(&self) -> Result<Plan, Refusal> {
        Ok(get(server_status::PATH)
            .timeout_ms(server_status::TIMEOUT_SECONDS * 1000)
            .response_cap(server_status::RESPONSE_LIMIT as u64)
            .into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        server_status::admit(&body)
            .map(Admitted::Done)
            .map_err(|refusal| Refusal::new(refusal.code, refusal.message))
    }
}

/// What a snapshot read means for the monitor, from its answer or refusal.
pub fn snapshot_read(outcome: Result<Value, Refusal>, now_ms: u64) -> ServerSnapshotRead {
    match outcome {
        Ok(snapshot) => {
            let published = snapshot
                .get("published_unix")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let (age_seconds, liveness) = liveness(published, now_ms);
            ServerSnapshotRead::Ok {
                snapshot,
                age_seconds,
                liveness,
            }
        }
        Err(refusal) if refusal.code == SNAPSHOT_MALFORMED => ServerSnapshotRead::Malformed,
        Err(refusal) if refusal.code == SNAPSHOT_MISSING || refusal.status == Some(404) => {
            ServerSnapshotRead::Missing
        }
        Err(refusal) if refusal.signed_out || matches!(refusal.status, Some(401 | 403)) => {
            ServerSnapshotRead::Unauthorized
        }
        Err(refusal) => ServerSnapshotRead::Transport {
            http_status: refusal.status,
        },
    }
}

/// One client event as the page observed it.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(deny_unknown_fields)]
pub struct SreClientEvent {
    /// `client_runtime` or `client_api`.
    pub source: String,
    pub action: String,
    pub action_class: String,
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub page_route: String,
    /// `success` or `failure`.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub status: Option<u16>,
    #[serde(default)]
    pub latency_ms: u64,
    #[serde(default)]
    pub deadline_s: u64,
    #[serde(default)]
    pub timed_out: bool,
    #[serde(default)]
    pub error_message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub ds_project: Option<String>,
    #[serde(default)]
    pub user_visible: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), tsify(optional))]
    pub user_action_id: Option<String>,
}

/// The lane an event is filed under, from the page's deployment lane.
pub fn event_lane(deploy_lane: &str) -> &'static str {
    match deploy_lane.trim().to_ascii_lowercase().as_str() {
        "run" | "canary" => "canary",
        "local" => "local",
        _ => "stable",
    }
}

fn clipped(text: &str, max: usize) -> String {
    text.trim().chars().take(max).collect()
}

/// Report one client event. The request id is the executor's, named here so
/// the event and its request agree.
#[derive(Clone, Debug, PartialEq)]
pub struct ClientEvent {
    pub event: SreClientEvent,
    pub deploy_lane: String,
    pub release: Option<String>,
    pub request_id: String,
}

impl Exchange for ClientEvent {
    type Output = bool;

    fn plan(&self) -> Result<Plan, Refusal> {
        let event = &self.event;
        if !["client_runtime", "client_api"].contains(&event.source.as_str())
            || !["success", "failure"].contains(&event.outcome.as_str())
            || (event.path.trim().is_empty() && event.action.trim().is_empty())
            || self.request_id.trim().is_empty()
        {
            return Err(Refusal::new(
                INPUT_INVALID,
                "A client event names its source, outcome and path or action.",
            ));
        }
        let user_action_id = event
            .user_action_id
            .as_deref()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .unwrap_or(&self.request_id);
        let mut body = Map::new();
        body.insert("source".into(), json!(event.source));
        body.insert("platform".into(), json!("web"));
        body.insert("lane".into(), json!(event_lane(&self.deploy_lane)));
        if let Some(release) = self.release.as_deref().filter(|r| !r.trim().is_empty()) {
            body.insert(
                "release".into(),
                json!(clipped(release, EVENT_TEXT_MAX_CHARS)),
            );
        }
        for (key, text) in [
            ("action", &event.action),
            ("action_class", &event.action_class),
            ("method", &event.method),
            ("path", &event.path),
            ("page_route", &event.page_route),
        ] {
            body.insert(key.into(), json!(clipped(text, EVENT_TEXT_MAX_CHARS)));
        }
        body.insert("outcome".into(), json!(event.outcome));
        if let Some(status) = event.status.filter(|status| *status > 0) {
            body.insert("status".into(), json!(status));
        }
        body.insert("latency_ms".into(), json!(event.latency_ms));
        body.insert("deadline_s".into(), json!(event.deadline_s));
        body.insert("timed_out".into(), json!(event.timed_out));
        body.insert(
            "error_message".into(),
            json!(clipped(&event.error_message, EVENT_MESSAGE_MAX_CHARS)),
        );
        if let Some(project) = event.ds_project.as_deref().filter(|p| !p.trim().is_empty()) {
            body.insert("ds_project".into(), json!(project.trim()));
        }
        body.insert("user_visible".into(), json!(event.user_visible));
        body.insert("request_id".into(), json!(self.request_id));
        body.insert("user_action_id".into(), json!(user_action_id));
        Ok(Request::post(EVENTS_PATH, Value::Object(body)).into())
    }

    fn admit(self, _status: u16, body: Value) -> Result<Admitted<Self>, Refusal> {
        Ok(Admitted::Done(
            data(&body).get("accepted").and_then(Value::as_bool) == Some(true),
        ))
    }

    fn project(&self) -> Option<&str> {
        self.event.ds_project.as_deref()
    }
}

/// The activity window this device last read, painted before any request;
/// no transport.
#[cfg(feature = "durable-store")]
pub async fn held_activity(edge: &impl Edge) -> Result<Option<SreActivity>, Refusal> {
    let observed = retained::read::<SreActivity>(edge, Kind::SreActivity, None, "").await?;
    Ok(observed.held.map(|held| held.value))
}

/// A fresh activity window from Brain, held under the account its read
/// observed so the next open (and an offline one) paints it at once.
#[cfg(feature = "durable-store")]
pub async fn activity(edge: &impl Edge, window: Activity) -> Result<SreActivity, Refusal> {
    let observed = retained::read::<serde::de::IgnoredAny>(edge, Kind::SreActivity, None, "").await.ok();
    let fresh = edge.fetch(window).await?;
    if let Some(observed) = observed {
        let owner = observed.owner.as_deref();
        let _ = retained::write(edge, owner, Kind::SreActivity, None, "", &fresh, None).await;
    }
    Ok(fresh)
}

/// The typed browser exports Svelte calls; each runs the shared executor.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use super::{snapshot_read, Activity, ClientEvent, Overview, ServerSnapshot, SreClientEvent};
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{attempt, facts, rejection, run, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use wasm_bindgen::prelude::*;

    /// The platform's reliability overview.
    #[wasm_bindgen(js_name = sreOverview, unchecked_return_type = "SreOverview")]
    pub async fn sre_overview(host: GatewayHost) -> Result<JsValue, JsValue> {
        typed(&run(&host, Overview).await?)
    }

    /// One window of request activity: `days` back, at most `limit` rows,
    /// held for the next open.
    #[wasm_bindgen(js_name = sreActivity, unchecked_return_type = "SreActivity")]
    pub async fn sre_activity(
        host: GatewayHost,
        store: StoreHost,
        days: u32,
        limit: u32,
    ) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        let fresh = super::activity(&edge, Activity { days, limit })
            .await
            .map_err(|refusal| rejection(&refusal))?;
        typed(&fresh)
    }

    /// The activity window this device holds, or null; no transport.
    #[wasm_bindgen(js_name = sreActivityHeld, unchecked_return_type = "SreActivity | null")]
    pub async fn sre_activity_held(host: GatewayHost, store: StoreHost) -> Result<JsValue, JsValue> {
        let edge = BrowserEdge::new(host, store);
        let held = super::held_activity(&edge)
            .await
            .map_err(|refusal| rejection(&refusal))?;
        typed(&held)
    }

    /// The box's status snapshot as the monitor reads it; never rejects for
    /// a refusal, which is one of the read's outcomes.
    #[wasm_bindgen(js_name = serverSnapshot, unchecked_return_type = "ServerSnapshotRead")]
    pub async fn server_snapshot(host: GatewayHost) -> Result<JsValue, JsValue> {
        let outcome = attempt(&host, ServerSnapshot).await?;
        typed(&snapshot_read(outcome, facts(&host)?.now_ms))
    }

    /// The age and liveness of a snapshot stamped `published_unix`, now.
    #[wasm_bindgen(js_name = serverSnapshotLiveness, unchecked_return_type = "{ age_seconds: number; liveness: Liveness }")]
    pub fn server_snapshot_liveness(published_unix: f64, now_ms: f64) -> Result<JsValue, JsValue> {
        let (age_seconds, liveness) =
            super::liveness(published_unix as i64, now_ms.max(0.0) as u64);
        typed(&serde_json::json!({"age_seconds": age_seconds, "liveness": liveness}))
    }

    /// Report one client event; answers whether the store accepted it.
    /// Never rejects: reporting must not affect what the person sees.
    #[wasm_bindgen(js_name = sreClientEvent)]
    pub async fn sre_client_event(
        host: GatewayHost,
        #[wasm_bindgen(unchecked_param_type = "SreClientEvent")] event: JsValue,
        deploy_lane: String,
        release: Option<String>,
    ) -> bool {
        let Ok(event) = tsify::serde_wasm_bindgen::from_value::<SreClientEvent>(event) else {
            return false;
        };
        let call = ClientEvent {
            event,
            deploy_lane,
            release,
            request_id: uuid::Uuid::new_v4().to_string(),
        };
        matches!(attempt(&host, call).await, Ok(Ok(true)))
    }
}

#[cfg(test)]
#[path = "sre_tests.rs"]
mod tests;
