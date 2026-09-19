//! Response policy — the one answer to "what does this HTTP outcome mean?".
//!
//! The browser, the desktop shell and `ds` all send a DS request through some
//! host transport, and each one then has to decide the same four things: is
//! this transient, should it be retried and how long from now, is it worth an
//! SRE report, and what does the operator get told. Those are decisions, not
//! presentation, so they live here and are answered identically everywhere.
//!
//! The host reports **facts** (a status, an error code, an attempt number, the
//! verbatim `Retry-After` header value, a bounded slice of the backend's text)
//! and receives **decisions**. No URL, token, header map or response body
//! crosses this seam, the reply never echoes the input back, and the module is
//! pure: `now` is an input and jitter is the host's to add.

use serde::{Deserialize, Serialize};

use crate::transfer::{backoff_ms, Cause};

pub const RESPONSE_POLICY_SCHEMA: &str = "ds.response-policy/v1";

/// Backend text is a fact to classify, never a payload to carry. Only this
/// many characters are read, and the reply is built from constants, so a long
/// or credential-bearing body cannot travel through this module.
pub const MESSAGE_MAX_CHARS: usize = 1024;

const MAX_INPUT_BYTES: usize = 16 * 1024;
const RETRY_AFTER_CAP_MS: u64 = 60_000;
const MIN_RETRY_DELAY_MS: u64 = 250;

const NETWORK_UNREACHABLE: &str = "Data Solutions API is not reachable right now. Retry after the service connection is restored.";
const BROWSER_OFFLINE: &str =
    "Data Solutions API is not reachable because this browser is offline. Reconnect and retry.";
const LOCAL_AUTH_EXPIRED: &str = "Local service authorization expired. Restart local services after refreshing credentials, then retry.";
const REPORT_UNAVAILABLE: &str = "Report actions are not available in this environment. Save without report actions, or retry after the report service is restored.";
const SETTINGS_UNREADABLE: &str =
    "Report actions could not read the project settings. Restart local services and try again.";

/// A signed-out response carries the backend's authentication complaint. The
/// session already ended, so surfacing it would report a failure the operator
/// did not cause; these keys are dropped from the body instead.
const SIGNED_OUT_REDACT_KEYS: &[&str] = &["error", "error_details", "error_message", "message"];

/// Transport-shaped failures the client already reflects in its own UI. They
/// are ambiguity about the link, not evidence about the service.
const CLIENT_TRANSPORT_ERROR_CODES: &[&str] = &[
    "NETWORK_ERROR",
    "NETWORK_OFFLINE",
    "REQUEST_CANCELLED",
    "REQUEST_TIMEOUT",
];

#[derive(Serialize, Debug, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// 401/403 arriving after the session ended: inert, never an error.
    SignedOut,
    /// Send it again after `retry_delay_ms`.
    Retry,
    /// Give the caller the result.
    Surface,
}

#[derive(Deserialize)]
struct Discriminator {
    kind: String,
}

/// Facts about one completed response, or one transport failure that produced
/// no response at all (`status: null`).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Classify {
    pub schema: String,
    pub kind: String,
    #[serde(default)]
    pub status: Option<u16>,
    /// Typed client code or backend `error_code`, whichever the host observed.
    #[serde(default)]
    pub error_code: Option<String>,
    #[serde(default)]
    pub timed_out: bool,
    /// Zero-based attempt already spent.
    #[serde(default)]
    pub attempt: u32,
    #[serde(default)]
    pub retries_allowed: u32,
    /// Verbatim `Retry-After` header value, if the response carried one.
    #[serde(default)]
    pub retry_after: Option<String>,
    /// Host clock in epoch milliseconds. The kernel keeps none of its own.
    #[serde(default)]
    pub now_ms: u64,
    /// Whether the host still holds a session — a store read, not a decision.
    #[serde(default)]
    pub has_session: bool,
    /// Whether intentional offline mode is on.
    #[serde(default)]
    pub offline_mode: bool,
    /// Bounded backend text. Classified, never echoed.
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Decision {
    pub schema: &'static str,
    pub decision: Outcome,
    pub transient: bool,
    pub retryable: bool,
    pub report_to_sre: bool,
    /// Deterministic. The host adds `random() * retry_jitter_ms` on top.
    pub retry_delay_ms: u64,
    /// Zero when the server named the delay: `Retry-After` is not a guess to
    /// spread out.
    pub retry_jitter_ms: u64,
    /// Product language replacing the backend's text, or null to keep it.
    pub public_message: Option<&'static str>,
    /// The refusal is this device's own offline mode; the host has one
    /// exported sentence for that state and uses it here.
    pub offline_refusal: bool,
    pub redact_keys: &'static [&'static str],
}

/// Facts about one request being reported to SRE.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Telemetry {
    pub schema: String,
    pub kind: String,
    /// The logical route as the caller named it.
    pub route: String,
    /// The resolved request path. A path, never an absolute URL.
    pub path: String,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub operation: Option<String>,
    #[serde(default)]
    pub job_action: Option<String>,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Classification {
    pub schema: &'static str,
    pub action: String,
    pub action_class: &'static str,
}

