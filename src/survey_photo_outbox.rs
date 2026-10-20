//! Durable survey-photo outbox decisions.
//!
//! The Kernel's `media_outbox` store holds the rows and their bytes and applies
//! these transitions; the `survey_media` drain carries the uploads. This module
//! owns the durable transition around one ordered, whole-plan attempt. The
//! transfer yields its retryable outcome back here; this outbox alone owns the
//! persisted ten-attempt budget.

use serde::{Deserialize, Serialize};

use crate::response_policy::{self, Classify, Outcome, RESPONSE_POLICY_SCHEMA};
use crate::transfer::{Cause, Phase};

pub const MAX_ATTEMPTS: u32 = 10;
pub const MAX_CONCURRENT_UPLOADS: u32 = 2;
pub const MAX_SUPERSEDED_REVISIONS: usize = 32;
pub const REVISION_COALESCE_MS: u64 = 500;
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
    /// Gateway-advertised revision protocol: the row's immutable revision,
    /// which the gateway may rebase over the named `supersedes` predecessors.
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
    /// The store has persisted the bytes and constructed this row.
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
    /// Retry after a surfaced failure without silently changing ownership.
    ManualRetry {
        scope: Scope,
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

#[derive(Debug, PartialEq)]
pub struct Reply {
    /// `None` settles the row: the store removes it.
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
    Reply { row: None, actions }
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
        Event::ManualRetry { scope } => {
            if same_scope(&row, &scope) && matches!(row.status, Status::Failed | Status::Blocked) {
                row.status = Status::Pending;
                row.attempts = 0;
                row.next_attempt_at = 0;
                row.last_error = None;
                row.updated_at = now;
            }
        }
    }
    Ok(Reply {
        row: Some(row),
        actions,
    })
}

#[cfg(test)]
#[path = "survey_photo_outbox_tests.rs"]
mod tests;
