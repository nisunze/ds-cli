//! What the whole Dashboard page says about this project — the ONE fold.
//!
//! The Design Dashboard was a pure TypeScript pass (`processing-dashboard.ts`,
//! ~560 lines) that counted the project's transformer status rows into a
//! pipeline, a momentum timeline, a crew, a geography, phase summaries, a
//! governance mix, an attention pile, a health score and a handful of fun
//! facts — and a second pure pass beside it (`collisions-insight.ts`) that
//! explained why transformers collide. Both are this module now, so `ds` can
//! answer the same question with no browser.
//!
//! It DECIDES nothing that is already decided. A row's issue list is
//! `design_health`'s verdict, read in-process; its latest action,
//! governance bucket, sync state and phase ownership are `design_status_row`'s
//! answer over the same rows, read in-process from ONE call. This module
//! counts, buckets and ranks — nothing else. The Dashboard's own `rowIssue`
//! ladder, which was a fourth reading of the status document, is gone: the
//! attention pile is the health finding list, ranked.
//!
//! Pure and cheap by the same contract as `design_health`: every input is
//! already in the `list_transformers_status` response the host holds. No
//! fetch, no clock of its own — `now_ms` and the host's UTC offset arrive in
//! the request so the 7-day split and the day buckets are deterministic.
//!
//! Labels are i18n KEYS. The only English that can leave here is backend text
//! that `design_health` already sanitised. The short date label stays in the
//! rendering client: this module says which day and how many, never how a
//! day is spelled.
//!
//! Ring geometry is decided here too. Every bucket list the page draws as a
//! donut — the districts and the health reading's `ring` — carries
//! each slice's `percent` of the whole and its cumulative `offset` (the
//! percent of everything ranked before it), both `Math.round`ed the way every
//! other percentage in this reply is. The page used to sum unrounded floats
//! in `pieSlices`; a cumulative share rounded once cannot drift, so 40
//! one-count districts still end at 100. A phase-summary bucket names its
//! status as a key too, so the page never spells `stale_processing` itself.
//!
//! The label keys added for that, exactly the set this module can emit:
//! `ddash_ring_error`, `ddash_ring_warning`, `ddash_ring_info`,
//! `ddash_ring_clean`, `ddash_phase_status_success`,
//! `ddash_phase_status_warning`, `ddash_phase_status_processing`,
//! `ddash_phase_status_stale_processing`, `ddash_phase_status_stale`,
//! `ddash_phase_status_error`, `ddash_phase_status_none`.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

use crate::design::{row_kind, RowKind};
use crate::design_health::{
    self, FindingSeverity, Phase, TransformerHealth, PROCESSED_OUTPUT_MISSING,
};
use crate::design_status_row::{self, GovernanceState, PhaseOwner, RowReply, SyncState};
use crate::response_policy::iso8601_ms;

pub const SCHEMA: &str = "ds.design-dashboard/v1";

/// A whole project's status list travels in one call, as it does for health
/// and row truth.
const MAX_INPUT_BYTES: usize = crate::design::MAX_STATUS_REGISTER_BYTES;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;
/// The district chips are generous enough that nothing is truncated for a
/// real project (Rwanda has 30 districts).
const DISTRICT_LIMIT: usize = 40;
const CREW_LIMIT: usize = 10;
const ERRORS_BY_USER_LIMIT: usize = 8;
const RECENT_LIMIT: usize = 8;
const COLLISION_USER_LIMIT: usize = 8;
const COLLISION_TOKEN_LIMIT: usize = 8;
const COLLISION_GROUP_LIMIT: usize = 8;

/// The phase statuses the summaries report, in the order they are reported.
const PHASE_STATUSES: [&str; 7] = [
    "success",
    "warning",
    "processing",
    "stale_processing",
    "stale",
    "error",
    "none",
];

// ── the vocabulary ──────────────────────────────────────────────────────

/// Declaration order IS `ATTENTION_RANK`: red first, then amber, then blue.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Red,
    Amber,
    Blue,
}

/// The sticky-note ink of a fun fact. Rendering, but the fact and its colour
/// are one editorial choice, so they travel together.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FactTone {
    Amber,
    Teal,
    Violet,
    Rose,
    Sky,
    Lime,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Trend {
    Up,
    Down,
    Flat,
}

/// Governance as the Dashboard buckets it: ds-brain's four states folded to
/// the two the mix shows, exactly as `governanceFilterValue` folded them.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GovernanceBucket {
    Locked,
    Draft,
}

