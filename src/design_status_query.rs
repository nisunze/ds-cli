//! Which transformers the register shows, in what order, which filter options
//! the operator may be offered, and which the picker lists — ONE selector over
//! the rows' truth (`design_status_row`) and verdict (`design_health`), with
//! two projections: the register (`query`) and the picker (`picker`).
//!
//! The browser had three layers (a pure function, a component post-filter and
//! a saved-selection pre-scope) and two vocabularies (the register's and the
//! picker's), plus a second authority that enumerated the options each filter
//! may offer and pruned stale filter state. All of it is one declarative
//! selector here. What a row IS (saved, locked, who acted, its lane, its
//! findings) is read from the two kernels named above, never re-derived.
use crate::design::{canonical_row_name, is_special_row};
use crate::design_health::{transformer_health, FindingSeverity, TransformerHealth};
use crate::design_status_row::{self, GovernanceState, RowReply, SyncState};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::cmp::Ordering;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub const SCHEMA: &str = "ds.design-status-query/v1";
const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
/// The admin filter value that selects rows with no bounds at that level.
pub const ADMIN_NONE_VALUE: &str = "__admin_bounds_none__";

const ADMIN_LEVELS: [&str; 4] = ["district", "sector", "cell", "village"];
const SORT_KEYS: [&str; 11] = [
    "name",
    "district",
    "tags",
    "legacy",
    "process",
    "report",
    "combined",
    "governance",
    "user",
    "updated",
    "version",
];
const PHASE_FILTER_ORDER: [&str; 7] = [
    "success",
    "warning",
    "processing",
    "stale_processing",
    "stale",
    "error",
    "none",
];
const COMBINED_FILTER_ORDER: [&str; 5] = ["success", "stale", "processing", "error", "none"];
const GOVERNANCE_FILTER_ORDER: [&str; 2] = ["locked", "draft"];
const SYNC_VALUES: [&str; 2] = ["saved", "unsaved"];
const LANE_VALUES: [&str; 2] = ["standard", "fast"];
const PICKER_FILTERS: [&str; 6] = ["all", "fast", "standard", "unsaved", "warning", "error"];
/// Deterministic aggregate-row order: compounded report first, then the
/// combined selection, then `mv_data`, then collisions, then anything else
/// the vocabulary calls special.
const SPECIAL_ORDER: [&str; 5] = [
    "compounded_report",
    "combined_transformer",
    "all_transformers",
    "mv_data",
    "collisions",
];

// ── the request ──────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Query(Box<QueryRequest>),
    Picker(PickerRequest),
}

/// The register's question: these rows, this operator's selector, this sort.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryRequest {
    schema: String,
    /// The status rows as the page holds them (server rows with the client's
    /// own `client_*` facts merged in).
    rows: Vec<Value>,
    #[serde(default)]
    context: Context,
    #[serde(default)]
    selector: Selector,
    #[serde(default)]
    sort: Option<Sort>,
    /// The Fast-lane tag summary per row name, as the page renders it.
    #[serde(default)]
    tag_summaries: Map<String, Value>,
    /// Ask for the picker's option facts for these names as well.
    #[serde(default)]
    picker_names: Option<Vec<String>>,
}

fn default_true() -> bool {
    true
}

/// What the host knows beyond the rows.
#[derive(Deserialize)]
#[serde(deny_unknown_fields, default)]
struct Context {
    active_session_transformer: Option<String>,
    current_user_email: String,
    local_edit_lock: Option<String>,
    now_ms: i64,
    /// Fast mode has no Draft/Sketch column and no lane filter (owner ruling
    /// 2026-08-26); it is the only lane with a Tags column.
    fast_lane: bool,
    /// Whether the combined-mirror column is on this page at all.
    #[serde(default = "default_true")]
    combined_ui: bool,
}

/// The one selector. Every field is optional; an absent field selects nothing
/// away. Values the vocabulary does not know are dropped, as the page's
/// persisted filter state always was.
#[derive(Deserialize, Default, Clone)]
#[serde(deny_unknown_fields, default)]
struct Selector {
    search: String,
    admin_level: Option<String>,
    admin_value: String,
    sync: Vec<String>,
    legacy: String,
    process: String,
    report: String,
    combined: String,
    governance: String,
    user: String,
    lane: Vec<String>,
    warning_types: Vec<String>,
    tag_presence: String,
    pinned_only: bool,
    pinned_names: Vec<String>,
    /// The names an applied tag filter selected; absent = no tag filter.
    tag_names: Option<Vec<String>>,
    /// `all`, `unsaved` or `conflicts`.
    review_scope: String,
    /// The rows the review context reports as conflicted.
    conflict_names: Vec<String>,
    /// The live member names of the saved selection the page was opened
    /// against; absent = the whole project. Membership is ds-brain's answer.
    selection_members: Option<Vec<String>>,
}

impl Default for Context {
    fn default() -> Self {
        Self {
            active_session_transformer: None,
            current_user_email: String::new(),
            local_edit_lock: None,
            now_ms: 0,
            fast_lane: false,
            combined_ui: true,
        }
    }
}

#[derive(Deserialize, Clone)]
#[serde(deny_unknown_fields)]
struct Sort {
    key: String,
    #[serde(default = "default_true")]
    asc: bool,
}

/// The picker's question over option facts the register projection produced.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PickerRequest {
    schema: String,
    options: Vec<OptionFacts>,
    #[serde(default)]
    filter: String,
    #[serde(default)]
    search: String,
    #[serde(default)]
    warning_types: Vec<String>,
    #[serde(default)]
    pinned_first: bool,
    #[serde(default)]
    pinned_names: Vec<String>,
    #[serde(default)]
    active_name: Option<String>,
}

/// What the picker knows about one option — the facts `query` projected.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct OptionFacts {
    pub name: String,
    pub label: String,
    #[serde(default)]
    pub lane: Option<String>,
    #[serde(default)]
    pub dirty: bool,
    #[serde(default)]
    pub warning: bool,
    #[serde(default)]
    pub error: bool,
    /// The row's warning types, once each, in finding order.
    #[serde(default)]
    pub warnings: Vec<WarningType>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct WarningType {
    pub key: String,
    pub label_key: String,
}

