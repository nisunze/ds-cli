//! Durable survey-photo outbox decisions.
//!
//! Photo bytes, IndexedDB, credentials, timers, and per-target upload IO belong
//! to the host. This module owns the durable transition around one ordered,
//! whole-plan attempt. The host's transfer driver yields its retryable outcome
//! back here; this outbox alone owns the persisted ten-attempt budget.

use serde::{Deserialize, Serialize};

use crate::response_policy::{self, Classify, Outcome, RESPONSE_POLICY_SCHEMA};
use crate::transfer::{Cause, Phase};

pub const SURVEY_PHOTO_OUTBOX_SCHEMA: &str = "ds.survey-photo-outbox/v1";
pub const MAX_ATTEMPTS: u32 = 10;
pub const MAX_CONCURRENT_UPLOADS: u32 = 2;
pub const MAX_SUPERSEDED_REVISIONS: usize = 32;
pub const REVISION_COALESCE_MS: u64 = 500;
const MAX_INPUT_BYTES: usize = 64 * 1024;
const MISSING_BYTES: &str = "Photo bytes are missing from local storage.";
const SCOPE_MISMATCH: &str = "Photo upload is fenced to a different account or project.";
const LEGACY_SCOPE: &str = "Photo upload needs its original account, project, and generation.";

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub account_id: String,
    pub project_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Uploading,
    Failed,
    /// The bytes remain pinned, but an operator/provenance action is required.
    Blocked,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub path: String,
    pub surface: String,
    pub owner_ref: String,
    pub content_type: String,
    pub size: u64,
    pub status: Status,
    pub attempts: u32,
    pub next_attempt_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    /// Added without changing legacy identity fields.  An absent scope is
    /// intentionally blocked until host evidence identifies its origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<Scope>,
    /// Incremented only when a request is dispatched; prevents a late result
    /// from an earlier request from settling a restashed/retried row.
    #[serde(default)]
    pub lease: u64,
    /// New UUID generated at stash time. It fences a late completion after a
    /// cancellation followed by a new stash at the same canonical path.
    #[serde(default)]
    pub generation: String,
    /// The remote object's conditional-write generation. This is an opaque
    /// transport fact: the kernel persists and returns it but never decides
    /// policy from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_generation: Option<String>,
    /// A locally generated JPEG thumbnail that travels with the original as
    /// one atomic upload plan. It is never an independently retried row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub companion: Option<Companion>,
    /// Gateway-advertised revision protocol. Only these rows may supersede a
    /// transfer whose storage commit could race cancellation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supersedes: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Companion {
    pub path: String,
    pub content_type: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_generation: Option<String>,
}

/// One ordered host IO target. The host must PUT and verify every target in
/// this order before reporting `TransportOutcome::Success` for the row.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct UploadTarget {
    pub path: String,
    pub content_type: String,
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_generation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supersedes: Vec<String>,
}

/// The canonical server-compatible thumbnail object name for an original
/// survey photo. Only the final filename extension is replaced.
pub fn thumbnail_path_for(path: &str) -> Result<String, String> {
    let (prefix, filename) = path
        .rsplit_once('/')
        .map_or(("", path), |(prefix, name)| (prefix, name));
    if filename.is_empty() || filename == "." || filename == ".." {
        return Err("Survey-photo original path needs a filename for its thumbnail.".into());
    }
    if filename.to_ascii_lowercase().ends_with("_thunder.jpeg") {
        return Ok(path.into());
    }
    let stem = filename
        .rfind('.')
        .filter(|index| *index > 0)
        .map_or(filename, |index| &filename[..index]);
    let thumbnail = format!("{stem}_thunder.jpeg");
    Ok(if prefix.is_empty() {
        thumbnail
    } else {
        format!("{prefix}/{thumbnail}")
    })
}

