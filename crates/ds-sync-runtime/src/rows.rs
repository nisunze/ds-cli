//! The rows the loop observes and the receipts it answers with, and the
//! mappings between them, the kernel's plan input and the store's rows.
//!
//! Nothing here decides: every function is a projection of one shape onto
//! another, in the vocabulary the authority already uses.

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
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
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
    /// The one resource this publication is about — for a report, its
    /// transformer. The plan keys a row's scope on it.
    pub resource: String,
    pub client_publish_id: String,
    pub outputs: Vec<LocalOutput>,
    /// The producer store reference the bytes live at — a batch id in the
    /// report artifact root, a durable job id for Solar — as the store keeps
    /// it (`ArtifactRow::bytes_locator`). The producer reads it to move or
    /// free the bytes; nothing else interprets it.
    pub bytes_locator: String,
}

impl LocalRow {
    /// The grant scope one upload of this row needs covered, spelled by the
    /// kernel's one constructor so this row and the grant offered for it can
    /// never disagree about what the key is.
    pub fn scope(&self) -> String {
        sync::scope_key(&self.engine_release, &self.resource)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct LocalOutput {
    pub filename: Option<String>,
    pub paper_size: Option<String>,
    pub presentation: Option<serde_json::Value>,
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

/// What the shared record's door answered for one publication — `open` or
/// `finalize` on `compute_artifacts`. `uploads` names each output the record
/// still lacks and the storage session that takes its bytes; an output the
/// record already holds is absent from it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RecordAnswer {
    pub work_id: String,
    /// `pending` | `published` | `stored_stale`, verbatim.
    pub state: String,
    pub head_revision: Option<i64>,
    /// Why a `stored_stale` run lost, in the record's words.
    pub conflict: Option<String>,
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

impl TransferReceipt {
    /// The transport host's one verdict for bytes that are all in the session.
    pub const COMPLETED: &'static str = "completed";

    /// Every byte is in the storage session and the transport says so. The
    /// record is asked to publish on nothing less.
    pub fn committed(&self) -> bool {
        self.outcome == Self::COMPLETED && self.committed_bytes == self.total_bytes
    }
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
    /// publication that lost (its outcome names the verdict it lost to), and
    /// `adopted`, a batch found on disk with no row and recorded once.
    pub action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub identity: Option<sync::Identity>,
    /// What happened, in the authority's own vocabulary where it has one.
    pub outcome: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub work_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_publish_id: Option<String>,
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
            work_id: None,
            client_publish_id: None,
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

    pub fn for_publication(mut self, row: &LocalRow) -> Self {
        self.client_publish_id = Some(row.client_publish_id.clone()).filter(|id| !id.is_empty());
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
            if self.reclaimed.batches == 1 {
                ""
            } else {
                "es"
            },
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

/// The record's answer to `open` or `finalize`, read from its data envelope.
pub fn parse_record_answer(answer: &Value) -> Result<RecordAnswer, String> {
    let parsed = RecordAnswer {
        work_id: text(answer, "work_id"),
        state: text(answer, "state"),
        head_revision: answer.get("head_revision").and_then(Value::as_i64),
        conflict: Some(text(answer, "conflict")).filter(|value| !value.is_empty()),
        uploads: answer
            .get("outputs")
            .and_then(Value::as_array)
            .map(|outputs| {
                outputs
                    .iter()
                    .filter(|output| text(output, "status") == "upload")
                    .map(|output| (text(output, "output_id"), text(output, "session_uri")))
                    .collect()
            })
            .unwrap_or_default(),
    };
    if parsed.work_id.is_empty() || parsed.state.is_empty() {
        return Err("the record's answer names no work or state".to_string());
    }
    if parsed
        .uploads
        .iter()
        .any(|(output, session)| output.is_empty() || session.is_empty())
    {
        return Err("the record asked for bytes without a storage session".to_string());
    }
    Ok(parsed)
}

/// The `open` declaration of one report publication on the shared record's
/// door. Everything in it is the row's own, as the seal recorded it: which
/// transformer export, which source inputs it was computed from, what it
/// produced, and — as provenance, never as a credential — the engine release
/// and build that produced it. The publish id is the run's idempotency key, so
/// a retry after any crash names the same run.
pub fn open_declaration(project: &str, row: &LocalRow) -> Value {
    json!({
        "action": "open",
        "project_id": project,
        "client_run_id": row.client_publish_id,
        "engine": row.identity.engine,
        "operation": row.identity.operation,
        "variant": row.identity.variant,
        "engine_version": row.engine_release,
        "engine_build_manifest_sha256": row.engine_build_manifest_sha256,
        "input_base_fingerprint": row.input_base_fingerprint.clone().unwrap_or_default(),
        "outputs": row
            .outputs
            .iter()
            .map(|output| {
                let mut declaration = json!({
                    "output_id": output.output_id,
                    "format": output.format,
                    "content_type": output.content_type,
                    "sha256": output.sha256,
                    "size_bytes": output.size_bytes,
                });
                for (key, value) in [
                    ("filename", output.filename.as_ref().map(|v| json!(v))),
                    ("paper_size", output.paper_size.as_ref().map(|v| json!(v))),
                    ("presentation", output.presentation.clone()),
                ] {
                    if let Some(value) = value { declaration[key] = value; }
                }
                declaration
            })
            .collect::<Vec<_>>(),
    })
}

/// The `finalize` of the run `open` answered with.
pub fn finalize_declaration(project: &str, row: &LocalRow, work_id: &str) -> Value {
    json!({
        "action": "finalize",
        "project_id": project,
        "work_id": work_id,
        "client_run_id": row.client_publish_id,
    })
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
pub fn store_row(project: &str, row: &LocalRow, now: u64) -> store::ArtifactRow {
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
        grant_engine: None,
        resource: Some(row.resource.clone()),
        client_publish_id: row.client_publish_id.clone(),
        acknowledged_work_id: None,
        outputs: row
            .outputs
            .iter()
            .map(|output| store::ArtifactOutput {
                filename: output.filename.clone(),
                paper_size: output.paper_size.clone(),
                presentation: output.presentation.clone(),
                output_id: output.output_id.clone(),
                format: output.format.clone(),
                content_type: output.content_type.clone(),
                sha256: output.sha256.clone(),
                size_bytes: output.size_bytes,
            })
            .collect(),
        bytes_locator: row.bytes_locator.clone(),
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
            // Additive reading of sealed rows written before producers used
            // the explicit source field. This never adopts producer jobs.
            sync::engines::engine(&row.identity.engine)
                .is_some_and(|engine| engine.legacy_source_in_base)
                .then(|| row.base_revision.clone())
                .flatten()
        }),
        readable: row.readable,
        engine_release: row.engine_release.clone(),
        engine_build_manifest_sha256: row.engine_build_manifest_sha256.clone().unwrap_or_default(),
        resource: row.resource.clone().unwrap_or_default(),
        client_publish_id: row.client_publish_id.clone(),
        outputs: row
            .outputs
            .iter()
            .map(|output| LocalOutput {
                filename: output.filename.clone(),
                paper_size: output.paper_size.clone(),
                presentation: output.presentation.clone(),
                output_id: output.output_id.clone(),
                format: output.format.clone(),
                content_type: output.content_type.clone(),
                sha256: output.sha256.clone(),
                size_bytes: output.size_bytes,
            })
            .collect(),
        bytes_locator: row.bytes_locator.clone(),
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
        grant_id: None,
        work_id: receipt.work_id.clone(),
        client_publish_id: receipt.client_publish_id.clone(),
        head_revision: receipt.revision.clone(),
        head_sha256: None,
        head_counter: receipt.head_revision,
        committed_bytes: receipt.bytes.or((transferred > 0).then_some(transferred)),
        at_ms: now,
    }
}

#[cfg(test)]
mod printing_metadata_tests {
    use super::*;
    #[test]
    fn durable_report_open_keeps_native_filename_paper_and_presentation() {
        let mut row = crate::fakes::local_row(b"sealed print bytes");
        row.outputs[0].filename = Some("Tx_1.governed.pdf".into());
        row.outputs[0].format = "pdf__governed".into();
        row.outputs[0].content_type = "application/pdf".into();
        row.outputs[0].paper_size = Some("A0".into());
        row.outputs[0].presentation = Some(
            json!({"layout_id":"governed","layout_name":"Governed","paper_name":"A0","orientation":"landscape","page_mm":[1189,841]}),
        );
        let stored = store_row("project-1", &row, 1);
        let restored = local_row_from_store(&stored);
        assert_eq!(restored.outputs, row.outputs);
        let open = open_declaration("project-1", &restored);
        assert_eq!(open["outputs"][0]["filename"], "Tx_1.governed.pdf");
        assert_eq!(open["outputs"][0]["paper_size"], "A0");
        assert_eq!(
            open["outputs"][0]["presentation"],
            row.outputs[0].presentation.clone().unwrap()
        );
    }
}
