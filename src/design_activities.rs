//! Who designed what, where and when — across EVERY project the owner reaches.
//!
//! The owner saw unauthorised activity on their system and needs, for every
//! project they can reach, the complete record of who designed what, where
//! and when. The only source that exists is the per-project transformer
//! status list (ds-brain `/report list_transformers_status`): there is no
//! event log for design writes, and the Dashboard's `recent[8]` is a top-N
//! projection, not a record. So this module owns three things:
//!
//! 1. **The ledger** (`action: "ledger"`). A host captures one project's raw
//!    status rows and the kernel compacts them into a COMPLETE per-transformer
//!    activity ledger: every phase stamp of every transformer, the document
//!    updater, the latest action, the health verdict and the admin bounds.
//!    Nothing is decided here that another module already decided — the
//!    phase maps are read with `design_dashboard`'s own readers, the latest
//!    action is `design_status_row::truths`, the health verdict is
//!    `design_health::transformer_health`, and the design attribution is the
//!    Dashboard's `design_stamp` / `stamp_user`. Special rows (`mv_data`,
//!    `combined_transformer`, `collisions`, …) are excluded by the crate's one
//!    row-identity table and counted only in `row_count`.
//! 2. **Retention** is the host's: it keeps successive ledgers per project.
//! 3. **The fold** (`action: "fold"`). The kernel folds the retained ledgers
//!    of many projects into one answer — users, projects, a complete timeline,
//!    the changes between each project's previous capture and its current
//!    one, anomalies and a handful of facts. The differences between sweeps
//!    ARE the history. This fold is the single authority behind
//!    `ds design activities read`, the MCP surface and the Governance page.
//!
//! Pure by the crate's contract: no clock (`now_ms` and `tz_offset_minutes`
//! are inputs), no randomness, no fetch. Labels are i18n KEYS.
//!
//! What a ledger says that a row does not: `designed` is the Dashboard's
//! design-stamp verdict (any successful compute phase whose output was
//! saved, attributed to its operator, then to the document updater), recorded
//! at capture time because the fold cannot re-run it — the saved-layers fact
//! is not in the ledger. `health.user` is who the Dashboard's attention note
//! charges, so `errors` by user fold exactly as `errors_by_user` counts them.
//!
//! What is NOT claimed, on purpose (docs/contracts/design-activities.md):
//! no device or installation attribution — rows carry none; nothing that
//! happened between two captures is visible; an action that never stamps a
//! row is invisible.
//!
//! The `gda_` label keys this module can emit, exactly:
//! `gda_anomaly_unknown_actor`, `gda_anomaly_stale_capture`, `gda_fact_busiest_actor`,
//! `gda_fact_most_active_project`, `gda_fact_newest_actor`,
//! `gda_fact_untouched_projects`,
//! `gda_fact_biggest_change`, and the plural pairs `gda_project_one`,
//! `gda_project_other`, `gda_stamp_one`, `gda_stamp_other`, `gda_actor_one`,
//! `gda_actor_other`, `gda_change_one`, `gda_change_other`. Timeline, change
//! and `latest` label keys are the status row's own phase action keys
//! (`dsr_action_sketch`, `dsr_action_draft`, `dsr_action_process`,
//! `dsr_action_report`, `dsr_action_combined`, `dsr_action_document_update`,
//! and whatever `latest_action` already emits), allowed as-is.

use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::design::{row_kind, RowKind};
use crate::design_dashboard::{
    self as dashboard, attention_note, design_stamp, locale_compare, object, phase_meta,
    phase_status, row_name, short_user, stamp_user, string_at, timestamp_ms, ActionFacts, Tone,
};
use crate::design_health;
use crate::design_status_row::{self, phase_stamp, RowReply};
use crate::execution_context::valid_project;

/// The request/reply schema of both actions.
pub const SCHEMA: &str = "ds.design-activities/v1";
/// The schema of the ledger a host retains, embedded in every fold.
pub const LEDGER_SCHEMA: &str = "ds.design-activity-ledger/v1";

/// A ledger request carries one project's whole status list, as the
/// Dashboard's does.
const LEDGER_INPUT_BYTES: usize = 32 * 1024 * 1024;
/// A fold carries many ledgers, each already compact.
const FOLD_INPUT_BYTES: usize = 64 * 1024 * 1024;
/// The client-core bound on one project's status list.
const MAX_ROWS: usize = 2000;
const MAX_LIMIT: usize = 5000;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// A capture older than this, against `now_ms`, is stale.
const STALE_AFTER_MS: i64 = DAY_MS;
const RECENT_WINDOW_MS: i64 = 7 * DAY_MS;
const UNTOUCHED_AFTER_MS: i64 = 30 * DAY_MS;
const MAX_FACTS: usize = 5;

// ── the ledger ──────────────────────────────────────────────────────────

/// The health colour of one transformer, as the Dashboard's attention pile
/// paints it: red for an error, amber for a warning or a run in flight or
/// stale, blue for a superseded phase, green when the row raises nothing.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum HealthTone {
    Red,
    Amber,
    Blue,
    #[default]
    Green,
}

/// One phase's stamp: the status the document wrote and, when it carries
/// one, when and by whom.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct PhaseStamp {
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

/// The five phase slots a status row can carry, each present only when the
/// document holds that phase's blob.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct LedgerPhases {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sketch: Option<PhaseStamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft: Option<PhaseStamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub process: Option<PhaseStamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report: Option<PhaseStamp>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub combined: Option<PhaseStamp>,
}

/// The most recent thing that happened to a transformer —
/// `design_status_row`'s `latest_action`, kept only when something happened.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct LatestStamp {
    pub label_key: String,
    pub when_ms: i64,
    pub user: String,
}

/// The Dashboard's design-stamp verdict at capture time: the transformer
/// counts as designed, by this user (the operator, then the document
/// updater) at this time. `user` is absent when nobody could be named.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct DesignedStamp {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub when_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

/// `design_health`'s verdict, reduced to what the fold counts.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct LedgerHealth {
    pub tone: HealthTone,
    pub error_count: usize,
    pub warning_count: usize,
    /// Who the Dashboard's attention note charges with the row's issue —
    /// the failing phase's operator, else the latest action's user. Absent
    /// when the row is green or nobody could be named.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

/// One transformer's complete activity record.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct LedgerTransformer {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub district: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at_ms: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub designed: Option<DesignedStamp>,
    pub phases: LedgerPhases,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest: Option<LatestStamp>,
    pub health: LedgerHealth,
}

