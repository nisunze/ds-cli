//! One gateway exchange, decided once in Rust for every host.
//!
//! A caller names what it wants: one typed operation of one domain, an
//! [`Exchange`] whose answer is its typed `Output`. An executor performs the
//! effect each returned [`Step`] names — one request, or one pause — and
//! reports what it observed. Everything else is decided here: the route,
//! method, query and body; the deadline and the response bound; what a
//! status, a backend error envelope or a transport failure means
//! ([`crate::response_policy`]); whether and when to send again; and what the
//! caller receives, its typed answer or a typed [`Refusal`].
//!
//! In the browser and the Desktop webview the executor is `browser`, which
//! sends with reqwest's WebAssembly client (the page's own `fetch`) behind
//! each domain's typed WASM exports. Nothing here assumes a browser: Core
//! runs the same typed exchanges natively. Each domain is a submodule owning
//! its operations' requests and answers; adding one never changes an
//! executor.
//!
//! Pure: no clock, IO, randomness or network. The executor's clock and
//! session facts arrive with each observation, and jitter is its to add.

use std::fmt::Debug;

use serde::Serialize;
use serde_json::{json, Value};

use crate::response_policy::{self, Classify, Outcome, Telemetry, RESPONSE_POLICY_SCHEMA};

pub mod help;
pub mod maps;

#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
pub(crate) mod browser;

/// The client deadline a call inherits unless it names its own.
pub const DEFAULT_TIMEOUT_MS: u64 = 30_000;
/// The response bound a call inherits unless it names its own.
pub const DEFAULT_RESPONSE_CAP: u64 = 1024 * 1024;
/// How much backend text a refusal or an SRE report carries.
const MESSAGE_MAX_CHARS: usize = response_policy::MESSAGE_MAX_CHARS;

pub const SIGNED_OUT: &str = "SIGNED_OUT";
pub const REQUEST_TIMEOUT: &str = "REQUEST_TIMEOUT";
pub const NETWORK_ERROR: &str = "NETWORK_ERROR";
pub const NETWORK_OFFLINE: &str = "NETWORK_OFFLINE";
pub const RESPONSE_TOO_LARGE: &str = "RESPONSE_TOO_LARGE";
pub const RESPONSE_MALFORMED: &str = "RESPONSE_MALFORMED";
pub const PROJECT_INVALID: &str = "project_invalid";

const SIGNED_OUT_MESSAGE: &str = "Request ignored because the session has ended.";

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    Get,
    Post,
    Put,
    Patch,
    Delete,
}

impl Method {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
        }
    }
}

/// The one request an executor sends. Every field is decided here.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Request {
    pub method: Method,
    /// Gateway-relative path; the executor joins it to its gateway origin.
    pub path: String,
    /// Query pairs in order, for a read that carries its input in the URL.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub query: Vec<(String, String)>,
    /// The JSON body, sent verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<Value>,
    /// Zero waits for completion without a client deadline.
    pub timeout_ms: u64,
    /// The most response bytes the executor reads before it stops and
    /// reports the answer as oversized.
    pub response_cap_bytes: u64,
    /// Whether the request carries `X-DS-Processing-Lane`.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub lane_header: bool,
    /// How a successful answer's body is encoded.
    #[serde(skip_serializing_if = "Encoding::is_json")]
    pub encoding: Encoding,
}

/// How a successful answer's body is encoded. A refusal's body is read as
/// JSON whatever the request named.
#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Encoding {
    #[default]
    Json,
    /// One JSON value per line, admitted as one array in line order. Blank
    /// lines are skipped; a malformed line or more than `max_lines` values
    /// refuses the answer.
    Ndjson { max_lines: usize },
}

impl Encoding {
    fn is_json(&self) -> bool {
        *self == Self::Json
    }
}

impl Request {
    /// A JSON POST under the default deadline and bound.
    pub fn post(path: &str, body: Value) -> Self {
        Self {
            method: Method::Post,
            path: path.into(),
            query: Vec::new(),
            body: Some(body),
            timeout_ms: DEFAULT_TIMEOUT_MS,
            response_cap_bytes: DEFAULT_RESPONSE_CAP,
            lane_header: false,
            encoding: Encoding::Json,
        }
    }