/// "Could a later attempt succeed?" is already decided once in this crate, by
/// the transfer machine's typed cause, over the same fact. So it is asked
/// there, not answered again here — a second table of statuses is exactly the
/// duplicate authority this module exists to remove.
fn transient_status(status: u16) -> bool {
    Cause::HttpStatus { status }.retryable()
}

/// A DIFFERENT, narrower question: "may this client send this exact request
/// again, right now?"
///
/// A resumable chunk is idempotent by protocol, so the transfer machine
/// re-sends on any 5xx. A gateway operation is not: a 500 may have applied its
/// effect before failing, and re-sending would duplicate it. Only the statuses
/// that mean the request never reached its handler (502/504), that the handler
/// asked for it later (408/425/429), or that the service is out (503) are
/// re-sent. The gap between the two answers is deliberate, not drift.
fn safe_to_resend(status: u16) -> bool {
    matches!(status, 408 | 425 | 429 | 502 | 503 | 504)
}

fn should_report_to_sre(status: Option<u16>, error_code: Option<&str>, timed_out: bool) -> bool {
    if timed_out {
        return false;
    }
    if let Some(code) = error_code {
        let code = code.trim().to_uppercase();
        if !code.is_empty() && CLIENT_TRANSPORT_ERROR_CODES.contains(&code.as_str()) {
            return false;
        }
    }
    !matches!(status, Some(status) if safe_to_resend(status))
}

/// Same curve as the transfer machine, one place: 1s doubling to a 30s cap.
/// Returned shy of the base so the host's bounded jitter lands around it.
fn retry_backoff(attempt: u32) -> (u64, u64) {
    let base = backoff_ms(attempt.saturating_add(1));
    let delay = base.saturating_sub(base / 10).max(MIN_RETRY_DELAY_MS);
    (delay, (base / 5).max(1))
}

fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_position = (month + 9) % 12;
    let day_of_year = (153 * month_position + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

fn month_from_name(name: &str) -> Option<i64> {
    const MONTHS: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let name = name.to_ascii_lowercase();
    MONTHS
        .iter()
        .position(|month| *month == name)
        .map(|index| index as i64 + 1)
}

fn epoch_ms(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> Option<i64> {
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }
    Some((days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60 + second) * 1_000)
}

fn hms(value: &str) -> Option<(i64, i64, i64)> {
    let mut parts = value.split(':');
    let hour = parts.next()?.parse::<i64>().ok()?;
    let minute = parts.next()?.parse::<i64>().ok()?;
    let second = parts.next()?;
    // Fractional seconds are legal in ISO-8601 and irrelevant at this
    // resolution; a retry is scheduled in whole milliseconds.
    let second = second.split('.').next()?.parse::<i64>().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((hour, minute, second))
}

/// IMF-fixdate — the one format RFC 9110 requires a server to generate:
/// `Sun, 06 Nov 1994 08:49:37 GMT`.
fn imf_fixdate_ms(raw: &str) -> Option<i64> {
    let parts: Vec<&str> = raw.split_whitespace().collect();
    if parts.len() != 6 || !parts[5].eq_ignore_ascii_case("GMT") {
        return None;
    }
    let day = parts[1].parse::<i64>().ok()?;
    let month = month_from_name(parts[2])?;
    let year = parts[3].parse::<i64>().ok()?;
    let (hour, minute, second) = hms(parts[4])?;
    epoch_ms(year, month, day, hour, minute, second)
}

/// The fractional-second field, in whole milliseconds. `Date.parse` keeps it
/// and `design_status_row` ties on it, so the one parser does too.
fn fraction_millis(time: &str) -> i64 {
    let Some((_, fraction)) = time.rsplit(':').next().and_then(|s| s.split_once('.')) else {
        return 0;
    };
    let digits: String = fraction
        .chars()
        .take_while(char::is_ascii_digit)
        .chain("000".chars())
        .take(3)
        .collect();
    digits.parse::<i64>().unwrap_or(0)
}

/// ISO-8601 in UTC or with a numeric offset — not an HTTP-date, but a common
/// enough server mistake that refusing it would silently discard a real delay.
/// Also the one ISO reader the crate has: `design_status_row` folds phase
/// timestamps with it rather than opening a second copy.
pub(crate) fn iso8601_ms(raw: &str) -> Option<i64> {
    let Some((date, rest)) = raw.split_once(['T', 't']) else {
        // A bare date is UTC midnight, exactly as `Date.parse` reads it.
        let mut date_parts = raw.split('-');
        let year = date_parts.next()?.parse::<i64>().ok()?;
        let month = date_parts.next()?.parse::<i64>().ok()?;
        let day = date_parts.next()?.parse::<i64>().ok()?;
        if date_parts.next().is_some() {
            return None;
        }
        return epoch_ms(year, month, day, 0, 0, 0);
    };
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i64>().ok()?;
    let month = date_parts.next()?.parse::<i64>().ok()?;
    let day = date_parts.next()?.parse::<i64>().ok()?;
    if date_parts.next().is_some() {
        return None;
    }
    let (time, offset_minutes) = if let Some(time) = rest.strip_suffix(['Z', 'z']) {
        (time, 0)
    } else {
        let (time, offset) = rest.split_at(rest.rfind(['+', '-'])?);
        let sign = if offset.starts_with('-') { -1 } else { 1 };
        let offset = &offset[1..];
        let (hours, minutes) = match offset.split_once(':') {
            Some((hours, minutes)) => (hours, minutes),
            None if offset.len() == 4 => offset.split_at(2),
            None => (offset, "0"),
        };
        (
            time,
            sign * (hours.parse::<i64>().ok()? * 60 + minutes.parse::<i64>().ok()?),
        )
    };
    let (hour, minute, second) = hms(time)?;
    Some(
        epoch_ms(year, month, day, hour, minute, second)? + fraction_millis(time)
            - offset_minutes * 60_000,
    )
}

/// `Retry-After` is delta-seconds or an HTTP-date. Anything else is not a
/// delay the server named, so the caller's own backoff stands.
fn retry_after_ms(raw: &str, now_ms: u64) -> Option<u64> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    if let Ok(seconds) = raw.parse::<f64>() {
        if seconds.is_finite() && seconds >= 0.0 {
            let millis = seconds * 1_000.0;
            return Some(if millis >= RETRY_AFTER_CAP_MS as f64 {
                RETRY_AFTER_CAP_MS
            } else {
                millis as u64
            });
        }
    }
    let at = imf_fixdate_ms(raw).or_else(|| iso8601_ms(raw))?;
    let delta = at.saturating_sub(now_ms as i64).max(0);
    Some((delta as u64).min(RETRY_AFTER_CAP_MS))
}