// ── the reply ────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct QueryReply {
    schema: &'static str,
    /// The register's rows, in order, by name.
    ordered: Vec<String>,
    /// The selector as the page must hold it: unknown values dropped, values
    /// no row exhibits pruned, dimensions the lane does not have cleared.
    normalized: Normalized,
    /// What each filter may offer, over the whole list.
    options: Options,
    review_counts: ReviewCounts,
    #[serde(skip_serializing_if = "Option::is_none")]
    picker: Option<PickerBuild>,
}

#[derive(Serialize, Default, Debug, PartialEq)]
struct Normalized {
    sync: Vec<String>,
    legacy: String,
    process: String,
    report: String,
    combined: String,
    governance: String,
    user: String,
    lane: Vec<String>,
    warning_types: Vec<String>,
}

#[derive(Serialize, Default, Debug)]
struct Options {
    admin: Vec<String>,
    legacy: Vec<String>,
    process: Vec<String>,
    report: Vec<String>,
    combined: Vec<String>,
    governance: Vec<String>,
    user: Vec<String>,
    warning_types: Vec<WarningTypeOption>,
    sync: Vec<String>,
    lane: Vec<String>,
}

#[derive(Serialize, Debug, PartialEq)]
struct WarningTypeOption {
    key: String,
    label_key: String,
    count: usize,
}

#[derive(Serialize, Default)]
struct ReviewCounts {
    unsaved: usize,
    conflicts: usize,
}

#[derive(Serialize)]
struct PickerBuild {
    options: Vec<PickerOption>,
}

/// One picker option: the facts the chips read, plus what the row renders.
#[derive(Serialize)]
struct PickerOption {
    #[serde(flatten)]
    facts: OptionFacts,
    process_status: String,
    process_when: String,
    local: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    server_known: Option<bool>,
    health: TransformerHealth,
}

#[derive(Serialize)]
struct PickerReply {
    schema: &'static str,
    ordered: Vec<String>,
    counts: BTreeMap<String, usize>,
    warning_types: Vec<WarningTypeOption>,
}

// ── one row's facts, read once ───────────────────────────────────────────

struct RowFacts<'a> {
    name: String,
    lower: String,
    special: bool,
    row: &'a Map<String, Value>,
    truth: Option<&'a RowReply>,
    health: TransformerHealth,
}

impl RowFacts<'_> {
    fn sync_state(&self) -> SyncState {
        self.truth
            .map(|truth| truth.view.sync_state)
            .unwrap_or(SyncState::Saved)
    }
    fn unsaved(&self) -> bool {
        self.sync_state() == SyncState::Unsaved
    }
    fn governance_bucket(&self) -> &'static str {
        match self.truth.map(|truth| truth.governance.state) {
            Some(GovernanceState::Open) | None => "draft",
            _ => "locked",
        }
    }
    /// The governance badge's state: any lock, block or unknown paints locked.
    fn governance_badge_locked(&self) -> bool {
        self.truth.is_some_and(|truth| {
            truth.governance.locally_overridden || truth.governance.state != GovernanceState::Open
        })
    }
    fn attributed_user(&self) -> &str {
        self.truth
            .map(|truth| truth.latest_action.attributed_user.as_str())
            .unwrap_or("")
    }
    fn latest_ms(&self) -> i64 {
        self.truth
            .map(|truth| truth.latest_action.when_ms)
            .unwrap_or(0)
    }
    fn version(&self) -> i64 {
        self.truth
            .map(|truth| truth.view.version.version)
            .unwrap_or(0)
    }
    fn phase(&self, key: &str) -> Option<&Map<String, Value>> {
        self.row.get(key).and_then(Value::as_object)
    }
    /// `t.process_metadata` was truthy: present and neither null, false, 0 nor "".
    fn has_process(&self) -> bool {
        match self.row.get("process_metadata") {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(Value::Number(n)) => n.as_f64() != Some(0.0),
            Some(Value::String(s)) => !s.is_empty(),
            Some(_) => true,
        }
    }
    /// The lane filter's reading: a run that named no lane is Standard.
    fn process_lane(&self) -> &'static str {
        match self
            .phase("process_metadata")
            .and_then(|meta| meta.get("lane"))
            .and_then(Value::as_str)
        {
            Some("fast") => "fast",
            _ => "standard",
        }
    }
    fn admin_value(&self, level: &str) -> Option<&str> {
        self.row
            .get("admin_bounds")
            .and_then(Value::as_object)
            .and_then(|bounds| bounds.get(level))
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    }
    fn combined_state(&self) -> &'static str {
        match self
            .row
            .get("computed")
            .and_then(Value::as_object)
            .and_then(|computed| computed.get("combined_inclusion_state"))
            .and_then(Value::as_str)
        {
            Some("success") => "success",
            Some("processing") => "processing",
            Some("error") => "error",
            Some("none") => "none",
            _ => "stale",
        }
    }
    fn warning_keys(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        let mut keys = Vec::new();
        for finding in &self.health.findings {
            if finding.severity != FindingSeverity::Warning {
                continue;
            }
            let key = warning_type_key(&finding.code, &phase_name(finding), &finding.label_key);
            if key.is_empty() || !seen.insert(key.clone()) {
                continue;
            }
            keys.push(key);
        }
        keys
    }
    fn warning_types(&self) -> Vec<WarningType> {
        let mut seen = BTreeSet::new();
        let mut out = Vec::new();
        for finding in &self.health.findings {
            if finding.severity != FindingSeverity::Warning {
                continue;
            }
            let key = warning_type_key(&finding.code, &phase_name(finding), &finding.label_key);
            if key.is_empty() || !seen.insert(key.clone()) {
                continue;
            }
            out.push(WarningType {
                key,
                label_key: finding.label_key.clone(),
            });
        }
        out
    }
}