/// One project's complete activity ledger at one capture. Read LENIENTLY
/// when embedded in a fold — every member defaults and unknown members are
/// allowed — because a retained ledger may carry members added later.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(default)]
pub struct Ledger {
    pub schema: String,
    pub ds_project: String,
    /// When the host captured the rows, epoch millis.
    pub captured_at_ms: i64,
    /// Every row the list held, special rows included.
    pub row_count: usize,
    /// The transformers actually recorded.
    pub transformer_count: usize,
    pub transformers: Vec<LedgerTransformer>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct LedgerReply {
    pub schema: &'static str,
    pub ledger: Ledger,
}

// ── the fold's reply ────────────────────────────────────────────────────

/// What the captures themselves say: how many, how old, how many with a
/// previous ledger to diff against.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Captured {
    pub project_count: usize,
    pub earliest_ms: i64,
    pub latest_ms: i64,
    /// Captures older than 24 h before `now_ms`.
    pub stale_count: usize,
    pub with_previous: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Totals {
    pub transformers: usize,
    pub designed: usize,
    pub report_ready: usize,
    pub combined: usize,
    pub errors: usize,
    /// Distinct named actors across every ledger.
    pub actors: usize,
    /// Projects whose ledger records at least one action.
    pub projects_with_activity: usize,
    /// Every change between a previous and a current ledger, before the bound.
    pub changes: usize,
}

/// One user's footprint in one project.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct UserProject {
    pub ds_project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub designs: usize,
    pub errors: usize,
    pub transformers_touched: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_ms: Option<i64>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct UserEntry {
    /// The account, or `""` for the unattributed bucket.
    pub name: String,
    pub short: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub unknown: bool,
    pub project_count: usize,
    /// Transformers this user is the design owner of.
    pub designs: usize,
    /// Red transformers charged to this user.
    pub errors: usize,
    pub transformers_touched: usize,
    /// The EARLIEST stamp this user holds in the retained captures — not the
    /// day they joined. A capture carries only the latest stamp per phase, so
    /// a long-standing user whose older stamps have all been overwritten
    /// looks new here. A host must not spell this as a first appearance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_seen_ms: Option<i64>,
    /// Where the earliest retained stamp was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_project: Option<String>,
    /// The latest stamp this user holds. It is not a sign-in, a session or a
    /// presence: a person who read, downloaded or exported all day and
    /// stamped nothing does not move it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_project: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_label_key: Option<String>,
    pub projects: Vec<UserProject>,
}

/// What changed in a project between its previous capture and this one.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct SincePrevious {
    /// The previous capture's time.
    pub captured_at_ms: i64,
    pub designed_delta: i64,
    pub report_ready_delta: i64,
    pub combined_delta: i64,
    pub error_delta: i64,
    pub new_transformers: usize,
    pub removed_transformers: usize,
    /// Stamps that appeared or changed.
    pub stamps_changed: usize,
    /// Short names of actors who appear now and did not before.
    pub new_actors: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct ProjectEntry {
    pub ds_project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    pub captured_at_ms: i64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub stale: bool,
    pub transformers: usize,
    pub designed: usize,
    pub report_ready: usize,
    pub combined: usize,
    pub errors: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health_score: Option<i64>,
    /// Distinct named design owners.
    pub contributors: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest: Option<LatestStamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_previous: Option<SincePrevious>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    NewStamp,
    Restamped,
    NewTransformer,
    RemovedTransformer,
}

/// One difference between a project's previous and current ledgers.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Change {
    pub ds_project: String,
    pub name: String,
    /// `sketch`, `draft`, `process`, `report`, `combined`, or `document` for
    /// the updater stamp and for a transformer appearing or vanishing.
    pub phase: String,
    /// How a reader spells `phase`: the same `dsr_action_*` key the timeline
    /// and the status row already use. A host renders the key, never the
    /// token, or a French page reads `process` in English.
    pub label_key: String,
    pub kind: ChangeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_when_ms: Option<i64>,
}

/// One stamp on the complete timeline.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct TimelineEntry {
    pub when_ms: i64,
    pub user: String,
    pub short: String,
    pub ds_project: String,
    pub name: String,
    pub phase: String,
    pub label_key: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AnomalyKind {
    UnknownActor,
    StaleCapture,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct Anomaly {
    pub kind: AnomalyKind,
    /// `gda_anomaly_<kind>`.
    pub label_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub short: Option<String>,
    pub ds_project: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub when_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FactTone {
    Amber,
    Green,
    Blue,
    Red,
}

/// A sticky note for the Governance page: a key, its parameters, and which
/// parameters are local days or plural selectors — the Dashboard's shape.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Fact {
    pub emoji: String,
    pub tone: FactTone,
    /// `gda_fact_*`.
    pub key: String,
    pub params: Map<String, Value>,
    /// Params that name a local day (`YYYY-MM-DD`) the client must spell.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub date_params: Vec<String>,
    /// Params whose count selects a singular/plural word, as
    /// `{param: "<one key>|<other key>"}`.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub plural_params: Map<String, Value>,
}

/// Which lists the limits cut short.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct More {
    pub users: bool,
    pub timeline: bool,
    pub projects: bool,
    pub changes: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Activities {
    pub captured: Captured,
    pub totals: Totals,
    pub users: Vec<UserEntry>,
    pub projects: Vec<ProjectEntry>,
    pub changes: Vec<Change>,
    pub timeline: Vec<TimelineEntry>,
    pub anomalies: Vec<Anomaly>,
    pub facts: Vec<Fact>,
    pub more: More,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct FoldReply {
    pub schema: &'static str,
    pub activities: Activities,
}

fn is_false(value: &bool) -> bool {
    !*value
}

// ── the requests ────────────────────────────────────────────────────────

/// The two members every request carries, read first and leniently so the
/// schema and the action can each be refused by name.
#[derive(Deserialize)]
struct Head {
    #[serde(default)]
    schema: String,
    #[serde(default)]
    action: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LedgerRequest {
    #[allow(dead_code)]
    schema: String,
    #[allow(dead_code)]
    action: String,
    ds_project: String,
    captured_at_ms: i64,
    /// The raw `list_transformers_status` rows, special rows included.
    rows: Vec<Value>,
    #[serde(default)]
    now_ms: i64,
}

/// How many entries each list may carry. `0` is refused: a list nobody may
/// read is a request nobody meant.
#[derive(Deserialize, Debug, Clone, Copy)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    #[serde(default = "default_users")]
    pub users: usize,
    #[serde(default = "default_timeline")]
    pub timeline: usize,
    #[serde(default = "default_projects")]
    pub projects: usize,
    #[serde(default = "default_changes")]
    pub changes: usize,
}

fn default_users() -> usize {
    500
}
fn default_timeline() -> usize {
    200
}
fn default_projects() -> usize {
    500
}
fn default_changes() -> usize {
    500
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            users: default_users(),
            timeline: default_timeline(),
            projects: default_projects(),
            changes: default_changes(),
        }
    }
}

/// The counts the host retained from the project's Dashboard `Model`. Only
/// `health_score` reaches the reply — the fold's own counts are the ledger's;
/// the rest are accepted so the host can pass its record unchanged.
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct DashboardSummary {
    #[serde(default)]
    pub health_score: Option<i64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub designed_count: Option<i64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub report_ready_count: Option<i64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub combined_count: Option<i64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub total: Option<i64>,
    #[serde(default)]
    #[allow(dead_code)]
    pub last7: Option<i64>,
}

/// One project as the host holds it: its current ledger, optionally the one
/// retained before it, and its Dashboard counts.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct FoldProject {
    pub ds_project: String,
    #[serde(default)]
    #[allow(dead_code)]
    pub project_name: Option<String>,
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub captured_by: Option<String>,
    pub ledger: Ledger,
    #[serde(default)]
    pub previous: Option<Ledger>,
    #[serde(default)]
    pub dashboard_summary: Option<DashboardSummary>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FoldRequest {
    #[allow(dead_code)]
    schema: String,
    #[allow(dead_code)]
    action: String,
    now_ms: i64,
    /// Minutes to ADD to UTC for the host's local day, as the Dashboard
    /// buckets its momentum.
    #[serde(default)]
    tz_offset_minutes: i64,
    #[serde(default)]
    limits: Limits,
    projects: Vec<FoldProject>,
}

// ── the ledger action ───────────────────────────────────────────────────

fn non_empty(value: String) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn non_zero(value: i64) -> Option<i64> {
    if value == 0 {
        None
    } else {
        Some(value)
    }
}

/// One phase's stamp: `design_status_row::phase_stamp`'s user and time, with
/// the Dashboard's `design_stamp` fallback to `progress.updated_at` when the
/// blob has no `when` of its own.
fn ledger_phase(meta: Option<&Map<String, Value>>) -> Option<PhaseStamp> {
    let meta = meta?;
    let stamp = phase_stamp(Some(meta))?;
    let mut when_ms = stamp.when_ms;
    if when_ms == 0 {
        when_ms = timestamp_ms(
            meta.get("progress")
                .and_then(object)
                .and_then(|progress| progress.get("updated_at")),
        );
    }
    Some(PhaseStamp {
        status: phase_status(Some(meta)).to_string(),
        when_ms: non_zero(when_ms),
        user: non_empty(stamp.user),
    })
}