    pub fn timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = timeout_ms;
        self
    }

    pub fn response_cap(mut self, bytes: u64) -> Self {
        self.response_cap_bytes = bytes;
        self
    }

    /// Read a successful answer as NDJSON of at most `max_lines` values.
    pub fn ndjson(mut self, max_lines: usize) -> Self {
        self.encoding = Encoding::Ndjson { max_lines };
        self
    }

    /// The verb a multiplexed route names, for SRE classification.
    fn verb(&self, key: &str) -> Option<String> {
        let from_body = self
            .body
            .as_ref()
            .and_then(|body| body.get(key))
            .and_then(Value::as_str);
        let from_query = self
            .query
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.as_str());
        from_body
            .or(from_query)
            .filter(|value| !value.trim().is_empty())
            .map(str::to_owned)
    }
}

/// What one call sends next, and how many times a failure the response
/// policy calls safe to resend may be sent again.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub request: Request,
    pub retries: u32,
}

impl From<Request> for Plan {
    fn from(request: Request) -> Self {
        Self {
            request,
            retries: 0,
        }
    }
}

/// What a successful answer means for a call: its result, or the call that
/// sends the next request (a page walk).
#[derive(Debug, PartialEq)]
pub enum Admitted<C: Exchange> {
    Done(C::Output),
    Next(C),
}

/// One typed operation: the request it sends and what a 2xx answer means.
/// Everything else about the exchange is decided once, below.
pub trait Exchange: Sized {
    type Output: Debug;
    fn plan(&self) -> Result<Plan, Refusal>;
    fn admit(self, status: u16, body: Value) -> Result<Admitted<Self>, Refusal>;
    /// The project an SRE report names, when the call has one.
    fn project(&self) -> Option<&str> {
        None
    }
}

/// The SRE client event a refusal is worth. The executor adds its client's
/// platform, lane, release, page and correlation facts and posts it.
#[derive(Clone, Debug, Serialize, PartialEq)]
#[cfg_attr(all(feature = "wasm", target_arch = "wasm32"), derive(tsify::Tsify))]
pub struct Report {
    pub action: String,
    pub action_class: &'static str,
    pub method: Method,
    pub path: String,
    pub status: u16,
    pub error_message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ds_project: Option<String>,
    pub deadline_s: u64,
}

/// Why a call ends without a result.
#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct Refusal {
    /// The backend's `error_code`, `HTTP_<status>` when it named none, or the
    /// typed client or domain code.
    pub code: String,
    /// What the operator is told.
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    pub transient: bool,
    pub retryable: bool,
    /// 401/403 after the session ended: inert, never an error to show.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub signed_out: bool,
    /// No answer arrived, so the request may or may not have been applied.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub outcome_unknown: bool,
    /// The backend's own text when `message` replaced it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub technical: Option<String>,
    /// The backend's structured `details`, verbatim.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<Report>,
}

impl Refusal {
    /// A refusal a call decides itself: invalid input or an inadmissible answer.
    pub fn new(code: &str, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            status: None,
            transient: false,
            retryable: false,
            signed_out: false,
            outcome_unknown: false,
            technical: None,
            details: None,
            report: None,
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for Refusal {}

/// The device's own offline mode: no request leaves while it is on.
pub fn offline_refusal() -> Refusal {
    Refusal::new(NETWORK_OFFLINE, crate::connectivity::OFFLINE_MESSAGE)
}

/// Where an exchange stands; the executor hands it back unchanged.
#[derive(Clone, Debug, PartialEq)]
pub struct Continuation<C> {
    exchange: C,
    /// Attempts already spent on the current request.
    attempt: u32,
}

pub enum Step<C: Exchange> {
    /// Send this request and report the answer with its body bytes.
    Fetch {
        request: Request,
        continuation: Continuation<C>,
    },
    /// Pause `delay_ms` plus up to `jitter_ms`, then report `Waited`.
    Wait {
        delay_ms: u64,
        jitter_ms: u64,
        continuation: Continuation<C>,
    },
    Done(C::Output),
    Refused(Refusal),
}

/// Why the executor received no answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The request's own deadline passed.
    Timeout,
    /// The link failed while the platform reported a network.
    Network,
    /// The platform reported no network at all.
    Offline,
    /// The answer exceeded the request's response bound.
    Oversized,
}

/// What the executor observed. A response's body bytes travel beside it.
#[derive(Clone, Debug, PartialEq)]
pub enum Observed {
    Response {
        status: u16,
        content_type: Option<String>,
        /// The verbatim `Retry-After` header value.
        retry_after: Option<String>,
    },
    Failure {
        cause: Failure,
        message: Option<String>,
    },
    Waited,
}

/// Host facts read at the moment of an observation, never decisions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    pub now_ms: u64,
    pub has_session: bool,
    pub offline_mode: bool,
}

