//! The rows the loop observes and the receipts it answers with, and the
//! mappings between them, the kernel's plan input and the store's rows.
//!
//! Nothing here decides: every function is a projection of one shape onto
//! another, in the vocabulary the authority already uses.

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use ds_command_kernel::sync;
use ds_command_kernel::sync_store as store;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The plan-and-receipts envelope this runtime answers with.
pub const SCHEMA: &str = "ds.sync-run/v1";

/// Bound on one project's inventories before the kernel's own bound applies.
pub const MAX_ROWS: usize = 4096;

// ---------------------------------------------------------------------------
// Rows
// ---------------------------------------------------------------------------

/// What this machine holds for one artifact identity, plus the closure needed
/// to publish it.
///
/// One row is one *publication*: a head is keyed by `(engine, operation,
/// variant)` and carries an inventory of outputs, so the digest that stands
/// for it on both sides is [`inventory_digest`] over that inventory — never
/// one output's digest, which would make a two-file head undecidable.
#[derive(Clone, Debug, PartialEq)]
pub struct LocalRow {
    pub identity: sync::Identity,
    pub sha256: String,
    pub size_bytes: u64,
    pub produced_at_ms: u64,
    pub base_revision: Option<String>,
    /// Source inputs used by a derived artifact, separate from the output
    /// head revision in `base_revision`.
    pub input_base_fingerprint: Option<String>,
    pub readable: bool,
    pub engine_release: String,
    /// The exact `ds.engine-build/v1` manifest digest of the build that
    /// produced these bytes, as the producing receipt recorded it. It is the
    /// artifact's own property, never the installed engine's current one: a
    /// row committed by an earlier build stays attributed to that build.
    pub engine_build_manifest_sha256: String,
    /// The engine name the **edge lane** spells, which is what a grant scopes.
    pub grant_engine: String,
    /// The resource this grant is scoped to. §1.3.1: the implemented Network
    /// Reporter slice admits exactly one canonical transformer per grant.
    pub resource: String,
    pub client_publish_id: String,
    pub outputs: Vec<LocalOutput>,
}

impl LocalRow {
    /// The grant scope one upload of this row needs covered, spelled by the
    /// kernel's one constructor so this row and the grant offered for it can
    /// never disagree about what the key is.
    pub fn scope(&self) -> String {
        sync::scope_key(&self.engine_release, &self.resource)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct LocalOutput {
    pub output_id: String,
    pub format: String,
    pub content_type: String,
    pub sha256: String,
    pub size_bytes: u64,
}

/// One head of the shared record.
#[derive(Clone, Debug, PartialEq)]
pub struct RemoteRow {
    pub identity: sync::Identity,
    /// The concurrency fence the server itself checks at publish — the v2
    /// OCC/head identity, not the monotonic counter beside it.
    pub revision: String,
    pub head_revision: i64,
    pub input_base_fingerprint: Option<String>,
    pub sha256: String,
    pub published_at_ms: u64,
    pub outputs: Vec<RemoteOutput>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RemoteOutput {
    pub output_id: String,
    pub sha256: String,
    pub size_bytes: u64,
}

/// The work grant in hand, as §1.2 minted it.
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HeldGrant {
    pub grant_id: String,
    pub install_id: String,
    pub project: String,
    pub engine: String,
    pub engine_release: String,
    /// The exact engine builds ds-brain admitted for this release when it
    /// minted the grant — `scope.engines[].manifest_sha256`, §2.5. Admission
    /// is decided once, at `work/open`; this is that decision, carried.
    /// Empty is not "any": it is a grant that admits nothing.
    #[serde(default)]
    pub admitted_builds: Vec<String>,
    pub resource: String,
    pub expires_at_ms: u64,
    /// What the snapshot pinned. Both `base_revisions` and every artifact's
    /// attribution are copied from here verbatim: ds-brain compares them field
    /// for field against the grant it signed, so a locally derived value is
    /// not a fallback, it is a refusal.
    pub snapshot_base: BTreeMap<String, String>,
}

impl HeldGrant {
    /// The scope this grant covers: engine release plus the single resource
    /// §1.3.1 admits per grant. Same constructor as [`LocalRow::scope`].
    pub fn scope(&self) -> String {
        sync::scope_key(&self.engine_release, &self.resource)
    }
}

/// The request §1.2 takes.
#[derive(Clone, Debug, PartialEq)]
pub struct WorkOpen {
    pub client_request_id: String,
    pub install_id: String,
    pub project: String,
    pub engine: String,
    pub engine_release: String,
    pub resource: String,
}

impl WorkOpen {
    pub fn body(&self) -> Value {
        json!({
            "client_request_id": self.client_request_id,
            "install_id": self.install_id,
            "ds_project": self.project,
            "engine": self.engine,
            "engine_release": self.engine_release,
            "resources": {
                "task": "export_transformer_report",
                "transformer_names": [self.resource],
            },
        })
    }
}

/// What §1.3 answered.
#[derive(Clone, Debug, PartialEq)]
pub struct PublishOutcome {
    /// `published` | `upload_required` | a typed conflict state.
    pub state: String,
    pub work_id: Option<String>,
    pub head_revision: Option<i64>,
    /// `(output id, session uri)` for every object the server says is missing.
    pub uploads: Vec<(String, String)>,
}

/// What one artifact's byte transfer did, reported by the transport host.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TransferReceipt {
    pub output_id: String,
    /// The transport host's typed outcome, verbatim.
    pub outcome: String,
    pub committed_bytes: u64,
    pub total_bytes: u64,
}

// ---------------------------------------------------------------------------
// Receipts
// ---------------------------------------------------------------------------

/// What one planned action actually did. A receipt is evidence, never a
/// decision: the Sync Center renders these and computes nothing.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    /// The planned action this answers: `open_grant`, `upload`, `download`,
    /// `conflict`, `nothing`, `refused`, `offline`, `lease`, `store` — and
    /// `reclaimed` / `reclaim_failed`, what became of the bytes of a
    /// publication that lost (its outcome names the verdict it lost to).
    pub action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<sync::Identity>,
    /// What happened, in the authority's own vocabulary where it has one.
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub grant_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_revision: Option<i64>,
    /// The record's head identity after a committed publish (the OCC revision
    /// the grant pinned): the store advances the held head from the receipt
    /// and never re-reads it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub transfers: Vec<TransferReceipt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u64>,
}

impl Receipt {
    pub fn new(action: &'static str, outcome: impl Into<String>) -> Self {
        Receipt {
            action,
            identity: None,
            outcome: outcome.into(),
            detail: None,
            grant_id: None,
            work_id: None,
            head_revision: None,
            revision: None,
            transfers: Vec::new(),
            bytes: None,
        }
    }