fn ledger_transformer(value: &Value, truth: Option<&RowReply>) -> LedgerTransformer {
    let empty = Map::new();
    let row = value.as_object().unwrap_or(&empty);
    let name = row_name(value);
    let metadata = row.get("metadata").and_then(object);
    let admin = row.get("admin_bounds").and_then(object);

    let health = design_health::transformer_health(value);
    let output_never_saved = health
        .findings
        .iter()
        .any(|finding| finding.code == design_health::PROCESSED_OUTPUT_MISSING);
    let stamp = design_stamp(row, output_never_saved);
    let designed = stamp.as_ref().map(|stamp| DesignedStamp {
        when_ms: non_zero(stamp.ms),
        user: non_empty(stamp_user(row, Some(stamp))),
    });

    // `latestAction` reports nothing at all when nothing has happened — the
    // Dashboard's own zeroing rule.
    let action = truth
        .map(|truth| ActionFacts {
            label_key: truth.latest_action.label_key.clone(),
            when_ms: truth.latest_action.when_ms,
            user: truth.latest_action.user.clone(),
        })
        .filter(|action| action.when_ms != 0)
        .unwrap_or(ActionFacts {
            label_key: String::new(),
            when_ms: 0,
            user: String::new(),
        });
    let note = attention_note(row, &name, &health, truth, &action, false);
    let tone = match note.as_ref().map(|note| note.tone) {
        Some(Tone::Red) => HealthTone::Red,
        Some(Tone::Amber) => HealthTone::Amber,
        Some(Tone::Blue) => HealthTone::Blue,
        None => HealthTone::Green,
    };

    LedgerTransformer {
        name,
        district: non_empty(string_at(admin, "district")),
        sector: non_empty(string_at(admin, "sector")),
        created_by: non_empty(string_at(metadata, "created_by")),
        created_at_ms: non_zero(timestamp_ms(
            metadata.and_then(|metadata| metadata.get("created_at")),
        )),
        updated_by: non_empty(string_at(metadata, "updated_by")),
        updated_at_ms: non_zero(timestamp_ms(
            metadata.and_then(|metadata| metadata.get("updated_at")),
        )),
        designed,
        phases: LedgerPhases {
            sketch: ledger_phase(phase_meta(row, "sketch_metadata")),
            draft: ledger_phase(phase_meta(row, "draft_metadata")),
            process: ledger_phase(phase_meta(row, "process_metadata")),
            report: ledger_phase(phase_meta(row, "report_metadata")),
            combined: ledger_phase(phase_meta(row, "combined_inclusion_metadata")),
        },
        latest: (action.when_ms != 0).then(|| LatestStamp {
            label_key: action.label_key.clone(),
            when_ms: action.when_ms,
            user: action.user.clone(),
        }),
        health: LedgerHealth {
            tone,
            error_count: health.error_count,
            warning_count: health.warning_count,
            user: note.and_then(|note| non_empty(note.user.trim().to_string())),
        },
    }
}

fn build_ledger(request: LedgerRequest) -> Result<LedgerReply, String> {
    if !valid_project(&request.ds_project) {
        return Err(format!(
            "Design activities refuses invalid project `{}`.",
            request.ds_project
        ));
    }
    if request.rows.len() > MAX_ROWS {
        return Err(format!(
            "Design activities ledger refuses more than {MAX_ROWS} rows."
        ));
    }
    let row_count = request.rows.len();
    // Special rows travel with the list and are excluded here, by the
    // crate's one row-identity table — counted, never recorded.
    let individual: Vec<Value> = request
        .rows
        .into_iter()
        .filter(|row| row_kind(&row_name(row)) == RowKind::Individual)
        .collect();
    // ONE `design_status_row` call over the whole list, as the Dashboard
    // makes it: no rooms, no editor, no lock.
    let mut truth: Vec<Option<RowReply>> = vec![None; individual.len()];
    for reply in design_status_row::truths(&individual, None, "", None, request.now_ms) {
        if let Some(slot) = reply.server_index.and_then(|index| truth.get_mut(index)) {
            *slot = Some(reply);
        }
    }
    let mut transformers: Vec<LedgerTransformer> = individual
        .iter()
        .enumerate()
        .map(|(index, row)| ledger_transformer(row, truth.get(index).and_then(Option::as_ref)))
        .collect();
    transformers.sort_by(|left, right| locale_compare(&left.name, &right.name));
    Ok(LedgerReply {
        schema: SCHEMA,
        ledger: Ledger {
            schema: LEDGER_SCHEMA.to_string(),
            ds_project: request.ds_project,
            captured_at_ms: request.captured_at_ms,
            row_count,
            transformer_count: transformers.len(),
            transformers,
        },
    })
}

// ── reading a ledger the way the Dashboard read the rows ────────────────

fn succeeded(stamp: Option<&PhaseStamp>) -> bool {
    stamp.is_some_and(|stamp| stamp.status == "success")
}

/// How a phase token is spelled, for every surface that shows one.
///
/// One mapping, used by the timeline, by the diff and by `stamps_of`, so a
/// change and the stamp it came from can never be named differently.
pub(crate) fn phase_label_key(phase: &str) -> &'static str {
    match phase {
        "sketch" => "dsr_action_sketch",
        "draft" => "dsr_action_draft",
        "process" => "dsr_action_process",
        "report" => "dsr_action_report",
        "combined" => "dsr_action_combined",
        _ => "dsr_action_document_update",
    }
}

/// The stamps a transformer carries, as `(phase, label_key, when_ms, user)`,
/// in slot order then the document updater — the timeline's raw material.
fn stamps_of(transformer: &LedgerTransformer) -> Vec<(&'static str, &'static str, i64, String)> {
    let phases = &transformer.phases;
    let mut out: Vec<(&'static str, &'static str, i64, String)> = Vec::new();
    for (phase, stamp) in [
        ("sketch", phases.sketch.as_ref()),
        ("draft", phases.draft.as_ref()),
        ("process", phases.process.as_ref()),
        ("report", phases.report.as_ref()),
        ("combined", phases.combined.as_ref()),
    ] {
        let label_key = phase_label_key(phase);
        if let Some(stamp) = stamp {
            if let Some(when_ms) = stamp.when_ms {
                out.push((
                    phase,
                    label_key,
                    when_ms,
                    stamp.user.clone().unwrap_or_default(),
                ));
            }
        }
    }
    if let Some(when_ms) = transformer.updated_at_ms {
        out.push((
            "document",
            phase_label_key("document"),
            when_ms,
            transformer.updated_by.clone().unwrap_or_default(),
        ));
    }
    out
}

/// A phase stamp as the diff compares it: `(when, user, status)`.
type StampKey = (Option<i64>, Option<String>, String);

fn stamp_keys(transformer: &LedgerTransformer) -> Vec<(&'static str, Option<StampKey>)> {
    let phases = &transformer.phases;
    let key = |stamp: Option<&PhaseStamp>| {
        stamp.map(|stamp| (stamp.when_ms, stamp.user.clone(), stamp.status.clone()))
    };
    vec![
        ("sketch", key(phases.sketch.as_ref())),
        ("draft", key(phases.draft.as_ref())),
        ("process", key(phases.process.as_ref())),
        ("report", key(phases.report.as_ref())),
        ("combined", key(phases.combined.as_ref())),
        (
            "document",
            transformer
                .updated_at_ms
                .map(|when_ms| (Some(when_ms), transformer.updated_by.clone(), String::new())),
        ),
    ]
}