/// Produce the single ordered upload plan. The original is always first so a
/// thumbnail cannot become visible for a failed original replacement.
pub fn upload_plan(row: &Row) -> Result<Vec<UploadTarget>, String> {
    if let Some(companion) = &row.companion {
        if companion.path == row.path
            || companion.path != thumbnail_path_for(&row.path)?
            || companion.content_type != "image/jpeg"
            || companion.size == 0
            || !valid_expected_generation(companion.expected_generation.as_deref())
        {
            return Err("Survey-photo thumbnail companion is not a canonical JPEG target.".into());
        }
    }
    let mut targets = vec![UploadTarget {
        path: row.path.clone(),
        content_type: row.content_type.clone(),
        size: row.size,
        expected_generation: row.expected_generation.clone(),
        revision: row.revision.clone(),
        supersedes: row.supersedes.clone(),
    }];
    if let Some(companion) = &row.companion {
        targets.push(UploadTarget {
            path: companion.path.clone(),
            content_type: companion.content_type.clone(),
            size: companion.size,
            expected_generation: companion.expected_generation.clone(),
            revision: row.revision.clone(),
            supersedes: row.supersedes.clone(),
        });
    }
    Ok(targets)
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TransportOutcome {
    Success,
    Cancelled,
    TransportFailure {
        /// Browser transfer-host fact. The outbox maps this typed observation
        /// through response_policy; TypeScript must not recreate the table.
        #[serde(default)]
        cause: Option<Cause>,
        #[serde(default)]
        status: Option<u16>,
        #[serde(default)]
        error_code: Option<String>,
        #[serde(default)]
        timed_out: bool,
        #[serde(default)]
        retry_after: Option<String>,
        #[serde(default)]
        has_session: bool,
        #[serde(default)]
        offline_mode: bool,
        #[serde(default)]
        message: Option<String>,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    /// A host has already persisted the pinned bytes and constructed this row.
    Admit {
        scope: Scope,
    },
    /// Process-restart recovery.  It never spends an attempt because no
    /// outcome for the abandoned request was observed.
    Recover,
    /// Decide whether this one row may claim a slot. `active_uploads` is a
    /// host fact over the other leased rows, not a caller-selected limit.
    Drive {
        online: bool,
        active_uploads: u32,
        bytes_present: bool,
        scope: Scope,
    },
    Result {
        scope: Scope,
        generation: String,
        lease: u64,
        outcome: TransportOutcome,
    },
    Cancel {
        scope: Scope,
    },
    /// Replace the exact local byte revision, retaining its remote baseline.
    /// The host atomically swaps both pinned blobs before executing Abort.
    Supersede {
        scope: Scope,
        generation: String,
        content_type: String,
        size: u64,
        companion: Companion,
    },
    PrepareSupersede {
        scope: Scope,
    },
    /// Retry after a surfaced failure without silently changing ownership.
    ManualRetry {
        scope: Scope,
    },
    /// Only a migration that derived the original scope from durable row
    /// provenance may bind an older row.  Never infer it from the current user.
    BindLegacyScope {
        inferred_scope: Scope,
        generation: String,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Dispatch {
        path: String,
        content_type: String,
        generation: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        expected_generation: Option<String>,
        lease: u64,
        targets: Vec<UploadTarget>,
    },
    Abort {
        path: String,
        generation: String,
        lease: u64,
    },
    WakeAt {
        at: u64,
    },
    RemoveOutboxRow {
        path: String,
    },
    /// Unpinning permits cache eviction later; it does not delete cached bytes.
    UnpinBlob {
        path: String,
    },
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub now: u64,
    #[serde(default)]
    pub row: Option<Row>,
    pub event: Event,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct Reply {
    pub schema: &'static str,
    /// Always present on the wire. `null` directs the host to remove the
    /// durable row after it completes the ordered cleanup actions.
    pub row: Option<Row>,
    pub actions: Vec<Action>,
}

fn valid_scope(scope: &Scope) -> bool {
    !scope.account_id.trim().is_empty() && !scope.project_id.trim().is_empty()
}

fn valid_expected_generation(value: Option<&str>) -> bool {
    value.is_none_or(|value| {
        !value.is_empty()
            && value.bytes().all(|byte| byte.is_ascii_digit())
            && value.bytes().any(|byte| byte != b'0')
    })
}

fn valid_revision(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}

fn ensure_row(row: Option<Row>) -> Result<Row, String> {
    let row = row.ok_or_else(|| "Survey-photo outbox event needs a row.".to_string())?;
    if row.path.trim().is_empty()
        || row.content_type.trim().is_empty()
        || row.surface.trim().is_empty()
    {
        return Err("Survey-photo outbox row is missing required identity.".into());
    }
    if !valid_expected_generation(row.expected_generation.as_deref()) {
        return Err("Survey-photo expected generation must be a positive decimal string.".into());
    }
    if row
        .revision
        .as_ref()
        .is_some_and(|revision| revision != &row.generation || !valid_revision(revision))
        || row.supersedes.len() > MAX_SUPERSEDED_REVISIONS
        || (!row.supersedes.is_empty() && row.revision.is_none())
        || row.supersedes.iter().enumerate().any(|(index, revision)| {
            !valid_revision(revision)
                || row.revision.as_ref() == Some(revision)
                || row.supersedes[..index].contains(revision)
        })
    {
        return Err("Survey-photo revision chain is invalid.".into());
    }
    if let Some(companion) = &row.companion {
        if companion.path == row.path
            || companion.path != thumbnail_path_for(&row.path)?
            || companion.content_type != "image/jpeg"
            || companion.size == 0
            || !valid_expected_generation(companion.expected_generation.as_deref())
        {
            return Err("Survey-photo thumbnail companion is not a canonical JPEG target.".into());
        }
    }
    Ok(row)
}

fn same_scope(row: &Row, scope: &Scope) -> bool {
    row.scope.as_ref() == Some(scope)
}

fn block(row: &mut Row, message: &str, now: u64) {
    row.status = Status::Blocked;
    row.next_attempt_at = 0;
    row.last_error = Some(message.into());
    row.updated_at = now;
}

fn remove(row: Row) -> Reply {
    let companion_path = row
        .companion
        .as_ref()
        .map(|companion| companion.path.clone());
    let mut actions = vec![
        Action::RemoveOutboxRow {
            path: row.path.clone(),
        },
        Action::UnpinBlob { path: row.path },
    ];
    if let Some(path) = companion_path {
        actions.push(Action::UnpinBlob { path });
    }
    Reply {
        schema: SURVEY_PHOTO_OUTBOX_SCHEMA,
        row: None,
        actions,
    }
}

fn response_facts_from_cause(
    cause: Option<&Cause>,
    status: Option<u16>,
    timed_out: bool,
    error_code: Option<String>,
    offline_mode: bool,
) -> (Option<u16>, bool, Option<String>, bool) {
    let Some(cause) = cause else {
        return (status, timed_out, error_code, offline_mode);
    };
    match cause {
        Cause::HttpStatus { status } => (Some(*status), timed_out, error_code, offline_mode),
        Cause::Timeout {
            phase: Phase::Connect | Phase::Request | Phase::Response | Phase::Idle,
        } => (
            status,
            true,
            error_code.or_else(|| Some("NETWORK_ERROR".into())),
            offline_mode,
        ),
        Cause::Offline => (
            status,
            timed_out,
            error_code.or_else(|| Some("NETWORK_OFFLINE".into())),
            true,
        ),
        Cause::Cancelled => (status, timed_out, error_code, offline_mode),
        Cause::DigestMismatch | Cause::LengthMismatch => {
            (Some(412), timed_out, error_code, offline_mode)
        }
        Cause::Dns
        | Cause::Connect
        | Cause::Tls
        | Cause::ConnectionClosed
        | Cause::Truncated
        | Cause::RangeMalformed
        | Cause::SessionExpired
        | Cause::BlockedByHostPolicy
        | Cause::Inconclusive => (
            status,
            timed_out,
            error_code.or_else(|| Some("NETWORK_ERROR".into())),
            offline_mode,
        ),
    }
}

pub fn apply(now: u64, row: Option<Row>, event: Event) -> Result<Reply, String> {
    let mut actions = Vec::new();
    let mut row = ensure_row(row)?;

    match event {
        Event::Admit { scope } => {
            if !valid_scope(&scope) {
                return Err("Survey-photo scope is incomplete.".into());
            }
            if row.generation.trim().is_empty() {
                return Err("Survey-photo generation is incomplete.".into());
            }
            row.scope = Some(scope);
            row.status = Status::Pending;
            row.attempts = 0;
            row.next_attempt_at = 0;
            row.last_error = None;
            row.lease = 0;
            row.updated_at = now;
            if row.revision.is_some() {
                row.next_attempt_at = now.saturating_add(REVISION_COALESCE_MS);
                actions.push(Action::WakeAt {
                    at: row.next_attempt_at,
                });
            }
        }
        Event::Recover => {
            if row.status == Status::Uploading {
                row.status = Status::Pending;
                row.next_attempt_at = now;
                row.updated_at = now;
            }
        }
        Event::Drive {
            online,
            active_uploads,
            bytes_present,
            scope,
        } => {
            if !valid_scope(&scope) {
                return Err("Survey-photo scope is incomplete.".into());
            }
            if row.scope.is_none() || row.generation.trim().is_empty() {
                block(&mut row, LEGACY_SCOPE, now);
            } else if !same_scope(&row, &scope) {
                block(&mut row, SCOPE_MISMATCH, now);
            } else if !bytes_present {
                row.status = Status::Failed;
                row.next_attempt_at = 0;
                row.last_error = Some(MISSING_BYTES.into());
                row.updated_at = now;
            } else if online
                && row.status == Status::Pending
                && row.attempts < MAX_ATTEMPTS
                && row.next_attempt_at <= now
                && active_uploads < MAX_CONCURRENT_UPLOADS
            {
                row.status = Status::Uploading;
                row.attempts = row.attempts.saturating_add(1);
                row.lease = row.lease.saturating_add(1);
                row.updated_at = now;
                actions.push(Action::Dispatch {
                    path: row.path.clone(),
                    content_type: row.content_type.clone(),
                    generation: row.generation.clone(),
                    expected_generation: row.expected_generation.clone(),
                    lease: row.lease,
                    targets: upload_plan(&row)?,
                });
            } else if row.status == Status::Pending && row.next_attempt_at > now {
                actions.push(Action::WakeAt {
                    at: row.next_attempt_at,
                });
            }
        }
        Event::Result {
            scope,
            generation,
            lease,
            outcome,
        } => {
            // A different account/project, a cancelled request, or an older
            // request cannot settle this durable row.
            if !same_scope(&row, &scope)
                || row.generation != generation
                || row.status != Status::Uploading
                || row.lease != lease
            {
                return Ok(Reply {
                    schema: SURVEY_PHOTO_OUTBOX_SCHEMA,
                    row: Some(row),
                    actions,
                });
            }
            match outcome {
                TransportOutcome::Success => return Ok(remove(row)),
                TransportOutcome::Cancelled => {
                    row.status = Status::Pending;
                    row.next_attempt_at = now;
                    row.last_error = None;
                    row.updated_at = now;
                }
                TransportOutcome::TransportFailure {
                    cause,
                    status,
                    error_code,
                    timed_out,
                    retry_after,
                    has_session,
                    offline_mode,
                    message,
                } => {
                    let supplied_message = message.clone();
                    let (mut status, timed_out, mut error_code, offline_mode) =
                        response_facts_from_cause(
                            cause.as_ref(),
                            status,
                            timed_out,
                            error_code,
                            offline_mode,
                        );
                    // A revision-aware conditional PUT may race the preceding
                    // cancelled commit. Retry through START, whose gateway CAS
                    // rebases only over the proved predecessor. A foreign cloud
                    // edit is still a terminal 409 from START. Integrity errors
                    // are never retried by this storage-precondition rule.
                    if row.revision.is_some()
                        && matches!(cause, Some(Cause::HttpStatus { status: 412, .. }))
                    {
                        status = None;
                        error_code = Some("NETWORK_ERROR".into());
                    }
                    let decision = response_policy::classify(Classify {
                        schema: RESPONSE_POLICY_SCHEMA.into(),
                        kind: "classify".into(),
                        status,
                        error_code,
                        timed_out,
                        attempt: row.attempts.saturating_sub(1),
                        retries_allowed: MAX_ATTEMPTS - 1,
                        retry_after,
                        now_ms: now,
                        has_session,
                        offline_mode,
                        message,
                    })?;
                    let message = decision
                        .public_message
                        .map(str::to_string)
                        .or(supplied_message)
                        .or_else(|| row.last_error.clone())
                        .unwrap_or_else(|| "Photo upload failed.".into());
                    if decision.decision == Outcome::Retry
                        && decision.retryable
                        && row.attempts < MAX_ATTEMPTS
                    {
                        row.status = Status::Pending;
                        row.next_attempt_at = now.saturating_add(decision.retry_delay_ms);
                        row.last_error = Some(message);
                        row.updated_at = now;
                        actions.push(Action::WakeAt {
                            at: row.next_attempt_at,
                        });
                    } else {
                        row.status = Status::Failed;
                        row.next_attempt_at = 0;
                        row.last_error = Some(message);
                        row.updated_at = now;
                    }
                }
            }
        }
        Event::Cancel { scope } => {
            if same_scope(&row, &scope) {
                if row.status == Status::Uploading {
                    actions.push(Action::Abort {
                        path: row.path.clone(),
                        generation: row.generation.clone(),
                        lease: row.lease,
                    });
                }
                let mut reply = remove(row);
                reply.actions.splice(0..0, actions);
                return Ok(reply);
            }
        }
        Event::PrepareSupersede { scope } => {
            validate_supersede(&row, &scope)?;
        }
        Event::Supersede {
            scope,
            generation,
            content_type,
            size,
            mut companion,
        } => {
            validate_supersede(&row, &scope)?;
            if !same_scope(&row, &scope) {
                return Err(SCOPE_MISMATCH.into());
            }
            let revision = row.revision.clone().ok_or_else(|| {
                "This pending photo needs the revision-aware gateway before another rotation."
                    .to_string()
            })?;
            if !matches!(row.status, Status::Pending | Status::Uploading)
                || !valid_revision(&generation)
                || generation == row.generation
                || row.supersedes.contains(&generation)
                || row.supersedes.len() >= MAX_SUPERSEDED_REVISIONS
                || !matches!(content_type.as_str(), "image/jpeg" | "image/png")
                || size == 0
                || row.companion.as_ref().map(|old| &old.path) != Some(&companion.path)
            {
                return Err("Photo changed or cannot accept another unpublished rotation.".into());
            }
            // A later rotation changes bytes, never the storage baseline it
            // observed. Only the gateway may rebase over a proved predecessor.
            companion.expected_generation = row
                .companion
                .as_ref()
                .and_then(|old| old.expected_generation.clone());
            if row.status == Status::Uploading {
                actions.push(Action::Abort {
                    path: row.path.clone(),
                    generation: row.generation.clone(),
                    lease: row.lease,
                });
            }
            row.supersedes.push(revision);
            row.generation = generation.clone();
            row.revision = Some(generation);
            row.content_type = content_type;
            row.size = size;
            row.companion = Some(companion);
            row.status = Status::Pending;
            row.attempts = 0;
            row.lease = 0;
            row.last_error = None;
            row.updated_at = now;
            row.next_attempt_at = now.saturating_add(REVISION_COALESCE_MS);
            ensure_row(Some(row.clone()))?;
            actions.push(Action::WakeAt {
                at: row.next_attempt_at,
            });
        }
        Event::ManualRetry { scope } => {
            if same_scope(&row, &scope) && matches!(row.status, Status::Failed | Status::Blocked) {
                row.status = Status::Pending;
                row.attempts = 0;
                row.next_attempt_at = 0;
                row.last_error = None;
                row.updated_at = now;
            }
        }
        Event::BindLegacyScope {
            inferred_scope,
            generation,
        } => {
            if !valid_scope(&inferred_scope) {
                return Err("Survey-photo scope is incomplete.".into());
            }
            if generation.trim().is_empty() {
                return Err("Survey-photo generation is incomplete.".into());
            }
            if row.scope.is_none() || row.generation.trim().is_empty() {
                row.scope = Some(inferred_scope);
                row.generation = generation;
                row.status = Status::Pending;
                row.next_attempt_at = 0;
                row.last_error = None;
                row.updated_at = now;
            }
        }
    }
    Ok(Reply {
        schema: SURVEY_PHOTO_OUTBOX_SCHEMA,
        row: Some(row),
        actions,
    })
}

fn validate_supersede(row: &Row, scope: &Scope) -> Result<(), String> {
    if !same_scope(row, scope) {
        return Err(SCOPE_MISMATCH.into());
    }
    if row.revision.is_none() {
        return Err(
            "This pending photo needs the revision-aware gateway before another rotation.".into(),
        );
    }
    if !matches!(row.status, Status::Pending | Status::Uploading)
        || row.companion.is_none()
        || row.supersedes.len() >= MAX_SUPERSEDED_REVISIONS
    {
        return Err("Photo changed or cannot accept another unpublished rotation.".into());
    }
    Ok(())
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.is_empty() || input.len() > MAX_INPUT_BYTES {
        return Err("Survey-photo outbox command exceeds its byte bound.".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid survey-photo outbox command: {error}"))?;
    if request.schema != SURVEY_PHOTO_OUTBOX_SCHEMA {
        return Err("Unsupported survey-photo outbox command schema.".into());
    }
    serde_json::to_string(&apply(request.now, request.row, request.event)?)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Scope {
        Scope {
            account_id: "account-a".into(),
            project_id: "project-a".into(),
        }
    }
    fn row() -> Row {
        Row {
            path: "survey/a.jpg".into(),
            surface: "survey-media".into(),
            owner_ref: "entry-a".into(),
            content_type: "image/jpeg".into(),
            size: 4,
            status: Status::Pending,
            attempts: 0,
            next_attempt_at: 0,
            last_error: None,
            created_at: 1,
            updated_at: 1,
            scope: Some(scope()),
            lease: 0,
            generation: "stash-a".into(),
            expected_generation: Some("123456".into()),
            companion: None,
            revision: None,
            supersedes: Vec::new(),
        }
    }
    #[test]
    fn supersede_retains_remote_baseline_aborts_old_lease_and_fences_late_success() {
        let mut previous = row();
        previous.path = "project/forms/form/entry/photo.jpg".into();
        previous.revision = Some(previous.generation.clone());
        previous.companion = Some(Companion {
            path: thumbnail_path_for(&previous.path).unwrap(),
            content_type: "image/jpeg".into(),
            size: 3,
            expected_generation: Some("123457".into()),
        });
        let previous = drive(previous, 1000).row.unwrap();
        let companion = previous.companion.clone().unwrap();
        let changed = apply(
            1001,
            Some(previous.clone()),
            Event::Supersede {
                scope: scope(),
                generation: "stash-b".into(),
                content_type: "image/png".into(),
                size: 5,
                companion: Companion {
                    expected_generation: Some("999".into()),
                    ..companion
                },
            },
        )
        .unwrap();
        assert!(changed.actions.iter().any(
            |action| matches!(action, Action::Abort { generation, .. } if generation == "stash-a")
        ));
        let current = changed.row.unwrap();
        assert_eq!(current.expected_generation, previous.expected_generation);
        assert_eq!(
            current
                .companion
                .as_ref()
                .unwrap()
                .expected_generation
                .as_deref(),
            Some("123457")
        );
        assert_eq!(current.supersedes, vec!["stash-a"]);
        assert_eq!(current.next_attempt_at, 1501);
        let late = apply(
            1002,
            Some(current.clone()),
            Event::Result {
                scope: scope(),
                generation: previous.generation,
                lease: previous.lease,
                outcome: TransportOutcome::Success,
            },
        )
        .unwrap();
        assert_eq!(late.row, Some(current));
        assert!(late.actions.is_empty());
    }

    #[test]
    fn supersede_preparation_refuses_old_gateway_and_foreign_scope() {
        assert!(apply(1, Some(row()), Event::PrepareSupersede { scope: scope() }).is_err());
        let mut r = row();
        r.revision = Some(r.generation.clone());
        let mut other = scope();
        other.account_id = "other-account".into();
        assert!(apply(1, Some(r), Event::PrepareSupersede { scope: other })
            .unwrap_err()
            .contains("different account"));
    }
    fn drive(row: Row, now: u64) -> Reply {
        apply(
            now,
            Some(row),
            Event::Drive {
                online: true,
                active_uploads: 0,
                bytes_present: true,
                scope: scope(),
            },
        )
        .unwrap()
    }

    #[test]
    fn dispatch_is_bounded_and_leased() {
        let reply = drive(row(), 10);
        assert_eq!(reply.row.as_ref().unwrap().status, Status::Uploading);
        assert_eq!(reply.row.as_ref().unwrap().attempts, 1);
        assert_eq!(
            reply.actions,
            vec![Action::Dispatch {
                path: "survey/a.jpg".into(),
                content_type: "image/jpeg".into(),
                generation: "stash-a".into(),
                expected_generation: Some("123456".into()),
                lease: 1,
                targets: vec![UploadTarget {
                    path: "survey/a.jpg".into(),
                    content_type: "image/jpeg".into(),
                    size: 4,
                    expected_generation: Some("123456".into()),
                    revision: None,
                    supersedes: Vec::new(),
                }],
            }]
        );
        let capped = apply(
            10,
            Some(row()),
            Event::Drive {
                online: true,
                active_uploads: MAX_CONCURRENT_UPLOADS,
                bytes_present: true,
                scope: scope(),
            },
        )
        .unwrap();
        assert!(capped.actions.is_empty());
    }

    #[test]
    fn companion_has_one_canonical_ordered_plan_and_cleanup_unpins_both_bytes() {
        let mut state = row();
        state.companion = Some(Companion {
            path: "survey/a_thunder.jpeg".into(),
            content_type: "image/jpeg".into(),
            size: 3,
            expected_generation: None,
        });
        let started = drive(state, 10);
        let Action::Dispatch { targets, .. } = &started.actions[0] else {
            panic!("dispatch expected")
        };
        assert_eq!(
            targets
                .iter()
                .map(|target| target.path.as_str())
                .collect::<Vec<_>>(),
            vec!["survey/a.jpg", "survey/a_thunder.jpeg"]
        );
        let removed = apply(
            11,
            started.row,
            Event::Result {
                scope: scope(),
                generation: "stash-a".into(),
                lease: 1,
                outcome: TransportOutcome::Success,
            },
        )
        .unwrap();
        assert!(removed.row.is_none());
        assert_eq!(
            removed
                .actions
                .iter()
                .filter(|action| matches!(action, Action::UnpinBlob { .. }))
                .count(),
            2
        );
    }

    #[test]
    fn invalid_companion_cannot_admit_or_create_an_upload_plan() {
        let mut state = row();
        state.companion = Some(Companion {
            path: "survey/a-thumb.jpg".into(),
            content_type: "image/png".into(),
            size: 0,
            expected_generation: None,
        });
        assert!(apply(1, Some(state.clone()), Event::Recover).is_err());
        assert!(upload_plan(&state).is_err());
        assert_eq!(
            thumbnail_path_for("folder/field.capture.png").unwrap(),
            "folder/field.capture_thunder.jpeg"
        );
        assert_eq!(
            thumbnail_path_for("folder/field_thunder.JPEG").unwrap(),
            "folder/field_thunder.JPEG"
        );
        let mut same_path = row();
        same_path.companion = Some(Companion {
            path: same_path.path.clone(),
            content_type: "image/jpeg".into(),
            size: 3,
            expected_generation: None,
        });
        assert!(apply(1, Some(same_path), Event::Recover).is_err());
    }

    #[test]
    fn restart_recovers_without_spending_an_attempt() {
        let mut state = row();
        state.status = Status::Uploading;
        state.attempts = 3;
        state.lease = 9;
        let reply = apply(50, Some(state), Event::Recover).unwrap();
        let recovered = reply.row.unwrap();
        assert_eq!(recovered.status, Status::Pending);
        assert_eq!(recovered.attempts, 3);
    }

    #[test]
    fn recovered_tenth_attempt_never_dispatches_an_eleventh() {
        let mut state = row();
        state.status = Status::Uploading;
        state.attempts = MAX_ATTEMPTS;
        let recovered = apply(50, Some(state), Event::Recover).unwrap().row.unwrap();
        let reply = drive(recovered, 51);
        assert!(reply.actions.is_empty());
        assert_eq!(reply.row.unwrap().status, Status::Pending);
    }

    #[test]
    fn expected_generation_is_opaque_but_must_be_positive_decimal() {
        let mut invalid = row();
        invalid.expected_generation = Some("not-a-generation".into());
        assert!(apply(1, Some(invalid), Event::Recover).is_err());
        let mut zero = row();
        zero.expected_generation = Some("0".into());
        assert!(apply(1, Some(zero), Event::Recover).is_err());
    }

    #[test]
    fn transient_failure_uses_response_policy_and_deadline() {
        let started = drive(row(), 100).row.unwrap();
        let reply = apply(
            100,
            Some(started),
            Event::Result {
                scope: scope(),
                generation: "stash-a".into(),
                lease: 1,
                outcome: TransportOutcome::TransportFailure {
                    cause: None,
                    status: Some(503),
                    error_code: None,
                    timed_out: false,
                    retry_after: None,
                    has_session: true,
                    offline_mode: false,
                    message: Some("unavailable".into()),
                },
            },
        )
        .unwrap();
        let state = reply.row.unwrap();
        assert_eq!(state.status, Status::Pending);
        assert_eq!(state.next_attempt_at, 1_900);
        assert_eq!(reply.actions, vec![Action::WakeAt { at: 1_900 }]);
    }

    #[test]
    fn tenth_failure_is_terminal_and_manual_retry_resets_budget() {
        let mut state = row();
        state.status = Status::Uploading;
        state.attempts = 10;
        state.lease = 4;
        let failed = apply(
            1,
            Some(state),
            Event::Result {
                scope: scope(),
                generation: "stash-a".into(),
                lease: 4,
                outcome: TransportOutcome::TransportFailure {
                    cause: None,
                    status: Some(503),
                    error_code: None,
                    timed_out: false,
                    retry_after: None,
                    has_session: true,
                    offline_mode: false,
                    message: None,
                },
            },
        )
        .unwrap()
        .row
        .unwrap();
        assert_eq!(failed.status, Status::Failed);
        let retried = apply(2, Some(failed), Event::ManualRetry { scope: scope() })
            .unwrap()
            .row
            .unwrap();
        assert_eq!((retried.status, retried.attempts), (Status::Pending, 0));
    }

    #[test]
    fn superseding_storage_precondition_retries_but_corrupt_bytes_do_not() {
        for (cause, expected) in [
            (Cause::HttpStatus { status: 412 }, Status::Pending),
            (Cause::DigestMismatch, Status::Failed),
            (Cause::LengthMismatch, Status::Failed),
            (Cause::HttpStatus { status: 409 }, Status::Failed),
        ] {
            let mut started = drive(row(), 0).row.unwrap();
            started.revision = Some(started.generation.clone());
            let result = apply(
                0,
                Some(started),
                Event::Result {
                    scope: scope(),
                    generation: "stash-a".into(),
                    lease: 1,
                    outcome: TransportOutcome::TransportFailure {
                        cause: Some(cause),
                        status: None,
                        error_code: None,
                        timed_out: false,
                        retry_after: None,
                        has_session: true,
                        offline_mode: false,
                        message: None,
                    },
                },
            )
            .unwrap()
            .row
            .unwrap();
            assert_eq!(result.status, expected);
        }
    }

    #[test]
    fn inconclusive_308_is_normalized_by_the_transport_and_412_is_terminal() {
        let started = drive(row(), 0).row.unwrap();
        let retry = apply(
            0,
            Some(started.clone()),
            Event::Result {
                scope: scope(),
                generation: "stash-a".into(),
                lease: 1,
                // The governed transport reports an incomplete 308 as this
                // no-status fact; response_policy then owns its retry timing.
                outcome: TransportOutcome::TransportFailure {
                    cause: None,
                    status: None,
                    error_code: Some("UPLOAD_INCONCLUSIVE".into()),
                    timed_out: false,
                    retry_after: None,
                    has_session: true,
                    offline_mode: false,
                    message: None,
                },
            },
        )
        .unwrap()
        .row
        .unwrap();
        assert_eq!(retry.status, Status::Pending);
        let conflict = apply(
            0,
            Some(started),
            Event::Result {
                scope: scope(),
                generation: "stash-a".into(),
                lease: 1,
                outcome: TransportOutcome::TransportFailure {
                    cause: None,
                    status: Some(412),
                    error_code: None,
                    timed_out: false,
                    retry_after: None,
                    has_session: true,
                    offline_mode: false,
                    message: None,
                },
            },
        )
        .unwrap()
        .row
        .unwrap();
        assert_eq!(conflict.status, Status::Failed);
    }

    #[test]
    fn typed_transfer_causes_share_response_policy_retry_and_terminal_rules() {
        for cause in [
            Cause::HttpStatus { status: 503 },
            Cause::Timeout {
                phase: Phase::Response,
            },
            Cause::SessionExpired,
            Cause::BlockedByHostPolicy,
        ] {
            let started = drive(row(), 0).row.unwrap();
            let reply = apply(
                0,
                Some(started),
                Event::Result {
                    scope: scope(),
                    generation: "stash-a".into(),
                    lease: 1,
                    outcome: TransportOutcome::TransportFailure {
                        cause: Some(cause),
                        status: None,
                        error_code: None,
                        timed_out: false,
                        retry_after: None,
                        has_session: true,
                        offline_mode: false,
                        message: None,
                    },
                },
            )
            .unwrap();
            assert_eq!(reply.row.unwrap().status, Status::Pending);
        }
        let started = drive(row(), 0).row.unwrap();
        let reply = apply(
            0,
            Some(started),
            Event::Result {
                scope: scope(),
                generation: "stash-a".into(),
                lease: 1,
                outcome: TransportOutcome::TransportFailure {
                    cause: Some(Cause::DigestMismatch),
                    status: None,
                    error_code: None,
                    timed_out: false,
                    retry_after: None,
                    has_session: true,
                    offline_mode: false,
                    message: None,
                },
            },
        )
        .unwrap();
        assert_eq!(reply.row.unwrap().status, Status::Failed);
    }

    #[test]
    fn missing_bytes_and_scope_change_never_dispatch() {
        let missing = apply(
            1,
            Some(row()),
            Event::Drive {
                online: true,
                active_uploads: 0,
                bytes_present: false,
                scope: scope(),
            },
        )
        .unwrap()
        .row
        .unwrap();
        assert_eq!(missing.status, Status::Failed);
        let other = Scope {
            account_id: "account-b".into(),
            project_id: "project-a".into(),
        };
        let blocked = apply(
            2,
            Some(row()),
            Event::Drive {
                online: true,
                active_uploads: 0,
                bytes_present: true,
                scope: other,
            },
        )
        .unwrap()
        .row
        .unwrap();
        assert_eq!(blocked.status, Status::Blocked);
    }

    #[test]
    fn cancellation_and_success_remove_row_but_never_cached_bytes() {
        let cancelled = apply(1, Some(row()), Event::Cancel { scope: scope() }).unwrap();
        assert!(cancelled.row.is_none());
        assert!(cancelled
            .actions
            .iter()
            .any(|a| matches!(a, Action::UnpinBlob { .. })));
        assert!(!serde_json::to_string(&cancelled)
            .unwrap()
            .contains("delete"));
        let started = drive(row(), 1).row.unwrap();
        let done = apply(
            2,
            Some(started),
            Event::Result {
                scope: scope(),
                generation: "stash-a".into(),
                lease: 1,
                outcome: TransportOutcome::Success,
            },
        )
        .unwrap();
        assert!(done.row.is_none());
    }

    #[test]
    fn settled_reply_carries_explicit_null_row_on_the_wire() {
        let started = drive(row(), 1).row.unwrap();
        let reply = apply(
            2,
            Some(started),
            Event::Result {
                scope: scope(),
                generation: "stash-a".into(),
                lease: 1,
                outcome: TransportOutcome::Success,
            },
        )
        .unwrap();
        let wire = serde_json::to_value(reply).unwrap();
        assert!(wire.get("row").unwrap().is_null());
    }

    #[test]
    fn late_completion_cannot_settle_a_new_lease() {
        let mut state = row();
        state.status = Status::Uploading;
        state.lease = 2;
        let reply = apply(
            1,
            Some(state),
            Event::Result {
                scope: scope(),
                generation: "stash-a".into(),
                lease: 1,
                outcome: TransportOutcome::Success,
            },
        )
        .unwrap();
        assert!(reply.row.is_some());
        assert!(reply.actions.is_empty());
    }

    #[test]
    fn unscoped_legacy_row_requires_explicit_provenance_binding() {
        let mut state = row();
        state.scope = None;
        let blocked = drive(state, 1).row.unwrap();
        assert_eq!(blocked.status, Status::Blocked);
        let rebound = apply(
            2,
            Some(blocked),
            Event::BindLegacyScope {
                inferred_scope: scope(),
                generation: "migrated-a".into(),
            },
        )
        .unwrap()
        .row
        .unwrap();
        assert_eq!(rebound.status, Status::Pending);
    }
}