/// The rewrite table. Every entry exists because an operator hit the raw text
/// and could not act on it; the replacement names what they can do instead.
fn public_message(message: &str) -> Option<&'static str> {
    let bounded: String = message.chars().take(MESSAGE_MAX_CHARS).collect();
    let message = bounded.trim();
    if message.is_empty() {
        return None;
    }
    let lower = message.to_lowercase();
    if lower == "failed to fetch"
        || lower == "fetch failed"
        || lower == "load failed"
        || lower.contains("networkerror when attempting to fetch resource")
        || lower.contains("network request failed")
    {
        return Some(NETWORK_UNREACHABLE);
    }
    if lower.contains("invalid_rapt")
        || lower.contains("invalid_grant")
        || lower.contains("per-rpc creds failed")
        || (lower.contains("failed to verify project lifecycle state")
            && lower.contains("unauthenticated"))
    {
        return Some(LOCAL_AUTH_EXPIRED);
    }
    if lower.contains("ds_network_reporter_url")
        || lower.contains("inline_save_actions")
        || lower.contains("switch to standard to run the python path")
        || (lower.contains("fast lane") && lower.contains("not implemented for this payload"))
        || lower.contains("downstream http://localhost")
        || lower.contains("downstream http://127.0.0.1")
    {
        return Some(REPORT_UNAVAILABLE);
    }
    if lower.contains("firestore variables read")
        || lower.contains("project variables")
        || lower.contains("http status error: 400 bad request")
    {
        return Some(SETTINGS_UNREADABLE);
    }
    None
}

pub fn classify(request: Classify) -> Result<Decision, String> {
    if request.schema != RESPONSE_POLICY_SCHEMA {
        return Err("Unsupported response policy schema.".into());
    }
    let status = request.status;
    let error_code = request.error_code.as_deref();
    let offline_refusal = request.offline_mode
        && error_code.map(|code| code.trim().to_uppercase()) == Some("NETWORK_OFFLINE".into());

    let decision = if matches!(status, Some(401) | Some(403)) && !request.has_session {
        Outcome::SignedOut
    } else if request.attempt < request.retries_allowed
        && status.map(safe_to_resend).unwrap_or(true)
    {
        Outcome::Retry
    } else {
        Outcome::Surface
    };

    let (backoff_delay, jitter) = retry_backoff(request.attempt);
    let (retry_delay_ms, retry_jitter_ms) = request
        .retry_after
        .as_deref()
        .and_then(|raw| retry_after_ms(raw, request.now_ms))
        .map(|named| (named, 0))
        .unwrap_or((backoff_delay, jitter));

    let public_message = if offline_refusal {
        None
    } else if error_code.map(|code| code.trim().to_uppercase()) == Some("NETWORK_OFFLINE".into()) {
        Some(BROWSER_OFFLINE)
    } else {
        request.message.as_deref().and_then(public_message)
    };

    Ok(Decision {
        schema: RESPONSE_POLICY_SCHEMA,
        decision,
        transient: status.map(transient_status).unwrap_or(true),
        retryable: status.map(transient_status).unwrap_or(true),
        report_to_sre: should_report_to_sre(status, error_code, request.timed_out),
        retry_delay_ms,
        retry_jitter_ms,
        public_message,
        offline_refusal,
        redact_keys: if decision == Outcome::SignedOut {
            SIGNED_OUT_REDACT_KEYS
        } else {
            &[]
        },
    })
}

fn normalize_event_key(value: &str) -> String {
    let normalized = value.trim().to_lowercase().replace(['-', ' '], "_");
    if normalized.is_empty() {
        "unknown".into()
    } else {
        normalized
    }
}

