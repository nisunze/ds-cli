//! Platform reliability: the fleet top line, and a bounded request-event window.
//!
//! Both reads are global. There is no project to select, none to fence and none
//! on the wire — a platform is not a project — and the identity that decides the
//! answer is the restored user's `platform.admin` capability, which ds-brain
//! checks on both routes.
//!
//! Until 2026-09-18 `ds sre overview` and `ds sre events` reached these same two
//! routes through a paired desktop window, which held the same signed-in user
//! and sent the same request. That put the health of the platform behind the one
//! host least likely to be running when it matters: on a server, where an agent
//! is asking why a job failed, platform health was simply unreadable. Nothing
//! about the read needs a window, and nothing here renders anything.
//!
//! ## What leaves this module
//!
//! Not the raw answer. The overview's five collections are each capped, with
//! exact totals and a per-collection `more` flag beside them, so a caller can
//! always tell a short list from a complete one. An event is projected onto a
//! fixed field list with every text field bounded, and each event names the
//! fields that were clipped. A terminal is not a log sink.
//!
//! ## The event window is a stream, and it is verified
//!
//! `/api/v1/data` `query_table` answers NDJSON: one row per line, closed by a
//! `__type: "summary"` trailer carrying the row count and a status. All three
//! guards are load-bearing. A stream that ends without its summary was cut off;
//! a stream whose summary says `error` failed after emitting rows; a summary
//! whose count disagrees with the rows received is a stream that lost some. Each
//! is a refusal, because a partial window silently returned as a whole one is
//! how an operator concludes there were no errors.

use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    ServiceRefusal, SreEventsCall, SreOverviewCall,
};
use serde_json::{json, Map, Value};

/// The registered tabular source this client may read. The route answers many
/// tables; this one is the SRE request-event store, and it is pinned here
/// because a table is a body value a caller must never choose.
pub const EVENTS_TABLE: &str = "sre_requests";

/// The oldest event window BigQuery holds for this source.
pub const MAX_DAYS: i64 = 365;
/// The most events one answer returns.
pub const MAX_EVENTS: i64 = 250;
/// The most events the owner scans before filtering.
pub const MAX_SCAN_EVENTS: i64 = 5_000;
/// The longest exact filter value.
pub const MAX_FILTER_CHARS: usize = 200;
/// The longest projected event text field, except the error message.
pub const MAX_EVENT_TEXT_CHARS: usize = 128;
/// The longest projected error message: the one field worth more room.
pub const MAX_ERROR_MESSAGE_CHARS: usize = 1_000;
/// The most rows any one overview collection returns.
pub const MAX_OVERVIEW_ROWS: usize = 250;

/// The three outcomes an event window may select.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Failure,
    Success,
    All,
}

impl Outcome {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Failure => "failure",
            Self::Success => "success",
            Self::All => "all",
        }
    }
    pub fn parse(value: &str) -> Result<Self, ClientError> {
        match value {
            "failure" => Ok(Self::Failure),
            "success" => Ok(Self::Success),
            "all" => Ok(Self::All),
            _ => Err(invalid("`outcome` must be failure, success or all")),
        }
    }
}

/// The exact, case-insensitive filters one event window applies.
///
/// `project` is metadata: it selects events whose `ds_project` matches, and it
/// neither requires nor selects an active project.
#[derive(Debug, Clone, Default)]
pub struct Filters {
    pub service: Option<String>,
    pub category: Option<String>,
    pub lane: Option<String>,
    pub action: Option<String>,
    pub project: Option<String>,
    pub source: Option<String>,
}

