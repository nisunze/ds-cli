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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub paper_size: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presentation: Option<serde_json::Value>,
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
    /// The source-input digest that produced this derived publication. This
    /// is independent of the output head revision used for OCC.
    #[serde(default)]
    pub input_base_fingerprint: Option<String>,
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
    /// The exact work acknowledged for this publication, never inferred from
    /// historical identity-only receipts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acknowledged_work_id: Option<String>,
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
    /// Idempotency across attempts.
    pub replay_key: String,
    pub updated_at_ms: u64,
    /// The transport's own persisted state for an in-flight transfer.
    #[serde(default)]
    pub transfer_state: Option<Value>,
}

impl ArtifactRow {
    /// Equal output bytes are reusable only with the same captured inputs
    /// and producer build. Every registered engine asks this same question.
    pub fn matches_publication(
        &self,
        inventory_sha256: &str,
        input_base_fingerprint: &str,
        engine_release: &str,
        engine_build_manifest_sha256: &str,
    ) -> bool {
        !crate::sync::engines::engine(&self.identity.engine)
            .is_some_and(|engine| engine.refreshes_equal_bytes(&self.identity.variant))
            && !input_base_fingerprint.is_empty()
            && !engine_release.is_empty()
            && !engine_build_manifest_sha256.is_empty()
            && self.sha256 == inventory_sha256
            && self.publication_input_base() == Some(input_base_fingerprint)
            && self.engine_release == engine_release
            && self.engine_build_manifest_sha256.as_deref() == Some(engine_build_manifest_sha256)
    }
    pub fn publication_input_base(&self) -> Option<&str> {
        self.input_base_fingerprint.as_deref().or_else(|| {
            crate::sync::engines::engine(&self.identity.engine)
                .is_some_and(|engine| engine.legacy_source_in_base)
                .then(|| self.base_revision.as_deref())
                .flatten()
        })
    }
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
    /// The source-input digest sealed by this published head, when the engine
    /// derives outputs from an independently versioned source model.
    #[serde(default)]
    pub input_base_fingerprint: Option<String>,
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
    /// The authority's exact typed refusal, independent of display text.
    /// Older receipts omit it; recovery must never infer a code from prose.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<String>,
    #[serde(default)]
    pub grant_id: Option<String>,
    #[serde(default)]
    pub work_id: Option<String>,
    /// The selected local publication this action actually attempted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_publish_id: Option<String>,
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
                let same_publication = row
                    .publication_input_base()
                    .zip(row.engine_build_manifest_sha256.as_deref())
                    .is_some_and(|(inputs, build)| {
                        existing.matches_publication(
                            &row.sha256,
                            inputs,
                            &row.engine_release,
                            build,
                        )
                    });
                if same_publication || existing.replay_key == row.replay_key {
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
            next.acknowledged_work_id = None;
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
            // An operation slot may now hold B while an attempt on A is in
            // flight. Keep that receipt as evidence without restating B.
            if matches!(
                receipt.action.as_str(),
                "upload" | "conflict" | "refused" | "reclaimed"
            ) {
                let Some(row) = existing else {
                    t.refuse(
                        "publication_missing",
                        "the receipt's publication is no longer held",
                    );
                    return Ok(t);
                };
                if receipt
                    .client_publish_id
                    .as_deref()
                    .filter(|id| !id.is_empty())
                    != Some(row.client_publish_id.as_str())
                {
                    t.refuse(
                        "publication_replaced",
                        "the receipt does not bind the current publication",
                    );
                    return Ok(t);
                }
                if receipt.action != "reclaimed"
                    && row.state == ArtifactState::Refused
                    && row.state_reason.as_deref() == Some("cancelled")
                {
                    t.refuse(
                        "publication_cancelled",
                        "the receipt cannot undo cancellation",
                    );
                    return Ok(t);
                }
                if receipt.action == "upload"
                    && receipt.outcome == "committed"
                    && (receipt.work_id.as_deref().is_none_or(str::is_empty)
                        || receipt
                            .head_sha256
                            .as_deref()
                            .is_some_and(|sha| sha != row.sha256))
                {
                    t.refuse(
                        "publication_ack_unbound",
                        "a committed receipt needs its exact work and output pins",
                    );
                    return Ok(t);
                }
            }
            let mut restate = |state: ArtifactState, reason: Option<String>| {
                if let Some(row) = existing {
                    let mut next = row.clone();
                    next.state = state;
                    next.acknowledged_work_id = (receipt.action == "upload"
                        && receipt.outcome == "committed")
                        .then(|| receipt.work_id.clone())
                        .flatten();
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
                                input_base_fingerprint: row.input_base_fingerprint.clone(),
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
                ("reclaimed", _) => {
                    // The producer freed the bytes of a row that lost (or
                    // found them already gone). The verdict and the row
                    // stay as evidence; what changes is that nothing on this
                    // machine holds the bytes any more, so the queue stops
                    // counting them as reclaimable and no pass frees them
                    // twice. A `reclaim_failed` receipt leaves the row as it
                    // is: the bytes are still there, and the next pass
                    // tries again in the open.
                    if let Some(row) = existing {
                        let mut next = row.clone();
                        next.readable = false;
                        next.bytes_locator = String::new();
                        next.transfer_state = None;
                        next.updated_at_ms = now;
                        t.changes.push(RowChange::UpsertArtifact(next));
                    }
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
            Some(row) => t.refuse(
                "not_cancellable",
                format!("{} work is never undone", row.state.as_str()),
            ),
            None => t.refuse("unknown_identity", identity.operation.clone()),
        },
    }
    Ok(t)
}

// ---------------------------------------------------------------------------
// The queue, read from the rows
// ---------------------------------------------------------------------------

/// How long queued work may wait with no live lease on its project before
/// the status calls it stuck. A healthy pump publishes within minutes of a
/// seal; an hour with nobody holding the project means nothing is pumping —
/// the desktop and the Server are both closed, or their pump is not running —
/// and that is something an operator has to do, which is what `stuck` means.
pub const ABANDONED_AFTER_MS: u64 = 60 * 60 * 1000;

/// How many room names one project's row carries before it is summarised.
pub const MAX_REPORTED_TRANSFORMERS: usize = 20;

/// How many distinct hold reasons one project's row carries.
pub const MAX_REPORTED_REASONS: usize = 5;

/// One project's rows, as the queue itself holds them, split by state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueuedProject {
    pub project_id: String,
    /// Rows still waiting to publish (`held` or `uploading`) — the only ones
    /// a pump will move.
    pub batches: usize,
    pub bytes: u64,
    pub oldest_produced_at_ms: u64,
    /// How long the oldest QUEUED row has been waiting.
    pub oldest_age_ms: u64,
    /// The rooms this project holds queued work for, oldest first, bounded.
    pub transformers: Vec<String>,
    /// Why queued rows are still here, when a receipt said: distinct
    /// `state_reason`s of the queued rows, bounded. Empty means nothing has
    /// refused them yet — they are simply waiting for a pass.
    pub reasons: Vec<String>,
    /// Rows whose bytes ARE the room's head: this machine's readable copy of
    /// what it published. Not queued, not reclaimable — they stay.
    pub held_batches: usize,
    pub held_bytes: u64,
    /// Rows that lost — a conflict or a refusal the store holds as terminal —
    /// whose bytes are still on this machine. They will never publish; the
    /// next pass frees them.
    pub reclaimable_batches: usize,
    pub reclaimable_bytes: u64,
    /// Whether a live lease is held on this project right now: a worker is
    /// pumping it.
    pub pumped: bool,
    /// A credential refusal is a session blocker while this project has
    /// queued rows. Derived from receipts; never persisted on an artifact.
    pub blocked: Option<QueueBlock>,
}

/// Output-only verdict over a held queue and its latest delivery receipt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueBlock {
    pub cause: String,
    pub since_ms: u64,
    pub detail: String,
}

