//! Application settings — one owner for the effective settings record a host
//! keeps for its operator, and for the things that decide it.
//!
//! The browser persists one JSON blob (`application_settings:v1`) in
//! IndexedDB and holds the effective record in a Svelte store. Everything
//! between the two — which keys exist, what each default is, how a stored
//! value is read, clipped or dropped, which first-launch defaults follow the
//! device, what a reset lands on — was decided in TypeScript at import time
//! (`stores/applicationSettings.ts`). It is decided here now. The host reads
//! the blob, asks, writes the answer.
//!
//! The rules are transcribed, bound for bound and default for default, from
//! ds-web's `DEFAULT_APPLICATION_SETTINGS`, every `normalize*` helper,
//! `normalizeSettings`, `loadApplicationSettings` (the device-aware first
//! launch), `resetApplicationSettings`, `normalizeBrowserBatchConcurrency`
//! (`batch/browser-bulk.ts`), `mapPerformanceProfileForBudget` and
//! `workingAreaUpdateIntervalLabel`. The effective record is the SAME JSON
//! the store already persisted — same keys, same units, same order — so an
//! existing profile reopens unchanged; that is the parity baseline. A saved
//! blob is user data: an unknown key is dropped and an unreadable value takes
//! its default, never a refusal. A request the host built wrong is an `Err`.
//!
//! There is one way to process a transformer (owner ruling 2026-09-22). The
//! record once carried a processing-lane preference (`processingLane`,
//! `standardLaneConfirmedAt`, `fastLaneConfirmedAt`) and the ops that read and
//! wrote it; a saved blob that still carries those keys is read tolerantly and
//! they are dropped on normalise, never written again. That is why the schema
//! is `v2`: the record has fewer keys and the request has fewer ops.
//!
//! Pure: the device class and the browser estimate are inputs, so the same
//! request always has the same answer.
//! `tests/fixtures/application-settings-cases.json` is the named proof,
//! replayed by the kernel and by ds-web over the vendored WASM.

use serde::Deserialize;
use serde_json::{json, Map, Value};

pub const SCHEMA: &str = "ds.application-settings/v2";
pub const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;

// ── Bounds and defaults, transcribed ────────────────────────────────────

pub const MIN_MAP_MEMORY_BUDGET: f64 = 1.0;
pub const MAX_MAP_MEMORY_BUDGET: f64 = 5.0;
pub const DEFAULT_MAP_MEMORY_BUDGET: f64 = 3.0;
pub const MIN_WORKING_AREA_UPDATE_INTERVAL: f64 = 15.0;
pub const MAX_WORKING_AREA_UPDATE_INTERVAL: f64 = 1440.0;
pub const MIN_MAP_INTERACTION_ZOOM: f64 = 0.0;
pub const MAX_MAP_INTERACTION_ZOOM: f64 = 22.0;
pub const DEFAULT_FEATURE_HOVER_POPUP_MIN_ZOOM: f64 = 14.0;
pub const DEFAULT_BOX_ZOOM_HIDE_MIN_ZOOM: f64 = 15.0;
pub const MIN_GPS_POSITION_UPDATE_INTERVAL_SECONDS: f64 = 1.0;
pub const MAX_GPS_POSITION_UPDATE_INTERVAL_SECONDS: f64 = 300.0;
pub const DEFAULT_GPS_POSITION_UPDATE_INTERVAL_SECONDS: f64 = 1.0;
pub const MIN_COORDINATE_GRID_INTERVAL_M: f64 = 0.1;
pub const MAX_COORDINATE_GRID_INTERVAL_M: f64 = 100_000.0;
pub const DEFAULT_BROWSER_BATCH_CONCURRENCY: f64 = 10.0;
pub const MIN_BROWSER_BATCH_CONCURRENCY: f64 = 1.0;
pub const MAX_BROWSER_BATCH_CONCURRENCY: f64 = 100.0;
/// `hiddenProfileAlignmentIds` keeps at most this many ids.
pub const MAX_HIDDEN_PROFILE_ALIGNMENT_IDS: usize = 500;

pub const FEATURE_HOVER_POPUP_PLACEMENTS: [&str; 5] = [
    "cursor",
    "top-right",
    "top-left",
    "bottom-right",
    "bottom-left",
];
pub const COORDINATE_GRID_LABEL_PLACEMENTS: [&str; 5] = [
    "top-left",
    "top-right",
    "bottom-left",
    "bottom-right",
    "none",
];
pub const POPUP_MODES: [&str; 2] = ["minimal", "normal"];
pub const PROFILE_GROUND_FILL_MODES: [&str; 3] = ["none", "to_floor", "to_clearance"];

const DEFAULT_FEATURE_HOVER_POPUP_PLACEMENT: &str = "bottom-left";
const DEFAULT_POPUP_MODE: &str = "normal";
const DEFAULT_PROFILE_VERTICAL_EXAGGERATION: f64 = 10.0;
const DEFAULT_PROFILE_ALIGNMENT_GAP_M: f64 = 100.0;
const DEFAULT_PROFILE_TERRAIN_CORRIDOR_HALF_WIDTH_M: f64 = 15.0;
const DEFAULT_PROFILE_TERRAIN_GAP_TOLERANCE_M: f64 = 100.0;
const DEFAULT_PROFILE_GROUND_CLEARANCE_OFFSET_M: f64 = 6.0;
const DEFAULT_PROFILE_LOWEST_WIRE_DROP_M: f64 = 0.0;
const DEFAULT_PROFILE_GROUND_FILL_MODE: &str = "none";
const DEFAULT_PROFILE_GROUND_COLOR: &str = "#38bdf8";
const DEFAULT_PROFILE_CLEARANCE_COLOR: &str = "#fb7185";
const DEFAULT_PROFILE_TERRAIN_POINT_COLOR: &str = "#d1d5db";
/// The two short-lived terrain-point defaults that migrate to the neutral
/// light gray.
const RETIRED_PROFILE_TERRAIN_POINT_COLORS: [&str; 2] = ["#38bdf8", "#94a3b8"];
const DEFAULT_PROFILE_TERRAIN_ORDINATE_OPACITY: f64 = 0.25;
const DEFAULT_PROFILE_GROUND_WIDTH_PX: f64 = 1.5;
const DEFAULT_PROFILE_CLEARANCE_WIDTH_PX: f64 = 1.25;
const DEFAULT_PROFILE_TERRAIN_POINT_WIDTH_PX: f64 = 2.5;
const DEFAULT_COORDINATE_GRID_LABEL_PLACEMENT: &str = "top-left";

/// The keys of the effective record, in the order the record is written.
pub const SETTING_KEYS: [&str; 43] = [
    "hideBoxZoomAtHighZoom",
    "boxZoomHideMinZoom",
    "featureHoverPopupsEnabled",
    "featureHoverPopupMinZoom",
    "featureHoverPopupPlacement",
    "popupMode",
    "profileVerticalExaggeration",
    "profileAlignmentGapM",
    "profileTerrainCorridorHalfWidthM",
    "profileTerrainGapToleranceM",
    "profileGroundClearanceOffsetM",
    "profileLowestWireDropM",
    "profileGroundFillMode",
    "showProfileGround",
    "showProfileSideProfiles",
    "showProfileClearanceLine",
    "showProfileTerrainPoints",
    "showProfileTerrainOrdinates",
    "showProfileGrid",
    "profileGroundColor",
    "profileClearanceColor",
    "profileTerrainPointColor",
    "profileTerrainOrdinateOpacity",
    "profileGroundWidthPx",
    "profileClearanceWidthPx",
    "profileTerrainPointWidthPx",
    "hiddenProfileAlignmentIds",
    "showProfileStructures",
    "showProfileWire",
    "showProfileSectionLabels",
    "showProfileSpanDistances",
    "showProfileStructureLabels",
    "showMapNavigationControls",
    "showMyLocationControl",
    "showCoordinateGrid",
    "coordinateGridIntervalM",
    "coordinateGridLabelPlacement",
    "snapToCoordinateGrid",
    "showZoomLevelReadout",
    "gpsPositionUpdateIntervalSeconds",
    "mapMemoryBudget",
    "maxParallelBatchProcessing",
    "workingAreaUpdateIntervalMinutes",
];

