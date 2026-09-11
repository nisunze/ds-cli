//! The sync store's row shapes and every transition over them — pure.
//!
//! Contract: `docs/contracts/ds-sync-store.md`, under
//! `ds-headless-application-runtime.md` §5–§6. One durable queue and artifact
//! store per execution context, shaped here, persisted by the host (SQLite
//! through `crates/ds-sync-store`, IndexedDB in the browser, the platform's
//! storage on mobile). A host applies the `RowChange`s a transition returns
//! and nothing else; it never re-decides a state.
//!
//! What this module guarantees:
//! - one artifact row per `(fence, scope, identity)`, one replay key per row,
//!   so the same work is never executed twice under two identities;
//! - a lease per scope, so two windows, instances, background hosts or
//!   workers never drain the same work;
//! - every action leaves a receipt (append-only evidence);
//! - the legacy per-user outbox is handed off row by row, never silently
//!   dropped, with the original replay identity, digest, acknowledgement and
//!   unresolved failure preserved and the bytes' presence verified by the host
//!   before the row is admitted.

use crate::sync::{Identity, Trigger};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const SCHEMA: &str = "ds.sync-store/v1";
const INPUT_BOUND: usize = 16 * 1024 * 1024;
const MAX_ROWS: usize = 8192;
/// A lease a worker did not renew within this long is dead.
pub const LEASE_TTL_MAX_MS: u64 = 15 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Scope {
    Project { project: String },
    Personal,
}

impl Scope {
    pub fn project(&self) -> &str {
        match self {
            Scope::Project { project } => project,
            Scope::Personal => "",
        }
    }
}

/// Every row lives under one fence; rows from another fence are never
/// executed. `deployment` is the gateway origin plus lane the credential is
/// for; `account` the signed-in principal; `install_id` this installation.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Fence {
    pub account: String,
    pub deployment: String,
    pub install_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactState {
    Held,
    Uploading,
    Published,
    Conflict,
    Refused,
}

impl ArtifactState {
    pub fn as_str(self) -> &'static str {
        match self {
            ArtifactState::Held => "held",
            ArtifactState::Uploading => "uploading",
            ArtifactState::Published => "published",
            ArtifactState::Conflict => "conflict",
            ArtifactState::Refused => "refused",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactOutput {
    pub output_id: String,
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub content_type: String,
    pub sha256: String,
    pub size_bytes: u64,
}

/// One publication this context holds: an inventory of outputs under one
/// identity, and the closure needed to publish it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRow {
    pub scope: Scope,
    #[serde(flatten)]
    pub identity: Identity,
    /// [`inventory_digest`] over `outputs` — the digest the remote head
    /// carries for the same inventory.
    pub sha256: String,
    pub size_bytes: u64,
    pub produced_at_ms: u64,
    #[serde(default)]
    pub base_revision: Option<String>,
    pub engine_release: String,
    #[serde(default)]
    pub engine_build_manifest_sha256: Option<String>,
    /// The engine name the edge lane spells (what a grant scopes), when it
    /// differs from `identity.engine`.
    #[serde(default)]
    pub grant_engine: Option<String>,
    /// The resource a grant is scoped to (one canonical transformer for the
    /// Network Reporter lane).
    #[serde(default)]
    pub resource: Option<String>,
    /// The server-side idempotency key of the publication.
    pub client_publish_id: String,
    #[serde(default)]
    pub outputs: Vec<ArtifactOutput>,
    /// Host-opaque: a producer store reference, a path, an IndexedDB key —
    /// never interpreted here.
    pub bytes_locator: String,
    #[serde(default = "default_true")]
    pub readable: bool,
    pub state: ArtifactState,
    #[serde(default)]
    pub state_reason: Option<String>,
    /// Idempotency across attempts and across the handoff.
    pub replay_key: String,
    pub updated_at_ms: u64,
    /// The transport's own persisted state for an in-flight transfer.
    #[serde(default)]
    pub transfer_state: Option<Value>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeadOutput {
    pub output_id: String,
    pub sha256: String,
    pub size_bytes: u64,
}

/// One head of the shared record, as last read or as a publish receipt
/// carried it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HeadRow {
    pub scope: Scope,
    #[serde(flatten)]
    pub identity: Identity,
    /// The concurrency fence the server checks at publish.
    pub revision: String,
    #[serde(default)]
    pub head_revision: i64,
    pub sha256: String,
    pub published_at_ms: u64,
    #[serde(default)]
    pub outputs: Vec<HeadOutput>,
    #[serde(default)]
    pub read_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GrantRow {
    pub scope: Scope,
    pub grant_id: String,
    pub install_id: String,
    #[serde(default)]
    pub engines: Vec<crate::sync::EngineRelease>,
    pub expires_at_ms: u64,
    /// The grant as the host holds it (the signed document and what the
    /// snapshot pinned), opaque to the store.
    pub raw: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReceiptRow {
    pub receipt_id: String,
    pub scope: Scope,
    pub action: String,
    #[serde(default)]
    pub identity: Option<Identity>,
    pub outcome: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub grant_id: Option<String>,
    #[serde(default)]
    pub work_id: Option<String>,
    #[serde(default)]
    pub head_revision: Option<String>,
    #[serde(default)]
    pub head_sha256: Option<String>,
    /// The record's monotonic head counter beside `head_revision`, when the
    /// publish answered with one.
    #[serde(default)]
    pub head_counter: Option<i64>,
    #[serde(default)]
    pub committed_bytes: Option<u64>,
    pub at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LeaseRow {
    pub scope: Scope,
    pub worker_id: String,
    pub taken_at_ms: u64,
    pub expires_at_ms: u64,
}

/// The rows a host holds for one scope under one fence, as it reads them.
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    #[serde(default)]
    pub artifacts: Vec<ArtifactRow>,
    #[serde(default)]
    pub heads: Vec<HeadRow>,
    #[serde(default)]
    pub grants: Vec<GrantRow>,
    #[serde(default)]
    pub lease: Option<LeaseRow>,
}

/// One output of a legacy per-user outbox row. `session_uri` is a scoped
/// bearer credential in that store; it is not read here and never copied
/// into evidence.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LegacyOutput {
    pub output_id: String,
    /// The producer's own locator for the bytes (a native batch reference on
    /// desktop); the host reads it, the store never does.
    #[serde(default)]
    pub local_path: String,
    #[serde(default)]
    pub format: String,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub sha256: String,
    #[serde(default)]
    pub size_bytes: u64,
    #[serde(default)]
    pub expected_sha256: Option<String>,
    #[serde(default)]
    pub expected_size_bytes: Option<u64>,
    #[serde(default)]
    pub state: String,
}

impl LegacyOutput {
    pub fn digest(&self) -> String {
        self.expected_sha256
            .clone()
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| self.sha256.clone())
    }
    pub fn size(&self) -> u64 {
        self.expected_size_bytes
            .filter(|value| *value > 0)
            .unwrap_or(self.size_bytes)
    }
}

/// A legacy per-user outbox row as the browser stored it (`kind:
/// compute_artifact` in the `outbox` object store). Only the fields the
/// handoff reads are typed; the rest travels in `rest` so the receipt keeps
/// the whole row as evidence.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct LegacyOutboxRow {
    pub id: String,
    #[serde(default)]
    pub owner_uid: String,
    #[serde(default)]
    pub project_id: String,
    pub engine: String,
    pub operation: String,
    #[serde(default = "default_variant")]
    pub variant: String,
    #[serde(default)]
    pub engine_version: String,
    #[serde(default)]
    pub engine_build_manifest_sha256: Option<String>,
    #[serde(default)]
    pub input_base_fingerprint: String,
    #[serde(default)]
    pub client_publish_id: Option<String>,
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub phase: String,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(default)]
    pub failure_code: Option<String>,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub outputs: Vec<LegacyOutput>,
    #[serde(flatten)]
    pub rest: Value,
}

