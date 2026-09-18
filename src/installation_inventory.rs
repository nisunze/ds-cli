//! What the installation inventory MEANS, decided once.
//!
//! The Governance → Installations page used to decide all of this in the
//! browser: which rows were visible, in what order, what `active / active`
//! stood for, what "last seen" was worth, and what an operator should do about
//! any of it. A decision reachable only from a browser is a decision the Server
//! cannot answer and `ds` cannot state, so it moves here.
//!
//! Three rules shape everything below.
//!
//! **Nothing here is English.** Every human-facing value is a stable token —
//! `host_kind`, `bucket`, `staleness`, `lease`, `remedy` — that a caller maps
//! to its own words. A terminal, a French browser and an English browser then
//! disagree about wording and never about meaning.
//!
//! **Silence is never an uninstallation.** A row that stopped reporting is
//! reported as silent, with how long, and nothing more. Retirement is a
//! recorded transition an operator or a machine makes explicitly; this module
//! reads it, and never infers it.
//!
//! **`last_seen` has exactly one source.** ds-brain stamps it inside the
//! license-refresh transaction and nowhere else, so this module labels it
//! `licence_refresh` rather than leaving a reader to guess whether a timestamp
//! means "it phoned home" or "somebody opened the app". The thresholds that
//! turn an age into a judgement are the product's own lease constants, not new
//! numbers invented for a table.

use serde_json::{json, Value};

use crate::installation::{evaluate, DesktopAccess, CHECK_IN_SECONDS, OFFLINE_LEASE_SECONDS};

/// The schema both directions of this facade carry.
pub const SCHEMA: &str = "ds.installation-inventory/v1";
/// The largest request this facade reads. An inventory page is a few hundred
/// small records; anything near this is a caller that lost its bound.
pub const MAX_REQUEST_BYTES: usize = 1024 * 1024;
/// The most installation records one request may carry.
pub const MAX_ROWS: usize = 5_000;
/// The most rows one projection returns, whatever the caller asks for.
pub const MAX_LIMIT: usize = 500;

fn bad(message: &str) -> String {
    format!("invalid_inventory_request: {message}")
}

fn text(row: &Value, key: &str) -> String {
    row[key].as_str().unwrap_or_default().trim().to_owned()
}

