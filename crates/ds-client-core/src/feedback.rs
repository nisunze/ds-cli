//! Fixed authenticated product backlog. No project selection or second store.
use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    FeedbackCall, ServiceRefusal,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    List {
        view: String,
        #[serde(default)]
        component: Option<String>,
        #[serde(default)]
        query: Option<String>,
        limit: usize,
        #[serde(default)]
        detail: bool,
        /// An opaque watermark the backlog issued. Absent opts into the one
        /// the backlog remembers for this account; `all` opts out entirely.
        #[serde(default)]
        cursor: Option<String>,
        #[serde(default)]
        all: bool,
    },
    Submit {
        title: String,
        detail: String,
        component: String,
        kind: String,
        severity: String,
        agent: String,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        client: Option<String>,
        #[serde(default)]
        evidence: Vec<String>,
        #[serde(default)]
        context: BTreeMap<String, String>,
    },
    Close {
        id: String,
        status: String,
        resolution: String,
        #[serde(default)]
        expected_version: Option<u64>,
    },
    Note {
        id: String,
        text: String,
        #[serde(default)]
        blocked_on: Option<String>,
        #[serde(default)]
        unblock: bool,
        #[serde(default)]
        expected_version: Option<u64>,
    },
}
fn invalid() -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, "Invalid bounded feedback request")
}
/// The live repositories a report may name as its owner: the workspace's live
/// stack. A component is `<repository>` or `<repository>/<area>`, so triage by
/// repository needs no hand-made mapping.
pub const LIVE_REPOSITORIES: &[&str] = &[
    "ds-apis-tf",
    "ds-brain",
    "ds-cli",
    "ds-command-kernel",
    "ds-integration",
    "ds-network",
    "ds-network-reporter",
    "ds-solar",
    "ds-solar-weather",
    "ds-sre",
    "ds-vector-tiler",
    "ds-web",
    "ds-work",
    "simple_notes",
];

/// Label prefixes earlier reports used, and the live repository that owns each.
pub const LEGACY_COMPONENTS: &[(&str, &str)] = &[
    ("ds-grid", "ds-network"),
    ("dsgrid", "ds-network"),
    ("ds/grid-", "ds-network"),
    ("ds/pls-library", "ds-network"),
    ("ds-report", "ds-network-reporter"),
    ("ds-printing", "ds-network-reporter"),
    ("ds-desktop", "ds-web"),
    ("ds pls desktop", "ds-cli"),
];

/// The refusal a component that names no live repository gets.
pub const COMPONENT_UNKNOWN: &str = "feedback_component_unknown";

/// The live repository a component names: its text before the first `/`.
pub fn component_repository(component: &str) -> Option<&'static str> {
    let leading = component.split('/').next().unwrap_or_default();
    LIVE_REPOSITORIES
        .iter()
        .copied()
        .find(|repository| *repository == leading)
}

/// The live repository a legacy label belongs to, for the refusal's remedy.
pub fn legacy_owner(component: &str) -> Option<&'static str> {
    LEGACY_COMPONENTS
        .iter()
        .find(|(prefix, _)| component.starts_with(prefix))
        .map(|(_, owner)| *owner)
}
fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "Feedback response violates the product backlog contract",
    )
}
fn text(s: &str, max: usize) -> bool {
    !s.is_empty()
        && s.trim() == s
        && s.chars().count() <= max
        && !s
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\t'))
}
/// The backlog's own view and blocked filter behind each view a caller names.
///
/// `open` is what is legitimately open: unsettled and waiting on nothing. A
/// report parked on a release, a deploy or a ruling names that blocker, and a
/// sweep that re-read it every time would pay for a report nobody can act on.
/// `waiting` is the other half. Both are the backlog's filter, never a
/// narrowing here, so its watermark stays truthful per view.
fn backlog_view(view: &str) -> (&str, Option<bool>) {
    match view {
        "open" => ("not_addressed", Some(false)),
        "waiting" => ("not_addressed", Some(true)),
        other => (other, None),
    }
}
fn date(s: &str) -> Option<time::OffsetDateTime> {
    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
}
impl Command {
    /// Two positions can carry distinct admitted 200-character report IDs.
    /// The closed bound includes worst-case JSON escaping and base64 encoding.
    pub const MAX_CURSOR_BYTES: usize = 4096;