/// The first step of a call.
pub fn start<C: Exchange>(exchange: C) -> Step<C> {
    match exchange.plan() {
        Ok(plan) => Step::Fetch {
            request: plan.request,
            continuation: Continuation {
                exchange,
                attempt: 0,
            },
        },
        Err(refusal) => Step::Refused(refusal),
    }
}

/// The step after the executor observed the previous one's effect. `body` is
/// the response's bytes, empty for anything but a response.
pub fn resume<C: Exchange>(
    continuation: Continuation<C>,
    observed: Observed,
    body: &[u8],
    facts: &Facts,
) -> Step<C> {
    let Continuation { exchange, attempt } = continuation;
    let plan = match exchange.plan() {
        Ok(plan) => plan,
        Err(refusal) => return Step::Refused(refusal),
    };
    match observed {
        Observed::Waited => Step::Fetch {
            request: plan.request,
            continuation: Continuation { exchange, attempt },
        },
        Observed::Response {
            status,
            content_type,
            retry_after,
        } => {
            if body.len() as u64 > plan.request.response_cap_bytes {
                return Step::Refused(oversized(&plan.request));
            }
            if !(200..300).contains(&status) {
                let value = decode_body(status, content_type.as_deref(), body);
                return answered(exchange, &plan, attempt, status, retry_after, &value, facts);
            }
            let value = match plan.request.encoding {
                Encoding::Json => decode_body(status, content_type.as_deref(), body),
                Encoding::Ndjson { max_lines } => match decode_ndjson(body, max_lines) {
                    Ok(lines) => lines,
                    Err(refusal) => return Step::Refused(refusal),
                },
            };
            match exchange.admit(status, value) {
                Ok(Admitted::Done(value)) => Step::Done(value),
                Ok(Admitted::Next(next)) => start(next),
                Err(refusal) => Step::Refused(refusal),
            }
        }
        Observed::Failure { cause, message } => {
            failed(exchange, &plan, attempt, cause, message, facts)
        }
    }
}

/// Brain answers `{success, data}`; a few handlers answer bare. The member a
/// call reads is `data` when present and non-null, else the answer itself.
pub fn data(body: &Value) -> &Value {
    body.get("data")
        .filter(|data| !data.is_null())
        .unwrap_or(body)
}

/// A project reference a call may send: the Kernel's one project-id rule.
pub fn project_reference(project: &str) -> Result<&str, Refusal> {
    if crate::execution_context::valid_project(project) {
        Ok(project)
    } else {
        Err(Refusal::new(
            PROJECT_INVALID,
            "A request names one valid project.",
        ))
    }
}

/// The answer as a JSON value, read the way the gateway serves it: JSON by
/// content type, other text as `{message}`, and nothing as `{}`.
fn decode_body(status: u16, content_type: Option<&str>, bytes: &[u8]) -> Value {
    if status == 204 || bytes.is_empty() {
        return json!({});
    }
    let is_json =
        content_type.is_some_and(|kind| kind.to_ascii_lowercase().contains("application/json"));
    if is_json {
        return match serde_json::from_slice::<Value>(bytes) {
            Ok(value @ (Value::Object(_) | Value::Array(_))) => value,
            Ok(other) => json!({ "data": other }),
            Err(_) => json!({}),
        };
    }
    let text = String::from_utf8_lossy(bytes);
    if text.is_empty() {
        json!({})
    } else {
        json!({ "message": text })
    }
}