/// Every named actor on a ledger: phase operators, document updaters and
/// design owners.
fn actors_of(ledger: &Ledger) -> BTreeSet<String> {
    let mut out: BTreeSet<String> = BTreeSet::new();
    for transformer in &ledger.transformers {
        for (_, _, _, user) in stamps_of(transformer) {
            if !user.is_empty() {
                out.insert(user);
            }
        }
        if let Some(user) = transformer.designed.as_ref().and_then(|d| d.user.clone()) {
            out.insert(user);
        }
    }
    out
}

/// The Dashboard's per-project counts, over the ledger: designed is the
/// recorded design stamp; report-ready and combined are a `success` status
/// on the phase; errors are red rows; contributors are distinct named owners.
struct ProjectCounts {
    designed: usize,
    report_ready: usize,
    combined: usize,
    errors: usize,
    contributors: usize,
}

fn project_counts(ledger: &Ledger) -> ProjectCounts {
    let owners: Vec<String> = ledger
        .transformers
        .iter()
        .filter_map(|t| t.designed.as_ref())
        .map(|d| d.user.clone().unwrap_or_default())
        .collect();
    ProjectCounts {
        designed: owners.len(),
        report_ready: ledger
            .transformers
            .iter()
            .filter(|t| succeeded(t.phases.report.as_ref()))
            .count(),
        combined: ledger
            .transformers
            .iter()
            .filter(|t| succeeded(t.phases.combined.as_ref()))
            .count(),
        errors: ledger
            .transformers
            .iter()
            .filter(|t| t.health.tone == HealthTone::Red)
            .count(),
        contributors: dashboard::distinct_count(&owners),
    }
}

/// The project's latest action: the newest transformer stamp, ties to the
/// first name, as the Dashboard ranks its `recent`.
fn project_latest(ledger: &Ledger) -> Option<LatestStamp> {
    ledger
        .transformers
        .iter()
        .filter_map(|t| t.latest.clone())
        .fold(None, |best: Option<LatestStamp>, latest| match best {
            Some(best) if best.when_ms >= latest.when_ms => Some(best),
            _ => Some(latest),
        })
}

// ── the diff ────────────────────────────────────────────────────────────

fn short_of(user: &str) -> Option<String> {
    non_empty(short_user(user))
}

fn diff_project(ds_project: &str, current: &Ledger, previous: &Ledger) -> Vec<Change> {
    let now: BTreeMap<&str, &LedgerTransformer> = current
        .transformers
        .iter()
        .map(|t| (t.name.as_str(), t))
        .collect();
    let then: BTreeMap<&str, &LedgerTransformer> = previous
        .transformers
        .iter()
        .map(|t| (t.name.as_str(), t))
        .collect();
    let mut changes: Vec<Change> = Vec::new();
    for (name, transformer) in &now {
        let Some(before) = then.get(name) else {
            // A transformer the previous capture did not hold: dated by its
            // own latest action, else by the capture that first saw it.
            let latest = transformer.latest.as_ref();
            changes.push(Change {
                ds_project: ds_project.to_string(),
                name: name.to_string(),
                phase: "document".into(),
                label_key: phase_label_key("document").into(),
                kind: ChangeKind::NewTransformer,
                when_ms: Some(latest.map_or(current.captured_at_ms, |l| l.when_ms)),
                user: latest.and_then(|l| non_empty(l.user.clone())),
                short: latest.and_then(|l| short_of(&l.user)),
                previous_user: None,
                previous_when_ms: None,
            });
            continue;
        };
        for ((phase, held), (_, was)) in stamp_keys(transformer).into_iter().zip(stamp_keys(before))
        {
            match (held, was) {
                (Some(held), None) => changes.push(Change {
                    ds_project: ds_project.to_string(),
                    name: name.to_string(),
                    phase: phase.into(),
                    label_key: phase_label_key(phase).into(),
                    kind: ChangeKind::NewStamp,
                    when_ms: held.0,
                    user: held.1.clone(),
                    short: held.1.as_deref().and_then(short_of),
                    previous_user: None,
                    previous_when_ms: None,
                }),
                (Some(held), Some(was)) if held != was => changes.push(Change {
                    ds_project: ds_project.to_string(),
                    name: name.to_string(),
                    phase: phase.into(),
                    label_key: phase_label_key(phase).into(),
                    kind: ChangeKind::Restamped,
                    when_ms: held.0,
                    user: held.1.clone(),
                    short: held.1.as_deref().and_then(short_of),
                    previous_user: was.1,
                    previous_when_ms: was.0,
                }),
                // A stamp the document no longer carries: restamped to
                // nothing, dated by the capture that first lacked it.
                (None, Some(was)) => changes.push(Change {
                    ds_project: ds_project.to_string(),
                    name: name.to_string(),
                    phase: phase.into(),
                    label_key: phase_label_key(phase).into(),
                    kind: ChangeKind::Restamped,
                    when_ms: Some(current.captured_at_ms),
                    user: None,
                    short: None,
                    previous_user: was.1,
                    previous_when_ms: was.0,
                }),
                _ => {}
            }
        }
    }
    for (name, before) in &then {
        if now.contains_key(name) {
            continue;
        }
        let latest = before.latest.as_ref();
        changes.push(Change {
            ds_project: ds_project.to_string(),
            name: name.to_string(),
            phase: "document".into(),
            label_key: phase_label_key("document").into(),
            kind: ChangeKind::RemovedTransformer,
            // The removal happened before this capture; that is the
            // tightest date the two ledgers give.
            when_ms: Some(current.captured_at_ms),
            user: None,
            short: None,
            previous_user: latest.and_then(|l| non_empty(l.user.clone())),
            previous_when_ms: latest.map(|l| l.when_ms),
        });
    }
    changes
}

// ── the fold ────────────────────────────────────────────────────────────

/// Per-user, per-project accumulation before ranking.
#[derive(Default)]
struct UserProjectAcc {
    designs: usize,
    errors: usize,
    touched: BTreeSet<String>,
    last_seen_ms: Option<i64>,
}

#[derive(Default)]
struct UserAcc {
    projects: BTreeMap<String, UserProjectAcc>,
    /// The earliest stamp and the project it was on.
    first_seen: Option<(i64, String)>,
    /// The newest stamp, its project and its label key.
    last_seen: Option<(i64, String, String)>,
}

fn later(held: Option<i64>, candidate: i64) -> Option<i64> {
    Some(held.map_or(candidate, |held| held.max(candidate)))
}

