//! Fenced, explicit recovery over the common publication store. No IO or engine branch.
use super::{ArtifactRow, ArtifactState, Fence, LeaseRow, ReceiptRow, Refusal, RowChange, Scope};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "ds.sync-recovery/v1";
pub const MAX_ROWS: usize = 4096;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub row: ArtifactRow,
    pub standing: bool,
    pub last_attempt: Option<ReceiptRow>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Status { limit: usize },
    VerifyArtifacts { row: String },
    Retry { row: String },
    SanitizePreview { limit: usize },
    SanitizeApply { digest: String },
}

/// The public request every host receives. A host injects its owner fence and durable facts.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub schema: String,
    pub project: String,
    pub command: Command,
}

impl Control {
    pub fn validate(&self) -> std::result::Result<(), String> {
        if self.schema != SCHEMA || !crate::execution_context::valid_project(&self.project) {
            return Err("Recovery requires its declared schema and one explicit project".into());
        }
        match &self.command {
            Command::Status { limit } | Command::SanitizePreview { limit }
                if !(1..=200).contains(limit) =>
            {
                Err("Recovery display limit must be 1..200".into())
            }
            Command::Retry { row } | Command::VerifyArtifacts { row }
                if row.is_empty()
                    || row.len() > 512
                    || row
                        .chars()
                        .any(|c| c.is_control() || c.is_whitespace() || c == '/' || c == '\\') =>
            {
                Err("Recovery requires the exact bounded row identity".into())
            }
            Command::SanitizeApply { digest } if !sha(digest) => {
                Err("Recovery requires the exact preview SHA-256".into())
            }
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub fence: Fence,
    pub scope: Scope,
    pub now_ms: u64,
    pub entries: Vec<Entry>,
    pub lease: Option<LeaseRow>,
    pub command: Command,
}

#[derive(Debug, Serialize)]
pub struct Decision {
    pub id: String,
    pub identity: crate::sync::Identity,
    pub state: ArtifactState,
    pub archived: bool,
    pub standing: bool,
    pub sha256: String,
    pub input_base_fingerprint: Option<String>,
    pub engine_release: String,
    pub engine_build_manifest_sha256: Option<String>,
    pub failure_code: Option<String>,
    pub retry_eligible: bool,
    pub action: &'static str,
    pub reason: &'static str,
}

/// Native verification reads one sealed SQLite payload, never publication authority.
#[derive(Debug, Serialize)]
pub struct ArtifactVerification {
    pub row: String,
    pub status: &'static str,
    pub output_count: usize,
    pub verified_bytes: u64,
    pub outputs: Vec<VerifiedOutput>,
}
#[derive(Debug, Serialize)]
pub struct VerifiedOutput {
    pub output_id: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct Result {
    pub schema: &'static str,
    pub project: String,
    pub digest: String,
    pub total: usize,
    pub more: bool,
    pub rows: Vec<Decision>,
    pub refusals: Vec<Refusal>,
    pub applied: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_verification: Option<ArtifactVerification>,
    #[serde(skip)]
    pub changes: Vec<RowChange>,
}

fn sha(value: &str) -> bool {
    crate::report_export::exact_lower_sha256(value)
}

/// Exact authority evidence may retain bytes for explicit recovery; prose alone never does.
pub fn retry_eligible(row: &ArtifactRow, attempt: Option<&ReceiptRow>) -> bool {
    let Some(attempt) = attempt else {
        return false;
    };
    !row.archived
        && matches!(row.state, ArtifactState::Conflict | ArtifactState::Refused)
        && row.state_reason.as_deref() != Some("cancelled")
        && row.readable
        && !row.bytes_locator.is_empty()
        && !row.outputs.is_empty()
        && row.outputs.len() <= 100
        && row
            .outputs
            .iter()
            .all(|output| sha(&output.sha256) && output.size_bytes > 0)
        && row.sha256
            == super::inventory_digest(
                &row.outputs
                    .iter()
                    .map(|output| (output.output_id.clone(), output.sha256.clone()))
                    .collect::<Vec<_>>(),
            )
        && row.engine_build_manifest_sha256.as_deref().is_some_and(sha)
        && row.publication_input_base().is_some_and(sha)
        && crate::sync::engines::engine(&row.identity.engine)
            .is_some_and(|engine| engine.valid_producer_release(&row.engine_release))
        && attempt.scope == row.scope
        && attempt.identity.as_ref() == Some(&row.identity)
        && attempt.client_publish_id.as_deref() == Some(row.client_publish_id.as_str())
        && attempt.action == "upload"
        && ((attempt.outcome == "refused"
            && attempt.failure_code.as_deref() == Some("ENGINE_BUILD_NOT_ADMITTED"))
            || (attempt.outcome == "conflict"
                && attempt.failure_code.as_deref() == Some("CONFLICT")
                && attempt.detail.as_deref()
                    == Some("compute artifact conflict: stored work changed before finalize")))
}

/// The same policy used before reclaim and after restart; retaining bytes never retries.
pub fn retain_failure_bytes(row: &ArtifactRow, attempt: &ReceiptRow) -> bool {
    if row.state_reason.as_deref() == Some("cancelled") {
        return false;
    }
    let mut settled = row.clone();
    settled.state = match attempt.outcome.as_str() {
        "refused" => ArtifactState::Refused,
        "conflict" => ArtifactState::Conflict,
        _ => return false,
    };
    retry_eligible(&settled, Some(attempt))
}

pub fn resolve(mut request: Request) -> std::result::Result<Result, String> {
    if request.schema != SCHEMA
        || request.fence.account.is_empty()
        || request.fence.deployment.is_empty()
        || request.fence.install_id.is_empty()
        || !crate::execution_context::valid_project(request.scope.project())
        || request.entries.len() > MAX_ROWS
    {
        return Err(
            "Recovery requires one complete owner fence, project and at most 4096 rows".into(),
        );
    }
    request
        .entries
        .sort_by(|a, b| a.row.replay_key.cmp(&b.row.replay_key));
    let mut keys = BTreeSet::new();
    let mut standing = BTreeMap::new();
    for entry in &request.entries {
        if entry.row.scope != request.scope
            || entry.row.replay_key.is_empty()
            || !keys.insert(entry.row.replay_key.as_str())
        {
            return Err("Recovery row scope or identity is invalid".into());
        }
        if entry.standing && standing.insert(&entry.row.identity, &entry.row).is_some() {
            return Err("Recovery has multiple standing rows for one identity".into());
        }
    }
    let mut rows = Vec::new();
    for entry in &request.entries {
        let row = &entry.row;
        let eligible = entry.standing && retry_eligible(row, entry.last_attempt.as_ref());
        let superseded = !entry.standing
            && !row.outputs.is_empty()
            && standing.get(&row.identity).is_some_and(|newer| {
                !newer.archived
                    && newer.readable
                    && !newer.outputs.is_empty()
                    && row.outputs.iter().all(|old| {
                        newer
                            .outputs
                            .iter()
                            .any(|output| output.output_id == old.output_id)
                    })
            });
        let (action, reason) = if row.archived {
            ("keep", "already_archived")
        } else if crate::sync::engines::engine(&row.identity.engine).is_none() {
            ("keep", "unregistered_engine")
        } else if matches!(row.state, ArtifactState::Held | ArtifactState::Uploading) {
            ("keep", "active_publication")
        } else if row.state == ArtifactState::Published {
            ("archive", "published_history")
        } else if superseded {
            ("archive", "superseded_inventory")
        } else if eligible {
            ("retry", "recoverable_failure")
        } else {
            ("keep", "requires_attention")
        };
        rows.push(Decision {
            id: row.replay_key.clone(),
            identity: row.identity.clone(),
            state: row.state,
            archived: row.archived,
            standing: entry.standing,
            sha256: row.sha256.clone(),
            input_base_fingerprint: row.input_base_fingerprint.clone(),
            engine_release: row.engine_release.clone(),
            engine_build_manifest_sha256: row.engine_build_manifest_sha256.clone(),
            failure_code: entry
                .last_attempt
                .as_ref()
                .and_then(|r| r.failure_code.clone()),
            retry_eligible: eligible,
            action,
            reason,
        });
    }
    let lease_held = request
        .lease
        .as_ref()
        .is_some_and(|lease| lease.scope == request.scope && lease.expires_at_ms > request.now_ms);
    let digest = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&(
                &request.fence,
                &request.scope,
                &request.entries,
                &request.lease,
                &rows
            ))
            .map_err(|e| e.to_string())?
        )
    );
    let mut result = Result {
        schema: SCHEMA,
        project: request.scope.project().into(),
        digest,
        total: rows.len(),
        more: false,
        rows,
        refusals: Vec::new(),
        applied: false,
        artifact_verification: None,
        changes: Vec::new(),
    };
    let limit = match request.command {
        Command::Status { limit } | Command::SanitizePreview { limit } => {
            if !(1..=200).contains(&limit) {
                return Err("Recovery display limit must be 1..200".into());
            }
            limit
        }
        Command::VerifyArtifacts { row } => {
            result
                .rows
                .retain(|entry| entry.id == row && entry.standing);
            result.total = result.rows.len();
            if result.rows.is_empty() {
                refuse(
                    &mut result,
                    "sync_row_not_found",
                    "The exact standing publication is absent from this fence and project",
                );
            }
            1
        }
        Command::Retry { row } => {
            if lease_held {
                refuse(
                    &mut result,
                    "sync_worker_active",
                    "A publication worker holds the project lease",
                );
            } else if let Some(index) = request
                .entries
                .iter()
                .position(|entry| entry.row.replay_key == row && entry.standing)
            {
                if result.rows[index].retry_eligible {
                    change(
                        &mut result,
                        &request.entries[index].row,
                        "retry",
                        request.now_ms,
                    );
                    result.applied = true;
                } else {
                    refuse(&mut result, "sync_row_not_retryable", "This exact retained row has no recoverable authority verdict or readable bytes");
                }
            } else {
                refuse(
                    &mut result,
                    "sync_row_not_found",
                    "The exact standing publication is absent from this fence and project",
                );
            }
            200
        }
        Command::SanitizeApply { digest } => {
            if lease_held {
                refuse(
                    &mut result,
                    "sync_worker_active",
                    "A publication worker holds the project lease",
                );
            } else if digest != result.digest {
                refuse(
                    &mut result,
                    "sync_sanitation_refused",
                    "The complete retained snapshot changed; inspect it again",
                );
            } else {
                let actions: Vec<_> = result.rows.iter().map(|row| row.action).collect();
                for (entry, action) in request.entries.iter().zip(actions) {
                    if action != "keep" {
                        change(&mut result, &entry.row, action, request.now_ms);
                    }
                }
                result.applied = true;
            }
            200
        }
    };
    result.more = result.rows.len() > limit;
    result.rows.truncate(limit);
    Ok(result)
}