/// What `ds report outbox status` answers, decided here from the store's own
/// rows and leases. Nothing on disk is enumerated to answer it: the store is
/// the queue, and the bytes directories are bytes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueStatus {
    /// Only what is QUEUED: `held` or `uploading`.
    pub queued_batches: usize,
    pub queued_bytes: u64,
    /// Over queued rows only.
    pub oldest_age_ms: u64,
    /// Published copies of the rooms' heads, kept readable on this machine.
    pub held_batches: usize,
    pub held_bytes: u64,
    /// Rows that lost and still hold bytes here: freed by the next pass.
    pub reclaimable_batches: usize,
    pub reclaimable_bytes: u64,
    pub projects: Vec<QueuedProject>,
    /// The live leases under this fence at `now_ms`: who is pumping what.
    pub leases: Vec<LeaseRow>,
    /// The first blocked queued project, if any. Each project also carries
    /// its own verdict.
    pub blocked: Option<QueueBlock>,
    /// Whether an operator has to do something. A non-empty queue is not
    /// stuck merely because it is waiting. A refused credential blocks it
    /// immediately; an unpumped queue becomes abandoned after
    /// [`ABANDONED_AFTER_MS`].
    pub stuck: bool,
    /// The one command to run next, always stated.
    pub next: &'static str,
}

