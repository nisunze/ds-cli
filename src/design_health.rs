//! What is wrong with this transformer — the ONE finding list.
//!
//! A transformer's status document is enumerated by the Design chapter in four
//! places today, and every pair of them disagrees: the browser's severity
//! normaliser, the Dashboard's attention note, the row's permanent-warning
//! scan, and the register's project-wide error/warning tables. This module is
//! the single answer all four become: one ordered finding list per row, a
//! worst-wins severity, and a fleet summary.
//!
//! Pure and cheap by contract (ds-web `CLAUDE.md`, "Transformer health"):
//! every input is already in the `list_transformers_status` response the host
//! holds. No fetch, no clock, no randomness.
//!
//! Two provenances share the list. A finding read from the transformer's own
//! phase blobs is `source: "document"` — it is what the document says and it
//! is what the severity badge counts. A finding the host supplies from the
//! last run's response is `source: "run"`: transient, lost on reload, and
//! deliberately outside the severity verdict, exactly as the browser's info
//! store has always treated it.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Number, Value};

pub const SCHEMA: &str = "ds.design-health/v1";

/// A whole project's status list travels in one call; ~900 rows of phase
/// blobs, uploads and artifacts is the shape this has to admit.
const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
/// Backend text is quoted, so it is bounded before it is read at all.
const MESSAGE_MAX_CHARS: usize = 512;
/// The register previews at most this many `warning.details` entries.
const DETAIL_PREVIEW_MAX: usize = 10;
/// A non-list `warning.details` is previewed as one bounded string.
const DETAIL_STRING_MAX_CHARS: usize = 200;
/// What replaces a URL, bearer or credential-looking run of characters.
const REDACTED: &str = "[redacted]";

// ── the vocabulary ──────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Sketch,
    Draft,
    Process,
    Report,
    Combined,
}

/// Declaration order IS the worst-wins rank.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Ok,
    Stale,
    Processing,
    Warning,
    Error,
}

/// Declaration order IS the finding sort rank (descending).
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum FindingSeverity {
    Stale,
    Processing,
    Warning,
    Error,
}

impl From<FindingSeverity> for Severity {
    fn from(value: FindingSeverity) -> Self {
        match value {
            FindingSeverity::Stale => Severity::Stale,
            FindingSeverity::Processing => Severity::Processing,
            FindingSeverity::Warning => Severity::Warning,
            FindingSeverity::Error => Severity::Error,
        }
    }
}

/// Where this finding was read: the transformer's own document, or the
/// response of the run that just finished.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Document,
    Run,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum LocatableKind {
    Layer,
    Attribute,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct LocatableFilter {
    pub field: String,
    pub value: Value,
}

/// Where the offending features are. `layer` highlights the whole design
/// layer; `attribute` first filters it, falling back to `fallback_layer`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Locatable {
    pub kind: LocatableKind,
    pub layer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filter: Option<LocatableFilter>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_layer: Option<String>,
}

fn located_layer(layer: &str) -> Locatable {
    Locatable {
        kind: LocatableKind::Layer,
        layer: layer.to_string(),
        filter: None,
        fallback_layer: None,
    }
}

/// The offending lines live in `lv_lines` with `connected=false` (the BURY
/// contract); the non-snapping buffer polygons are the fallback highlight.
fn located_unconnected_lv_lines() -> Locatable {
    Locatable {
        kind: LocatableKind::Attribute,
        layer: "lv_lines".into(),
        filter: Some(LocatableFilter {
            field: "connected".into(),
            value: Value::Bool(false),
        }),
        fallback_layer: Some("non_snapping_lv_lines".into()),
    }
}

/// Diagnostic design layers whose non-empty presence IS the warning signal —
/// the BURY contract keeps unconnected/non-snapping features rather than
/// dropping them, so a positive layer count is the cheap "needs review" cue.
/// The layer names are ds-network's (`network/build_network.rs`).
/// The layer's own directive is NOT the coded-warning table's: `cycle` is the
/// warning code and `drafting_errors` is the layer it buries into, so only the
/// layer form is addressable. The two tables are kept apart because they
/// always were.
fn diagnostic_layers() -> [(&'static str, &'static str, Locatable); 6] {
    [
        (
            "non_snapping_lv_lines",
            "dsh_finding_non_snapping_lv_lines",
            located_unconnected_lv_lines(),
        ),
        (
            "drafting_errors",
            "dsh_finding_drafting_errors",
            located_layer("drafting_errors"),
        ),
        (
            "unconnected_customers",
            "dsh_finding_unconnected_customers",
            located_layer("unconnected_customers"),
        ),
        (
            "unconnected_lv_poles",
            "dsh_finding_unconnected_lv_poles",
            located_layer("unconnected_lv_poles"),
        ),
        (
            "unconnected_service_cables",
            "dsh_finding_unconnected_service_cables",
            located_layer("unconnected_service_cables"),
        ),
        (
            "dropped_lv_poles",
            "dsh_finding_dropped_lv_poles",
            located_layer("dropped_lv_poles"),
        ),
    ]
}

/// The layers a successful LV process always persists. ds-network owns the
/// vocabulary (`network/mod.rs` writes them into `gdfs`); it exports no
/// constant for it, so it is declared here once and named to its source
/// rather than transcribed into a second client.
const PROCESSED_OUTPUT_LAYERS: [&str; 3] = ["lv_poles", "service_cables", "spans"];

/// The warning code an LV process saved output that never reached the record.
pub const PROCESSED_OUTPUT_MISSING: &str = "processed_output_missing";