fn phase_name(finding: &crate::design_health::Finding) -> String {
    serde_json::to_value(finding.phase)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// `warningTypeKey`: the backend code, or `<phase>:<label key>` for a generic one.
fn warning_type_key(code: &str, phase: &str, label_key: &str) -> String {
    let code = code.trim();
    if code.is_empty() {
        format!("{phase}:{label_key}")
    } else {
        code.to_string()
    }
}

/// `phaseFilterValue`: the phase's status, `none` when the phase is absent.
/// A status that is not a string can never match a filter and never
/// becomes an option.
fn phase_filter_value(meta: Option<&Map<String, Value>>) -> String {
    match meta.and_then(|meta| meta.get("status")) {
        None | Some(Value::Null) => "none".to_string(),
        Some(Value::String(status)) => status.clone(),
        Some(_) => "\u{0}".to_string(),
    }
}

/// `latestLegacyPhase`: Draft or Sketch, whichever stamped later.
fn latest_legacy_phase(row: &Map<String, Value>) -> Option<&Map<String, Value>> {
    let draft = row.get("draft_metadata").and_then(Value::as_object);
    let sketch = row.get("sketch_metadata").and_then(Value::as_object);
    match (draft, sketch) {
        (None, None) => None,
        (None, Some(sketch)) => Some(sketch),
        (Some(draft), None) => Some(draft),
        (Some(draft), Some(sketch)) => {
            let sketch_when = when_ms(sketch.get("sketched_at").or_else(|| sketch.get("when")));
            let draft_when = when_ms(draft.get("drafted_at").or_else(|| draft.get("when")));
            if sketch_when >= draft_when {
                Some(sketch)
            } else {
                Some(draft)
            }
        }
    }
}

fn when_ms(value: Option<&Value>) -> i64 {
    design_status_row::timestamp_ms(value)
}

const STATUS_ORDER: [(&str, i64); 5] = [
    ("success", 0),
    ("processing", 1),
    ("stale_processing", 2),
    ("stale", 3),
    ("error", 4),
];

fn status_order(status: &str) -> i64 {
    STATUS_ORDER
        .iter()
        .find(|(name, _)| *name == status)
        .map(|(_, rank)| *rank)
        .unwrap_or(9)
}

fn combined_order(state: &str) -> i64 {
    match state {
        "success" => 0,
        "processing" => 1,
        "stale" => 2,
        "error" => 3,
        _ => 9,
    }
}

// ── search and names ─────────────────────────────────────────────────────

/// `parseNameQuery`: a query wrapped in double quotes is an exact match.
fn parse_name_query(raw: &str) -> (String, bool) {
    let q = raw.trim();
    if q.chars().count() >= 2 && q.starts_with('"') && q.ends_with('"') {
        return (q[1..q.len() - 1].to_lowercase(), true);
    }
    (q.to_lowercase(), false)
}

/// `formatTransformerName`: the label the picker shows and searches.
pub fn format_transformer_name(name: &str) -> String {
    let normalized: String = name
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    match normalized.as_str() {
        "mv_data" => "MV Data".to_string(),
        "combined_transformer" | "all_transformers" => "Combined Report".to_string(),
        _ => name.replace('_', " "),
    }
}

/// `normalizeTransformerSearch`: case- and separator-insensitive
/// (underscores, whitespace and hyphens collapse to one space).
pub fn normalize_transformer_search(value: &str) -> String {
    let lowered = value.trim().to_lowercase();
    let mut out = String::with_capacity(lowered.len());
    let mut in_separator = false;
    for c in lowered.chars() {
        if c == '_' || c == '-' || c.is_whitespace() {
            if !in_separator {
                out.push(' ');
                in_separator = true;
            }
        } else {
            out.push(c);
            in_separator = false;
        }
    }
    out
}

/// A name the working set may pin: trimmed, non-empty and a transformer
/// (the kernel's row-kind vocabulary, not the browser's three name lists).
fn pinnable(name: &str) -> Option<String> {
    let trimmed = name.trim();
    (!trimmed.is_empty() && !is_special_row(trimmed)).then(|| trimmed.to_string())
}

fn pinned_set(names: &[String]) -> HashSet<String> {
    names.iter().filter_map(|name| pinnable(name)).collect()
}

/// `normalizeMultiFilter`: known values once each; every value = no filter.
fn normalize_multi(values: &[String], allowed: &[&str]) -> Vec<String> {
    let mut unique: Vec<String> = Vec::new();
    for value in values {
        if allowed.contains(&value.as_str()) && !unique.contains(value) {
            unique.push(value.clone());
        }
    }
    if unique.len() >= allowed.len() {
        Vec::new()
    } else {
        unique
    }
}

/// `localeCompare` on ASCII account names: letters compare regardless of
/// case first, and a lowercase letter precedes its uppercase twin.
fn locale_cmp(a: &str, b: &str) -> Ordering {
    let key = |s: &str| {
        (
            s.to_lowercase(),
            s.chars().map(char::is_uppercase).collect::<Vec<bool>>(),
        )
    };
    key(a).cmp(&key(b))
}

// ── the register projection ──────────────────────────────────────────────

fn evaluate_query(request: QueryRequest) -> Result<QueryReply, String> {
    if request.schema != SCHEMA {
        return Err("Unsupported design status query schema.".into());
    }
    let admin_level = request
        .selector
        .admin_level
        .clone()
        .unwrap_or_else(|| "district".to_string());
    if !ADMIN_LEVELS.contains(&admin_level.as_str()) {
        return Err(format!("Unknown admin level: {admin_level}."));
    }
    if let Some(sort) = &request.sort {
        if !SORT_KEYS.contains(&sort.key.as_str()) {
            return Err(format!("Unknown sort key: {}.", sort.key));
        }
    }
    match request.selector.tag_presence.as_str() {
        "" | "all" | "tagged" | "untagged" => {}
        other => return Err(format!("Unknown tag presence: {other}.")),
    }
    match request.selector.review_scope.as_str() {
        "" | "all" | "unsaved" | "conflicts" => {}
        other => return Err(format!("Unknown review scope: {other}.")),
    }

    let context = &request.context;
    let truths = design_status_row::truths(
        &request.rows,
        context.active_session_transformer.as_deref(),
        &context.current_user_email,
        context.local_edit_lock.as_deref(),
        context.now_ms,
    );
    let truth_by_canonical: HashMap<String, &RowReply> = truths
        .iter()
        .map(|truth| (canonical_row_name(&truth.name), truth))
        .collect();
    let empty = Map::new();
    let facts: Vec<RowFacts> = request
        .rows
        .iter()
        .map(|row| {
            let object = row.as_object().unwrap_or(&empty);
            let name = object
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            RowFacts {
                lower: name.to_lowercase(),
                special: is_special_row(&name),
                row: object,
                truth: truth_by_canonical.get(&canonical_row_name(&name)).copied(),
                health: transformer_health(row),
                name,
            }
        })
        .collect();

    let options = enumerate_options(&facts, &admin_level, context.combined_ui);
    let normalized = normalize(&request.selector, &facts, context, &options);
    let ordered = select_and_order(&request, &facts, &admin_level);
    let conflicts: HashSet<&str> = request
        .selector
        .conflict_names
        .iter()
        .map(String::as_str)
        .collect();
    let review_counts = ReviewCounts {
        unsaved: facts.iter().filter(|row| row.unsaved()).count(),
        conflicts: facts
            .iter()
            .filter(|row| conflicts.contains(row.name.as_str()))
            .count(),
    };
    let picker = request.picker_names.as_ref().map(|names| {
        let by_name: HashMap<&str, &RowFacts> =
            facts.iter().map(|row| (row.name.as_str(), row)).collect();
        PickerBuild {
            options: names
                .iter()
                .map(|name| picker_option(name, by_name.get(name.as_str()).copied()))
                .collect(),
        }
    });
    Ok(QueryReply {
        schema: SCHEMA,
        ordered,
        normalized,
        options,
        review_counts,
        picker,
    })
}

/// What each filter may offer, over the whole list: per-fleet options never
/// count the special rows.
fn enumerate_options(facts: &[RowFacts], admin_level: &str, combined_ui: bool) -> Options {
    let fleet: Vec<&RowFacts> = facts.iter().filter(|row| !row.special).collect();
    let mut admin: Vec<String> = fleet
        .iter()
        .filter_map(|row| row.admin_value(admin_level).map(str::to_string))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if fleet
        .iter()
        .any(|row| row.admin_value(admin_level).is_none())
    {
        admin.insert(0, ADMIN_NONE_VALUE.to_string());
    }
    let present = |pick: &dyn Fn(&RowFacts) -> String, order: &[&str]| -> Vec<String> {
        let present: HashSet<String> = fleet.iter().map(|row| pick(row)).collect();
        order
            .iter()
            .filter(|value| present.contains(**value))
            .map(|value| value.to_string())
            .collect()
    };
    let legacy = present(
        &|row| phase_filter_value(latest_legacy_phase(row.row)),
        &PHASE_FILTER_ORDER,
    );
    let process = present(
        &|row| phase_filter_value(row.phase("process_metadata")),
        &PHASE_FILTER_ORDER,
    );
    let report = present(
        &|row| phase_filter_value(row.phase("report_metadata")),
        &PHASE_FILTER_ORDER,
    );
    let combined = if combined_ui {
        present(
            &|row| row.combined_state().to_string(),
            &COMBINED_FILTER_ORDER,
        )
    } else {
        Vec::new()
    };
    let governance = present(
        &|row| row.governance_bucket().to_string(),
        &GOVERNANCE_FILTER_ORDER,
    );
    let mut user: Vec<String> = fleet
        .iter()
        .map(|row| row.attributed_user().to_string())
        .filter(|user| !user.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    user.sort_by(|a, b| locale_cmp(a, b));
    // Warning types are counted over the fleet, once per row, in order of
    // first appearance.
    let mut warning_types: Vec<WarningTypeOption> = Vec::new();
    for row in &fleet {
        for warning in row.warning_types() {
            match warning_types
                .iter_mut()
                .find(|option| option.key == warning.key)
            {
                Some(option) => option.count += 1,
                None => warning_types.push(WarningTypeOption {
                    key: warning.key,
                    label_key: warning.label_key,
                    count: 1,
                }),
            }
        }
    }
    let sync = present(
        &|row| {
            match row.sync_state() {
                SyncState::Saved => "saved",
                SyncState::Unsaved => "unsaved",
            }
            .to_string()
        },
        &SYNC_VALUES,
    );
    let lane_present: HashSet<&str> = fleet
        .iter()
        .filter(|row| row.has_process())
        .map(|row| row.process_lane())
        .collect();
    let lane = LANE_VALUES
        .iter()
        .filter(|value| lane_present.contains(**value))
        .map(|value| value.to_string())
        .collect();
    Options {
        admin,
        legacy,
        process,
        report,
        combined,
        governance,
        user,
        warning_types,
        sync,
        lane,
    }
}

/// `normalizeStatusFilterState` and the register's option-pruning effect, as
/// one rule: drop unknown values, clear what this lane does not have, keep
/// only what some row exhibits.
fn normalize(
    selector: &Selector,
    facts: &[RowFacts],
    context: &Context,
    options: &Options,
) -> Normalized {
    let mut next = Normalized {
        sync: normalize_multi(&selector.sync, &SYNC_VALUES),
        legacy: selector.legacy.clone(),
        process: selector.process.clone(),
        report: selector.report.clone(),
        combined: selector.combined.clone(),
        governance: selector.governance.clone(),
        user: selector.user.clone(),
        lane: normalize_multi(&selector.lane, &LANE_VALUES),
        warning_types: selector
            .warning_types
            .iter()
            .filter(|key| !key.is_empty())
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect(),
    };
    if context.fast_lane {
        next.legacy.clear();
        next.lane.clear();
    }
    if !next.sync.is_empty() {
        next.sync.retain(|value| options.sync.contains(value));
    }
    if !next.lane.is_empty() {
        next.lane.retain(|value| options.lane.contains(value));
    }
    if !next.warning_types.is_empty() {
        let present: HashSet<String> = facts
            .iter()
            .filter(|row| !row.special)
            .flat_map(|row| row.warning_keys())
            .collect();
        next.warning_types.retain(|key| present.contains(key));
    }
    if !next.legacy.is_empty() && !options.legacy.contains(&next.legacy) {
        next.legacy.clear();
    }
    if !next.process.is_empty() && !options.process.contains(&next.process) {
        next.process.clear();
    }
    if !next.report.is_empty() && !options.report.contains(&next.report) {
        next.report.clear();
    }
    if !context.combined_ui
        || (!next.combined.is_empty() && !options.combined.contains(&next.combined))
    {
        next.combined.clear();
    }
    if !next.user.is_empty() && !options.user.contains(&next.user) {
        next.user.clear();
    }
    next
}

#[derive(PartialEq, Clone, Debug)]
enum SortVal {
    Str(String),
    Num(i64),
}

fn compare(a: &SortVal, b: &SortVal) -> Ordering {
    match (a, b) {
        (SortVal::Str(a), SortVal::Str(b)) => a.cmp(b),
        (SortVal::Num(a), SortVal::Num(b)) => a.cmp(b),
        _ => Ordering::Equal,
    }
}

fn sort_value(row: &RowFacts, key: &str, tag_summaries: &Map<String, Value>) -> SortVal {
    match key {
        "name" => SortVal::Str(row.lower.clone()),
        "district" => SortVal::Str(row.admin_value("district").unwrap_or("").to_lowercase()),
        "tags" => SortVal::Str(
            tag_summaries
                .get(&row.name)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_lowercase(),
        ),
        "legacy" => SortVal::Num(status_order(
            latest_legacy_phase(row.row)
                .and_then(|meta| meta.get("status"))
                .and_then(Value::as_str)
                .unwrap_or(""),
        )),
        "process" => SortVal::Num(status_order(
            row.phase("process_metadata")
                .and_then(|meta| meta.get("status"))
                .and_then(Value::as_str)
                .unwrap_or(""),
        )),
        "report" => SortVal::Num(status_order(
            row.phase("report_metadata")
                .and_then(|meta| meta.get("status"))
                .and_then(Value::as_str)
                .unwrap_or(""),
        )),
        "combined" => SortVal::Num(combined_order(row.combined_state())),
        "governance" => SortVal::Num(if row.governance_badge_locked() { 1 } else { 9 }),
        "user" => SortVal::Str(row.attributed_user().to_lowercase()),
        "updated" => SortVal::Num(row.latest_ms()),
        "version" => SortVal::Num(row.version()),
        _ => SortVal::Num(0),
    }
}

/// `filterAndSortTransformers` with the page's pre-scope and post-filters
/// folded in — one pass, one order.
fn select_and_order(request: &QueryRequest, facts: &[RowFacts], admin_level: &str) -> Vec<String> {
    let selector = &request.selector;
    let context = &request.context;
    let combined = if context.combined_ui {
        selector.combined.as_str()
    } else {
        ""
    };
    // The Tags column exists only in Fast mode; elsewhere its filter is moot.
    let tag_presence = if context.fast_lane {
        match selector.tag_presence.as_str() {
            "tagged" => "tagged",
            "untagged" => "untagged",
            _ => "all",
        }
    } else {
        "all"
    };
    let sync: Vec<&str> = selector.sync.iter().map(String::as_str).collect();
    let lane: Vec<&str> = selector.lane.iter().map(String::as_str).collect();
    let warning_types: HashSet<&str> = selector
        .warning_types
        .iter()
        .map(String::as_str)
        .filter(|key| !key.is_empty())
        .collect();
    let has_secondary = !sync.is_empty()
        || !selector.legacy.is_empty()
        || !selector.process.is_empty()
        || !selector.report.is_empty()
        || !combined.is_empty()
        || !selector.governance.is_empty()
        || !selector.user.is_empty()
        || !lane.is_empty()
        || !warning_types.is_empty()
        || tag_presence != "all";
    let (needle, exact) = parse_name_query(&selector.search);
    let pinned = pinned_set(&selector.pinned_names);
    let selection: Option<HashSet<&str>> = selector
        .selection_members
        .as_ref()
        .map(|names| names.iter().map(String::as_str).collect());
    let tag_names: Option<HashSet<&str>> = selector
        .tag_names
        .as_ref()
        .map(|names| names.iter().map(String::as_str).collect());
    let conflicts: HashSet<&str> = selector.conflict_names.iter().map(String::as_str).collect();
    let review_scope = selector.review_scope.as_str();

    let matched: Vec<&RowFacts> = facts
        .iter()
        .filter(|row| {
            if selection
                .as_ref()
                .is_some_and(|members| !members.contains(row.name.as_str()))
            {
                return false;
            }
            if selector.pinned_only && (row.special || !pinned.contains(&row.name)) {
                return false;
            }
            if !needle.is_empty() {
                let hit = if exact {
                    row.lower == needle
                } else {
                    row.lower.contains(needle.as_str())
                };
                if !hit {
                    return false;
                }
            }
            if !selector.admin_value.is_empty() && !row.special {
                let value = row.admin_value(admin_level);
                let hit = if selector.admin_value == ADMIN_NONE_VALUE {
                    value.is_none()
                } else {
                    value == Some(selector.admin_value.as_str())
                };
                if !hit {
                    return false;
                }
            }
            if has_secondary && row.special {
                return false;
            }
            if !sync.is_empty() {
                let state = match row.sync_state() {
                    SyncState::Saved => "saved",
                    SyncState::Unsaved => "unsaved",
                };
                if !sync.contains(&state) {
                    return false;
                }
            }
            if !selector.legacy.is_empty()
                && phase_filter_value(latest_legacy_phase(row.row)) != selector.legacy
            {
                return false;
            }
            if !selector.process.is_empty()
                && phase_filter_value(row.phase("process_metadata")) != selector.process
            {
                return false;
            }
            if !selector.report.is_empty()
                && phase_filter_value(row.phase("report_metadata")) != selector.report
            {
                return false;
            }
            if !combined.is_empty() && row.combined_state() != combined {
                return false;
            }
            if !selector.governance.is_empty() && row.governance_bucket() != selector.governance {
                return false;
            }
            if !selector.user.is_empty() && row.attributed_user() != selector.user {
                return false;
            }
            if tag_presence != "all" {
                let tagged = request
                    .tag_summaries
                    .get(&row.name)
                    .and_then(Value::as_str)
                    .is_some_and(|summary| !summary.trim().is_empty());
                if (tag_presence == "tagged") != tagged {
                    return false;
                }
            }
            if !warning_types.is_empty()
                && !row
                    .warning_keys()
                    .iter()
                    .any(|key| warning_types.contains(key.as_str()))
            {
                return false;
            }
            if !lane.is_empty() {
                if !row.has_process() {
                    return false;
                }
                if !lane.contains(&row.process_lane()) {
                    return false;
                }
            }
            if tag_names
                .as_ref()
                .is_some_and(|names| !names.contains(row.name.as_str()))
            {
                return false;
            }
            match review_scope {
                "unsaved" => row.unsaved(),
                "conflicts" => conflicts.contains(row.name.as_str()),
                _ => true,
            }
        })
        .collect();

    // Latest Action sorting is the exception: mv_data is a real project
    // document, so it joins timestamp sort while aggregate rows stay pinned.
    let sort_project_docs_with_rows = request.sort.as_ref().is_some_and(|s| s.key == "updated");
    let joins_rows = |row: &RowFacts| sort_project_docs_with_rows && row.name == "mv_data";
    let mut special: Vec<&RowFacts> = matched
        .iter()
        .copied()
        .filter(|row| row.special && !joins_rows(row))
        .collect();
    let mut rest: Vec<&RowFacts> = matched
        .iter()
        .copied()
        .filter(|row| !row.special || joins_rows(row))
        .collect();
    let special_rank = |name: &str| {
        SPECIAL_ORDER
            .iter()
            .position(|special| *special == name)
            .unwrap_or(SPECIAL_ORDER.len())
    };
    special.sort_by_key(|row| special_rank(&row.name));
    if let Some(sort) = &request.sort {
        let keys: Vec<SortVal> = rest
            .iter()
            .map(|row| sort_value(row, &sort.key, &request.tag_summaries))
            .collect();
        let mut indexed: Vec<usize> = (0..rest.len()).collect();
        indexed.sort_by(|&a, &b| {
            let ordering = compare(&keys[a], &keys[b]);
            if sort.asc {
                ordering
            } else {
                ordering.reverse()
            }
        });
        rest = indexed.into_iter().map(|index| rest[index]).collect();
    }
    special
        .into_iter()
        .chain(rest)
        .map(|row| row.name.clone())
        .collect()
}

// ── the picker projection ────────────────────────────────────────────────

fn picker_option(name: &str, row: Option<&RowFacts>) -> PickerOption {
    let process = row.and_then(|row| row.phase("process_metadata"));
    let process_status = process
        .and_then(|meta| meta.get("status"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let lane = process.filter(|_| !process_status.is_empty()).map(|meta| {
        if meta.get("lane").and_then(Value::as_str) == Some("fast") {
            "fast".to_string()
        } else {
            "standard".to_string()
        }
    });
    let health = row
        .map(|row| row.health.clone())
        .unwrap_or_else(|| TransformerHealth {
            severity: crate::design_health::Severity::Ok,
            findings: Vec::new(),
            error_count: 0,
            warning_count: 0,
        });
    PickerOption {
        facts: OptionFacts {
            name: name.to_string(),
            label: format_transformer_name(name),
            lane,
            dirty: row.is_some_and(RowFacts::unsaved),
            warning: health.warning_count > 0,
            error: health.error_count > 0,
            warnings: row.map(RowFacts::warning_types).unwrap_or_default(),
        },
        process_status,
        process_when: process
            .and_then(|meta| meta.get("when"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        // Availability, not dirtiness: `client_cached` is set for every room
        // this device holds.
        local: row.is_some_and(|row| row.row.get("client_cached") == Some(&Value::Bool(true))),
        server_known: row
            .and_then(|row| row.row.get("client_server_known").and_then(Value::as_bool)),
        health,
    }
}

fn option_matches_chip(option: &OptionFacts, chip: &str) -> bool {
    match chip {
        "fast" => option.lane.as_deref() == Some("fast"),
        "standard" => option.lane.as_deref() == Some("standard"),
        "unsaved" => option.dirty,
        "warning" => option.warning,
        "error" => option.error,
        _ => true,
    }
}

fn option_matches_search(option: &OptionFacts, query: &str) -> bool {
    query.is_empty()
        || normalize_transformer_search(&option.label).contains(query)
        || normalize_transformer_search(&option.name).contains(query)
}

fn evaluate_picker(request: PickerRequest) -> Result<PickerReply, String> {
    if request.schema != SCHEMA {
        return Err("Unsupported design status query schema.".into());
    }
    let chip = if request.filter.is_empty() {
        "all"
    } else {
        request.filter.as_str()
    };
    if !PICKER_FILTERS.contains(&chip) {
        return Err(format!("Unknown picker filter: {chip}."));
    }
    let query = normalize_transformer_search(&request.search);
    let selected: HashSet<&str> = request
        .warning_types
        .iter()
        .map(String::as_str)
        .filter(|key| !key.is_empty())
        .collect();
    let mut counts: BTreeMap<String, usize> = PICKER_FILTERS
        .iter()
        .map(|chip| (chip.to_string(), 0))
        .collect();
    for option in &request.options {
        if !option_matches_search(option, &query) {
            continue;
        }
        for chip in PICKER_FILTERS {
            if option_matches_chip(option, chip) {
                *counts.get_mut(chip).expect("chip") += 1;
            }
        }
    }
    let mut warning_types: Vec<WarningTypeOption> = Vec::new();
    for option in &request.options {
        for warning in &option.warnings {
            match warning_types
                .iter_mut()
                .find(|existing| existing.key == warning.key)
            {
                Some(existing) => existing.count += 1,
                None => warning_types.push(WarningTypeOption {
                    key: warning.key.clone(),
                    label_key: warning.label_key.clone(),
                    count: 1,
                }),
            }
        }
    }
    let mut filtered: Vec<&OptionFacts> = request
        .options
        .iter()
        .filter(|option| option_matches_chip(option, chip))
        .filter(|option| {
            selected.is_empty()
                || option
                    .warnings
                    .iter()
                    .any(|warning| selected.contains(warning.key.as_str()))
        })
        .filter(|option| option_matches_search(option, &query))
        .collect();
    if request.pinned_first {
        let mut names = request.pinned_names.clone();
        names.push(request.active_name.clone().unwrap_or_default());
        let effective = pinned_set(&names);
        // Stable: pinned options keep their relative order, and so do the rest.
        filtered.sort_by_key(|option| !effective.contains(&option.name));
    }
    Ok(PickerReply {
        schema: SCHEMA,
        ordered: filtered
            .into_iter()
            .map(|option| option.name.clone())
            .collect(),
        counts,
        warning_types,
    })
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("Design status query request exceeds its bound.".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid design status query request: {error}"))?;
    match request {
        Request::Query(query) => {
            serde_json::to_string(&evaluate_query(*query)?).map_err(|error| error.to_string())
        }
        Request::Picker(picker) => {
            serde_json::to_string(&evaluate_picker(picker)?).map_err(|error| error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn reply(request: Value) -> Value {
        serde_json::from_str(&evaluate(&serde_json::to_vec(&request).unwrap()).expect("reply"))
            .expect("reply parses")
    }

    fn names(value: &Value) -> Vec<&str> {
        value
            .as_array()
            .expect("array")
            .iter()
            .map(|v| v.as_str().expect("name"))
            .collect()
    }

    fn computed(combined: &str, report: &str) -> Value {
        json!({
            "combined_inclusion_state": combined,
            "report_state": report,
            "report_has_files": report == "success",
            "write_blocked": false,
            "edit_locked": false,
        })
    }

    fn rows() -> Vec<Value> {
        vec![
            json!({
                "name": "alpha",
                "computed": computed("success", "success"),
                "admin_bounds": {"district": "Rutsiro"},
                "draft_metadata": {"status": "success", "drafted_at": "2026-04-20T10:00:00Z", "user": "alice@example.com"},
                "process_metadata": {"status": "success", "when": "2026-04-20T12:00:00Z", "user": "alice@example.com", "lane": "fast"},
                "report_metadata": {"status": "success", "when": "2026-04-20T13:00:00Z", "user": "alice@example.com"},
            }),
            json!({
                "name": "beta",
                "computed": computed("stale", "stale"),
                "admin_bounds": {"district": "Karongi"},
                "draft_metadata": {"status": "error", "drafted_at": "2026-04-20T10:00:00Z", "user": "bob@example.com"},
                "process_metadata": {"status": "warning", "when": "2026-04-20T15:00:00Z", "user": "bob@example.com",
                    "warning": {"code": "non_snapping_lv_lines", "message": "3 lines", "count": 3}},
                "report_metadata": {"status": "stale", "when": "2026-04-20T14:00:00Z", "user": "bob@example.com"},
                "client_cached": true,
                "client_dirty": true,
                "client_server_known": true,
            }),
            json!({"name": "gamma", "computed": computed("none", "none"), "admin_bounds": {"district": "Karongi"}}),
            json!({"name": "epsilon", "computed": computed("none", "none")}),
            json!({
                "name": "delta",
                "computed": computed("error", "error"),
                "admin_bounds": {"district": "Rubavu"},
                "report_metadata": {"status": "error", "when": "2026-04-20T09:00:00Z", "user": "dana@example.com", "error_message": "boom"},
            }),
            json!({"name": "combined_transformer", "computed": computed("stale", "error"), "report_metadata": {"status": "error", "when": "2026-04-20T16:00:00Z"}}),
            json!({"name": "mv_data", "computed": computed("stale", "success"), "report_metadata": {"status": "success", "when": "2026-04-20T17:00:00Z"}}),
        ]
    }

    fn query(selector: Value, sort: Option<Value>) -> Value {
        let mut request =
            json!({"op": "query", "schema": SCHEMA, "rows": rows(), "selector": selector});
        if let Some(sort) = sort {
            request["sort"] = sort;
        }
        reply(request)
    }

    #[test]
    fn special_rows_lead_in_their_pinned_order_and_the_rest_sort_by_name() {
        let out = query(json!({}), Some(json!({"key": "name", "asc": true})));
        assert_eq!(
            names(&out["ordered"]),
            [
                "combined_transformer",
                "mv_data",
                "alpha",
                "beta",
                "delta",
                "epsilon",
                "gamma"
            ]
        );
        let out = query(json!({}), Some(json!({"key": "name", "asc": false})));
        assert_eq!(
            names(&out["ordered"]),
            [
                "combined_transformer",
                "mv_data",
                "gamma",
                "epsilon",
                "delta",
                "beta",
                "alpha"
            ]
        );
    }

    #[test]
    fn latest_action_sort_lets_mv_data_join_the_rows_newest_first() {
        let out = query(json!({}), Some(json!({"key": "updated", "asc": false})));
        assert_eq!(
            names(&out["ordered"]),
            // Equal stamps keep their list order under a stable sort.
            [
                "combined_transformer",
                "mv_data",
                "beta",
                "alpha",
                "delta",
                "gamma",
                "epsilon"
            ]
        );
    }

    #[test]
    fn a_quoted_search_is_exact_and_a_bare_one_is_a_substring() {
        let out = query(json!({"search": "a"}), None);
        assert_eq!(
            names(&out["ordered"]),
            [
                "combined_transformer",
                "mv_data",
                "alpha",
                "beta",
                "gamma",
                "delta"
            ]
        );
        let out = query(json!({"search": "\"alpha\""}), None);
        assert_eq!(names(&out["ordered"]), ["alpha"]);
    }

    #[test]
    fn secondary_filters_hide_the_special_rows_and_admin_filters_keep_them() {
        let out = query(json!({"process": "success"}), None);
        assert_eq!(names(&out["ordered"]), ["alpha"]);
        let out = query(
            json!({"admin_level": "district", "admin_value": "Karongi"}),
            None,
        );
        assert_eq!(
            names(&out["ordered"]),
            ["combined_transformer", "mv_data", "beta", "gamma"]
        );
        let out = query(json!({"admin_value": ADMIN_NONE_VALUE}), None);
        assert_eq!(
            names(&out["ordered"]),
            ["combined_transformer", "mv_data", "epsilon"]
        );
    }

    #[test]
    fn options_are_enumerated_over_the_fleet_in_vocabulary_order() {
        let out = query(json!({}), None);
        assert_eq!(
            names(&out["options"]["admin"]),
            [ADMIN_NONE_VALUE, "Karongi", "Rubavu", "Rutsiro"]
        );
        assert_eq!(
            names(&out["options"]["legacy"]),
            ["success", "error", "none"]
        );
        assert_eq!(
            names(&out["options"]["process"]),
            ["success", "warning", "none"]
        );
        assert_eq!(
            names(&out["options"]["report"]),
            ["success", "stale", "error", "none"]
        );
        assert_eq!(
            names(&out["options"]["combined"]),
            ["success", "stale", "error", "none"]
        );
        assert_eq!(names(&out["options"]["governance"]), ["draft"]);
        assert_eq!(
            names(&out["options"]["user"]),
            ["alice@example.com", "bob@example.com", "dana@example.com"]
        );
        assert_eq!(names(&out["options"]["sync"]), ["saved", "unsaved"]);
        assert_eq!(names(&out["options"]["lane"]), ["standard", "fast"]);
        assert_eq!(
            out["options"]["warning_types"][0]["key"],
            "non_snapping_lv_lines"
        );
        assert_eq!(out["options"]["warning_types"][0]["count"], 1);
    }

    #[test]
    fn normalisation_drops_unknowns_prunes_absent_values_and_clears_fast_lane_columns() {
        let out = query(
            json!({"sync": ["unsaved", "bogus"], "lane": ["fast", "standard"], "warning_types": ["z", "non_snapping_lv_lines", ""], "process": "stale", "user": "nobody@example.com", "legacy": "success"}),
            None,
        );
        assert_eq!(out["normalized"]["sync"], json!(["unsaved"]));
        assert_eq!(out["normalized"]["lane"], json!([]));
        assert_eq!(
            out["normalized"]["warning_types"],
            json!(["non_snapping_lv_lines"])
        );
        assert_eq!(out["normalized"]["process"], "");
        assert_eq!(out["normalized"]["user"], "");
        assert_eq!(out["normalized"]["legacy"], "success");
        let mut request = json!({"op": "query", "schema": SCHEMA, "rows": rows(), "selector": {"legacy": "success", "lane": ["fast"]}, "context": {"fast_lane": true}});
        let out = reply(request.clone());
        assert_eq!(out["normalized"]["legacy"], "");
        assert_eq!(out["normalized"]["lane"], json!([]));
        request["context"] = json!({"combined_ui": false});
        request["selector"] = json!({"combined": "success"});
        let out = reply(request);
        assert_eq!(out["normalized"]["combined"], "");
        assert_eq!(out["options"]["combined"], json!([]));
        // The combined filter is moot without its column: nothing is hidden.
        assert_eq!(names(&out["ordered"]).len(), 7);
    }

    #[test]
    fn the_three_page_layers_are_one_selector() {
        let out = reply(
            json!({"op": "query", "schema": SCHEMA, "rows": rows(), "selector": {"selection_members": ["beta", "gamma", "mv_data"]}, "sort": {"key": "name"}}),
        );
        assert_eq!(names(&out["ordered"]), ["mv_data", "beta", "gamma"]);
        let out = reply(
            json!({"op": "query", "schema": SCHEMA, "rows": rows(), "selector": {"tag_names": ["alpha", "delta"]}}),
        );
        assert_eq!(names(&out["ordered"]), ["alpha", "delta"]);
        let out = reply(
            json!({"op": "query", "schema": SCHEMA, "rows": rows(), "selector": {"review_scope": "unsaved"}}),
        );
        assert_eq!(names(&out["ordered"]), ["beta"]);
        let out = reply(
            json!({"op": "query", "schema": SCHEMA, "rows": rows(), "selector": {"review_scope": "conflicts", "conflict_names": ["gamma"]}}),
        );
        assert_eq!(names(&out["ordered"]), ["gamma"]);
        assert_eq!(out["review_counts"], json!({"unsaved": 1, "conflicts": 1}));
        // The active edit session makes its row unsaved.
        let out = reply(
            json!({"op": "query", "schema": SCHEMA, "rows": rows(), "selector": {"sync": ["unsaved"]}, "context": {"active_session_transformer": "alpha"}}),
        );
        assert_eq!(names(&out["ordered"]), ["alpha", "beta"]);
    }

    #[test]
    fn the_picker_is_a_projection_of_the_same_rows() {
        let out = reply(
            json!({"op": "query", "schema": SCHEMA, "rows": rows(), "picker_names": ["beta", "alpha", "mv_data", "unknown_tr"]}),
        );
        assert!(out["picker"]["options"].is_array(), "{}", out["picker"]);
        let options = out["picker"]["options"].as_array().expect("options");
        assert_eq!(options[0]["name"], "beta");
        assert_eq!(options[0]["dirty"], true);
        assert_eq!(options[0]["local"], true);
        assert_eq!(options[0]["warning"], true);
        assert_eq!(options[0]["lane"], "standard");
        assert_eq!(options[0]["warnings"][0]["key"], "non_snapping_lv_lines");
        assert_eq!(options[1]["lane"], "fast");
        assert_eq!(options[2]["label"], "MV Data");
        assert_eq!(options[3]["label"], "unknown tr");
        assert_eq!(options[3]["health"]["severity"], "ok");
        assert!(options[3].get("server_known").is_none());
        // The picker's second question carries only the facts the chips read.
        let facts: Vec<Value> = options
            .iter()
            .map(|option| {
                let mut facts = Map::new();
                for key in [
                    "name", "label", "lane", "dirty", "warning", "error", "warnings",
                ] {
                    facts.insert(key.into(), option[key].clone());
                }
                Value::Object(facts)
            })
            .collect();
        let options = &facts;
        let picker = reply(
            json!({"op": "picker", "schema": SCHEMA, "options": options, "filter": "all", "search": "", "pinned_first": true, "pinned_names": ["alpha"], "active_name": "mv_data"}),
        );
        assert_eq!(
            names(&picker["ordered"]),
            ["alpha", "beta", "mv_data", "unknown_tr"]
        );
        assert_eq!(
            picker["counts"],
            json!({"all": 4, "error": 0, "fast": 1, "standard": 1, "unsaved": 1, "warning": 1})
        );
        let picker = reply(
            json!({"op": "picker", "schema": SCHEMA, "options": options, "filter": "warning", "search": " BETA "}),
        );
        assert_eq!(names(&picker["ordered"]), ["beta"]);
        let picker = reply(
            json!({"op": "picker", "schema": SCHEMA, "options": options, "search": "mv data"}),
        );
        assert_eq!(names(&picker["ordered"]), ["mv_data"]);
        assert_eq!(picker["counts"]["all"], 1);
        assert_eq!(picker["warning_types"][0]["count"], 1);
    }

    #[test]
    fn unknown_vocabulary_is_refused() {
        let error = evaluate(
            &serde_json::to_vec(
                &json!({"op": "query", "schema": SCHEMA, "rows": [], "sort": {"key": "colour"}}),
            )
            .unwrap(),
        )
        .unwrap_err();
        assert_eq!(error, "Unknown sort key: colour.");
        let error = evaluate(&serde_json::to_vec(&json!({"op": "query", "schema": SCHEMA, "rows": [], "selector": {"admin_level": "planet"}})).unwrap()).unwrap_err();
        assert_eq!(error, "Unknown admin level: planet.");
        let error = evaluate(
            &serde_json::to_vec(
                &json!({"op": "picker", "schema": SCHEMA, "options": [], "filter": "spicy"}),
            )
            .unwrap(),
        )
        .unwrap_err();
        assert_eq!(error, "Unknown picker filter: spicy.");
    }
}