    pub fn validate(&self) -> Result<(), ClientError> {
        let optional = |s: &Option<String>, max| s.as_ref().is_none_or(|v| text(v, max));
        let valid = match self {
            Self::List {
                view,
                component,
                query,
                limit,
                cursor,
                ..
            } => {
                ["open", "waiting", "not_addressed", "addressed", "all"].contains(&view.as_str())
                    && optional(component, 200)
                    && optional(query, 200)
                    && (1..=50).contains(limit)
                    // The cursor is opaque: this validates its shape, never its
                    // meaning. A caller that could compose one would be back to
                    // remembering timestamps, which is the habit it replaces.
                    && cursor.as_ref().is_none_or(|c| text(c, Self::MAX_CURSOR_BYTES) && c.len() <= Self::MAX_CURSOR_BYTES && c.starts_with("fb1."))
            }
            Self::Submit {
                title,
                detail,
                component,
                kind,
                severity,
                agent,
                model,
                client,
                evidence,
                context,
            } => {
                text(title, 200)
                    && text(detail, 16384)
                    && detail.len() <= 16384
                    && text(component, 200)
                    && ["obstacle", "bug", "friction", "idea", "question"].contains(&kind.as_str())
                    && ["blocker", "major", "minor", "trivial"].contains(&severity.as_str())
                    && text(agent, 200)
                    && optional(model, 200)
                    && optional(client, 200)
                    && evidence.len() <= 20
                    && evidence.iter().all(|s| text(s, 500))
                    && context.len() <= 24
                    && context.iter().all(|(k, v)| text(k, 500) && text(v, 1000))
            }
            Self::Close {
                id,
                status,
                resolution,
                expected_version,
            } => {
                text(id, 200)
                    && ["resolved", "wont_fix"].contains(&status.as_str())
                    && text(resolution, 1000)
                    && expected_version.is_none_or(|v| v > 0 && v <= i64::MAX as u64)
            }
            Self::Note {
                id,
                text: body,
                blocked_on,
                unblock,
                expected_version,
            } => {
                text(id, 200)
                    && text(body, 1000)
                    && optional(blocked_on, 200)
                    // One note cannot both raise a dependency and clear one:
                    // the record would not say which happened.
                    && !(blocked_on.is_some() && *unblock)
                    && expected_version.is_none_or(|v| v > 0 && v <= i64::MAX as u64)
            }
        };
        if !valid {
            return Err(invalid());
        }
        match self {
            Self::Submit { component, .. } if component_repository(component).is_none() => {
                Err(ClientError::new(ErrorKind::InvalidInput, COMPONENT_UNKNOWN))
            }
            _ => Ok(()),
        }
    }
}
/// How much of the acceptance text a LIST ROW carries before `--detail`.
///
/// This number is the mountain, measured. The backlog reached 124 open reports
/// whose acceptance text runs long, and a row that carried 1200 characters of
/// it meant roughly 150 000 characters — about 37 000 tokens — for one visit,
/// which is how a handful of visits came to cost 1.5 million. The watermark
/// stops the RE-reading; this stops the first read from being a mountain in
/// the first place.
///
/// A row exists to decide "do I care about this", and what decides that is the
/// title, the status, whether it is blocked and on what, and the latest note —
/// all of which are on the row in full. The acceptance condition decides
/// something else entirely: whether the report can be CLOSED. That is the
/// handful of ids at the end of a triage pass, and `--detail` is how they are
/// read. `detail_truncated` says which of the two this row is.
const ROW_DETAIL_CHARS: usize = 240;

#[derive(Deserialize, Serialize)]
struct Report {
    id: String,
    status: String,
    kind: String,
    severity: String,
    component: String,
    surface: String,
    title: String,
    detail: String,
    occurrences: u64,
    reporters: Vec<String>,
    reporter_kind: String,
    version: u64,
    last_seen_at: String,
    updated_at: String,
    updated_by: String,
    #[serde(default)]
    resolution: String,
    // Everything below is what makes a ROW decide. Each is optional so an older
    // backlog that does not send it still decodes.
    #[serde(default)]
    blocked: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    blocked_on: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    blocked_since: Option<String>,
    #[serde(default)]
    note_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    latest_note: Option<Note>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    notes: Vec<Note>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    supersedes: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    superseded_by: String,
}