/// ONE warning-code vocabulary. The first nine are the severity normaliser's;
/// the last six are the register's, which named codes the normaliser did not.
/// Label keys are i18n keys and are kept exactly as each surface spelled them.
fn code_meta(code: &str) -> Option<(&'static str, Option<Locatable>)> {
    Some(match code {
        "unconnected_lv_lines" => (
            "dsh_finding_unconnected_lv_lines",
            Some(located_unconnected_lv_lines()),
        ),
        "non_snapping_lv_lines" => (
            "dsh_finding_non_snapping_lv_lines",
            Some(located_unconnected_lv_lines()),
        ),
        "unconnected_customers" => (
            "dsh_finding_unconnected_customers",
            Some(located_layer("unconnected_customers")),
        ),
        "unconnected_lv_poles" => (
            "dsh_finding_unconnected_lv_poles",
            Some(located_layer("unconnected_lv_poles")),
        ),
        "unconnected_service_cables" => (
            "dsh_finding_unconnected_service_cables",
            Some(located_layer("unconnected_service_cables")),
        ),
        "dropped_lv_poles" => (
            "dsh_finding_dropped_lv_poles",
            Some(located_layer("dropped_lv_poles")),
        ),
        "lost_nodes" => ("dsh_finding_lost_nodes", None),
        "cycle" => ("dsh_finding_lv_line_loop", None),
        "lv_line_loop" => ("dsh_finding_lv_line_loop", None),
        "missing_geometry_dropped" => ("dlbl_warn_missing_geometry_dropped", None),
        "invalid_geometry_dropped" => ("dlbl_warn_invalid_geometry_dropped", None),
        "self_loop_dropped" => ("dlbl_warn_self_loop_dropped", None),
        "survey_form_export" => ("dlbl_warn_survey_form_export", None),
        "report_export_partial" => ("dlbl_warn_report_export_partial", None),
        "input_warning" => ("dlbl_warn_input_warning", None),
        PROCESSED_OUTPUT_MISSING => ("dsh_finding_processed_output_missing", None),
        _ => return None,
    })
}

// ── the finding ─────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct Finding {
    pub phase: Phase,
    pub severity: FindingSeverity,
    /// Backend warning code or diagnostic layer key. Empty = generic.
    pub code: String,
    /// i18n key for the short label, resolved by the rendering surface.
    pub label_key: String,
    /// The backend's own detail, bounded and sanitised. May be empty.
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<Number>,
    /// How many features the register reports as affected. Present only where
    /// the phase warning carries the arithmetic for it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub affected: Option<Number>,
    /// The layer the backend named on the warning block, when it named one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    /// The first `DETAIL_PREVIEW_MAX` `warning.details` entries, already
    /// reduced to their parts. The renderer joins them; it decides nothing.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub detail_parts: Vec<String>,
    /// How many detail entries there were in all, so a renderer can say how
    /// many it is not showing.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub detail_total: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locatable: Option<Locatable>,
    pub source: Source,
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct TransformerHealth {
    pub severity: Severity,
    pub findings: Vec<Finding>,
    /// Document-sourced error findings. This is what the badge counts.
    pub error_count: usize,
    /// Document-sourced warning findings. This is what the badge counts.
    pub warning_count: usize,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub struct HealthRow {
    pub name: String,
    #[serde(flatten)]
    pub health: TransformerHealth,
}

/// How many transformers a refresh should report, counted exactly as the
/// browser's refresh feedback has counted them: by row, per bucket, so a row
/// carrying both an error and a warning is counted in both.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Summary {
    pub total: usize,
    pub processing: usize,
    pub warnings: usize,
    pub errors: usize,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
pub struct Reply {
    pub schema: &'static str,
    pub rows: Vec<HealthRow>,
    pub summary: Summary,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    rows: Vec<Value>,
    /// Last-run responses by transformer name. Their warnings join the list
    /// as `source: "run"` and never move the severity verdict.
    #[serde(default)]
    run: Map<String, Value>,
}

// ── reading JSON the way the browser read it ────────────────────────────

fn object(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

/// `typeof value === 'string' ? value.trim() : ''`
fn string_value(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(|text| text.trim().to_string())
        .unwrap_or_default()
}

/// `typeof v === 'number' && Number.isFinite(v) ? v : undefined` — a numeric
/// STRING is not a number in JavaScript and is not one here either.
fn finite_number(value: Option<&Value>) -> Option<Number> {
    let value = value?;
    if !value.is_number() {
        return None;
    }
    let number = value.as_f64()?;
    if !number.is_finite() {
        return None;
    }
    js_number(number)
}

/// JavaScript prints an integral double without a decimal point, and the
/// corpus is compared as JSON, so an integral count must not become `3.0`.
fn js_number(value: f64) -> Option<Number> {
    if !value.is_finite() {
        return None;
    }
    if value.fract() == 0.0 && value.abs() <= 9_007_199_254_740_992.0 {
        return Some(Number::from(value as i64));
    }
    Number::from_f64(value)
}

/// `Number(value)` for the JSON subset: `null` is 0, a boolean is 0 or 1, a
/// blank string is 0, and anything unparseable is NaN.
fn js_to_number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => {
            if *flag {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return 0.0;
            }
            match trimmed {
                "Infinity" | "+Infinity" => f64::INFINITY,
                "-Infinity" => f64::NEG_INFINITY,
                _ => trimmed.parse::<f64>().unwrap_or(f64::NAN),
            }
        }
        Some(Value::Array(items)) => match items.len() {
            0 => 0.0,
            1 => js_to_number(items.first()),
            _ => f64::NAN,
        },
        Some(Value::Object(_)) => f64::NAN,
    }
}

/// `String(value)` for the JSON subset.
fn js_string(value: Option<&Value>) -> String {
    match value {
        None => "undefined".into(),
        Some(Value::Null) => "null".into(),
        Some(Value::Bool(flag)) => flag.to_string(),
        Some(Value::Number(number)) => number
            .as_f64()
            .and_then(js_number)
            .map(|value| value.to_string())
            .unwrap_or_else(|| number.to_string()),
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => js_string(Some(other)),
            })
            .collect::<Vec<_>>()
            .join(","),
        Some(Value::Object(_)) => "[object Object]".into(),
    }
}

/// `stringValue(a) || stringValue(b) || stringValue(c)` — each candidate is
/// TRIMMED before it is tested, so a whitespace-only value falls through.
fn first_non_empty(candidates: [Option<&Value>; 3]) -> String {
    for candidate in candidates {
        let text = string_value(candidate);
        if !text.is_empty() {
            return text;
        }
    }
    String::new()
}

/// `(a || b || '').trim()` — the raw value decides truthiness and only the
/// winner is trimmed, so a whitespace-only `error_message` wins and then
/// trims to nothing. The phase blobs are read this way; the outcome blobs are
/// read the other way. Both are ported as they were written.
fn first_truthy_string(candidates: [Option<&Value>; 2]) -> String {
    for candidate in candidates {
        if let Some(text) = candidate.and_then(Value::as_str) {
            if !text.is_empty() {
                return text.trim().to_string();
            }
        }
    }
    String::new()
}

