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
        /// The failed invocation, exactly enough to run it again.
        #[serde(default)]
        replay: Option<Replay>,
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
    "ds-sre",
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
/// What a coding session needs to run one failed invocation again, exactly:
/// the build, the platform, the command and its argument list (ds-command-kernel
/// `docs/contracts/ds-cli-reliability-and-feedback.md` §4).
///
/// The host builds it from the command's descriptor. Every flag, choice and
/// value travels as typed, except a value the descriptor marks sensitive
/// ([`REDACTED`]), a value no declared input takes ([`UNBOUND`]) and a value too
/// long for the backlog's bound ([`ELIDED`]). A panic carries its `file:line`
/// and never its message.
///
/// On the wire it is one `context` entry, [`REPLAY_CONTEXT_KEY`], holding this
/// object's compact JSON: the backlog already stores context per sighting, so a
/// deployed backlog keeps it without a change, and a row decodes it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Replay {
    /// The `ds` version, as `ds version` prints it.
    pub version: String,
    /// The exact source commit the executable was built from, or `unknown`.
    pub source_sha: String,
    /// The build carried uncommitted changes, so the commit alone is not it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub dirty: bool,
    /// `stable`, `canary` or `local`.
    pub channel: String,
    pub os: String,
    pub arch: String,
    /// The dotted command id.
    pub command: String,
    /// Every word after `ds`, in order.
    pub argv: Vec<String>,
    /// Inputs whose values the descriptor marks sensitive and were withheld.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub redacted: Vec<String>,
    /// Inputs whose values were too long for the backlog and were withheld.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub elided: Vec<String>,
    /// Where a crash panicked, `file:line`; never the panic message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub panic_location: Option<String>,
}

/// The `context` key a [`Replay`] travels under. A caller's own `--context`
/// may not use it.
pub const REPLAY_CONTEXT_KEY: &str = "replay";
/// The longest encoded replay: the backlog's bound on one context value.
pub const REPLAY_MAX_CHARS: usize = 1_000;
/// The place of a value the descriptor marks sensitive.
pub const REDACTED: &str = "<redacted>";
/// The place of a value no declared input takes.
pub const UNBOUND: &str = "<unbound>";
/// The place of a value too long for [`REPLAY_MAX_CHARS`].
pub const ELIDED: &str = "<elided>";
const REPLAY_MAX_WORDS: usize = 256;
const REPLAY_MAX_NAMES: usize = 64;

impl Replay {
    /// The compact JSON that travels as the context value.
    pub fn encode(&self) -> String {
        serde_json::to_string(self).expect("a replay serializes")
    }

    /// Whether every field holds its shape and the encoding fits the bound.
    pub fn is_valid(&self) -> bool {
        let token = |s: &str, max: usize| {
            !s.is_empty()
                && s.len() <= max
                && s.chars().all(|c| {
                    c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '_' | '.' | '-')
                })
        };
        let names = |names: &[String]| {
            names.len() <= REPLAY_MAX_NAMES && names.iter().all(|name| text(name, 64))
        };
        text(&self.version, 64)
            && !self.version.contains(char::is_whitespace)
            // A build without its commit says so rather than guessing one.
            && (self.source_sha == "unknown"
                || ((7..=64).contains(&self.source_sha.len())
                    && self.source_sha.chars().all(|c| c.is_ascii_hexdigit())))
            && ["stable", "canary", "local"].contains(&self.channel.as_str())
            && token(&self.os, 32)
            && token(&self.arch, 32)
            && token(&self.command, 128)
            && self.argv.len() <= REPLAY_MAX_WORDS
            && names(&self.redacted)
            && names(&self.elided)
            && self
                .panic_location
                .as_deref()
                .is_none_or(|at| text(at, 300) && at.contains(':') && !at.contains('\n'))
            && self.encode().chars().count() <= REPLAY_MAX_CHARS
    }

    /// The replay a stored report's context carries, when it carries a valid
    /// one. A report filed before replays, or by hand, has none.
    pub fn from_context(context: &BTreeMap<String, String>) -> Option<Self> {
        let replay: Self = serde_json::from_str(context.get(REPLAY_CONTEXT_KEY)?).ok()?;
        replay.is_valid().then_some(replay)
    }
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
                replay,
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
                    // The replay travels as one more context entry, under a
                    // key a caller's own context may not take.
                    && context.len() + usize::from(replay.is_some()) <= 24
                    && !context.contains_key(REPLAY_CONTEXT_KEY)
                    && context.iter().all(|(k, v)| text(k, 500) && text(v, 1000))
                    && replay.as_ref().is_none_or(Replay::is_valid)
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
    /// The latest sighting's context. A row shows only the replay it carries.
    #[serde(default, skip_serializing)]
    context: BTreeMap<String, String>,
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
        // The replay is what closing a report needs, like the full detail,
        // so it rides on the same full read.
        let replay = full.then(|| Replay::from_context(&self.context)).flatten();
        let mut row = serde_json::to_value(self).expect("feedback row");
        row["detail_truncated"] = json!(clipped);
        if let Some(replay) = replay {
            row["replay"] = json!(replay);
        }
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
            // `drains` describes continuation, not consent to remember it.
            // Explicit cursors are echoed without use_watermark; only a plain
            // unfiltered sweep opts into the account's remembered position.
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
            replay,
        } => {
            let mut a = json!({"name":agent});
            if let Some(m) = model {
                a["model"] = json!(m)
            }
            if let Some(c) = client {
                a["client"] = json!(c)
            }
            let mut context = context.clone();
            if let Some(replay) = replay {
                context.insert(REPLAY_CONTEXT_KEY.to_string(), replay.encode());
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
#[path = "feedback_tests.rs"]
mod tests;