/// An NDJSON answer as one array in line order. Blank lines are skipped; a
/// line that is not JSON, or more than `max_lines` values, refuses it.
fn decode_ndjson(bytes: &[u8], max_lines: usize) -> Result<Value, Refusal> {
    let mut values = Vec::new();
    for (index, line) in bytes.split(|byte| *byte == b'\n').enumerate() {
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if values.len() == max_lines {
            return Err(Refusal::new(
                RESPONSE_TOO_LARGE,
                format!("The answer exceeded its {max_lines} line bound."),
            ));
        }
        let value = serde_json::from_slice(line).map_err(|_| {
            Refusal::new(
                RESPONSE_MALFORMED,
                format!("The answer's line {} is not JSON.", index + 1),
            )
        })?;
        values.push(value);
    }
    Ok(Value::Array(values))
}

fn nonempty(value: Option<&Value>) -> Option<&str> {
    value
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
}

/// The backend's words for a refusal, in the order Brain and its proxies put
/// them.
fn backend_message(status: u16, body: &Value) -> String {
    nonempty(body.get("error"))
        .or_else(|| nonempty(body.get("error").and_then(|error| error.get("message"))))
        .or_else(|| nonempty(body.get("message")))
        .or_else(|| nonempty(body.get("error_message")))
        .or_else(|| nonempty(body.get("detail")))
        .or_else(|| nonempty(body.get("detail").and_then(|detail| detail.get("message"))))
        .map(|text| text.chars().take(MESSAGE_MAX_CHARS).collect())
        .unwrap_or_else(|| format!("Request failed with status {status}"))
}

fn backend_code(body: &Value) -> Option<String> {
    nonempty(body.get("error").and_then(|error| error.get("code")))
        .or_else(|| nonempty(body.get("detail").and_then(|detail| detail.get("code"))))
        .or_else(|| nonempty(body.get("error_code")))
        .map(str::to_owned)
}

fn backend_details(body: &Value) -> Option<Value> {
    [
        body.get("error").and_then(|error| error.get("details")),
        body.get("detail").and_then(|detail| detail.get("details")),
        body.get("error_details"),
    ]
    .into_iter()
    .flatten()
    .find(|details| !details.is_null())
    .cloned()
}

struct Observation<'a> {
    status: Option<u16>,
    error_code: Option<&'a str>,
    timed_out: bool,
    retry_after: Option<String>,
    message: Option<&'a str>,
}

fn classify(
    attempt: u32,
    retries: u32,
    facts: &Facts,
    seen: Observation<'_>,
) -> response_policy::Decision {
    response_policy::classify(Classify {
        schema: RESPONSE_POLICY_SCHEMA.into(),
        kind: "classify".into(),
        status: seen.status,
        error_code: seen.error_code.map(str::to_owned),
        timed_out: seen.timed_out,
        attempt,
        retries_allowed: retries,
        retry_after: seen.retry_after,
        now_ms: facts.now_ms,
        has_session: facts.has_session,
        offline_mode: facts.offline_mode,
        message: seen.message.map(str::to_owned),
    })
    .expect("the exchange always names the response policy's own schema")
}

fn deadline_s(request: &Request) -> u64 {
    if request.timeout_ms == 0 {
        0
    } else {
        request.timeout_ms.div_ceil(1000).max(1)
    }
}

fn report<C: Exchange>(
    exchange: &C,
    request: &Request,
    status: u16,
    message: &str,
) -> Option<Report> {
    let classification = response_policy::telemetry(Telemetry {
        schema: RESPONSE_POLICY_SCHEMA.into(),
        kind: "telemetry".into(),
        route: request.path.clone(),
        path: request.path.clone(),
        action: request.verb("action"),
        operation: request.verb("operation"),
        job_action: request.verb("job_action"),
    })
    .ok()?;
    Some(Report {
        action: classification.action,
        action_class: classification.action_class,
        method: request.method,
        path: request.path.clone(),
        status,
        error_message: message.chars().take(MESSAGE_MAX_CHARS).collect(),
        ds_project: exchange.project().map(str::to_owned),
        deadline_s: deadline_s(request),
    })
}

fn retry<C: Exchange>(exchange: C, attempt: u32, decision: &response_policy::Decision) -> Step<C> {
    Step::Wait {
        delay_ms: decision.retry_delay_ms,
        jitter_ms: decision.retry_jitter_ms,
        continuation: Continuation {
            exchange,
            attempt: attempt + 1,
        },
    }
}