/// `DEFAULT_APPLICATION_SETTINGS`: the shipped record. `popupMode` and
/// `showMyLocationControl` are the desktop-safe static defaults here; the
/// device-aware first-launch values are `normalize`'s and `reset`'s answer.
pub fn default_settings() -> Map<String, Value> {
    let entries: [(&str, Value); 43] = [
        ("hideBoxZoomAtHighZoom", true.into()),
        ("boxZoomHideMinZoom", 15.into()),
        ("featureHoverPopupsEnabled", true.into()),
        ("featureHoverPopupMinZoom", 14.into()),
        (
            "featureHoverPopupPlacement",
            DEFAULT_FEATURE_HOVER_POPUP_PLACEMENT.into(),
        ),
        ("popupMode", DEFAULT_POPUP_MODE.into()),
        ("profileVerticalExaggeration", 10.into()),
        ("profileAlignmentGapM", 100.into()),
        ("profileTerrainCorridorHalfWidthM", 15.into()),
        ("profileTerrainGapToleranceM", 100.into()),
        ("profileGroundClearanceOffsetM", 6.into()),
        ("profileLowestWireDropM", 0.into()),
        (
            "profileGroundFillMode",
            DEFAULT_PROFILE_GROUND_FILL_MODE.into(),
        ),
        ("showProfileGround", true.into()),
        ("showProfileSideProfiles", false.into()),
        ("showProfileClearanceLine", true.into()),
        ("showProfileTerrainPoints", true.into()),
        ("showProfileTerrainOrdinates", false.into()),
        ("showProfileGrid", true.into()),
        ("profileGroundColor", DEFAULT_PROFILE_GROUND_COLOR.into()),
        (
            "profileClearanceColor",
            DEFAULT_PROFILE_CLEARANCE_COLOR.into(),
        ),
        (
            "profileTerrainPointColor",
            DEFAULT_PROFILE_TERRAIN_POINT_COLOR.into(),
        ),
        ("profileTerrainOrdinateOpacity", 0.25.into()),
        ("profileGroundWidthPx", 1.5.into()),
        ("profileClearanceWidthPx", 1.25.into()),
        ("profileTerrainPointWidthPx", 2.5.into()),
        ("hiddenProfileAlignmentIds", Value::Array(Vec::new())),
        ("showProfileStructures", true.into()),
        ("showProfileWire", true.into()),
        ("showProfileSectionLabels", false.into()),
        ("showProfileSpanDistances", true.into()),
        ("showProfileStructureLabels", true.into()),
        ("showMapNavigationControls", false.into()),
        ("showMyLocationControl", false.into()),
        ("showCoordinateGrid", true.into()),
        ("coordinateGridIntervalM", Value::Null),
        (
            "coordinateGridLabelPlacement",
            DEFAULT_COORDINATE_GRID_LABEL_PLACEMENT.into(),
        ),
        ("snapToCoordinateGrid", true.into()),
        ("showZoomLevelReadout", false.into()),
        ("gpsPositionUpdateIntervalSeconds", 1.into()),
        ("mapMemoryBudget", 3.into()),
        ("maxParallelBatchProcessing", 10.into()),
        ("workingAreaUpdateIntervalMinutes", Value::Null),
    ];
    entries
        .into_iter()
        .map(|(key, value)| (key.to_owned(), value))
        .collect()
}

// ── JavaScript-shaped coercion, transcribed ─────────────────────────────
//
// The store read its blob with `Number(value)`, `typeof`, `??`, `Math.round`
// and `String(value).trim()`. A saved value is whatever an earlier build
// wrote, so the reading has to be the same reading — over the JSON value
// space, where `undefined` is an absent key and `null` is `null`.

/// `String.prototype.trim`: WhiteSpace and LineTerminator, no more (U+0085
/// is not one) and no less (U+FEFF is one).
fn is_js_whitespace(c: char) -> bool {
    matches!(
        c,
        '\u{9}'
            | '\u{A}'
            | '\u{B}'
            | '\u{C}'
            | '\u{D}'
            | '\u{20}'
            | '\u{A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
}

fn js_trim(text: &str) -> &str {
    text.trim_matches(is_js_whitespace)
}

/// `Math.round`: half rounds toward +∞, whatever the sign.
fn js_round(number: f64) -> f64 {
    let floor = number.floor();
    if number - floor >= 0.5 {
        floor + 1.0
    } else {
        floor
    }
}

/// `Boolean(value)`.
fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(flag) => *flag,
        Value::Number(number) => number.as_f64().is_some_and(|n| n != 0.0),
        Value::String(text) => !text.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `value == null`: absent or `null`.
fn nullish(value: Option<&Value>) -> bool {
    matches!(value, None | Some(Value::Null))
}

/// `Number.prototype.toString()`, radix 10.
fn js_number_to_string(number: f64) -> String {
    if number.is_nan() {
        return "NaN".to_owned();
    }
    if number == 0.0 {
        return "0".to_owned();
    }
    if number.is_infinite() {
        return if number > 0.0 {
            "Infinity"
        } else {
            "-Infinity"
        }
        .to_owned();
    }
    // The shortest round-trip digits, then laid out the way the spec lays
    // them out: plain up to 21 integer digits, plain down to six leading
    // zeros, exponent form beyond either.
    let scientific = format!("{:e}", number.abs());
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("LowerExp always writes an exponent");
    let exponent: i32 = exponent
        .parse()
        .expect("LowerExp writes an integer exponent");
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = i32::try_from(digits.len()).expect("a double has fewer than 2^31 digits");
    let n = exponent + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        let (whole, fraction) = digits.split_at(n as usize);
        format!("{whole}.{fraction}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e < 0 { '-' } else { '+' };
        if k == 1 {
            format!("{digits}e{sign}{}", e.abs())
        } else {
            let (first, rest) = digits.split_at(1);
            format!("{first}.{rest}e{sign}{}", e.abs())
        }
    };
    if number < 0.0 {
        format!("-{body}")
    } else {
        body
    }
}

/// `String(value)`. Inside an array, `null` joins as nothing.
fn js_to_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_owned(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => js_number_to_string(number.as_f64().unwrap_or(f64::NAN)),
        Value::String(text) => text.clone(),
        Value::Array(items) => items
            .iter()
            .map(|item| match item {
                Value::Null => String::new(),
                other => js_to_string(other),
            })
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".to_owned(),
    }
}

/// A StrDecimalLiteral without its sign: digits, an optional fraction, an
/// optional exponent, and at least one digit before the exponent.
fn decimal_literal(body: &str) -> bool {
    let all_digits = |text: &str| !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(at) => (&body[..at], Some(&body[at + 1..])),
        None => (body, None),
    };
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (mantissa, None),
    };
    let whole_ok = whole.is_empty() || all_digits(whole);
    let fraction_ok = fraction.is_none_or(|f| f.is_empty() || all_digits(f));
    let has_digit = !whole.is_empty() || fraction.is_some_and(|f| !f.is_empty());
    let exponent_ok = exponent.is_none_or(|e| all_digits(e.strip_prefix(['+', '-']).unwrap_or(e)));
    whole_ok && fraction_ok && has_digit && exponent_ok
}

fn radix_number(digits: &str, radix: u32) -> Option<f64> {
    if digits.is_empty() {
        return None;
    }
    digits.chars().try_fold(0.0_f64, |total, c| {
        c.to_digit(radix)
            .map(|digit| total * f64::from(radix) + f64::from(digit))
    })
}

/// `Number(string)`: the StringNumericLiteral grammar, `None` is `NaN`.
fn js_string_to_number(text: &str) -> Option<f64> {
    let text = js_trim(text);
    if text.is_empty() {
        return Some(0.0);
    }
    let bytes = text.as_bytes();
    if bytes.len() > 2 && bytes[0] == b'0' {
        let radix = match bytes[1] {
            b'x' | b'X' => 16,
            b'o' | b'O' => 8,
            b'b' | b'B' => 2,
            _ => 0,
        };
        if radix != 0 {
            return radix_number(&text[2..], radix);
        }
    }
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let magnitude = if body == "Infinity" {
        f64::INFINITY
    } else if decimal_literal(body) {
        body.parse::<f64>().ok()?
    } else {
        return None;
    };
    Some(if negative { -magnitude } else { magnitude })
}

/// `Number(value)`; `None` is `NaN`.
fn js_number(value: Option<&Value>) -> Option<f64> {
    match value {
        None | Some(Value::Object(_)) => None,
        Some(Value::Null) => Some(0.0),
        Some(Value::Bool(flag)) => Some(f64::from(u8::from(*flag))),
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => js_string_to_number(text),
        Some(array @ Value::Array(_)) => js_string_to_number(&js_to_string(array)),
    }
}

/// `Number(value)` when it is finite.
fn finite(value: Option<&Value>) -> Option<f64> {
    js_number(value).filter(|number| number.is_finite())
}

/// A JSON number the way `JSON.stringify` writes it: an integral value has no
/// fraction.
fn number_value(number: f64) -> Value {
    if number.fract() == 0.0 && number.abs() < 9_007_199_254_740_992.0 {
        Value::from(number as i64)
    } else {
        Value::from(number)
    }
}

fn optional_number_value(number: Option<f64>) -> Value {
    number.map_or(Value::Null, number_value)
}

/// `value ?? fallback`.
fn coalesce<'a>(value: Option<&'a Value>, fallback: &'a Value) -> Option<&'a Value> {
    if nullish(value) {
        Some(fallback)
    } else {
        value
    }
}

/// `typeof value === 'boolean' ? value : fallback`.
fn bool_or(value: Option<&Value>, fallback: bool) -> bool {
    match value {
        Some(Value::Bool(flag)) => *flag,
        _ => fallback,
    }
}