/// One later observation about a report: who, when, what, and what it waits on.
#[derive(Deserialize, Serialize)]
struct Note {
    author: String,
    at: String,
    text: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    blocked_on: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    clears_block: bool,
}
impl Report {
    fn validate(&self) -> Result<(), ClientError> {
        if !text(&self.id, 200)
            || !text(&self.title, 200)
            || self.detail.len() > 16384
            || self.version == 0
            || self.occurrences == 0
            || self.reporters.len() > 50
            || date(&self.last_seen_at).is_none()
            || date(&self.updated_at).is_none()
            || !["open", "triaged", "in_progress", "resolved", "wont_fix"]
                .contains(&self.status.as_str())
            || self.note_count > 20
            || self.notes.len() > 20
            || self.blocked_on.chars().count() > 200
            || self
                .latest_note
                .as_ref()
                .is_some_and(|note| note.text.chars().count() > 1000)
            || self
                .notes
                .iter()
                .any(|note| note.text.chars().count() > 1000)
        {
            return Err(unreadable());
        }
        Ok(())
    }
    fn row(mut self, full: bool) -> Value {
        let clipped = !full && self.detail.chars().count() > ROW_DETAIL_CHARS;
        if clipped {
            self.detail = self.detail.chars().take(ROW_DETAIL_CHARS).collect();
        }
        let mut row = serde_json::to_value(self).expect("feedback row");
        row["detail_truncated"] = json!(clipped);
        row
    }
}
fn report(v: Value) -> Result<Report, ClientError> {
    let r: Report = serde_json::from_value(v).map_err(|_| unreadable())?;
    r.validate()?;
    Ok(r)
}
/// The feedback endpoint answered a retryable service failure, after authorization.
pub const SERVICE_UNAVAILABLE: &str = "feedback_service_unavailable";