fn default_variant() -> String {
    "default".into()
}

/// What the host established about a legacy row's bytes before the handoff.
/// Copied metadata is never proof that bytes arrived: the host reads them
/// through its own producer store and checks the digests, or says it could not.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum BytesEvidence {
    Verified { locator: String },
    Missing { reason: String },
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LegacyHandoff {
    pub row: LegacyOutboxRow,
    pub bytes: BytesEvidence,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    /// A producer committed a publication locally. The row arrives complete
    /// from the host; the store decides whether it is new, a re-record, or a
    /// supersession of the identity's earlier publication.
    LocalProduced {
        row: ArtifactRow,
    },
    /// The shared record was read for one scope: every head, at once.
    HeadsRead {
        scope: Scope,
        heads: Vec<HeadRow>,
    },
    GrantOpened {
        grant: GrantRow,
    },
    ActionReceipt {
        receipt: ReceiptRow,
    },
    LeaseTake {
        scope: Scope,
        worker_id: String,
        ttl_ms: u64,
    },
    LeaseRenew {
        scope: Scope,
        worker_id: String,
        ttl_ms: u64,
    },
    LeaseRelease {
        scope: Scope,
        worker_id: String,
    },
    Cancel {
        scope: Scope,
        identity: Identity,
        /// A job-owned publication cancels only the exact local replay key it
        /// observed. `None` preserves the existing identity-only UI action.
        #[serde(default)]
        expected_replay_key: Option<String>,
    },
    HandoffLegacy {
        rows: Vec<LegacyHandoff>,
    },
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub fence: Fence,
    pub now_ms: u64,
    #[serde(default)]
    pub snapshot: Snapshot,
    pub event: Event,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RowChange {
    UpsertArtifact(ArtifactRow),
    /// Every head of the scope, read at `read_at_ms` — carried on the change
    /// itself so a scope with no heads still records that it was read.
    ReplaceHeads {
        scope: Scope,
        heads: Vec<HeadRow>,
        read_at_ms: u64,
    },
    UpsertHead(HeadRow),
    UpsertGrant(GrantRow),
    DropGrant {
        scope: Scope,
        grant_id: String,
    },
    AppendReceipt(ReceiptRow),
    SetLease(LeaseRow),
    ClearLease {
        scope: Scope,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Refusal {
    pub code: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub schema: String,
    pub changes: Vec<RowChange>,
    pub refusals: Vec<Refusal>,
    /// The trigger the host should plan with after applying the changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plan_trigger: Option<Trigger>,
}

impl Transition {
    fn new() -> Self {
        Transition {
            schema: SCHEMA.into(),
            changes: Vec::new(),
            refusals: Vec::new(),
            plan_trigger: None,
        }
    }
    fn refuse(&mut self, code: &str, detail: impl Into<String>) {
        self.refusals.push(Refusal {
            code: code.into(),
            detail: detail.into(),
        });
    }
}

/// The one digest that stands for a publication's output inventory, on the
/// local row and the remote head alike: sorted by output id, one
/// `"<id>\0<sha256>\n"` line each. Stable across hosts and insertion orders.
pub fn inventory_digest(outputs: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = outputs.iter().collect();
    sorted.sort();
    let mut hasher = Sha256::new();
    for (output_id, sha256) in sorted {
        hasher.update(output_id.as_bytes());
        hasher.update([0]);
        hasher.update(sha256.as_bytes());
        hasher.update(*b"\n");
    }
    format!("{:x}", hasher.finalize())
}

fn live_lease<'a>(snapshot: &'a Snapshot, scope: &Scope, now_ms: u64) -> Option<&'a LeaseRow> {
    snapshot
        .lease
        .as_ref()
        .filter(|lease| &lease.scope == scope && lease.expires_at_ms > now_ms)
}

fn find_artifact<'a>(
    snapshot: &'a Snapshot,
    scope: &Scope,
    identity: &Identity,
) -> Option<&'a ArtifactRow> {
    snapshot
        .artifacts
        .iter()
        .find(|row| &row.scope == scope && &row.identity == identity)
}

