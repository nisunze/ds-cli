//! Shared, transport-free reconciliation authority. Hosts commit the returned
//! queue and local content in ONE transaction before executing any effect.
//! A transport adapter may submit an acknowledgement only after authenticated
//! domain admission. An upload or HTTP 200 is never a publication receipt.
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "ds.reconciliation/v1";
const MAX_ROWS: usize = 4096;
const MAX_ATTEMPTS: u32 = 128;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub principal: String,
    pub audience: String,
    pub lane: String,
    pub project: String,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Domain {
    Design,
    Solar,
    Survey,
    Settings,
    Layers,
    Styles,
    Portfolios,
    Reports,
    Attachments,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    DesignVersion,
    DesignComparison,
    DesignSnapshot,
    DesignDelete,
    SolarInput,
    SolarResult,
    SurveyEntry,
    ProjectSettings,
    ProjectLayers,
    Style,
    Portfolio,
    Report,
    Attachment,
}
impl Operation {
    fn domain(self) -> Domain {
        match self {
            Self::DesignVersion
            | Self::DesignComparison
            | Self::DesignSnapshot
            | Self::DesignDelete => Domain::Design,
            Self::SolarInput | Self::SolarResult => Domain::Solar,
            Self::SurveyEntry => Domain::Survey,
            Self::ProjectSettings => Domain::Settings,
            Self::ProjectLayers => Domain::Layers,
            Self::Style => Domain::Styles,
            Self::Portfolio => Domain::Portfolios,
            Self::Report => Domain::Reports,
            Self::Attachment => Domain::Attachments,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "kind",
    content = "revision",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum BaseRevision {
    Unknown,
    Absent,
    Server(String),
    /// Append-only content does not mutate a remote head.
    Immutable,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
pub struct Content {
    pub sha256: String,
    pub size: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub operation_id: String,
    pub domain: Domain,
    pub object_id: String,
    pub command: Operation,
    pub base: BaseRevision,
    pub input_fingerprint: String,
    pub payload: Content,
    pub artifacts: BTreeMap<String, Content>,
    pub dependencies: BTreeSet<String>,
    pub policy_revision: String,
    pub tombstone: bool,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    PendingTransport,
    Pending,
    Leased,
    Uploaded,
    RetryWait,
    Interrupted,
    CancelRequested,
    Cancelled,
    CredentialExpired,
    AuthorizationRefused,
    Conflict,
    Acknowledged,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Lease {
    pub worker: String,
    pub epoch: u64,
    pub expires_at_ms: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub scope: Scope,
    pub operation_id: String,
    pub request_sha256: String,
    pub input_fingerprint: String,
    pub payload: Content,
    pub artifacts: BTreeMap<String, Content>,
    pub server_revision: String,
    pub policy_revision: String,
    pub receipt_id: String,
    /// Required for tombstones; names the exact recoverable payload.
    pub recovery: Option<Recovery>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Recovery {
    pub record_id: String,
    pub payload: Content,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub at_ms: u64,
    pub phase: Phase,
    pub reason: String,
    pub epoch: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub intent: Intent,
    pub request_sha256: String,
    pub phase: Phase,
    pub attempts: u32,
    pub epoch: u64,
    pub lease: Option<Lease>,
    pub retry_at_ms: Option<u64>,
    pub receipt: Option<Receipt>,
    pub events: Vec<Event>,
}
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Queue {
    pub schema: String,
    pub scope: Scope,
    pub revision: u64,
    pub clock_ms: u64,
    pub entries: BTreeMap<String, Entry>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    Enqueue {
        intent: Intent,
    },
    Claim {
        operation_id: String,
        worker: String,
        lease_ms: u64,
        transport_available: bool,
    },
    Uploaded {
        operation_id: String,
        lease: Lease,
        payload: Content,
        artifacts: BTreeMap<String, Content>,
    },
    Acknowledge {
        operation_id: String,
        lease: Lease,
        receipt: Box<Receipt>,
    },
    Fail {
        operation_id: String,
        lease: Lease,
        failure: Failure,
        reason: String,
    },
    Cancel {
        operation_id: String,
    },
    Resume {
        operation_id: String,
    },
    Recover,
    Inspect,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Failure {
    Transient,
    Interrupted,
    CredentialExpired,
    AuthorizationRefused,
    Conflict,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub scope: Scope,
    pub expected_revision: u64,
    pub now_ms: u64,
    pub queue: Option<Queue>,
    pub command: Action,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Output {
    pub queue: Queue,
    pub changed: bool,
    /// Every retained payload remains protected, including cancelled work and
    /// acknowledged Survey data. Publication is not an eviction instruction.
    pub retained_content: BTreeSet<Content>,
    pub summary: Summary,
    pub rows: Vec<RowView>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Summary {
    pub pending: usize,
    pub blocked: usize,
    pub acknowledged: usize,
    pub cancelled: usize,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct RowView {
    pub operation_id: String,
    pub domain: Domain,
    pub object_id: String,
    pub phase: Phase,
    pub attempts: u32,
    pub last_event: Option<Event>,
    pub can_cancel: bool,
    pub can_resume: bool,
}

fn text(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.trim() != value
        || value.len() > 512
        || value.chars().any(char::is_control)
    {
        return Err("invalid bounded reconciliation identity".into());
    }
    Ok(())
}
fn hash(value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("reconciliation content requires a lowercase SHA-256".into());
    }
    Ok(())
}
fn content(value: &Content) -> Result<(), String> {
    hash(&value.sha256)?;
    if value.size > 16 * 1024 * 1024 * 1024 {
        return Err("content exceeds 16 GiB bound".into());
    }
    Ok(())
}
fn scope(value: &Scope) -> Result<(), String> {
    for s in [
        &value.principal,
        &value.audience,
        &value.lane,
        &value.project,
    ] {
        text(s)?;
    }
    Ok(())
}
fn validate_intent(value: &Intent) -> Result<(), String> {
    for s in [
        &value.operation_id,
        &value.object_id,
        &value.policy_revision,
    ] {
        text(s)?;
    }
    if value.command.domain() != value.domain
        || (value.command == Operation::DesignDelete) != value.tombstone
    {
        return Err("operation/domain/tombstone mismatch".into());
    }
    hash(&value.input_fingerprint)?;
    content(&value.payload)?;
    if let BaseRevision::Server(revision) = &value.base {
        text(revision)?;
    }
    if value.artifacts.len() > 128 || value.dependencies.len() > 128 {
        return Err("intent reference bound exceeded".into());
    }
    for (name, reference) in &value.artifacts {
        text(name)?;
        content(reference)?;
    }
    for id in &value.dependencies {
        text(id)?;
        if id == &value.operation_id {
            return Err("an intent cannot depend on itself".into());
        }
    }
    Ok(())
}
pub fn request_digest(scope: &Scope, intent: &Intent) -> Result<String, String> {
    // Normalize object ordering through Value, independent of struct field order.
    let value = serde_json::to_value((SCHEMA, scope, intent)).map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec(&value).map_err(|e| e.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}
fn record(entry: &mut Entry, phase: Phase, now: u64, reason: &str) -> Result<(), String> {
    text(reason)?;
    if entry.events.len() >= 1024 {
        return Err(
            "retained operation history is full; resolve this operation before creating new work"
                .into(),
        );
    }
    entry.phase = phase;
    entry.events.push(Event {
        at_ms: now,
        phase,
        reason: reason.into(),
        epoch: entry.epoch,
    });
    Ok(())
}
fn check_lease(entry: &Entry, lease: &Lease, now: u64) -> Result<(), String> {
    if entry.lease.as_ref() != Some(lease) || now >= lease.expires_at_ms {
        return Err(
            "worker claim expired or was superseded; inspect and recover before retrying".into(),
        );
    }
    Ok(())
}
fn validate_receipt(scope: &Scope, entry: &Entry, receipt: &Receipt) -> Result<(), String> {
    let intent = &entry.intent;
    if receipt.schema != SCHEMA
        || &receipt.scope != scope
        || receipt.operation_id != intent.operation_id
        || receipt.request_sha256 != entry.request_sha256
        || receipt.input_fingerprint != intent.input_fingerprint
        || receipt.payload != intent.payload
        || receipt.artifacts != intent.artifacts
        || receipt.policy_revision != intent.policy_revision
    {
        return Err(
            "publication acknowledgement does not identify the exact admitted request and content"
                .into(),
        );
    }
    text(&receipt.receipt_id)?;
    text(&receipt.server_revision)?;
    if intent.tombstone {
        let recovery = receipt
            .recovery
            .as_ref()
            .ok_or("deletion acknowledgement lacks Recovery Center evidence")?;
        text(&recovery.record_id)?;
        if recovery.payload != intent.payload {
            return Err("recovery receipt names another transformer revision".into());
        }
    }
    Ok(())
}
fn validate_queue(queue: &Queue) -> Result<(), String> {
    if queue.schema != SCHEMA || queue.entries.len() > MAX_ROWS {
        return Err("unsupported or oversized reconciliation queue".into());
    }
    scope(&queue.scope)?;
    for (id, entry) in &queue.entries {
        validate_intent(&entry.intent)?;
        if id != &entry.intent.operation_id
            || entry.request_sha256 != request_digest(&queue.scope, &entry.intent)?
        {
            return Err("stored operation identity or immutable request digest changed".into());
        }
        if entry.events.is_empty()
            || entry.events.len() > 1024
            || entry.attempts > MAX_ATTEMPTS
            || u64::from(entry.attempts) != entry.epoch
            || entry
                .events
                .last()
                .is_none_or(|event| event.phase != entry.phase || event.epoch != entry.epoch)
            || entry
                .events
                .iter()
                .any(|event| event.at_ms > queue.clock_ms || event.epoch > entry.epoch)
            || entry
                .events
                .windows(2)
                .any(|pair| pair[0].at_ms > pair[1].at_ms || pair[0].epoch > pair[1].epoch)
        {
            return Err("invalid retained operation history".into());
        }
        match (&entry.lease, entry.phase) {
            (Some(lease), Phase::Leased | Phase::Uploaded | Phase::CancelRequested) => {
                text(&lease.worker)?;
                if lease.epoch != entry.epoch || lease.epoch == 0 {
                    return Err("invalid durable worker claim".into());
                }
            }
            (None, Phase::Leased | Phase::Uploaded) | (Some(_), _) => {
                return Err("phase and durable worker claim disagree".into())
            }
            _ => {}
        }
        if let Some(receipt) = &entry.receipt {
            validate_receipt(&queue.scope, entry, receipt)?;
        }
        if (entry.phase == Phase::Acknowledged) != entry.receipt.is_some() {
            return Err("acknowledged state requires retained publication evidence".into());
        }
    }
    Ok(())
}

pub fn apply(request: Request) -> Result<Output, String> {
    if request.schema != SCHEMA {
        return Err("unsupported reconciliation command schema".into());
    }
    scope(&request.scope)?;
    let mut queue = request.queue.unwrap_or(Queue {
        schema: SCHEMA.into(),
        scope: request.scope.clone(),
        revision: 0,
        clock_ms: 0,
        entries: BTreeMap::new(),
    });
    validate_queue(&queue)?;
    if queue.scope != request.scope {
        return Err("reconciliation principal, audience, lane or project mismatch".into());
    }
    if queue.revision != request.expected_revision {
        return Err("reconciliation transaction revision conflict".into());
    }
    let original = queue.clone();
    let now = request.now_ms;
    // Inspection is always local and remains available during clock rollback.
    if !matches!(request.command, Action::Inspect) {
        if now == 0 || now < queue.clock_ms {
            return Err(
                "clock rollback: preserve queue and revalidate time before mutation".into(),
            );
        }
        queue.clock_ms = now;
    }
    match request.command {
        Action::Inspect => {}
        Action::Enqueue { intent } => {
            validate_intent(&intent)?;
            let digest = request_digest(&queue.scope, &intent)?;
            if let Some(existing) = queue.entries.get(&intent.operation_id) {
                if existing.request_sha256 != digest {
                    return Err("operation id already binds different immutable input".into());
                }
            } else {
                if queue.entries.len() >= MAX_ROWS {
                    return Err("reconciliation queue is full; preserve existing work".into());
                }
                // Refer only to previously admitted operations: this prevents
                // cycles and permits disconnected creation chains.
                if intent
                    .dependencies
                    .iter()
                    .any(|id| !queue.entries.contains_key(id))
                {
                    return Err("dependency has not been durably admitted".into());
                }
                let mut entry = Entry {
                    intent,
                    request_sha256: digest,
                    phase: Phase::PendingTransport,
                    attempts: 0,
                    epoch: 0,
                    lease: None,
                    retry_at_ms: None,
                    receipt: None,
                    events: vec![],
                };
                record(
                    &mut entry,
                    Phase::PendingTransport,
                    now,
                    "local content and intent committed",
                )?;
                queue
                    .entries
                    .insert(entry.intent.operation_id.clone(), entry);
            }
        }
        Action::Recover => {
            for entry in queue.entries.values_mut() {
                if entry
                    .lease
                    .as_ref()
                    .is_some_and(|lease| now >= lease.expires_at_ms)
                {
                    entry.lease = None;
                    let phase = if entry.phase == Phase::CancelRequested {
                        Phase::CancelRequested
                    } else {
                        Phase::Interrupted
                    };
                    record(
                        entry,
                        phase,
                        now,
                        "worker lease expired; remote outcome may be unknown",
                    )?;
                }
            }
        }
        Action::Claim {
            operation_id,
            worker,
            lease_ms,
            transport_available,
        } => {
            text(&worker)?;
            if !(1000..=300_000).contains(&lease_ms) {
                return Err("lease must be between 1 second and 5 minutes".into());
            }
            let entry = queue
                .entries
                .get(&operation_id)
                .ok_or("operation not found")?;
            if !transport_available {
                return Err(
                    "authenticated domain transport is unavailable; local work remains retained"
                        .into(),
                );
            }
            if entry.intent.base == BaseRevision::Unknown {
                return Err("remote base revision is unknown; resolve explicitly without changing this request".into());
            }
            if entry.intent.dependencies.iter().any(|id| {
                queue
                    .entries
                    .get(id)
                    .is_none_or(|e| e.phase != Phase::Acknowledged)
            }) {
                return Err("operation dependencies are not acknowledged".into());
            }
            if entry.lease.is_some()
                || !matches!(
                    entry.phase,
                    Phase::PendingTransport
                        | Phase::Pending
                        | Phase::RetryWait
                        | Phase::Interrupted
                )
            {
                return Err("operation is not claimable".into());
            }
            if entry.retry_at_ms.is_some_and(|at| now < at) {
                return Err("retry is not due".into());
            }
            if entry.attempts >= MAX_ATTEMPTS {
                return Err(
                    "automatic retry limit reached; retained operation requires review".into(),
                );
            }
            let entry = queue.entries.get_mut(&operation_id).unwrap();
            entry.attempts += 1;
            entry.epoch = entry.epoch.checked_add(1).ok_or("claim epoch exhausted")?;
            entry.lease = Some(Lease {
                worker,
                epoch: entry.epoch,
                expires_at_ms: now.checked_add(lease_ms).ok_or("lease time overflow")?,
            });
            entry.retry_at_ms = None;
            record(
                entry,
                Phase::Leased,
                now,
                "worker claimed immutable request",
            )?;
        }
        Action::Cancel { operation_id } => {
            let entry = queue
                .entries
                .get_mut(&operation_id)
                .ok_or("operation not found")?;
            if !matches!(
                entry.phase,
                Phase::Acknowledged | Phase::Cancelled | Phase::CancelRequested
            ) {
                entry.retry_at_ms = None;
                // After any attempted delivery, absence of an acknowledgement
                // cannot prove absence of a server commit.
                let phase = if entry.attempts == 0 {
                    Phase::Cancelled
                } else {
                    Phase::CancelRequested
                };
                record(
                    entry,
                    phase,
                    now,
                    "operator requested cancellation; local content retained",
                )?;
            }
        }
        Action::Resume { operation_id } => {
            let entry = queue
                .entries
                .get_mut(&operation_id)
                .ok_or("operation not found")?;
            if entry.lease.is_some() || matches!(entry.phase, Phase::Acknowledged | Phase::Conflict)
            {
                return Err("operation cannot resume; preserve conflict evidence and submit a new resolved request".into());
            }
            entry.retry_at_ms = None;
            record(
                entry,
                Phase::PendingTransport,
                now,
                "explicit retry of the same immutable request",
            )?;
        }
        Action::Uploaded {
            operation_id,
            lease,
            payload,
            artifacts,
        } => {
            let entry = queue
                .entries
                .get_mut(&operation_id)
                .ok_or("operation not found")?;
            check_lease(entry, &lease, now)?;
            if payload != entry.intent.payload || artifacts != entry.intent.artifacts {
                return Err("uploaded content does not match the intent".into());
            }
            let phase = if entry.phase == Phase::CancelRequested {
                Phase::CancelRequested
            } else {
                Phase::Uploaded
            };
            record(
                entry,
                phase,
                now,
                "content size and digest verified; publication still pending",
            )?;
        }
        Action::Acknowledge {
            operation_id,
            lease,
            receipt,
        } => {
            let entry = queue
                .entries
                .get_mut(&operation_id)
                .ok_or("operation not found")?;
            validate_receipt(&queue.scope, entry, &receipt)?;
            if entry.receipt.as_ref() == Some(receipt.as_ref()) { /* exact replay is harmless */
            } else {
                check_lease(entry, &lease, now)?;
                entry.receipt = Some(*receipt);
                entry.lease = None;
                entry.retry_at_ms = None;
                record(
                    entry,
                    Phase::Acknowledged,
                    now,
                    "authenticated exact-content publication acknowledged",
                )?;
            }
        }
        Action::Fail {
            operation_id,
            lease,
            failure,
            reason,
        } => {
            let entry = queue
                .entries
                .get_mut(&operation_id)
                .ok_or("operation not found")?;
            check_lease(entry, &lease, now)?;
            entry.lease = None;
            let phase = if entry.phase == Phase::CancelRequested {
                Phase::CancelRequested
            } else {
                match failure {
                    Failure::Transient => {
                        entry.retry_at_ms = Some(
                            now.saturating_add((2000u64 << entry.attempts.min(8)).min(300_000)),
                        );
                        Phase::RetryWait
                    }
                    Failure::Interrupted => Phase::Interrupted,
                    Failure::CredentialExpired => Phase::CredentialExpired,
                    Failure::AuthorizationRefused => Phase::AuthorizationRefused,
                    Failure::Conflict => Phase::Conflict,
                }
            };
            record(entry, phase, now, &reason)?;
        }
    }
    let changed = queue != original;
    if changed {
        queue.revision = queue
            .revision
            .checked_add(1)
            .ok_or("queue revision exhausted")?;
    }
    let retained_content = queue
        .entries
        .values()
        .flat_map(|e| {
            std::iter::once(e.intent.payload.clone()).chain(e.intent.artifacts.values().cloned())
        })
        .collect();
    let mut summary = Summary::default();
    let rows = queue
        .entries
        .values()
        .map(|entry| {
            match entry.phase {
                Phase::Acknowledged => summary.acknowledged += 1,
                Phase::Cancelled => summary.cancelled += 1,
                Phase::PendingTransport
                | Phase::CredentialExpired
                | Phase::AuthorizationRefused
                | Phase::Conflict
                | Phase::Interrupted
                | Phase::CancelRequested => summary.blocked += 1,
                _ => summary.pending += 1,
            }
            RowView {
                operation_id: entry.intent.operation_id.clone(),
                domain: entry.intent.domain,
                object_id: entry.intent.object_id.clone(),
                phase: entry.phase,
                attempts: entry.attempts,
                last_event: entry.events.last().cloned(),
                can_cancel: !matches!(
                    entry.phase,
                    Phase::Acknowledged | Phase::Cancelled | Phase::CancelRequested
                ),
                can_resume: entry.lease.is_none()
                    && matches!(
                        entry.phase,
                        Phase::Cancelled
                            | Phase::CancelRequested
                            | Phase::CredentialExpired
                            | Phase::AuthorizationRefused
                            | Phase::Interrupted
                            | Phase::RetryWait
                    ),
            }
        })
        .collect();
    Ok(Output {
        queue,
        changed,
        retained_content,
        summary,
        rows,
    })
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > 16 * 1024 * 1024 {
        return Err("reconciliation metadata exceeds 16 MiB".into());
    }
    let request = serde_json::from_slice(input)
        .map_err(|e| format!("invalid reconciliation command: {e}"))?;
    serde_json::to_string(&apply(request)?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> Scope {
        Scope {
            principal: "engineer".into(),
            audience: "local-firebase".into(),
            lane: "local".into(),
            project: "project".into(),
        }
    }
    fn intent(id: &str) -> Intent {
        Intent {
            operation_id: id.into(),
            domain: Domain::Design,
            object_id: "transformer".into(),
            command: Operation::DesignVersion,
            base: BaseRevision::Immutable,
            input_fingerprint: "a".repeat(64),
            payload: Content {
                sha256: "b".repeat(64),
                size: 100,
            },
            artifacts: BTreeMap::new(),
            dependencies: BTreeSet::new(),
            policy_revision: "policy-1".into(),
            tombstone: false,
        }
    }
    fn run(queue: Option<Queue>, now: u64, command: Action) -> Result<Output, String> {
        apply(Request {
            schema: SCHEMA.into(),
            scope: scope(),
            expected_revision: queue.as_ref().map_or(0, |q| q.revision),
            now_ms: now,
            queue,
            command,
        })
    }
    fn queued(value: Intent) -> Queue {
        run(None, 1000, Action::Enqueue { intent: value })
            .unwrap()
            .queue
    }
    fn claim(queue: Queue, now: u64) -> Queue {
        run(
            Some(queue),
            now,
            Action::Claim {
                operation_id: "one".into(),
                worker: "worker".into(),
                lease_ms: 1000,
                transport_available: true,
            },
        )
        .unwrap()
        .queue
    }
    fn receipt(queue: &Queue) -> Receipt {
        let row = &queue.entries["one"];
        Receipt {
            schema: SCHEMA.into(),
            scope: scope(),
            operation_id: "one".into(),
            request_sha256: row.request_sha256.clone(),
            input_fingerprint: row.intent.input_fingerprint.clone(),
            payload: row.intent.payload.clone(),
            artifacts: row.intent.artifacts.clone(),
            server_revision: "server-v7".into(),
            policy_revision: "policy-1".into(),
            receipt_id: "receipt-1".into(),
            recovery: None,
        }
    }
    #[test]
    fn immutable_identity_scope_and_transaction_fences() {
        let queue = queued(intent("one"));
        assert!(run(
            Some(queue.clone()),
            1001,
            Action::Enqueue {
                intent: intent("one")
            }
        )
        .is_ok());
        let mut changed = intent("one");
        changed.payload.size += 1;
        assert!(run(
            Some(queue.clone()),
            1001,
            Action::Enqueue { intent: changed }
        )
        .is_err());
        let mut other = scope();
        other.lane = "production".into();
        assert!(apply(Request {
            schema: SCHEMA.into(),
            scope: other,
            expected_revision: queue.revision,
            now_ms: 1001,
            queue: Some(queue.clone()),
            command: Action::Inspect
        })
        .is_err());
        assert!(apply(Request {
            schema: SCHEMA.into(),
            scope: scope(),
            expected_revision: 0,
            now_ms: 1001,
            queue: Some(queue),
            command: Action::Inspect
        })
        .is_err());
    }
    #[test]
    fn expired_worker_cannot_acknowledge_reclaimed_work() {
        let queue = claim(queued(intent("one")), 1001);
        let old = queue.entries["one"].lease.clone().unwrap();
        let queue = run(Some(queue), 2001, Action::Recover).unwrap().queue;
        assert_eq!(queue.entries["one"].phase, Phase::Interrupted);
        let queue = claim(queue, 2002);
        let proof = receipt(&queue);
        assert!(run(
            Some(queue.clone()),
            2003,
            Action::Acknowledge {
                operation_id: "one".into(),
                lease: old,
                receipt: Box::new(proof.clone())
            }
        )
        .is_err());
        let lease = queue.entries["one"].lease.clone().unwrap();
        let output = run(
            Some(queue),
            2003,
            Action::Acknowledge {
                operation_id: "one".into(),
                lease,
                receipt: Box::new(proof),
            },
        )
        .unwrap();
        assert_eq!(output.queue.entries["one"].phase, Phase::Acknowledged);
        assert_eq!(output.retained_content.len(), 1);
    }
    #[test]
    fn bytes_uploaded_are_not_publication_and_mismatched_receipts_refuse() {
        let mut value = intent("one");
        value.artifacts.insert(
            "report".into(),
            Content {
                sha256: "c".repeat(64),
                size: 45,
            },
        );
        let queue = claim(queued(value.clone()), 1001);
        let lease = queue.entries["one"].lease.clone().unwrap();
        let queue = run(
            Some(queue),
            1002,
            Action::Uploaded {
                operation_id: "one".into(),
                lease: lease.clone(),
                payload: value.payload,
                artifacts: value.artifacts,
            },
        )
        .unwrap()
        .queue;
        assert_eq!(queue.entries["one"].phase, Phase::Uploaded);
        assert!(queue.entries["one"].receipt.is_none());
        let mut proof = receipt(&queue);
        proof.artifacts.clear();
        assert!(run(
            Some(queue),
            1003,
            Action::Acknowledge {
                operation_id: "one".into(),
                lease,
                receipt: Box::new(proof)
            }
        )
        .is_err());
    }
    #[test]
    fn deletion_cannot_finalize_without_exact_recovery_evidence() {
        let mut value = intent("one");
        value.command = Operation::DesignDelete;
        value.tombstone = true;
        let queue = claim(queued(value), 1001);
        let lease = queue.entries["one"].lease.clone().unwrap();
        let mut proof = receipt(&queue);
        assert!(run(
            Some(queue.clone()),
            1002,
            Action::Acknowledge {
                operation_id: "one".into(),
                lease: lease.clone(),
                receipt: Box::new(proof.clone())
            }
        )
        .is_err());
        proof.recovery = Some(Recovery {
            record_id: "recovery-1".into(),
            payload: proof.payload.clone(),
        });
        assert!(run(
            Some(queue),
            1002,
            Action::Acknowledge {
                operation_id: "one".into(),
                lease,
                receipt: Box::new(proof)
            }
        )
        .is_ok());
    }
    #[test]
    fn cancellation_preserves_content_and_committed_receipt_wins() {
        let queue = claim(queued(intent("one")), 1001);
        let lease = queue.entries["one"].lease.clone().unwrap();
        let queue = run(
            Some(queue),
            1002,
            Action::Cancel {
                operation_id: "one".into(),
            },
        )
        .unwrap()
        .queue;
        assert_eq!(queue.entries["one"].phase, Phase::CancelRequested);
        let proof = receipt(&queue);
        let output = run(
            Some(queue),
            1003,
            Action::Acknowledge {
                operation_id: "one".into(),
                lease,
                receipt: Box::new(proof),
            },
        )
        .unwrap();
        assert_eq!(output.queue.entries["one"].phase, Phase::Acknowledged);
        let output = run(
            Some(queued(intent("one"))),
            1002,
            Action::Cancel {
                operation_id: "one".into(),
            },
        )
        .unwrap();
        assert_eq!(output.queue.entries["one"].phase, Phase::Cancelled);
        assert!(!output.retained_content.is_empty());
    }
    #[test]
    fn dependency_waits_for_publication_and_cycles_cannot_enter() {
        let mut child = intent("two");
        child.dependencies.insert("one".into());
        assert!(run(
            None,
            1000,
            Action::Enqueue {
                intent: child.clone()
            }
        )
        .is_err());
        let queue = run(
            Some(queued(intent("one"))),
            1001,
            Action::Enqueue { intent: child },
        )
        .unwrap()
        .queue;
        assert!(run(
            Some(queue.clone()),
            1002,
            Action::Claim {
                operation_id: "two".into(),
                worker: "worker".into(),
                lease_ms: 1000,
                transport_available: true
            }
        )
        .is_err());
        let queue = claim(queue, 1002);
        let lease = queue.entries["one"].lease.clone().unwrap();
        let proof = receipt(&queue);
        let queue = run(
            Some(queue),
            1003,
            Action::Acknowledge {
                operation_id: "one".into(),
                lease,
                receipt: Box::new(proof),
            },
        )
        .unwrap()
        .queue;
        assert!(run(
            Some(queue),
            1004,
            Action::Claim {
                operation_id: "two".into(),
                worker: "worker".into(),
                lease_ms: 1000,
                transport_available: true
            }
        )
        .is_ok());
    }
    #[test]
    fn credential_refusal_stops_automatic_work_and_inspection_survives_clock_rollback() {
        let queue = claim(queued(intent("one")), 1001);
        let lease = queue.entries["one"].lease.clone().unwrap();
        let queue = run(
            Some(queue),
            1002,
            Action::Fail {
                operation_id: "one".into(),
                lease,
                failure: Failure::CredentialExpired,
                reason: "renew user credential".into(),
            },
        )
        .unwrap()
        .queue;
        assert_eq!(queue.entries["one"].phase, Phase::CredentialExpired);
        assert!(run(
            Some(queue.clone()),
            1003,
            Action::Claim {
                operation_id: "one".into(),
                worker: "worker".into(),
                lease_ms: 1000,
                transport_available: true
            }
        )
        .is_err());
        assert!(run(Some(queue.clone()), 1, Action::Inspect).is_ok());
        assert!(run(
            Some(queue),
            1,
            Action::Resume {
                operation_id: "one".into()
            }
        )
        .is_err());
    }
    #[test]
    fn corrupt_history_cannot_manufacture_acknowledgement() {
        let mut queue = queued(intent("one"));
        queue.entries.get_mut("one").unwrap().phase = Phase::Acknowledged;
        assert!(run(Some(queue), 1001, Action::Inspect).is_err());
    }
}