fn call<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    payload: Value,
) -> Result<Value, ClientError> {
    let response = transport.feedback(FeedbackCall {
        profile,
        authority: authority.call(),
        payload,
    })?;
    // The backlog names each refusal, and each name carries a different remedy:
    // a settled report wants a NEW report, a note limit wants a close, a bad
    // cursor wants the cursor dropped. Collapsing them onto the HTTP status
    // would strand a caller that can act on the difference.
    if response.status != 200 {
        let named = (response.body.len() <= 64 * 1024)
            .then(|| serde_json::from_slice::<Value>(&response.body).ok())
            .flatten()
            .and_then(|envelope| envelope["error"]["code"].as_str().map(str::to_owned));
        return Err(match named.as_deref() {
            Some("FEEDBACK_SETTLED") => {
                ClientError::new(ErrorKind::InvalidInput, "feedback_settled")
            }
            Some("FEEDBACK_NOTE_LIMIT") => {
                ClientError::new(ErrorKind::InvalidInput, "feedback_note_limit")
            }
            Some("FEEDBACK_CURSOR_INVALID" | "FEEDBACK_CURSOR_SCOPE") => {
                ClientError::new(ErrorKind::InvalidInput, "feedback_cursor_rejected")
            }
            Some("FEEDBACK_BACKLOG_TOO_LARGE") => {
                ClientError::new(ErrorKind::InvalidInput, "feedback_backlog_too_large")
            }
            _ => match response.status {
                403 => {
                    ClientError::new(ErrorKind::AuthenticationRejected, "feedback_not_permitted")
                }
                404 => ClientError::new(ErrorKind::ResourceNotFound, "feedback_not_found"),
                409 => ClientError::new(ErrorKind::InvalidInput, "feedback_conflict"),
                status @ (408 | 425 | 429 | 500..=599) => ClientError::with_service_refusal(
                    ErrorKind::Transient,
                    SERVICE_UNAVAILABLE,
                    // Only the status crosses: a backend stack, private details,
                    // or an index-creation link cannot become operator output.
                    ServiceRefusal::new(status, Some(SERVICE_UNAVAILABLE), None),
                ),
                other => crate::authorized::classify_tile_error(other),
            },
        });
    }
    if response.body.len() > 4 * 1024 * 1024 {
        return Err(unreadable());
    }
    let envelope: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    if envelope["success"] != true || !envelope["data"].is_object() {
        return Err(unreadable());
    }
    Ok(envelope["data"].clone())
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    command: &Command,
) -> Result<Value, ClientError> {
    command.validate()?;
    match command {
        Command::List {
            view,
            component,
            query,
            limit,
            detail,
            cursor,
            all,
        } => {
            // Every filter goes to the backlog, and nothing is narrowed after
            // the answer arrives. That is what makes the watermark it returns
            // truthful: a row this client hid would still have been counted as
            // read, and would never appear in a difference again.
            let (backlog, blocked) = backlog_view(view);
            let mut payload = json!({
                "action": "list",
                "limit": limit,
                "filter": {
                    "view": backlog,
                    "component_contains": component.clone().unwrap_or_default(),
                    "query": query.clone().unwrap_or_default(),
                    "limit": limit,
                },
            });
            if let Some(blocked) = blocked {
                payload["filter"]["blocked"] = json!(blocked);
            }
            // A read that participates in the watermark DRAINS: the backlog
            // hands over the matching reports this account has gone longest
            // without seeing, and a limited answer means the next call carries
            // the next chunk. A read that does not participate re-reads the
            // top every time, so the same truncation means the opposite thing
            // — narrow the question or it will return these rows forever.
            // Only this layer knows which of the two was asked for, so it says
            // so rather than leaving the caller to guess from `cursor_source`,
            // which reads "none" on the first drain and on a full read alike.
            //
            // A SWEEP AND A LOOKUP ARE NOT THE SAME QUESTION, and only a sweep
            // wants a difference. `--component` and `--query` ask "which
            // reports are about this?" — a question whose truthful answer is
            // the same reports every time somebody asks it. Draining it answers
            // it once and answers "nothing changed" ever after, which is how
            // the second triage session goes blind to the very reports it has
            // to match its work against. (Per-scope watermarks are bounded and
            // evicted oldest-first as well, so a session that searched a ninth
            // component would get its lookup answered in full or answered empty
            // depending on eviction order — nondeterminism no caller can reason
            // about.) So the mountain is drained and a lookup is answered. A
            // caller that genuinely wants to drain a narrowed question still
            // can, by echoing the cursor that read returns.
            let narrowed = component.is_some() || query.is_some();
            let drains = cursor.is_some() || !(*all || narrowed);
            if let Some(cursor) = cursor {
                payload["cursor"] = json!(cursor);
            } else if drains {
                // Opting in is explicit, so a client that predates the
                // watermark keeps getting the whole backlog it expects — and a
                // lookup neither reads nor advances a watermark, so searching
                // never moves where the sweep left off.
                payload["use_watermark"] = json!(true);
            }
            let data = call(profile, transport, authority, payload)?;
            let raw = data["reports"]
                .as_array()
                .filter(|r| r.len() <= *limit)
                .ok_or_else(unreadable)?;
            let mut rows = Vec::with_capacity(raw.len());
            for raw in raw {
                rows.push(report(raw.clone())?.row(*detail));
            }
            // `changed: false` is the answer this whole mechanism exists for.
            // It is a field, not an empty list somebody has to interpret.
            let changed = data["changed"].as_bool().ok_or_else(unreadable)?;
            let complete = data["complete"].as_bool().ok_or_else(unreadable)?;
            let total = data["matched"].as_u64().ok_or_else(unreadable)?;
            let next = data["cursor"].as_str().ok_or_else(unreadable)?;
            let mut out = json!({
                "view": view,
                "total": total,
                "count": rows.len(),
                "reports": rows,
                "changed": changed,
                "complete": complete,
                "truncated": !complete,
                "drains": drains,
                "cursor": next,
                "cursor_source": data["cursor_source"].as_str().unwrap_or("none"),
            });
            if let Some(backlog) = data["backlog"].as_object() {
                out["backlog"] = Value::Object(backlog.clone());
            }
            // The one way a complete enumeration can still be partial: a stored
            // document the backlog could not decode. It is left out of the
            // answer AND passed over by the watermark, so it would otherwise be
            // missing from every future difference too — the silent form of the
            // `scan_incomplete` that was removed for admitting partiality.
            // The backlog names it; dropping it here is what would make it
            // silent again, because this is the surface an agent reads.
            if let Some(count) = data["unreadable"].as_u64().filter(|count| *count > 0) {
                out["unreadable"] = json!(count);
            }
            Ok(out)
        }
        Command::Submit {
            title,
            detail,
            component,
            kind,
            severity,
            agent,
            model,
            client,
            evidence,
            context,
        } => {
            let mut a = json!({"name":agent});
            if let Some(m) = model {
                a["model"] = json!(m)
            }
            if let Some(c) = client {
                a["client"] = json!(c)
            }
            let data = call(
                profile,
                transport,
                authority,
                json!({"action":"submit","report":{"title":title,"detail":detail,"component":component,"kind":kind,"severity":severity,"surface":"cli","reporter_kind":"agent","agent":a,"evidence":evidence,"context":context}}),
            )?;
            let r = report(data["report"].clone())?;
            let deduplicated = data["deduplicated"].as_bool().ok_or_else(unreadable)?;
            Ok(json!({"report":r.row(true),"deduplicated":deduplicated}))
        }
        Command::Close {
            id,
            status,
            resolution,
            expected_version,
        } => {
            let current = report(
                call(
                    profile,
                    transport,
                    authority,
                    json!({"action":"get","id":id}),
                )?["report"]
                    .clone(),
            )?;
            if current.id != *id {
                return Err(unreadable());
            }
            let version = expected_version.unwrap_or(current.version);
            let changed=report(call(profile,transport,authority,json!({"action":"update_status","id":id,"status":status,"resolution":resolution,"expected_version":version}))?["report"].clone())?;
            if changed.id != *id
                || changed.status != *status
                || changed.resolution != *resolution
                || changed.version < version
            {
                return Err(unreadable());
            }
            Ok(json!({"report":changed.row(true),"previous_status":current.status}))
        }
        Command::Note {
            id,
            text: body,
            blocked_on,
            unblock,
            expected_version,
        } => {
            let mut note = json!({"text": body});
            if let Some(blocked_on) = blocked_on {
                note["blocked_on"] = json!(blocked_on);
            }
            if *unblock {
                note["unblock"] = json!(true);
            }
            let mut payload = json!({"action":"note","id":id,"note":note});
            if let Some(version) = expected_version {
                payload["expected_version"] = json!(version);
            }
            let noted = report(call(profile, transport, authority, payload)?["report"].clone())?;
            if noted.id != *id {
                return Err(unreadable());
            }
            // A note never touches the submitter's sighting and never moves the
            // report; if the backlog says otherwise, the answer is unreadable
            // rather than quietly accepted.
            if !["open", "triaged", "in_progress"].contains(&noted.status.as_str()) {
                return Err(unreadable());
            }
            let blocked = noted.blocked;
            let blocked_on = noted.blocked_on.clone();
            let note_count = noted.note_count;
            Ok(json!({
                "report": noted.row(true),
                "blocked": blocked,
                "blocked_on": blocked_on,
                "note_count": note_count,
            }))
        }
    }
}