impl Filters {
    fn validate(&self) -> Result<(), ClientError> {
        for (_, value) in self.named() {
            if let Some(value) = value {
                if value.is_empty()
                    || value.trim() != value
                    || value.chars().count() > MAX_FILTER_CHARS
                {
                    return Err(invalid(
                        "every event filter must be one non-empty trimmed value of at most 200 characters",
                    ));
                }
            }
        }
        Ok(())
    }
    fn named(&self) -> [(&'static str, Option<&String>); 6] {
        [
            ("service", self.service.as_ref()),
            ("category", self.category.as_ref()),
            ("lane", self.lane.as_ref()),
            ("action", self.action.as_ref()),
            ("project", self.project.as_ref()),
            ("source", self.source.as_ref()),
        ]
    }
    fn declared(&self, outcome: Outcome) -> Value {
        let mut out = Map::new();
        out.insert("outcome".into(), json!(outcome.token()));
        for (name, value) in self.named() {
            out.insert(name.into(), value.map(|v| json!(v)).unwrap_or(Value::Null));
        }
        Value::Object(out)
    }
}

/// One reliability read.
pub enum Command {
    /// The fleet top line. It takes nothing, because it selects nothing.
    Overview,
    /// One bounded newest-first window of request events.
    Events {
        days: i64,
        limit: i64,
        scan_limit: i64,
        outcome: Outcome,
        filters: Filters,
    },
}

impl Command {
    pub fn validate(&self) -> Result<(), ClientError> {
        let Self::Events {
            days,
            limit,
            scan_limit,
            filters,
            ..
        } = self
        else {
            return Ok(());
        };
        bound(
            *days,
            MAX_DAYS,
            "`days` must be a whole number from 1 through 365",
        )?;
        bound(
            *limit,
            MAX_EVENTS,
            "`limit` must be a whole number from 1 through 250",
        )?;
        bound(
            *scan_limit,
            MAX_SCAN_EVENTS,
            "`scan_limit` must be a whole number from 1 through 5000",
        )?;
        filters.validate()
    }
}

fn bound(value: i64, max: i64, message: &'static str) -> Result<(), ClientError> {
    if value < 1 || value > max {
        return Err(invalid(message));
    }
    Ok(())
}

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

fn unreadable(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, message)
}

/// One refusal from either reliability route.
///
/// 403 is the one an operator meets most: both routes are gated by
/// `platform.admin`, and a signed-in user without it is authenticated and not
/// authorized. Saying so exactly is the difference between "sign in again" and
/// "ask for reliability access".
fn refusal(status: u16, body: &[u8]) -> ClientError {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let envelope = ServiceRefusal::new(
        status,
        None,
        value["error"]
            .as_str()
            .or_else(|| value["message"].as_str()),
    );
    let (kind, message) = match status {
        400 | 422 => (
            ErrorKind::InvalidInput,
            "the reliability authority refused this window",
        ),
        401 => (
            ErrorKind::AuthenticationRejected,
            "the reliability authority did not accept this identity",
        ),
        403 => (
            ErrorKind::AuthenticationRejected,
            "this account may not read platform reliability",
        ),
        _ => (
            ErrorKind::Transient,
            "the reliability authority could not answer this read",
        ),
    };
    if envelope.message().is_none() {
        return ClientError::new(kind, message);
    }
    ClientError::with_service_refusal(kind, message, envelope)
}

// ---------------------------------------------------------------------------
// The overview
// ---------------------------------------------------------------------------

/// The five bounded collections the overview carries, in the order the owner
/// returns them.
const OVERVIEW_COLLECTIONS: [&str; 5] = [
    "services",
    "service_ops",
    "stale",
    "incidents",
    "error_catalog",
];

/// Project the owner's overview onto a bounded answer.
///
/// Public because it is pure: a host proves what it renders without a
/// transport. Every collection is capped, every exact total is kept beside the
/// cap, and `more` names each collection that was cut — so an empty
/// `incidents` says "the owner's feed is empty", never "nothing was truncated".
pub fn project_overview(body: &Value) -> Result<Value, ClientError> {
    if !body.is_object() {
        return Err(unreadable("the reliability overview is not an object"));
    }
    let mut totals = Map::new();
    let mut more = Map::new();
    let mut out = Map::new();
    out.insert(
        "generated_at".into(),
        json!(body["generated_at"].as_str().unwrap_or_default()),
    );
    out.insert("fleet".into(), body["fleet"].clone());
    out.insert("combined_reports".into(), body["combined_reports"].clone());
    for name in OVERVIEW_COLLECTIONS {
        let rows = match &body[name] {
            Value::Array(rows) => rows.clone(),
            Value::Null => Vec::new(),
            _ => {
                return Err(unreadable(
                    "a reliability overview collection is not a list",
                ))
            }
        };
        totals.insert(name.into(), json!(rows.len()));
        more.insert(name.into(), json!(rows.len() > MAX_OVERVIEW_ROWS));
        out.insert(
            name.into(),
            Value::Array(rows.into_iter().take(MAX_OVERVIEW_ROWS).collect()),
        );
    }
    out.insert("totals".into(), Value::Object(totals));
    out.insert("more".into(), Value::Object(more));
    Ok(Value::Object(out))
}