/// The room a row is about, for a reader: the grant resource where the
/// producer named one, else the operation with the engine's `export-` prefix
/// removed, else the operation itself.
fn room_of(row: &ArtifactRow) -> String {
    match row.resource.as_deref().filter(|value| !value.is_empty()) {
        Some(resource) => resource.to_string(),
        None => row
            .identity
            .operation
            .strip_prefix("export-")
            .unwrap_or(&row.identity.operation)
            .to_string(),
    }
}

/// Read the queue: what is sealed and waiting, how old it is, what has
/// settled and stays or goes, and who is pumping — from the rows and leases
/// of one fence, at `now_ms`. Pure: the host reads the rows, this counts.
///
/// `project` narrows to one project; `None` reads every project the fence
/// holds rows for.
pub fn queue_status(
    rows: &[ArtifactRow],
    leases: &[LeaseRow],
    receipts: &[ReceiptRow],
    project: Option<&str>,
    now_ms: u64,
) -> QueueStatus {
    let live: Vec<LeaseRow> = leases
        .iter()
        .filter(|lease| lease.expires_at_ms > now_ms)
        .filter(|lease| project.is_none_or(|wanted| lease.scope.project() == wanted))
        .cloned()
        .collect();
    let mut projects: Vec<QueuedProject> = Vec::new();
    for row in rows {
        let Scope::Project {
            project: project_id,
        } = &row.scope
        else {
            continue;
        };
        if project.is_some_and(|wanted| wanted != project_id) {
            continue;
        }
        let entry = match projects
            .iter_mut()
            .find(|entry| &entry.project_id == project_id)
        {
            Some(entry) => entry,
            None => {
                projects.push(QueuedProject {
                    project_id: project_id.clone(),
                    batches: 0,
                    bytes: 0,
                    oldest_produced_at_ms: 0,
                    oldest_age_ms: 0,
                    transformers: Vec::new(),
                    reasons: Vec::new(),
                    held_batches: 0,
                    held_bytes: 0,
                    reclaimable_batches: 0,
                    reclaimable_bytes: 0,
                    pumped: live.iter().any(|lease| lease.scope.project() == project_id),
                    blocked: None,
                });
                projects.last_mut().expect("just pushed")
            }
        };
        match row.state {
            ArtifactState::Held | ArtifactState::Uploading => {
                entry.batches += 1;
                entry.bytes = entry.bytes.saturating_add(row.size_bytes);
                if row.produced_at_ms > 0
                    && (entry.oldest_produced_at_ms == 0
                        || row.produced_at_ms < entry.oldest_produced_at_ms)
                {
                    entry.oldest_produced_at_ms = row.produced_at_ms;
                }
                let room = room_of(row);
                if entry.transformers.len() < MAX_REPORTED_TRANSFORMERS
                    && !entry.transformers.contains(&room)
                {
                    entry.transformers.push(room);
                }
                if let Some(reason) = row.state_reason.as_deref().filter(|r| !r.is_empty()) {
                    if entry.reasons.len() < MAX_REPORTED_REASONS
                        && !entry.reasons.iter().any(|held| held == reason)
                    {
                        entry.reasons.push(reason.to_string());
                    }
                }
            }
            ArtifactState::Published => {
                entry.held_batches += 1;
                entry.held_bytes = entry.held_bytes.saturating_add(row.size_bytes);
            }
            ArtifactState::Conflict | ArtifactState::Refused => {
                // Reclaimable only while the bytes are still here: a
                // `reclaimed` receipt clears the locator and the readable
                // flag. A row without readable bytes holds nothing to free.
                if row.readable && !row.bytes_locator.is_empty() {
                    entry.reclaimable_batches += 1;
                    entry.reclaimable_bytes =
                        entry.reclaimable_bytes.saturating_add(row.size_bytes);
                }
            }
        }
    }
    for entry in &mut projects {
        if entry.oldest_produced_at_ms > 0 {
            entry.oldest_age_ms = now_ms.saturating_sub(entry.oldest_produced_at_ms);
        }
        // The caller supplies newest-first delivery receipts from the same
        // fence and scope as the rows. Clock values are for display only:
        // ordering is the store's append sequence, not two unrelated clocks.
        if entry.batches > 0 {
            if let Some(receipt) = receipts.iter().find(|receipt| {
                receipt.scope.project() == entry.project_id
                    && matches!(receipt.action.as_str(), "upload" | "download" | "session")
            }) {
                if receipt.outcome == "credential_refused" {
                    entry.blocked = Some(QueueBlock {
                        cause: "credential_refused".into(),
                        since_ms: receipt.at_ms,
                        detail: receipt.detail.clone().unwrap_or_else(|| {
                            "the gateway refused this machine's credential (401)".into()
                        }),
                    });
                }
            }
        }
    }
    projects.sort_by(|left, right| {
        right
            .oldest_age_ms
            .cmp(&left.oldest_age_ms)
            .then_with(|| left.project_id.cmp(&right.project_id))
    });
    let queued_batches: usize = projects.iter().map(|project| project.batches).sum();
    let reclaimable_batches: usize = projects
        .iter()
        .map(|project| project.reclaimable_batches)
        .sum();
    let oldest_age_ms = projects
        .iter()
        .map(|project| project.oldest_age_ms)
        .max()
        .unwrap_or(0);
    // Queued work nobody is pumping: no live lease on a project whose
    // oldest queued row has waited longer than the abandonment bound.
    let abandoned = projects.iter().any(|project| {
        project.batches > 0 && !project.pumped && project.oldest_age_ms > ABANDONED_AFTER_MS
    });
    let blocked = projects.iter().find_map(|project| project.blocked.clone());
    QueueStatus {
        queued_batches,
        queued_bytes: projects
            .iter()
            .fold(0_u64, |total, project| total.saturating_add(project.bytes)),
        oldest_age_ms,
        held_batches: projects.iter().map(|project| project.held_batches).sum(),
        held_bytes: projects.iter().fold(0_u64, |total, project| {
            total.saturating_add(project.held_bytes)
        }),
        reclaimable_batches,
        reclaimable_bytes: projects.iter().fold(0_u64, |total, project| {
            total.saturating_add(project.reclaimable_bytes)
        }),
        next: if blocked.is_some() {
            "sign in again in DS GridDesign Desktop, or run `ds account connect` for CLI and Server; the queue resumes automatically"
        } else if abandoned {
            "no publisher holds this queue — start DS GridDesign or the DS Server, which pumps it, or run `ds report outbox drain`"
        } else if queued_batches == 0 && reclaimable_batches == 0 {
            "nothing is queued on this machine"
        } else {
            "ds report outbox drain"
        },
        stuck: blocked.is_some() || abandoned,
        blocked,
        projects,
        leases: live,
    }
}