pub fn apply(request: &Request) -> Result<Transition, String> {
    if request.schema != SCHEMA {
        return Err(format!(
            "unsupported sync store schema `{}`",
            request.schema
        ));
    }
    if request.fence.account.is_empty()
        || request.fence.deployment.is_empty()
        || request.fence.install_id.is_empty()
    {
        return Err("a sync store request carries a complete fence".into());
    }
    if request.snapshot.artifacts.len() > MAX_ROWS || request.snapshot.heads.len() > MAX_ROWS {
        return Err("sync store snapshot exceeds its bound".into());
    }
    let now = request.now_ms;
    let snapshot = &request.snapshot;
    let mut t = Transition::new();
    match &request.event {
        Event::LocalProduced { row } => {
            if row.replay_key.is_empty()
                || row.bytes_locator.is_empty()
                || row.client_publish_id.is_empty()
            {
                return Err(
                    "a produced artifact carries a replay key, a publish id and a bytes locator"
                        .into(),
                );
            }
            if let Some(existing) = find_artifact(snapshot, &row.scope, &row.identity) {
                if existing.sha256 == row.sha256 || existing.replay_key == row.replay_key {
                    // The same publication, already recorded: whatever its
                    // state, it is not re-executed under a second key.
                    t.refuse(
                        "already_recorded",
                        format!("{} is {}", existing.replay_key, existing.state.as_str()),
                    );
                    return Ok(t);
                }
            }
            if let Some(clash) = snapshot
                .artifacts
                .iter()
                .find(|existing| existing.replay_key == row.replay_key)
            {
                if clash.identity != row.identity || clash.scope != row.scope {
                    t.refuse(
                        "replay_key_taken",
                        format!(
                            "{} already names {}/{}/{}",
                            row.replay_key,
                            clash.identity.engine,
                            clash.identity.operation,
                            clash.identity.variant
                        ),
                    );
                    return Ok(t);
                }
            }
            let mut next = row.clone();
            next.state = ArtifactState::Held;
            next.state_reason = None;
            next.transfer_state = None;
            next.updated_at_ms = now;
            t.changes.push(RowChange::UpsertArtifact(next));
            // Uploads never wait: the next plan is a local change.
            t.plan_trigger = Some(Trigger::LocalChange);
        }
        Event::HeadsRead { scope, heads } => {
            let rows = heads
                .iter()
                .map(|head| {
                    let mut row = head.clone();
                    row.scope = scope.clone();
                    row.read_at_ms = now;
                    row
                })
                .collect();
            t.changes.push(RowChange::ReplaceHeads {
                scope: scope.clone(),
                heads: rows,
                read_at_ms: now,
            });
        }
        Event::GrantOpened { grant } => {
            if grant.install_id != request.fence.install_id {
                t.refuse("grant_scope_mismatch", "the grant names another install");
                return Ok(t);
            }
            if grant.grant_id.is_empty() {
                return Err("a grant carries its id".into());
            }
            for old in snapshot.grants.iter().filter(|g| g.scope == grant.scope) {
                t.changes.push(RowChange::DropGrant {
                    scope: grant.scope.clone(),
                    grant_id: old.grant_id.clone(),
                });
            }
            t.changes.push(RowChange::UpsertGrant(grant.clone()));
            t.plan_trigger = Some(Trigger::GrantOpened);
        }
        Event::ActionReceipt { receipt } => {
            if receipt.receipt_id.is_empty() {
                return Err("a receipt carries its id".into());
            }
            t.changes.push(RowChange::AppendReceipt(receipt.clone()));
            let Some(identity) = &receipt.identity else {
                return Ok(t);
            };
            let existing = find_artifact(snapshot, &receipt.scope, identity);
            let mut restate = |state: ArtifactState, reason: Option<String>| {
                if let Some(row) = existing {
                    let mut next = row.clone();
                    next.state = state;
                    next.state_reason = reason;
                    if !matches!(state, ArtifactState::Uploading) {
                        next.transfer_state = None;
                    }
                    next.updated_at_ms = now;
                    t.changes.push(RowChange::UpsertArtifact(next));
                    true
                } else {
                    false
                }
            };
            match (receipt.action.as_str(), receipt.outcome.as_str()) {
                ("upload", "committed") => {
                    if restate(ArtifactState::Published, None) {
                        if let (Some(row), Some(revision)) = (existing, &receipt.head_revision) {
                            // The head is carried by the receipt, never re-read.
                            t.changes.push(RowChange::UpsertHead(HeadRow {
                                scope: receipt.scope.clone(),
                                identity: identity.clone(),
                                revision: revision.clone(),
                                head_revision: receipt.head_counter.unwrap_or_default(),
                                sha256: receipt
                                    .head_sha256
                                    .clone()
                                    .unwrap_or_else(|| row.sha256.clone()),
                                published_at_ms: receipt.at_ms,
                                outputs: row
                                    .outputs
                                    .iter()
                                    .map(|output| HeadOutput {
                                        output_id: output.output_id.clone(),
                                        sha256: output.sha256.clone(),
                                        size_bytes: output.size_bytes,
                                    })
                                    .collect(),
                                read_at_ms: receipt.at_ms,
                            }));
                        }
                        t.plan_trigger = Some(Trigger::PublishCompleted);
                    }
                }
                ("upload", "in_progress") => {
                    restate(ArtifactState::Uploading, None);
                }
                ("upload", "conflict") | ("conflict", _) => {
                    restate(ArtifactState::Conflict, receipt.detail.clone());
                }
                ("upload", "refused") | ("refused", _) => {
                    restate(ArtifactState::Refused, receipt.detail.clone());
                }
                ("upload", _) => {
                    // A failed attempt keeps the work held, with the reason
                    // on the row so the next run and the operator see it.
                    restate(ArtifactState::Held, receipt.detail.clone());
                }
                ("download", "committed") => {
                    t.plan_trigger = Some(Trigger::DownloadCompleted);
                }
                _ => {}
            }
        }
        Event::LeaseTake {
            scope,
            worker_id,
            ttl_ms,
        }
        | Event::LeaseRenew {
            scope,
            worker_id,
            ttl_ms,
        } => {
            if *ttl_ms == 0 || *ttl_ms > LEASE_TTL_MAX_MS || worker_id.is_empty() {
                return Err("a lease names its worker and lasts between 1 ms and 15 min".into());
            }
            let renewing = matches!(request.event, Event::LeaseRenew { .. });
            match live_lease(snapshot, scope, now) {
                Some(lease) if lease.worker_id != *worker_id => {
                    t.refuse(
                        "lease_held",
                        format!("{} until {}", lease.worker_id, lease.expires_at_ms),
                    );
                }
                None if renewing => {
                    t.refuse("lease_expired", "renew a live lease; take a new one");
                }
                _ => t.changes.push(RowChange::SetLease(LeaseRow {
                    scope: scope.clone(),
                    worker_id: worker_id.clone(),
                    taken_at_ms: now,
                    expires_at_ms: now.saturating_add(*ttl_ms),
                })),
            }
        }
        Event::LeaseRelease { scope, worker_id } => match live_lease(snapshot, scope, now) {
            Some(lease) if lease.worker_id == *worker_id => t.changes.push(RowChange::ClearLease {
                scope: scope.clone(),
            }),
            Some(lease) => t.refuse("lease_held", format!("{} holds it", lease.worker_id)),
            None => {}
        },
        Event::Cancel {
            scope,
            identity,
            expected_replay_key,
        } => match find_artifact(snapshot, scope, identity) {
            Some(row)
                if expected_replay_key
                    .as_deref()
                    .is_some_and(|expected| expected != row.replay_key) =>
            {
                t.refuse(
                    "publication_replaced",
                    "the current publication has another replay key",
                );
            }
            Some(row) if matches!(row.state, ArtifactState::Held | ArtifactState::Uploading) => {
                let mut next = row.clone();
                next.state = ArtifactState::Refused;
                next.state_reason = Some("cancelled".into());
                next.transfer_state = None;
                next.updated_at_ms = now;
                t.changes.push(RowChange::UpsertArtifact(next));
                t.changes.push(RowChange::AppendReceipt(ReceiptRow {
                    receipt_id: format!("cancel:{}:{}", row.replay_key, now),
                    scope: scope.clone(),
                    action: "cancel".into(),
                    identity: Some(identity.clone()),
                    outcome: "cancelled".into(),
                    detail: None,
                    grant_id: None,
                    work_id: None,
                    head_revision: None,
                    head_sha256: None,
                    head_counter: None,
                    committed_bytes: None,
                    at_ms: now,
                }));
            }
            Some(row) => t.refuse(
                "not_cancellable",
                format!("{} work is never undone", row.state.as_str()),
            ),
            None => t.refuse("unknown_identity", identity.operation.clone()),
        },
        Event::HandoffLegacy { rows } => {
            if rows.len() > MAX_ROWS {
                return Err("legacy handoff exceeds its bound".into());
            }
            for handoff in rows {
                handoff_one(snapshot, handoff, now, &mut t);
            }
            t.plan_trigger = Some(Trigger::Startup);
        }
    }
    Ok(t)
}