// ── the reply ───────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct MomentumPoint {
    /// The local day, `YYYY-MM-DD`, at the host's offset. The client spells it.
    pub key: String,
    pub count: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Momentum {
    pub points: Vec<MomentumPoint>,
    /// Total dated designs represented by the points — the honest denominator.
    pub total: usize,
    pub max: usize,
    pub last7: usize,
    pub prev7: usize,
    pub trend: Trend,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub busiest: Option<MomentumPoint>,
    pub active_days: usize,
    pub avg_per_active_day: Number,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PipelineStage {
    pub key: String,
    pub label_key: String,
    pub count: usize,
    /// Share of the whole design set that has reached this milestone (0–100).
    /// NOT a funnel conversion — draft/process/report are independent phases
    /// over the same record, so a later milestone can legitimately out-count
    /// an earlier one. Each stage is measured against the total.
    pub pct_of_total: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CrewMember {
    /// The account, or `""` when no run on this row named one.
    pub name: String,
    pub short: String,
    pub count: usize,
    /// No account could be attributed; the client names the bucket.
    #[serde(default, skip_serializing_if = "is_false")]
    pub unknown: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DistrictTag {
    /// The district as the row's admin bounds name it, or `""` for the
    /// leftovers bucket, which the client names.
    pub label: String,
    pub count: usize,
    /// 0–1 weight vs the busiest district — drives chip size + ink.
    pub weight: f64,
    /// Share of the district donut, 0–100, over the chips actually reported.
    pub percent: i64,
    /// Cumulative percent of the chips ranked before this one — the SVG ring's
    /// dash offset.
    pub offset: i64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub unassigned: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PhaseBucket {
    pub status: String,
    /// `ddash_phase_status_<status>` — how the status is spelled is the
    /// catalogue's, not the page's.
    pub label_key: String,
    pub count: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct PhaseSummary {
    pub key: String,
    pub label_key: String,
    pub total: usize,
    pub buckets: Vec<PhaseBucket>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct GovernanceMix {
    pub status: GovernanceBucket,
    pub label_key: String,
    pub count: usize,
}

/// One row's single most important issue. The label is the health finding's
/// own key; `phase` is the parameter the note's sentence takes.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct AttentionNote {
    pub name: String,
    pub tone: Tone,
    pub label_key: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase: Option<Phase>,
    /// The backend's own sentence, already sanitised by `design_health`.
    pub message: String,
    /// What the note says when the backend said nothing.
    pub detail_key: String,
    pub when_ms: i64,
    pub user: String,
}

/// One slice of the health ring: a severity band of the transformer set.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RingSlice {
    /// `error`, `warning`, `info` or `clean`, in that order.
    pub key: String,
    /// `ddash_ring_<key>`.
    pub label_key: String,
    pub count: usize,
    /// Share of the whole transformer set, 0–100.
    pub percent: i64,
    /// Cumulative percent of the bands ranked before this one.
    pub offset: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct HealthReading {
    pub score: i64,
    pub label_key: String,
    pub error_count: usize,
    pub warning_count: usize,
    pub info_count: usize,
    pub clean_count: usize,
    /// The four counts as donut slices, red to green, empty bands dropped.
    /// Omitted when there are no transformers to draw.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ring: Vec<RingSlice>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct ActionFacts {
    /// `""` when nothing has ever happened to this row.
    pub label_key: String,
    pub when_ms: i64,
    pub user: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct RecentAction {
    pub name: String,
    pub action: ActionFacts,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct FunFact {
    pub emoji: String,
    pub tone: FactTone,
    pub key: String,
    /// Interpolation values for the key's sentence.
    pub params: Map<String, Value>,
    /// Params that name a local day (`YYYY-MM-DD`) the client must spell.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub date_params: Vec<String>,
    /// Params whose count selects a singular/plural word, as
    /// `{param: "<one key>|<other key>"}`.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub plural_params: Map<String, Value>,
    /// Params whose value is itself an i18n key — the unattributed crew
    /// bucket is the only one, and naming it is the client's job.
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub key_params: Map<String, Value>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Crew {
    pub members: Vec<CrewMember>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader: Option<CrewMember>,
    pub contributors: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Model {
    pub total: usize,
    /// Transformers with a computed LV network.
    pub designed_count: usize,
    pub report_ready_count: usize,
    pub combined_count: usize,
    pub latest: ActionFacts,
    pub pipeline: Vec<PipelineStage>,
    pub momentum: Momentum,
    pub crew: Crew,
    /// Error attention notes grouped by the user who ran the failed phase.
    pub errors_by_user: Vec<CrewMember>,
    pub districts: Vec<DistrictTag>,
    pub district_count: usize,
    pub sector_count: usize,
    pub phase_summaries: Vec<PhaseSummary>,
    pub governance: Vec<GovernanceMix>,
    pub attention: Vec<AttentionNote>,
    pub health: HealthReading,
    pub recent: Vec<RecentAction>,
    pub facts: Vec<FunFact>,
}

// ── the collisions insight ──────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CollisionUserBucket {
    pub name: String,
    pub short: String,
    pub count: usize,
    #[serde(default, skip_serializing_if = "is_false")]
    pub unknown: bool,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct NameTokenBucket {
    pub token: String,
    /// Number of DISTINCT colliding transformers whose name contains it.
    pub count: usize,
    /// Share of the colliding set carrying it, 0–100.
    pub pct: i64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct DuplicateBaseGroup {
    pub base: String,
    pub names: Vec<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct CollisionsInsight {
    pub collision_count: usize,
    pub colliding_transformers: Vec<String>,
    pub colliding_count: usize,
    pub by_user: Vec<CollisionUserBucket>,
    /// Name-parts shared by ≥2 colliding transformers, most common first.
    pub common_tokens: Vec<NameTokenBucket>,
    /// Colliding names identical but for a trailing version digit.
    pub duplicate_bases: Vec<DuplicateBaseGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example_pair: Option<[String; 2]>,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Reply {
    pub schema: &'static str,
    pub dashboard: Model,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub collisions: Option<CollisionsInsight>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

// ── the request ─────────────────────────────────────────────────────────

/// One live process diagnostic the host holds for THIS project: a run that
/// finished badly and whose verdict is not in the document yet.
#[derive(Deserialize, Debug, Clone)]
#[serde(deny_unknown_fields)]
pub struct ProcessDiagnostic {
    pub transformer_name: String,
    /// `error` or anything else, which is a warning.
    #[serde(default)]
    pub severity: String,
    #[serde(default)]
    pub message: String,
    /// Epoch millis, as the host's store records it.
    #[serde(default)]
    pub at: i64,
    /// Accepted so the host can pass its own record unchanged; the stored
    /// stamp is wire data no reading here branches on.
    #[serde(default)]
    #[allow(dead_code)]
    pub lane: Option<String>,
    #[serde(default)]
    pub user: Option<String>,
    /// Accepted so the host can pass its own record unchanged; the host has
    /// already scoped the list to one project.
    #[serde(default)]
    #[allow(dead_code)]
    pub project_id: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub source: Option<String>,
}

#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct CollisionsRequest {
    /// The reporter-computed collision features, as the mirror holds them.
    #[serde(default)]
    pub features: Vec<Value>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    /// The project's status rows. Special rows travel with them and are
    /// excluded here, by the crate's one row-identity table.
    rows: Vec<Value>,
    #[serde(default)]
    diagnostics: Vec<ProcessDiagnostic>,
    #[serde(default)]
    now_ms: i64,
    /// Minutes to ADD to UTC for the host's local day — the browser's
    /// `-getTimezoneOffset()`. The momentum timeline buckets by local day, as
    /// the page always has.
    #[serde(default)]
    tz_offset_minutes: i64,
    #[serde(default)]
    collisions: Option<CollisionsRequest>,
}

// ── reading JSON the way the page read it ───────────────────────────────

pub(crate) fn object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

pub(crate) fn string_at(map: Option<&Map<String, Value>>, key: &str) -> String {
    map.and_then(|map| map.get(key))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

pub(crate) fn row_name(row: &Value) -> String {
    object(row)
        .and_then(|row| row.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

/// `canonical` (processing-dashboard.ts): trim and lowercase. Not the row
/// identity table's canonicalisation — the diagnostics map was keyed this way.
fn canonical(value: &str) -> String {
    value.trim().to_lowercase()
}

/// `timestampMs`: a finite number is epoch millis, a string is whatever the
/// crate's one ISO reader makes of it, everything else is "no timestamp".
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

/// The first of the keys the phase blob actually carries, `??`-style: a key
/// that is present but empty still wins, exactly as the page read it.
pub(crate) fn first_present<'meta>(
    meta: &'meta Map<String, Value>,
    keys: &[&str],
) -> Option<&'meta Value> {
    for key in keys {
        match meta.get(*key) {
            Some(Value::Null) | None => continue,
            Some(value) => return Some(value),
        }
    }
    None
}

pub(crate) fn phase_meta<'row>(
    row: &'row Map<String, Value>,
    key: &str,
) -> Option<&'row Map<String, Value>> {
    row.get(key).and_then(object)
}

/// `phaseFilterValue`: the status the document wrote, or `none`.
pub(crate) fn phase_status(meta: Option<&Map<String, Value>>) -> &str {
    meta.and_then(|meta| meta.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("none")
}

pub(crate) fn phase_succeeded(meta: Option<&Map<String, Value>>) -> bool {
    phase_status(meta) == "success"
}

/// `latestLegacyPhase`: draft and sketch write the same output, so the more
/// recent of the two owns the merged cell.
pub(crate) fn latest_legacy_phase(row: &Map<String, Value>) -> Option<&Map<String, Value>> {
    let draft = phase_meta(row, "draft_metadata");
    let sketch = phase_meta(row, "sketch_metadata");
    match (draft, sketch) {
        (None, None) => None,
        (None, sketch) => sketch,
        (draft, None) => draft,
        (Some(draft), Some(sketch)) => {
            let sketch_ms = timestamp_ms(first_present(sketch, &["sketched_at", "when"]));
            let draft_ms = timestamp_ms(first_present(draft, &["drafted_at", "when"]));
            if sketch_ms >= draft_ms {
                Some(sketch)
            } else {
                Some(draft)
            }
        }
    }
}

/// `shortUser`: the local part of an address, or the address itself.
pub(crate) fn short_user(account: &str) -> String {
    match account.split_once('@') {
        Some((local, _)) if !local.is_empty() => local.to_string(),
        _ => account.to_string(),
    }
}

/// `Math.round`, which rounds a half UP. Every value here is non-negative, so
/// this is `f64::round` with the tie direction said out loud.
pub(crate) fn js_round(value: f64) -> i64 {
    (value + 0.5).floor() as i64
}

pub(crate) fn pct(part: usize, whole: usize) -> i64 {
    if whole == 0 {
        0
    } else {
        js_round((part as f64 / whole as f64) * 100.0)
    }
}

/// `pieSlices`, with the module's rounding: each bucket's share of the
/// whole and the share of everything before it, as `(percent, offset)` in
/// bucket order. The whole is the sum of the buckets handed in, exactly as
/// the page summed what it fed the ring.
fn ring_geometry(counts: &[usize]) -> Vec<(i64, i64)> {
    let whole: usize = counts.iter().sum();
    let mut before = 0usize;
    counts
        .iter()
        .map(|count| {
            let slice = (pct(*count, whole), pct(before, whole));
            before += count;
            slice
        })
        .collect()
}

/// The ASCII subset of the CLDR root order `localeCompare` applies to the
/// names, districts and accounts these buckets hold: the connector `_` before
/// other punctuation, punctuation before digits, digits before letters, case
/// only a tiebreak. The page's every bucket tie broke this way.
fn collate_key(text: &str) -> Vec<(u8, u32)> {
    text.chars()
        .map(|character| {
            if character.is_whitespace() {
                (0, character as u32)
            } else if character == '_' {
                (1, 0)
            } else if character.is_ascii_digit() {
                (3, character as u32)
            } else if character.is_alphabetic() {
                (
                    4,
                    character.to_lowercase().next().unwrap_or(character) as u32,
                )
            } else {
                (2, character as u32)
            }
        })
        .collect()
}

/// The tertiary weight: lowercase sorts before uppercase, as the root
/// collation orders a pair that is otherwise the same word.
fn case_key(text: &str) -> Vec<u8> {
    text.chars().map(|c| u8::from(c.is_uppercase())).collect()
}

pub(crate) fn locale_compare(left: &str, right: &str) -> Ordering {
    collate_key(left)
        .cmp(&collate_key(right))
        .then_with(|| case_key(left).cmp(&case_key(right)))
        .then_with(|| left.cmp(right))
}

/// Civil date at an offset, `YYYY-MM-DD` — the day key the timeline buckets
/// by. Howard Hinnant's `civil_from_days`, which is what `new Date(ms)`
/// reports for a fixed offset.
pub(crate) fn day_key(ms: i64, tz_offset_minutes: i64) -> String {
    let local = ms + tz_offset_minutes * 60_000;
    let days = local.div_euclid(DAY_MS);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    format!("{year:04}-{month:02}-{day:02}")
}

/// `new Date(ms).toISOString()` — what the page wrote into the phase blob it
/// synthesised for a live diagnostic.
fn iso_from_ms(ms: i64) -> String {
    let day = day_key(ms, 0);
    let within = ms.rem_euclid(DAY_MS);
    let millis = within % 1_000;
    let seconds = (within / 1_000) % 60;
    let minutes = (within / 60_000) % 60;
    let hours = within / 3_600_000;
    format!("{day}T{hours:02}:{minutes:02}:{seconds:02}.{millis:03}Z")
}

// ── the diagnostics fold ────────────────────────────────────────────────

/// The host's live process diagnostics, folded into the rows as the phase
/// blobs they will become. A row that already reports an error or a warning
/// keeps its own; a diagnostic naming a transformer the list does not hold
/// becomes a row of its own, exactly as the page synthesised one.
fn rows_with_diagnostics(rows: Vec<Value>, diagnostics: &[ProcessDiagnostic]) -> Vec<Value> {
    if diagnostics.is_empty() {
        return rows;
    }
    let mut order: Vec<String> = Vec::with_capacity(rows.len());
    let mut by_name: BTreeMap<String, Value> = BTreeMap::new();
    for row in rows {
        let key = canonical(&row_name(&row));
        if !by_name.contains_key(&key) {
            order.push(key.clone());
        }
        by_name.insert(key, row);
    }
    for diagnostic in diagnostics {
        let name = canonical(&diagnostic.transformer_name);
        if name.is_empty() {
            continue;
        }
        let existing = by_name.get(&name);
        let status = existing
            .and_then(object)
            .and_then(|row| row.get("process_metadata"))
            .and_then(object)
            .and_then(|meta| meta.get("status"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if status == "error" || status == "warning" {
            continue;
        }
        let error = diagnostic.severity == "error";
        let mut meta = existing
            .and_then(object)
            .and_then(|row| row.get("process_metadata"))
            .and_then(object)
            .cloned()
            .unwrap_or_default();
        meta.insert(
            "status".into(),
            Value::String(if error { "error" } else { "warning" }.into()),
        );
        meta.insert("when".into(), Value::String(iso_from_ms(diagnostic.at)));
        // The spread wrote this key whether or not the diagnostic named it,
        // so a user the document carried is overwritten, not kept.
        meta.insert(
            "user".into(),
            diagnostic
                .user
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        if error {
            meta.insert(
                "error_message".into(),
                Value::String(diagnostic.message.clone()),
            );
            let mut detail = Map::new();
            detail.insert("message".into(), Value::String(diagnostic.message.clone()));
            meta.insert("error".into(), Value::Object(detail));
        } else {
            let mut warning = Map::new();
            warning.insert(
                "code".into(),
                Value::String("live_process_diagnostic".into()),
            );
            warning.insert("message".into(), Value::String(diagnostic.message.clone()));
            meta.insert("warning".into(), Value::Object(warning));
        }
        match by_name.get_mut(&name) {
            Some(row) => {
                if let Some(row) = row.as_object_mut() {
                    row.insert("process_metadata".into(), Value::Object(meta));
                }
            }
            None => {
                let mut synthetic = Map::new();
                synthetic.insert(
                    "name".into(),
                    Value::String(diagnostic.transformer_name.clone()),
                );
                synthetic.insert("kind".into(), Value::String("individual".into()));
                synthetic.insert("process_metadata".into(), Value::Object(meta));
                synthetic.insert("client_local".into(), Value::Bool(true));
                synthetic.insert("client_dirty".into(), Value::Bool(false));
                synthetic.insert("client_server_known".into(), Value::Bool(true));
                order.push(name.clone());
                by_name.insert(name, Value::Object(synthetic));
            }
        }
    }
    order
        .into_iter()
        .filter_map(|key| by_name.remove(&key))
        .collect()
}

// ── what the other two modules already decided ──────────────────────────

#[derive(Deserialize)]
struct TruthReply {
    rows: Vec<RowReply>,
}

/// ONE `design_status_row` call over the whole list. The Dashboard never asks
/// a row about its lane, its latest action, its governance or its sync state
/// twice, and never answers any of them itself.
fn row_truth(rows: &[Value], now_ms: i64) -> Vec<Option<RowReply>> {
    let mut truth: Vec<Option<RowReply>> = vec![None; rows.len()];
    let request = serde_json::json!({
        "schema": design_status_row::SCHEMA,
        "server_rows": rows,
        "local_room_headers": [],
        "current_user_email": "",
        "now_ms": now_ms,
    });
    let Ok(input) = serde_json::to_vec(&request) else {
        return truth;
    };
    let Ok(reply) = design_status_row::evaluate(&input) else {
        return truth;
    };
    let Ok(reply) = serde_json::from_str::<TruthReply>(&reply) else {
        return truth;
    };
    for row in reply.rows {
        if let Some(slot) = row.server_index.and_then(|index| truth.get_mut(index)) {
            *slot = Some(row);
        }
    }
    truth
}

/// The most recent successful design compute on a row, or none when the LV
/// network has never been computed. A transformer becomes "designed" through
/// ANY compute phase — sketch or draft (the retired phases, still stamped on
/// older documents) or process. A green process stamp whose saved output layers are
/// missing does NOT count: that is `design_health`'s
/// `processed_output_missing`, and it lands in the attention pile instead.
pub(crate) struct ComputeStamp {
    pub(crate) ms: i64,
    pub(crate) user: String,
}

pub(crate) fn design_stamp(
    row: &Map<String, Value>,
    output_never_saved: bool,
) -> Option<ComputeStamp> {
    let mut candidates: Vec<ComputeStamp> = Vec::new();
    if let Some(legacy) = latest_legacy_phase(row) {
        if phase_succeeded(Some(legacy)) {
            candidates.push(ComputeStamp {
                ms: timestamp_ms(first_present(
                    legacy,
                    &["sketched_at", "drafted_at", "when"],
                )),
                user: string_at(Some(legacy), "user"),
            });
        }
    }
    let process = phase_meta(row, "process_metadata");
    if phase_succeeded(process) && !output_never_saved {
        let meta = process.expect("a succeeded phase has a blob");
        let mut ms = timestamp_ms(meta.get("when"));
        if ms == 0 {
            ms = timestamp_ms(
                meta.get("progress")
                    .and_then(object)
                    .and_then(|progress| progress.get("updated_at")),
            );
        }
        candidates.push(ComputeStamp {
            ms,
            user: string_at(Some(meta), "user"),
        });
    }
    candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.ms));
    candidates.into_iter().next()
}

/// Who "owns" a transformer — the user who last designed it, then the last
/// document updater. `""` when neither named one.
pub(crate) fn stamp_user(row: &Map<String, Value>, stamp: Option<&ComputeStamp>) -> String {
    let designed = stamp.map(|stamp| stamp.user.clone()).unwrap_or_default();
    if !designed.is_empty() {
        return designed;
    }
    string_at(row.get("metadata").and_then(object), "updated_by")
}

// ── counted buckets ─────────────────────────────────────────────────────

/// `countedBuckets`: most frequent first, ties by label. The unattributed
/// bucket travels as an empty label and sorts LAST within a count — the page
/// sorted it by the English word it printed, which is not a fact a kernel may
/// hold.
fn counted_buckets(values: &[String], limit: usize) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    for value in values {
        *counts.entry(value.clone()).or_insert(0) += 1;
    }
    let mut buckets: Vec<(String, usize)> = counts.into_iter().collect();
    buckets.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| left.0.is_empty().cmp(&right.0.is_empty()))
            .then_with(|| locale_compare(&left.0, &right.0))
    });
    buckets.truncate(limit);
    buckets
}

pub(crate) fn distinct_count(values: &[String]) -> usize {
    values
        .iter()
        .filter(|value| !value.trim().is_empty())
        .map(|value| value.trim().to_string())
        .collect::<std::collections::BTreeSet<String>>()
        .len()
}

fn crew_member(name: &str, count: usize) -> CrewMember {
    if name.is_empty() {
        return CrewMember {
            name: String::new(),
            short: String::new(),
            count,
            unknown: true,
        };
    }
    CrewMember {
        name: name.to_string(),
        short: short_user(name),
        count,
        unknown: false,
    }
}

// ── the attention pile ──────────────────────────────────────────────────

fn phase_slot(phase: Phase) -> &'static str {
    match phase {
        Phase::Sketch => "sketch_metadata",
        Phase::Draft => "draft_metadata",
        Phase::Process => "process_metadata",
        Phase::Report => "report_metadata",
        Phase::Combined => "combined_inclusion_metadata",
    }
}

/// The Draft/Sketch phases of the retired second process path. Older
/// documents still carry their stamps; no reading here treats one as a
/// finding a person can act on today.
pub(crate) fn is_retired_phase(phase: Phase) -> bool {
    matches!(phase, Phase::Draft | Phase::Sketch)
}

/// The note a health finding becomes. The label is the finding's own key
/// wrapped in the sentence the pile speaks; the detail is the backend's own
/// message, or the key that stands in when it said nothing.
fn note_from_finding(
    row: &Map<String, Value>,
    name: &str,
    finding: &design_health::Finding,
    fallback_user: &str,
    when_ms: i64,
) -> AttentionNote {
    let (label_key, detail_key, tone) = if finding.code == PROCESSED_OUTPUT_MISSING {
        (
            "ddash_note_output_missing",
            "ddash_note_detail_output_missing",
            Tone::Red,
        )
    } else {
        match finding.severity {
            FindingSeverity::Error => (
                "ddash_note_phase_error",
                "ddash_note_detail_phase_error",
                Tone::Red,
            ),
            FindingSeverity::Warning if finding.code == "stuck_processing" => (
                "ddash_note_phase_stuck",
                "ddash_note_detail_processing",
                Tone::Amber,
            ),
            FindingSeverity::Warning => (
                "ddash_note_phase_warning",
                "ddash_note_detail_warning",
                Tone::Amber,
            ),
            FindingSeverity::Processing => (
                "ddash_note_phase_processing",
                "ddash_note_detail_processing",
                Tone::Amber,
            ),
            FindingSeverity::Stale => (
                "ddash_note_phase_stale",
                "ddash_note_detail_processing",
                Tone::Amber,
            ),
        }
    };
    // A failing phase is attributed to its own operator, not merely to the
    // latest toucher — that is who "errors by user" must point at. A warning
    // or a run in flight stays with the row's latest action, as it always has.
    let phase_user = if tone == Tone::Red {
        string_at(phase_meta(row, phase_slot(finding.phase)), "user")
    } else {
        String::new()
    };
    AttentionNote {
        name: name.to_string(),
        tone,
        label_key: label_key.into(),
        phase: Some(finding.phase),
        message: finding.message.clone(),
        detail_key: detail_key.into(),
        when_ms,
        user: if phase_user.is_empty() {
            fallback_user.to_string()
        } else {
            phase_user
        },
    }
}

/// The score band. The page's four thresholds, as keys.
fn health_label_key(score: i64) -> &'static str {
    if score >= 90 {
        "ddash_health_thriving"
    } else if score >= 70 {
        "ddash_health_healthy"
    } else if score >= 40 {
        "ddash_health_needs_care"
    } else {
        "ddash_health_critical"
    }
}

fn number(value: f64) -> Number {
    Number::from_f64(value).unwrap_or_else(|| Number::from(0))
}

fn fact(emoji: &str, tone: FactTone, key: &str, params: Vec<(&str, Value)>) -> FunFact {
    FunFact {
        emoji: emoji.into(),
        tone,
        key: key.into(),
        params: params
            .into_iter()
            .map(|(name, value)| (name.to_string(), value))
            .collect(),
        date_params: Vec::new(),
        plural_params: Map::new(),
        key_params: Map::new(),
    }
}

/// The fun facts, in the order the page pinned them up.
fn build_facts(
    momentum: &Momentum,
    leader: Option<&CrewMember>,
    district_count: usize,
    sector_count: usize,
    locked_pct: i64,
) -> Vec<FunFact> {
    let mut facts: Vec<FunFact> = Vec::new();
    if let Some(busiest) = &momentum.busiest {
        let mut note = fact(
            "⭐",
            FactTone::Amber,
            "ddash_fact_busiest_day",
            vec![
                ("label", Value::String(busiest.key.clone())),
                ("count", Value::from(busiest.count)),
            ],
        );
        note.date_params = vec!["label".into()];
        facts.push(note);
    }
    if let Some(leader) = leader.filter(|leader| leader.count > 0) {
        let mut note = fact(
            "🏆",
            FactTone::Violet,
            "ddash_fact_leader",
            vec![
                ("short", Value::String(leader.short.clone())),
                ("count", Value::from(leader.count)),
            ],
        );
        if leader.unknown {
            note.key_params
                .insert("short".into(), Value::String("ddash_owner_unknown".into()));
        }
        facts.push(note);
    }
    if district_count > 0 {
        let mut note = fact(
            "🗺️",
            FactTone::Teal,
            "ddash_fact_geography",
            vec![
                ("districts", Value::from(district_count)),
                ("sectors", Value::from(sector_count)),
            ],
        );
        note.plural_params.insert(
            "districts".into(),
            Value::String("ddash_district_one|ddash_district_other".into()),
        );
        note.plural_params.insert(
            "sectors".into(),
            Value::String("ddash_sector_one|ddash_sector_other".into()),
        );
        facts.push(note);
    }
    if momentum.total > 0 && momentum.last7 > 0 {
        let share = pct(momentum.last7, momentum.total).min(100);
        facts.push(fact(
            "📈",
            FactTone::Sky,
            "ddash_fact_last7_share",
            vec![("pct", Value::from(share))],
        ));
    }
    if locked_pct > 0 {
        facts.push(fact(
            "🔒",
            FactTone::Rose,
            "ddash_fact_locked_share",
            vec![("pct", Value::from(locked_pct))],
        ));
    }
    if momentum.active_days > 1 {
        facts.push(fact(
            "🔥",
            FactTone::Amber,
            "ddash_fact_active_days",
            vec![
                ("days", Value::from(momentum.active_days)),
                ("avg", Value::Number(momentum.avg_per_active_day.clone())),
            ],
        ));
    }
    facts
}

// ── the fold ────────────────────────────────────────────────────────────

fn build_model(request: &Request) -> (Model, BTreeMap<String, String>) {
    // Special rows travel with the list and are excluded here, by the crate's
    // one row-identity table — the page filtered them with the same rule.
    let individual: Vec<Value> = request
        .rows
        .iter()
        .filter(|row| row_kind(&row_name(row)) == RowKind::Individual)
        .cloned()
        .collect();
    let rows = rows_with_diagnostics(individual, &request.diagnostics);
    let total = rows.len();
    let truth = row_truth(&rows, request.now_ms);
    let empty = Map::new();

    let mut designed_users: Vec<String> = Vec::new();
    let mut designed_count = 0usize;
    let mut report_ready_count = 0usize;
    let mut combined_count = 0usize;
    let mut day_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut dated_total = 0usize;
    let mut last7 = 0usize;
    let mut prev7 = 0usize;
    let mut districts: Vec<String> = Vec::new();
    let mut district_names: Vec<String> = Vec::new();
    let mut sector_names: Vec<String> = Vec::new();
    let mut gov_locked = 0usize;
    let mut gov_draft = 0usize;
    let mut notes: Vec<AttentionNote> = Vec::new();
    let mut actions: Vec<(usize, String, ActionFacts)> = Vec::new();
    let mut phase_counts: Vec<BTreeMap<String, usize>> = vec![BTreeMap::new(); 3];
    let mut owner_by_name: BTreeMap<String, String> = BTreeMap::new();

    for (index, value) in rows.iter().enumerate() {
        let row = value.as_object().unwrap_or(&empty);
        let name = row_name(value);
        let health = design_health::transformer_health(value);
        let output_never_saved = health
            .findings
            .iter()
            .any(|finding| finding.code == PROCESSED_OUTPUT_MISSING);
        let stamp = design_stamp(row, output_never_saved);
        let owner = stamp_user(row, stamp.as_ref());
        owner_by_name.insert(canonical(&name), owner.clone());

        if let Some(stamp) = &stamp {
            designed_count += 1;
            designed_users.push(owner.clone());
            if stamp.ms > 0 {
                dated_total += 1;
                *day_counts
                    .entry(day_key(stamp.ms, request.tz_offset_minutes))
                    .or_insert(0) += 1;
                let age = request.now_ms - stamp.ms;
                if (0..7 * DAY_MS).contains(&age) {
                    last7 += 1;
                } else if (7 * DAY_MS..14 * DAY_MS).contains(&age) {
                    prev7 += 1;
                }
            }
        }
        let process = phase_meta(row, "process_metadata");
        if phase_succeeded(phase_meta(row, "report_metadata")) {
            report_ready_count += 1;
        }
        if phase_succeeded(phase_meta(row, "combined_inclusion_metadata")) {
            combined_count += 1;
        }

        let district = string_at(row.get("admin_bounds").and_then(object), "district");
        districts.push(district.clone());
        district_names.push(district);
        sector_names.push(string_at(
            row.get("admin_bounds").and_then(object),
            "sector",
        ));

        let truth = truth.get(index).and_then(Option::as_ref);
        match truth.map(|truth| truth.governance.state) {
            Some(GovernanceState::Open) => gov_draft += 1,
            _ => gov_locked += 1,
        }

        let action = truth
            .map(|truth| ActionFacts {
                label_key: truth.latest_action.label_key.clone(),
                when_ms: truth.latest_action.when_ms,
                user: truth.latest_action.user.clone(),
            })
            .unwrap_or(ActionFacts {
                label_key: String::new(),
                when_ms: 0,
                user: String::new(),
            });
        // `latestAction` reports nothing at all when nothing has happened.
        let action = if action.when_ms == 0 {
            ActionFacts {
                label_key: String::new(),
                when_ms: 0,
                user: String::new(),
            }
        } else {
            action
        };

        // Phase-by-phase, in the three slots the page summarises. The retired
        // Draft/Sketch phase is not a slot: there is one way to process a
        // transformer, so a stamp an older document still carries is no
        // summary of anything.
        for (slot, status) in [
            (0usize, phase_status(process).to_string()),
            (
                1,
                phase_status(phase_meta(row, "report_metadata")).to_string(),
            ),
            (
                2,
                phase_status(phase_meta(row, "combined_inclusion_metadata")).to_string(),
            ),
        ] {
            *phase_counts[slot].entry(status).or_insert(0) += 1;
        }

        if let Some(note) = attention_note(row, &name, &health, truth, &action) {
            notes.push(note);
        }
        actions.push((index, name, action));
    }

    // ── the aggregates ──────────────────────────────────────────────
    let crew_buckets = counted_buckets(&designed_users, CREW_LIMIT);
    let members: Vec<CrewMember> = crew_buckets
        .iter()
        .map(|(name, count)| crew_member(name, *count))
        .collect();
    let contributors = distinct_count(&designed_users);
    let leader = members.first().cloned();

    let points: Vec<MomentumPoint> = day_counts
        .into_iter()
        .map(|(key, count)| MomentumPoint { key, count })
        .collect();
    let busiest = points
        .iter()
        .fold(None::<&MomentumPoint>, |best, point| match best {
            Some(best) if best.count >= point.count => Some(best),
            _ => Some(point),
        })
        .cloned();
    let active_days = points.len();
    let avg_per_active_day = if active_days == 0 {
        number(0.0)
    } else {
        number(js_round((dated_total as f64 / active_days as f64) * 10.0) as f64 / 10.0)
    };
    let momentum = Momentum {
        max: points
            .iter()
            .map(|point| point.count)
            .max()
            .unwrap_or(1)
            .max(1),
        points,
        total: dated_total,
        last7,
        prev7,
        trend: match last7.cmp(&prev7) {
            Ordering::Greater => Trend::Up,
            Ordering::Less => Trend::Down,
            Ordering::Equal => Trend::Flat,
        },
        busiest,
        active_days,
        avg_per_active_day,
    };

    let district_buckets = counted_buckets(&districts, DISTRICT_LIMIT);
    let district_max = district_buckets
        .iter()
        .map(|(_, count)| *count)
        .max()
        .unwrap_or(1)
        .max(1);
    let district_ring = ring_geometry(
        &district_buckets
            .iter()
            .map(|(_, count)| *count)
            .collect::<Vec<usize>>(),
    );
    let districts: Vec<DistrictTag> = district_buckets
        .into_iter()
        .zip(district_ring)
        .map(|((label, count), (percent, offset))| DistrictTag {
            unassigned: label.is_empty(),
            weight: count as f64 / district_max as f64,
            label,
            count,
            percent,
            offset,
        })
        .collect();
    let district_count = distinct_count(&district_names);
    let sector_count = distinct_count(&sector_names);

    let governance: Vec<GovernanceMix> = [
        (GovernanceBucket::Locked, "ddash_leg_locked", gov_locked),
        (GovernanceBucket::Draft, "ddash_leg_draft", gov_draft),
    ]
    .into_iter()
    .filter(|(_, _, count)| *count > 0)
    .map(|(status, label_key, count)| GovernanceMix {
        status,
        label_key: label_key.into(),
        count,
    })
    .collect();
    let locked_pct = pct(gov_locked, total);

    notes.sort_by(|left, right| {
        left.tone
            .cmp(&right.tone)
            .then_with(|| locale_compare(&left.name, &right.name))
    });
    let error_count = notes.iter().filter(|note| note.tone == Tone::Red).count();
    let warning_count = notes.iter().filter(|note| note.tone == Tone::Amber).count();
    let info_count = notes.iter().filter(|note| note.tone == Tone::Blue).count();
    let clean_count = total.saturating_sub(notes.len());
    let score = if total == 0 {
        100
    } else {
        pct(clean_count, total)
    };
    let ring_buckets: Vec<(&str, usize)> = [
        ("error", error_count),
        ("warning", warning_count),
        ("info", info_count),
        ("clean", clean_count),
    ]
    .into_iter()
    .filter(|(_, count)| *count > 0)
    .collect();
    let ring: Vec<RingSlice> = ring_buckets
        .iter()
        .zip(ring_geometry(
            &ring_buckets
                .iter()
                .map(|(_, count)| *count)
                .collect::<Vec<usize>>(),
        ))
        .map(|((key, count), (percent, offset))| RingSlice {
            key: (*key).into(),
            label_key: format!("ddash_ring_{key}"),
            count: *count,
            percent,
            offset,
        })
        .collect();

    let error_users: Vec<String> = notes
        .iter()
        .filter(|note| note.tone == Tone::Red)
        .map(|note| note.user.trim().to_string())
        .collect();
    let errors_by_user: Vec<CrewMember> = counted_buckets(&error_users, ERRORS_BY_USER_LIMIT)
        .into_iter()
        .map(|(name, count)| crew_member(&name, count))
        .collect();

    let latest = {
        let mut ranked: Vec<&(usize, String, ActionFacts)> = actions.iter().collect();
        ranked.sort_by_key(|entry| std::cmp::Reverse(entry.2.when_ms));
        ranked
            .first()
            .map(|entry| entry.2.clone())
            .unwrap_or(ActionFacts {
                label_key: String::new(),
                when_ms: 0,
                user: String::new(),
            })
    };
    let recent: Vec<RecentAction> = {
        let mut ranked: Vec<&(usize, String, ActionFacts)> = actions.iter().collect();
        ranked.sort_by_key(|entry| std::cmp::Reverse(entry.2.when_ms));
        ranked
            .into_iter()
            .take(RECENT_LIMIT)
            .map(|entry| RecentAction {
                name: entry.1.clone(),
                action: entry.2.clone(),
            })
            .collect()
    };

    let phase_slots: [(&str, &str, usize); 3] = [
        ("process", "dsh_phase_label_process", 0),
        ("report", "dsh_phase_label_report", 1),
        ("combined", "dsh_phase_label_combined", 2),
    ];
    let phase_summaries: Vec<PhaseSummary> = phase_slots
        .into_iter()
        .map(|(key, label_key, slot)| PhaseSummary {
            key: key.into(),
            label_key: label_key.into(),
            total,
            buckets: PHASE_STATUSES
                .iter()
                .filter_map(|status| {
                    phase_counts[slot]
                        .get(*status)
                        .filter(|count| **count > 0)
                        .map(|count| PhaseBucket {
                            status: (*status).into(),
                            label_key: format!("ddash_phase_status_{status}"),
                            count: *count,
                        })
                })
                .collect(),
        })
        .collect();

    let facts = build_facts(
        &momentum,
        leader.as_ref(),
        district_count,
        sector_count,
        locked_pct,
    );

    let model = Model {
        total,
        designed_count,
        report_ready_count,
        combined_count,
        latest,
        pipeline: vec![
            ("design", "ddash_pipeline_design", total),
            ("designed", "ddash_pipeline_designed", designed_count),
            ("reported", "ddash_pipeline_reported", report_ready_count),
            ("combined", "ddash_pipeline_combined", combined_count),
        ]
        .into_iter()
        .map(|(key, label_key, count)| PipelineStage {
            key: key.into(),
            label_key: label_key.into(),
            count,
            pct_of_total: pct(count, total),
        })
        .collect(),
        momentum,
        crew: Crew {
            members,
            leader,
            contributors,
        },
        errors_by_user,
        districts,
        district_count,
        sector_count,
        phase_summaries,
        governance,
        attention: notes,
        health: HealthReading {
            score,
            label_key: health_label_key(score).into(),
            error_count,
            warning_count,
            info_count,
            clean_count,
            ring,
        },
        recent,
        facts,
    };
    (model, owner_by_name)
}

/// The one issue this row raises, or nothing when it is clean.
///
/// The pile IS `design_health`'s verdict: its first finding is the row's
/// worst, in phase-slot order inside a severity band. The two notes health
/// does not carry — unsaved browser work and a superseded phase — come from
/// `design_status_row`'s answer, not from a second reading of the document.
pub(crate) fn attention_note(
    row: &Map<String, Value>,
    name: &str,
    health: &TransformerHealth,
    truth: Option<&RowReply>,
    action: &ActionFacts,
) -> Option<AttentionNote> {
    // A retired Draft/Sketch phase never reaches the pile: there is one way
    // to process a transformer (owner ruling 2026-09-22), and a stamp an
    // older document still carries is not a finding anyone can act on.
    let finding = health
        .findings
        .iter()
        .find(|finding| !is_retired_phase(finding.phase));
    if let Some(finding) = finding {
        return Some(note_from_finding(
            row,
            name,
            finding,
            &action.user,
            action.when_ms,
        ));
    }
    let truth = truth?;
    if truth.view.sync_state == SyncState::Unsaved {
        return Some(AttentionNote {
            name: name.to_string(),
            tone: Tone::Amber,
            label_key: "ddash_note_unsaved".into(),
            phase: None,
            message: String::new(),
            detail_key: truth.view.sync_title_key.clone(),
            when_ms: action.when_ms,
            user: action.user.clone(),
        });
    }
    let detail_key = match truth.phase_ownership.owner {
        // Process owns the record: the legacy run is the superseded one.
        PhaseOwner::Process => "ddash_note_superseded_legacy",
        PhaseOwner::Legacy => "ddash_note_superseded_process",
        PhaseOwner::None => return None,
    };
    Some(AttentionNote {
        name: name.to_string(),
        tone: Tone::Blue,
        label_key: "ddash_note_superseded".into(),
        phase: None,
        message: String::new(),
        detail_key: detail_key.into(),
        when_ms: action.when_ms,
        user: action.user.clone(),
    })
}

// ── why these transformers collide ──────────────────────────────────────
//
// A collision means two (or more) transformers claim overlapping ground or a
// clashing identity. The combined report will not export a valid document
// while any collision stands, because the combine step needs every
// transformer to be ONE unique unit moving through one lifecycle. Duplicate
// or near-duplicate names — `wnmajona_edcl2` vs `skwhqafc_edcl` share the
// `edcl` signal; `foo_edcl` vs `foo_edcl2` differ only by a version digit —
// are a common root cause. The modelling is deliberately simple statistics
// over names, not geometry: the geometry is ds-network-reporter's.

/// Involved transformer names on a collision feature (dissolved set first).
fn feature_transformers(feature: &Value) -> Vec<String> {
    let props = feature
        .get("properties")
        .and_then(object)
        .cloned()
        .unwrap_or_default();
    if let Some(joined) = props.get("transformers").and_then(Value::as_str) {
        if !joined.trim().is_empty() {
            return joined
                .split(',')
                .map(|name| name.trim().to_string())
                .filter(|name| !name.is_empty())
                .collect();
        }
    }
    ["transfo_a", "transfo_b"]
        .into_iter()
        .filter_map(|key| props.get(key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// Split a name into alpha and numeric runs: `wnmajona_edcl2` →
/// [wnmajona, edcl, 2].
fn name_tokens(name: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut run = String::new();
    let mut digits = false;
    for character in name.to_lowercase().chars() {
        let is_alpha = character.is_ascii_lowercase();
        let is_digit = character.is_ascii_digit();
        if !is_alpha && !is_digit {
            if !run.is_empty() {
                tokens.push(std::mem::take(&mut run));
            }
            continue;
        }
        if !run.is_empty() && digits != is_digit {
            tokens.push(std::mem::take(&mut run));
        }
        digits = is_digit;
        run.push(character);
    }
    if !run.is_empty() {
        tokens.push(run);
    }
    tokens
}

/// Alpha tokens worth surfacing as a shared "signal" (drop 1-char noise and
/// pure digits), deduped, first occurrence first.
fn signal_tokens(name: &str) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for token in name_tokens(name) {
        if token.len() < 3 || !token.chars().any(|c| c.is_ascii_lowercase()) {
            continue;
        }
        if !seen.contains(&token) {
            seen.push(token);
        }
    }
    seen
}

/// Compact identity: lowercase with separators removed, so `Foo_EDCL` and
/// `foo-edcl` collapse to the same `fooedcl`.
fn normalize_name(name: &str) -> String {
    name.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        .collect()
}

/// Strip ONE trailing version-ish suffix — a small number (optional leading
/// `v`) or a copy/dup word — leaving the un-versioned stem.
fn strip_version_suffix(normalized: &str) -> String {
    let mut word = normalized.to_string();
    for suffix in ["copy", "duplicate", "dup", "bis"] {
        if let Some(stem) = word.strip_suffix(suffix) {
            word = stem.to_string();
            break;
        }
    }
    let digits = word
        .chars()
        .rev()
        .take_while(char::is_ascii_digit)
        .count()
        .min(3);
    let mut stem = word.clone();
    if digits > 0 {
        stem.truncate(word.len() - digits);
        if stem.ends_with('v') {
            stem.pop();
        }
    }
    if stem.is_empty() {
        normalized.to_string()
    } else {
        stem
    }
}

fn pick_base(names: &[String]) -> String {
    let mut sorted = names.to_vec();
    sorted.sort_by(|left, right| {
        left.chars()
            .count()
            .cmp(&right.chars().count())
            .then_with(|| locale_compare(left, right))
    });
    sorted.first().cloned().unwrap_or_default()
}

/// Names that look like the SAME transformer entered twice. Deliberately
/// conservative so it never smears a legitimate numbered series
/// (`kigali_11` vs `kigali_12` are DIFFERENT transformers): a group is
/// reported only when the un-versioned original is actually present, or when
/// two names collapse to the same identity after normalising case and
/// separators.
fn find_look_alike_duplicates(names: &[String]) -> Vec<DuplicateBaseGroup> {
    let mut groups: BTreeMap<String, (Vec<String>, bool)> = BTreeMap::new();
    for name in names {
        let normalized = normalize_name(name);
        if normalized.is_empty() {
            continue;
        }
        let stem = strip_version_suffix(&normalized);
        let group = groups.entry(stem.clone()).or_insert((Vec::new(), false));
        if !group.0.contains(name) {
            group.0.push(name.clone());
        }
        if normalized == stem {
            group.1 = true; // an un-versioned original is present in the set
        }
    }
    let mut out: Vec<DuplicateBaseGroup> = Vec::new();
    for (members, bare) in groups.into_values() {
        if members.len() < 2 {
            continue;
        }
        let distinct: std::collections::BTreeSet<String> =
            members.iter().map(|name| normalize_name(name)).collect();
        // Two spellings, one identity.
        let exact_twins = distinct.len() < members.len();
        if !bare && !exact_twins {
            continue; // a plain numbered series — not a duplicate
        }
        let mut sorted = members.clone();
        sorted.sort_by(|left, right| locale_compare(left, right));
        out.push(DuplicateBaseGroup {
            base: pick_base(&sorted),
            names: sorted,
        });
    }
    out.sort_by(|left, right| {
        right
            .names
            .len()
            .cmp(&left.names.len())
            .then_with(|| locale_compare(&left.base, &right.base))
    });
    out.truncate(COLLISION_GROUP_LIMIT);
    out
}

fn build_collisions(
    features: &[Value],
    owner_by_name: &BTreeMap<String, String>,
) -> CollisionsInsight {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut user_counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut example_pair: Option<[String; 2]> = None;

    for feature in features {
        let names = feature_transformers(feature);
        for name in &names {
            seen.entry(name.to_lowercase())
                .or_insert_with(|| name.clone());
        }
        if example_pair.is_none() && names.len() >= 2 {
            example_pair = Some([names[0].clone(), names[1].clone()]);
        }
        // One collision counts once per involved user: two of a user's own
        // transformers colliding is still one collision they own.
        let users: std::collections::BTreeSet<String> = names
            .iter()
            .map(|name| {
                owner_by_name
                    .get(&canonical(name))
                    .cloned()
                    .unwrap_or_default()
                    .trim()
                    .to_string()
            })
            .collect();
        for user in users {
            *user_counts.entry(user).or_insert(0) += 1;
        }
    }

    let mut colliding: Vec<String> = seen.into_values().collect();
    colliding.sort_by(|left, right| locale_compare(left, right));
    let colliding_count = colliding.len();

    let mut by_user: Vec<(String, usize)> = user_counts.into_iter().collect();
    by_user.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| left.0.is_empty().cmp(&right.0.is_empty()))
            .then_with(|| locale_compare(&left.0, &right.0))
    });
    by_user.truncate(COLLISION_USER_LIMIT);
    let by_user: Vec<CollisionUserBucket> = by_user
        .into_iter()
        .map(|(name, count)| CollisionUserBucket {
            short: if name.is_empty() {
                String::new()
            } else {
                short_user(&name)
            },
            unknown: name.is_empty(),
            name,
            count,
        })
        .collect();

    let mut token_counts: BTreeMap<String, usize> = BTreeMap::new();
    for name in &colliding {
        for token in signal_tokens(name) {
            *token_counts.entry(token).or_insert(0) += 1;
        }
    }
    let mut tokens: Vec<(String, usize)> = token_counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .collect();
    tokens.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| locale_compare(&left.0, &right.0))
    });
    tokens.truncate(COLLISION_TOKEN_LIMIT);
    let common_tokens: Vec<NameTokenBucket> = tokens
        .into_iter()
        .map(|(token, count)| NameTokenBucket {
            token,
            pct: pct(count, colliding_count),
            count,
        })
        .collect();

    CollisionsInsight {
        collision_count: features.len(),
        duplicate_bases: find_look_alike_duplicates(&colliding),
        colliding_transformers: colliding,
        colliding_count,
        by_user,
        common_tokens,
        example_pair,
    }
}

// ── the call ────────────────────────────────────────────────────────────

fn build(request: Request) -> Result<Reply, String> {
    if request.schema != SCHEMA {
        return Err("Unsupported design dashboard schema.".into());
    }
    let (dashboard, owner_by_name) = build_model(&request);
    let collisions = request
        .collisions
        .as_ref()
        .map(|collisions| build_collisions(&collisions.features, &owner_by_name));
    Ok(Reply {
        schema: SCHEMA,
        dashboard,
        collisions,
    })
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("Design dashboard request exceeds its bound.".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid design dashboard request: {error}"))?;
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

    fn dashboard(rows: Value) -> Value {
        reply(json!({"schema": SCHEMA, "rows": rows, "now_ms": 1_787_743_440_000i64}))["dashboard"]
            .clone()
    }

    #[test]
    fn an_empty_list_is_a_zeroed_but_well_formed_model() {
        let model = dashboard(json!([]));
        assert_eq!(model["total"], 0);
        assert_eq!(model["health"]["score"], 100);
        assert_eq!(model["health"]["label_key"], "ddash_health_thriving");
        assert_eq!(model["pipeline"][0]["key"], "design");
        assert_eq!(model["pipeline"][0]["pct_of_total"], 0);
        assert!(model["facts"].as_array().unwrap().is_empty());
        assert_eq!(model["latest"]["when_ms"], 0);
    }

    #[test]
    fn a_transformer_is_designed_through_any_compute_phase() {
        let model = dashboard(json!([
            {"name": "a", "process_metadata": {"status": "success", "when": "2026-08-25T09:00:00Z", "user": "a@x", "lane": "fast"},
             "layers": {"lv_poles": 1, "service_cables": 1, "spans": 1}},
            {"name": "b", "draft_metadata": {"status": "success", "when": "2026-08-24T09:00:00Z", "user": "b@x"}},
            {"name": "c", "sketch_metadata": {"status": "success", "sketched_at": "2026-08-23T09:00:00Z", "user": "b@x"}},
        ]));
        assert_eq!(model["designed_count"], 3);
        // Not a funnel: each stage is measured against the total.
        assert_eq!(model["pipeline"][1]["pct_of_total"], 100);
        // The model has no lane split, no lane share and no per-lane count:
        // there is one way to process a transformer.
        for retired in ["lanes", "fast_pct", "process_lane_count"] {
            assert!(model.get(retired).is_none(), "{retired} is still answered");
        }
        assert!(model["facts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|fact| fact["key"] != "ddash_fact_fast_share"));
    }

    #[test]
    fn a_success_stamp_with_no_saved_output_is_never_designed_and_is_always_red() {
        let model = dashboard(json!([
            {"name": "missing", "process_metadata": {"status": "success", "when": "2026-08-25T09:00:00Z", "user": "a@x"},
             "layers": {"other": 1}},
        ]));
        assert_eq!(model["designed_count"], 0);
        assert_eq!(model["health"]["error_count"], 1);
        assert_eq!(
            model["attention"][0]["label_key"],
            "ddash_note_output_missing"
        );
        assert_eq!(model["attention"][0]["user"], "a@x");
    }

    #[test]
    fn special_rows_are_excluded_by_the_crate_s_one_row_identity_table() {
        let model = dashboard(json!([
            {"name": "tx_1"},
            {"name": "combined_transformer"},
            {"name": "mv_data"},
            {"name": "collisions"},
            {"name": "all_transformers"},
        ]));
        assert_eq!(model["total"], 1);
    }

    #[test]
    fn the_retired_draft_sketch_phase_has_no_summary_and_reaches_no_pile() {
        let rows = json!([
            {"name": "a", "draft_metadata": {"status": "error", "when": "2026-08-25T09:00:00Z", "error_message": "bad"}},
            {"name": "b", "process_metadata": {"status": "success", "when": "2026-08-25T09:00:00Z", "lane": "standard"},
             "layers": {"lv_poles": 1, "service_cables": 1, "spans": 1}},
            {"name": "c", "draft_metadata": {"status": "error", "when": "2026-08-20T10:00:00Z", "error_message": "Drafting failed."},
             "process_metadata": {"status": "success", "when": "2026-08-25T10:00:00Z"},
             "layers": {"lv_poles": 1, "service_cables": 1, "spans": 1}},
        ]);
        let model = reply(json!({"schema": SCHEMA, "rows": rows, "now_ms": 1_787_743_440_000i64}))
            ["dashboard"]
            .clone();
        let keys: Vec<&str> = model["phase_summaries"]
            .as_array()
            .unwrap()
            .iter()
            .map(|summary| summary["key"].as_str().unwrap())
            .collect();
        assert_eq!(keys, ["process", "report", "combined"]);
        // `a` errored only on the retired phase; `c` carries a superseded
        // legacy error. Neither is anything a person can act on today.
        assert!(model["attention"].as_array().unwrap().is_empty());
        // The request has no lane switch any more.
        let bytes = serde_json::to_vec(
            &json!({"schema": SCHEMA, "rows": [], "now_ms": 1_787_743_440_000i64, "fast_lane": true}),
        )
        .unwrap();
        assert!(evaluate(&bytes).is_err());
    }

    #[test]
    fn a_live_diagnostic_becomes_a_phase_blob_unless_the_row_already_reports_one() {
        let model = reply(json!({
            "schema": SCHEMA,
            "rows": [
                {"name": "clean"},
                {"name": "already", "process_metadata": {"status": "error", "error_message": "its own"}},
            ],
            "now_ms": 1_787_743_440_000i64,
            "diagnostics": [
                {"transformer_name": "clean", "severity": "error", "message": "run failed", "at": 1_787_743_000_000i64, "user": "a@x"},
                {"transformer_name": "already", "severity": "warning", "message": "ignored", "at": 1_787_743_000_000i64},
                {"transformer_name": "ghost", "severity": "warning", "message": "never landed", "at": 1_787_743_000_000i64},
            ],
        }))["dashboard"]
            .clone();
        assert_eq!(model["total"], 3, "the ghost row is synthesised");
        let notes = model["attention"].as_array().unwrap();
        assert_eq!(notes[0]["name"], "already");
        assert_eq!(notes[0]["message"], "its own");
        assert_eq!(notes[1]["name"], "clean");
        assert_eq!(notes[1]["message"], "run failed");
        assert_eq!(notes[2]["name"], "ghost");
        assert_eq!(notes[2]["tone"], "amber");
    }

    #[test]
    fn the_momentum_timeline_buckets_by_the_host_s_local_day() {
        let row = json!([{"name": "a",
            "process_metadata": {"status": "success", "when": "2026-08-25T23:30:00Z", "user": "a@x"},
            "layers": {"lv_poles": 1, "service_cables": 1, "spans": 1}}]);
        let utc = reply(json!({"schema": SCHEMA, "rows": row, "now_ms": 1_787_743_440_000i64}));
        assert_eq!(
            utc["dashboard"]["momentum"]["points"][0]["key"],
            "2026-08-25"
        );
        let kigali = reply(
            json!({"schema": SCHEMA, "rows": row, "now_ms": 1_787_743_440_000i64, "tz_offset_minutes": 120}),
        );
        assert_eq!(
            kigali["dashboard"]["momentum"]["points"][0]["key"],
            "2026-08-26"
        );
    }

    #[test]
    fn the_day_key_and_the_iso_stamp_are_each_other_s_inverse() {
        assert_eq!(day_key(0, 0), "1970-01-01");
        assert_eq!(day_key(1_787_743_440_000, 0), "2026-08-26");
        assert_eq!(day_key(1_787_743_440_000, -720), "2026-08-25");
        assert_eq!(iso_from_ms(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(iso_from_ms(1_787_743_440_000), "2026-08-26T11:24:00.000Z");
    }

    #[test]
    fn the_bucket_order_is_the_root_collation_the_page_sorted_with() {
        assert_eq!(locale_compare("a", "B"), Ordering::Less);
        assert_eq!(locale_compare("tx_1", "tx1"), Ordering::Less);
        assert_eq!(locale_compare("tx2", "tx10"), Ordering::Greater);
        assert_eq!(locale_compare("Gasabo", "gasabo"), Ordering::Greater);
    }

    #[test]
    fn a_version_suffix_is_stripped_but_a_numbered_series_is_not_a_duplicate() {
        assert_eq!(strip_version_suffix("fooedcl2"), "fooedcl");
        assert_eq!(strip_version_suffix("fooedclv12"), "fooedcl");
        assert_eq!(strip_version_suffix("foocopy"), "foo");
        assert_eq!(
            strip_version_suffix("1234"),
            "1",
            "the trailing run is capped at three digits"
        );
        let groups = find_look_alike_duplicates(&[
            "foo_edcl".into(),
            "foo_edcl2".into(),
            "kigali_11".into(),
            "kigali_12".into(),
            "Nyamata".into(),
            "nyamata".into(),
        ]);
        let bases: Vec<&str> = groups.iter().map(|group| group.base.as_str()).collect();
        assert!(
            bases.contains(&"foo_edcl"),
            "the un-versioned original is present"
        );
        assert!(
            bases.contains(&"nyamata"),
            "two spellings, one identity, named by the lowercase one"
        );
        assert!(
            !groups
                .iter()
                .any(|group| group.names.iter().any(|name| name == "kigali_11")),
            "a legitimate numbered series is never smeared"
        );
    }

    #[test]
    fn a_name_splits_into_the_alpha_and_numeric_runs_the_signal_count_reads() {
        assert_eq!(name_tokens("wnmajona_edcl2"), ["wnmajona", "edcl", "2"]);
        assert_eq!(signal_tokens("wnmajona_edcl2"), ["wnmajona", "edcl"]);
        assert!(
            signal_tokens("a_b_12").is_empty(),
            "noise and digits are dropped"
        );
    }

    #[test]
    fn a_dissolved_collision_feature_names_every_transformer_on_it() {
        let insight = build_collisions(
            &[
                json!({"properties": {"transformers": " a , b ,c "}}),
                json!({"properties": {"transfo_a": "d", "transfo_b": "e"}}),
                json!({"properties": {}}),
            ],
            &BTreeMap::from([("a".to_string(), "owner@x".to_string())]),
        );
        assert_eq!(insight.collision_count, 3);
        assert_eq!(insight.colliding_count, 5);
        assert_eq!(insight.example_pair, Some(["a".into(), "b".into()]));
        assert_eq!(insight.by_user[0].count, 2, "the unattributed bucket leads");
        assert!(insight.by_user[0].unknown);
        assert_eq!(insight.by_user[1].name, "owner@x");
    }

    #[test]
    fn the_request_is_bounded_and_its_schema_is_checked() {
        assert!(evaluate(&vec![b' '; MAX_INPUT_BYTES + 1]).is_err());
        assert!(evaluate(br#"{"schema":"ds.other/v1","rows":[]}"#).is_err());
    }

    #[test]
    fn ring_geometry_rounds_each_share_once_so_the_offsets_never_drift() {
        assert_eq!(ring_geometry(&[]), Vec::<(i64, i64)>::new());
        assert_eq!(ring_geometry(&[3]), [(100, 0)]);
        // 1/3 each: the summed rounded percents would end at 99; the
        // cumulative share rounded once lands the last slice at 67.
        assert_eq!(ring_geometry(&[1, 1, 1]), [(33, 0), (33, 33), (33, 67)]);
        // Forty single-count chips over 65: the page's float sum ended at
        // 100 - 1.54; the summed ROUNDED percents would have ended at 80.
        let mut counts = vec![25usize];
        counts.extend(std::iter::repeat_n(1, 40));
        let last = *ring_geometry(&counts).last().unwrap();
        assert_eq!(last, (2, 98));
    }

    #[test]
    fn the_donuts_carry_their_own_geometry_and_the_health_ring_drops_empty_bands() {
        let model = dashboard(json!([
            {"name": "a", "admin_bounds": {"district": "Gasabo"},
             "process_metadata": {"status": "success", "when": "2026-08-25T09:00:00Z", "user": "a@x", "lane": "fast"},
             "layers": {"lv_poles": 1, "service_cables": 1, "spans": 1}},
            {"name": "b", "admin_bounds": {"district": "Gasabo"},
             "process_metadata": {"status": "success", "when": "2026-08-25T09:00:00Z", "user": "a@x", "lane": "standard"},
             "layers": {"lv_poles": 1, "service_cables": 1, "spans": 1}},
            {"name": "c", "admin_bounds": {"district": "Kicukiro"},
             "process_metadata": {"status": "error", "when": "2026-08-25T09:00:00Z", "error_message": "bad"}},
            {"name": "d"},
        ]));
        let districts = model["districts"].as_array().unwrap();
        assert_eq!(districts[0]["label"], "Gasabo");
        assert_eq!(districts[0]["percent"], 50);
        assert_eq!(districts[0]["offset"], 0);
        assert_eq!(districts[1]["percent"], 25);
        assert_eq!(districts[1]["offset"], 50);
        assert_eq!(districts[2]["unassigned"], true);
        assert_eq!(districts[2]["offset"], 75);
        let ring = model["health"]["ring"].as_array().unwrap();
        assert_eq!(ring.len(), 2, "no warning or info band is drawn");
        assert_eq!(ring[0]["key"], "error");
        assert_eq!(ring[0]["label_key"], "ddash_ring_error");
        assert_eq!(ring[0]["count"], 1);
        assert_eq!(ring[0]["percent"], 25);
        assert_eq!(ring[0]["offset"], 0);
        assert_eq!(ring[1]["key"], "clean");
        assert_eq!(ring[1]["label_key"], "ddash_ring_clean");
        assert_eq!(ring[1]["percent"], 75);
        assert_eq!(ring[1]["offset"], 25);
        let empty = dashboard(json!([]));
        assert!(
            empty["health"].get("ring").is_none(),
            "nothing to draw, nothing sent"
        );
    }

    #[test]
    fn a_phase_bucket_names_its_status_as_a_key() {
        let model = dashboard(json!([
            {"name": "a", "process_metadata": {"status": "stale_processing"}},
            {"name": "b"},
        ]));
        let process = model["phase_summaries"]
            .as_array()
            .unwrap()
            .iter()
            .find(|summary| summary["key"] == "process")
            .unwrap();
        let buckets = process["buckets"].as_array().unwrap();
        assert_eq!(buckets[0]["status"], "stale_processing");
        assert_eq!(
            buckets[0]["label_key"],
            "ddash_phase_status_stale_processing"
        );
        assert_eq!(buckets[1]["status"], "none");
        assert_eq!(buckets[1]["label_key"], "ddash_phase_status_none");
    }
}