/// The identity-free machine diagnostic combines separately fenced queue
/// readings. A credential verdict from one fence never clears another
/// fence's refusal merely because both used the same project id.
pub fn combine_queue_statuses(statuses: &[QueueStatus]) -> QueueStatus {
    let mut projects: Vec<QueuedProject> = statuses
        .iter()
        .flat_map(|status| status.projects.iter().cloned())
        .collect();
    projects.sort_by(|left, right| {
        right
            .oldest_age_ms
            .cmp(&left.oldest_age_ms)
            .then_with(|| left.project_id.cmp(&right.project_id))
    });
    let blocked = projects.iter().find_map(|project| project.blocked.clone());
    let abandoned = statuses
        .iter()
        .any(|status| status.stuck && status.blocked.is_none());
    let queued_batches = statuses.iter().map(|status| status.queued_batches).sum();
    let reclaimable_batches = statuses
        .iter()
        .map(|status| status.reclaimable_batches)
        .sum();
    QueueStatus {
        queued_batches,
        queued_bytes: statuses.iter().fold(0_u64, |total, status| {
            total.saturating_add(status.queued_bytes)
        }),
        oldest_age_ms: statuses
            .iter()
            .map(|status| status.oldest_age_ms)
            .max()
            .unwrap_or(0),
        held_batches: statuses.iter().map(|status| status.held_batches).sum(),
        held_bytes: statuses.iter().fold(0_u64, |total, status| {
            total.saturating_add(status.held_bytes)
        }),
        reclaimable_batches,
        reclaimable_bytes: statuses.iter().fold(0_u64, |total, status| {
            total.saturating_add(status.reclaimable_bytes)
        }),
        projects,
        leases: statuses
            .iter()
            .flat_map(|status| status.leases.iter().cloned())
            .collect(),
        blocked: blocked.clone(),
        stuck: blocked.is_some() || abandoned,
        next: if blocked.is_some() {
            "sign in again in DS GridDesign Desktop, or run `ds account connect` for CLI and Server; the queue resumes automatically"
        } else if abandoned {
            "no publisher holds this queue — start DS GridDesign or the DS Server, which pumps it, or run `ds report outbox drain`"
        } else if queued_batches == 0 && reclaimable_batches == 0 {
            "nothing is queued on this machine"
        } else {
            "ds report outbox drain"
        },
    }
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
            input_base_fingerprint: None,
            engine_release: "r".into(),
            engine_build_manifest_sha256: None,
            grant_engine: None,
            resource: None,
            client_publish_id: key.into(),
            acknowledged_work_id: None,
            outputs: vec![ArtifactOutput {
                filename: None,
                paper_size: None,
                presentation: None,
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
            failure_code: None,
            grant_id: None,
            work_id: None,
            client_publish_id: Some("k1".into()),
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
    fn explicit_preview_refresh_keeps_new_run_even_when_pdf_bytes_match() {
        let mut prior = held("a", "k1");
        prior.identity.variant = crate::printing::retained_preview::VARIANT.into();
        prior.state = ArtifactState::Published;
        let mut refreshed = prior.clone();
        refreshed.replay_key = "k2".into();
        refreshed.client_publish_id = "k2".into();
        let transition = apply(&request(
            Snapshot {
                artifacts: vec![prior],
                ..Default::default()
            },
            Event::LocalProduced {
                row: refreshed.clone(),
            },
        ))
        .unwrap();
        assert!(transition.refusals.is_empty());
        assert_eq!(artifacts(&transition)[0].replay_key, "k2");
        let retry = apply(&request(
            Snapshot {
                artifacts: vec![refreshed.clone()],
                ..Default::default()
            },
            Event::LocalProduced { row: refreshed },
        ))
        .unwrap();
        assert_eq!(retry.refusals[0].code, "already_recorded");
    }

    #[test]
    fn both_engines_equal_bytes_require_matching_input_and_producer_provenance() {
        for engine in crate::sync::engines::ENGINES {
            let mut prior = held("export-tx_1", "k1");
            prior.identity.engine = engine.name.into();
            prior.input_base_fingerprint = Some("b".repeat(64));
            prior.engine_build_manifest_sha256 = Some("c".repeat(64));
            prior.state = ArtifactState::Published;
            let mut candidate = prior.clone();
            candidate.replay_key = "k2".into();
            candidate.client_publish_id = "k2".into();
            let transition = |row| {
                apply(&request(
                    Snapshot {
                        artifacts: vec![prior.clone()],
                        ..Default::default()
                    },
                    Event::LocalProduced { row },
                ))
                .unwrap()
            };
            let duplicate = transition(candidate.clone());
            assert!(duplicate.changes.is_empty());
            assert_eq!(duplicate.refusals[0].code, "already_recorded");

            for changed in [
                "inputs",
                "engine",
                "build",
                "missing_inputs",
                "missing_build",
            ] {
                let mut row = candidate.clone();
                match changed {
                    "inputs" => row.input_base_fingerprint = Some("d".repeat(64)),
                    "engine" => row.engine_release = "new-release".into(),
                    "build" => row.engine_build_manifest_sha256 = Some("e".repeat(64)),
                    "missing_inputs" => row.input_base_fingerprint = None,
                    "missing_build" => row.engine_build_manifest_sha256 = None,
                    _ => unreachable!(),
                }
                let refreshed = transition(row);
                assert!(
                    refreshed.refusals.is_empty(),
                    "{changed}: {:?}",
                    refreshed.refusals
                );
                let rows = artifacts(&refreshed);
                assert_eq!(rows.len(), 1, "{changed}");
                assert_eq!(rows[0].replay_key, "k2", "{changed}");
                assert_eq!(rows[0].state, ArtifactState::Held, "{changed}");
            }
        }
    }

    #[test]
    fn solar_equal_bytes_with_new_provenance_record_a_fresh_publication() {
        let mut prior = held("export-city", "k1");
        prior.identity.engine = "solar".into();
        let mut candidate = prior.clone();
        candidate.replay_key = "k2".into();
        candidate.client_publish_id = "k2".into();
        candidate.input_base_fingerprint = Some("b".repeat(64));
        candidate.engine_release = "new-release".into();
        candidate.engine_build_manifest_sha256 = Some("c".repeat(64));
        let duplicate = apply(&request(
            Snapshot {
                artifacts: vec![prior],
                ..Default::default()
            },
            Event::LocalProduced { row: candidate },
        ))
        .unwrap();
        assert!(duplicate.refusals.is_empty());
        assert_eq!(artifacts(&duplicate)[0].replay_key, "k2");
    }

    #[test]
    fn a_committed_upload_publishes_the_row_and_carries_the_head() {
        let mut r = receipt("upload", "committed", "a");
        r.head_revision = Some("rev2".into());
        r.work_id = Some("work-k1".into());
        let t = apply(&request(
            Snapshot {
                artifacts: vec![held("a", "k1")],
                ..Default::default()
            },
            Event::ActionReceipt { receipt: r },
        ))
        .unwrap();
        assert!(
            matches!(&t.changes[1], RowChange::UpsertArtifact(row) if row.state == ArtifactState::Published && row.acknowledged_work_id.as_deref() == Some("work-k1"))
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
    fn receipts_bind_exact_publications_and_never_revive_cancelled_or_historical_rows() {
        let current = held("a", "k2");
        let mut ack = receipt("upload", "committed", "a");
        ack.work_id = Some("work-k2".into());
        ack.head_revision = Some("rev2".into());
        let mut cancelled = current.clone();
        cancelled.state = ArtifactState::Refused;
        cancelled.state_reason = Some("cancelled".into());
        let mut unbound = ack.clone();
        unbound.client_publish_id = None;
        let mut current_ack = ack.clone();
        current_ack.client_publish_id = Some("k2".into());
        let mut no_work = current_ack.clone();
        no_work.work_id = None;
        let mut wrong_output = current_ack.clone();
        wrong_output.head_sha256 = Some("b".repeat(64));
        for (row, attempt, reason) in [
            (current.clone(), ack, "publication_replaced"),
            (current.clone(), unbound, "publication_replaced"),
            (cancelled, current_ack.clone(), "publication_cancelled"),
            (current.clone(), no_work, "publication_ack_unbound"),
            (current.clone(), wrong_output, "publication_ack_unbound"),
        ] {
            let transition = apply(&request(
                Snapshot {
                    artifacts: vec![row],
                    ..Default::default()
                },
                Event::ActionReceipt { receipt: attempt },
            ))
            .unwrap();
            assert_eq!(transition.refusals[0].code, reason);
            assert!(artifacts(&transition).is_empty());
            assert_eq!(
                receipts(&transition).len(),
                1,
                "attempt remains audit evidence"
            );
            assert!(!transition
                .changes
                .iter()
                .any(|change| matches!(change, RowChange::UpsertHead(_))));
            assert!(transition.plan_trigger.is_none());
        }
        let transition = apply(&request(
            Snapshot {
                artifacts: vec![current.clone()],
                ..Default::default()
            },
            Event::ActionReceipt {
                receipt: current_ack,
            },
        ))
        .unwrap();
        assert_eq!(
            artifacts(&transition)[0].acknowledged_work_id.as_deref(),
            Some("work-k2")
        );
        let mut cancelled = current.clone();
        cancelled.state = ArtifactState::Refused;
        cancelled.state_reason = Some("cancelled".into());
        let mut reclaimed = receipt("reclaimed", "cancelled", "a");
        reclaimed.client_publish_id = Some("k2".into());
        let transition = apply(&request(
            Snapshot {
                artifacts: vec![cancelled],
                ..Default::default()
            },
            Event::ActionReceipt { receipt: reclaimed },
        ))
        .unwrap();
        assert!(transition.refusals.is_empty());
        assert_eq!(
            artifacts(&transition)[0].state_reason.as_deref(),
            Some("cancelled")
        );
        assert!(!artifacts(&transition)[0].readable);

        let mut forged = current;
        forged.acknowledged_work_id = Some("producer-cannot-ack".into());
        let sealed = apply(&request(
            Snapshot::default(),
            Event::LocalProduced {
                row: forged.clone(),
            },
        ))
        .unwrap();
        assert!(artifacts(&sealed)[0].acknowledged_work_id.is_none());
        let mut historical = serde_json::to_value(forged).unwrap();
        historical
            .as_object_mut()
            .unwrap()
            .remove("acknowledged_work_id");
        let historical: ArtifactRow = serde_json::from_value(historical).unwrap();
        assert!(historical.acknowledged_work_id.is_none());
    }

    #[test]
    fn heads_read_replaces_the_scope_and_stamps_when() {
        let head = HeadRow {
            scope: Scope::Personal,
            identity: ident("a"),
            revision: "r1".into(),
            head_revision: 4,
            input_base_fingerprint: Some("source-1".into()),
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

    #[test]
    fn a_reclaimed_receipt_marks_the_bytes_gone_and_keeps_the_verdict() {
        // WHY: the queue counts a lost row as reclaimable only while its
        // bytes are here. Without this the pass that freed them would meet
        // the same row on every observation and either free it "again" or
        // report bytes that are not there.
        let mut lost = held("a", "k1");
        lost.state = ArtifactState::Conflict;
        lost.state_reason = Some("source_base_moved".into());
        let mut r = receipt("reclaimed", "conflict", "a");
        r.committed_bytes = Some(10);
        let t = apply(&request(
            Snapshot {
                artifacts: vec![lost.clone()],
                ..Default::default()
            },
            Event::ActionReceipt { receipt: r },
        ))
        .unwrap();
        let rows = artifacts(&t);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state, ArtifactState::Conflict, "the verdict stays");
        assert_eq!(
            rows[0].state_reason.as_deref(),
            Some("source_base_moved"),
            "and so does its reason"
        );
        assert!(!rows[0].readable);
        assert!(
            rows[0].bytes_locator.is_empty(),
            "nothing here holds the bytes"
        );
        // A failed reclaim leaves the row exactly as it was: still
        // reclaimable, retried next pass.
        let t = apply(&request(
            Snapshot {
                artifacts: vec![lost.clone()],
                ..Default::default()
            },
            Event::ActionReceipt {
                receipt: receipt("reclaim_failed", "conflict", "a"),
            },
        ))
        .unwrap();
        assert!(artifacts(&t).is_empty());
        assert_eq!(receipts(&t).len(), 1);
    }

    fn lease(project: &str, worker: &str, expires_at_ms: u64) -> LeaseRow {
        LeaseRow {
            scope: Scope::Project {
                project: project.into(),
            },
            worker_id: worker.into(),
            taken_at_ms: 0,
            expires_at_ms,
        }
    }

    fn row_in(
        project: &str,
        op: &str,
        key: &str,
        state: ArtifactState,
        produced_at_ms: u64,
    ) -> ArtifactRow {
        let mut row = held(op, key);
        row.scope = Scope::Project {
            project: project.into(),
        };
        row.state = state;
        row.produced_at_ms = produced_at_ms;
        row.resource = Some(op.strip_prefix("export-").unwrap_or(op).into());
        row
    }

    #[test]
    fn the_queue_counts_only_what_is_queued_and_names_what_stays_and_what_goes() {
        // WHY: 350 batches and 2.08 GB were reported queued on a machine
        // where 172 had published and 86 had lost. The store is the queue
        // now: held/uploading are queued, published copies stay, and a lost
        // row is reclaimable only while its bytes are still here.
        let now = 10 * ABANDONED_AFTER_MS;
        let mut gone = row_in("p", "export-tx_gone", "k4", ArtifactState::Refused, 1);
        gone.readable = false;
        gone.bytes_locator = String::new();
        let rows = vec![
            row_in(
                "p",
                "export-tx_queued",
                "k1",
                ArtifactState::Held,
                now - 1_000,
            ),
            row_in("p", "export-tx_head", "k2", ArtifactState::Published, 1),
            row_in("p", "export-tx_lost", "k3", ArtifactState::Conflict, 1),
            gone,
            row_in(
                "q",
                "export-tx_up",
                "k5",
                ArtifactState::Uploading,
                now - 5_000,
            ),
        ];
        let status = queue_status(&rows, &[], &[], None, now);
        assert_eq!(status.queued_batches, 2);
        assert_eq!(status.queued_bytes, 20);
        assert_eq!(status.held_batches, 1);
        assert_eq!(
            status.reclaimable_batches, 1,
            "a row whose bytes are gone is not reclaimable"
        );
        assert_eq!(status.reclaimable_bytes, 10);
        assert_eq!(status.oldest_age_ms, 5_000, "age is over queued rows only");
        assert_eq!(status.projects.len(), 2);
        assert_eq!(
            status.projects[0].project_id, "q",
            "oldest queued work first"
        );
        assert_eq!(
            status.projects[1].transformers,
            vec!["tx_queued".to_string()]
        );
        assert!(!status.stuck);
        assert_eq!(status.next, "ds report outbox drain");
        assert_eq!(serde_json::to_value(&status).unwrap()["queuedBatches"], 2);

        let scoped = queue_status(&rows, &[], &[], Some("q"), now);
        assert_eq!(scoped.queued_batches, 1);
        assert_eq!(scoped.projects.len(), 1);
        let empty = queue_status(&rows, &[], &[], Some("r"), now);
        assert_eq!(empty.queued_batches, 0);
        assert_eq!(empty.next, "nothing is queued on this machine");
    }

    #[test]
    fn queued_work_nobody_has_pumped_for_an_hour_is_stuck_and_a_live_lease_is_the_pump() {
        // WHY: the owner's box: a 7-day-old queue, no pump, and a confident
        // `stuck: false`. Liveness is the store's lease, not a file lock.
        let now = 10 * ABANDONED_AFTER_MS;
        let mut waiting = row_in(
            "p",
            "export-tx_1",
            "k1",
            ArtifactState::Held,
            now - ABANDONED_AFTER_MS - 60_000,
        );
        waiting.state_reason = Some("the gateway answered 503".into());
        let rows = vec![waiting];
        let abandoned = queue_status(&rows, &[], &[], None, now);
        assert!(abandoned.stuck);
        assert!(abandoned.next.contains("no publisher holds this queue"));
        assert_eq!(
            abandoned.projects[0].reasons,
            vec!["the gateway answered 503".to_string()]
        );
        assert!(!abandoned.projects[0].pumped);

        let pumped = queue_status(&rows, &[lease("p", "server#1", now + 1)], &[], None, now);
        assert!(
            !pumped.stuck,
            "a live lease is a pump; old work under it is slow, not abandoned"
        );
        assert!(pumped.projects[0].pumped);
        assert_eq!(pumped.leases.len(), 1);
        assert_eq!(pumped.next, "ds report outbox drain");

        let dead = queue_status(&rows, &[lease("p", "server#1", now)], &[], None, now);
        assert!(dead.stuck, "an expired lease is no pump");
        assert!(dead.leases.is_empty());
        let elsewhere = queue_status(&rows, &[lease("q", "server#1", now + 1)], &[], None, now);
        assert!(
            elsewhere.stuck,
            "a lease on another project pumps that project"
        );
    }

    #[test]
    fn an_empty_queue_is_never_stuck_however_old_its_settled_rows_are() {
        let now = 10 * ABANDONED_AFTER_MS;
        let rows = vec![
            row_in("p", "export-tx_head", "k2", ArtifactState::Published, 1),
            row_in("p", "export-tx_lost", "k3", ArtifactState::Conflict, 1),
        ];
        let status = queue_status(&rows, &[], &[], None, now);
        assert_eq!(status.queued_batches, 0);
        assert_eq!(status.oldest_age_ms, 0);
        assert!(!status.stuck);
        assert_eq!(
            status.next, "ds report outbox drain",
            "a lost row is freed by the next pass"
        );
        let only_head = queue_status(&rows[..1], &[], &[], None, now);
        assert_eq!(only_head.next, "nothing is queued on this machine");
    }

    #[test]
    fn a_refused_credential_blocks_a_live_queue_until_a_newer_delivery_receipt() {
        let now = 10_000;
        let rows = vec![row_in(
            "p",
            "export-tx_1",
            "k1",
            ArtifactState::Held,
            now - 1,
        )];
        let lease = lease("p", "desktop#1", now + 1);
        let mut refused = receipt("upload", "credential_refused", "export-tx_1");
        refused.detail = Some("the gateway refused this machine's credential (401)".into());
        let blocked = queue_status(
            &rows,
            std::slice::from_ref(&lease),
            std::slice::from_ref(&refused),
            None,
            now,
        );
        assert!(
            blocked.stuck,
            "a live worker cannot pump with a refused credential"
        );
        assert_eq!(
            blocked.blocked.as_ref().unwrap().cause,
            "credential_refused"
        );
        assert_eq!(blocked.projects[0].blocked, blocked.blocked);
        assert!(blocked.next.contains("sign in again"));
        assert_eq!(
            serde_json::to_value(&blocked).unwrap()["blocked"]["sinceMs"],
            1_000
        );

        // A refused heads read is evidence even when the loop returned no
        // run, and a later accepted attempt supersedes the old refusal.
        let mut session = receipt("session", "credential_refused", "export-tx_1");
        session.identity = None;
        let heads_blocked =
            queue_status(&rows, std::slice::from_ref(&lease), &[session], None, now);
        assert_eq!(heads_blocked.blocked.unwrap().cause, "credential_refused");
        let accepted = receipt("upload", "failed", "export-tx_1");
        let recovered = queue_status(&rows, &[lease], &[accepted, refused], None, now);
        assert!(recovered.blocked.is_none());
        assert!(!recovered.stuck);
        assert_eq!(recovered.next, "ds report outbox drain");
    }
}
