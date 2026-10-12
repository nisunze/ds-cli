//! What is true about one status row — the register's whole truth, decided once.
//!
//! Eight authorities used to answer this question, in four TypeScript files,
//! and every pair of them disagreed somewhere:
//!
//! - `statusRowView` folded Saved/Unsaved, locality, version, presence and
//!   the foreign room;
//! - `mergeLocalTransformerStatus` decided the client half of that fold from
//!   this browser's rooms — and FIRED presence reconciliation from inside
//!   itself, an authority hidden in a call order;
//! - two different rules answered phase supersession, one of them by
//!   REWRITING the incoming document so a legacy error went quiet;
//! - `canRetryProcess` let a client-side layer count overrule ds-brain's
//!   published `retry_capabilities.process.can_retry === false`;
//! - `latestAction` ranked ~20 timestamp candidates on a ladder written
//!   nowhere but in four bare integers;
//! - `transformerGovernanceBadge` re-labelled ds-brain's verdict, with a
//!   live edit-lock override painted beside it by the component;
//! - and three overlapping name sets said which rows were not transformers.
//!
//! This module is the single answer. It decides nothing ds-brain decided:
//! governance and retry admissibility are read, folded and labelled, never
//! re-judged. Every label is an i18n KEY; the only English that leaves here
//! is the backend's own text, sanitised.
//!
//! Pure and cheap by contract: every input is already in the
//! `list_transformers_status` response the host holds, plus the room headers
//! the host enumerates (contract §4 — IDB/OPFS/desktop-cache enumeration is
//! permanently the host's) and the clock, which is an INPUT.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::design::{canonical_row_name, is_special_row, row_allows, row_kind, RowAllows, RowKind};
use crate::design_health::sanitize_message;
use crate::response_policy::iso8601_ms;

pub const SCHEMA: &str = "ds.design-status-row/v1";

/// A whole project's rows travel in one call, as they do for health.
const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;

// ── the vocabulary ──────────────────────────────────────────────────────

/// The register's own wire words, kept so the projections are one-liners.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncState {
    Saved,
    Unsaved,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Locality {
    Unsaved,
    Local,
    Remote,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CombinedState {
    Success,
    Stale,
    Processing,
    Error,
    None,
}

/// Which run owns the saved record.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum PhaseOwner {
    Process,
    Legacy,
    #[default]
    None,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LegacyPhase {
    Draft,
    Sketch,
}

/// The latest-action ladder. Declaration order IS the rank (later wins a
/// tie), replacing the bare 2/3/4/5 that were its only record.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ActionRank {
    DocumentMetadata,
    Upload,
    Phase,
    BrowserEdit,
}

/// ds-brain's verdict, folded. `Unknown` is the row that carries no
/// `computed` block at all: not open, not judged — refresh first.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceState {
    Open,
    Locked,
    Blocked,
    Unknown,
}

/// Why a retry is or is not admissible. The first three are ds-brain's own
/// verdict; the rest are the inference that may answer only when ds-brain
/// published none.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetryReason {
    BackendAllowed,
    BackendRefused,
    BackendSourceUnsupported,
    BackendSourceOnly,
    SavedLayers,
    DraftingSource,
    NoSource,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RetrySource {
    None,
    FirestoreReplay,
}

/// Where the row came from. A `browser_only` row has no server document: the
/// host synthesises it from the header this reply points at.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RowOrigin {
    Server,
    BrowserOnly,
}

// ── the reply ───────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ForeignRoom {
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<i64>,
}