    pub fn about(mut self, identity: &sync::Identity) -> Self {
        self.identity = Some(identity.clone());
        self
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }
}

/// What one pass freed on this machine: sealed batches that lost — to a
/// later computation of their room, or to the source having moved — and
/// will never publish. Counted from the pass's own `reclaimed` receipts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reclaimed {
    pub batches: usize,
    pub bytes: u64,
}

impl Reclaimed {
    /// Tally every `reclaimed` receipt that actually freed bytes. One that
    /// found the batch already gone is evidence, not a freed batch.
    pub fn from_receipts(receipts: &[Receipt]) -> Self {
        receipts
            .iter()
            .filter(|receipt| receipt.action == "reclaimed")
            .filter_map(|receipt| receipt.bytes.filter(|bytes| *bytes > 0))
            .fold(Reclaimed::default(), |tally, bytes| Reclaimed {
                batches: tally.batches + 1,
                bytes: tally.bytes.saturating_add(bytes),
            })
    }
}

/// The plan, what performing it did, and the plan as it stands afterwards.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRun {
    pub schema: &'static str,
    pub project: String,
    /// Why this run happened. There is no reason that is "time passed".
    pub trigger: sync::Trigger,
    pub plan: sync::Plan,
    pub receipts: Vec<Receipt>,
    /// Re-planned after the actions, so a caller sees the state it is now in
    /// without asking twice.
    pub replanned: sync::Plan,
    /// What this pass freed. A pass over a non-empty queue that moved
    /// nothing and freed nothing must still say why: see [`SyncRun::idle`].
    pub reclaimed: Reclaimed,
}

impl SyncRun {
    /// Why this pass moved no bytes, when it moved none — bounded, from the
    /// plan's own summary, so "drained 0" is never the whole answer. `None`
    /// when the plan held an upload or a download, or an action was refused.
    pub fn idle(&self) -> Option<String> {
        let summary = &self.plan.summary;
        if summary.uploads > 0 || summary.downloads > 0 || summary.refused > 0 {
            return None;
        }
        let offline = self
            .plan
            .actions
            .iter()
            .any(|action| matches!(action, sync::Action::Offline));
        Some(format!(
            "nothing to move{}: {} in sync, {} in conflict, {} refused, {} receipt{}, {} batch{} reclaimed",
            if offline { " while offline" } else { "" },
            summary.in_sync,
            summary.conflicts,
            summary.refused,
            self.receipts.len(),
            if self.receipts.len() == 1 { "" } else { "s" },
            self.reclaimed.batches,
            if self.reclaimed.batches == 1 { "" } else { "es" },
        ))
    }
}