/// One legacy row becomes exactly one artifact row and one receipt naming its
/// origin — never silent loss. A row the shape cannot admit, or whose bytes
/// the host could not verify, is kept as a refused row whose reason says what
/// happened and what recovers it.
fn handoff_one(snapshot: &Snapshot, handoff: &LegacyHandoff, now: u64, t: &mut Transition) {
    let legacy = &handoff.row;
    let replay_key = legacy
        .client_publish_id
        .clone()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| legacy.id.clone());
    let identity = Identity {
        engine: legacy.engine.clone(),
        operation: legacy.operation.clone(),
        variant: legacy.variant.clone(),
    };
    let scope = Scope::Project {
        project: legacy.project_id.clone(),
    };
    // Already handed off, or already recorded by the producer under the same
    // publish id: the store holds it; nothing is imported twice.
    if snapshot
        .artifacts
        .iter()
        .any(|row| row.replay_key == replay_key || row.replay_key == legacy.id)
    {
        return;
    }
    let outputs: Vec<ArtifactOutput> = legacy
        .outputs
        .iter()
        .map(|output| ArtifactOutput {
            output_id: output.output_id.clone(),
            format: output.format.clone(),
            content_type: output.content_type.clone(),
            sha256: output.digest(),
            size_bytes: output.size(),
        })
        .collect();
    let digests_complete =
        !outputs.is_empty() && outputs.iter().all(|output| output.sha256.len() == 64);
    let pairs: Vec<(String, String)> = outputs
        .iter()
        .map(|output| (output.output_id.clone(), output.sha256.clone()))
        .collect();
    let (state, reason) = match (legacy.status.as_str(), legacy.phase.as_str()) {
        ("synced", _) | (_, "published") => (ArtifactState::Published, None),
        ("conflict", _) | (_, "stored_stale") => (ArtifactState::Conflict, legacy.error.clone()),
        _ => (ArtifactState::Held, legacy.error.clone()),
    };
    let (state, reason, readable, locator, outcome) = if legacy.project_id.is_empty()
        || !digests_complete
    {
        (
            ArtifactState::Refused,
            Some(
                "handoff_unmapped: the legacy row names no project or carries no complete output digests; no automatic remedy — run the engine again to reproduce this publication"
                    .to_string(),
            ),
            false,
            format!("legacy:{}", legacy.id),
            "unmapped",
        )
    } else {
        match &handoff.bytes {
            BytesEvidence::Verified { locator } => {
                (state, reason, true, locator.clone(), "imported")
            }
            BytesEvidence::Missing { reason: why } => (
                ArtifactState::Refused,
                Some(format!(
                    "handoff_bytes_missing: {why}; run the engine again to reproduce this publication"
                )),
                false,
                format!("legacy:{}", legacy.id),
                "bytes_missing",
            ),
        }
    };
    t.changes.push(RowChange::UpsertArtifact(ArtifactRow {
        scope: scope.clone(),
        identity: identity.clone(),
        sha256: inventory_digest(&pairs),
        size_bytes: outputs.iter().map(|output| output.size_bytes).sum(),
        produced_at_ms: legacy.created_at,
        base_revision: Some(legacy.input_base_fingerprint.clone()).filter(|v| !v.is_empty()),
        engine_release: legacy.engine_version.clone(),
        engine_build_manifest_sha256: legacy.engine_build_manifest_sha256.clone(),
        grant_engine: None,
        resource: None,
        client_publish_id: replay_key.clone(),
        outputs,
        bytes_locator: locator,
        readable,
        state,
        state_reason: reason,
        replay_key,
        updated_at_ms: now,
        transfer_state: None,
    }));
    t.changes.push(RowChange::AppendReceipt(ReceiptRow {
        receipt_id: format!("handoff:{}", legacy.id),
        scope,
        action: "handoff".into(),
        identity: Some(identity),
        outcome: outcome.into(),
        detail: Some(legacy_row_evidence(legacy)),
        grant_id: None,
        work_id: None,
        head_revision: None,
        head_sha256: None,
        head_counter: None,
        committed_bytes: None,
        at_ms: now,
    }));
}