/// A non-2xx answer: signed out, sent again, or surfaced with the backend's
/// code, the operator's words and the backend's own text beside them.
fn answered<C: Exchange>(
    exchange: C,
    plan: &Plan,
    attempt: u32,
    status: u16,
    retry_after: Option<String>,
    body: &Value,
    facts: &Facts,
) -> Step<C> {
    let message = backend_message(status, body);
    let code = backend_code(body);
    let decision = classify(
        attempt,
        plan.retries,
        facts,
        Observation {
            status: Some(status),
            error_code: code.as_deref(),
            timed_out: false,
            retry_after,
            message: Some(&message),
        },
    );
    match decision.decision {
        Outcome::SignedOut => Step::Refused(Refusal {
            status: Some(status),
            transient: decision.transient,
            retryable: decision.retryable,
            signed_out: true,
            ..Refusal::new(SIGNED_OUT, SIGNED_OUT_MESSAGE)
        }),
        Outcome::Retry => retry(exchange, attempt, &decision),
        Outcome::Surface => {
            let public = decision
                .public_message
                .map(str::to_owned)
                .unwrap_or_else(|| message.trim().to_owned());
            let report = if decision.report_to_sre {
                report(&exchange, &plan.request, status, &message)
            } else {
                None
            };
            Step::Refused(Refusal {
                status: Some(status),
                transient: decision.transient,
                retryable: decision.retryable,
                technical: (public != message).then_some(message),
                details: backend_details(body),
                report,
                ..Refusal::new(&code.unwrap_or_else(|| format!("HTTP_{status}")), public)
            })
        }
    }
}

fn transport_refusal(code: &str, message: String, decision: &response_policy::Decision) -> Refusal {
    let public = decision
        .public_message
        .map(str::to_owned)
        .unwrap_or_else(|| message.trim().to_owned());
    Refusal {
        transient: decision.transient,
        retryable: decision.retryable,
        outcome_unknown: true,
        technical: (public != message).then_some(message),
        ..Refusal::new(code, public)
    }
}

fn oversized(request: &Request) -> Refusal {
    Refusal::new(
        RESPONSE_TOO_LARGE,
        format!(
            "The answer exceeded its {} byte bound.",
            request.response_cap_bytes
        ),
    )
}

/// No answer arrived. A timeout is surfaced, never re-sent; a link cut by the
/// device's own offline mode is that refusal; any other failed link is
/// re-sent within the call's budget, then surfaced as offline or unreachable.
fn failed<C: Exchange>(
    exchange: C,
    plan: &Plan,
    attempt: u32,
    cause: Failure,
    message: Option<String>,
    facts: &Facts,
) -> Step<C> {
    let message = message
        .map(|text| text.chars().take(MESSAGE_MAX_CHARS).collect::<String>())
        .filter(|text| !text.trim().is_empty());
    match cause {
        Failure::Oversized => Step::Refused(oversized(&plan.request)),
        Failure::Timeout => {
            let message = format!(
                "Request timed out after {}s",
                plan.request.timeout_ms.div_ceil(1000)
            );
            let decision = classify(
                attempt,
                0,
                facts,
                Observation {
                    status: None,
                    error_code: Some(REQUEST_TIMEOUT),
                    timed_out: true,
                    retry_after: None,
                    message: Some(&message),
                },
            );
            Step::Refused(transport_refusal(REQUEST_TIMEOUT, message, &decision))
        }
        Failure::Network | Failure::Offline if facts.offline_mode => {
            Step::Refused(offline_refusal())
        }
        Failure::Network | Failure::Offline => {
            let again = classify(
                attempt,
                plan.retries,
                facts,
                Observation {
                    status: None,
                    error_code: None,
                    timed_out: false,
                    retry_after: None,
                    message: None,
                },
            );
            if again.decision == Outcome::Retry {
                return retry(exchange, attempt, &again);
            }
            let code = if cause == Failure::Offline {
                NETWORK_OFFLINE
            } else {
                NETWORK_ERROR
            };
            let message = message.unwrap_or_else(|| "Request failed".into());
            let decision = classify(
                attempt,
                0,
                facts,
                Observation {
                    status: None,
                    error_code: Some(code),
                    timed_out: false,
                    retry_after: None,
                    message: Some(&message),
                },
            );
            Step::Refused(transport_refusal(code, message, &decision))
        }
    }
}

#[cfg(test)]
#[path = "gateway_exchange_tests.rs"]
mod tests;