// ---------------------------------------------------------------------------
// Inventory facts
// ---------------------------------------------------------------------------

/// The one digest that stands for a publication's output inventory.
///
/// Used for the local row and the remote head alike, so "the digests differ"
/// means "the output sets differ" and nothing else. Defined once, in the
/// artifact crate, so the queue's "is this batch the head" and this loop's
/// "is this row in sync" can never compute two different answers.
pub use ds_report_artifacts::inventory_digest;

/// Wall-clock milliseconds since the epoch; the host's high-water mark of
/// server-observed time until a server stamp says otherwise.
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            since.as_millis().min(u128::from(u64::MAX)) as u64
        })
}

/// Server time as the authority spells it: RFC 3339, converted to the
/// millisecond high-water the kernel takes.
pub fn server_time_ms(value: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|stamp| stamp.timestamp_millis().max(0) as u64)
}

pub(crate) fn text(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn kernel_local(row: &LocalRow) -> sync::LocalArtifact {
    sync::LocalArtifact {
        identity: row.identity.clone(),
        sha256: row.sha256.clone(),
        size_bytes: row.size_bytes,
        produced_at_ms: row.produced_at_ms,
        base_revision: row.base_revision.clone(),
        input_base_fingerprint: row.input_base_fingerprint.clone(),
        readable: row.readable,
        engine_release: row.engine_release.clone(),
        // Without this the plan cannot tell one publication's scope from
        // another's, and one covered row answers for every uncovered one.
        resource: row.resource.clone(),
    }
}

pub(crate) fn kernel_remote(row: &RemoteRow) -> sync::RemoteHead {
    sync::RemoteHead {
        identity: row.identity.clone(),
        revision: row.revision.clone(),
        input_base_fingerprint: row.input_base_fingerprint.clone(),
        sha256: row.sha256.clone(),
        published_at_ms: row.published_at_ms,
    }
}

/// The grant as the kernel's plan reads it.
pub fn kernel_grant(grant: &HeldGrant) -> sync::Grant {
    sync::Grant {
        grant_id: grant.grant_id.clone(),
        install_id: grant.install_id.clone(),
        project: grant.project.clone(),
        // One entry per admitted digest, the way ds-brain registers them. A
        // grant that carries none is shown as it is — the kernel matches on
        // the release, and the digest is the server's per-row claim.
        engines: if grant.admitted_builds.is_empty() {
            vec![sync::EngineRelease {
                name: grant.engine.clone(),
                release: grant.engine_release.clone(),
                manifest_sha256: None,
            }]
        } else {
            grant
                .admitted_builds
                .iter()
                .map(|digest| sync::EngineRelease {
                    name: grant.engine.clone(),
                    release: grant.engine_release.clone(),
                    manifest_sha256: Some(digest.clone()),
                })
                .collect()
        },
        resource: grant.resource.clone(),
        expires_at_ms: grant.expires_at_ms,
        snapshot_base: grant.snapshot_base.clone(),
    }
}

// ---------------------------------------------------------------------------
// Gateway answers → rows
// ---------------------------------------------------------------------------

/// One head of the shared record, read from a `compute_artifacts` list row.
pub fn remote_row(head: &Value) -> RemoteRow {
    let mut outputs: Vec<RemoteOutput> = head
        .get("outputs")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(|output| RemoteOutput {
                    output_id: text(output, "output_id"),
                    sha256: text(output, "sha256"),
                    size_bytes: output
                        .get("size_bytes")
                        .and_then(Value::as_u64)
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    outputs.sort_by(|left, right| left.output_id.cmp(&right.output_id));
    let mut pairs: Vec<(String, String)> = outputs
        .iter()
        .map(|output| (output.output_id.clone(), output.sha256.clone()))
        .collect();
    let head_revision = head
        .get("head_revision")
        .and_then(Value::as_i64)
        .unwrap_or_default();
    let fingerprint = Some(text(head, "input_base_fingerprint")).filter(|value| !value.is_empty());
    RemoteRow {
        identity: sync::Identity {
            engine: text(head, "engine"),
            operation: text(head, "operation"),
            variant: text(head, "variant"),
        },
        revision: format!("head-{head_revision}"),
        head_revision,
        input_base_fingerprint: fingerprint,
        sha256: inventory_digest(&mut pairs),
        published_at_ms: server_time_ms(&text(head, "updated_at")).unwrap_or_default(),
        outputs,
    }
}

/// The grant §1.2 minted, read from the `work/open` data envelope. The
/// resource is the request's: a grant is scoped to the one transformer the
/// host asked for.
pub fn parse_work_open(answer: &Value, request: &WorkOpen) -> Result<HeldGrant, String> {
    let payload = answer.get("grant").cloned().unwrap_or(Value::Null);
    let scope = payload.get("scope").cloned().unwrap_or(Value::Null);
    let engines = scope
        .get("engines")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let engine = engines.first().cloned().unwrap_or(Value::Null);
    // One release can resolve to several registered builds (one per
    // platform asset), so the claim is the whole list, not the first.
    let admitted_builds: Vec<String> = engines
        .iter()
        .map(|entry| text(entry, "manifest_sha256"))
        .filter(|digest| !digest.is_empty())
        .collect();
    let expires_at_ms = server_time_ms(&text(&payload, "expires_at"))
        .ok_or_else(|| "the work grant has no readable expiry".to_string())?;
    let grant = HeldGrant {
        grant_id: text(&payload, "grant_id"),
        install_id: text(&payload, "install_id"),
        project: text(&scope, "ds_project"),
        engine: text(&engine, "name"),
        engine_release: text(&engine, "release"),
        admitted_builds,
        resource: request.resource.clone(),
        expires_at_ms,
        snapshot_base: payload
            .get("snapshot_base")
            .and_then(|base| serde_json::from_value(base.clone()).ok())
            .unwrap_or_default(),
    };
    if grant.grant_id.is_empty() || grant.project.is_empty() || grant.engine_release.is_empty() {
        return Err("the work grant is incomplete".to_string());
    }
    Ok(grant)
}

/// What §1.3 answered, read from the `work/publish` data envelope.
pub fn parse_publish(answer: &Value) -> PublishOutcome {
    PublishOutcome {
        state: text(answer, "state"),
        work_id: Some(text(answer, "work_id")).filter(|value| !value.is_empty()),
        head_revision: answer.get("head_revision").and_then(Value::as_i64),
        uploads: answer
            .get("uploads")
            .and_then(Value::as_array)
            .map(|uploads| {
                uploads
                    .iter()
                    .filter_map(|upload| {
                        let session = text(upload, "session_uri");
                        (!session.is_empty()).then(|| (text(upload, "kind"), session))
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

// ---------------------------------------------------------------------------
// Store rows ↔ loop rows
// ---------------------------------------------------------------------------

pub fn store_scope(project: &str) -> store::Scope {
    store::Scope::Project {
        project: project.to_string(),
    }
}

/// A producer's row as the store keeps it. The locator names the producer
/// store that owns the bytes; the store never interprets it.
pub fn store_row(
    project: &str,
    row: &LocalRow,
    bytes_locator: String,
    now: u64,
) -> store::ArtifactRow {
    store::ArtifactRow {
        scope: store_scope(project),
        identity: row.identity.clone(),
        sha256: row.sha256.clone(),
        size_bytes: row.size_bytes,
        produced_at_ms: row.produced_at_ms,
        base_revision: row.base_revision.clone(),
        input_base_fingerprint: row.input_base_fingerprint.clone(),
        engine_release: row.engine_release.clone(),
        engine_build_manifest_sha256: Some(row.engine_build_manifest_sha256.clone())
            .filter(|digest| !digest.is_empty()),
        grant_engine: Some(row.grant_engine.clone()),
        resource: Some(row.resource.clone()),
        client_publish_id: row.client_publish_id.clone(),
        outputs: row
            .outputs
            .iter()
            .map(|output| store::ArtifactOutput {
                output_id: output.output_id.clone(),
                format: output.format.clone(),
                content_type: output.content_type.clone(),
                sha256: output.sha256.clone(),
                size_bytes: output.size_bytes,
            })
            .collect(),
        bytes_locator,
        readable: row.readable,
        state: store::ArtifactState::Held,
        state_reason: None,
        replay_key: row.client_publish_id.clone(),
        updated_at_ms: now,
        transfer_state: None,
    }
}

pub fn local_row_from_store(row: &store::ArtifactRow) -> LocalRow {
    LocalRow {
        identity: row.identity.clone(),
        sha256: row.sha256.clone(),
        size_bytes: row.size_bytes,
        produced_at_ms: row.produced_at_ms,
        base_revision: row.base_revision.clone(),
        input_base_fingerprint: row.input_base_fingerprint.clone().or_else(|| {
            // Additive migration for report rows written before the store
            // gained the explicit source-input field.
            (row.grant_engine.as_deref() == Some("ds-network-reporter")
                || row.identity.engine == "network_reporter")
                .then(|| row.base_revision.clone())
                .flatten()
        }),
        readable: row.readable,
        engine_release: row.engine_release.clone(),
        engine_build_manifest_sha256: row.engine_build_manifest_sha256.clone().unwrap_or_default(),
        grant_engine: row
            .grant_engine
            .clone()
            .unwrap_or_else(|| row.identity.engine.clone()),
        resource: row.resource.clone().unwrap_or_default(),
        client_publish_id: row.client_publish_id.clone(),
        outputs: row
            .outputs
            .iter()
            .map(|output| LocalOutput {
                output_id: output.output_id.clone(),
                format: output.format.clone(),
                content_type: output.content_type.clone(),
                sha256: output.sha256.clone(),
                size_bytes: output.size_bytes,
            })
            .collect(),
    }
}

pub fn remote_row_from_head(head: &store::HeadRow) -> RemoteRow {
    RemoteRow {
        identity: head.identity.clone(),
        revision: head.revision.clone(),
        head_revision: head.head_revision,
        input_base_fingerprint: head.input_base_fingerprint.clone(),
        sha256: head.sha256.clone(),
        published_at_ms: head.published_at_ms,
        outputs: head
            .outputs
            .iter()
            .map(|output| RemoteOutput {
                output_id: output.output_id.clone(),
                sha256: output.sha256.clone(),
                size_bytes: output.size_bytes,
            })
            .collect(),
    }
}

pub fn head_from_remote(project: &str, head: &RemoteRow) -> store::HeadRow {
    store::HeadRow {
        scope: store_scope(project),
        identity: head.identity.clone(),
        revision: head.revision.clone(),
        head_revision: head.head_revision,
        input_base_fingerprint: head.input_base_fingerprint.clone(),
        sha256: head.sha256.clone(),
        published_at_ms: head.published_at_ms,
        outputs: head
            .outputs
            .iter()
            .map(|output| store::HeadOutput {
                output_id: output.output_id.clone(),
                sha256: output.sha256.clone(),
                size_bytes: output.size_bytes,
            })
            .collect(),
        read_at_ms: 0,
    }
}

/// One run receipt as the store keeps it. Outcomes are the store's
/// vocabulary: a publish that reached `published` is `committed`; a typed
/// conflict state is `conflict`; a plan refusal carries its reason as detail.
pub fn receipt_row(project: &str, receipt: &Receipt, now: u64) -> store::ReceiptRow {
    let outcome = match (receipt.action, receipt.outcome.as_str()) {
        ("upload", "published") => "committed".to_string(),
        ("upload", "upload_required") => "incomplete".to_string(),
        ("upload", state) if state.starts_with("conflict") || state.contains("stale") => {
            "conflict".to_string()
        }
        ("download", "downloaded") => "committed".to_string(),
        (_, other) => other.to_string(),
    };
    let detail = receipt.detail.clone().or_else(|| {
        matches!(receipt.action, "refused" | "conflict").then(|| receipt.outcome.clone())
    });
    let transferred: u64 = receipt
        .transfers
        .iter()
        .map(|transfer| transfer.committed_bytes)
        .sum();
    store::ReceiptRow {
        receipt_id: format!("{}:{now}:{}", receipt.action, uuid::Uuid::new_v4().simple()),
        scope: store_scope(project),
        action: receipt.action.to_string(),
        identity: receipt.identity.clone(),
        outcome,
        detail,
        grant_id: receipt.grant_id.clone(),
        work_id: receipt.work_id.clone(),
        head_revision: receipt.revision.clone(),
        head_sha256: None,
        head_counter: receipt.head_revision,
        committed_bytes: receipt.bytes.or((transferred > 0).then_some(transferred)),
        at_ms: now,
    }
}

/// The grant as the store keeps it: the public scope beside the opaque
/// document the host holds.
pub fn grant_row(project: &str, grant: &HeldGrant) -> Result<store::GrantRow, String> {
    Ok(store::GrantRow {
        scope: store_scope(project),
        grant_id: grant.grant_id.clone(),
        install_id: grant.install_id.clone(),
        engines: kernel_grant(grant).engines,
        expires_at_ms: grant.expires_at_ms,
        raw: serde_json::to_value(grant).map_err(|error| error.to_string())?,
    })
}