fn event_action(request: &Telemetry) -> String {
    for candidate in [&request.action, &request.operation, &request.job_action]
        .into_iter()
        .flatten()
    {
        if !candidate.trim().is_empty() {
            return normalize_event_key(candidate);
        }
    }
    let head = request.route.split('?').next().unwrap_or(&request.route);
    let trimmed = head.trim_end_matches('/');
    let clean = if trimmed.is_empty() {
        request.route.as_str()
    } else {
        trimmed
    };
    normalize_event_key(
        clean
            .split('/')
            .rfind(|segment| !segment.is_empty())
            .unwrap_or("unknown"),
    )
}

/// Three classes, because that is what the event store aggregates on: work the
/// operator waits for a document from, work that runs asynchronously, and the
/// orchestration chatter around both.
fn event_action_class(action: &str, path: &str) -> &'static str {
    let value = normalize_event_key(action);
    let path = path.to_lowercase();
    if value.contains("report")
        || value.contains("export")
        || value.contains("download")
        || path.contains("/report")
        || path.contains("/interpret")
    {
        return "report";
    }
    if value.contains("process")
        || value.contains("tile")
        || value.contains("convert")
        || value.contains("calculate")
        || path.contains("/process")
        || path.contains("/tile")
        || path.contains("/solar/calculate")
    {
        return "async";
    }
    "orchestration"
}