// ---------------------------------------------------------------------------
// The event window
// ---------------------------------------------------------------------------

/// Every field one projected event carries, and how much text each may hold.
/// `None` means the value is passed through as it arrived — a number, a boolean
/// or null — because bounding those would change what they mean.
const EVENT_FIELDS: [(&str, Option<usize>); 33] = [
    ("event_id", Some(MAX_EVENT_TEXT_CHARS)),
    ("ts", Some(MAX_EVENT_TEXT_CHARS)),
    ("trace_id", Some(MAX_EVENT_TEXT_CHARS)),
    ("request_id", Some(MAX_EVENT_TEXT_CHARS)),
    ("user_action_id", Some(MAX_EVENT_TEXT_CHARS)),
    ("container_kind", Some(MAX_EVENT_TEXT_CHARS)),
    ("platform", Some(MAX_EVENT_TEXT_CHARS)),
    ("user_visible", None),
    ("page_route", Some(MAX_EVENT_TEXT_CHARS)),
    ("release", Some(MAX_EVENT_TEXT_CHARS)),
    ("service", Some(MAX_EVENT_TEXT_CHARS)),
    ("profile", Some(MAX_EVENT_TEXT_CHARS)),
    ("lane", Some(MAX_EVENT_TEXT_CHARS)),
    ("revision", Some(MAX_EVENT_TEXT_CHARS)),
    ("source", Some(MAX_EVENT_TEXT_CHARS)),
    ("action_class", Some(MAX_EVENT_TEXT_CHARS)),
    ("action", Some(MAX_EVENT_TEXT_CHARS)),
    ("method", Some(MAX_EVENT_TEXT_CHARS)),
    ("path", Some(MAX_EVENT_TEXT_CHARS)),
    ("outcome", Some(MAX_EVENT_TEXT_CHARS)),
    ("status", None),
    ("latency_ms", None),
    ("deadline_s", None),
    ("timed_out", None),
    ("error_message", Some(MAX_ERROR_MESSAGE_CHARS)),
    ("error_category", Some(MAX_EVENT_TEXT_CHARS)),
    ("error_code", Some(MAX_EVENT_TEXT_CHARS)),
    ("retry_budget", Some(MAX_EVENT_TEXT_CHARS)),
    ("retry_budget_max", None),
    ("retry_attempts", None),
    ("retry_budget_exhausted", None),
    ("ds_project", Some(MAX_EVENT_TEXT_CHARS)),
    ("user_email", Some(MAX_EVENT_TEXT_CHARS)),
];

fn project_event(row: &Value) -> Value {
    let mut truncated: Vec<Value> = Vec::new();
    let mut out = Map::new();
    for (field, bound) in EVENT_FIELDS {
        let value = &row[field];
        match bound {
            None => {
                out.insert(field.into(), value.clone());
            }
            Some(max) => match value.as_str() {
                None => {
                    out.insert(field.into(), Value::Null);
                }
                Some(text) if text.chars().count() <= max => {
                    out.insert(field.into(), json!(text));
                }
                Some(text) => {
                    truncated.push(json!(field));
                    out.insert(
                        field.into(),
                        json!(text.chars().take(max).collect::<String>()),
                    );
                }
            },
        }
    }
    out.insert(
        "error_message_truncated".into(),
        json!(truncated.iter().any(|f| f == "error_message")),
    );
    out.insert("truncated_fields".into(), Value::Array(truncated));
    Value::Object(out)
}