/// `OPTIONS.includes(value) ? value : fallback`.
fn word_or<'a>(value: Option<&Value>, options: &[&'a str], fallback: &'a str) -> &'a str {
    match value.and_then(Value::as_str) {
        Some(word) => options
            .iter()
            .copied()
            .find(|option| *option == word)
            .unwrap_or(fallback),
        None => fallback,
    }
}

// ── The normalisers, one per rule ───────────────────────────────────────

/// `normalizeMapMemoryBudget`.
pub fn normalize_map_memory_budget(value: Option<&Value>) -> f64 {
    match finite(value) {
        Some(number) => js_round(number).clamp(MIN_MAP_MEMORY_BUDGET, MAX_MAP_MEMORY_BUDGET),
        None => DEFAULT_MAP_MEMORY_BUDGET,
    }
}

/// `normalizeWorkingAreaUpdateInterval`: `null` and `'never'` are never,
/// absent and `''` are the default (also never), anything else clips.
pub fn normalize_working_area_update_interval(value: Option<&Value>) -> Option<f64> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text == "never" || text.is_empty() => None,
        _ => finite(value).map(|number| {
            js_round(number).clamp(
                MIN_WORKING_AREA_UPDATE_INTERVAL,
                MAX_WORKING_AREA_UPDATE_INTERVAL,
            )
        }),
    }
}

/// `normalizeMapInteractionZoom`.
pub fn normalize_map_interaction_zoom(value: Option<&Value>, fallback: f64) -> f64 {
    match finite(value) {
        Some(number) => js_round(number).clamp(MIN_MAP_INTERACTION_ZOOM, MAX_MAP_INTERACTION_ZOOM),
        None => fallback,
    }
}

/// `normalizeGPSPositionUpdateIntervalSeconds`.
pub fn normalize_gps_position_update_interval_seconds(value: Option<&Value>) -> f64 {
    match finite(value) {
        Some(number) => js_round(number).clamp(
            MIN_GPS_POSITION_UPDATE_INTERVAL_SECONDS,
            MAX_GPS_POSITION_UPDATE_INTERVAL_SECONDS,
        ),
        None => DEFAULT_GPS_POSITION_UPDATE_INTERVAL_SECONDS,
    }
}

/// `normalizeCoordinateGridInterval`: `None` is the zoom-adaptive interval.
pub fn normalize_coordinate_grid_interval(value: Option<&Value>) -> Option<f64> {
    match value {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) if text == "auto" || text.is_empty() => None,
        _ => finite(value).filter(|number| *number > 0.0).map(|number| {
            number.clamp(
                MIN_COORDINATE_GRID_INTERVAL_M,
                MAX_COORDINATE_GRID_INTERVAL_M,
            )
        }),
    }
}

/// `normalizeProfileExaggeration`: mirrors the renderer's clamp.
fn normalize_profile_exaggeration(value: Option<&Value>) -> f64 {
    match finite(value) {
        Some(number) if number > 0.0 => number.clamp(0.5, 100.0),
        _ => DEFAULT_PROFILE_VERTICAL_EXAGGERATION,
    }
}

/// `normalizeProfileAlignmentGap`.
pub fn normalize_profile_alignment_gap(value: Option<&Value>) -> f64 {
    match finite(value) {
        Some(number) if number >= 0.0 => js_round(number).min(10_000.0),
        _ => DEFAULT_PROFILE_ALIGNMENT_GAP_M,
    }
}

/// `normalizeProfileTerrainWidth`.
pub fn normalize_profile_terrain_width(value: Option<&Value>) -> f64 {
    match finite(value) {
        Some(number) if number > 0.0 => js_round(number).clamp(1.0, 500.0),
        _ => DEFAULT_PROFILE_TERRAIN_CORRIDOR_HALF_WIDTH_M,
    }
}

fn normalize_profile_terrain_gap(value: Option<&Value>) -> f64 {
    match finite(value) {
        Some(number) if number > 0.0 => js_round(number).clamp(1.0, 5000.0),
        _ => DEFAULT_PROFILE_TERRAIN_GAP_TOLERANCE_M,
    }
}

/// A metre offset kept to a tenth, at most 100.
fn tenth_metres_or(value: Option<&Value>, fallback: f64) -> f64 {
    match finite(value) {
        Some(number) if number >= 0.0 => (js_round(number * 10.0) / 10.0).min(100.0),
        _ => fallback,
    }
}

/// `normalizeProfileColor`: a trimmed `#rrggbb`, lower-cased, or the fallback.
fn normalize_profile_color(value: Option<&Value>, fallback: &str) -> String {
    let color = value.and_then(Value::as_str).map(js_trim).unwrap_or("");
    let hex = color
        .strip_prefix('#')
        .filter(|rest| rest.len() == 6 && rest.bytes().all(|b| b.is_ascii_hexdigit()));
    match hex {
        Some(_) => color.to_ascii_lowercase(),
        None => fallback.to_owned(),
    }
}

/// `normalizeProfileTerrainColor`: the two short-lived blue/slate defaults
/// migrate to the neutral light gray.
fn normalize_profile_terrain_color(value: Option<&Value>) -> String {
    let color = normalize_profile_color(value, DEFAULT_PROFILE_TERRAIN_POINT_COLOR);
    if RETIRED_PROFILE_TERRAIN_POINT_COLORS.contains(&color.as_str()) {
        DEFAULT_PROFILE_TERRAIN_POINT_COLOR.to_owned()
    } else {
        color
    }
}

/// `normalizeProfileStrokeWidth`: quarter pixels, 0.5 to 8.
fn normalize_profile_stroke_width(value: Option<&Value>, fallback: f64) -> f64 {
    match finite(value) {
        Some(number) => (js_round(number * 4.0) / 4.0).clamp(0.5, 8.0),
        None => fallback,
    }
}

/// `normalizeProfilePointWidth`: half pixels, 1 to 24.
fn normalize_profile_point_width(value: Option<&Value>) -> f64 {
    match finite(value) {
        Some(number) => (js_round(number * 2.0) / 2.0).clamp(1.0, 24.0),
        None => DEFAULT_PROFILE_TERRAIN_POINT_WIDTH_PX,
    }
}

/// `normalizeProfileOrdinateOpacity`: hundredths, 0.05 to 1.
fn normalize_profile_ordinate_opacity(value: Option<&Value>) -> f64 {
    match finite(value) {
        Some(number) => (js_round(number * 100.0) / 100.0).clamp(0.05, 1.0),
        None => DEFAULT_PROFILE_TERRAIN_ORDINATE_OPACITY,
    }
}

/// `normalizeBrowserBatchConcurrency` (`batch/browser-bulk.ts`): a number as
/// is, a string read as one (blank is the default), anything else the
/// default; whole, 1 to 100.
pub fn normalize_browser_batch_concurrency(value: Option<&Value>) -> f64 {
    let numeric = match value {
        Some(Value::Number(number)) => number.as_f64(),
        Some(Value::String(text)) => {
            if js_trim(text).is_empty() {
                Some(DEFAULT_BROWSER_BATCH_CONCURRENCY)
            } else {
                js_string_to_number(text)
            }
        }
        _ => Some(DEFAULT_BROWSER_BATCH_CONCURRENCY),
    };
    match numeric {
        Some(number) if number.is_finite() => number
            .floor()
            .clamp(MIN_BROWSER_BATCH_CONCURRENCY, MAX_BROWSER_BATCH_CONCURRENCY),
        _ => DEFAULT_BROWSER_BATCH_CONCURRENCY,
    }
}