pub fn telemetry(request: Telemetry) -> Result<Classification, String> {
    if request.schema != RESPONSE_POLICY_SCHEMA {
        return Err("Unsupported response policy schema.".into());
    }
    // A path is safe to classify; an absolute URL carries an origin and may
    // carry a credential, so it is refused rather than quietly accepted.
    if request.path.contains("://") || request.route.contains("://") {
        return Err("Response policy takes a request path, never an absolute URL.".into());
    }
    let action = event_action(&request);
    let action_class = event_action_class(&action, &request.path);
    Ok(Classification {
        schema: RESPONSE_POLICY_SCHEMA,
        action,
        action_class,
    })
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("Response policy request exceeds its bound.".into());
    }
    let discriminator: Discriminator = serde_json::from_slice(input)
        .map_err(|e| format!("Invalid response policy request: {e}"))?;
    match discriminator.kind.as_str() {
        "classify" => {
            let request = serde_json::from_slice(input)
                .map_err(|e| format!("Invalid response policy request: {e}"))?;
            serde_json::to_string(&classify(request)?).map_err(|e| e.to_string())
        }
        "telemetry" => {
            let request = serde_json::from_slice(input)
                .map_err(|e| format!("Invalid response policy request: {e}"))?;
            serde_json::to_string(&telemetry(request)?).map_err(|e| e.to_string())
        }
        _ => Err("Unsupported response policy request kind.".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    fn classify_json(overrides: Value) -> Value {
        let mut request = json!({
            "schema": RESPONSE_POLICY_SCHEMA,
            "kind": "classify",
            "has_session": true,
        });
        let map = request.as_object_mut().unwrap();
        for (key, value) in overrides.as_object().unwrap() {
            map.insert(key.clone(), value.clone());
        }
        serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).unwrap()).unwrap()
    }

    fn telemetry_json(overrides: Value) -> Value {
        let mut request = json!({
            "schema": RESPONSE_POLICY_SCHEMA,
            "kind": "telemetry",
            "route": "/api/v1/data",
            "path": "/api/v1/data",
        });
        let map = request.as_object_mut().unwrap();
        for (key, value) in overrides.as_object().unwrap() {
            map.insert(key.clone(), value.clone());
        }
        serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).unwrap()).unwrap()
    }

    #[test]
    fn transient_and_retryable_statuses_keep_their_exact_membership() {
        for status in [500, 502, 503, 504, 408, 425, 429, 599] {
            assert!(transient_status(status), "{status} must be transient");
        }
        for status in [200, 400, 401, 403, 404, 409, 413, 422, 499] {
            assert!(!transient_status(status), "{status} must not be transient");
        }
        for status in [408, 425, 429, 502, 503, 504] {
            assert!(safe_to_resend(status), "{status} must be re-sent");
        }
        // A 500 is transient but NOT re-sent: it may already have applied.
        for status in [400, 401, 403, 409, 500, 501, 505] {
            assert!(!safe_to_resend(status), "{status} must not be re-sent");
        }
        assert!(transient_status(500) && !safe_to_resend(500));
    }

    #[test]
    fn transport_ambiguity_and_busy_gateways_are_not_sre_reports() {
        for code in [
            "NETWORK_ERROR",
            "NETWORK_OFFLINE",
            "REQUEST_CANCELLED",
            "REQUEST_TIMEOUT",
            " request_timeout ",
        ] {
            assert!(!should_report_to_sre(None, Some(code), false), "{code}");
        }
        assert!(!should_report_to_sre(None, None, true));
        for status in [408, 425, 429, 502, 503, 504] {
            assert!(!should_report_to_sre(Some(status), None, false), "{status}");
        }
        for status in [400, 403, 500] {
            assert!(should_report_to_sre(Some(status), None, false), "{status}");
        }
        assert!(should_report_to_sre(None, Some(""), false));
        assert!(should_report_to_sre(None, Some("STALE_SETTINGS"), false));
    }

    #[test]
    fn backoff_is_deterministic_and_leaves_room_for_host_jitter() {
        // Same curve the transfer machine already runs: 1s doubling to 30s.
        assert_eq!(retry_backoff(0), (900, 200));
        assert_eq!(retry_backoff(1), (1_800, 400));
        assert_eq!(retry_backoff(2), (3_600, 800));
        assert_eq!(retry_backoff(4), (14_400, 3_200));
        // Capped at 30s from attempt 5 on, and it stays there.
        assert_eq!(retry_backoff(5), (27_000, 6_000));
        assert_eq!(retry_backoff(9), (27_000, 6_000));
        assert_eq!(retry_backoff(u32::MAX), (27_000, 6_000));
    }

    #[test]
    fn retry_after_reads_delta_seconds_and_http_dates_within_one_minute() {
        assert_eq!(retry_after_ms("5", 0), Some(5_000));
        assert_eq!(retry_after_ms(" 5 ", 0), Some(5_000));
        assert_eq!(retry_after_ms("0", 0), Some(0));
        assert_eq!(retry_after_ms("5.5", 0), Some(5_500));
        assert_eq!(retry_after_ms("120", 0), Some(60_000));
        assert_eq!(retry_after_ms("-1", 0), None);
        assert_eq!(retry_after_ms("", 0), None);
        assert_eq!(retry_after_ms("soon", 0), None);

        // 1994-11-06T08:49:37Z is 784111777 seconds after the epoch.
        let at = 784_111_777_000;
        assert_eq!(
            retry_after_ms("Sun, 06 Nov 1994 08:49:37 GMT", at - 10_000),
            Some(10_000)
        );
        assert_eq!(
            retry_after_ms("1994-11-06T08:49:37Z", at - 10_000),
            Some(10_000)
        );
        assert_eq!(
            retry_after_ms("1994-11-06T09:49:37+01:00", at - 10_000),
            Some(10_000)
        );
        // A past date is a zero delay, never a negative one, and a far future
        // date is still capped at the same minute.
        assert_eq!(
            retry_after_ms("Sun, 06 Nov 1994 08:49:37 GMT", at + 5),
            Some(0)
        );
        assert_eq!(
            retry_after_ms("Sun, 06 Nov 1994 08:49:37 GMT", 0),
            Some(60_000)
        );
        assert_eq!(retry_after_ms("Sun, 06 Nov 1994 08:49:37 UTC", at), None);
    }

    #[test]
    fn a_named_delay_replaces_the_backoff_and_is_not_jittered() {
        let decision = classify_json(json!({"status": 429, "attempt": 0, "retries_allowed": 2,
            "retry_after": "7"}));
        assert_eq!(decision["decision"], "retry");
        assert_eq!(decision["retry_delay_ms"], 7_000);
        assert_eq!(decision["retry_jitter_ms"], 0);

        let decision = classify_json(json!({"status": 429, "attempt": 0, "retries_allowed": 2}));
        assert_eq!(decision["retry_delay_ms"], 900);
        assert_eq!(decision["retry_jitter_ms"], 200);

        // An unparseable header leaves the caller's own backoff standing.
        let decision = classify_json(json!({"status": 503, "attempt": 1, "retries_allowed": 2,
            "retry_after": "whenever"}));
        assert_eq!(decision["retry_delay_ms"], 1_800);
        assert_eq!(decision["retry_jitter_ms"], 400);
    }

    #[test]
    fn retry_is_bounded_by_the_attempt_budget_and_the_status_class() {
        for (status, attempt, retries, expected) in [
            (Some(503), 0, 2, "retry"),
            (Some(503), 2, 2, "surface"),
            (Some(500), 0, 2, "surface"),
            (Some(400), 0, 2, "surface"),
            (None, 0, 2, "retry"),
            (None, 0, 0, "surface"),
        ] {
            let decision = classify_json(
                json!({"status": status, "attempt": attempt, "retries_allowed": retries}),
            );
            assert_eq!(decision["decision"], expected, "{status:?}/{attempt}");
        }
    }

    #[test]
    fn a_late_auth_failure_is_signed_out_only_once_the_session_has_gone() {
        for status in [401, 403] {
            let decision = classify_json(json!({"status": status, "has_session": false}));
            assert_eq!(decision["decision"], "signed_out");
            assert_eq!(decision["transient"], false);
            assert_eq!(decision["retryable"], false);
            assert_eq!(
                decision["redact_keys"],
                json!(["error", "error_details", "error_message", "message"])
            );
            assert_eq!(
                classify_json(json!({"status": status}))["decision"],
                "surface"
            );
        }
        // A signed-out session does not make every status inert.
        let decision = classify_json(json!({"status": 500, "has_session": false}));
        assert_eq!(decision["decision"], "surface");
        assert_eq!(decision["redact_keys"], json!([]));
    }

    #[test]
    fn every_operator_facing_rewrite_is_pinned() {
        for raw in [
            "Failed to fetch",
            "fetch failed",
            "  Load failed  ",
            "NetworkError when attempting to fetch resource.",
            "TypeError: Network request failed",
        ] {
            assert_eq!(public_message(raw), Some(NETWORK_UNREACHABLE), "{raw}");
        }
        for raw in [
            "reauth related error (invalid_rapt)",
            "auth: \"invalid_grant\"",
            "transport: per-RPC creds failed due to error",
            "Failed to verify project lifecycle state: rpc error: code = Unauthenticated",
        ] {
            assert_eq!(public_message(raw), Some(LOCAL_AUTH_EXPIRED), "{raw}");
        }
        // Both halves are required; one alone is not this failure.
        assert_eq!(
            public_message("Failed to verify project lifecycle state"),
            None
        );
        for raw in [
            "DS_NETWORK_REPORTER_URL is not configured",
            "inline_save_actions unavailable",
            "Switch to Standard to run the Python path.",
            "Fast lane /report action \"download_transfo\" is not implemented for this payload.",
            "downstream http://localhost:8090 refused",
            "downstream http://127.0.0.1:8090 refused",
        ] {
            assert_eq!(public_message(raw), Some(REPORT_UNAVAILABLE), "{raw}");
        }
        assert_eq!(public_message("Fast lane is busy"), None);
        for raw in [
            "firestore variables read failed",
            "project variables missing",
            "HTTP status error: 400 Bad Request",
        ] {
            assert_eq!(public_message(raw), Some(SETTINGS_UNREADABLE), "{raw}");
        }
        // Anything else is the backend's own words, kept.
        assert_eq!(
            public_message("settings were updated by another writer"),
            None
        );
        assert_eq!(public_message("   "), None);
    }

    #[test]
    fn the_first_matching_rewrite_wins_in_declaration_order() {
        // Text that satisfies rules 2 and 3 resolves to 2, as the table reads.
        assert_eq!(
            public_message("invalid_grant while reading project variables"),
            Some(LOCAL_AUTH_EXPIRED)
        );
    }

    #[test]
    fn the_backend_message_is_bounded_and_never_echoed_back() {
        assert_eq!(MESSAGE_MAX_CHARS, 1_024);
        // Beyond the bound the text is not read, so it cannot match and cannot
        // travel: the reply is built from constants only.
        let far = format!("{}invalid_rapt", "x".repeat(MESSAGE_MAX_CHARS));
        assert_eq!(public_message(&far), None);
        let near = format!("{}invalid_rapt", "x".repeat(MESSAGE_MAX_CHARS - 20));
        assert_eq!(public_message(&near), Some(LOCAL_AUTH_EXPIRED));

        let secret = "Bearer ya29.a0AfH6SMB-secret-token-value";
        let reply = evaluate(
            &serde_json::to_vec(
                &json!({"schema": RESPONSE_POLICY_SCHEMA, "kind": "classify",
                "status": 400, "message": secret}),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(!reply.contains("ya29"), "{reply}");
        assert!(reply.contains("\"public_message\":null"), "{reply}");

        assert!(evaluate(&vec![b' '; MAX_INPUT_BYTES + 1]).is_err());
    }

    #[test]
    fn an_offline_refusal_is_this_devices_state_and_a_browser_offline_is_not() {
        let decision = classify_json(
            json!({"error_code": "NETWORK_OFFLINE", "offline_mode": true,
            "message": "Failed to fetch"}),
        );
        assert_eq!(decision["offline_refusal"], true);
        assert_eq!(decision["public_message"], Value::Null);

        let decision = classify_json(json!({"error_code": "NETWORK_OFFLINE",
            "message": "Failed to fetch"}));
        assert_eq!(decision["offline_refusal"], false);
        assert_eq!(decision["public_message"], BROWSER_OFFLINE);

        // Offline mode alone does not make an ordinary failure a refusal.
        let decision = classify_json(json!({"error_code": "NETWORK_ERROR", "offline_mode": true,
            "message": "Failed to fetch"}));
        assert_eq!(decision["offline_refusal"], false);
        assert_eq!(decision["public_message"], NETWORK_UNREACHABLE);
    }

    #[test]
    fn the_action_is_the_payload_verb_and_otherwise_the_route_tail() {
        assert_eq!(
            telemetry_json(json!({"action": "Save-Transformer Data"}))["action"],
            "save_transformer_data"
        );
        assert_eq!(
            telemetry_json(json!({"action": "  ", "operation": "publish"}))["action"],
            "publish"
        );
        assert_eq!(
            telemetry_json(json!({"job_action": "tile_generate"}))["action"],
            "tile_generate"
        );
        assert_eq!(
            telemetry_json(json!({"route": "/api/v1/entries/mutate/"}))["action"],
            "mutate"
        );
        assert_eq!(
            telemetry_json(json!({"route": "/api/v1/layers?force=true"}))["action"],
            "layers"
        );
        assert_eq!(telemetry_json(json!({"route": "///"}))["action"], "unknown");
        assert_eq!(telemetry_json(json!({"route": ""}))["action"], "unknown");
    }

    #[test]
    fn the_action_class_reads_the_verb_first_and_the_path_second() {
        for (action, path, expected) in [
            ("export_reports", "/api/v1/data", "report"),
            ("download_transfo", "/api/v1/data", "report"),
            ("refresh", "/report", "report"),
            ("interpret", "/api/v1/interpret", "report"),
            ("process_transformer", "/api/v1/data", "async"),
            ("convert", "/api/v1/data", "async"),
            ("calculate", "/api/v1/data", "async"),
            ("generate", "/api/v1/tiles/tile", "async"),
            ("run", "/api/v1/solar/calculate", "async"),
            ("save", "/api/v1/data", "orchestration"),
            // A report verb outranks an async path.
            ("export", "/api/v1/process", "report"),
        ] {
            let classification =
                telemetry_json(json!({"action": action, "path": path, "route": path}));
            assert_eq!(classification["action_class"], expected, "{action} {path}");
        }
    }

    #[test]
    fn the_seam_refuses_an_absolute_url_a_wrong_schema_and_an_unknown_field() {
        let absolute = json!({"schema": RESPONSE_POLICY_SCHEMA, "kind": "telemetry",
            "route": "/api/v1/data", "path": "https://brain.test/api/v1/data"});
        assert!(evaluate(&serde_json::to_vec(&absolute).unwrap())
            .unwrap_err()
            .contains("never an absolute URL"));

        let wrong_schema = json!({"schema": "ds.other/v1", "kind": "classify"});
        assert!(evaluate(&serde_json::to_vec(&wrong_schema).unwrap()).is_err());

        let unknown = json!({"schema": RESPONSE_POLICY_SCHEMA, "kind": "classify",
            "authorization": "Bearer secret"});
        assert!(evaluate(&serde_json::to_vec(&unknown).unwrap()).is_err());

        let unknown_kind = json!({"schema": RESPONSE_POLICY_SCHEMA, "kind": "http"});
        assert!(evaluate(&serde_json::to_vec(&unknown_kind).unwrap()).is_err());
        assert!(evaluate(b"not json").is_err());
    }
}

/// Parity with the TypeScript this module replaced.
///
/// `tests/fixtures/response-policy-parity.json` was produced by RUNNING
/// `ds-web/src/lib/api/client.ts`'s decision helpers — `transientStatus`,
/// `retryableTransportStatus`, `shouldReportClientFailureToSre`,
/// `retryDelayMs`, `retryAfterMs`, `publicErrorMessage`, `eventAction` and
/// `eventActionClass` — over every input they handled. Nothing in it was
/// transcribed by hand. The deletion of those helpers is only safe because
/// this test reproduces their output, so the corpus outlives them.
///
/// Where this module deliberately does NOT reproduce the old answer, the
/// corpus keeps the old value and the divergence is named below with its
/// reason. A silent difference would fail here.
#[cfg(test)]
mod parity {
    use super::*;
    use serde_json::{json, Value};

    fn corpus() -> Value {
        serde_json::from_str(include_str!(
            "../tests/fixtures/response-policy-parity.json"
        ))
        .unwrap()
    }

    fn decide(request: Value) -> Value {
        serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).unwrap()).unwrap()
    }

    fn classify_request(mut overrides: Value, now_ms: u64) -> Value {
        let map = overrides.as_object_mut().unwrap();
        map.insert("schema".into(), json!(RESPONSE_POLICY_SCHEMA));
        map.insert("kind".into(), json!("classify"));
        map.insert("now_ms".into(), json!(now_ms));
        map.entry("has_session").or_insert(json!(true));
        overrides
    }

    #[test]
    fn every_operator_message_rewrite_matches_the_deleted_typescript() {
        // The TypeScript read an unbounded backend message and returned either
        // a product sentence or the operator's own text back. This module
        // reads MESSAGE_MAX_CHARS and returns ONLY the product sentence, so
        // the exact parity statement is: the same rewrite fires, or neither
        // does — with the host keeping the untruncated text either way.
        let mut bound_changed_the_answer = 0;
        for row in corpus()["messages"].as_array().unwrap() {
            let input = row["input"].as_str().unwrap();
            let bounded: String = input.chars().take(MESSAGE_MAX_CHARS).collect();
            let typescript = row["public_bounded"].as_str().unwrap();
            let expected = (typescript != bounded.trim()).then_some(typescript);
            let decision = decide(classify_request(json!({"message": input}), 0));
            assert_eq!(
                decision["public_message"].as_str(),
                expected,
                "message {input:?}"
            );

            // And end to end, through the host's `?? raw.trim()`: what the
            // operator reads is byte-for-byte what they read before.
            let operator_reads = decision["public_message"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| input.trim().to_string());
            if row["public"] == row["public_bounded"] {
                assert_eq!(operator_reads, row["public"].as_str().unwrap(), "{input:?}");
            } else {
                bound_changed_the_answer += 1;
                assert!(
                    input.chars().count() > MESSAGE_MAX_CHARS,
                    "the bound may only change the answer for oversized text",
                );
            }
        }
        // One synthetic row: 1,212 characters with its marker past the bound.
        // Every message an operator actually hit is well inside it, so the
        // bound costs nothing real and buys a seam a response body cannot
        // carry a credential through.
        assert_eq!(bound_changed_the_answer, 1);
    }

    #[test]
    fn every_status_classification_matches_the_deleted_typescript() {
        for row in corpus()["statuses"].as_array().unwrap() {
            let status = row["status"].as_u64().unwrap() as u16;
            let decision = decide(classify_request(
                json!({"status": status, "attempt": 0, "retries_allowed": 1}),
                0,
            ));
            assert_eq!(
                decision["transient"], row["transient"],
                "transient {status}"
            );
            assert_eq!(
                decision["retryable"], row["transient"],
                "retryable {status}"
            );
            assert_eq!(
                decision["decision"] == "retry",
                row["resend"].as_bool().unwrap(),
                "resend {status}",
            );
        }
    }

    #[test]
    fn every_sre_reporting_answer_matches_the_deleted_typescript() {
        for row in corpus()["sre"].as_array().unwrap() {
            let mut request = json!({"timed_out": row["timed_out"]});
            if let Some(status) = row["status"].as_u64() {
                request["status"] = json!(status);
            }
            if let Some(code) = row["error_code"].as_str() {
                request["error_code"] = json!(code);
            }
            let decision = decide(classify_request(request, 0));
            assert_eq!(decision["report_to_sre"], row["report"], "sre {row}");
        }
    }

    #[test]
    fn every_backoff_envelope_matches_the_deleted_typescript() {
        for row in corpus()["backoff"].as_array().unwrap() {
            let attempt = row["attempt"].as_u64().unwrap();
            let decision = decide(classify_request(
                json!({"attempt": attempt, "retries_allowed": attempt + 1}),
                0,
            ));
            assert_eq!(decision["decision"], "retry", "attempt {attempt}");
            let delay = decision["retry_delay_ms"].as_u64().unwrap();
            let jitter = decision["retry_jitter_ms"].as_u64().unwrap();
            // TypeScript drew its own jitter inside the delay; the kernel is
            // deterministic and hands the host the same envelope to draw from.
            assert_eq!(delay, row["min_ms"].as_u64().unwrap(), "min {attempt}");
            assert_eq!(
                delay + jitter - 1,
                row["max_ms"].as_u64().unwrap(),
                "max {attempt}"
            );
        }
    }

    #[test]
    fn every_retry_after_header_matches_the_deleted_typescript_or_a_named_divergence() {
        // `Retry-After: -1` is malformed under RFC 9110 (delta-seconds is a
        // non-negative integer). `Date.parse('-1')` nonetheless returned a
        // future instant in V8, so the browser waited a full minute on a
        // header that named nothing. That accident is not preserved: a
        // malformed header leaves the client's own backoff standing.
        let divergent = ["-1"];
        let corpus = corpus();
        let now = corpus["now_ms"].as_u64().unwrap();
        let backoff = retry_backoff(0);
        for row in corpus["retry_after"].as_array().unwrap() {
            let header = row["header"].as_str();
            let mut request = json!({"status": 503, "attempt": 0, "retries_allowed": 1});
            if let Some(header) = header {
                request["retry_after"] = json!(header);
            }
            let decision = decide(classify_request(request, now));
            let delay = decision["retry_delay_ms"].as_u64().unwrap();
            let jitter = decision["retry_jitter_ms"].as_u64().unwrap();
            let named = header.is_some_and(|header| divergent.contains(&header));
            match row["delay_ms"].as_u64() {
                Some(expected) if !named => {
                    assert_eq!(delay, expected, "retry-after {header:?}");
                    assert_eq!(jitter, 0, "a named delay is never jittered: {header:?}");
                }
                _ => {
                    // No usable header: the client's own backoff stands.
                    assert_eq!((delay, jitter), backoff, "fallback for {header:?}");
                }
            }
        }
    }

    #[test]
    fn every_telemetry_classification_matches_the_deleted_typescript() {
        for row in corpus()["telemetry"].as_array().unwrap() {
            let mut request = json!({
                "schema": RESPONSE_POLICY_SCHEMA, "kind": "telemetry",
                "route": row["route"], "path": row["path"],
            });
            for key in ["action", "operation", "job_action"] {
                if let Some(value) = row[key].as_str() {
                    request[key] = json!(value);
                }
            }
            let classification = decide(request);
            assert_eq!(classification["action"], row["expected_action"], "{row}");
            assert_eq!(
                classification["action_class"], row["expected_class"],
                "{row}"
            );
        }
    }

    #[test]
    fn the_corpus_names_the_typescript_it_was_generated_from() {
        let corpus = corpus();
        assert!(corpus["generated_from"]
            .as_str()
            .unwrap()
            .starts_with("ds-web src/lib/api/client.ts @"));
        // A shrinking corpus is how a parity proof quietly stops proving.
        assert!(corpus["messages"].as_array().unwrap().len() >= 48);
        assert!(corpus["sre"].as_array().unwrap().len() >= 264);
        assert!(corpus["statuses"].as_array().unwrap().len() >= 32);
        assert!(corpus["telemetry"].as_array().unwrap().len() >= 19);
    }
}