// ── message text ────────────────────────────────────────────────────────

fn credential_patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            // A bearer credential, with or without the scheme word attached.
            r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]+",
            // Any absolute location: it is a link an operator cannot act on
            // and it is where signed URLs and their signatures travel.
            r"(?i)\b[a-z][a-z0-9+.-]*://\S+",
            // Google OAuth access tokens.
            r"\bya29\.[A-Za-z0-9._~+/=-]+",
            // A JWT, wherever it leaked from.
            r"\beyJ[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]{4,}\.[A-Za-z0-9_-]+",
            // `key=…` / `token: …` and their neighbours.
            r"(?i)\b(?:api[_-]?key|access[_-]?token|refresh[_-]?token|token|secret|password|signature)\s*[:=]\s*\S+",
        ]
        .iter()
        .map(|pattern| Regex::new(pattern).expect("credential pattern compiles"))
        .collect()
    })
}

fn jargon_rewrites() -> &'static [(Regex, &'static str)] {
    static REWRITES: OnceLock<Vec<(Regex, &'static str)>> = OnceLock::new();
    REWRITES.get_or_init(|| {
        // The multi-word phrase first, so the acronym rules cannot fragment it.
        [
            (r"(?i)\bGoogle Cloud Storage\b", "file storage"),
            (r"(?i)\bBigQuery\b", "the project database"),
            (r"(?i)\bFirestore\b", "the project database"),
            (r"(?i)\bIndexedDB\b", "this browser's local storage"),
            (r"(?i)\bGCS\b", "file storage"),
            (r"(?i)\bBQ\b", "the project database"),
            (r"(?i)\bWASM\b", "the processing engine"),
        ]
        .iter()
        .map(|(pattern, replacement)| {
            (
                Regex::new(pattern).expect("rewrite pattern compiles"),
                *replacement,
            )
        })
        .collect()
    })
}

fn article_duplicate() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r"(?i)\b(the) the\b").expect("article pattern compiles"))
}

fn double_space() -> &'static Regex {
    static PATTERN: OnceLock<Regex> = OnceLock::new();
    PATTERN.get_or_init(|| Regex::new(r" {2,}").expect("space pattern compiles"))
}

/// The backend's own words, made safe to show and safe to log.
///
/// Bounded first, so a very long body is never scanned at all; then every
/// credential- or location-shaped run is replaced; then the platform
/// vocabulary an operator cannot act on is rewritten into product language,
/// exactly as the browser's display sink rewrote it.
pub fn sanitize_message(message: &str) -> String {
    let bounded: String = message.chars().take(MESSAGE_MAX_CHARS).collect();
    let mut out = bounded.trim().to_string();
    if out.is_empty() {
        return out;
    }
    for pattern in credential_patterns() {
        out = pattern.replace_all(&out, REDACTED).into_owned();
    }
    for (pattern, replacement) in jargon_rewrites() {
        out = pattern.replace_all(&out, *replacement).into_owned();
    }
    out = article_duplicate().replace_all(&out, "$1").into_owned();
    out = double_space().replace_all(&out, " ").into_owned();
    out.trim().to_string()
}

// ── phase folding ───────────────────────────────────────────────────────

const PHASE_SLOTS: [(Phase, &str); 5] = [
    (Phase::Sketch, "sketch_metadata"),
    (Phase::Draft, "draft_metadata"),
    (Phase::Process, "process_metadata"),
    (Phase::Report, "report_metadata"),
    (Phase::Combined, "combined_inclusion_metadata"),
];

fn coded_finding(
    phase: Phase,
    severity: FindingSeverity,
    code: &str,
    message: String,
    count: Option<Number>,
    source: Source,
) -> Finding {
    let (label_key, locatable) = match code_meta(code) {
        Some((key, locatable)) => (key.to_string(), locatable),
        None => (generic_label(severity).to_string(), None),
    };
    Finding {
        phase,
        severity,
        code: code.to_string(),
        label_key,
        message,
        count,
        affected: None,
        layer: None,
        detail_parts: Vec::new(),
        detail_total: 0,
        locatable,
        source,
    }
}

fn generic_label(severity: FindingSeverity) -> &'static str {
    match severity {
        FindingSeverity::Error => "dsh_phase_error",
        _ => "dsh_phase_warning",
    }
}

/// `warning.count` when the backend stated one, else how many detail entries
/// it listed.
fn warning_count(warning: Option<&Map<String, Value>>) -> Option<Number> {
    let warning = warning?;
    if let Some(count) = finite_number(warning.get("count")) {
        return Some(count);
    }
    warning
        .get("details")
        .and_then(Value::as_array)
        .map(|details| Number::from(details.len() as u64))
}

/// The register's affected-feature ladder, in its three branches: the count
/// the backend stated, else the sum of the counts its detail entries state,
/// else how many entries there are.
fn affected_count(warning: Option<&Map<String, Value>>) -> Option<Number> {
    let warning = warning?;
    if let Some(count) = finite_number(warning.get("count")) {
        return Some(count);
    }
    let details = warning.get("details")?.as_array()?;
    let counted: Vec<f64> = details
        .iter()
        .map(|entry| {
            if entry.is_object() {
                js_to_number(entry.as_object().and_then(|item| item.get("count")))
            } else {
                f64::NAN
            }
        })
        .filter(|count| count.is_finite())
        .collect();
    if counted.is_empty() {
        Some(Number::from(details.len() as u64))
    } else {
        js_number(counted.iter().sum())
    }
}