/// `hiddenProfileAlignmentIds`: each item as a string, trimmed, blanks out,
/// first occurrence kept, at most 500.
fn normalize_hidden_profile_alignment_ids(value: Option<&Value>) -> Vec<String> {
    let Some(Value::Array(items)) = value else {
        return Vec::new();
    };
    let mut ids: Vec<String> = Vec::new();
    for item in items {
        let id = js_trim(&js_to_string(item)).to_owned();
        if !id.is_empty() && !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids.truncate(MAX_HIDDEN_PROFILE_ALIGNMENT_IDS);
    ids
}

/// `normalizeSettings`: the effective record from any object. Unknown keys
/// are dropped, every known key is read the way the store read it.
pub fn normalize_settings(value: &Map<String, Value>) -> Map<String, Value> {
    let defaults = default_settings();
    let get = |key: &str| value.get(key);
    let fallback = |key: &str| defaults.get(key).expect("every default is written");

    let mut out = Map::new();
    let mut put = |key: &str, value: Value| {
        out.insert(key.to_owned(), value);
    };
    let flag = |key: &str| {
        bool_or(
            get(key),
            fallback(key).as_bool().expect("a boolean default"),
        )
    };

    put(
        "hideBoxZoomAtHighZoom",
        flag("hideBoxZoomAtHighZoom").into(),
    );
    put(
        "boxZoomHideMinZoom",
        number_value(normalize_map_interaction_zoom(
            coalesce(get("boxZoomHideMinZoom"), fallback("boxZoomHideMinZoom")),
            DEFAULT_BOX_ZOOM_HIDE_MIN_ZOOM,
        )),
    );
    put(
        "featureHoverPopupsEnabled",
        flag("featureHoverPopupsEnabled").into(),
    );
    put(
        "featureHoverPopupMinZoom",
        number_value(normalize_map_interaction_zoom(
            coalesce(
                get("featureHoverPopupMinZoom"),
                fallback("featureHoverPopupMinZoom"),
            ),
            DEFAULT_FEATURE_HOVER_POPUP_MIN_ZOOM,
        )),
    );
    put(
        "featureHoverPopupPlacement",
        word_or(
            get("featureHoverPopupPlacement"),
            &FEATURE_HOVER_POPUP_PLACEMENTS,
            DEFAULT_FEATURE_HOVER_POPUP_PLACEMENT,
        )
        .into(),
    );
    put(
        "popupMode",
        word_or(get("popupMode"), &POPUP_MODES, DEFAULT_POPUP_MODE).into(),
    );
    put(
        "profileVerticalExaggeration",
        number_value(normalize_profile_exaggeration(get(
            "profileVerticalExaggeration",
        ))),
    );
    put(
        "profileAlignmentGapM",
        number_value(normalize_profile_alignment_gap(get("profileAlignmentGapM"))),
    );
    put(
        "profileTerrainCorridorHalfWidthM",
        number_value(normalize_profile_terrain_width(get(
            "profileTerrainCorridorHalfWidthM",
        ))),
    );
    put(
        "profileTerrainGapToleranceM",
        number_value(normalize_profile_terrain_gap(get(
            "profileTerrainGapToleranceM",
        ))),
    );
    put(
        "profileGroundClearanceOffsetM",
        number_value(tenth_metres_or(
            get("profileGroundClearanceOffsetM"),
            DEFAULT_PROFILE_GROUND_CLEARANCE_OFFSET_M,
        )),
    );
    put(
        "profileLowestWireDropM",
        number_value(tenth_metres_or(
            get("profileLowestWireDropM"),
            DEFAULT_PROFILE_LOWEST_WIRE_DROP_M,
        )),
    );
    put(
        "profileGroundFillMode",
        word_or(
            get("profileGroundFillMode"),
            &PROFILE_GROUND_FILL_MODES,
            DEFAULT_PROFILE_GROUND_FILL_MODE,
        )
        .into(),
    );
    for key in [
        "showProfileGround",
        "showProfileSideProfiles",
        "showProfileClearanceLine",
        "showProfileTerrainPoints",
        "showProfileTerrainOrdinates",
        "showProfileGrid",
    ] {
        put(key, flag(key).into());
    }
    put(
        "profileGroundColor",
        normalize_profile_color(get("profileGroundColor"), DEFAULT_PROFILE_GROUND_COLOR).into(),
    );
    put(
        "profileClearanceColor",
        normalize_profile_color(
            get("profileClearanceColor"),
            DEFAULT_PROFILE_CLEARANCE_COLOR,
        )
        .into(),
    );
    put(
        "profileTerrainPointColor",
        normalize_profile_terrain_color(get("profileTerrainPointColor")).into(),
    );
    put(
        "profileTerrainOrdinateOpacity",
        number_value(normalize_profile_ordinate_opacity(get(
            "profileTerrainOrdinateOpacity",
        ))),
    );
    put(
        "profileGroundWidthPx",
        number_value(normalize_profile_stroke_width(
            get("profileGroundWidthPx"),
            DEFAULT_PROFILE_GROUND_WIDTH_PX,
        )),
    );
    put(
        "profileClearanceWidthPx",
        number_value(normalize_profile_stroke_width(
            get("profileClearanceWidthPx"),
            DEFAULT_PROFILE_CLEARANCE_WIDTH_PX,
        )),
    );
    // The width was once persisted as a radius; the old key is read when the
    // new one is absent.
    let point_width = if nullish(get("profileTerrainPointWidthPx")) {
        get("profileTerrainPointRadiusPx")
    } else {
        get("profileTerrainPointWidthPx")
    };
    put(
        "profileTerrainPointWidthPx",
        number_value(normalize_profile_point_width(point_width)),
    );
    put(
        "hiddenProfileAlignmentIds",
        normalize_hidden_profile_alignment_ids(get("hiddenProfileAlignmentIds")).into(),
    );
    for key in [
        "showProfileStructures",
        "showProfileWire",
        "showProfileSectionLabels",
        "showProfileSpanDistances",
        "showProfileStructureLabels",
        "showMapNavigationControls",
        "showMyLocationControl",
        "showCoordinateGrid",
    ] {
        put(key, flag(key).into());
    }
    put(
        "coordinateGridIntervalM",
        optional_number_value(normalize_coordinate_grid_interval(get(
            "coordinateGridIntervalM",
        ))),
    );
    put(
        "coordinateGridLabelPlacement",
        word_or(
            get("coordinateGridLabelPlacement"),
            &COORDINATE_GRID_LABEL_PLACEMENTS,
            DEFAULT_COORDINATE_GRID_LABEL_PLACEMENT,
        )
        .into(),
    );
    put("snapToCoordinateGrid", flag("snapToCoordinateGrid").into());
    put("showZoomLevelReadout", flag("showZoomLevelReadout").into());
    put(
        "gpsPositionUpdateIntervalSeconds",
        number_value(normalize_gps_position_update_interval_seconds(coalesce(
            get("gpsPositionUpdateIntervalSeconds"),
            fallback("gpsPositionUpdateIntervalSeconds"),
        ))),
    );
    put(
        "mapMemoryBudget",
        number_value(normalize_map_memory_budget(coalesce(
            get("mapMemoryBudget"),
            fallback("mapMemoryBudget"),
        ))),
    );
    put(
        "maxParallelBatchProcessing",
        number_value(normalize_browser_batch_concurrency(coalesce(
            get("maxParallelBatchProcessing"),
            fallback("maxParallelBatchProcessing"),
        ))),
    );
    put(
        "workingAreaUpdateIntervalMinutes",
        optional_number_value(normalize_working_area_update_interval(coalesce(
            get("workingAreaUpdateIntervalMinutes"),
            fallback("workingAreaUpdateIntervalMinutes"),
        ))),
    );
    out
}

/// `workingAreaUpdateIntervalLabel`.
pub fn working_area_update_interval_label(value: Option<&Value>) -> String {
    let Some(interval) = normalize_working_area_update_interval(value) else {
        return "Never".to_owned();
    };
    let interval = interval as i64;
    if interval < 60 {
        format!("{interval} min")
    } else if interval == 60 {
        "1 hour".to_owned()
    } else if interval % 60 == 0 {
        format!("{} hours", interval / 60)
    } else {
        let hours = interval / 60;
        let minutes = interval % 60;
        let unit = if hours == 1 { "hour" } else { "hours" };
        format!("{hours} {unit} {minutes} min")
    }
}

// ── The device ──────────────────────────────────────────────────────────

/// What the host detected about the machine: the two device-aware defaults
/// are the kernel's answer from these two facts.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Device {
    /// `detectPlatform().deviceClass === 'phone'`.
    pub phone: bool,
    /// `defaultMyLocationControlVisible()`.
    pub my_location_default: bool,
}

impl Device {
    /// `defaultPopupModeForDevice`: phones get the minimal bottom card,
    /// tablets and desktops the full popup.
    const fn popup_mode(self) -> &'static str {
        if self.phone {
            "minimal"
        } else {
            "normal"
        }
    }
}

// ── The performance profile ─────────────────────────────────────────────

/// What the host measured about its browser, each read with `Number()`.
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceHint {
    #[serde(default)]
    pub js_heap_limit_bytes: Option<Value>,
    #[serde(default)]
    pub device_memory_gb: Option<Value>,
    #[serde(default)]
    pub hardware_concurrency: Option<Value>,
}

/// `MAP_PERFORMANCE_PROFILES`, budget 1 to 5: label, description, tile cache
/// size, cleanup delay, refresh expired tiles.
const MAP_PERFORMANCE_PROFILES: [(&str, &str, f64, i64, bool); 5] = [
    (
        "Saver",
        "Small tile cache and quicker cleanup for low-memory browsers.",
        48.0,
        120,
        false,
    ),
    (
        "Low",
        "Keeps memory tight while preserving normal map navigation.",
        96.0,
        200,
        false,
    ),
    (
        "Balanced",
        "Default cache budget for everyday project editing.",
        160.0,
        350,
        true,
    ),
    (
        "Smooth",
        "Larger tile cache for smoother panning on capable machines.",
        256.0,
        650,
        true,
    ),
    (
        "Maximum",
        "Keeps more tiles resident for large screens and high-memory devices.",
        384.0,
        1000,
        true,
    ),
];

/// `finiteNumber`: `Number(value)` when finite and positive.
fn finite_positive(value: Option<&Value>) -> Option<f64> {
    finite(value).filter(|number| *number > 0.0)
}