/// One exact, case-insensitive match. An absent filter matches everything; a
/// present one matches an absent field only when the field is empty.
fn matches(value: &Value, filter: Option<&String>) -> bool {
    let Some(filter) = filter else {
        return true;
    };
    value
        .as_str()
        .unwrap_or_default()
        .eq_ignore_ascii_case(filter)
}

/// Filter and bound one scanned window.
///
/// Public because it is pure. Duplicate event ids appear transiently while a
/// server timestamp materializes; the materialized row wins, exactly as the
/// owner's own page resolves it.
pub fn project_events(
    rows: &[Value],
    outcome: Outcome,
    filters: &Filters,
    limit: i64,
    scan_limit: i64,
) -> Value {
    let mut order: Vec<String> = Vec::new();
    let mut deduped: std::collections::HashMap<String, Value> = std::collections::HashMap::new();
    for row in rows {
        let id = row["event_id"].as_str().unwrap_or_default().to_string();
        match deduped.get(&id) {
            None => {
                order.push(id.clone());
                deduped.insert(id, row.clone());
            }
            Some(existing) => {
                if existing["ts"].is_null() && !row["ts"].is_null() {
                    deduped.insert(id, row.clone());
                }
            }
        }
    }
    let matching: Vec<&Value> = order
        .iter()
        .filter_map(|id| deduped.get(id))
        .filter(|row| {
            (outcome == Outcome::All
                || row["outcome"]
                    .as_str()
                    .is_some_and(|value| value.eq_ignore_ascii_case(outcome.token())))
                && matches(&row["service"], filters.service.as_ref())
                && matches(&row["error_category"], filters.category.as_ref())
                && matches(&row["lane"], filters.lane.as_ref())
                && matches(&row["action"], filters.action.as_ref())
                && matches(&row["ds_project"], filters.project.as_ref())
                && matches(&row["source"], filters.source.as_ref())
        })
        .collect();
    let limit = limit.max(0) as usize;
    json!({
        "filters": filters.declared(outcome),
        "scanned": rows.len(),
        "matching": matching.len(),
        "returned": matching.len().min(limit),
        "events": matching.iter().take(limit).map(|row| project_event(row)).collect::<Vec<_>>(),
        "more": {
            "matching": matching.len() > limit,
            "scan": rows.len() as i64 >= scan_limit,
        },
    })
}