/// The bounded preview of `warning.details`, reduced to the parts the
/// register prints. Joining them is the renderer's job.
fn detail_preview(warning: Option<&Map<String, Value>>) -> (Vec<String>, usize) {
    let Some(warning) = warning else {
        return (Vec::new(), 0);
    };
    let Some(details) = warning.get("details") else {
        return (Vec::new(), 0);
    };
    if let Some(entries) = details.as_array() {
        let parts = entries
            .iter()
            .take(DETAIL_PREVIEW_MAX)
            .map(|entry| match entry {
                Value::Object(item) => {
                    let joined = ["layer", "count", "code"]
                        .iter()
                        .filter_map(|key| item.get(*key))
                        .filter(|part| !part.is_null() && part.as_str() != Some(""))
                        .map(|part| js_string(Some(part)))
                        .collect::<Vec<_>>()
                        .join(" / ");
                    if joined.is_empty() {
                        match item.get("message") {
                            Some(Value::Null) | None => String::new(),
                            other => js_string(other),
                        }
                    } else {
                        joined
                    }
                }
                other => js_string(Some(other)),
            })
            .collect::<Vec<_>>();
        return (parts, entries.len());
    }
    if details.is_null() {
        return (Vec::new(), 0);
    }
    let single: String = js_string(Some(details))
        .chars()
        .take(DETAIL_STRING_MAX_CHARS)
        .collect();
    (vec![single], 1)
}

fn phase_finding(phase: Phase, meta: Option<&Map<String, Value>>) -> Option<Finding> {
    let meta = meta?;
    let status = meta.get("status").and_then(Value::as_str).unwrap_or("");
    if status.is_empty() {
        return None;
    }
    match status {
        "error" => Some(Finding {
            phase,
            severity: FindingSeverity::Error,
            code: String::new(),
            label_key: "dsh_phase_error".into(),
            message: sanitize_message(&first_truthy_string([
                meta.get("error_message"),
                meta.get("error")
                    .and_then(object)
                    .and_then(|e| e.get("message")),
            ])),
            count: None,
            affected: None,
            layer: None,
            detail_parts: Vec::new(),
            detail_total: 0,
            locatable: None,
            source: Source::Document,
        }),
        "warning" => {
            let warning = meta.get("warning").and_then(object);
            let code = string_value(warning.and_then(|w| w.get("code")));
            let (label_key, locatable) = match code_meta(&code) {
                Some((key, locatable)) => (key.to_string(), locatable),
                None => ("dsh_phase_warning".to_string(), None),
            };
            let (detail_parts, detail_total) = detail_preview(warning);
            Some(Finding {
                phase,
                severity: FindingSeverity::Warning,
                label_key,
                code,
                message: sanitize_message(&first_truthy_string([
                    warning.and_then(|w| w.get("message")),
                    meta.get("error_message"),
                ])),
                count: warning_count(warning),
                affected: affected_count(warning),
                layer: Some(string_value(warning.and_then(|w| w.get("layer"))))
                    .filter(|layer| !layer.is_empty()),
                detail_parts,
                detail_total,
                locatable,
                source: Source::Document,
            })
        }
        "processing" => Some(Finding {
            phase,
            severity: FindingSeverity::Processing,
            code: String::new(),
            label_key: "dsh_phase_processing".into(),
            message: sanitize_message(&first_truthy_string([
                meta.get("progress")
                    .and_then(object)
                    .and_then(|p| p.get("message")),
                None,
            ])),
            count: None,
            affected: None,
            layer: None,
            detail_parts: Vec::new(),
            detail_total: 0,
            locatable: None,
            source: Source::Document,
        }),
        // A background task that started but never landed a terminal stamp —
        // ds-brain flips it to stale_processing after 20 min. Actionable:
        // re-run.
        "stale_processing" => Some(Finding {
            phase,
            severity: FindingSeverity::Warning,
            code: "stuck_processing".into(),
            label_key: "dsh_phase_stuck".into(),
            message: String::new(),
            count: None,
            affected: None,
            layer: None,
            detail_parts: Vec::new(),
            detail_total: 0,
            locatable: None,
            source: Source::Document,
        }),
        // `stale` deliberately yields nothing: a report going stale after a
        // save is routine, not a problem. The supersession pass rewrites a
        // legacy phase to `stale` before this is read, so a superseded legacy
        // error yields no finding either — the campaign's next slice makes
        // that supersession explicit and gives the retained error a finding.
        _ => None,
    }
}

/// Some phases carry the actionable detail inside `outcomes` sub-objects
/// (report export legs, survey-form export) rather than the top-level
/// `warning` block — a sub-leg can fail or warn while the phase status still
/// reads success. Only coded outcomes are emitted; they can be deduped and
/// located.
fn outcome_findings(phase: Phase, meta: Option<&Map<String, Value>>) -> Vec<Finding> {
    let Some(outcomes) = meta.and_then(|meta| meta.get("outcomes")).and_then(object) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, raw) in outcomes {
        let Some(raw) = raw.as_object() else { continue };
        let status = string_value(raw.get("status")).to_lowercase();
        let message = sanitize_message(&outcome_message(raw));
        let count = finite_number(raw.get("count"));
        if status == "error" {
            out.push(coded_finding(
                phase,
                FindingSeverity::Error,
                key,
                message,
                count,
                Source::Document,
            ));
        } else if status == "warning" {
            out.push(coded_finding(
                phase,
                FindingSeverity::Warning,
                key,
                message,
                count,
                Source::Document,
            ));
        }
        if let Some(nested) = raw.get("warnings").and_then(Value::as_array) {
            for entry in nested {
                let Some(entry) = entry.as_object() else {
                    continue;
                };
                let code = string_value(entry.get("code"));
                if code.is_empty() {
                    continue;
                }
                let severity = if string_value(entry.get("severity")).to_lowercase() == "error" {
                    FindingSeverity::Error
                } else {
                    FindingSeverity::Warning
                };
                out.push(coded_finding(
                    phase,
                    severity,
                    &code,
                    sanitize_message(&string_value(entry.get("message"))),
                    finite_number(entry.get("count")),
                    Source::Document,
                ));
            }
        }
    }
    out
}

fn first_nested_outcome_message(value: Option<&Value>) -> String {
    let Some(record) = value.and_then(object) else {
        return String::new();
    };
    first_non_empty([
        record.get("technical_detail"),
        record.get("error_message"),
        record.get("message"),
    ])
}

fn outcome_message(raw: &Map<String, Value>) -> String {
    let direct = first_non_empty([
        raw.get("technical_detail"),
        raw.get("error_message"),
        raw.get("message"),
    ]);
    if !direct.is_empty() {
        return direct;
    }
    if let Some(errors) = raw.get("errors").and_then(Value::as_array) {
        for entry in errors {
            let message = first_nested_outcome_message(Some(entry));
            if !message.is_empty() {
                return message;
            }
        }
    }
    for key in ["media", "survey_export", "design_layer_export"] {
        let message = first_nested_outcome_message(raw.get(key));
        if !message.is_empty() {
            return message;
        }
    }
    String::new()
}