/// `resourceScaleForHint`.
fn resource_scale(hint: &ResourceHint) -> (f64, &'static str) {
    let heap_gib = hint
        .js_heap_limit_bytes
        .as_ref()
        .filter(|raw| js_truthy(raw))
        .and_then(|raw| js_number(Some(raw)))
        .map(|bytes| bytes / (1024.0 * 1024.0 * 1024.0));
    let device_memory_gb = finite_positive(hint.device_memory_gb.as_ref());
    let cores = finite_positive(hint.hardware_concurrency.as_ref());
    let at_least = |value: Option<f64>, bound: f64| value.is_some_and(|value| value >= bound);
    let at_most = |value: Option<f64>, bound: f64| value.is_some_and(|value| value <= bound);
    if at_least(heap_gib, 6.0) || at_least(device_memory_gb, 8.0) {
        (1.5, "Large browser estimate")
    } else if at_least(heap_gib, 3.0) || at_least(device_memory_gb, 4.0) || at_least(cores, 8.0) {
        (4.0 / 3.0, "Capable browser estimate")
    } else if at_most(heap_gib, 1.5) || at_most(device_memory_gb, 2.0) {
        (0.75, "Constrained browser estimate")
    } else {
        (1.0, "Standard browser estimate")
    }
}

/// `roundTileCacheSize`: whole sixteens, at least 24.
fn round_tile_cache_size(value: f64) -> f64 {
    (js_round(value / 16.0) * 16.0).max(24.0)
}

/// `mapPerformanceProfileForBudget`.
pub fn map_performance_profile(budget: Option<&Value>, hint: &ResourceHint) -> Value {
    let budget = normalize_map_memory_budget(budget);
    let (label, description, tile_cache, cleanup_delay_ms, refresh_expired_tiles) =
        MAP_PERFORMANCE_PROFILES[(budget as usize) - 1];
    let (scale, resource_label) = resource_scale(hint);
    json!({
        "budget": number_value(budget),
        "label": label,
        "description": description,
        "maxTileCacheSize": number_value(round_tile_cache_size(tile_cache * scale)),
        "tileLodLevels": number_value(budget + 1.0),
        "tileLodRatio": number_value((round_tile_cache_size(tile_cache * scale) / 48.0).clamp(1.0, 6.0)),
        "cleanupDelayMs": cleanup_delay_ms,
        "refreshExpiredTiles": refresh_expired_tiles,
        "resourceLabel": resource_label,
        "resourceScale": number_value(scale),
    })
}

// ── The ops ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    op: Op,
}

/// What a host may ask about its operator's settings.
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Op {
    /// `loadApplicationSettings` without the I/O: the effective record from
    /// the saved blob and the device, and whether the host writes it back.
    Normalize {
        #[serde(default)]
        saved: Option<Value>,
        device: Device,
    },
    /// `updateApplicationSettings`: the record after a change. A `null` in the
    /// patch is the store's `undefined`: the key is absent from the merged
    /// record and reads as its default, whatever the key (a `null` in a saved
    /// blob, by contrast, is read as `null`).
    Patch {
        current: Map<String, Value>,
        patch: Map<String, Value>,
    },
    /// `resetApplicationSettings`: the named keys back to their shipped
    /// defaults — the device-aware ones to the device's value, exactly where a
    /// fresh install would land.
    Reset {
        current: Map<String, Value>,
        keys: Vec<String>,
        device: Device,
    },
    /// `mapPerformanceProfileForBudget`: the MapLibre cache profile for a
    /// budget, scaled by what the host measured about its browser.
    PerformanceProfile {
        budget: Value,
        #[serde(default)]
        resource_hint: Option<ResourceHint>,
    },
    /// `workingAreaUpdateIntervalLabel`.
    IntervalLabel { value: Value },
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("application settings request exceeds its bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid application settings request: {error}"))?;
    if request.schema != SCHEMA {
        return Err(format!(
            "unsupported application settings schema; expected {SCHEMA}"
        ));
    }
    let value = apply(request.op)?;
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

/// `{...current, ...patch}` as the store spread it, where the patch's `null`
/// is the store's `undefined`: JSON cannot carry `undefined`, so the host
/// sends `null`, and the key is removed rather than merged — a merged `null`
/// would be read as `Number(null)`, 0, on the keys the store did not guard
/// with `??` (a stroke width would clip to 0.5 where the store's `undefined`
/// gave the shipped 1.5).
fn merged(current: &Map<String, Value>, patch: Map<String, Value>) -> Map<String, Value> {
    let mut next = current.clone();
    for (key, value) in patch {
        if value.is_null() {
            next.remove(&key);
        } else {
            next.insert(key, value);
        }
    }
    next
}

/// The keys a saved blob may still carry from the retired processing-lane
/// preference. They are read tolerantly and dropped on normalise; a blob that
/// carries one is written back once so the retired record does not linger.
const RETIRED_KEYS: [&str; 3] = [
    "processingLane",
    "standardLaneConfirmedAt",
    "fastLaneConfirmedAt",
];

/// The answer every op that yields a record gives: the record.
fn record(settings: Map<String, Value>) -> Map<String, Value> {
    let mut answer = Map::new();
    answer.insert("settings".to_owned(), Value::Object(settings));
    answer
}