fn refuse(result: &mut Result, code: &str, detail: &str) {
    result.refusals.push(Refusal {
        code: code.into(),
        detail: detail.into(),
    });
}

fn change(result: &mut Result, row: &ArtifactRow, action: &str, now: u64) {
    let mut next = row.clone();
    if action == "archive" {
        next.archived = true;
    } else {
        next.state = ArtifactState::Held;
        next.state_reason = None;
        next.transfer_state = None;
        next.acknowledged_work_id = None;
    }
    next.updated_at_ms = now;
    result.changes.push(RowChange::UpsertArtifact(next));
    result.changes.push(RowChange::AppendReceipt(ReceiptRow {
        receipt_id: format!("recovery:{action}:{}:{now}", row.replay_key),
        scope: row.scope.clone(),
        action: action.into(),
        identity: Some(row.identity.clone()),
        outcome: action.into(),
        detail: Some("Explicit native recovery under the unchanged publication fence".into()),
        failure_code: None,
        grant_id: None,
        work_id: None,
        client_publish_id: Some(row.client_publish_id.clone()),
        head_revision: None,
        head_sha256: None,
        head_counter: None,
        committed_bytes: None,
        at_ms: now,
    }));
}

pub fn evaluate(bytes: &[u8]) -> std::result::Result<String, String> {
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("Recovery exceeds its byte bound".into());
    }
    let request =
        serde_json::from_slice(bytes).map_err(|e| format!("Invalid recovery request: {e}"))?;
    serde_json::to_string(&resolve(request)?).map_err(|e| e.to_string())
}