/// `typeof v === 'number' ? v : Array.isArray(v) ? v.length : 0`
fn layer_count(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(number)) => number
            .as_f64()
            .filter(|count| count.is_finite())
            .unwrap_or(0.0),
        Some(Value::Array(items)) => items.len() as f64,
        _ => 0.0,
    }
}

/// The green Process badge lies when the run succeeded but none of the layers
/// an LV process always persists reached the saved record. An older document
/// with no layer counts at all is unknown, not wrong — it stays silent.
fn processed_output_missing(row: &Map<String, Value>) -> bool {
    // An aggregate/synthetic row is not an LV transformer and never persists
    // these layers; the vocabulary is the crate's one copy.
    if crate::project_control::is_special_status_row(&string_value(row.get("name"))) {
        return false;
    }
    let succeeded = row
        .get("process_metadata")
        .and_then(object)
        .and_then(|meta| meta.get("status"))
        .and_then(Value::as_str)
        == Some("success");
    if !succeeded {
        return false;
    }
    let Some(layers) = row.get("layers").and_then(object) else {
        return false;
    };
    if layers.is_empty() {
        return false;
    }
    !PROCESSED_OUTPUT_LAYERS
        .iter()
        .any(|layer| layers.contains_key(*layer))
}

// ── the verdict ─────────────────────────────────────────────────────────

/// Normalise one status row into a single health verdict.
///
/// Findings are ordered error → warning → processing, phase-slot order within
/// a severity, and the diagnostic-layer findings come after the phase findings
/// (deduped by code, so a phase warning that already named a layer is not
/// counted twice).
pub fn transformer_health(row: &Value) -> TransformerHealth {
    transformer_health_with_run(row, None)
}

/// The same verdict, with the last run's own warnings folded in as transient
/// `source: "run"` findings. They are appended after the document's, deduped
/// against them, and left out of the severity and the counts — the badge
/// reports what the document says, never what the last response said.
pub fn transformer_health_with_run(row: &Value, run: Option<&Value>) -> TransformerHealth {
    let empty = Map::new();
    let row = row.as_object().unwrap_or(&empty);
    let mut findings: Vec<Finding> = Vec::new();
    let mut seen_codes: BTreeSet<String> = BTreeSet::new();

    for (phase, key) in PHASE_SLOTS {
        let meta = row.get(key).and_then(object);
        if let Some(finding) = phase_finding(phase, meta) {
            if !finding.code.is_empty() {
                seen_codes.insert(finding.code.clone());
            }
            findings.push(finding);
        }
        for outcome in outcome_findings(phase, meta) {
            if seen_codes.contains(&outcome.code) {
                continue;
            }
            seen_codes.insert(outcome.code.clone());
            findings.push(outcome);
        }
    }

    if processed_output_missing(row) && !seen_codes.contains(PROCESSED_OUTPUT_MISSING) {
        seen_codes.insert(PROCESSED_OUTPUT_MISSING.into());
        findings.push(coded_finding(
            Phase::Process,
            FindingSeverity::Error,
            PROCESSED_OUTPUT_MISSING,
            String::new(),
            None,
            Source::Document,
        ));
    }

    let layers = row.get("layers").and_then(object);
    for (layer, label_key, locatable) in diagnostic_layers() {
        if seen_codes.contains(layer) {
            continue;
        }
        let count = layer_count(layers.and_then(|layers| layers.get(layer)));
        if count <= 0.0 {
            continue;
        }
        seen_codes.insert(layer.to_string());
        findings.push(Finding {
            phase: Phase::Process,
            severity: FindingSeverity::Warning,
            code: layer.to_string(),
            label_key: label_key.to_string(),
            message: String::new(),
            count: js_number(count),
            affected: None,
            layer: None,
            detail_parts: Vec::new(),
            detail_total: 0,
            locatable: Some(locatable),
            source: Source::Document,
        });
    }

    // Stable, so phase-slot order survives inside a severity band.
    findings.sort_by_key(|finding| std::cmp::Reverse(finding.severity));

    let mut error_count = 0;
    let mut warning_count = 0;
    let mut severity = Severity::Ok;
    for finding in &findings {
        match finding.severity {
            FindingSeverity::Error => error_count += 1,
            FindingSeverity::Warning => warning_count += 1,
            _ => {}
        }
        let candidate = Severity::from(finding.severity);
        if candidate > severity {
            severity = candidate;
        }
    }

    for finding in run_findings(run) {
        if findings.iter().any(|held| same_warning(held, &finding)) {
            continue;
        }
        findings.push(finding);
    }

    TransformerHealth {
        severity,
        findings,
        error_count,
        warning_count,
    }
}

/// The last run's own warnings, as the process response reported them.
fn run_findings(run: Option<&Value>) -> Vec<Finding> {
    let Some(warnings) = run
        .and_then(object)
        .and_then(|response| response.get("warnings"))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    warnings
        .iter()
        .filter_map(|entry| entry.as_object())
        .map(|entry| {
            let severity = if string_value(entry.get("severity")).to_lowercase() == "error" {
                FindingSeverity::Error
            } else {
                FindingSeverity::Warning
            };
            let code = string_value(entry.get("code"));
            let mut finding = coded_finding(
                Phase::Process,
                severity,
                &code,
                sanitize_message(&string_value(entry.get("message"))),
                finite_number(entry.get("count")),
                Source::Run,
            );
            finding.layer =
                Some(string_value(entry.get("layer"))).filter(|layer| !layer.is_empty());
            finding
        })
        .collect()
}

/// The browser's info store deduped a transient warning against a permanent
/// one by exactly these parts, preferring the permanent copy.
fn same_warning(held: &Finding, candidate: &Finding) -> bool {
    held.phase == candidate.phase
        && held.severity == candidate.severity
        && held.code == candidate.code
        && held.layer == candidate.layer
        && held.message == candidate.message
        && held.count == candidate.count
}