/// The client half of the row: what THIS device holds.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Client {
    pub cached: bool,
    pub dirty: bool,
    pub server_known: bool,
    pub local: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_version: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreign_room: Option<ForeignRoom>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Presence {
    pub user: String,
    pub mine: bool,
    pub hold: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_ms: Option<i64>,
    pub session_live: bool,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub feature_counts: Map<String, Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VersionView {
    pub version: i64,
    pub latest: i64,
    /// The lead is not the newest snapshot — something was restored.
    pub restored: bool,
    pub unversioned: bool,
    pub baseline_ready: bool,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_origin: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_version_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_revision_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub changed_since_version: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct View {
    pub sync_state: SyncState,
    pub sync_title_key: String,
    pub locality: Locality,
    pub combined_state: CombinedState,
    pub report_state: CombinedState,
    pub report_has_files: bool,
    pub version: VersionView,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub presence: Option<Presence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub foreign_room: Option<ForeignRoom>,
    pub show_sync_badge: bool,
    pub server_known: bool,
    pub client_dirty: bool,
    pub active_edit_dirty: bool,
    pub process_status: String,
    pub report_status: String,
    pub badges: Vec<Badge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locality_hint_key: Option<String>,
    /// The transformer's voltage-drop analysis; absent on rows that are not
    /// transformers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voltage_drop: Option<VoltageDropView>,
}

/// A transformer's voltage-drop analysis as the register shows it: the one
/// classification printing also reads (`printing::inventory::voltage_drop`),
/// labelled. `state` is `saved_json`, `local_changes` (computed on this device,
/// not saved), `missing`, `stale` or `invalid_receipt`; a saved analysis
/// carries its verdict, and the engine's own reason when it is incomplete or
/// was not calculated.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct VoltageDropView {
    pub state: String,
    pub calculated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    pub label_key: String,
    pub tone: String,
}

fn voltage_drop_view(row: &Map<String, Value>, dirty: bool) -> VoltageDropView {
    let preview = crate::printing::inventory::voltage_drop(row, dirty);
    let (label_key, tone) = match (preview.state.as_str(), preview.verdict.as_deref()) {
        ("saved_json", Some("complies")) => ("dsr_vd_complies", "emerald"),
        ("saved_json", Some("complies_with_reinforcement")) => {
            ("dsr_vd_complies_with_reinforcement", "amber")
        }
        ("saved_json", Some("does_not_comply")) => ("dsr_vd_does_not_comply", "red"),
        ("saved_json", Some("incomplete")) => ("dsr_vd_incomplete", "amber"),
        // A refused calculation has no electrical verdict. Keep its input
        // issue visible without presenting it as a calculated failure.
        ("saved_json", Some("refused")) => ("dsr_vd_not_calculated", "amber"),
        ("saved_json", Some("reserved")) => ("dsr_vd_reserved", "slate"),
        ("local_changes", _) => ("dsr_vd_local_changes", "blue"),
        ("stale", _) => ("dsr_vd_stale", "amber"),
        ("missing", _) => ("dsr_vd_missing", "slate"),
        _ => ("dsr_vd_invalid_receipt", "red"),
    };
    VoltageDropView {
        state: preview.state,
        calculated: preview.calculated,
        verdict: preview.verdict,
        reason: preview.reason,
        label_key: label_key.into(),
        tone: tone.into(),
    }
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Badge {
    pub key: String,
    pub label_key: String,
    pub tone: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

/// The legacy phase whose output the newer run overwrote, named explicitly
/// instead of rewritten into the document.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Superseded {
    pub phase: LegacyPhase,
    /// When it lost the record — the newer run's stamp.
    pub at_ms: i64,
    /// The error it was carrying, kept loud rather than moved aside.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retained_error: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PhaseOwnership {
    pub owner: PhaseOwner,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded: Option<Superseded>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LatestAction {
    /// `""` when nothing has ever happened to this row.
    pub label_key: String,
    pub when_ms: i64,
    pub user: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rank: Option<ActionRank>,
    /// How long ago, against the clock the host passed in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_ms: Option<i64>,
    /// The user the register attributes the row to — the action's user, then
    /// the document's, then each phase's in slot order.
    pub attributed_user: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Stamp {
    pub user: String,
    pub when_ms: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Governance {
    /// Final technical locks block entry; cooperative leases remain warn-only.
    pub entry_blocked: bool,
    pub state: GovernanceState,
    /// ds-brain's own `write_block_code`, echoed.
    pub code: String,
    pub message_key: String,
    /// ds-brain's own sentence, sanitised. The only English here.
    pub message: String,
    /// This browser's open edit session is what makes it locked, and the
    /// server has not said so yet.
    pub locally_overridden: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Retry {
    pub allowed: bool,
    pub source: RetrySource,
    pub reason: RetryReason,
}

/// The exact optimistic-concurrency fence a headless process can carry back
/// into the server. `present` is the one admission fact consumers branch on;
/// the optional members remain visible so diagnostics say which half is
/// missing without inventing either value.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_version: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_digest: Option<String>,
    pub present: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RowReply {
    pub name: String,
    pub origin: RowOrigin,
    /// Index into the request's `server_rows`, when this row has a document.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_index: Option<usize>,
    /// Index into the request's `local_room_headers`, when a room of this
    /// browser contributed to the row.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header_index: Option<usize>,
    pub client: Client,
    pub view: View,
    pub phase_ownership: PhaseOwnership,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub legacy_phase: Option<LegacyPhase>,
    pub creation: Stamp,
    pub phase_bylines: Map<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drafting_source: Option<String>,
    pub process_settings_used: Map<String, Value>,
    pub saved_layer_names: Vec<String>,
    pub latest_action: LatestAction,
    pub governance: Governance,
    pub retry: Retry,
    pub fence: Fence,
    pub kind: RowKind,
    pub allows: RowAllows,
}

/// A room whose lease this refresh should reconcile. RETURNED, never fired:
/// the merge used to launch this from inside itself, which made a call order
/// the authority (contract §6a).
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PresenceRoom {
    pub name: String,
    pub updated_at_ms: i64,
    pub server_known: bool,
    pub dirty: bool,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub feature_counts: Map<String, Value>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Reply {
    pub schema: &'static str,
    pub rows: Vec<RowReply>,
    pub presence_rooms: Vec<PresenceRoom>,
    /// The reconciliation covers every room, so vanished leases may be
    /// released. False when the refresh named targets.
    pub presence_complete: bool,
}

// ── the request ─────────────────────────────────────────────────────────

/// The `SaveRoomHeader` `planDifferentialSave` already takes, widened by the
/// four facts the merge reads and a save plan does not. One host-supplied
/// room inventory feeds both calls.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct LocalRoomHeader {
    pub transformer_name: String,
    #[serde(default)]
    pub local_dirty: bool,
    #[serde(default)]
    pub server_known: bool,
    #[serde(default)]
    pub base_server_version: Option<i64>,
    // Accepted so one inventory feeds both calls; the save plan reads these,
    // row truth does not.
    #[serde(default)]
    #[allow(dead_code)]
    pub local_content_digest: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub server_content_digest: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub feature_count: Option<i64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub estimated_payload_bytes: Option<i64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub pending_operation_id: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub has_layers: Option<bool>,
    /// The account that wrote the room. Rooms are per browser origin, not per
    /// account (owner ruling 2026-08-26).
    #[serde(default)]
    pub owner_email: Option<String>,
    #[serde(default)]
    pub updated_at_ms: Option<i64>,
    #[serde(default)]
    pub layer_feature_counts: Map<String, Value>,
    /// Process output held locally; it governs the row's process phase until
    /// a save lands.
    #[serde(default)]
    pub process_metadata: Option<Value>,
}

/// This browser's own edit lock — a host FACT, never a local decision.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
struct LocalEditLock {
    transformer: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    server_rows: Vec<Value>,
    #[serde(default)]
    local_room_headers: Vec<LocalRoomHeader>,
    /// The transformer whose live edit session holds unsaved work in memory.
    #[serde(default)]
    active_session_transformer: Option<String>,
    #[serde(default)]
    current_user_email: String,
    #[serde(default)]
    now_ms: i64,
    #[serde(default)]
    local_edit_lock: Option<LocalEditLock>,
    /// The names a targeted refresh asked about. Empty = the whole project,
    /// which is also what makes the presence reconciliation complete.
    #[serde(default)]
    room_targets: Vec<String>,
}

// ── reading JSON the way the browser read it ────────────────────────────

fn object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

fn string_at(map: Option<&Map<String, Value>>, key: &str) -> String {
    map.and_then(|map| map.get(key))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// `parseTimestampMs` (status-table.ts): a finite number is epoch millis; a
/// string is whatever `Date.parse` makes of it; everything else, and 0,
/// is "no timestamp".
pub(crate) fn timestamp_ms(value: Option<&Value>) -> i64 {
    match value {
        Some(Value::Number(number)) => number
            .as_f64()
            .filter(|value| value.is_finite())
            .map(|value| value as i64)
            .unwrap_or(0),
        Some(Value::String(raw)) => {
            let raw = raw.trim();
            if raw.is_empty() {
                0
            } else {
                iso8601_ms(raw).unwrap_or(0)
            }
        }
        _ => 0,
    }
}

fn phase<'row>(row: &'row Map<String, Value>, key: &str) -> Option<&'row Map<String, Value>> {
    row.get(key).and_then(object)
}

fn phase_status(meta: Option<&Map<String, Value>>) -> &str {
    meta.and_then(|meta| meta.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("")
}

/// `computedPhaseState` — an unrecognised value is not a state.
fn combined_state(value: Option<&Value>) -> Option<CombinedState> {
    match value.and_then(Value::as_str) {
        Some("success") => Some(CombinedState::Success),
        Some("stale") => Some(CombinedState::Stale),
        Some("processing") => Some(CombinedState::Processing),
        Some("error") => Some(CombinedState::Error),
        Some("none") => Some(CombinedState::None),
        _ => None,
    }
}

/// `finiteInteger` (transformer-version.ts): truncated, and never negative.
/// `design_version` reads the same ordinals off the same rows, so it shares
/// this one rather than carrying a second spelling of it.
pub(crate) fn finite_ordinal(value: Option<&Value>) -> Option<i64> {
    let numeric = match value? {
        Value::Number(number) => number.as_f64()?,
        Value::String(raw) => raw.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    if !numeric.is_finite() {
        return None;
    }
    let rounded = numeric.trunc() as i64;
    (rounded >= 0).then_some(rounded)
}

const UNVERSIONED: i64 = 0;

/// The lead/latest/restored/unversioned/reason half of every version answer.
/// `design_version` folds `next`, `count`, the label and the movement test
/// around THIS — the register and the prompt must never decide the lead twice.
pub(crate) fn version_view(row: &Map<String, Value>) -> VersionView {
    let metadata = row.get("version_metadata").and_then(object);
    let origin = string_at(metadata, "baseline_origin");
    let current_version_id = string_at(metadata, "current_version_id");
    let current_revision_id = string_at(metadata, "current_revision_id");
    let display = metadata
        .and_then(|m| m.get("version_display"))
        .and_then(Value::as_u64)
        .filter(|n| *n <= 1_000_000)
        .map(|n| n as i64);
    let baseline_ready = metadata
        .and_then(|m| m.get("baseline_ready"))
        .and_then(Value::as_bool)
        == Some(true)
        && matches!(
            origin.as_str(),
            "source_pinned_import" | "baseline_today" | "initial_creation"
        )
        && crate::design_version_catalog::version_id(&current_version_id)
        && crate::design_version_catalog::revision_id(&current_revision_id)
        && display.is_some();
    // Positive legacy ordinals remain observable; zero is never baseline evidence.
    let version = if baseline_ready {
        display.unwrap()
    } else {
        finite_ordinal(metadata.and_then(|m| m.get("lead_version"))).unwrap_or(UNVERSIONED)
    };
    let latest = finite_ordinal(metadata.and_then(|m| m.get("latest_version")))
        .unwrap_or(version)
        .max(version);
    let reason = string_at(metadata, "lead_reason");
    let review_status = string_at(metadata, "review_status");
    VersionView {
        version,
        latest,
        restored: latest > version && (baseline_ready || version > 0),
        unversioned: !baseline_ready && version == UNVERSIONED,
        baseline_ready,
        label: if baseline_ready || version > 0 {
            format!("v{version}")
        } else {
            "baseline_required".into()
        },
        baseline_origin: baseline_ready.then_some(origin),
        current_version_id: baseline_ready.then_some(current_version_id),
        current_revision_id: baseline_ready.then_some(current_revision_id),
        changed_since_version: baseline_ready
            .then(|| {
                metadata
                    .and_then(|m| m.get("changed_since_version"))
                    .and_then(Value::as_bool)
            })
            .flatten(),
        review_status: (!review_status.is_empty()).then_some(review_status),
        reason: (!reason.is_empty()).then_some(reason),
    }
}

// ── phase supersession: ONE rule, applied at ingest ─────────────────────

/// Which legacy phases lost the record to a newer terminal Process run, and
/// which side owns it. Applied once, read by `design_health`, the register
/// and the Dashboard — never by rewriting the document underneath them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PhaseSupersession {
    pub owner: PhaseOwner,
    pub draft: Option<Superseded>,
    pub sketch: Option<Superseded>,
}

/// A legacy phase that was still open — errored, running, stuck or stale —
/// when a newer terminal Process run landed. Its output is gone.
fn open_legacy_status(status: &str) -> bool {
    matches!(
        status,
        "error" | "processing" | "stale_processing" | "stale"
    )
}

fn legacy_stamp(meta: &Map<String, Value>) -> i64 {
    for key in ["when", "processing_when", "drafted_at", "sketched_at"] {
        let stamp = timestamp_ms(meta.get(key));
        if stamp != 0 {
            return stamp;
        }
    }
    0
}

/// The draft/sketch phase that owns the legacy record: the later of the two.
fn latest_legacy_phase(row: &Map<String, Value>) -> Option<(LegacyPhase, &Map<String, Value>)> {
    let draft = phase(row, "draft_metadata");
    let sketch = phase(row, "sketch_metadata");
    match (draft, sketch) {
        (None, None) => None,
        (None, Some(sketch)) => Some((LegacyPhase::Sketch, sketch)),
        (Some(draft), None) => Some((LegacyPhase::Draft, draft)),
        (Some(draft), Some(sketch)) => {
            let sketch_when =
                timestamp_ms(sketch.get("sketched_at").or_else(|| sketch.get("when")));
            let draft_when = timestamp_ms(draft.get("drafted_at").or_else(|| draft.get("when")));
            if sketch_when >= draft_when {
                Some((LegacyPhase::Sketch, sketch))
            } else {
                Some((LegacyPhase::Draft, draft))
            }
        }
    }
}

fn superseded_of(
    meta: Option<&Map<String, Value>>,
    which: LegacyPhase,
    process_when: i64,
    process_terminal: bool,
) -> Option<Superseded> {
    let meta = meta?;
    let status = phase_status(Some(meta));
    // A document written before this rule was explicit already carries the
    // rewrite the store used to perform. Read it rather than lose the error.
    let already_marked =
        meta.get("superseded_by").and_then(Value::as_str) == Some("process_metadata");
    if already_marked {
        let retained = sanitize_message(&first_message(meta, &["superseded_error_message"]));
        return Some(Superseded {
            phase: which,
            at_ms: timestamp_ms(meta.get("superseded_at")),
            retained_error: (!retained.is_empty()).then_some(retained),
        });
    }
    if !process_terminal || process_when == 0 || !open_legacy_status(status) {
        return None;
    }
    let stamp = legacy_stamp(meta);
    if stamp == 0 || stamp >= process_when {
        return None;
    }
    let retained = if status == "error" {
        sanitize_message(&first_message(meta, &["error_message"]))
    } else {
        String::new()
    };
    Some(Superseded {
        phase: which,
        at_ms: process_when,
        retained_error: (!retained.is_empty()).then_some(retained),
    })
}

fn first_message(meta: &Map<String, Value>, keys: &[&str]) -> String {
    for key in keys {
        let value = meta.get(*key).and_then(Value::as_str).unwrap_or("").trim();
        if !value.is_empty() {
            return value.to_string();
        }
    }
    meta.get("error")
        .and_then(object)
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// The ONE supersession rule. `design_health` calls this instead of reading a
/// document somebody rewrote on the way in.
pub fn phase_supersession(row: &Value) -> PhaseSupersession {
    let empty = Map::new();
    let row = row.as_object().unwrap_or(&empty);
    let process = phase(row, "process_metadata");
    let process_status = phase_status(process);
    let process_terminal = matches!(process_status, "success" | "warning" | "error");
    let process_when = process
        .map(|meta| timestamp_ms(meta.get("when")))
        .unwrap_or(0);

    let draft = superseded_of(
        phase(row, "draft_metadata"),
        LegacyPhase::Draft,
        process_when,
        process_terminal,
    );
    let sketch = superseded_of(
        phase(row, "sketch_metadata"),
        LegacyPhase::Sketch,
        process_when,
        process_terminal,
    );

    // Ownership: success against success is the only ambiguity — one run
    // overwrote the other's output and both say they worked. An error is
    // never reported as superseded ownership; it stays loud.
    let owner = if is_special_row(&string_at(Some(row), "name")) {
        PhaseOwner::None
    } else {
        match latest_legacy_phase(row) {
            Some((_, legacy))
                if phase_status(Some(legacy)) == "success" && process_status == "success" =>
            {
                let legacy_when = timestamp_ms(
                    legacy
                        .get("sketched_at")
                        .or_else(|| legacy.get("drafted_at"))
                        .or_else(|| legacy.get("when")),
                );
                if legacy_when == 0 || process_when == 0 {
                    PhaseOwner::None
                } else if legacy_when > process_when {
                    PhaseOwner::Legacy
                } else {
                    PhaseOwner::Process
                }
            }
            _ => PhaseOwner::None,
        }
    };

    PhaseSupersession {
        owner,
        draft,
        sketch,
    }
}

// ── latest action ───────────────────────────────────────────────────────

struct Candidate {
    label_key: &'static str,
    ms: i64,
    user: String,
    rank: ActionRank,
}

fn push_candidate(
    out: &mut Vec<Candidate>,
    label_key: &'static str,
    raw: Option<&Value>,
    user: &str,
    rank: ActionRank,
) {
    let ms = timestamp_ms(raw);
    if ms == 0 {
        return;
    }
    out.push(Candidate {
        label_key,
        ms,
        user: user.to_string(),
        rank,
    });
}

const PHASE_ACTION_KEYS: [(&str, &str, &str, &str); 5] = [
    (
        "draft_metadata",
        "dsr_action_draft",
        "dsr_action_draft_progress",
        "dsr_action_draft_superseded",
    ),
    (
        "sketch_metadata",
        "dsr_action_sketch",
        "dsr_action_sketch_progress",
        "dsr_action_sketch_superseded",
    ),
    (
        "process_metadata",
        "dsr_action_process",
        "dsr_action_process_progress",
        "dsr_action_process_superseded",
    ),
    (
        "report_metadata",
        "dsr_action_report",
        "dsr_action_report_progress",
        "dsr_action_report_superseded",
    ),
    (
        "combined_inclusion_metadata",
        "dsr_action_combined",
        "dsr_action_combined_progress",
        "dsr_action_combined_superseded",
    ),
];

const UPLOAD_ACTION_KEYS: [(&str, &str); 3] = [
    ("draft", "dsr_action_draft_upload"),
    ("sketch", "dsr_action_sketch_upload"),
    ("process", "dsr_action_process_upload"),
];

fn candidates(row: &Map<String, Value>, client: &Client) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let metadata = row.get("metadata").and_then(object);
    push_candidate(
        &mut out,
        "dsr_action_document_update",
        metadata.and_then(|m| m.get("updated_at")),
        &string_at(metadata, "updated_by"),
        ActionRank::DocumentMetadata,
    );
    if client.dirty || !client.server_known {
        push_candidate(
            &mut out,
            "dsr_action_browser_edit",
            client
                .updated_at_ms
                .map(Value::from)
                .as_ref()
                .or_else(|| row.get("client_updated_at")),
            "",
            ActionRank::BrowserEdit,
        );
    }
    for (key, label, progress_label, superseded_label) in PHASE_ACTION_KEYS {
        let Some(meta) = phase(row, key) else {
            continue;
        };
        let user = string_at(Some(meta), "user");
        for field in ["when", "drafted_at", "sketched_at"] {
            push_candidate(&mut out, label, meta.get(field), &user, ActionRank::Phase);
        }
        push_candidate(
            &mut out,
            progress_label,
            meta.get("progress")
                .and_then(object)
                .and_then(|progress| progress.get("updated_at")),
            &user,
            ActionRank::Phase,
        );
        push_candidate(
            &mut out,
            superseded_label,
            meta.get("superseded_at"),
            &user,
            ActionRank::Phase,
        );
    }
    let uploads = row.get("uploads").and_then(object);
    for (slot, label) in UPLOAD_ACTION_KEYS {
        let Some(items) = uploads
            .and_then(|uploads| uploads.get(slot))
            .and_then(Value::as_array)
        else {
            continue;
        };
        for upload in items {
            let upload = upload.as_object();
            push_candidate(
                &mut out,
                label,
                upload.and_then(|upload| upload.get("uploaded_at")),
                &string_at(upload, "uploaded_by"),
                ActionRank::Upload,
            );
        }
    }
    if let Some(reports) = row.get("combined_reports").and_then(object) {
        for report in reports.values() {
            let report = report.as_object();
            push_candidate(
                &mut out,
                "dsr_action_combined_report",
                report.and_then(|report| report.get("when")),
                &string_at(report, "user"),
                ActionRank::Phase,
            );
        }
    }
    out
}

/// The register's attribution chain when the winning candidate names nobody.
fn attributed_user(row: &Map<String, Value>, action_user: &str) -> String {
    if !action_user.is_empty() {
        return action_user.to_string();
    }
    let metadata = string_at(row.get("metadata").and_then(object), "updated_by");
    if !metadata.is_empty() {
        return metadata;
    }
    for key in [
        "process_metadata",
        "report_metadata",
        "combined_inclusion_metadata",
        "draft_metadata",
        "sketch_metadata",
    ] {
        let user = string_at(phase(row, key), "user");
        if !user.is_empty() {
            return user;
        }
    }
    String::new()
}

fn latest_action(row: &Map<String, Value>, client: &Client, now_ms: i64) -> LatestAction {
    let mut found = candidates(row, client);
    // Time, then a known user, then the ladder — exactly the old sort.
    found.sort_by(|a, b| {
        b.ms.cmp(&a.ms)
            .then(a.user.is_empty().cmp(&b.user.is_empty()))
            .then(b.rank.cmp(&a.rank))
    });
    let Some(best) = found.first() else {
        return LatestAction {
            label_key: String::new(),
            when_ms: 0,
            user: String::new(),
            rank: None,
            age_ms: None,
            attributed_user: attributed_user(row, ""),
        };
    };
    LatestAction {
        label_key: best.label_key.to_string(),
        when_ms: best.ms,
        user: best.user.clone(),
        rank: Some(best.rank),
        age_ms: (now_ms > 0).then(|| now_ms - best.ms),
        attributed_user: attributed_user(row, &best.user),
    }
}

pub(crate) fn phase_stamp(meta: Option<&Map<String, Value>>) -> Option<Stamp> {
    let meta = meta?;
    let when_ms = ["when", "processing_when", "drafted_at", "sketched_at"]
        .into_iter()
        .map(|key| timestamp_ms(meta.get(key)))
        .find(|stamp| *stamp > 0)
        .unwrap_or(0);
    Some(Stamp {
        user: string_at(Some(meta), "user"),
        when_ms,
    })
}

fn phase_bylines(row: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for (name, key) in [
        ("draft", "draft_metadata"),
        ("sketch", "sketch_metadata"),
        ("process", "process_metadata"),
        ("report", "report_metadata"),
    ] {
        if let Some(stamp) = phase_stamp(phase(row, key)) {
            out.insert(
                name.into(),
                serde_json::to_value(stamp).unwrap_or(Value::Null),
            );
        }
    }
    out
}

fn drafting_source(row: &Map<String, Value>) -> Option<String> {
    ["process_metadata", "draft_metadata", "sketch_metadata"]
        .into_iter()
        .filter_map(|key| {
            let meta = phase(row, key)?;
            let source = string_at(Some(meta), "drafting_source");
            (!source.is_empty()).then(|| (legacy_stamp(meta), source))
        })
        .max_by_key(|(stamp, _)| *stamp)
        .map(|(_, source)| source)
}

fn creation(row: &Map<String, Value>, latest: &LatestAction) -> Stamp {
    for key in ["draft_metadata", "sketch_metadata"] {
        if let Some(stamp) = phase_stamp(phase(row, key)) {
            if !stamp.user.is_empty() || stamp.when_ms > 0 {
                return stamp;
            }
        }
    }
    Stamp {
        user: latest.attributed_user.clone(),
        when_ms: latest.when_ms,
    }
}

// ── governance: ds-brain's verdict, folded and labelled ─────────────────

const EDIT_LOCK_CODES: [&str; 2] = ["TRANSFORMER_EDIT_LOCKED", "TRANSFORMER_FINAL_LOCKED"];

fn governance(row: &Map<String, Value>, locally_locked: bool) -> Governance {
    let Some(computed) = row.get("computed").and_then(object) else {
        // No verdict was published at all. Not open — unjudged.
        return Governance {
            entry_blocked: false,
            state: GovernanceState::Unknown,
            code: String::new(),
            message_key: "dsr_gov_unavailable".into(),
            message: String::new(),
            locally_overridden: false,
        };
    };
    let code = string_at(Some(computed), "write_block_code");
    let message = sanitize_message(&string_at(Some(computed), "write_block_message"));
    let edit_locked = computed.get("edit_locked") == Some(&Value::Bool(true))
        || EDIT_LOCK_CODES.contains(&code.as_str());
    if edit_locked {
        return Governance {
            entry_blocked: computed.get("write_blocked") == Some(&Value::Bool(true))
                && code == "TRANSFORMER_FINAL_LOCKED",
            state: GovernanceState::Locked,
            code,
            message_key: "dsr_gov_locked".into(),
            message,
            locally_overridden: false,
        };
    }
    if computed.get("write_blocked") == Some(&Value::Bool(true)) {
        return Governance {
            entry_blocked: false,
            state: GovernanceState::Blocked,
            code,
            message_key: "dsr_gov_blocked".into(),
            message,
            locally_overridden: false,
        };
    }
    if locally_locked {
        // This browser holds the lease and the server has not caught up. An
        // override never reports a lock the server already denies — it is
        // reachable only from Open.
        return Governance {
            entry_blocked: false,
            state: GovernanceState::Locked,
            code,
            message_key: "dsr_gov_live_edit_lock".into(),
            message: String::new(),
            locally_overridden: true,
        };
    }
    Governance {
        entry_blocked: false,
        state: GovernanceState::Open,
        code,
        message_key: "dsr_gov_open".into(),
        message,
        locally_overridden: false,
    }
}

// ── retry: ds-brain decides, this reads ─────────────────────────────────

fn has_saved_layers(row: &Map<String, Value>) -> bool {
    row.get("layers")
        .and_then(object)
        .map(|layers| {
            layers
                .values()
                .any(|count| count.as_f64().is_some_and(|count| count > 0.0))
        })
        .unwrap_or(false)
}

fn retry(row: &Map<String, Value>) -> Retry {
    let capability = row
        .get("retry_capabilities")
        .and_then(object)
        .and_then(|caps| caps.get("process"))
        .and_then(object);
    let source = capability
        .map(|caps| string_at(Some(caps), "source").to_ascii_lowercase())
        .unwrap_or_default();
    let replay = source == "firestore_replay";
    match capability
        .and_then(|caps| caps.get("can_retry"))
        .and_then(Value::as_bool)
    {
        // The published verdict is authoritative when it exists. A saved
        // layer count is not an appeal against it.
        Some(false) => Retry {
            allowed: false,
            source: RetrySource::None,
            reason: RetryReason::BackendRefused,
        },
        Some(true) if replay => Retry {
            allowed: true,
            source: RetrySource::FirestoreReplay,
            reason: RetryReason::BackendAllowed,
        },
        Some(true) => Retry {
            allowed: false,
            source: RetrySource::None,
            reason: RetryReason::BackendSourceUnsupported,
        },
        // Older documents published no verdict; only then may the row answer.
        None if replay => Retry {
            allowed: true,
            source: RetrySource::FirestoreReplay,
            reason: RetryReason::BackendSourceOnly,
        },
        None if has_saved_layers(row) => Retry {
            allowed: true,
            source: RetrySource::FirestoreReplay,
            reason: RetryReason::SavedLayers,
        },
        None if string_at(phase(row, "process_metadata"), "drafting_source")
            .eq_ignore_ascii_case("ds-web") =>
        {
            Retry {
                allowed: true,
                source: RetrySource::FirestoreReplay,
                reason: RetryReason::DraftingSource,
            }
        }
        None => Retry {
            allowed: false,
            source: RetrySource::None,
            reason: RetryReason::NoSource,
        },
    }
}

fn fence(row: &Map<String, Value>) -> Fence {
    let metadata = row.get("metadata").and_then(object);
    let server_version = finite_ordinal(metadata.and_then(|value| value.get("version")))
        .filter(|version| *version > 0);
    let content_digest = metadata
        .and_then(|value| value.get("content_digest"))
        .and_then(Value::as_str)
        .filter(|digest| {
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .map(str::to_owned);
    Fence {
        present: server_version.is_some() && content_digest.is_some(),
        server_version,
        content_digest,
    }
}

// ── presence ────────────────────────────────────────────────────────────

fn presence(row: &Map<String, Value>, current_user: &str) -> Option<Presence> {
    let presence = row
        .get("computed")
        .and_then(object)
        .and_then(|computed| computed.get("presence"))
        .and_then(object)?;
    let user = string_at(Some(presence), "locked_by").to_ascii_lowercase();
    if user.is_empty() {
        return None;
    }
    let hold = presence.get("hold") == Some(&Value::Bool(true));
    let locked_at = presence.get("locked_at");
    let since = if hold {
        presence.get("unsaved_since").or(locked_at)
    } else {
        locked_at
    };
    let since_ms = timestamp_ms(since);
    let last_seen = timestamp_ms(presence.get("last_seen"));
    let locked_at_ms = timestamp_ms(locked_at);
    Some(Presence {
        mine: !current_user.is_empty() && current_user == user,
        user,
        hold,
        since_ms: (since_ms != 0).then_some(since_ms),
        last_seen_ms: if last_seen != 0 {
            Some(last_seen)
        } else {
            (locked_at_ms != 0).then_some(locked_at_ms)
        },
        session_live: !string_at(Some(presence), "session_id").is_empty(),
        feature_counts: presence
            .get("feature_counts")
            .and_then(object)
            .cloned()
            .unwrap_or_default(),
    })
}

// ── the fold ────────────────────────────────────────────────────────────

struct Entry {
    name: String,
    origin: RowOrigin,
    server_index: Option<usize>,
    header_index: Option<usize>,
    client: Client,
}

fn client_from_row(row: &Map<String, Value>) -> Client {
    Client {
        cached: row.get("client_cached") == Some(&Value::Bool(true)),
        dirty: row.get("client_dirty") == Some(&Value::Bool(true)),
        server_known: row.get("client_server_known") != Some(&Value::Bool(false)),
        local: row.get("client_local") == Some(&Value::Bool(true)),
        base_version: finite_ordinal(row.get("client_base_server_version")),
        updated_at_ms: match row.get("client_updated_at") {
            Some(Value::Number(number)) => number.as_i64(),
            _ => None,
        },
        foreign_room: row
            .get("client_foreign_room")
            .and_then(object)
            .map(|room| ForeignRoom {
                owner: string_at(Some(room), "owner"),
                updated_at: match room.get("updated_at") {
                    Some(Value::Number(number)) => number.as_i64(),
                    _ => None,
                },
            }),
    }
}

fn merge(request: &Request) -> (Vec<Entry>, Vec<PresenceRoom>, bool) {
    let empty = Map::new();
    let current_user = request.current_user_email.trim().to_ascii_lowercase();
    let include_all = request.room_targets.is_empty();
    let targets: Vec<String> = request
        .room_targets
        .iter()
        .map(|name| canonical_row_name(name))
        .collect();

    let mut order: Vec<String> = Vec::new();
    let mut entries: Vec<Entry> = Vec::new();
    let mut index_of = std::collections::HashMap::<String, usize>::new();

    for (index, row) in request.server_rows.iter().enumerate() {
        let row = row.as_object().unwrap_or(&empty);
        let name = row.get("name").and_then(Value::as_str).unwrap_or_default();
        let key = canonical_row_name(name);
        let entry = Entry {
            name: name.to_string(),
            origin: RowOrigin::Server,
            server_index: Some(index),
            header_index: None,
            client: client_from_row(row),
        };
        // A Map keyed by canonical name keeps the FIRST position and the LAST
        // value, which is what the browser's `byName` did.
        match index_of.get(&key) {
            Some(at) => entries[*at] = entry,
            None => {
                index_of.insert(key.clone(), entries.len());
                order.push(key);
                entries.push(entry);
            }
        }
    }

    let mut presence_rooms: Vec<PresenceRoom> = Vec::new();
    for (index, header) in request.local_room_headers.iter().enumerate() {
        if header.transformer_name.is_empty() {
            continue;
        }
        let key = canonical_row_name(&header.transformer_name);
        if !include_all && !targets.contains(&key) {
            continue;
        }
        let owner = header
            .owner_email
            .as_deref()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        // Rooms are per browser origin, not per account (owner ruling
        // 2026-08-26). Another account's room is THEIR copy, never this
        // user's unsaved work, and its lease is not ours to reconcile.
        if !owner.is_empty() && !current_user.is_empty() && owner != current_user {
            if let Some(at) = index_of.get(&key) {
                entries[*at].client.foreign_room = Some(ForeignRoom {
                    owner,
                    updated_at: header.updated_at_ms,
                });
            }
            continue;
        }
        let local_dirty = header.local_dirty || !header.server_known;
        // A server row proves the document exists even when a stale header
        // still says "server unknown".
        let server_known = header.server_known
            || index_of
                .get(&key)
                .map(|at| entries[*at].client.server_known)
                .unwrap_or(false);
        presence_rooms.push(PresenceRoom {
            name: header.transformer_name.clone(),
            updated_at_ms: header.updated_at_ms.unwrap_or(0),
            server_known,
            dirty: local_dirty,
            feature_counts: header.layer_feature_counts.clone(),
        });
        if !local_dirty {
            // Clean and cached: the availability fact only. A clean room is
            // server-known by definition, so a missing server row means the
            // transformer was deleted server-side — it must not be
            // resurrected in the register by the local copy.
            if let Some(at) = index_of.get(&key) {
                entries[*at].client.cached = true;
            }
            continue;
        }
        match index_of.get(&key) {
            Some(at) => {
                let entry = &mut entries[*at];
                entry.header_index = Some(index);
                entry.client.cached = true;
                entry.client.local = true;
                entry.client.dirty = true;
                entry.client.server_known = server_known;
                if header.base_server_version.is_some() {
                    entry.client.base_version = header.base_server_version;
                }
                entry.client.updated_at_ms = header.updated_at_ms;
            }
            None => {
                index_of.insert(key.clone(), entries.len());
                order.push(key);
                entries.push(Entry {
                    name: header.transformer_name.clone(),
                    origin: RowOrigin::BrowserOnly,
                    server_index: None,
                    header_index: Some(index),
                    client: Client {
                        cached: true,
                        dirty: true,
                        server_known,
                        local: true,
                        base_version: header.base_server_version,
                        updated_at_ms: header.updated_at_ms,
                        foreign_room: None,
                    },
                });
            }
        }
    }
    (entries, presence_rooms, include_all)
}

/// The effective document for a row: the server's, with the local room's
/// process stamp where the room holds newer output, or a synthetic record for
/// a row that exists only in this browser.
fn effective_row(request: &Request, entry: &Entry) -> Value {
    let mut row = entry
        .server_index
        .and_then(|index| request.server_rows.get(index))
        .cloned()
        .unwrap_or_else(|| {
            let header = entry
                .header_index
                .and_then(|i| request.local_room_headers.get(i));
            let mut synthetic = Map::new();
            synthetic.insert("name".into(), Value::String(entry.name.clone()));
            synthetic.insert("kind".into(), Value::String("individual".into()));
            if let Some(header) = header {
                synthetic.insert(
                    "layers".into(),
                    Value::Object(header.layer_feature_counts.clone()),
                );
            }
            Value::Object(synthetic)
        });
    if let (Some(index), Some(object)) = (entry.header_index, row.as_object_mut()) {
        if let Some(process) = request
            .local_room_headers
            .get(index)
            .and_then(|header| header.process_metadata.clone())
        {
            object.insert("process_metadata".into(), process);
        }
    }
    row
}

fn row_reply(request: &Request, entry: Entry, current_user: &str) -> RowReply {
    let document = effective_row(request, &entry);
    let empty = Map::new();
    let row = document.as_object().unwrap_or(&empty);
    let client = entry.client;

    let active = request
        .active_session_transformer
        .as_deref()
        .map(canonical_row_name)
        .unwrap_or_default();
    let key = canonical_row_name(&entry.name);
    let active_edit_dirty = !active.is_empty() && active == key;
    let sync_state = if client.dirty || !client.server_known || active_edit_dirty {
        SyncState::Unsaved
    } else {
        SyncState::Saved
    };
    let sync_title_key = match sync_state {
        SyncState::Saved => "dsr_sync_saved_title",
        SyncState::Unsaved if !client.server_known => "dsr_sync_browser_only_title",
        SyncState::Unsaved if client.dirty => "dsr_sync_local_edits_title",
        // An active edit session is unsaved work held in memory; it has no
        // sentence of its own beyond the badge's own word.
        SyncState::Unsaved => "ui_unsaved",
    };
    // An active edit session counts as unsaved here even before it reaches a
    // room, so "Local copy" never contradicts the amber badge.
    let locality = match sync_state {
        SyncState::Unsaved => Locality::Unsaved,
        SyncState::Saved if client.cached => Locality::Local,
        SyncState::Saved => Locality::Remote,
    };
    let process = phase(row, "process_metadata");
    let computed = row.get("computed").and_then(object);
    let supersession = phase_supersession(&document);
    let locally_locked = request
        .local_edit_lock
        .as_ref()
        .map(|lock| canonical_row_name(&lock.transformer) == key)
        .unwrap_or(false);

    let superseded = match (supersession.draft.clone(), supersession.sketch.clone()) {
        (Some(draft), Some(sketch)) => {
            // Both lost the record; the one that OWNED it is the later.
            Some(match latest_legacy_phase(row) {
                Some((LegacyPhase::Sketch, _)) => sketch,
                _ => draft,
            })
        }
        (draft, sketch) => draft.or(sketch),
    };
    let latest = latest_action(row, &client, request.now_ms);
    let legacy_phase = latest_legacy_phase(row).map(|(phase, _)| phase);
    let process_settings_used = phase(row, "process_metadata")
        .and_then(|meta| meta.get("settings"))
        .and_then(object)
        .cloned()
        .unwrap_or_default();
    let mut saved_layer_names = row
        .get("layers")
        .and_then(object)
        .map(|layers| layers.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    saved_layer_names.sort();
    let present = presence(row, current_user);
    let mut badges = Vec::new();
    if !is_special_row(&entry.name) {
        badges.push(Badge {
            key: "sync".into(),
            label_key: match sync_state {
                SyncState::Saved => "dsr_badge_saved",
                SyncState::Unsaved => "dsr_badge_unsaved",
            }
            .into(),
            tone: match sync_state {
                SyncState::Saved => "slate",
                SyncState::Unsaved => "amber",
            }
            .into(),
            user: None,
        });
        if let Some(presence) = &present {
            if !presence.mine {
                badges.push(Badge {
                    key: "presence".into(),
                    label_key: if presence.hold {
                        "dsr_badge_user_editing"
                    } else {
                        "dsr_badge_user_editing_now"
                    }
                    .into(),
                    tone: "violet".into(),
                    user: Some(presence.user.clone()),
                });
            } else if presence.hold && sync_state == SyncState::Saved {
                badges.push(Badge {
                    key: "presence".into(),
                    label_key: "dsr_badge_you_editing_elsewhere".into(),
                    tone: "amber".into(),
                    user: None,
                });
            }
        }
        if let Some(room) = &client.foreign_room {
            badges.push(Badge {
                key: "foreign_room".into(),
                label_key: "dsr_badge_foreign_room".into(),
                tone: "slate".into(),
                user: Some(room.owner.clone()),
            });
        }
    }

    RowReply {
        origin: entry.origin,
        server_index: entry.server_index,
        header_index: entry.header_index,
        view: View {
            sync_state,
            sync_title_key: sync_title_key.into(),
            locality,
            combined_state: combined_state(
                computed.and_then(|c| c.get("combined_inclusion_state")),
            )
            .unwrap_or(CombinedState::Stale),
            report_state: combined_state(computed.and_then(|c| c.get("report_state")))
                .unwrap_or(CombinedState::Stale),
            report_has_files: computed.and_then(|c| c.get("report_has_files"))
                == Some(&Value::Bool(true)),
            version: version_view(row),
            presence: present,
            foreign_room: client.foreign_room.clone(),
            show_sync_badge: !is_special_row(&entry.name),
            server_known: client.server_known,
            client_dirty: client.dirty,
            active_edit_dirty,
            process_status: phase_status(process).to_string(),
            report_status: phase_status(phase(row, "report_metadata")).to_string(),
            badges,
            locality_hint_key: (locality == Locality::Local).then(|| "dsr_local_copy".into()),
            voltage_drop: (!is_special_row(&entry.name))
                .then(|| voltage_drop_view(row, client.dirty)),
        },
        phase_ownership: PhaseOwnership {
            owner: supersession.owner,
            superseded,
        },
        legacy_phase,
        creation: creation(row, &latest),
        phase_bylines: phase_bylines(row),
        drafting_source: drafting_source(row),
        process_settings_used,
        saved_layer_names,
        latest_action: latest,
        governance: governance(row, locally_locked),
        retry: retry(row),
        fence: fence(row),
        kind: row_kind(&entry.name),
        allows: row_allows(&entry.name),
        client,
        name: entry.name,
    }
}

fn build(request: Request) -> Result<Reply, String> {
    if request.schema != SCHEMA {
        return Err("Unsupported design status row schema.".into());
    }
    let current_user = request.current_user_email.trim().to_ascii_lowercase();
    let (entries, presence_rooms, presence_complete) = merge(&request);
    let rows = entries
        .into_iter()
        .map(|entry| row_reply(&request, entry, &current_user))
        .collect();
    Ok(Reply {
        schema: SCHEMA,
        rows,
        presence_rooms,
        presence_complete,
    })
}

/// The truth of these rows under these editor facts, with no browser rooms —
/// what `design_status_query` reads per row. Rows come back in merge order; a
/// duplicate canonical name keeps the first slot and the last value, as the
/// browser's `byName` did, so read them by `server_index` or canonical name.
pub fn truths(
    rows: &[Value],
    active_session_transformer: Option<&str>,
    current_user_email: &str,
    local_edit_lock: Option<&str>,
    now_ms: i64,
) -> Vec<RowReply> {
    let request = Request {
        schema: SCHEMA.to_string(),
        server_rows: rows.to_vec(),
        local_room_headers: Vec::new(),
        active_session_transformer: active_session_transformer
            .filter(|name| !name.is_empty())
            .map(str::to_string),
        current_user_email: current_user_email.to_string(),
        now_ms,
        local_edit_lock: local_edit_lock
            .filter(|name| !name.is_empty())
            .map(|transformer| LocalEditLock {
                transformer: transformer.to_string(),
            }),
        room_targets: Vec::new(),
    };
    build(request).map(|reply| reply.rows).unwrap_or_default()
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("Design status row request exceeds its bound.".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid design status row request: {error}"))?;
    serde_json::to_string(&build(request)?).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reply(request: Value) -> Value {
        serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).expect("reply"))
            .expect("reply parses")
    }

    fn request(rows: Value) -> Value {
        json!({"schema": SCHEMA, "server_rows": rows})
    }

    #[test]
    fn only_a_final_server_write_lock_blocks_edit_entry() {
        for (code, blocked, expected) in [
            ("TRANSFORMER_FINAL_LOCKED", true, true),
            ("TRANSFORMER_FINAL_LOCKED", false, false),
            ("TRANSFORMER_EDIT_LOCKED", true, false),
            ("PROJECT_EXPIRED", true, false),
            ("", false, false),
        ] {
            let row = json!({"computed":{"write_blocked":blocked,"write_block_code":code}});
            assert_eq!(
                governance(row.as_object().unwrap(), false).entry_blocked,
                expected
            );
        }
        assert!(!governance(json!({}).as_object().unwrap(), true).entry_blocked);
    }

    /// The register shows each transformer's analysis as printing classifies
    /// it, labelled, with the engine's own reason when it was not calculated.
    #[test]
    fn each_transformer_row_shows_its_voltage_drop_analysis() {
        let saved = |status: &str, summary: Value| {
            json!({"name": format!("tx_{status}"),
                "metadata": {"version": 3, "content_digest": "a".repeat(64)},
                "voltage_drop_metadata": {"state": "ready",
                    "schema": "ds.lv-voltage-drop.analysis/v1", "method": "ds-lv-vd/1",
                    "status": status, "nature": null, "content_digest": "a".repeat(64),
                    "analysis_sha256": "b".repeat(64), "inputs_digest": "c".repeat(64),
                    "analysis_inputs_digest": "d".repeat(64), "processed_digest": "e".repeat(64),
                    "json_bytes": 2048, "summary": summary}})
        };
        let answer = reply(request(json!([
            saved("calculated", json!({"method": "ds-lv-vd/1", "status": "calculated",
                "transformer_nature": null, "verdict": "complies", "issues": []})),
            saved("refused", json!({"method": "ds-lv-vd/1", "status": "not_calculated",
                "transformer_nature": null, "verdict": "incomplete",
                "issues": ["the network has no transformer node to start from"]})),
            {"name": "tx_local", "client_dirty": true},
            {"name": "tx_never"},
            {"name": "mv_data"}
        ])));
        let view = |name: &str| {
            answer["rows"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["name"] == name)
                .unwrap()["view"]["voltage_drop"]
                .clone()
        };
        assert_eq!(
            view("tx_calculated"),
            json!({"state": "saved_json", "calculated": true, "verdict": "complies",
                "label_key": "dsr_vd_complies", "tone": "emerald"})
        );
        assert_eq!(
            view("tx_refused"),
            json!({"state": "saved_json", "calculated": false, "verdict": "refused",
                "reason": "the network has no transformer node to start from",
                "label_key": "dsr_vd_not_calculated", "tone": "amber"})
        );
        assert_eq!(view("tx_local")["state"], "local_changes");
        assert_eq!(view("tx_local")["label_key"], "dsr_vd_local_changes");
        assert_eq!(view("tx_never")["label_key"], "dsr_vd_missing");
        assert!(
            view("mv_data").is_null(),
            "an aggregate row has no analysis"
        );
    }

    #[test]
    fn reserved_voltage_drop_is_neutral_and_names_do_not_override_saved_analysis() {
        for nature in ["fill_in", "upgrade"] {
            let mut row = json!({"name": "tx_reserved",
                "metadata": {"version": 2, "content_digest": "a".repeat(64)},
                "voltage_drop_metadata": {"state": "ready",
                    "schema": "ds.lv-voltage-drop.analysis/v1", "method": "ds-lv-vd/1",
                    "status": "reserved", "nature": nature, "content_digest": "a".repeat(64),
                    "analysis_sha256": "b".repeat(64), "inputs_digest": "c".repeat(64),
                    "analysis_inputs_digest": "d".repeat(64), "processed_digest": "e".repeat(64),
                    "json_bytes": 2048, "summary": {"method": "ds-lv-vd/1", "status": "reserved",
                        "transformer_nature": nature, "verdict": "not_calculated",
                        "calculation": "Existing network not fully surveyed"}}});
            let reserved = voltage_drop_view(row.as_object().unwrap(), false);
            assert!(!reserved.calculated);
            assert_eq!(reserved.verdict.as_deref(), Some("reserved"));
            assert_eq!(reserved.label_key, "dsr_vd_reserved");
            assert_eq!(reserved.tone, "slate");
            assert_eq!(reserved.reason.as_deref(), Some("Existing network not fully surveyed"));

            // A positive calculation override can produce a real failure for
            // either nature. Its saved verdict must still be shown in red.
            row["voltage_drop_metadata"]["status"] = json!("calculated");
            row["voltage_drop_metadata"]["summary"]["status"] = json!("calculated");
            row["voltage_drop_metadata"]["summary"]["verdict"] = json!("does_not_comply");
            row["name"] = json!(format!("{nature}_named_site"));
            let failed = voltage_drop_view(row.as_object().unwrap(), false);
            assert!(failed.calculated);
            assert_eq!(failed.label_key, "dsr_vd_does_not_comply");
            assert_eq!(failed.tone, "red");

            // Legacy classification does not invent an authoritative tag or
            // turn a refused saved calculation into a policy reservation.
            row["metadata"]["transfo_type"] = json!(nature);
            row["voltage_drop_metadata"]["nature"] = Value::Null;
            row["voltage_drop_metadata"]["status"] = json!("refused");
            row["voltage_drop_metadata"]["summary"]["transformer_nature"] = Value::Null;
            row["voltage_drop_metadata"]["summary"]["status"] = json!("not_calculated");
            row["voltage_drop_metadata"]["summary"]["issues"] = json!(["Missing transformer node"]);
            let unavailable = voltage_drop_view(row.as_object().unwrap(), false);
            assert!(!unavailable.calculated);
            assert_eq!(unavailable.verdict.as_deref(), Some("refused"));
            assert_eq!(unavailable.tone, "amber");
            assert_eq!(unavailable.reason.as_deref(), Some("Missing transformer node"));
        }
    }

    #[test]
    fn server_rows_keep_their_first_position_and_their_last_value() {
        // The browser's `byName` map: a duplicate canonical name replaced the
        // value in place and never moved the row.
        let answer = reply(request(json!([
            {"name": "tx_1", "computed": {"combined_inclusion_state": "error"}},
            {"name": "tx_2"},
            {"name": "TX-1", "computed": {"combined_inclusion_state": "success"}}
        ])));
        let rows = answer["rows"].as_array().expect("rows");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["name"], "TX-1");
        assert_eq!(rows[0]["view"]["combined_state"], "success");
        assert_eq!(rows[1]["name"], "tx_2");
    }

    #[test]
    fn the_presence_reconciliation_is_returned_never_fired() {
        let answer = reply(json!({
            "schema": SCHEMA,
            "server_rows": [{"name": "tx_1"}],
            "local_room_headers": [
                {"transformer_name": "tx_1", "local_dirty": true, "server_known": true,
                 "updated_at_ms": 1_700_000_000_000i64, "layer_feature_counts": {"lv_poles": 4}},
                {"transformer_name": "tx_2", "local_dirty": false, "server_known": true,
                 "updated_at_ms": 1_700_000_001_000i64}
            ],
            "current_user_email": "me@x.com"
        }));
        // Both rooms are reconciled, including the clean one whose server row
        // is gone — and that room does NOT become a register row.
        let rooms = answer["presence_rooms"].as_array().expect("rooms");
        assert_eq!(rooms.len(), 2);
        assert_eq!(rooms[0]["dirty"], true);
        assert_eq!(rooms[1]["dirty"], false);
        assert_eq!(answer["presence_complete"], true);
        assert_eq!(answer["rows"].as_array().expect("rows").len(), 1);
    }

    #[test]
    fn a_targeted_refresh_is_never_a_complete_reconciliation() {
        let answer = reply(json!({
            "schema": SCHEMA,
            "server_rows": [{"name": "tx_1"}, {"name": "tx_2"}],
            "local_room_headers": [
                {"transformer_name": "tx_1", "local_dirty": true, "server_known": true},
                {"transformer_name": "tx_2", "local_dirty": true, "server_known": true}
            ],
            "room_targets": ["TX-2"]
        }));
        let rooms = answer["presence_rooms"].as_array().expect("rooms");
        assert_eq!(rooms.len(), 1);
        assert_eq!(rooms[0]["name"], "tx_2");
        assert_eq!(answer["presence_complete"], false);
        // The untargeted row keeps the server's own client truth.
        assert_eq!(answer["rows"][0]["client"]["dirty"], false);
    }

    #[test]
    fn one_supersession_rule_answers_both_readers() {
        let row = json!({
            "name": "tx_1",
            "draft_metadata": {"status": "error", "when": "2026-08-20T10:00:00Z",
                               "error_message": "Drafting failed."},
            "process_metadata": {"status": "success", "when": "2026-08-25T10:00:00Z"}
        });
        let supersession = phase_supersession(&row);
        let draft = supersession.draft.expect("draft lost the record");
        assert_eq!(draft.phase, LegacyPhase::Draft);
        assert_eq!(draft.retained_error.as_deref(), Some("Drafting failed."));
        // Ownership is the success-against-success question and this is not
        // one: an error is never reported as superseded ownership.
        assert_eq!(supersession.owner, PhaseOwner::None);
        // And `design_health` reads the same rule rather than a rewritten doc.
        let health = crate::design_health::transformer_health(&row);
        assert_eq!(health.severity, crate::design_health::Severity::Error);
        assert_eq!(
            health.findings[0].code,
            crate::design_health::LEGACY_PHASE_SUPERSEDED
        );
        assert_eq!(health.findings[0].message, "Drafting failed.");
    }

    #[test]
    fn a_browser_only_row_points_at_the_header_that_makes_it() {
        let answer = reply(json!({
            "schema": SCHEMA,
            "server_rows": [],
            "local_room_headers": [{
                "transformer_name": "tx_new", "local_dirty": true, "server_known": false,
                "updated_at_ms": 7, "layer_feature_counts": {"lv_poles": 3},
                "process_metadata": {"status": "success", "when": "2026-08-26T11:20:00Z"}
            }]
        }));
        let row = &answer["rows"][0];
        assert_eq!(row["origin"], "browser_only");
        assert_eq!(row["header_index"], 0);
        assert!(row.get("server_index").is_none());
        assert_eq!(row["kind"], "individual");
        assert_eq!(row["client"]["server_known"], false);
        assert_eq!(row["view"]["sync_state"], "unsaved");
        assert_eq!(row["view"]["sync_title_key"], "dsr_sync_browser_only_title");
        // The room's own process stamp governs the row until a save lands.
        assert_eq!(row["view"]["process_status"], "success");
        // A stored `lane` stamp is wire data other consumers read; the row
        // model never answers it.
        assert!(row["view"].get("lane").is_none());
        assert!(row["view"].get("phase_lanes").is_none());
    }

    #[test]
    fn ds_brains_verdicts_are_read_and_never_re_decided() {
        let answer = reply(request(json!([{
            "name": "tx_1",
            "layers": {"lv_poles": 40},
            "retry_capabilities": {"process": {"can_retry": false, "source": "firestore_replay"}},
            "computed": {"write_blocked": true, "write_block_message": "Project is read-only."}
        }])));
        let row = &answer["rows"][0];
        assert_eq!(
            row["retry"],
            json!({"allowed": false, "source": "none", "reason": "backend_refused"})
        );
        assert_eq!(row["governance"]["state"], "blocked");
        assert_eq!(row["governance"]["message"], "Project is read-only.");
        assert_eq!(row["governance"]["locally_overridden"], false);
    }

    #[test]
    fn a_complete_source_fence_is_explicit_and_malformed_halves_stay_visible() {
        let digest = "a".repeat(64);
        let answer = reply(request(json!([
            {"name":"ready", "metadata":{"version":7,"content_digest":digest}},
            {"name":"missing_digest", "metadata":{"version":3}},
            {"name":"bad_digest", "metadata":{"version":4,"content_digest":"SHA256:ABC"}}
        ])));
        assert_eq!(
            answer["rows"][0]["fence"],
            json!({"server_version":7,"content_digest":"a".repeat(64),"present":true})
        );
        assert_eq!(
            answer["rows"][1]["fence"],
            json!({"server_version":3,"present":false})
        );
        assert_eq!(
            answer["rows"][2]["fence"],
            json!({"server_version":4,"present":false})
        );
    }
}