#[cfg(test)]
mod component_tests {
    use super::*;

    fn submit(component: &str) -> Command {
        Command::Submit {
            title: "t".into(),
            detail: "d".into(),
            component: component.into(),
            kind: "bug".into(),
            severity: "minor".into(),
            agent: "a".into(),
            model: None,
            client: None,
            evidence: Vec::new(),
            context: BTreeMap::new(),
        }
    }

    /// FEEDBACK `9ab57db2`: a report names a live repository, optionally with
    /// an area; a retired or invented label is refused, and a legacy label's
    /// refusal can name the repository that owns it.
    #[test]
    fn a_report_names_a_live_repository_or_is_refused() {
        for live in [
            "ds-cli",
            "ds-cli/feedback",
            "ds-network/spotting",
            "simple_notes",
        ] {
            assert!(submit(live).validate().is_ok(), "{live} is accepted");
        }
        for unknown in [
            "ds-grid",
            "ds-grid-engine/sag",
            "platform",
            "ds",
            "ds-desktop/x",
        ] {
            let error = submit(unknown).validate().unwrap_err();
            assert_eq!(error.to_string(), COMPONENT_UNKNOWN, "{unknown}");
        }
        assert_eq!(legacy_owner("ds-grid-engine"), Some("ds-network"));
        assert_eq!(legacy_owner("ds-report-pdf"), Some("ds-network-reporter"));
        assert_eq!(legacy_owner("ds pls desktop (windows)"), Some("ds-cli"));
        assert_eq!(legacy_owner("platform"), None);
        // A component the bounds already refuse keeps its bounded refusal.
        assert_ne!(
            submit(" ds-cli").validate().unwrap_err().to_string(),
            COMPONENT_UNKNOWN
        );
    }
}

#[cfg(test)]
mod cursor_tests {
    use super::*;
    fn list(cursor: String) -> Command {
        Command::List {
            view: "not_addressed".into(),
            component: Some("ds-cli/feedback".into()),
            query: None,
            limit: 1,
            detail: false,
            cursor: Some(cursor),
            all: false,
        }
    }
    #[test]
    fn bounded_page_tokens_accept_admitted_long_ids_without_local_decoding() {
        for size in [741, Command::MAX_CURSOR_BYTES] {
            assert!(list(format!("fb1.{}", "a".repeat(size - 4)))
                .validate()
                .is_ok());
        }
        assert!(list("fb1.legacy.digest".into()).validate().is_ok());
    }
    #[test]
    fn oversized_and_non_token_cursors_are_refused_before_transport() {
        for cursor in [
            format!("fb1.{}", "a".repeat(Command::MAX_CURSOR_BYTES - 3)),
            format!("fb1.{}", "é".repeat(Command::MAX_CURSOR_BYTES / 2)),
            "forged-without-prefix".into(),
            "".into(),
        ] {
            assert_eq!(
                list(cursor).validate().unwrap_err().kind(),
                ErrorKind::InvalidInput
            );
        }
    }
}