/// Read one NDJSON window: the rows, verified against their own summary.
///
/// Public because it is pure, and because what it refuses is the point: a
/// window that lost rows must not read as a window that found none.
pub fn read_event_stream(body: &[u8]) -> Result<Vec<Value>, ClientError> {
    let text = std::str::from_utf8(body)
        .map_err(|_| unreadable("the reliability event stream is not text"))?;
    let mut rows: Vec<Value> = Vec::new();
    let mut summary: Option<Value> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if summary.is_some() {
            return Err(unreadable(
                "the reliability event stream continued after its closing summary",
            ));
        }
        let value: Value = serde_json::from_str(line).map_err(|_| {
            unreadable("the reliability event stream contains a line that is not an object")
        })?;
        if !value.is_object() {
            return Err(unreadable(
                "the reliability event stream contains a line that is not an object",
            ));
        }
        if value["__type"] == json!("summary") {
            summary = Some(value);
            continue;
        }
        rows.push(value);
    }
    let Some(summary) = summary else {
        return Err(unreadable(
            "the reliability event stream ended without its closing summary",
        ));
    };
    if summary["status"] != json!("ok") {
        return Err(unreadable(
            "the reliability event stream failed before it completed",
        ));
    }
    let counted = summary["count"]
        .as_u64()
        .ok_or_else(|| unreadable("the reliability event stream reported no row count"))?;
    if counted != rows.len() as u64 {
        return Err(unreadable(
            "the reliability event stream delivered fewer rows than its summary counted",
        ));
    }
    Ok(rows)
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    command: &Command,
) -> Result<Value, ClientError> {
    command.validate()?;
    match command {
        Command::Overview => {
            let response = transport.sre_overview(SreOverviewCall {
                profile,
                authority: authority.call(),
            })?;
            if response.status != 200 {
                return Err(refusal(response.status, &response.body));
            }
            if response.body.len() > crate::transport::SRE_OVERVIEW_RESPONSE_LIMIT {
                return Err(unreadable("the reliability overview exceeds its bound"));
            }
            let body: Value = serde_json::from_slice(&response.body)
                .map_err(|_| unreadable("the reliability overview is not JSON"))?;
            project_overview(&body)
        }
        Command::Events {
            days,
            limit,
            scan_limit,
            outcome,
            filters,
        } => {
            let response = transport.sre_events(SreEventsCall {
                profile,
                authority: authority.call(),
                payload: json!({
                    "action": crate::SRE_EVENTS_ACTIONS[0],
                    "table": EVENTS_TABLE,
                    "days": days,
                    "limit": scan_limit,
                }),
            })?;
            if response.status != 200 {
                return Err(refusal(response.status, &response.body));
            }
            if response.body.len() > crate::transport::SRE_EVENTS_RESPONSE_LIMIT {
                return Err(unreadable("the reliability event window exceeds its bound"));
            }
            let rows = read_event_stream(&response.body)?;
            let mut out = project_events(&rows, *outcome, filters, *limit, *scan_limit);
            out["window_days"] = json!(days);
            out["scan_limit"] = json!(scan_limit);
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, outcome: &str, service: &str) -> Value {
        json!({
            "event_id": id,
            "ts": "2026-09-18T00:00:00Z",
            "outcome": outcome,
            "service": service,
            "lane": "stable",
            "action": "export_transformers",
            "error_category": "container_pressure",
            "ds_project": "kigali",
            "source": "forwarded_compute",
            "status": 503,
            "latency_ms": 8659,
            "timed_out": false,
        })
    }

    fn stream(rows: &[Value], status: &str) -> Vec<u8> {
        let mut out = String::new();
        for row in rows {
            out.push_str(&serde_json::to_string(row).unwrap());
            out.push('\n');
        }
        out.push_str(&format!(
            "{{\"__type\":\"summary\",\"table\":\"sre_requests\",\"count\":{},\"status\":\"{status}\"}}\n",
            rows.len()
        ));
        out.into_bytes()
    }

    #[test]
    fn a_window_that_lost_rows_is_refused_not_shortened() {
        let rows = [event("a", "failure", "ds-brain")];
        let complete = stream(&rows, "ok");
        assert_eq!(read_event_stream(&complete).unwrap().len(), 1);

        // The summary counts a row the stream never delivered.
        let short = String::from_utf8(complete.clone())
            .unwrap()
            .replace("\"count\":1", "\"count\":2")
            .into_bytes();
        assert_eq!(
            read_event_stream(&short).unwrap_err().kind(),
            ErrorKind::UnreadableResponse
        );
        // No summary at all: the connection ended early.
        let cut = b"{\"event_id\":\"a\"}\n".to_vec();
        assert_eq!(
            read_event_stream(&cut).unwrap_err().kind(),
            ErrorKind::UnreadableResponse
        );
        // The owner says its own scan failed.
        assert_eq!(
            read_event_stream(&stream(&rows, "error"))
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
    }

    #[test]
    fn filters_are_exact_and_case_insensitive() {
        let rows = vec![
            event("a", "failure", "ds-network-reporter"),
            event("b", "success", "ds-brain"),
            event("c", "failure", "ds-brain"),
        ];
        let mut filters = Filters::default();
        filters.service = Some("DS-Brain".into());
        let answer = project_events(&rows, Outcome::Failure, &filters, 50, 1_000);
        assert_eq!(answer["matching"], json!(1));
        assert_eq!(answer["events"][0]["event_id"], json!("c"));
        // A filter is exact: a prefix of a service name matches nothing.
        filters.service = Some("ds-".into());
        assert_eq!(
            project_events(&rows, Outcome::Failure, &filters, 50, 1_000)["matching"],
            json!(0)
        );
    }

    #[test]
    fn a_materialized_row_wins_over_its_pending_twin() {
        let mut pending = event("a", "failure", "ds-brain");
        pending["ts"] = Value::Null;
        let rows = vec![pending, event("a", "failure", "ds-brain")];
        let answer = project_events(&rows, Outcome::All, &Filters::default(), 50, 1_000);
        assert_eq!(answer["matching"], json!(1));
        assert_eq!(answer["events"][0]["ts"], json!("2026-09-18T00:00:00Z"));
    }

    #[test]
    fn a_truncated_field_says_so_by_name() {
        let mut row = event("a", "failure", "ds-brain");
        row["error_message"] = json!("x".repeat(MAX_ERROR_MESSAGE_CHARS + 1));
        row["action"] = json!("y".repeat(MAX_EVENT_TEXT_CHARS + 1));
        let projected = project_event(&row);
        assert_eq!(
            projected["truncated_fields"],
            json!(["action", "error_message"])
        );
        assert_eq!(projected["error_message_truncated"], json!(true));
        assert_eq!(
            projected["error_message"].as_str().unwrap().chars().count(),
            MAX_ERROR_MESSAGE_CHARS
        );
    }

    #[test]
    fn a_saturated_scan_and_a_clipped_answer_are_both_declared() {
        let rows: Vec<Value> = (0..10)
            .map(|i| event(&format!("e{i}"), "failure", "ds-brain"))
            .collect();
        let answer = project_events(&rows, Outcome::Failure, &Filters::default(), 3, 10);
        assert_eq!(answer["returned"], json!(3));
        assert_eq!(answer["matching"], json!(10));
        assert_eq!(answer["more"]["matching"], json!(true));
        assert_eq!(answer["more"]["scan"], json!(true));
    }

    #[test]
    fn an_overview_keeps_exact_totals_beside_its_caps() {
        let services: Vec<Value> = (0..MAX_OVERVIEW_ROWS + 7)
            .map(|i| json!({ "key": i }))
            .collect();
        let answer = project_overview(&json!({
            "generated_at": "2026-09-18T00:00:00Z",
            "services": services,
            "service_ops": [],
            "incidents": [],
        }))
        .unwrap();
        assert_eq!(
            answer["services"].as_array().unwrap().len(),
            MAX_OVERVIEW_ROWS
        );
        assert_eq!(answer["totals"]["services"], json!(MAX_OVERVIEW_ROWS + 7));
        assert_eq!(answer["more"]["services"], json!(true));
        // An empty feed is a fact, not a truncation.
        assert_eq!(answer["totals"]["incidents"], json!(0));
        assert_eq!(answer["more"]["incidents"], json!(false));
        // A collection the owner did not send is zero, not a refusal.
        assert_eq!(answer["totals"]["stale"], json!(0));
    }

    #[test]
    fn every_bound_is_checked_before_the_window_is_opened() {
        let window = |days, limit, scan| Command::Events {
            days,
            limit,
            scan_limit: scan,
            outcome: Outcome::Failure,
            filters: Filters::default(),
        };
        assert!(window(3, 50, 1_000).validate().is_ok());
        for bad in [
            window(0, 50, 1_000),
            window(MAX_DAYS + 1, 50, 1_000),
            window(3, 0, 1_000),
            window(3, MAX_EVENTS + 1, 1_000),
            window(3, 50, 0),
            window(3, 50, MAX_SCAN_EVENTS + 1),
        ] {
            assert_eq!(bad.validate().unwrap_err().kind(), ErrorKind::InvalidInput);
        }
        let mut filters = Filters::default();
        filters.lane = Some(" stable".into());
        assert_eq!(
            Command::Events {
                days: 3,
                limit: 50,
                scan_limit: 1_000,
                outcome: Outcome::All,
                filters,
            }
            .validate()
            .unwrap_err()
            .kind(),
            ErrorKind::InvalidInput
        );
    }

    #[test]
    fn a_missing_capability_is_named_as_one() {
        let denied = refusal(403, br#"{"error":"platform.admin capability required"}"#);
        assert_eq!(denied.kind(), ErrorKind::AuthenticationRejected);
        assert!(denied
            .to_string()
            .contains("may not read platform reliability"));
    }
}