fn apply(op: Op) -> Result<Value, String> {
    match op {
        Op::Normalize { saved, device } => {
            // `{...saved}`: a blob that is not an object spreads no setting.
            let saved = match saved {
                Some(Value::Object(saved)) => saved,
                _ => Map::new(),
            };
            let carries_retired_key = RETIRED_KEYS.iter().any(|key| saved.contains_key(*key));
            let first_launch_popup_mode = nullish(saved.get("popupMode"));
            let first_launch_my_location = nullish(saved.get("showMyLocationControl"));
            let mut input = saved;
            if first_launch_popup_mode {
                input.insert("popupMode".to_owned(), device.popup_mode().into());
            }
            if first_launch_my_location {
                input.insert(
                    "showMyLocationControl".to_owned(),
                    device.my_location_default.into(),
                );
            }
            let mut answer = record(normalize_settings(&input));
            answer.insert("persist".to_owned(), carries_retired_key.into());
            answer.insert(
                "first_launch".to_owned(),
                json!({
                    "popup_mode": first_launch_popup_mode,
                    "my_location": first_launch_my_location,
                }),
            );
            Ok(Value::Object(answer))
        }
        Op::Patch { current, patch } => Ok(Value::Object(record(normalize_settings(&merged(
            &current, patch,
        ))))),
        Op::Reset {
            current,
            keys,
            device,
        } => {
            let unknown: Vec<&str> = keys
                .iter()
                .map(String::as_str)
                .filter(|key| !SETTING_KEYS.contains(key))
                .collect();
            if !unknown.is_empty() {
                return Err(format!(
                    "reset names settings that do not exist: {}",
                    unknown.join(", ")
                ));
            }
            let defaults = default_settings();
            let mut patch = Map::new();
            for key in keys {
                let value = match key.as_str() {
                    "popupMode" => device.popup_mode().into(),
                    "showMyLocationControl" => device.my_location_default.into(),
                    _ => defaults.get(&key).cloned().unwrap_or(Value::Null),
                };
                patch.insert(key, value);
            }
            Ok(Value::Object(record(normalize_settings(&merged(
                &current, patch,
            )))))
        }
        Op::PerformanceProfile {
            budget,
            resource_hint,
        } => Ok(json!({
            "profile": map_performance_profile(Some(&budget), &resource_hint.unwrap_or_default()),
        })),
        Op::IntervalLabel { value } => Ok(json!({
            "label": working_area_update_interval_label(Some(&value)),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::to_vec;

    fn ask(op: Value) -> Result<Value, String> {
        evaluate(&to_vec(&json!({"schema": SCHEMA, "op": op})).unwrap())
            .map(|answer| serde_json::from_str(&answer).unwrap())
    }

    fn settings_of(answer: &Value) -> &Map<String, Value> {
        answer["settings"].as_object().expect("a record")
    }

    // ── JavaScript coercion ─────────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn number_reads_strings_the_way_the_browser_did() {
        let read = |text: &str| js_string_to_number(text);
        assert_eq!(read("4.4"), Some(4.4));
        assert_eq!(read(" 12 "), Some(12.0));
        assert_eq!(read(""), Some(0.0));
        assert_eq!(read("   "), Some(0.0));
        assert_eq!(read("\u{FEFF}7\u{A0}"), Some(7.0));
        assert_eq!(read("bad"), None);
        assert_eq!(read("0x10"), Some(16.0));
        assert_eq!(read("0o17"), Some(15.0));
        assert_eq!(read("0b101"), Some(5.0));
        assert_eq!(read("-0x10"), None);
        assert_eq!(read("0x"), None);
        assert_eq!(read("1e3"), Some(1000.0));
        assert_eq!(read("2.5E1"), Some(25.0));
        assert_eq!(read(".5"), Some(0.5));
        assert_eq!(read("5."), Some(5.0));
        assert_eq!(read("."), None);
        assert_eq!(read("1e"), None);
        assert_eq!(read("1_000"), None);
        assert_eq!(read("Infinity"), Some(f64::INFINITY));
        assert_eq!(read("-Infinity"), Some(f64::NEG_INFINITY));
        assert_eq!(read("infinity"), None);
        assert_eq!(read("inf"), None);
        assert_eq!(read("NaN"), None);
        assert_eq!(read("+3"), Some(3.0));
        assert_eq!(read("\u{85}3"), None);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn number_reads_every_json_shape() {
        assert_eq!(js_number(None), None);
        assert_eq!(js_number(Some(&json!(null))), Some(0.0));
        assert_eq!(js_number(Some(&json!(true))), Some(1.0));
        assert_eq!(js_number(Some(&json!(false))), Some(0.0));
        assert_eq!(js_number(Some(&json!(2.5))), Some(2.5));
        assert_eq!(js_number(Some(&json!([]))), Some(0.0));
        assert_eq!(js_number(Some(&json!([7]))), Some(7.0));
        assert_eq!(js_number(Some(&json!(["8"]))), Some(8.0));
        assert_eq!(js_number(Some(&json!([null]))), Some(0.0));
        assert_eq!(js_number(Some(&json!([[9]]))), Some(9.0));
        assert_eq!(js_number(Some(&json!([1, 2]))), None);
        assert_eq!(js_number(Some(&json!([true]))), None);
        assert_eq!(js_number(Some(&json!({}))), None);
        assert_eq!(js_number(Some(&json!([{}]))), None);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn numbers_print_the_way_the_browser_prints_them() {
        assert_eq!(js_number_to_string(1.0), "1");
        assert_eq!(js_number_to_string(-2.5), "-2.5");
        assert_eq!(js_number_to_string(0.1), "0.1");
        assert_eq!(js_number_to_string(0.0), "0");
        assert_eq!(js_number_to_string(-0.0), "0");
        assert_eq!(js_number_to_string(123.456), "123.456");
        assert_eq!(js_number_to_string(1e21), "1e+21");
        assert_eq!(js_number_to_string(1e20), "100000000000000000000");
        assert_eq!(js_number_to_string(1.5e22), "1.5e+22");
        assert_eq!(js_number_to_string(0.000001), "0.000001");
        assert_eq!(js_number_to_string(1e-7), "1e-7");
        assert_eq!(js_number_to_string(1.25e-7), "1.25e-7");
        assert_eq!(js_number_to_string(f64::INFINITY), "Infinity");
        assert_eq!(js_number_to_string(f64::NAN), "NaN");
        assert_eq!(js_to_string(&json!([1, null, "a", [2, 3]])), "1,,a,2,3");
        assert_eq!(js_to_string(&json!(null)), "null");
        assert_eq!(js_to_string(&json!({"a": 1})), "[object Object]");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn round_takes_halves_toward_positive_infinity() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-2.5), -2.0);
        assert_eq!(js_round(4.4), 4.0);
        assert_eq!(js_round(0.499_999_999_999_999_94), 0.0);
        assert_eq!(js_round(-0.4), 0.0);
        assert_eq!(js_round(1e300), 1e300);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn trim_is_the_browser_trim() {
        assert_eq!(js_trim("\u{FEFF} a \u{3000}"), "a");
        assert_eq!(js_trim("\u{85}a"), "\u{85}a");
        assert_eq!(js_trim("\t\n"), "");
    }

    // ── Each bound, transcribed ─────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn map_memory_budget_clips_to_the_integer_range() {
        assert_eq!(normalize_map_memory_budget(None), 3.0);
        assert_eq!(normalize_map_memory_budget(Some(&json!("bad"))), 3.0);
        assert_eq!(normalize_map_memory_budget(Some(&json!(-9))), 1.0);
        assert_eq!(normalize_map_memory_budget(Some(&json!(15))), 5.0);
        assert_eq!(normalize_map_memory_budget(Some(&json!("4.4"))), 4.0);
        assert_eq!(normalize_map_memory_budget(Some(&json!("4.5"))), 5.0);
        assert_eq!(normalize_map_memory_budget(Some(&json!(null))), 1.0);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn gps_interval_defaults_to_one_second_and_clips() {
        assert_eq!(normalize_gps_position_update_interval_seconds(None), 1.0);
        assert_eq!(
            normalize_gps_position_update_interval_seconds(Some(&json!("bad"))),
            1.0
        );
        assert_eq!(
            normalize_gps_position_update_interval_seconds(Some(&json!(0))),
            1.0
        );
        assert_eq!(
            normalize_gps_position_update_interval_seconds(Some(&json!("3.6"))),
            4.0
        );
        assert_eq!(
            normalize_gps_position_update_interval_seconds(Some(&json!(310))),
            300.0
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn working_area_interval_is_never_by_default_and_clips_opt_ins() {
        assert_eq!(normalize_working_area_update_interval(None), None);
        assert_eq!(
            normalize_working_area_update_interval(Some(&json!(null))),
            None
        );
        assert_eq!(
            normalize_working_area_update_interval(Some(&json!("never"))),
            None
        );
        assert_eq!(
            normalize_working_area_update_interval(Some(&json!(""))),
            None
        );
        assert_eq!(
            normalize_working_area_update_interval(Some(&json!("bad"))),
            None
        );
        assert_eq!(
            normalize_working_area_update_interval(Some(&json!("15"))),
            Some(15.0)
        );
        assert_eq!(
            normalize_working_area_update_interval(Some(&json!(60))),
            Some(60.0)
        );
        assert_eq!(
            normalize_working_area_update_interval(Some(&json!(90))),
            Some(90.0)
        );
        assert_eq!(
            normalize_working_area_update_interval(Some(&json!(5))),
            Some(15.0)
        );
        assert_eq!(
            normalize_working_area_update_interval(Some(&json!(9999))),
            Some(1440.0)
        );
        assert_eq!(
            working_area_update_interval_label(Some(&json!(null))),
            "Never"
        );
        assert_eq!(
            working_area_update_interval_label(Some(&json!(15))),
            "15 min"
        );
        assert_eq!(
            working_area_update_interval_label(Some(&json!(60))),
            "1 hour"
        );
        assert_eq!(
            working_area_update_interval_label(Some(&json!(90))),
            "1 hour 30 min"
        );
        assert_eq!(
            working_area_update_interval_label(Some(&json!(150))),
            "2 hours 30 min"
        );
        assert_eq!(
            working_area_update_interval_label(Some(&json!(1440))),
            "24 hours"
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn coordinate_grid_interval_and_placement() {
        assert_eq!(
            normalize_coordinate_grid_interval(Some(&json!("auto"))),
            None
        );
        assert_eq!(
            normalize_coordinate_grid_interval(Some(&json!("250"))),
            Some(250.0)
        );
        assert_eq!(normalize_coordinate_grid_interval(Some(&json!(-1))), None);
        assert_eq!(normalize_coordinate_grid_interval(Some(&json!(0))), None);
        assert_eq!(
            normalize_coordinate_grid_interval(Some(&json!(0.01))),
            Some(0.1)
        );
        assert_eq!(
            normalize_coordinate_grid_interval(Some(&json!(1e9))),
            Some(100_000.0)
        );
        assert_eq!(
            normalize_coordinate_grid_interval(Some(&json!(33.3))),
            Some(33.3)
        );
        assert_eq!(
            word_or(
                Some(&json!("bottom-right")),
                &COORDINATE_GRID_LABEL_PLACEMENTS,
                "top-left"
            ),
            "bottom-right"
        );
        assert_eq!(
            word_or(
                Some(&json!("all")),
                &COORDINATE_GRID_LABEL_PLACEMENTS,
                "top-left"
            ),
            "top-left"
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn batch_concurrency_is_whole_and_reads_only_numbers_and_strings() {
        assert_eq!(normalize_browser_batch_concurrency(Some(&json!(7.9))), 7.0);
        assert_eq!(
            normalize_browser_batch_concurrency(Some(&json!("7.9"))),
            7.0
        );
        assert_eq!(
            normalize_browser_batch_concurrency(Some(&json!("  "))),
            10.0
        );
        assert_eq!(
            normalize_browser_batch_concurrency(Some(&json!("bad"))),
            10.0
        );
        assert_eq!(
            normalize_browser_batch_concurrency(Some(&json!(true))),
            10.0
        );
        assert_eq!(normalize_browser_batch_concurrency(Some(&json!([5]))), 10.0);
        assert_eq!(normalize_browser_batch_concurrency(Some(&json!(0))), 1.0);
        assert_eq!(
            normalize_browser_batch_concurrency(Some(&json!(1000))),
            100.0
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn profile_styles_clip_to_their_steps() {
        assert_eq!(normalize_profile_exaggeration(Some(&json!(0))), 10.0);
        assert_eq!(normalize_profile_exaggeration(Some(&json!(0.1))), 0.5);
        assert_eq!(normalize_profile_exaggeration(Some(&json!(7.3))), 7.3);
        assert_eq!(normalize_profile_exaggeration(Some(&json!(500))), 100.0);
        assert_eq!(normalize_profile_alignment_gap(Some(&json!(-1))), 100.0);
        assert_eq!(normalize_profile_alignment_gap(Some(&json!(0.4))), 0.0);
        assert_eq!(
            normalize_profile_alignment_gap(Some(&json!(99_999))),
            10_000.0
        );
        assert_eq!(normalize_profile_terrain_width(Some(&json!(0.3))), 1.0);
        assert_eq!(normalize_profile_terrain_width(Some(&json!(0))), 15.0);
        assert_eq!(normalize_profile_terrain_width(Some(&json!(900))), 500.0);
        assert_eq!(normalize_profile_terrain_gap(Some(&json!(7000))), 5000.0);
        assert_eq!(normalize_profile_terrain_gap(Some(&json!(-3))), 100.0);
        assert_eq!(tenth_metres_or(Some(&json!(250)), 6.0), 100.0);
        assert_eq!(tenth_metres_or(Some(&json!(6.25)), 6.0), 6.3);
        assert_eq!(tenth_metres_or(Some(&json!(-0.5)), 6.0), 6.0);
        assert_eq!(tenth_metres_or(Some(&json!(null)), 6.0), 0.0);
        assert_eq!(normalize_profile_stroke_width(Some(&json!(30)), 1.5), 8.0);
        assert_eq!(
            normalize_profile_stroke_width(Some(&json!(1.125)), 1.5),
            1.25
        );
        assert_eq!(normalize_profile_stroke_width(Some(&json!(0)), 1.5), 0.5);
        assert_eq!(normalize_profile_stroke_width(Some(&json!("x")), 1.5), 1.5);
        assert_eq!(normalize_profile_point_width(Some(&json!(0))), 1.0);
        assert_eq!(normalize_profile_point_width(Some(&json!(2.25))), 2.5);
        assert_eq!(normalize_profile_point_width(Some(&json!(99))), 24.0);
        assert_eq!(normalize_profile_ordinate_opacity(Some(&json!(2))), 1.0);
        assert_eq!(normalize_profile_ordinate_opacity(Some(&json!(0))), 0.05);
        assert_eq!(
            normalize_profile_ordinate_opacity(Some(&json!(0.125))),
            0.13
        );
        assert_eq!(normalize_profile_ordinate_opacity(None), 0.25);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn colours_are_lower_cased_hex_or_the_default_and_the_blues_migrate() {
        assert_eq!(
            normalize_profile_color(Some(&json!(" #ABCDEF ")), "#000000"),
            "#abcdef"
        );
        assert_eq!(
            normalize_profile_color(Some(&json!("invalid")), "#000000"),
            "#000000"
        );
        assert_eq!(
            normalize_profile_color(Some(&json!("#abc")), "#000000"),
            "#000000"
        );
        assert_eq!(
            normalize_profile_color(Some(&json!("#abcdefa")), "#000000"),
            "#000000"
        );
        assert_eq!(
            normalize_profile_color(Some(&json!(12)), "#000000"),
            "#000000"
        );
        assert_eq!(
            normalize_profile_terrain_color(Some(&json!("#94a3b8"))),
            "#d1d5db"
        );
        assert_eq!(
            normalize_profile_terrain_color(Some(&json!("#38BDF8"))),
            "#d1d5db"
        );
        assert_eq!(
            normalize_profile_terrain_color(Some(&json!("#e5e7eb"))),
            "#e5e7eb"
        );
        assert_eq!(normalize_profile_terrain_color(None), "#d1d5db");
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn hidden_ids_are_strings_trimmed_deduplicated_and_bounded() {
        assert_eq!(
            normalize_hidden_profile_alignment_ids(Some(&json!([
                "side-a",
                " side-a ",
                "",
                "side-b",
                7,
                null,
                true,
                [1, 2]
            ]))),
            ["side-a", "side-b", "7", "null", "true", "1,2"]
        );
        assert_eq!(
            normalize_hidden_profile_alignment_ids(Some(&json!("side-a"))),
            Vec::<String>::new()
        );
        let many: Vec<Value> = (0..600).map(|i| json!(format!("id-{i}"))).collect();
        assert_eq!(
            normalize_hidden_profile_alignment_ids(Some(&Value::Array(many))).len(),
            MAX_HIDDEN_PROFILE_ALIGNMENT_IDS
        );
    }

    // ── The record ──────────────────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn the_defaults_are_the_shipped_record_in_order() {
        let defaults = default_settings();
        assert_eq!(
            defaults.keys().map(String::as_str).collect::<Vec<_>>(),
            SETTING_KEYS
        );
        assert_eq!(normalize_settings(&Map::new()), defaults);
        assert_eq!(normalize_settings(&defaults), defaults);
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn unknown_keys_are_dropped_never_refused() {
        let Value::Object(blob) = json!({
            "retiredSetting": "ignored",
            "showImageRotationInProperties": true,
            "showZoomLevelReadout": true,
        }) else {
            unreachable!()
        };
        let record = normalize_settings(&blob);
        assert_eq!(record["showZoomLevelReadout"], json!(true));
        assert!(!record.contains_key("retiredSetting"));
        assert!(!record.contains_key("showImageRotationInProperties"));
        assert!(record
            .keys()
            .all(|key| SETTING_KEYS.contains(&key.as_str())));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn the_retired_lane_keys_are_read_tolerantly_and_dropped() {
        let Value::Object(blob) = json!({
            "processingLane": "standard",
            "standardLaneConfirmedAt": "2026-05-31T10:00:00.000Z",
            "fastLaneConfirmedAt": "2026-05-19T03:55:00.000Z",
            "mapMemoryBudget": 5,
        }) else {
            unreachable!()
        };
        let record = normalize_settings(&blob);
        assert_eq!(record["mapMemoryBudget"], json!(5));
        for key in RETIRED_KEYS {
            assert!(!record.contains_key(key), "{key} was written");
        }
        assert_eq!(record.len(), SETTING_KEYS.len());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn the_legacy_radius_key_is_read_when_the_width_is_absent() {
        let Value::Object(old) = json!({"profileTerrainPointRadiusPx": 3}) else {
            unreachable!()
        };
        assert_eq!(
            normalize_settings(&old)["profileTerrainPointWidthPx"],
            json!(3)
        );
        let Value::Object(both) = json!({
            "profileTerrainPointRadiusPx": 3,
            "profileTerrainPointWidthPx": 4,
        }) else {
            unreachable!()
        };
        assert_eq!(
            normalize_settings(&both)["profileTerrainPointWidthPx"],
            json!(4)
        );
    }

    // ── The ops ─────────────────────────────────────────────────────────

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn a_first_launch_follows_the_device_and_is_not_written_back() {
        let answer = ask(json!({
            "kind": "normalize",
            "saved": null,
            "device": {"phone": true, "my_location_default": true},
        }))
        .unwrap();
        assert_eq!(answer["settings"]["popupMode"], json!("minimal"));
        assert_eq!(answer["settings"]["showMyLocationControl"], json!(true));
        assert_eq!(answer["persist"], json!(false));
        assert_eq!(
            answer["first_launch"],
            json!({"popup_mode": true, "my_location": true})
        );
        assert_eq!(
            answer.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["settings", "persist", "first_launch"]
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn a_saved_choice_is_never_overridden_by_the_device() {
        let answer = ask(json!({
            "kind": "normalize",
            "saved": {"popupMode": "normal", "showMyLocationControl": false},
            "device": {"phone": true, "my_location_default": true},
        }))
        .unwrap();
        assert_eq!(answer["settings"]["popupMode"], json!("normal"));
        assert_eq!(answer["settings"]["showMyLocationControl"], json!(false));
        assert_eq!(
            answer["first_launch"],
            json!({"popup_mode": false, "my_location": false})
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn a_blob_carrying_a_retired_lane_key_is_written_back_once_without_it() {
        let device = json!({"phone": false, "my_location_default": false});
        for saved in [
            json!({"processingLane": "fast"}),
            json!({"processingLane": "standard", "standardLaneConfirmedAt": "2026-05-31T10:00:00.000Z"}),
            json!({"fastLaneConfirmedAt": "2026-05-19T03:55:00.000Z"}),
            json!({"processingLane": null}),
        ] {
            let answer =
                ask(json!({"kind": "normalize", "saved": saved, "device": device})).unwrap();
            assert_eq!(answer["persist"], json!(true), "{saved}");
            for key in RETIRED_KEYS {
                assert!(settings_of(&answer).get(key).is_none(), "{saved}: {key}");
            }
        }
        // A blob without one is never written from `normalize`.
        let clean = ask(json!({
            "kind": "normalize", "saved": {"mapMemoryBudget": 5}, "device": device,
        }))
        .unwrap();
        assert_eq!(clean["persist"], json!(false));
        // The retired legacy-lane record is not a request member any more.
        assert!(ask(json!({
            "kind": "normalize", "saved": null, "legacy_lane": "fast", "device": device,
        }))
        .is_err());
        assert!(ask(json!({
            "kind": "normalize", "saved": null, "device": device,
            "lane_facts": {"desktop_surface": false, "project_standard_lane": true},
        }))
        .is_err());
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn a_blob_that_is_not_an_object_spreads_nothing() {
        let answer = ask(json!({
            "kind": "normalize",
            "saved": "garbage",
            "device": {"phone": false, "my_location_default": false},
        }))
        .unwrap();
        assert_eq!(settings_of(&answer), &default_settings());
        assert_eq!(
            answer["first_launch"],
            json!({"popup_mode": true, "my_location": true})
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn patch_merges_over_current_and_null_clears() {
        let answer = ask(json!({
            "kind": "patch",
            "current": {
                "processingLane": "standard",
                "standardLaneConfirmedAt": "2026-05-31T10:00:00.000Z",
                "mapMemoryBudget": 5,
                "showZoomLevelReadout": true,
            },
            "patch": {"showZoomLevelReadout": null, "retired": 1},
        }))
        .unwrap();
        assert_eq!(answer["settings"]["showZoomLevelReadout"], json!(false));
        assert_eq!(answer["settings"]["mapMemoryBudget"], json!(5));
        assert!(settings_of(&answer).get("processingLane").is_none());
        assert!(settings_of(&answer).get("retired").is_none());
        assert_eq!(
            answer.as_object().unwrap().keys().collect::<Vec<_>>(),
            ["settings"]
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn a_null_in_a_patch_is_the_stores_undefined_on_every_key() {
        // The store spread `{...current, key: undefined}` and every reader
        // then took its default. The keys it did not guard with `??` read a
        // merged `null` as 0 — so the patch's `null` must remove the key.
        let mut current = default_settings();
        current.insert("profileGroundWidthPx".into(), json!(3));
        current.insert("profileClearanceWidthPx".into(), json!(4));
        current.insert("profileAlignmentGapM".into(), json!(250));
        current.insert("profileGroundClearanceOffsetM".into(), json!(9.5));
        current.insert("profileTerrainOrdinateOpacity".into(), json!(0.5));
        current.insert("hiddenProfileAlignmentIds".into(), json!(["a"]));
        current.insert("coordinateGridIntervalM".into(), json!(50));
        current.insert("boxZoomHideMinZoom".into(), json!(12));
        let answer = ask(json!({
            "kind": "patch",
            "current": current,
            "patch": {
                "profileGroundWidthPx": null,
                "profileClearanceWidthPx": null,
                "profileAlignmentGapM": null,
                "profileGroundClearanceOffsetM": null,
                "profileTerrainOrdinateOpacity": null,
                "hiddenProfileAlignmentIds": null,
                "coordinateGridIntervalM": null,
                "boxZoomHideMinZoom": null,
            },
        }))
        .unwrap();
        let settings = settings_of(&answer);
        for key in [
            "profileGroundWidthPx",
            "profileClearanceWidthPx",
            "profileAlignmentGapM",
            "profileGroundClearanceOffsetM",
            "profileTerrainOrdinateOpacity",
            "hiddenProfileAlignmentIds",
            "coordinateGridIntervalM",
            "boxZoomHideMinZoom",
        ] {
            assert_eq!(settings[key], default_settings()[key], "{key}");
        }
        // A saved blob is read as written: its `null` is `Number(null)`.
        let saved = ask(json!({
            "kind": "normalize",
            "saved": {"profileGroundWidthPx": null, "profileAlignmentGapM": null},
            "device": {"phone": false, "my_location_default": false},
        }))
        .unwrap();
        assert_eq!(saved["settings"]["profileGroundWidthPx"], json!(0.5));
        assert_eq!(saved["settings"]["profileAlignmentGapM"], json!(0));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn reset_lands_where_a_fresh_install_would() {
        let answer = ask(json!({
            "kind": "reset",
            "current": {
                "mapMemoryBudget": 5,
                "gpsPositionUpdateIntervalSeconds": 300,
                "profileVerticalExaggeration": 7,
                "popupMode": "normal",
                "showMyLocationControl": false,
            },
            "keys": ["mapMemoryBudget", "gpsPositionUpdateIntervalSeconds", "popupMode", "showMyLocationControl"],
            "device": {"phone": true, "my_location_default": true},
        }))
        .unwrap();
        let record = settings_of(&answer);
        assert_eq!(record["mapMemoryBudget"], json!(3));
        assert_eq!(record["gpsPositionUpdateIntervalSeconds"], json!(1));
        assert_eq!(record["profileVerticalExaggeration"], json!(7));
        assert_eq!(record["popupMode"], json!("minimal"));
        assert_eq!(record["showMyLocationControl"], json!(true));

        let error = ask(json!({
            "kind": "reset",
            "current": {},
            "keys": ["mapMemoryBudget", "bogus", "alsoBogus"],
            "device": {"phone": false, "my_location_default": false},
        }))
        .unwrap_err();
        assert_eq!(
            error,
            "reset names settings that do not exist: bogus, alsoBogus"
        );
        // A retired key is not a setting a reset may name.
        let retired = ask(json!({
            "kind": "reset",
            "current": {},
            "keys": ["standardLaneConfirmedAt"],
            "device": {"phone": false, "my_location_default": false},
        }))
        .unwrap_err();
        assert_eq!(
            retired,
            "reset names settings that do not exist: standardLaneConfirmedAt"
        );
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn the_performance_profile_scales_with_the_browser_estimate() {
        let profile = |budget: Value, hint: Value| {
            ask(json!({"kind": "performance_profile", "budget": budget, "resource_hint": hint}))
                .unwrap()["profile"]
                .clone()
        };
        let none = json!({"js_heap_limit_bytes": null, "device_memory_gb": null, "hardware_concurrency": null});
        let low = profile(json!(1), none.clone());
        let high = profile(json!(5), none.clone());
        assert_eq!(low["maxTileCacheSize"], json!(48));
        assert_eq!(high["maxTileCacheSize"], json!(384));
        assert_eq!(low["tileLodLevels"], json!(2));
        assert_eq!(low["tileLodRatio"], json!(1));
        assert_eq!(high["tileLodLevels"], json!(6));
        assert_eq!(high["tileLodRatio"], json!(6));
        assert_eq!(low["cleanupDelayMs"], json!(120));
        assert_eq!(high["cleanupDelayMs"], json!(1000));
        assert_eq!(low["refreshExpiredTiles"], json!(false));
        assert_eq!(high["refreshExpiredTiles"], json!(true));
        assert_eq!(high["resourceScale"], json!(1));

        let capable = profile(json!(5), json!({"js_heap_limit_bytes": 4_294_967_296_u64}));
        assert_eq!(capable["maxTileCacheSize"], json!(512));
        assert_eq!(capable["resourceLabel"], json!("Capable browser estimate"));
        assert_eq!(capable["resourceScale"], json!(4.0 / 3.0));

        let large = profile(json!(3), json!({"device_memory_gb": 8}));
        assert_eq!(large["maxTileCacheSize"], json!(240));
        assert_eq!(large["resourceLabel"], json!("Large browser estimate"));

        let constrained = profile(json!(1), json!({"device_memory_gb": 2}));
        assert_eq!(constrained["maxTileCacheSize"], json!(32));
        assert_eq!(constrained["tileLodRatio"], json!(1));
        assert_eq!(
            constrained["resourceLabel"],
            json!("Constrained browser estimate")
        );

        let cores = profile(json!(2), json!({"hardware_concurrency": "8"}));
        assert_eq!(cores["maxTileCacheSize"], json!(128));
        assert_eq!(cores["resourceLabel"], json!("Capable browser estimate"));

        let bad = profile(json!("bad"), json!({}));
        assert_eq!(bad["label"], json!("Balanced"));
        let absent =
            ask(json!({"kind": "performance_profile", "budget": 4})).unwrap()["profile"].clone();
        assert_eq!(absent["label"], json!("Smooth"));
        assert_eq!(absent["maxTileCacheSize"], json!(256));
    }

    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    #[cfg_attr(not(target_arch = "wasm32"), test)]
    fn the_request_itself_is_refused_when_the_host_built_it_wrong() {
        assert!(evaluate(b"{").is_err());
        assert!(evaluate(&vec![b' '; MAX_INPUT_BYTES + 1]).is_err());
        let other_schema = to_vec(&json!({
            "schema": "ds.application-settings/v0",
            "op": {"kind": "interval_label", "value": 15},
        }))
        .unwrap();
        assert_eq!(
            evaluate(&other_schema).unwrap_err(),
            "unsupported application settings schema; expected ds.application-settings/v2"
        );
        assert!(ask(json!({"kind": "sideways"})).is_err());
        assert!(ask(json!({"kind": "lane", "settings": {}})).is_err());
        assert!(ask(json!({"kind": "set_lane", "current": {}, "requested": "fast"})).is_err());
        assert!(ask(json!({
            "kind": "normalize",
            "device": {"phone": false, "my_location_default": false},
            "extra": 1,
        }))
        .is_err());
        assert!(ask(json!({
            "kind": "normalize",
            "device": {"phone": false, "my_location_default": false, "tablet": true},
        }))
        .is_err());
        assert!(ask(json!({"kind": "patch", "current": {}, "patch": "x"})).is_err());
        assert!(ask(json!({"kind": "interval_label"})).is_err());
        assert_eq!(
            ask(json!({"kind": "interval_label", "value": 90})).unwrap(),
            json!({"label": "1 hour 30 min"})
        );
    }
}