fn compare_desc_none_last(left: Option<i64>, right: Option<i64>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => right.cmp(&left),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn fact(emoji: &str, tone: FactTone, key: &str, params: Vec<(&str, Value)>) -> Fact {
    Fact {
        emoji: emoji.into(),
        tone,
        key: key.into(),
        params: params
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
        date_params: Vec::new(),
        plural_params: Map::new(),
    }
}

fn check_fold(request: &FoldRequest) -> Result<(), String> {
    for (name, limit) in [
        ("users", request.limits.users),
        ("timeline", request.limits.timeline),
        ("projects", request.limits.projects),
        ("changes", request.limits.changes),
    ] {
        if limit == 0 || limit > MAX_LIMIT {
            return Err(format!(
                "Design activities limits must be between 1 and {MAX_LIMIT}: `{name}` is {limit}."
            ));
        }
    }
    if request.projects.is_empty() {
        return Err("Design activities fold needs at least one project.".into());
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for project in &request.projects {
        if !valid_project(&project.ds_project) {
            return Err(format!(
                "Design activities refuses invalid project `{}`.",
                project.ds_project
            ));
        }
        if !seen.insert(project.ds_project.as_str()) {
            return Err(format!(
                "Design activities fold refuses duplicate project `{}`.",
                project.ds_project
            ));
        }
        for (which, ledger) in [
            ("ledger", Some(&project.ledger)),
            ("previous", project.previous.as_ref()),
        ] {
            let Some(ledger) = ledger else { continue };
            if ledger.schema != LEDGER_SCHEMA {
                return Err(format!(
                    "Design activities {which} for `{}` has unsupported schema `{}`.",
                    project.ds_project, ledger.schema
                ));
            }
            if ledger.ds_project != project.ds_project {
                return Err(format!(
                    "Design activities {which} for `{}` names project `{}`.",
                    project.ds_project, ledger.ds_project
                ));
            }
        }
        if let Some(previous) = &project.previous {
            if previous.captured_at_ms > project.ledger.captured_at_ms {
                return Err(format!(
                    "Design activities previous ledger for `{}` is newer than its current capture.",
                    project.ds_project
                ));
            }
        }
    }
    Ok(())
}

fn build_fold(request: FoldRequest) -> Result<FoldReply, String> {
    check_fold(&request)?;
    let now_ms = request.now_ms;
    let limits = request.limits;

    let mut users: BTreeMap<String, UserAcc> = BTreeMap::new();
    let mut projects: Vec<ProjectEntry> = Vec::new();
    let mut changes: Vec<Change> = Vec::new();
    let mut timeline: BTreeMap<(i64, String, String, String), TimelineEntry> = BTreeMap::new();
    let mut anomalies: Vec<Anomaly> = Vec::new();
    let mut recent_by_project: Vec<(String, usize)> = Vec::new();
    let mut change_totals: Vec<(String, usize)> = Vec::new();
    let mut display_names: BTreeMap<String, String> = BTreeMap::new();
    let mut with_previous = 0usize;
    let mut stale_count = 0usize;

    for project in &request.projects {
        let ds_project = project.ds_project.clone();
        let ledger = &project.ledger;
        if let Some(display) = project.display_name.clone().and_then(non_empty) {
            display_names.insert(ds_project.clone(), display);
        }
        let stale = ledger.captured_at_ms < now_ms - STALE_AFTER_MS;
        if stale {
            stale_count += 1;
        }
        let counts = project_counts(ledger);
        let latest = project_latest(ledger);

        // Stamps of the current AND the previous ledger reach the timeline:
        // a stamp the current capture overwrote is still history.
        let mut recent = 0usize;
        let mut unknown_stamps: (usize, Option<i64>) = (0, None);
        for (which, held) in [(true, Some(ledger)), (false, project.previous.as_ref())] {
            let Some(held) = held else { continue };
            for transformer in &held.transformers {
                for (phase, label_key, when_ms, user) in stamps_of(transformer) {
                    timeline
                        .entry((
                            -when_ms,
                            ds_project.clone(),
                            transformer.name.clone(),
                            phase.to_string(),
                        ))
                        .or_insert_with(|| TimelineEntry {
                            when_ms,
                            short: short_user(&user),
                            user: user.clone(),
                            ds_project: ds_project.clone(),
                            name: transformer.name.clone(),
                            phase: phase.to_string(),
                            label_key: label_key.to_string(),
                        });
                    if which {
                        let age = now_ms - when_ms;
                        if (0..RECENT_WINDOW_MS).contains(&age) {
                            recent += 1;
                        }
                        if user.is_empty() {
                            unknown_stamps.0 += 1;
                            unknown_stamps.1 = later(unknown_stamps.1, when_ms);
                        }
                    }
                    let acc = users.entry(user.clone()).or_default();
                    if acc
                        .first_seen
                        .as_ref()
                        .is_none_or(|(held, _)| when_ms < *held)
                    {
                        acc.first_seen = Some((when_ms, ds_project.clone()));
                    }
                    if acc
                        .last_seen
                        .as_ref()
                        .is_none_or(|(held, _, _)| when_ms > *held)
                    {
                        acc.last_seen = Some((when_ms, ds_project.clone(), label_key.to_string()));
                    }
                    let per = acc.projects.entry(ds_project.clone()).or_default();
                    per.touched.insert(transformer.name.clone());
                    per.last_seen_ms = later(per.last_seen_ms, when_ms);
                }
            }
        }
        // Designs and errors are the CURRENT state, counted as the Dashboard
        // counts its crew and its errors by user.
        for transformer in &ledger.transformers {
            if let Some(designed) = &transformer.designed {
                let user = designed.user.clone().unwrap_or_default();
                let per = users
                    .entry(user)
                    .or_default()
                    .projects
                    .entry(ds_project.clone())
                    .or_default();
                per.designs += 1;
                per.touched.insert(transformer.name.clone());
            }
            if transformer.health.tone == HealthTone::Red {
                let user = transformer.health.user.clone().unwrap_or_default();
                let per = users
                    .entry(user)
                    .or_default()
                    .projects
                    .entry(ds_project.clone())
                    .or_default();
                per.errors += 1;
                per.touched.insert(transformer.name.clone());
            }
        }
        recent_by_project.push((ds_project.clone(), recent));
        if unknown_stamps.0 > 0 {
            anomalies.push(Anomaly {
                kind: AnomalyKind::UnknownActor,
                label_key: "gda_anomaly_unknown_actor".into(),
                user: None,
                short: None,
                ds_project: ds_project.clone(),
                when_ms: unknown_stamps.1,
                count: Some(unknown_stamps.0),
            });
        }
        if stale {
            anomalies.push(Anomaly {
                kind: AnomalyKind::StaleCapture,
                label_key: "gda_anomaly_stale_capture".into(),
                user: None,
                short: None,
                ds_project: ds_project.clone(),
                when_ms: Some(ledger.captured_at_ms),
                count: None,
            });
        }

        let since_previous = project.previous.as_ref().map(|previous| {
            with_previous += 1;
            let before = project_counts(previous);
            let diff = diff_project(&ds_project, ledger, previous);
            let stamps_changed = diff
                .iter()
                .filter(|c| matches!(c.kind, ChangeKind::NewStamp | ChangeKind::Restamped))
                .count();
            let new_transformers = diff
                .iter()
                .filter(|c| c.kind == ChangeKind::NewTransformer)
                .count();
            let removed_transformers = diff
                .iter()
                .filter(|c| c.kind == ChangeKind::RemovedTransformer)
                .count();
            let was: BTreeSet<String> = actors_of(previous);
            let new_actors: Vec<String> = actors_of(ledger)
                .difference(&was)
                .map(|user| short_user(user))
                .collect::<BTreeSet<String>>()
                .into_iter()
                .collect();
            change_totals.push((ds_project.clone(), diff.len()));
            changes.extend(diff);
            SincePrevious {
                captured_at_ms: previous.captured_at_ms,
                designed_delta: counts.designed as i64 - before.designed as i64,
                report_ready_delta: counts.report_ready as i64 - before.report_ready as i64,
                combined_delta: counts.combined as i64 - before.combined as i64,
                error_delta: counts.errors as i64 - before.errors as i64,
                new_transformers,
                removed_transformers,
                stamps_changed,
                new_actors,
            }
        });

        projects.push(ProjectEntry {
            ds_project: ds_project.clone(),
            display_name: display_names.get(&ds_project).cloned(),
            status: project.status.clone().and_then(non_empty),
            captured_at_ms: ledger.captured_at_ms,
            stale,
            transformers: ledger.transformers.len(),
            designed: counts.designed,
            report_ready: counts.report_ready,
            combined: counts.combined,
            errors: counts.errors,
            health_score: project
                .dashboard_summary
                .as_ref()
                .and_then(|summary| summary.health_score),
            contributors: counts.contributors,
            latest,
            since_previous,
        });
    }

    // ── users ───────────────────────────────────────────────────────
    let mut user_entries: Vec<UserEntry> = users
        .into_iter()
        .map(|(name, acc)| {
            let mut per_project: Vec<UserProject> = acc
                .projects
                .iter()
                .map(|(ds_project, per)| UserProject {
                    ds_project: ds_project.clone(),
                    display_name: display_names.get(ds_project).cloned(),
                    designs: per.designs,
                    errors: per.errors,
                    transformers_touched: per.touched.len(),
                    last_seen_ms: per.last_seen_ms,
                })
                .collect();
            per_project.sort_by(|left, right| {
                compare_desc_none_last(left.last_seen_ms, right.last_seen_ms)
                    .then_with(|| left.ds_project.cmp(&right.ds_project))
            });
            UserEntry {
                short: short_user(&name),
                unknown: name.is_empty(),
                project_count: per_project.len(),
                designs: per_project.iter().map(|p| p.designs).sum(),
                errors: per_project.iter().map(|p| p.errors).sum(),
                transformers_touched: per_project.iter().map(|p| p.transformers_touched).sum(),
                first_seen_ms: acc.first_seen.as_ref().map(|(when, _)| *when),
                first_project: acc.first_seen.as_ref().map(|(_, project)| project.clone()),
                last_seen_ms: acc.last_seen.as_ref().map(|(when, _, _)| *when),
                last_project: acc
                    .last_seen
                    .as_ref()
                    .map(|(_, project, _)| project.clone()),
                last_label_key: acc.last_seen.as_ref().map(|(_, _, key)| key.clone()),
                projects: per_project,
                name,
            }
        })
        .collect();
    user_entries.sort_by(|left, right| {
        compare_desc_none_last(left.last_seen_ms, right.last_seen_ms)
            .then_with(|| left.name.is_empty().cmp(&right.name.is_empty()))
            .then_with(|| locale_compare(&left.name, &right.name))
    });
    let actors = user_entries.iter().filter(|user| !user.unknown).count();

    anomalies.sort_by(|left, right| {
        let rank = |kind: AnomalyKind| match kind {
            AnomalyKind::UnknownActor => 0,
            AnomalyKind::StaleCapture => 1,
        };
        rank(left.kind)
            .cmp(&rank(right.kind))
            .then_with(|| left.ds_project.cmp(&right.ds_project))
            .then_with(|| left.user.cmp(&right.user))
    });

    // ── projects, changes, timeline ─────────────────────────────────
    projects.sort_by(|left, right| {
        compare_desc_none_last(
            left.latest.as_ref().map(|l| l.when_ms),
            right.latest.as_ref().map(|l| l.when_ms),
        )
        .then_with(|| left.ds_project.cmp(&right.ds_project))
    });
    changes.sort_by(|left, right| {
        compare_desc_none_last(left.when_ms, right.when_ms)
            .then_with(|| left.ds_project.cmp(&right.ds_project))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.phase.cmp(&right.phase))
    });
    let mut timeline: Vec<TimelineEntry> = timeline.into_values().collect();

    // ── facts ───────────────────────────────────────────────────────
    let totals = Totals {
        transformers: projects.iter().map(|p| p.transformers).sum(),
        designed: projects.iter().map(|p| p.designed).sum(),
        report_ready: projects.iter().map(|p| p.report_ready).sum(),
        combined: projects.iter().map(|p| p.combined).sum(),
        errors: projects.iter().map(|p| p.errors).sum(),
        actors,
        projects_with_activity: projects.iter().filter(|p| p.latest.is_some()).count(),
        changes: changes.len(),
    };
    let mut facts: Vec<Fact> = Vec::new();
    if let Some(busiest) = user_entries
        .iter()
        .filter(|user| !user.unknown && user.designs > 0)
        .max_by(|left, right| {
            left.designs
                .cmp(&right.designs)
                .then_with(|| locale_compare(&right.name, &left.name))
        })
    {
        let mut note = fact(
            "🏆",
            FactTone::Blue,
            "gda_fact_busiest_actor",
            vec![
                ("short", Value::String(busiest.short.clone())),
                ("designs", Value::from(busiest.designs)),
                (
                    "pct",
                    Value::from(dashboard::pct(busiest.designs, totals.designed)),
                ),
                ("projects", Value::from(busiest.project_count)),
            ],
        );
        note.plural_params.insert(
            "projects".into(),
            Value::String("gda_project_one|gda_project_other".into()),
        );
        facts.push(note);
    }
    if let Some((ds_project, count)) = recent_by_project
        .iter()
        .filter(|(_, count)| *count > 0)
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
    {
        let mut note = fact(
            "📈",
            FactTone::Green,
            "gda_fact_most_active_project",
            vec![
                ("ds_project", Value::String(ds_project.clone())),
                (
                    "display_name",
                    Value::String(
                        display_names
                            .get(ds_project)
                            .cloned()
                            .unwrap_or_else(|| ds_project.clone()),
                    ),
                ),
                ("count", Value::from(*count)),
                ("days", Value::from(7)),
            ],
        );
        note.plural_params.insert(
            "count".into(),
            Value::String("gda_stamp_one|gda_stamp_other".into()),
        );
        facts.push(note);
    }
    if let Some(newest) = user_entries
        .iter()
        .filter(|user| !user.unknown)
        .filter(|user| {
            user.first_seen_ms
                .is_some_and(|first| (0..RECENT_WINDOW_MS).contains(&(now_ms - first)))
        })
        .max_by(|left, right| {
            left.first_seen_ms
                .cmp(&right.first_seen_ms)
                .then_with(|| locale_compare(&right.name, &left.name))
        })
    {
        let first = newest.first_seen_ms.unwrap_or(0);
        let project = newest.first_project.clone().unwrap_or_default();
        let mut note = fact(
            "🆕",
            FactTone::Amber,
            "gda_fact_newest_actor",
            vec![
                ("short", Value::String(newest.short.clone())),
                (
                    "day",
                    Value::String(dashboard::day_key(first, request.tz_offset_minutes)),
                ),
                (
                    "display_name",
                    Value::String(display_names.get(&project).cloned().unwrap_or(project)),
                ),
            ],
        );
        note.date_params = vec!["day".into()];
        facts.push(note);
    }
    let untouched = projects
        .iter()
        .filter(|p| {
            p.latest
                .as_ref()
                .is_none_or(|latest| now_ms - latest.when_ms >= UNTOUCHED_AFTER_MS)
        })
        .count();
    if untouched > 0 {
        let mut note = fact(
            "💤",
            FactTone::Amber,
            "gda_fact_untouched_projects",
            vec![("count", Value::from(untouched)), ("days", Value::from(30))],
        );
        note.plural_params.insert(
            "count".into(),
            Value::String("gda_project_one|gda_project_other".into()),
        );
        facts.push(note);
    }
    if let Some((ds_project, count)) = change_totals
        .iter()
        .filter(|(_, count)| *count > 0)
        .max_by(|left, right| left.1.cmp(&right.1).then_with(|| right.0.cmp(&left.0)))
    {
        let mut note = fact(
            "🔀",
            FactTone::Blue,
            "gda_fact_biggest_change",
            vec![
                ("ds_project", Value::String(ds_project.clone())),
                (
                    "display_name",
                    Value::String(
                        display_names
                            .get(ds_project)
                            .cloned()
                            .unwrap_or_else(|| ds_project.clone()),
                    ),
                ),
                ("count", Value::from(*count)),
            ],
        );
        note.plural_params.insert(
            "count".into(),
            Value::String("gda_change_one|gda_change_other".into()),
        );
        facts.push(note);
    }
    facts.truncate(MAX_FACTS);

    // ── the bounds ──────────────────────────────────────────────────
    let more = More {
        users: user_entries.len() > limits.users,
        timeline: timeline.len() > limits.timeline,
        projects: projects.len() > limits.projects,
        changes: changes.len() > limits.changes,
    };
    user_entries.truncate(limits.users);
    timeline.truncate(limits.timeline);
    projects.truncate(limits.projects);
    changes.truncate(limits.changes);

    let captured = Captured {
        project_count: request.projects.len(),
        earliest_ms: request
            .projects
            .iter()
            .map(|p| p.ledger.captured_at_ms)
            .min()
            .unwrap_or(0),
        latest_ms: request
            .projects
            .iter()
            .map(|p| p.ledger.captured_at_ms)
            .max()
            .unwrap_or(0),
        stale_count,
        with_previous,
    };

    Ok(FoldReply {
        schema: SCHEMA,
        activities: Activities {
            captured,
            totals,
            users: user_entries,
            projects,
            changes,
            timeline,
            anomalies,
            facts,
            more,
        },
    })
}

// ── the call ────────────────────────────────────────────────────────────

/// Evaluate one request: `action: "ledger"` compacts a project's rows into
/// its ledger; `action: "fold"` folds retained ledgers across projects.
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > FOLD_INPUT_BYTES {
        return Err("Design activities request exceeds its bound.".into());
    }
    let head: Head = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid design activities request: {error}"))?;
    if head.schema != SCHEMA {
        return Err("Unsupported design activities schema.".into());
    }
    match head.action.as_str() {
        "ledger" => {
            if input.len() > LEDGER_INPUT_BYTES {
                return Err("Design activities request exceeds its bound.".into());
            }
            let request: LedgerRequest = serde_json::from_slice(input)
                .map_err(|error| format!("Invalid design activities request: {error}"))?;
            serde_json::to_string(&build_ledger(request)?).map_err(|error| error.to_string())
        }
        "fold" => {
            let request: FoldRequest = serde_json::from_slice(input)
                .map_err(|error| format!("Invalid design activities request: {error}"))?;
            serde_json::to_string(&build_fold(request)?).map_err(|error| error.to_string())
        }
        other => Err(format!("Unsupported design activities action `{other}`.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn run(request: Value) -> Result<Value, String> {
        evaluate(&serde_json::to_vec(&request).unwrap())
            .map(|reply| serde_json::from_str(&reply).expect("reply parses"))
    }

    fn ledger_of(project: &str, captured_at_ms: i64, rows: Value) -> Value {
        run(json!({
            "schema": SCHEMA, "action": "ledger", "ds_project": project,
            "captured_at_ms": captured_at_ms, "rows": rows, "now_ms": 1_789_732_800_000i64,
        }))
        .expect("ledger")["ledger"]
            .clone()
    }

    fn fold(projects: Value) -> Result<Value, String> {
        run(json!({
            "schema": SCHEMA, "action": "fold", "now_ms": 1_789_732_800_000i64,
            "tz_offset_minutes": 120, "projects": projects,
        }))
    }

    #[test]
    fn the_schema_and_the_action_are_each_refused_by_name() {
        let err = run(json!({"schema": "ds.other/v1", "action": "ledger"})).unwrap_err();
        assert!(
            err.starts_with("Unsupported design activities schema"),
            "{err}"
        );
        let err = run(json!({"schema": SCHEMA, "action": "sweep"})).unwrap_err();
        assert!(
            err.starts_with("Unsupported design activities action"),
            "{err}"
        );
        let err = run(json!({"schema": SCHEMA})).unwrap_err();
        assert!(
            err.starts_with("Unsupported design activities action"),
            "{err}"
        );
        assert!(evaluate(b"not json")
            .unwrap_err()
            .starts_with("Invalid design activities request"));
    }

    #[test]
    fn the_input_is_bounded_per_action() {
        let err = evaluate(&vec![b' '; FOLD_INPUT_BYTES + 1]).unwrap_err();
        assert!(
            err.starts_with("Design activities request exceeds its bound"),
            "{err}"
        );
        let mut padded = format!(
            r#"{{"schema":"{SCHEMA}","action":"ledger","ds_project":"p","captured_at_ms":1,"rows":[],"now_ms":1"#
        )
        .into_bytes();
        padded.extend(std::iter::repeat_n(b' ', LEDGER_INPUT_BYTES));
        padded.push(b'}');
        let err = evaluate(&padded).unwrap_err();
        assert!(
            err.starts_with("Design activities request exceeds its bound"),
            "{err}"
        );
    }

    #[test]
    fn an_unknown_request_member_is_a_contract_break() {
        let err = run(json!({
            "schema": SCHEMA, "action": "ledger", "ds_project": "p", "captured_at_ms": 1,
            "rows": [], "extra": 1,
        }))
        .unwrap_err();
        assert!(
            err.starts_with("Invalid design activities request"),
            "{err}"
        );
        let err =
            fold(json!([{"ds_project": "p", "ledger": ledger_of("p", 1, json!([])), "nope": 1}]))
                .unwrap_err();
        assert!(
            err.starts_with("Invalid design activities request"),
            "{err}"
        );
        let err = run(json!({
            "schema": SCHEMA, "action": "fold", "now_ms": 1, "limits": {"users": 1, "rows": 1},
            "projects": [{"ds_project": "p", "ledger": ledger_of("p", 1, json!([]))}],
        }))
        .unwrap_err();
        assert!(
            err.starts_with("Invalid design activities request"),
            "{err}"
        );
    }

    #[test]
    fn a_ledger_refuses_an_invalid_project_and_too_many_rows() {
        let err = run(json!({
            "schema": SCHEMA, "action": "ledger", "ds_project": "bad name", "captured_at_ms": 1, "rows": [],
        }))
        .unwrap_err();
        assert!(
            err.starts_with("Design activities refuses invalid project"),
            "{err}"
        );
        let rows: Vec<Value> = (0..=MAX_ROWS)
            .map(|i| json!({"name": format!("t{i}")}))
            .collect();
        let err = run(json!({
            "schema": SCHEMA, "action": "ledger", "ds_project": "p", "captured_at_ms": 1, "rows": rows,
        }))
        .unwrap_err();
        assert!(
            err.starts_with("Design activities ledger refuses more than"),
            "{err}"
        );
    }

    #[test]
    fn a_ledger_records_every_stamp_and_excludes_special_rows() {
        let ledger = ledger_of(
            "p",
            1_789_731_000_000,
            json!([
                {"name": "b", "metadata": {"updated_at": "2026-09-17T10:00:00Z", "updated_by": "ops@x.com", "created_by": "maker@x.com", "created_at": 1_758_000_000_000i64},
                 "draft_metadata": {"status": "success", "when": "2026-09-16T09:00:00Z", "user": "draft@x.com"},
                 "process_metadata": {"status": "success", "when": "2026-09-17T09:00:00Z", "user": "proc@x.com", "lane": "fast"},
                 "layers": {"lv_poles": 1, "service_cables": 1, "spans": 1},
                 "admin_bounds": {"district": "Gasabo", "sector": "Gasabo-S1"}},
                {"name": "c", "process_metadata": {"status": "processing", "user": "proc@x.com", "lane": "fast", "progress": {"updated_at": "2026-09-17T09:00:00Z"}}},
                {"name": "a", "report_metadata": {"status": "error", "when": "2026-09-15T09:00:00Z", "user": "erin@x.com", "error": {"message": "workbook refused"}}},
                {"name": "mv_data"},
                {"name": "combined_transformer"},
            ]),
        );
        assert_eq!(ledger["schema"], LEDGER_SCHEMA);
        assert_eq!(ledger["row_count"], 5);
        assert_eq!(ledger["transformer_count"], 3);
        let a = &ledger["transformers"][0];
        let b = &ledger["transformers"][1];
        assert_eq!(a["name"], "a", "sorted by name");
        assert_eq!(a["health"]["tone"], "red");
        assert_eq!(a["health"]["user"], "erin@x.com");
        assert_eq!(a["health"]["error_count"], 1);
        assert!(a.get("designed").is_none());
        assert_eq!(a["latest"]["label_key"], "dsr_action_report");
        assert_eq!(b["district"], "Gasabo");
        assert_eq!(b["created_by"], "maker@x.com");
        assert_eq!(b["created_at_ms"], 1_758_000_000_000i64);
        assert_eq!(b["updated_by"], "ops@x.com");
        assert_eq!(b["phases"]["draft"]["user"], "draft@x.com");
        assert_eq!(b["phases"]["process"]["when_ms"], 1_789_635_600_000i64);
        assert_eq!(
            ledger["transformers"][2]["phases"]["process"]["when_ms"], 1_789_635_600_000i64,
            "progress.updated_at stands in for a missing when"
        );
        assert_eq!(b["designed"]["user"], "proc@x.com");
        assert_eq!(b["health"]["tone"], "blue", "the draft is superseded");
        assert!(b["phases"].get("report").is_none());
    }

    #[test]
    fn a_fold_refuses_what_it_cannot_trust() {
        let ledger = ledger_of("p", 1_789_731_000_000, json!([{"name": "t"}]));
        let err = fold(json!([])).unwrap_err();
        assert!(
            err.starts_with("Design activities fold needs at least one project"),
            "{err}"
        );
        let err = fold(json!([
            {"ds_project": "p", "ledger": ledger},
            {"ds_project": "p", "ledger": ledger},
        ]))
        .unwrap_err();
        assert!(
            err.starts_with("Design activities fold refuses duplicate project"),
            "{err}"
        );
        let err = fold(json!([{"ds_project": "bad name", "ledger": ledger}])).unwrap_err();
        assert!(
            err.starts_with("Design activities refuses invalid project"),
            "{err}"
        );
        let err = fold(json!([{"ds_project": "q", "ledger": ledger}])).unwrap_err();
        assert!(
            err.starts_with("Design activities ledger for `q` names project `p`"),
            "{err}"
        );
        let mut foreign = ledger.clone();
        foreign["schema"] = json!("ds.other/v1");
        let err = fold(json!([{"ds_project": "p", "ledger": foreign}])).unwrap_err();
        assert!(
            err.starts_with("Design activities ledger for `p` has unsupported schema"),
            "{err}"
        );
        let newer = ledger_of("p", 1_789_731_000_001, json!([]));
        let err =
            fold(json!([{"ds_project": "p", "ledger": ledger, "previous": newer}])).unwrap_err();
        assert!(
            err.starts_with("Design activities previous ledger for `p` is newer"),
            "{err}"
        );
        for limits in [json!({"users": 0}), json!({"changes": 5001})] {
            let err = run(json!({
                "schema": SCHEMA, "action": "fold", "now_ms": 1, "limits": limits,
                "projects": [{"ds_project": "p", "ledger": ledger}],
            }))
            .unwrap_err();
            assert!(
                err.starts_with("Design activities limits must be between 1 and 5000"),
                "{err}"
            );
        }
    }

    #[test]
    fn a_fold_diffs_each_project_against_its_previous_capture() {
        let previous = ledger_of(
            "p",
            1_789_100_000_000,
            json!([
                {"name": "kept", "process_metadata": {"status": "success", "when": "2026-09-10T09:00:00Z", "user": "a@x.com", "lane": "fast"},
                 "layers": {"lv_poles": 1, "service_cables": 1, "spans": 1}},
                {"name": "gone", "draft_metadata": {"status": "success", "when": "2026-09-01T09:00:00Z", "user": "a@x.com"}},
            ]),
        );
        let current = ledger_of(
            "p",
            1_789_731_000_000,
            json!([
                {"name": "kept", "process_metadata": {"status": "success", "when": "2026-09-16T09:00:00Z", "user": "b@x.com", "lane": "fast"},
                 "report_metadata": {"status": "success", "when": "2026-09-17T09:00:00Z", "user": "b@x.com"},
                 "layers": {"lv_poles": 1, "service_cables": 1, "spans": 1}},
                {"name": "born", "draft_metadata": {"status": "success", "when": "2026-09-15T09:00:00Z", "user": "c@x.com"}},
            ]),
        );
        let reply = fold(json!([{
            "ds_project": "p", "display_name": "P", "ledger": current, "previous": previous,
        }]))
        .unwrap();
        let activities = &reply["activities"];
        let since = &activities["projects"][0]["since_previous"];
        assert_eq!(since["captured_at_ms"], 1_789_100_000_000i64);
        assert_eq!(since["new_transformers"], 1);
        assert_eq!(since["removed_transformers"], 1);
        assert_eq!(since["stamps_changed"], 2, "one restamp, one new stamp");
        assert_eq!(since["report_ready_delta"], 1);
        assert_eq!(since["new_actors"], json!(["b", "c"]));
        let kinds: Vec<&str> = activities["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["kind"].as_str().unwrap())
            .collect();
        assert_eq!(
            kinds,
            [
                "removed_transformer",
                "new_stamp",
                "restamped",
                "new_transformer"
            ],
            "newest first: the removal is dated by the capture"
        );
        let restamped = &activities["changes"][2];
        assert_eq!(restamped["previous_user"], "a@x.com");
        assert_eq!(restamped["user"], "b@x.com");
        assert_eq!(restamped["short"], "b");
        // The overwritten stamp is still history.
        assert!(activities["timeline"]
            .as_array()
            .unwrap()
            .iter()
            .any(|entry| entry["name"] == "kept" && entry["user"] == "a@x.com"));
        assert_eq!(activities["totals"]["changes"], 4);
    }

    #[test]
    fn a_stale_capture_and_a_tiny_limit_are_both_said_out_loud() {
        let ledger = ledger_of(
            "p",
            1_789_100_000_000,
            json!([
                {"name": "a", "process_metadata": {"status": "success", "when": "2026-09-10T09:00:00Z", "user": "a@x.com", "lane": "fast"}, "layers": {"lv_poles": 1, "service_cables": 1, "spans": 1}},
                {"name": "b", "process_metadata": {"status": "success", "when": "2026-09-11T09:00:00Z", "lane": "fast"}, "layers": {"lv_poles": 1, "service_cables": 1, "spans": 1}},
            ]),
        );
        let reply = run(json!({
            "schema": SCHEMA, "action": "fold", "now_ms": 1_789_732_800_000i64,
            "limits": {"timeline": 1, "users": 1},
            "projects": [{"ds_project": "p", "ledger": ledger}],
        }))
        .unwrap();
        let activities = &reply["activities"];
        assert_eq!(activities["captured"]["stale_count"], 1);
        assert_eq!(activities["projects"][0]["stale"], true);
        assert_eq!(activities["more"]["timeline"], true);
        assert_eq!(activities["more"]["users"], true);
        assert_eq!(activities["more"]["projects"], false);
        assert_eq!(activities["timeline"].as_array().unwrap().len(), 1);
        assert_eq!(activities["users"].as_array().unwrap().len(), 1);
        assert_eq!(
            activities["users"][0]["unknown"], true,
            "the newest stamp names nobody"
        );
        let kinds: Vec<&str> = activities["anomalies"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["kind"].as_str().unwrap())
            .collect();
        assert_eq!(kinds, ["unknown_actor", "stale_capture"]);
    }

    #[test]
    fn a_retained_ledger_is_read_leniently() {
        let mut ledger = ledger_of("p", 1_789_731_000_000, json!([{"name": "t"}]));
        ledger["future_member"] = json!({"anything": true});
        ledger["transformers"][0]["future_member"] = json!(1);
        ledger["transformers"][0]["phases"]["future_phase"] = json!({"status": "success"});
        let reply = fold(json!([{"ds_project": "p", "ledger": ledger}])).unwrap();
        assert_eq!(reply["activities"]["totals"]["transformers"], 1);
    }
}