fn optional(row: &Value, key: &str) -> Option<String> {
    let value = text(row, key);
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

/// RFC3339 to epoch milliseconds, without a date library.
///
/// ds-brain writes every timestamp as UTC RFC3339, and the browser used to
/// hand these straight to `new Date()`. Parsing them here is what lets the
/// projection state an age rather than a string, and lets a terminal say the
/// same thing a browser does.
fn epoch_ms(value: &str) -> Option<i64> {
    let value = value.trim();
    if value.len() < 19 {
        return None;
    }
    let bytes = value.as_bytes();
    let part = |from: usize, to: usize| -> Option<i64> { value.get(from..to)?.parse().ok() };
    if bytes[4] != b'-' || bytes[7] != b'-' || (bytes[10] != b'T' && bytes[10] != b' ') {
        return None;
    }
    let (year, month, day) = (part(0, 4)?, part(5, 7)?, part(8, 10)?);
    let (hour, minute, second) = (part(11, 13)?, part(14, 16)?, part(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    // An offset is read, never ignored. Firestore hands these back in UTC, but
    // a row that comes straight out of a write on a host that is not on UTC
    // carries `+02:00`, and reading that as UTC would place "last seen" two
    // hours in the future — the one direction an inventory must not err in.
    let offset_seconds = match value[19..].find(['+', '-']) {
        Some(at) => {
            let tail = &value[19 + at..];
            let sign = if tail.starts_with('-') { -1 } else { 1 };
            let (offset_hour, offset_minute) = match tail[1..].split_once(':') {
                Some((hours, minutes)) => (hours, minutes),
                None if tail.len() == 5 => (&tail[1..3], &tail[3..5]),
                None => return None,
            };
            let offset_hour: i64 = offset_hour.parse().ok()?;
            let offset_minute: i64 = offset_minute.parse().ok()?;
            if offset_hour > 23 || offset_minute > 59 {
                return None;
            }
            sign * (offset_hour * 3_600 + offset_minute * 60)
        }
        None => 0,
    };
    // Days from the civil calendar, Howard Hinnant's algorithm. Exact for the
    // whole proleptic Gregorian range and free of any leap-second fiction.
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let year_of_era = y - era * 400;
    let day_of_year = (153 * (if month > 2 { month - 3 } else { month + 9 }) + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    let days = era * 146_097 + day_of_era - 719_468;
    Some((days * 86_400 + hour * 3_600 + minute * 60 + second.min(59) - offset_seconds) * 1_000)
}

/// Compare two dotted numeric versions. Anything unparsable sorts as zero,
/// which is the safe direction: an unreadable version is never treated as new
/// enough to satisfy a minimum.
fn version_at_least(candidate: &str, minimum: &str) -> bool {
    let parts = |value: &str| -> [u64; 3] {
        let mut out = [0u64; 3];
        for (slot, piece) in out.iter_mut().zip(value.trim().split('.')) {
            *slot = piece
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap_or(0);
        }
        out
    };
    parts(candidate) >= parts(minimum)
}

/// How long ago, in words a caller can translate. Coarse on purpose: an
/// operator deciding whether a machine is still in use does not need seconds,
/// and a precise-looking relative time invites more trust than the underlying
/// 24-hour check-in can support.
fn bucket(age_seconds: i64) -> &'static str {
    match age_seconds {
        ..=-1 => "ahead",
        0..=59 => "moments",
        60..=3_599 => "minutes",
        3_600..=86_399 => "hours",
        86_400..=604_799 => "days",
        604_800..=2_591_999 => "weeks",
        _ => "months",
    }
}

/// `current` while the install is inside its own daily check-in; `stale` while
/// it could still be running on a retained offline lease; `silent` once even
/// that has run out. Both boundaries are the lease constants themselves, so
/// this judgement cannot drift from what the product actually enforces.
fn staleness(age_seconds: i64) -> &'static str {
    if age_seconds <= CHECK_IN_SECONDS {
        "current"
    } else if age_seconds <= OFFLINE_LEASE_SECONDS {
        "stale"
    } else {
        "silent"
    }
}

fn lease_token(access: &DesktopAccess) -> &'static str {
    match access {
        DesktopAccess::Allowed => "allowed",
        DesktopAccess::Blocked => "blocked",
        DesktopAccess::UpgradeRequired => "upgrade_required",
        DesktopAccess::HandshakeRequired => "handshake_required",
        DesktopAccess::ClockRollback => "clock_rollback",
    }
}

struct Projected {
    host_kind: &'static str,
    last_seen_ms: i64,
    install_id: String,
    row: Value,
    blocked: bool,
    silent: bool,
    retired: bool,
}

/// One installation, read as an operator needs to read it.
fn project_row(
    record: &Value,
    now_ms: i64,
    min_supported_version: &str,
) -> Result<Projected, String> {
    let install_id = text(record, "install_id");
    if install_id.is_empty() || install_id.len() > 128 {
        return Err(bad("an installation record has no usable install_id"));
    }
    let host_kind = match text(record, "host_kind").as_str() {
        "server" => "server",
        "desktop" => "desktop",
        // A row written before host kinds existed still carries the markers a
        // windowless host reports; ds-brain normalizes on read, and this is
        // the same reading for any caller that hands over raw bytes.
        _ if text(record, "webview_version").eq_ignore_ascii_case("native-server") => "server",
        _ => "desktop",
    };

    let last_seen_ms = epoch_ms(&text(record, "last_seen_at")).unwrap_or(0);
    let first_seen_ms = epoch_ms(&text(record, "first_seen_at")).unwrap_or(0);
    let lease_expires_ms = epoch_ms(&text(record, "lease_expires_at")).unwrap_or(0);
    let age_seconds = if last_seen_ms > 0 {
        (now_ms - last_seen_ms) / 1_000
    } else {
        i64::MAX / 2
    };

    let device = match text(record, "status").as_str() {
        "blocked" => "blocked",
        _ => "active",
    };
    let licence = match text(record, "license_status").as_str() {
        "blocked" => "blocked",
        // An empty token is a row that predates licence controls, which
        // ds-brain reads as allowed. Saying so beats rendering a blank.
        _ => "active",
    };
    let blocked = device == "blocked" || licence == "blocked";
    let app_version = text(record, "app_version");
    let upgrade_required = !min_supported_version.is_empty()
        && !app_version.is_empty()
        && !version_at_least(&app_version, min_supported_version);

    // The lease the install is actually holding, decided by the one function
    // the installed product itself calls. A second implementation here would
    // be a second answer, and the page would eventually contradict the app.
    let lease_status = if blocked {
        "blocked"
    } else if upgrade_required {
        "upgrade_required"
    } else {
        "active"
    };
    let issued = last_seen_ms / 1_000;
    let lease = evaluate(
        lease_status,
        issued,
        lease_expires_ms / 1_000,
        now_ms / 1_000,
        issued,
    );

    let retired_at_ms = optional(record, "retired_at").and_then(|value| epoch_ms(&value));
    let retired = retired_at_ms.is_some();
    // A retired install that refreshed its licence AFTER the removal was
    // recorded is the one contradiction worth showing loudly: either the
    // removal did not take, or the record was wrong. It is never resolved
    // silently, and it does not lapse — the contradiction is that the machine
    // reported after it was declared gone, which stays true however long ago
    // that was. Tying it to a fresh check-in would have erased the evidence a
    // day later, in the family built to preserve exactly this.
    let reporting_after_retirement = retired && retired_at_ms.is_some_and(|at| last_seen_ms > at);

    let principal_email = optional(record, "account_email");
    let principal_uid = optional(record, "account_uid");
    let authenticated = principal_email.is_some() || principal_uid.is_some();
    let silent = last_seen_ms == 0 || staleness(age_seconds) == "silent";

    // One remedy per row, most consequential first. An operator reading a
    // table wants the next action, not a list of adjectives.
    let remedy = if reporting_after_retirement {
        "retired_but_reporting"
    } else if retired {
        "retired_recorded"
    } else if blocked {
        "blocked_by_operator"
    } else if upgrade_required {
        "upgrade_required"
    } else if matches!(lease.access, DesktopAccess::ClockRollback) {
        "clock_rollback"
    } else if !authenticated {
        "never_handshaked"
    } else if silent {
        "silent_consider_retiring"
    } else if matches!(lease.access, DesktopAccess::HandshakeRequired) {
        "handshake_required"
    } else {
        "none"
    };

    let row = json!({
        "install_id": install_id,
        "identity": {
            "platform": text(record, "device"),
            "arch": text(record, "arch"),
            "version": app_version,
            "lane": text(record, "channel"),
            "host_kind": host_kind,
            "os_version": text(record, "os_version"),
            "webview_version": text(record, "webview_version"),
        },
        "principal": {
            "kind": if authenticated { "account" } else { "unauthenticated" },
            "email": principal_email,
            "uid": principal_uid,
            "organization_id": optional(record, "organization_id"),
        },
        "first_seen": { "at_ms": first_seen_ms },
        "last_seen": {
            "at_ms": last_seen_ms,
            "age_seconds": if last_seen_ms > 0 { Value::from(age_seconds) } else { Value::Null },
            "bucket": if last_seen_ms > 0 { bucket(age_seconds) } else { "never" },
            // One writer, named. ds-brain stamps last_seen inside the
            // license-refresh transaction and in no other place.
            "source": "licence_refresh",
            "staleness": if last_seen_ms > 0 { staleness(age_seconds) } else { "silent" },
        },
        "state": {
            "governed": if blocked { "blocked" } else { "allowed" },
            "device": device,
            "licence": licence,
            // Both tokens gate work identically; only a licence block also
            // travels into the signed offline lease. When they disagree, say
            // so rather than collapsing them into one word.
            "tokens_disagree": device != licence,
            "lease": lease_token(&lease.access),
            "lease_expires_at_ms": lease_expires_ms,
            "lease_remaining_seconds": lease.remaining_seconds,
            "handshake_due": lease.handshake_due,
            "retired": retired,
            "retired_at_ms": retired_at_ms,
            "retired_by": optional(record, "retired_by"),
            "retired_reason": optional(record, "retired_reason"),
            "reporting_after_retirement": reporting_after_retirement,
        },
        "remedy": remedy,
        "policy": {
            "revision": record["policy_revision"].as_i64().unwrap_or(0),
            "actor": optional(record, "policy_actor"),
            "reason": optional(record, "policy_reason"),
            "changed_at_ms": optional(record, "policy_changed_at").and_then(|value| epoch_ms(&value)),
        },
    });

    Ok(Projected {
        host_kind,
        last_seen_ms,
        install_id,
        row,
        blocked,
        silent,
        retired,
    })
}

fn matches(projected: &Projected, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    let identity = &projected.row["identity"];
    let principal = &projected.row["principal"];
    [
        projected.install_id.as_str(),
        identity["platform"].as_str().unwrap_or_default(),
        identity["version"].as_str().unwrap_or_default(),
        identity["lane"].as_str().unwrap_or_default(),
        identity["host_kind"].as_str().unwrap_or_default(),
        principal["email"].as_str().unwrap_or_default(),
        principal["uid"].as_str().unwrap_or_default(),
    ]
    .iter()
    .any(|field| field.to_lowercase().contains(needle))
}

fn rows_of(request: &Value) -> Result<&Vec<Value>, String> {
    let page = &request["page"];
    let rows = page["installs"]
        .as_array()
        .ok_or_else(|| bad("page.installs must be the array ds-brain returned"))?;
    if rows.len() > MAX_ROWS {
        return Err(bad("page.installs exceeds 5000 records"));
    }
    Ok(rows)
}

fn now_of(request: &Value) -> Result<i64, String> {
    request["now_ms"]
        .as_i64()
        .filter(|value| *value > 0)
        .ok_or_else(|| bad("now_ms must be the host's clock in epoch milliseconds"))
}

/// The list projection: grouped, ordered, counted and bounded.
fn project(request: &Value) -> Result<Value, String> {
    let now_ms = now_of(request)?;
    let observed_at_ms = request["observed_at_ms"].as_i64().unwrap_or(now_ms);
    let include_retired = request["include_retired"].as_bool().unwrap_or(false);
    let minimum = request["min_supported_version"]
        .as_str()
        .unwrap_or_default();
    let needle = request["search"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    let limit = request["limit"]
        .as_u64()
        .map(|value| (value as usize).clamp(1, MAX_LIMIT))
        .unwrap_or(MAX_LIMIT);

    // Pages accumulate in the host; one install has one row whichever page it
    // arrived on, and the freshest record for an id wins.
    let mut ordered: Vec<Projected> = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for record in rows_of(request)? {
        let projected = project_row(record, now_ms, minimum)?;
        match seen.get(&projected.install_id) {
            Some(index) => ordered[*index] = projected,
            None => {
                seen.insert(projected.install_id.clone(), ordered.len());
                ordered.push(projected);
            }
        }
    }

    let total = ordered.len();
    // What this view is hiding, not what the page happens to contain: a
    // removal that the search would have excluded anyway was never hidden by
    // the retirement filter, and counting it would overstate what one click on
    // "show retired" brings back.
    let retired_hidden = if include_retired {
        0
    } else {
        ordered
            .iter()
            .filter(|row| row.retired && matches(row, &needle))
            .count()
    };
    let mut kept: Vec<Projected> = ordered
        .into_iter()
        .filter(|row| (include_retired || !row.retired) && matches(row, &needle))
        .collect();

    // Freshest first: the machine that spoke most recently is the one an
    // operator is most likely to be asking about. Ties fall back to the id so
    // two readings of the same page never reorder.
    kept.sort_by(|left, right| {
        right
            .last_seen_ms
            .cmp(&left.last_seen_ms)
            .then_with(|| left.install_id.cmp(&right.install_id))
    });
    let matched = kept.len();
    let truncated = matched > limit;
    kept.truncate(limit);

    let mut groups: Vec<Value> = Vec::new();
    for key in ["desktop", "server"] {
        let rows: Vec<Value> = kept
            .iter()
            .filter(|row| row.host_kind == key)
            .map(|row| row.row.clone())
            .collect();
        groups.push(json!({ "key": key, "count": rows.len(), "rows": rows }));
    }

    Ok(json!({
        "schema": SCHEMA,
        "operation": "project",
        "now_ms": now_ms,
        "observed_at_ms": observed_at_ms,
        "groups": groups,
        "totals": {
            "shown": kept.len(),
            "matched": matched,
            "received": total,
            "retired_hidden": retired_hidden,
            "blocked": kept.iter().filter(|row| row.blocked).count(),
            "silent": kept.iter().filter(|row| row.silent).count(),
            "servers": kept.iter().filter(|row| row.host_kind == "server").count(),
            "desktops": kept.iter().filter(|row| row.host_kind == "desktop").count(),
            "truncated": truncated,
            "limit": limit,
        },
        "next_cursor": request["page"]["next_cursor"].clone(),
        "include_retired": include_retired,
    }))
}

/// The detail projection: one install, its observed users and its immutable
/// history, in the same vocabulary the list uses.
fn detail(request: &Value) -> Result<Value, String> {
    let now_ms = now_of(request)?;
    let observed_at_ms = request["observed_at_ms"].as_i64().unwrap_or(now_ms);
    let minimum = request["min_supported_version"]
        .as_str()
        .unwrap_or_default();
    let record = &request["detail"]["install"];
    if !record.is_object() {
        return Err(bad("detail.install must be the record ds-brain returned"));
    }
    let projected = project_row(record, now_ms, minimum)?;

    let bounded = |key: &str| -> Result<&Vec<Value>, String> {
        let rows = request["detail"][key]
            .as_array()
            .ok_or_else(|| bad("detail.users and detail.events must be arrays"))?;
        if rows.len() > MAX_ROWS {
            return Err(bad("detail collection exceeds 5000 records"));
        }
        Ok(rows)
    };

    let users: Vec<Value> = bounded("users")?
        .iter()
        .map(|user| {
            let last = epoch_ms(&text(user, "last_seen_at")).unwrap_or(0);
            let age = if last > 0 {
                (now_ms - last) / 1_000
            } else {
                i64::MAX / 2
            };
            json!({
                "uid": text(user, "uid"),
                "email": optional(user, "email"),
                "first_seen_at_ms": epoch_ms(&text(user, "first_seen_at")).unwrap_or(0),
                "last_seen_at_ms": last,
                "bucket": if last > 0 { bucket(age) } else { "never" },
                "handshake_count": user["handshake_count"].as_i64().unwrap_or(0),
            })
        })
        .collect();

    let events: Vec<Value> = bounded("events")?
        .iter()
        .map(|event| {
            // An event written before retirement existed carries no kind; it
            // was a policy change, and reading it as one rewrites nothing.
            let kind = match text(event, "kind").as_str() {
                "retirement" => "retirement",
                _ => "policy",
            };
            json!({
                "revision": event["revision"].as_i64().unwrap_or(0),
                "kind": kind,
                "actor": text(event, "actor"),
                "at_ms": epoch_ms(&text(event, "at")).unwrap_or(0),
                "reason": text(event, "reason"),
                "device": { "from": text(event, "previous_device_status"), "to": text(event, "device_status") },
                "licence": { "from": text(event, "previous_license_status"), "to": text(event, "license_status") },
                "retired": { "from": event["previous_retired"].as_bool().unwrap_or(false), "to": event["retired"].as_bool().unwrap_or(false) },
            })
        })
        .collect();

    Ok(json!({
        "schema": SCHEMA,
        "operation": "detail",
        "now_ms": now_ms,
        "observed_at_ms": observed_at_ms,
        "install": projected.row,
        "users": users,
        "events": events,
        "next_user_cursor": request["detail"]["next_user_cursor"].clone(),
        "next_event_cursor": request["detail"]["next_event_cursor"].clone(),
        "totals": { "users": users.len(), "events": events.len() },
    }))
}

/// The one door. Hosts supply authenticated transport and the raw records;
/// this captures nothing, calls nothing, and holds no clock of its own.
pub fn evaluate_request(input: &[u8]) -> Result<Value, String> {
    if input.len() > MAX_REQUEST_BYTES {
        return Err(bad("installation inventory request exceeds 1 MiB"));
    }
    let request: Value = serde_json::from_slice(input).map_err(|error| bad(&error.to_string()))?;
    if request["schema"].as_str() != Some(SCHEMA) {
        return Err(bad("schema must be ds.installation-inventory/v1"));
    }
    match request["operation"].as_str() {
        Some("project") => project(&request),
        Some("detail") => detail(&request),
        _ => Err(bad("operation must be project or detail")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-09-18T04:00:00Z — the clock every fixture below is read against.
    const NOW: i64 = 1_789_704_000_000;

    fn record(id: &str, last_seen: &str, extra: Value) -> Value {
        let mut row = json!({
            "install_id": id,
            "device": "linux",
            "arch": "x86_64",
            "app_version": "0.1.3",
            "channel": "canary",
            "os_version": "Ubuntu 26.04",
            "webview_version": "120.0",
            "first_seen_at": "2026-01-01T00:00:00Z",
            "last_seen_at": last_seen,
            "status": "active",
            "license_status": "active",
            // Seven days from the freshest fixture check-in: exactly the lease
            // the product issues, so `evaluate` is exercised, not bypassed.
            "lease_expires_at": "2026-09-25T00:00:00Z",
            "policy_revision": 0,
        });
        for (key, value) in extra.as_object().unwrap() {
            row[key] = value.clone();
        }
        row
    }

    fn project_of(rows: Vec<Value>, extra: Value) -> Value {
        let mut request = json!({
            "schema": SCHEMA, "operation": "project", "now_ms": NOW,
            "min_supported_version": "0.1.0",
            "page": { "installs": rows },
        });
        for (key, value) in extra.as_object().unwrap() {
            request[key] = value.clone();
        }
        evaluate_request(&serde_json::to_vec(&request).unwrap()).unwrap()
    }

    #[test]
    fn rfc3339_becomes_an_exact_epoch() {
        assert_eq!(epoch_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(epoch_ms("2026-09-18T02:27:48Z"), Some(1_789_698_468_000));
        assert_eq!(epoch_ms("2000-02-29T12:00:00Z"), Some(951_825_600_000));
        assert_eq!(epoch_ms("not a time"), None);
        // An offset is honoured, so a record written on a host that is not on
        // UTC cannot read as hours in the future.
        assert_eq!(
            epoch_ms("2026-09-18T04:27:48+02:00"),
            epoch_ms("2026-09-18T02:27:48Z")
        );
        assert_eq!(
            epoch_ms("2026-09-17T21:27:48.500-05:00"),
            epoch_ms("2026-09-18T02:27:48Z")
        );
        assert_eq!(
            epoch_ms("2026-09-18T02:27:48.123456789Z"),
            Some(1_789_698_468_000)
        );
    }

    #[test]
    fn a_server_install_is_its_own_group_even_on_a_row_that_predates_host_kinds() {
        let projected = project_of(
            vec![
                record("a1", "2026-09-18T02:00:00Z", json!({})),
                record(
                    "b2",
                    "2026-09-18T02:00:00Z",
                    json!({"webview_version": "native-server", "os_version": "linux native-server"}),
                ),
            ],
            json!({}),
        );
        let groups = projected["groups"].as_array().unwrap();
        assert_eq!(groups[0]["key"], "desktop");
        assert_eq!(groups[0]["count"], 1);
        assert_eq!(groups[1]["key"], "server");
        assert_eq!(groups[1]["rows"][0]["identity"]["host_kind"], "server");
        assert_eq!(projected["totals"]["servers"], 1);
    }

    #[test]
    fn last_seen_names_its_single_writer_and_grades_itself_by_the_lease_constants() {
        // The boundaries are the product's, not this module's: one check-in
        // period, then one offline lease, then silence.
        assert_eq!(staleness(CHECK_IN_SECONDS), "current");
        assert_eq!(staleness(CHECK_IN_SECONDS + 1), "stale");
        assert_eq!(staleness(OFFLINE_LEASE_SECONDS), "stale");
        assert_eq!(staleness(OFFLINE_LEASE_SECONDS + 1), "silent");
        assert_eq!(bucket(30), "moments");
        assert_eq!(bucket(3_600), "hours");
        assert_eq!(bucket(10 * 86_400), "weeks");
        let projected = project_of(
            vec![record(
                "a1",
                "2026-09-18T02:00:00Z",
                json!({"account_email": "a@example.test"}),
            )],
            json!({}),
        );
        let row = &projected["groups"][0]["rows"][0];
        assert_eq!(row["last_seen"]["source"], "licence_refresh");
        assert_eq!(row["last_seen"]["bucket"], "hours");
        assert_eq!(row["last_seen"]["staleness"], "current");
        assert_eq!(row["state"]["lease"], "allowed");
        assert_eq!(row["remedy"], "none");
    }

    #[test]
    fn an_install_that_never_handshook_says_so_rather_than_naming_nobody() {
        let projected = project_of(
            vec![record("a1", "2026-09-18T02:00:00Z", json!({}))],
            json!({}),
        );
        let row = &projected["groups"][0]["rows"][0];
        assert_eq!(row["principal"]["kind"], "unauthenticated");
        assert_eq!(row["principal"]["email"], Value::Null);
        assert_eq!(row["remedy"], "never_handshaked");
    }

    #[test]
    fn a_blocked_licence_reads_as_one_governed_state_and_flags_disagreement() {
        let projected = project_of(
            vec![record(
                "a1",
                "2026-09-18T02:00:00Z",
                json!({"license_status": "blocked", "account_email": "user@example.test"}),
            )],
            json!({}),
        );
        let row = &projected["groups"][0]["rows"][0];
        assert_eq!(row["state"]["governed"], "blocked");
        assert_eq!(row["state"]["tokens_disagree"], true);
        assert_eq!(row["state"]["lease"], "blocked");
        assert_eq!(row["remedy"], "blocked_by_operator");
        assert_eq!(projected["totals"]["blocked"], 1);
    }

    #[test]
    fn an_install_below_the_minimum_version_is_told_to_upgrade() {
        let projected = project_of(
            vec![record(
                "a1",
                "2026-09-18T02:00:00Z",
                json!({"app_version": "0.0.9", "account_email": "user@example.test"}),
            )],
            json!({"min_supported_version": "0.1.2"}),
        );
        assert_eq!(
            projected["groups"][0]["rows"][0]["state"]["lease"],
            "upgrade_required"
        );
        assert_eq!(
            projected["groups"][0]["rows"][0]["remedy"],
            "upgrade_required"
        );
    }

    #[test]
    fn a_retired_install_leaves_the_default_view_and_is_counted_not_deleted() {
        let rows = vec![
            record(
                "a1",
                "2026-09-18T02:00:00Z",
                json!({"account_email": "a@example.test"}),
            ),
            record(
                "b2",
                "2026-06-01T02:00:00Z",
                json!({"retired_at": "2026-06-02T00:00:00Z", "retired_by": "admin@example.test", "retired_reason": "wiped"}),
            ),
        ];
        let hidden = project_of(rows.clone(), json!({}));
        assert_eq!(hidden["totals"]["shown"], 1);
        assert_eq!(hidden["totals"]["retired_hidden"], 1);
        let shown = project_of(rows, json!({"include_retired": true}));
        assert_eq!(shown["totals"]["shown"], 2);
        assert_eq!(shown["totals"]["retired_hidden"], 0);
        let retired = shown["groups"][0]["rows"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["install_id"] == "b2")
            .unwrap()
            .clone();
        assert_eq!(retired["state"]["retired"], true);
        assert_eq!(retired["state"]["retired_by"], "admin@example.test");
        assert_eq!(retired["remedy"], "retired_recorded");
    }

    #[test]
    fn silence_is_reported_as_silence_and_never_as_an_uninstallation() {
        let projected = project_of(
            vec![record(
                "a1",
                "2026-06-01T02:00:00Z",
                json!({"account_email": "a@example.test"}),
            )],
            json!({}),
        );
        let row = &projected["groups"][0]["rows"][0];
        assert_eq!(row["state"]["retired"], false);
        assert_eq!(row["last_seen"]["staleness"], "silent");
        assert_eq!(row["last_seen"]["bucket"], "months");
        assert_eq!(row["remedy"], "silent_consider_retiring");
        assert_eq!(projected["totals"]["silent"], 1);
    }

    #[test]
    fn a_retired_install_that_keeps_checking_in_is_shown_as_the_contradiction_it_is() {
        let projected = project_of(
            vec![record(
                "b2",
                "2026-09-18T02:00:00Z",
                json!({"retired_at": "2026-09-17T00:00:00Z", "account_email": "a@example.test"}),
            )],
            json!({"include_retired": true}),
        );
        let row = &projected["groups"][0]["rows"][0];
        assert_eq!(row["state"]["reporting_after_retirement"], true);
        assert_eq!(row["remedy"], "retired_but_reporting");
    }

    /// The contradiction is "it reported after it was declared gone", and that
    /// does not stop being true once the machine falls silent again. An
    /// operator reading this row a month later is reading evidence.
    #[test]
    fn a_check_in_after_a_recorded_removal_stays_visible_once_the_machine_goes_quiet() {
        let projected = project_of(
            vec![record(
                "b2",
                "2026-06-10T00:00:00Z",
                json!({"retired_at": "2026-06-02T00:00:00Z", "account_email": "a@example.test"}),
            )],
            json!({"include_retired": true}),
        );
        let row = &projected["groups"][0]["rows"][0];
        assert_eq!(row["last_seen"]["staleness"], "silent");
        assert_eq!(row["state"]["reporting_after_retirement"], true);
        assert_eq!(row["remedy"], "retired_but_reporting");
    }

    /// A removal the search would have dropped anyway was never hidden by the
    /// retirement filter, so the count of what one toggle brings back is the
    /// count of removals in THIS view.
    #[test]
    fn the_hidden_removal_count_describes_this_view_and_not_the_whole_page() {
        let rows = vec![
            record(
                "a1",
                "2026-09-18T02:00:00Z",
                json!({"account_email": "alice@example.test"}),
            ),
            record(
                "b2",
                "2026-06-01T02:00:00Z",
                json!({"retired_at": "2026-06-02T00:00:00Z", "account_email": "bob@example.test"}),
            ),
        ];
        assert_eq!(
            project_of(rows.clone(), json!({"search": "alice"}))["totals"]["retired_hidden"],
            0
        );
        assert_eq!(
            project_of(rows, json!({"search": "bob"}))["totals"]["retired_hidden"],
            1
        );
    }

    #[test]
    fn pages_merge_by_identity_order_by_freshness_and_report_their_bound() {
        let projected = project_of(
            vec![
                record(
                    "a1",
                    "2026-09-01T00:00:00Z",
                    json!({"account_email": "a@example.test"}),
                ),
                record(
                    "b2",
                    "2026-09-17T00:00:00Z",
                    json!({"account_email": "b@example.test"}),
                ),
                record(
                    "a1",
                    "2026-09-18T02:00:00Z",
                    json!({"account_email": "a@example.test"}),
                ),
            ],
            json!({"limit": 1}),
        );
        assert_eq!(projected["totals"]["received"], 2);
        assert_eq!(projected["totals"]["matched"], 2);
        assert_eq!(projected["totals"]["truncated"], true);
        assert_eq!(projected["totals"]["limit"], 1);
        assert_eq!(projected["groups"][0]["rows"][0]["install_id"], "a1");
    }

    #[test]
    fn search_is_one_projection_over_identity_and_principal() {
        let rows = vec![
            record(
                "a1",
                "2026-09-18T02:00:00Z",
                json!({"account_email": "alice@example.test"}),
            ),
            record(
                "b2",
                "2026-09-18T02:00:00Z",
                json!({"account_email": "bob@example.test"}),
            ),
        ];
        let projected = project_of(rows, json!({"search": "ALICE"}));
        assert_eq!(projected["totals"]["matched"], 1);
        assert_eq!(projected["groups"][0]["rows"][0]["install_id"], "a1");
    }

    #[test]
    fn a_history_event_written_before_retirement_existed_still_reads_as_a_policy_change() {
        let request = json!({
            "schema": SCHEMA, "operation": "detail", "now_ms": NOW,
            "min_supported_version": "0.1.0",
            "detail": {
                "install": record("a1", "2026-09-18T02:00:00Z", json!({})),
                "users": [{"uid": "u1", "email": "a@example.test", "first_seen_at": "2026-01-01T00:00:00Z", "last_seen_at": "2026-09-18T00:00:00Z", "handshake_count": 4}],
                "events": [{"revision": 1, "actor": "admin@example.test", "at": "2026-05-01T00:00:00Z", "reason": "suspended", "previous_device_status": "active", "previous_license_status": "active", "device_status": "active", "license_status": "blocked"}],
            },
        });
        let out = evaluate_request(&serde_json::to_vec(&request).unwrap()).unwrap();
        assert_eq!(out["events"][0]["kind"], "policy");
        assert_eq!(out["events"][0]["licence"]["to"], "blocked");
        assert_eq!(out["events"][0]["retired"]["to"], false);
        assert_eq!(out["users"][0]["bucket"], "hours");
        assert_eq!(out["totals"]["events"], 1);
    }

    #[test]
    fn a_request_outside_its_schema_or_bound_is_refused_with_a_code() {
        for bad_input in [
            &b"{}"[..],
            br#"{"schema":"other","operation":"project","now_ms":1}"#,
            br#"{"schema":"ds.installation-inventory/v1","operation":"delete","now_ms":1}"#,
            br#"{"schema":"ds.installation-inventory/v1","operation":"project"}"#,
        ] {
            let refusal = evaluate_request(bad_input).unwrap_err();
            assert!(
                refusal.starts_with("invalid_inventory_request:"),
                "{refusal}"
            );
        }
        let too_many = json!({
            "schema": SCHEMA, "operation": "project", "now_ms": NOW,
            "page": { "installs": vec![json!({"install_id": "a1"}); MAX_ROWS + 1] },
        });
        assert!(evaluate_request(&serde_json::to_vec(&too_many).unwrap())
            .unwrap_err()
            .contains("5000"));
    }
}