pub fn summarize<'a>(healths: impl Iterator<Item = &'a TransformerHealth>) -> Summary {
    let mut summary = Summary::default();
    for health in healths {
        summary.total += 1;
        if health
            .findings
            .iter()
            .any(|finding| finding.severity == FindingSeverity::Processing)
        {
            summary.processing += 1;
        }
        if health.warning_count > 0 {
            summary.warnings += 1;
        }
        if health.error_count > 0 {
            summary.errors += 1;
        }
    }
    summary
}

fn build(request: Request) -> Result<Reply, String> {
    if request.schema != SCHEMA {
        return Err("Unsupported design health schema.".into());
    }
    let rows: Vec<HealthRow> = request
        .rows
        .iter()
        .map(|row| {
            let name = string_value(row.as_object().and_then(|row| row.get("name")));
            let run = request.run.get(&name);
            HealthRow {
                health: transformer_health_with_run(row, run),
                name,
            }
        })
        .collect();
    let summary = summarize(rows.iter().map(|row| &row.health));
    Ok(Reply {
        schema: SCHEMA,
        rows,
        summary,
    })
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("Design health request exceeds its bound.".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid design health request: {error}"))?;
    serde_json::to_string(&build(request)?).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn health(row: Value) -> TransformerHealth {
        transformer_health(&row)
    }

    fn codes(health: &TransformerHealth) -> Vec<&str> {
        health
            .findings
            .iter()
            .map(|finding| finding.code.as_str())
            .collect()
    }

    #[test]
    fn a_row_with_no_phase_metadata_is_ok_and_says_nothing() {
        let verdict = health(json!({"name": "tr_1"}));
        assert_eq!(verdict.severity, Severity::Ok);
        assert!(verdict.findings.is_empty());
        assert_eq!(verdict.error_count, 0);
        assert_eq!(verdict.warning_count, 0);
    }

    #[test]
    fn a_phase_error_reads_its_message_from_either_place_it_is_written() {
        let verdict = health(json!({"name": "tr_1",
            "process_metadata": {"status": "error", "error_message": "network build failed"}}));
        assert_eq!(verdict.severity, Severity::Error);
        assert_eq!(verdict.error_count, 1);
        assert_eq!(verdict.findings[0].phase, Phase::Process);
        assert_eq!(verdict.findings[0].label_key, "dsh_phase_error");
        assert_eq!(verdict.findings[0].message, "network build failed");

        let verdict = health(json!({"name": "tr_1",
            "report_metadata": {"status": "error", "error": {"message": "export crashed"}}}));
        assert_eq!(verdict.findings[0].message, "export crashed");

        // A whitespace-only `error_message` is truthy in JavaScript: it wins
        // the choice and then trims to nothing. The nested message is NOT
        // consulted, and that is the behaviour, not a bug to fix here.
        let verdict = health(json!({"name": "tr_1",
            "draft_metadata": {"status": "error", "error_message": "   ",
            "error": {"message": "never read"}}}));
        assert_eq!(verdict.findings[0].message, "");
    }

    #[test]
    fn a_coded_warning_carries_its_label_count_and_where_to_look() {
        let verdict = health(
            json!({"name": "tr_1", "process_metadata": {"status": "warning",
            "warning": {"code": "unconnected_lv_lines", "message": "3 LV lines could not connect", "count": 3}}}),
        );
        assert_eq!(verdict.severity, Severity::Warning);
        assert_eq!(verdict.warning_count, 1);
        let finding = &verdict.findings[0];
        assert_eq!(finding.code, "unconnected_lv_lines");
        assert_eq!(finding.label_key, "dsh_finding_unconnected_lv_lines");
        assert_eq!(finding.count, Some(Number::from(3)));
        assert_eq!(finding.affected, Some(Number::from(3)));
        assert_eq!(
            finding.locatable,
            Some(located_unconnected_lv_lines()),
            "the offending lines are addressable"
        );
        // An integral count is written the way JavaScript writes it.
        assert!(serde_json::to_string(finding)
            .unwrap()
            .contains("\"count\":3"));
    }

    #[test]
    fn a_warning_with_no_count_is_counted_from_its_details() {
        let verdict = health(
            json!({"name": "tr_1", "process_metadata": {"status": "warning",
            "warning": {"code": "lost_nodes", "message": "nodes lost", "details": ["n1", "n2", "n3", "n4"]}}}),
        );
        assert_eq!(verdict.findings[0].count, Some(Number::from(4)));
        assert_eq!(verdict.findings[0].affected, Some(Number::from(4)));
        assert_eq!(verdict.findings[0].label_key, "dsh_finding_lost_nodes");
    }

    #[test]
    fn the_affected_ladder_sums_the_counts_the_details_state() {
        // Branch two: no top-level count, but the entries carry their own —
        // including a numeric STRING, which `Number()` reads and `typeof` does
        // not. `count` still reports how many entries there are.
        let verdict = health(
            json!({"name": "tr_1", "report_metadata": {"status": "warning",
            "warning": {"code": "report_export_partial", "details": [
                {"layer": "customers", "count": 2, "code": "dropped"},
                {"layer": "spans", "count": "3"}]}}}),
        );
        let finding = &verdict.findings[0];
        assert_eq!(finding.count, Some(Number::from(2)));
        assert_eq!(finding.affected, Some(Number::from(5)));
        assert_eq!(
            finding.detail_parts,
            vec!["customers / 2 / dropped", "spans / 3"]
        );
        assert_eq!(finding.detail_total, 2);
        assert_eq!(finding.label_key, "dlbl_warn_report_export_partial");
    }

    #[test]
    fn a_details_preview_is_bounded_and_a_non_list_detail_is_one_string() {
        let entries: Vec<Value> = (0..14)
            .map(|index| json!({"layer": format!("l{index}")}))
            .collect();
        let verdict = health(
            json!({"name": "tr_1", "process_metadata": {"status": "warning",
            "warning": {"code": "lost_nodes", "details": entries}}}),
        );
        assert_eq!(verdict.findings[0].detail_parts.len(), DETAIL_PREVIEW_MAX);
        assert_eq!(verdict.findings[0].detail_total, 14);

        let verdict = health(
            json!({"name": "tr_1", "process_metadata": {"status": "warning",
            "warning": {"code": "lost_nodes", "details": "a".repeat(400)}}}),
        );
        assert_eq!(
            verdict.findings[0].detail_parts[0].chars().count(),
            DETAIL_STRING_MAX_CHARS
        );
        assert_eq!(verdict.findings[0].detail_total, 1);
        // No list, so nothing to count: the ladder reports nothing rather than
        // inventing a number.
        assert_eq!(verdict.findings[0].affected, None);
    }

    #[test]
    fn a_positive_diagnostic_layer_is_the_warning_and_is_never_counted_twice() {
        let verdict = health(
            json!({"name": "tr_1", "process_metadata": {"status": "success"},
            "layers": {"lv_lines": 40, "non_snapping_lv_lines": 5, "lv_poles": 12}}),
        );
        assert_eq!(verdict.severity, Severity::Warning);
        let finding = verdict
            .findings
            .iter()
            .find(|finding| finding.code == "non_snapping_lv_lines")
            .expect("the buried lines are reported");
        assert_eq!(finding.count, Some(Number::from(5)));
        assert_eq!(finding.locatable.as_ref().unwrap().layer, "lv_lines");

        // Supplied as a feature array rather than a count.
        let verdict =
            health(json!({"name": "tr_1", "layers": {"unconnected_customers": [{}, {}, {}]}}));
        assert_eq!(verdict.findings[0].count, Some(Number::from(3)));

        // The phase warning already named it, so the layer does not repeat it.
        let verdict = health(
            json!({"name": "tr_1", "process_metadata": {"status": "warning",
            "warning": {"code": "non_snapping_lv_lines", "message": "buried", "count": 2}},
            "layers": {"non_snapping_lv_lines": 2}}),
        );
        assert_eq!(codes(&verdict), vec!["non_snapping_lv_lines"]);
        assert_eq!(verdict.findings[0].message, "buried");
    }

    #[test]
    fn every_diagnostic_layer_has_a_label_and_a_place_to_look() {
        for (layer, label_key, locatable) in diagnostic_layers() {
            let verdict = health(json!({"name": "tr_1", "layers": {layer: 1}}));
            assert_eq!(codes(&verdict), vec![layer]);
            assert_eq!(verdict.findings[0].label_key, label_key);
            assert!(verdict.findings[0].locatable.is_some(), "{layer}");
        }
    }

    #[test]
    fn a_run_that_never_landed_is_stuck_and_one_in_flight_is_not_a_problem() {
        let verdict =
            health(json!({"name": "tr_1", "process_metadata": {"status": "stale_processing"}}));
        assert_eq!(verdict.severity, Severity::Warning);
        assert_eq!(verdict.findings[0].code, "stuck_processing");
        assert_eq!(verdict.findings[0].label_key, "dsh_phase_stuck");

        let verdict = health(
            json!({"name": "tr_1", "process_metadata": {"status": "processing",
            "progress": {"message": "building network"}}}),
        );
        assert_eq!(verdict.severity, Severity::Processing);
        assert_eq!(verdict.error_count + verdict.warning_count, 0);
        assert_eq!(verdict.findings[0].message, "building network");
    }

    #[test]
    fn a_routine_post_save_stale_report_says_nothing_at_all() {
        let verdict = health(
            json!({"name": "tr_1", "report_metadata": {"status": "stale"},
            "combined_inclusion_metadata": {"status": "stale"}}),
        );
        assert_eq!(verdict.severity, Severity::Ok);
        assert!(verdict.findings.is_empty());
        // And so does a legacy phase the supersession pass rewrote to `stale`.
        let verdict = health(json!({"name": "tr_1", "draft_metadata": {"status": "stale",
            "superseded_by": "process", "superseded_error_message": "old failure"}}));
        assert!(verdict.findings.is_empty());
    }

    #[test]
    fn an_outcome_sub_leg_is_surfaced_even_when_its_phase_reads_success() {
        let verdict = health(
            json!({"name": "tr_1", "report_metadata": {"status": "success",
            "outcomes": {"report_export_partial": {"status": "warning", "message": "partial export", "count": 2}}}}),
        );
        assert_eq!(verdict.severity, Severity::Warning);
        assert_eq!(verdict.findings[0].code, "report_export_partial");
        assert_eq!(verdict.findings[0].message, "partial export");
        assert_eq!(verdict.findings[0].count, Some(Number::from(2)));

        let verdict = health(
            json!({"name": "tr_1", "report_metadata": {"status": "success",
            "outcomes": {"survey_form_export": {"status": "error", "error_message": "form export failed"}}}}),
        );
        assert_eq!(verdict.severity, Severity::Error);
        assert_eq!(verdict.findings[0].severity, FindingSeverity::Error);
        assert_eq!(verdict.findings[0].message, "form export failed");

        // A nested warning list under a sub-leg that itself reads success.
        let verdict = health(
            json!({"name": "tr_1", "report_metadata": {"status": "success",
            "outcomes": {"media": {"status": "success",
                "warnings": [{"code": "missing_geometry_dropped", "message": "2 rows", "count": 2},
                             {"severity": "error", "code": "self_loop_dropped"},
                             {"message": "no code, not reported"}]}}}}),
        );
        assert_eq!(
            codes(&verdict),
            vec!["self_loop_dropped", "missing_geometry_dropped"]
        );
        assert_eq!(
            verdict.findings[1].label_key,
            "dlbl_warn_missing_geometry_dropped"
        );

        // A sub-leg with nothing of its own reaches into the legs beneath it.
        let verdict = health(
            json!({"name": "tr_1", "report_metadata": {"status": "success",
            "outcomes": {"export": {"status": "error",
                "errors": [{}, {"technical_detail": "row 4 has no geometry"}]}}}}),
        );
        assert_eq!(verdict.findings[0].message, "row 4 has no geometry");
    }

    #[test]
    fn findings_are_ordered_error_first_and_the_worst_one_is_the_verdict() {
        let verdict = health(json!({"name": "tr_1",
            "process_metadata": {"status": "warning", "warning": {"code": "lost_nodes", "message": "w"}},
            "report_metadata": {"status": "error", "error_message": "boom"},
            "layers": {"drafting_errors": 1}}));
        assert_eq!(verdict.severity, Severity::Error);
        assert_eq!(verdict.findings[0].severity, FindingSeverity::Error);
        assert_eq!(verdict.error_count, 1);
        assert_eq!(verdict.warning_count, 2);
        // Within a severity the phase-slot order stands: the process warning
        // was read before the diagnostic layer.
        assert_eq!(codes(&verdict), vec!["", "lost_nodes", "drafting_errors"]);
    }

    #[test]
    fn a_green_process_badge_that_saved_no_output_is_an_error_and_an_empty_map_is_silence() {
        let verdict = health(
            json!({"name": "tr_1", "process_metadata": {"status": "success"},
            "layers": {"customers": 10, "lv_lines": 4}}),
        );
        assert_eq!(verdict.severity, Severity::Error);
        assert_eq!(codes(&verdict), vec![PROCESSED_OUTPUT_MISSING]);
        assert_eq!(verdict.findings[0].phase, Phase::Process);
        assert!(verdict.findings[0].locatable.is_none());

        for layer in PROCESSED_OUTPUT_LAYERS {
            let verdict = health(
                json!({"name": "tr_1", "process_metadata": {"status": "success"},
                "layers": {layer: 1}}),
            );
            assert!(verdict.findings.is_empty(), "{layer} is output");
        }
        // Older documents carry no layer counts: unknown, so no alarm.
        for row in [
            json!({"name": "tr_1", "process_metadata": {"status": "success"}}),
            json!({"name": "tr_1", "process_metadata": {"status": "success"}, "layers": {}}),
        ] {
            assert!(health(row).findings.is_empty());
        }
        // An aggregate row is not an LV transformer and never persists them.
        let verdict = health(
            json!({"name": "mv_data", "process_metadata": {"status": "success"},
            "layers": {"mv_lines": 10}}),
        );
        assert!(verdict.findings.is_empty());
    }

    #[test]
    fn the_backend_words_are_bounded_and_never_carry_a_credential() {
        let verdict = health(
            json!({"name": "tr_1", "process_metadata": {"status": "error",
            "error_message": "upload to https://storage.googleapis.com/b/o?X-Goog-Signature=deadbeef failed with Bearer ya29.a0AfH6SMBsecret"}}),
        );
        let message = &verdict.findings[0].message;
        assert!(!message.contains("ya29"), "{message}");
        assert!(!message.contains("googleapis"), "{message}");
        assert!(message.contains(REDACTED), "{message}");

        // Platform vocabulary an operator cannot act on becomes product words.
        let verdict = health(
            json!({"name": "tr_1", "process_metadata": {"status": "error",
            "error_message": "the Firestore write failed and IndexedDB is full"}}),
        );
        assert_eq!(
            verdict.findings[0].message,
            "the project database write failed and this browser's local storage is full"
        );

        let long = "x".repeat(MESSAGE_MAX_CHARS + 200);
        let verdict = health(
            json!({"name": "tr_1", "process_metadata": {"status": "error", "error_message": long}}),
        );
        assert_eq!(
            verdict.findings[0].message.chars().count(),
            MESSAGE_MAX_CHARS
        );
    }

    #[test]
    fn the_last_run_speaks_beside_the_document_and_never_over_it() {
        let row = json!({"name": "tr_1", "process_metadata": {"status": "warning",
            "warning": {"code": "lost_nodes", "message": "nodes lost", "count": 2}}});
        let run = json!({"warnings": [
            {"code": "lost_nodes", "message": "nodes lost", "count": 2},
            {"code": "welded_lv_lines", "message": "3 taps noded", "count": 3, "layer": "lv_lines"},
            {"code": "dropped_lv_poles", "severity": "error", "message": "1 pole dropped"}]});
        let verdict = transformer_health_with_run(&row, Some(&run));
        // The document's copy is kept and the run's duplicate is dropped.
        assert_eq!(
            codes(&verdict),
            vec!["lost_nodes", "welded_lv_lines", "dropped_lv_poles"]
        );
        assert_eq!(verdict.findings[0].source, Source::Document);
        assert_eq!(verdict.findings[1].source, Source::Run);
        assert_eq!(verdict.findings[1].layer.as_deref(), Some("lv_lines"));
        assert_eq!(verdict.findings[2].severity, FindingSeverity::Error);
        // The badge still reports what the document says.
        assert_eq!(verdict.severity, Severity::Warning);
        assert_eq!(verdict.error_count, 0);
        assert_eq!(verdict.warning_count, 1);
    }

    #[test]
    fn the_summary_counts_rows_the_way_a_refresh_reports_them() {
        let reply = evaluate(
            &serde_json::to_vec(&json!({"schema": SCHEMA, "rows": [
                {"name": "a", "process_metadata": {"status": "error", "error_message": "boom"},
                 "report_metadata": {"status": "processing"}},
                {"name": "b", "layers": {"drafting_errors": 3}},
                {"name": "c", "process_metadata": {"status": "processing"}},
                {"name": "d"}]}))
            .unwrap(),
        )
        .unwrap();
        let reply: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(reply["schema"], SCHEMA);
        // A row carrying both an error and an in-flight phase is counted in
        // both buckets, exactly as the refresh feedback has always counted it.
        assert_eq!(
            reply["summary"],
            json!({"total": 4, "processing": 2, "warnings": 1, "errors": 1})
        );
        assert_eq!(reply["rows"][0]["name"], "a");
        assert_eq!(reply["rows"][0]["severity"], "error");
        assert_eq!(reply["rows"][1]["warning_count"], 1);
    }

    #[test]
    fn the_door_is_closed_to_anything_it_did_not_declare() {
        assert!(evaluate(br#"{"schema":"other","rows":[]}"#).is_err());
        assert!(evaluate(br#"{"schema":"ds.design-health/v1","rows":[],"now_ms":1}"#).is_err());
        assert!(evaluate(br#"{"schema":"ds.design-health/v1"}"#).is_err());
        assert!(evaluate(&vec![b' '; MAX_INPUT_BYTES + 1]).is_err());
        // A row that is not an object is a row with nothing to say, not a
        // decode failure: ds-brain has been known to send `null` entries.
        let reply = evaluate(br#"{"schema":"ds.design-health/v1","rows":[null,3,"x"]}"#).unwrap();
        let reply: Value = serde_json::from_str(&reply).unwrap();
        assert_eq!(reply["summary"]["total"], 3);
        assert_eq!(reply["rows"][0]["name"], "");
    }
}