/// The whole legacy row as evidence on its handoff receipt — every field the
/// handoff read and every field it did not — minus the scoped upload
/// credentials the legacy store kept beside each output.
fn legacy_row_evidence(legacy: &LegacyOutboxRow) -> String {
    let mut kept = match serde_json::to_value(legacy) {
        Ok(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    kept.remove("session_uri");
    if let Some(Value::Array(outputs)) = kept.get_mut("outputs") {
        for output in outputs.iter_mut() {
            if let Value::Object(map) = output {
                map.remove("session_uri");
            }
        }
    }
    serde_json::to_string(&Value::Object(kept)).unwrap_or_default()
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > INPUT_BOUND {
        return Err("sync store input exceeds its byte bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid sync store request: {error}"))?;
    let transition = apply(&request)?;
    serde_json::to_string(&json!(transition)).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::EngineRelease;

    const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    fn scope() -> Scope {
        Scope::Project {
            project: "p".into(),
        }
    }
    fn fence() -> Fence {
        Fence {
            account: "acct".into(),
            deployment: "stable@gateway".into(),
            install_id: "install-1".into(),
        }
    }
    fn ident(op: &str) -> Identity {
        Identity {
            engine: "network_reporter".into(),
            operation: op.into(),
            variant: "default".into(),
        }
    }
    fn held(op: &str, key: &str) -> ArtifactRow {
        ArtifactRow {
            scope: scope(),
            identity: ident(op),
            sha256: SHA.into(),
            size_bytes: 10,
            produced_at_ms: 1,
            base_revision: None,
            engine_release: "r".into(),
            engine_build_manifest_sha256: None,
            grant_engine: None,
            resource: None,
            client_publish_id: key.into(),
            outputs: vec![ArtifactOutput {
                output_id: "xlsx".into(),
                format: "xlsx".into(),
                content_type: "application/octet-stream".into(),
                sha256: SHA.into(),
                size_bytes: 10,
            }],
            bytes_locator: "loc".into(),
            readable: true,
            state: ArtifactState::Held,
            state_reason: None,
            replay_key: key.into(),
            updated_at_ms: 1,
            transfer_state: None,
        }
    }
    fn receipt(action: &str, outcome: &str, op: &str) -> ReceiptRow {
        ReceiptRow {
            receipt_id: format!("{action}:{outcome}:{op}"),
            scope: scope(),
            action: action.into(),
            identity: Some(ident(op)),
            outcome: outcome.into(),
            detail: None,
            grant_id: None,
            work_id: None,
            head_revision: None,
            head_sha256: None,
            head_counter: None,
            committed_bytes: None,
            at_ms: 1_000,
        }
    }
    fn request(snapshot: Snapshot, event: Event) -> Request {
        Request {
            schema: SCHEMA.into(),
            fence: fence(),
            now_ms: 1_000,
            snapshot,
            event,
        }
    }
    fn artifacts(t: &Transition) -> Vec<&ArtifactRow> {
        t.changes
            .iter()
            .filter_map(|c| match c {
                RowChange::UpsertArtifact(row) => Some(row),
                _ => None,
            })
            .collect()
    }
    fn receipts(t: &Transition) -> Vec<&ReceiptRow> {
        t.changes
            .iter()
            .filter_map(|c| match c {
                RowChange::AppendReceipt(row) => Some(row),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_inventory_digest_is_order_free_and_matches_the_transcribed_algorithm() {
        let a = inventory_digest(&[("b".into(), "2".into()), ("a".into(), "1".into())]);
        let b = inventory_digest(&[("a".into(), "1".into()), ("b".into(), "2".into())]);
        assert_eq!(a, b);
        let mut hasher = Sha256::new();
        hasher.update(b"a\0" as &[u8]);
        hasher.update(b"1\n");
        hasher.update(b"b\0");
        hasher.update(b"2\n");
        assert_eq!(a, format!("{:x}", hasher.finalize()));
    }

    #[test]
    fn a_produced_artifact_is_held_once_and_plans_a_local_change() {
        let t = apply(&request(
            Snapshot::default(),
            Event::LocalProduced {
                row: held("a", "k1"),
            },
        ))
        .unwrap();
        assert!(
            matches!(&t.changes[0], RowChange::UpsertArtifact(row) if row.state == ArtifactState::Held && row.replay_key == "k1")
        );
        assert_eq!(t.plan_trigger, Some(Trigger::LocalChange));
        let mut published = held("a", "k1");
        published.state = ArtifactState::Published;
        let again = apply(&request(
            Snapshot {
                artifacts: vec![published],
                ..Default::default()
            },
            Event::LocalProduced {
                row: held("a", "k1"),
            },
        ))
        .unwrap();
        assert!(again.changes.is_empty());
        assert_eq!(again.refusals[0].code, "already_recorded");
        // A new publication of the same identity supersedes the old row.
        let mut newer = held("a", "k2");
        newer.sha256 = "b".repeat(64);
        let superseded = apply(&request(
            Snapshot {
                artifacts: vec![held("a", "k1")],
                ..Default::default()
            },
            Event::LocalProduced { row: newer },
        ))
        .unwrap();
        assert!(
            matches!(&superseded.changes[0], RowChange::UpsertArtifact(row) if row.replay_key == "k2")
        );
        // A replay key never names two identities.
        let clash = apply(&request(
            Snapshot {
                artifacts: vec![held("a", "k1")],
                ..Default::default()
            },
            Event::LocalProduced {
                row: held("b", "k1"),
            },
        ))
        .unwrap();
        assert_eq!(clash.refusals[0].code, "replay_key_taken");
    }

    #[test]
    fn a_committed_upload_publishes_the_row_and_carries_the_head() {
        let mut r = receipt("upload", "committed", "a");
        r.head_revision = Some("rev2".into());
        let t = apply(&request(
            Snapshot {
                artifacts: vec![held("a", "k1")],
                ..Default::default()
            },
            Event::ActionReceipt { receipt: r },
        ))
        .unwrap();
        assert!(
            matches!(&t.changes[1], RowChange::UpsertArtifact(row) if row.state == ArtifactState::Published)
        );
        assert!(
            matches!(&t.changes[2], RowChange::UpsertHead(head) if head.revision == "rev2" && head.read_at_ms == 1_000 && head.sha256 == SHA && head.outputs.len() == 1)
        );
        assert_eq!(t.plan_trigger, Some(Trigger::PublishCompleted));
        // A failed attempt keeps the work held with its reason.
        let mut failed = receipt("upload", "failed", "a");
        failed.detail = Some("the gateway answered 503".into());
        let t = apply(&request(
            Snapshot {
                artifacts: vec![held("a", "k1")],
                ..Default::default()
            },
            Event::ActionReceipt { receipt: failed },
        ))
        .unwrap();
        assert!(
            matches!(&t.changes[1], RowChange::UpsertArtifact(row) if row.state == ArtifactState::Held && row.state_reason.as_deref() == Some("the gateway answered 503"))
        );
    }

    #[test]
    fn heads_read_replaces_the_scope_and_stamps_when() {
        let head = HeadRow {
            scope: Scope::Personal,
            identity: ident("a"),
            revision: "r1".into(),
            head_revision: 4,
            sha256: SHA.into(),
            published_at_ms: 5,
            outputs: vec![],
            read_at_ms: 0,
        };
        let t = apply(&request(
            Snapshot::default(),
            Event::HeadsRead {
                scope: scope(),
                heads: vec![head],
            },
        ))
        .unwrap();
        assert!(
            matches!(&t.changes[0], RowChange::ReplaceHeads { scope: s, heads, read_at_ms } if *s == scope() && *read_at_ms == 1_000 && heads[0].read_at_ms == 1_000 && heads[0].scope == scope())
        );
    }

    #[test]
    fn one_lease_per_scope_and_two_workers_never_share_it() {
        let taken = apply(&request(
            Snapshot::default(),
            Event::LeaseTake {
                scope: scope(),
                worker_id: "w1".into(),
                ttl_ms: 60_000,
            },
        ))
        .unwrap();
        let RowChange::SetLease(lease) = &taken.changes[0] else {
            panic!("lease")
        };
        assert_eq!(lease.expires_at_ms, 61_000);
        let snapshot = Snapshot {
            lease: Some(lease.clone()),
            ..Default::default()
        };
        let other = apply(&request(
            snapshot,
            Event::LeaseTake {
                scope: scope(),
                worker_id: "w2".into(),
                ttl_ms: 60_000,
            },
        ))
        .unwrap();
        assert_eq!(other.refusals[0].code, "lease_held");
        let mut dead = lease.clone();
        dead.expires_at_ms = 999;
        let retaken = apply(&request(
            Snapshot {
                lease: Some(dead),
                ..Default::default()
            },
            Event::LeaseTake {
                scope: scope(),
                worker_id: "w2".into(),
                ttl_ms: 60_000,
            },
        ))
        .unwrap();
        assert!(matches!(&retaken.changes[0], RowChange::SetLease(l) if l.worker_id == "w2"));
        let renew_dead = apply(&request(
            Snapshot::default(),
            Event::LeaseRenew {
                scope: scope(),
                worker_id: "w2".into(),
                ttl_ms: 60_000,
            },
        ))
        .unwrap();
        assert_eq!(renew_dead.refusals[0].code, "lease_expired");
        assert!(apply(&request(
            Snapshot::default(),
            Event::LeaseTake {
                scope: scope(),
                worker_id: "w1".into(),
                ttl_ms: LEASE_TTL_MAX_MS + 1
            }
        ))
        .is_err());
    }

    #[test]
    fn a_grant_for_another_install_is_refused_and_a_newer_one_supersedes() {
        let grant = |install: &str, id: &str| GrantRow {
            scope: scope(),
            grant_id: id.into(),
            install_id: install.into(),
            engines: vec![EngineRelease {
                name: "network_reporter".into(),
                release: "r".into(),
                manifest_sha256: None,
            }],
            expires_at_ms: 9_999,
            raw: json!({"signed": true}),
        };
        let wrong = apply(&request(
            Snapshot::default(),
            Event::GrantOpened {
                grant: grant("other", "g"),
            },
        ))
        .unwrap();
        assert_eq!(wrong.refusals[0].code, "grant_scope_mismatch");
        let t = apply(&request(
            Snapshot {
                grants: vec![grant("install-1", "g0")],
                ..Default::default()
            },
            Event::GrantOpened {
                grant: grant("install-1", "g1"),
            },
        ))
        .unwrap();
        assert!(matches!(&t.changes[0], RowChange::DropGrant { grant_id, .. } if grant_id == "g0"));
        assert!(matches!(&t.changes[1], RowChange::UpsertGrant(g) if g.grant_id == "g1"));
        assert_eq!(t.plan_trigger, Some(Trigger::GrantOpened));
    }

    #[test]
    fn cancel_refuses_held_work_and_never_undoes_committed_work() {
        let t = apply(&request(
            Snapshot {
                artifacts: vec![held("a", "k1")],
                ..Default::default()
            },
            Event::Cancel {
                scope: scope(),
                identity: ident("a"),
                expected_replay_key: None,
            },
        ))
        .unwrap();
        assert!(
            matches!(&t.changes[0], RowChange::UpsertArtifact(row) if row.state == ArtifactState::Refused && row.state_reason.as_deref() == Some("cancelled"))
        );
        assert_eq!(receipts(&t)[0].action, "cancel");
        let mut done = held("a", "k1");
        done.state = ArtifactState::Published;
        let t = apply(&request(
            Snapshot {
                artifacts: vec![done],
                ..Default::default()
            },
            Event::Cancel {
                scope: scope(),
                identity: ident("a"),
                expected_replay_key: None,
            },
        ))
        .unwrap();
        assert_eq!(t.refusals[0].code, "not_cancellable");
    }

    #[test]
    fn cancel_with_an_expected_replay_key_never_refuses_a_replacement_row() {
        let t = apply(&request(
            Snapshot {
                artifacts: vec![held("a", "new-job")],
                ..Default::default()
            },
            Event::Cancel {
                scope: scope(),
                identity: ident("a"),
                expected_replay_key: Some("old-job".into()),
            },
        ))
        .unwrap();
        assert_eq!(t.refusals[0].code, "publication_replaced");
        assert!(t.changes.is_empty(), "replacement stays held");

        let matching = apply(&request(
            Snapshot {
                artifacts: vec![held("a", "new-job")],
                ..Default::default()
            },
            Event::Cancel {
                scope: scope(),
                identity: ident("a"),
                expected_replay_key: Some("new-job".into()),
            },
        ))
        .unwrap();
        assert!(matches!(
            &matching.changes[0],
            RowChange::UpsertArtifact(row)
                if row.state == ArtifactState::Refused
                    && row.state_reason.as_deref() == Some("cancelled")
        ));
    }

    fn legacy(id: &str, project: &str, status: &str, phase: &str, outputs: Value) -> Value {
        json!({
            "kind": "compute_artifact", "id": id, "owner_uid": "u", "project_id": project, "root": "/r",
            "client_run_id": "run-1", "engine": "network_reporter", "operation": id, "variant": "default",
            "engine_version": "r", "engine_build_manifest_sha256": SHA, "input_base_fingerprint": "base-9",
            "client_publish_id": format!("pub-{id}"), "status": status, "phase": phase, "attempts": 1,
            "created_at": 5, "updated_at": 6, "synced_at": null, "error": "The producing engine build is not admitted.",
            "failure_code": "ENGINE_BUILD_NOT_ADMITTED", "outputs": outputs
        })
    }

    #[test]
    fn the_legacy_handoff_keeps_every_row_verifies_bytes_and_names_what_it_could_not_map() {
        let output = json!({"output_id": "xlsx", "local_path": "/x", "format": "xlsx", "content_type": "application/x", "sha256": SHA, "size_bytes": 7, "state": "pending", "session_uri": "https://storage/secret-session"});
        let rows: Vec<LegacyHandoff> = serde_json::from_value(json!([
            {"row": legacy("report-1", "p", "failed", "pending", json!([output.clone()])), "bytes": {"kind": "verified", "locator": "batch:pub-report-1"}},
            {"row": legacy("report-2", "p", "synced", "published", json!([output.clone()])), "bytes": {"kind": "verified", "locator": "batch:pub-report-2"}},
            {"row": legacy("report-3", "p", "pending", "pending", json!([output.clone()])), "bytes": {"kind": "missing", "reason": "no committed batch names pub-report-3"}},
            {"row": legacy("orphan", "", "pending", "pending", json!([])), "bytes": {"kind": "missing", "reason": "no project"}}
        ]))
        .unwrap();
        let t = apply(&request(Snapshot::default(), Event::HandoffLegacy { rows })).unwrap();
        let rows = artifacts(&t);
        let evidence = receipts(&t);
        assert_eq!(rows.len(), 4);
        assert_eq!(evidence.len(), 4);
        assert_eq!(rows[0].state, ArtifactState::Held);
        assert_eq!(
            rows[0].state_reason.as_deref(),
            Some("The producing engine build is not admitted.")
        );
        assert_eq!(rows[0].bytes_locator, "batch:pub-report-1");
        assert_eq!(rows[0].replay_key, "pub-report-1");
        assert_eq!(rows[0].base_revision.as_deref(), Some("base-9"));
        assert_eq!(
            rows[0].sha256,
            inventory_digest(&[("xlsx".into(), SHA.into())])
        );
        assert_eq!(rows[1].state, ArtifactState::Published);
        assert_eq!(rows[2].state, ArtifactState::Refused);
        assert!(rows[2]
            .state_reason
            .as_deref()
            .unwrap()
            .starts_with("handoff_bytes_missing: no committed batch"));
        assert!(!rows[2].readable);
        assert_eq!(rows[3].state, ArtifactState::Refused);
        assert!(rows[3]
            .state_reason
            .as_deref()
            .unwrap()
            .starts_with("handoff_unmapped"));
        assert_eq!(evidence[0].outcome, "imported");
        assert_eq!(evidence[2].outcome, "bytes_missing");
        assert_eq!(evidence[3].outcome, "unmapped");
        let kept = evidence[0].detail.as_deref().unwrap();
        assert!(kept.contains("ENGINE_BUILD_NOT_ADMITTED") && kept.contains("\"attempts\":1"));
        assert!(
            !kept.contains("secret-session"),
            "a scoped upload credential never becomes evidence"
        );
        assert_eq!(t.plan_trigger, Some(Trigger::Startup));
        // Re-running the handoff imports nothing twice — by publish id or by legacy id.
        let snapshot = Snapshot {
            artifacts: rows.into_iter().cloned().collect(),
            ..Default::default()
        };
        let rows: Vec<LegacyHandoff> = serde_json::from_value(json!([
            {"row": legacy("report-1", "p", "failed", "pending", json!([])), "bytes": {"kind": "verified", "locator": "x"}},
            {"row": legacy("orphan", "", "pending", "pending", json!([])), "bytes": {"kind": "missing", "reason": "no project"}}
        ]))
        .unwrap();
        let again = apply(&request(snapshot, Event::HandoffLegacy { rows })).unwrap();
        assert!(again.changes.is_empty());
    }

    #[test]
    fn the_entry_point_is_closed_and_fenced() {
        let ok = evaluate(
            serde_json::to_string(&json!({"schema": SCHEMA, "fence": {"account": "a", "deployment": "d", "install_id": "i"}, "now_ms": 1,
                "event": {"kind": "lease_release", "scope": {"kind": "project", "project": "p"}, "worker_id": "w"}}))
            .unwrap()
            .as_bytes(),
        )
        .unwrap();
        assert!(ok.contains("\"changes\":[]"));
        assert!(evaluate(br#"{"schema":"ds.sync-store/v1","fence":{"account":"","deployment":"d","install_id":"i"},"now_ms":1,"event":{"kind":"lease_release","scope":{"kind":"personal"},"worker_id":"w"}}"#).is_err());
        assert!(evaluate(br#"{"schema":"ds.sync-store/v1","fence":{"account":"a","deployment":"d","install_id":"i"},"now_ms":1,"event":{"kind":"lease_release","scope":{"kind":"personal"},"worker_id":"w"},"extra":1}"#).is_err());
    }
}
