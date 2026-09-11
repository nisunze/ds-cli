//! Process settings, decided once for every host.
//!
//! Which settings a run may carry, what a named preset produces for THIS
//! project, which dependent flags collapse into the effective contract, which
//! settings the operator may see for a lane and preset, what a hidden setting
//! puts on the wire, and what the project's default preset is — all of this
//! was TypeScript in three modules (with module-level state that depended on
//! which project was loaded last) and, for the dependency collapse, a second
//! copy in the engine that disagreed on one key. This module is the one
//! answer: pure over the processor catalogue (ds-network's, handed in as a
//! fact), the project's configuration sheets, the lane, the preset and the
//! operator's own toggles. It performs no IO.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const SCHEMA: &str = "ds.process-settings/v1";
pub const MAX_INPUT_BYTES: usize = 8 * 1024 * 1024;

const DESIRABLE_PROPERTIES: &str = "write_desirable_properties";
const KEEP_LV_LINES_TOPOLOGY: &str = "keep_lv_lines_topology";
const KEEP_LV_LINES_LENGTH: &str = "keep_lv_lines_length";
const KEEP_LV_LINES_SIZES: &str = "keep_lv_lines_sizes";
const JOIN_LV_CABLES_OF_SAME_SIZE: &str = "join_lv_cables_of_same_size";
const KEEP_LV_POLES: &str = "keep_lv_poles";
const CONNECT_UNCONNECTED_POLES: &str = "connect_unconnected_poles";
const KEEP_SERVICE_CABLES: &str = "keep_service_cables";
const KEEP_SERVICE_CABLES_SIZES: &str = "keep_service_cables_sizes";
const EXTEND_SERVICE_CABLES: &str = "extend_service_cables";
const GEOMETRY_PROPERTIES: &str = "calculate_geometry_properties";
const CONNECT_UNCONNECTED_CUSTOMERS: &str = "connect_unconnected_customers";
const INCLUDE_IN_COMBINED_REPORTS: &str = "include_in_combined_reports";

/// The keep flags whose presence lets computed ("desirable") properties be
/// written. These are the engine's five (`LvProcessSettings::
/// normalize_dependencies`); the browser also counted `keep_flying_stay`,
/// which the engine cleared again on arrival — the operator's checkbox did
/// nothing and nothing said so.
const PROPERTY_KEEP_KEYS: [&str; 5] = [
    KEEP_LV_POLES,
    KEEP_LV_LINES_LENGTH,
    KEEP_LV_LINES_SIZES,
    KEEP_SERVICE_CABLES,
    KEEP_SERVICE_CABLES_SIZES,
];
/// Settings that re-seed to their catalogue default when a parent toggle
/// reveals them, so a revealed option comes up at its default rather than at
/// whatever the dependency collapse zeroed it to while hidden.
const REVEAL_DEFAULT_KEYS: [&str; 1] = [KEEP_LV_LINES_SIZES];

const CUSTOMER_SOURCE_KEYS: [&str; 3] = [
    "pull_customers_from_bldg",
    "pull_customers_from_hh_table",
    "override_uploaded_customers",
];
const REPORT_ACTION_KEYS: [&str; 2] = ["export_reports_for_download", INCLUDE_IN_COMBINED_REPORTS];
const REPORT_DETAIL_KEYS: [&str; 2] = [
    "derive_transfo_before_mirror",
    "enrich_admin_bounds_before_mirror",
];

const TRANSFORMER_SETTINGS_SHEET: &str = "transformer_settings";
const PROJECT_SETTINGS_SHEET: &str = "project_settings";
const DRAFTING_TOLERANCES_SHEET: &str = "drafting_tolerances";
const DEFAULT_PROCESS_PRESET_PARAMETER: &str = "default_process_preset";

// ── the catalogue, as ds-network publishes it ──────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct VisibleWhen {
    pub key: String,
    pub equals: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BoolSetting {
    pub key: String,
    #[serde(default)]
    pub label: String,
    pub default: Value,
    #[serde(default)]
    pub visible_when: Vec<VisibleWhen>,
    #[serde(default)]
    pub value_when_hidden: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct NumericSetting {
    pub key: String,
    #[serde(default)]
    pub label: String,
    pub default: Value,
    #[serde(default)]
    pub visible_when: Vec<VisibleWhen>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Group {
    #[serde(default)]
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub settings: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Preset {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub overrides: Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Catalog {
    pub settings: Vec<BoolSetting>,
    #[serde(default)]
    pub numeric_settings: Vec<NumericSetting>,
    pub groups: Vec<Group>,
    pub presets: Vec<Preset>,
    #[serde(default)]
    pub labels: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    Standard,
    Fast,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Preset2 {
    Drafting,
    Sketch,
}

impl Preset2 {
    fn token(self) -> &'static str {
        match self {
            Preset2::Drafting => "drafting",
            Preset2::Sketch => "sketch",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "drafting" => Some(Preset2::Drafting),
            "sketch" => Some(Preset2::Sketch),
            _ => None,
        }
    }
}

/// Settings are booleans and finite numbers, nothing else.
pub type Settings = Map<String, Value>;

fn is_setting_value(value: &Value) -> bool {
    value.is_boolean() || value.as_f64().is_some_and(f64::is_finite)
}

fn same_type(a: &Value, b: &Value) -> bool {
    (a.is_boolean() && b.is_boolean()) || (a.is_number() && b.is_number())
}

fn truthy(settings: &Settings, key: &str) -> bool {
    settings.get(key) == Some(&Value::Bool(true))
}

// ── the model: keys, groups, base settings ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupView {
    pub key: String,
    pub label: String,
    pub settings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Model {
    pub keys: Vec<String>,
    pub groups: Vec<GroupView>,
    pub base_settings: Settings,
}

/// Numeric drafting tolerances the project declares (`drafting_tolerances`
/// rows), keyed exactly as the row names them.
pub fn numeric_defaults(project_config: Option<&Value>) -> BTreeMap<String, f64> {
    let mut out = BTreeMap::new();
    for row in sheet_rows(project_config, DRAFTING_TOLERANCES_SHEET) {
        let key = row
            .get("parameter")
            .or_else(|| row.get("name"))
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if key.is_empty() {
            continue;
        }
        let value = match row.get("value") {
            Some(Value::Number(n)) => n.as_f64(),
            Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
            Some(Value::Bool(b)) => Some(if *b { 1. } else { 0. }),
            _ => None,
        };
        if let Some(value) = value.filter(|v| v.is_finite()) {
            out.insert(key, value);
        }
    }
    out
}

fn sheet_rows(project_config: Option<&Value>, sheet: &str) -> Vec<Map<String, Value>> {
    project_config
        .and_then(|c| c.get("sheets"))
        .and_then(|s| s.get(sheet))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_object)
                .filter(|row| row.get("parameter").is_some_and(Value::is_string))
                .cloned()
                .collect()
        })
        .unwrap_or_default()
}

/// The canonical spelling of a configuration parameter, as the project
/// template readers spell it: lowercase, runs of non-alphanumerics become
/// `_`, edges trimmed, a leading `_` kept for experimental keys.
pub use crate::design_config::canonical_parameter_name;

use crate::design_config::process_setting_value as setting_value;

/// The project's own preset rows (`transformer_settings` rows carrying a
/// `preset` column), keyed by preset.
pub fn project_preset_overrides(project_config: Option<&Value>) -> BTreeMap<String, Settings> {
    let mut out: BTreeMap<String, Settings> = BTreeMap::new();
    out.insert("drafting".into(), Map::new());
    out.insert("sketch".into(), Map::new());
    for row in sheet_rows(project_config, TRANSFORMER_SETTINGS_SHEET) {
        let Some(preset) = row
            .get("preset")
            .and_then(Value::as_str)
            .and_then(Preset2::parse)
        else {
            continue;
        };
        let key =
            canonical_parameter_name(row.get("parameter").and_then(Value::as_str).unwrap_or(""));
        let Some(value) = row.get("value").and_then(setting_value) else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        out.entry(preset.token().into())
            .or_default()
            .insert(key, value);
    }
    out
}

pub fn model(catalog: &Catalog, project_config: Option<&Value>) -> Result<Model, String> {
    let numeric = numeric_defaults(project_config);
    let mut base = Settings::new();
    let mut keys = Vec::new();
    for setting in &catalog.settings {
        if setting.key.is_empty() {
            continue;
        }
        let Value::Bool(default) = setting.default else {
            return Err(format!("Invalid processor default: {}", setting.key));
        };
        keys.push(setting.key.clone());
        base.insert(setting.key.clone(), Value::Bool(default));
    }
    for setting in &catalog.numeric_settings {
        if setting.key.is_empty() {
            continue;
        }
        keys.push(setting.key.clone());
        let value = match numeric.get(&setting.key) {
            Some(v) => json!(v),
            None => match setting.default.as_f64() {
                Some(v) if v.is_finite() => setting.default.clone(),
                _ => return Err(format!("Invalid processor default: {}", setting.key)),
            },
        };
        base.insert(setting.key.clone(), value);
    }
    let known: BTreeSet<&str> = keys.iter().map(String::as_str).collect();
    let groups = catalog
        .groups
        .iter()
        .map(|group| GroupView {
            key: group.key.clone(),
            label: group.label.clone(),
            settings: group
                .settings
                .iter()
                .filter(|key| known.contains(key.as_str()))
                .cloned()
                .collect(),
        })
        .filter(|group| {
            !group.key.is_empty() && !group.label.is_empty() && !group.settings.is_empty()
        })
        .collect();
    Ok(Model {
        keys,
        groups,
        base_settings: normalize(&base),
    })
}

// ── the dependency collapse ─────────────────────────────────────────────

/// Collapse dependent flags into the effective process contract.
pub fn normalize(state: &Settings) -> Settings {
    let mut out = state.clone();
    let set = |out: &mut Settings, key: &str, value: bool| {
        if out.contains_key(key) {
            out.insert(key.into(), Value::Bool(value));
        }
    };
    if out.get(KEEP_LV_LINES_TOPOLOGY) == Some(&Value::Bool(false)) {
        set(&mut out, KEEP_LV_LINES_LENGTH, false);
        set(&mut out, KEEP_LV_LINES_SIZES, false);
    }
    if truthy(&out, KEEP_LV_LINES_LENGTH) {
        set(&mut out, JOIN_LV_CABLES_OF_SAME_SIZE, false);
    }
    if out.get(KEEP_LV_POLES) == Some(&Value::Bool(false)) {
        set(&mut out, CONNECT_UNCONNECTED_POLES, false);
    }
    if out.contains_key(KEEP_SERVICE_CABLES) {
        let keep = truthy(&out, KEEP_SERVICE_CABLES);
        set(&mut out, KEEP_SERVICE_CABLES_SIZES, keep);
        if keep {
            set(&mut out, EXTEND_SERVICE_CABLES, false);
        }
    }
    if out.contains_key(DESIRABLE_PROPERTIES)
        && !PROPERTY_KEEP_KEYS.iter().any(|key| truthy(&out, key))
    {
        out.insert(DESIRABLE_PROPERTIES.into(), Value::Bool(false));
    }
    out
}

// ── visibility ──────────────────────────────────────────────────────────

fn visibility_rules<'a>(catalog: &'a Catalog, key: &str) -> Option<&'a [VisibleWhen]> {
    catalog
        .settings
        .iter()
        .find(|s| s.key == key)
        .map(|s| s.visible_when.as_slice())
        .or_else(|| {
            catalog
                .numeric_settings
                .iter()
                .find(|s| s.key == key)
                .map(|s| s.visible_when.as_slice())
        })
        .filter(|rules| !rules.is_empty())
}

/// May the operator see this setting, given the live state? AND over every
/// clause; a clause on a key the state does not hold is unmet.
pub fn is_visible(catalog: &Catalog, key: &str, state: &Settings) -> bool {
    if key == DESIRABLE_PROPERTIES {
        return PROPERTY_KEEP_KEYS.iter().any(|k| truthy(state, k));
    }
    match visibility_rules(catalog, key) {
        None => true,
        Some(rules) => rules.iter().all(|clause| {
            state
                .get(&clause.key)
                .is_some_and(|value| value == &clause.equals)
        }),
    }
}

fn hidden_value(catalog: &Catalog, key: &str) -> Option<bool> {
    catalog
        .settings
        .iter()
        .find(|s| s.key == key)
        .and_then(|s| s.value_when_hidden)
}

fn catalog_default(catalog: &Catalog, key: &str) -> bool {
    catalog
        .settings
        .iter()
        .find(|s| s.key == key)
        .is_some_and(|s| s.default == Value::Bool(true))
}

/// One kept-or-forced-or-dropped decision about a wire key.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Dropped {
    pub key: String,
    pub why: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forced_to: Option<Value>,
}

/// The wire payload from the local state, honouring visibility rules and
/// forced-while-hidden values: visible → verbatim; hidden with
/// `value_when_hidden` → that value; hidden otherwise → omitted.
pub fn prune_hidden(catalog: &Catalog, state: &Settings, dropped: &mut Vec<Dropped>) -> Settings {
    let normalized = normalize(state);
    let mut out = Settings::new();
    for (key, value) in &normalized {
        if is_visible(catalog, key, &normalized) {
            out.insert(key.clone(), value.clone());
        } else if let Some(forced) = hidden_value(catalog, key) {
            out.insert(key.clone(), Value::Bool(forced));
            dropped.push(Dropped {
                key: key.clone(),
                why: "hidden_by_visibility_forced".into(),
                forced_to: Some(Value::Bool(forced)),
            });
        } else {
            dropped.push(Dropped {
                key: key.clone(),
                why: "hidden_by_visibility".into(),
                forced_to: None,
            });
        }
    }
    normalize(&out)
}

/// Apply one interactive change and return the normalized next state; a
/// setting this change reveals re-seeds to its catalogue default.
pub fn change(catalog: &Catalog, state: &Settings, changes: &Settings) -> Settings {
    let mut next = state.clone();
    for (key, value) in changes {
        next.insert(key.clone(), value.clone());
    }
    for key in REVEAL_DEFAULT_KEYS {
        if !next.contains_key(key) || changes.contains_key(key) {
            continue;
        }
        if !is_visible(catalog, key, state) && is_visible(catalog, key, &next) {
            next.insert(key.into(), Value::Bool(catalog_default(catalog, key)));
        }
    }
    normalize(&next)
}

// ── presets ─────────────────────────────────────────────────────────────

/// The settings a named preset produces for this project: catalogue
/// overrides first, then the project's own rows — which win only for a key
/// the catalogue knows, with the catalogue's type.
pub fn apply_preset(
    catalog: &Catalog,
    project_config: Option<&Value>,
    base: &Settings,
    preset: Preset2,
) -> Settings {
    let empty = Map::new();
    let overrides = catalog
        .presets
        .iter()
        .find(|p| p.name == preset.token())
        .map(|p| &p.overrides)
        .unwrap_or(&empty);
    let project = project_preset_overrides(project_config);
    let project = project.get(preset.token()).unwrap_or(&empty);
    let mut applied = base.clone();
    for (key, current) in base {
        if let Some(value) = overrides.get(key) {
            applied.insert(key.clone(), value.clone());
        }
        if let Some(value) = project.get(key) {
            if same_type(value, current) {
                applied.insert(key.clone(), value.clone());
            }
        }
    }
    normalize(&applied)
}

/// Overlay persisted or declarative settings onto a known preset-shaped
/// contract: unknown keys and type changes are ignored.
pub fn merge_known(defaults: &Settings, raw: &Settings) -> Settings {
    let allow_all = defaults.is_empty();
    let mut out = defaults.clone();
    for (key, value) in raw {
        if !is_setting_value(value) {
            continue;
        }
        match defaults.get(key) {
            Some(current) if same_type(current, value) => {
                out.insert(key.clone(), value.clone());
            }
            None if allow_all => {
                out.insert(key.clone(), value.clone());
            }
            _ => {}
        }
    }
    normalize(&out)
}

// ── the dialog: what may be seen, what goes on the wire ─────────────────

#[derive(Debug, Clone, Copy, Deserialize)]
pub struct DialogContext {
    pub lane: Lane,
    pub preset: Preset2,
    #[serde(default)]
    pub use_firestore_design_data: bool,
}

fn dialog_hidden(context: DialogContext) -> BTreeMap<&'static str, &'static str> {
    let mut hidden: BTreeMap<&str, &str> = BTreeMap::new();
    for key in REPORT_DETAIL_KEYS {
        hidden.insert(key, "not_operator_tunable");
    }
    hidden.insert(GEOMETRY_PROPERTIES, "not_operator_tunable");
    if context.lane == Lane::Fast || context.preset == Preset2::Sketch {
        for key in CUSTOMER_SOURCE_KEYS {
            hidden.insert(
                key,
                if context.lane == Lane::Fast {
                    "customer_source_chosen_in_fast_setup"
                } else {
                    "sketch_has_no_customer_source"
                },
            );
        }
    }
    if context.lane == Lane::Fast {
        for key in REPORT_ACTION_KEYS {
            hidden.insert(key, "reporting_is_a_separate_fast_action");
        }
    } else {
        hidden.insert(DESIRABLE_PROPERTIES, "browser_pipeline_only");
    }
    if context.use_firestore_design_data {
        hidden.insert(
            INCLUDE_IN_COMBINED_REPORTS,
            "firestore_design_data_has_no_combined_mirror",
        );
    }
    if context.preset == Preset2::Sketch {
        hidden.insert(CONNECT_UNCONNECTED_CUSTOMERS, "drafting_only_control");
    }
    hidden
}

/// The dialog's groups for this lane and preset.
pub fn visible_groups(groups: &[GroupView], context: DialogContext) -> Vec<GroupView> {
    let hidden = dialog_hidden(context);
    groups
        .iter()
        .map(|group| GroupView {
            key: group.key.clone(),
            label: group.label.clone(),
            settings: group
                .settings
                .iter()
                .filter(|key| !hidden.contains_key(key.as_str()))
                .cloned()
                .collect(),
        })
        .filter(|group| !group.settings.is_empty())
        .collect()
}

/// The dialog's opening state: the preset applied, normalized, with geometry
/// property calculation pinned on when the state carries that key.
pub fn dialog_defaults(settings: &Settings) -> Settings {
    let mut out = normalize(settings);
    if out.contains_key(GEOMETRY_PROPERTIES) {
        out.insert(GEOMETRY_PROPERTIES.into(), Value::Bool(true));
    }
    out
}

/// The wire payload: hidden-by-visibility keys pruned, dialog-hidden keys
/// dropped, geometry pinned on, the Sketch customer-connection contract
/// stamped, and every removal named.
pub fn wire(
    catalog: &Catalog,
    settings: &Settings,
    context: DialogContext,
) -> (Settings, Vec<Dropped>) {
    let mut dropped = Vec::new();
    let pruned = prune_hidden(catalog, settings, &mut dropped);
    let normalized = normalize(&pruned);
    let hidden = dialog_hidden(context);
    let mut out = Settings::new();
    for (key, value) in &normalized {
        match hidden.get(key.as_str()) {
            Some(why) => dropped.push(Dropped {
                key: key.clone(),
                why: (*why).into(),
                forced_to: None,
            }),
            None => {
                out.insert(key.clone(), value.clone());
            }
        }
    }
    out.insert(GEOMETRY_PROPERTIES.into(), Value::Bool(true));
    dropped.push(Dropped {
        key: GEOMETRY_PROPERTIES.into(),
        why: "contract_pinned_on".into(),
        forced_to: Some(Value::Bool(true)),
    });
    if context.preset == Preset2::Sketch {
        out.insert(CONNECT_UNCONNECTED_CUSTOMERS.into(), Value::Bool(false));
        dropped.push(Dropped {
            key: CONNECT_UNCONNECTED_CUSTOMERS.into(),
            why: "sketch_preset_contract".into(),
            forced_to: Some(Value::Bool(false)),
        });
    }
    (normalize(&out), dropped)
}

// ── the project's default preset ────────────────────────────────────────

fn project_type_token(value: &str) -> Option<&'static str> {
    let raw: String = value
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c == '-' || c.is_whitespace() {
                '_'
            } else {
                c
            }
        })
        .collect();
    // Collapse runs of `_` the way the browser's `[-\s]+` did.
    let mut collapsed = String::new();
    for ch in raw.chars() {
        if ch == '_' && collapsed.ends_with('_') {
            continue;
        }
        collapsed.push(ch);
    }
    match collapsed.as_str() {
        "new" | "new_project" | "new_build" | "greenfield" => Some("new_build"),
        "assessment" | "assessment_project" | "verification" | "verification_project" => {
            Some("assessment")
        }
        "as_built" | "asbuilt" | "as_built_project" => Some("as_built"),
        _ => None,
    }
}

/// The preset a project starts with: the network template speaks first
/// (`project_settings.default_process_preset`), the project type is the
/// fallback (new-build projects draft, every other kind sketches).
pub fn default_preset(
    project_config: Option<&Value>,
    project_type: Option<&str>,
) -> (Preset2, &'static str) {
    for row in sheet_rows(project_config, PROJECT_SETTINGS_SHEET) {
        let name = row.get("parameter").and_then(Value::as_str).unwrap_or("");
        if canonical_parameter_name(name) != DEFAULT_PROCESS_PRESET_PARAMETER {
            continue;
        }
        let value = match row.get("value") {
            Some(Value::String(s)) => s.clone(),
            Some(other) => other.to_string(),
            None => String::new(),
        };
        return match Preset2::parse(&value) {
            Some(preset) => (preset, "template"),
            None => project_type_default(project_type),
        };
    }
    project_type_default(project_type)
}

fn project_type_default(project_type: Option<&str>) -> (Preset2, &'static str) {
    match project_type.and_then(project_type_token) {
        Some("new_build") => (Preset2::Drafting, "project_type"),
        Some(_) => (Preset2::Sketch, "project_type"),
        None => (Preset2::Sketch, "fallback"),
    }
}

// ── the door ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Model {
        catalog: Catalog,
        #[serde(default)]
        project_config: Option<Value>,
    },
    Preset {
        catalog: Catalog,
        #[serde(default)]
        project_config: Option<Value>,
        preset: Preset2,
    },
    Resolve {
        catalog: Catalog,
        #[serde(default)]
        project_config: Option<Value>,
        lane: Lane,
        preset: Preset2,
        #[serde(default)]
        use_firestore_design_data: bool,
        #[serde(default)]
        operator: Option<Settings>,
    },
    Normalize {
        settings: Settings,
    },
    Change {
        catalog: Catalog,
        state: Settings,
        changes: Settings,
    },
    /// Visibility for `keys` (default: every key in the state) against the
    /// live state — a key the state does not hold is still answered.
    Visible {
        catalog: Catalog,
        state: Settings,
        #[serde(default)]
        keys: Option<Vec<String>>,
    },
    MergeKnown {
        defaults: Settings,
        raw: Settings,
    },
    DefaultPreset {
        #[serde(default)]
        project_config: Option<Value>,
        #[serde(default)]
        project_type: Option<String>,
    },
    DialogDefaults {
        settings: Settings,
    },
    /// The wire-shaped state after the visibility rules alone: hidden keys
    /// forced or omitted, dialog context not yet applied.
    Prune {
        catalog: Catalog,
        settings: Settings,
    },
    /// The dialog's groups for a lane and preset.
    VisibleGroups {
        groups: Vec<GroupView>,
        lane: Lane,
        preset: Preset2,
        #[serde(default)]
        use_firestore_design_data: bool,
    },
    /// The project's own preset rows, keyed by preset.
    ProjectPresets {
        #[serde(default)]
        project_config: Option<Value>,
    },
    Wire {
        catalog: Catalog,
        settings: Settings,
        lane: Lane,
        preset: Preset2,
        #[serde(default)]
        use_firestore_design_data: bool,
    },
}

fn check_settings(settings: &Settings) -> Result<(), String> {
    for (key, value) in settings {
        if key.is_empty() || key.len() > 128 || !is_setting_value(value) {
            return Err(
                "Process settings must have bounded names and boolean or finite numeric values"
                    .into(),
            );
        }
    }
    Ok(())
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("process settings request exceeds 8 MiB".into());
    }
    let command: Command = serde_json::from_slice(input)
        .map_err(|e| format!("invalid process settings request: {e}"))?;
    let value = match command {
        Command::Model {
            catalog,
            project_config,
        } => serde_json::to_value(model(&catalog, project_config.as_ref())?)
            .map_err(|e| e.to_string())?,
        Command::Preset {
            catalog,
            project_config,
            preset,
        } => {
            let base = model(&catalog, project_config.as_ref())?.base_settings;
            json!({"settings": apply_preset(&catalog, project_config.as_ref(), &base, preset)})
        }
        Command::Resolve {
            catalog,
            project_config,
            lane,
            preset,
            use_firestore_design_data,
            operator,
        } => {
            let context = DialogContext {
                lane,
                preset,
                use_firestore_design_data,
            };
            let model = model(&catalog, project_config.as_ref())?;
            let mut settings = dialog_defaults(&apply_preset(
                &catalog,
                project_config.as_ref(),
                &model.base_settings,
                preset,
            ));
            if let Some(operator) = operator {
                check_settings(&operator)?;
                settings = merge_known(&settings, &operator);
            }
            let (wire_settings, dropped) = wire(&catalog, &settings, context);
            json!({
                "settings": settings,
                "visible_groups": visible_groups(&model.groups, context),
                "wire_settings": wire_settings,
                "dropped": dropped,
            })
        }
        Command::Normalize { settings } => {
            check_settings(&settings)?;
            json!({"settings": normalize(&settings)})
        }
        Command::Change {
            catalog,
            state,
            changes,
        } => {
            check_settings(&state)?;
            check_settings(&changes)?;
            json!({"settings": change(&catalog, &state, &changes)})
        }
        Command::Visible {
            catalog,
            state,
            keys,
        } => {
            let keys: Vec<String> = keys.unwrap_or_else(|| state.keys().cloned().collect());
            let visible: Map<String, Value> = keys
                .iter()
                .map(|key| (key.clone(), Value::Bool(is_visible(&catalog, key, &state))))
                .collect();
            json!({"visible": visible})
        }
        Command::MergeKnown { defaults, raw } => {
            check_settings(&defaults)?;
            json!({"settings": merge_known(&defaults, &raw)})
        }
        Command::DefaultPreset {
            project_config,
            project_type,
        } => {
            let (preset, source) = default_preset(project_config.as_ref(), project_type.as_deref());
            json!({"preset": preset, "source": source})
        }
        Command::DialogDefaults { settings } => {
            check_settings(&settings)?;
            json!({"settings": dialog_defaults(&settings)})
        }
        Command::Prune { catalog, settings } => {
            check_settings(&settings)?;
            let mut dropped = Vec::new();
            let pruned = prune_hidden(&catalog, &settings, &mut dropped);
            json!({"settings": pruned, "dropped": dropped})
        }
        Command::VisibleGroups {
            groups,
            lane,
            preset,
            use_firestore_design_data,
        } => {
            json!({"groups": visible_groups(&groups, DialogContext { lane, preset, use_firestore_design_data })})
        }
        Command::ProjectPresets { project_config } => {
            json!({"presets": project_preset_overrides(project_config.as_ref())})
        }
        Command::Wire {
            catalog,
            settings,
            lane,
            preset,
            use_firestore_design_data,
        } => {
            check_settings(&settings)?;
            let (wire_settings, dropped) = wire(
                &catalog,
                &settings,
                DialogContext {
                    lane,
                    preset,
                    use_firestore_design_data,
                },
            );
            json!({"wire_settings": wire_settings, "dropped": dropped})
        }
    };
    serde_json::to_string(&value).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: &str = include_str!("../vendor/ds-geo-lite/network/processor_presets.json");

    fn catalog() -> Catalog {
        serde_json::from_str(CATALOG).expect("the engine's catalogue parses")
    }

    fn b(v: bool) -> Value {
        Value::Bool(v)
    }

    #[test]
    fn the_model_carries_every_engine_default_and_group() {
        let m = model(&catalog(), None).unwrap();
        assert_eq!(m.keys.len(), 35);
        assert_eq!(m.groups.len(), 10);
        assert_eq!(m.base_settings.len(), 35);
        assert!(m.base_settings.contains_key("weld_tolerance"));
    }

    #[test]
    fn numeric_tolerances_come_from_the_project_and_preset_rows_win_only_when_typed_and_known() {
        let config = json!({"sheets": {
        "drafting_tolerances": [{"parameter": "weld_tolerance", "value": "2.5"}],
        "transformer_settings": [
            {"parameter": "Keep LV Poles", "value": "yes", "preset": "drafting"},
            {"parameter": "keep_lv_poles", "value": "7", "preset": "sketch"},
            {"parameter": "not_a_setting", "value": true, "preset": "drafting"}
        ]}});
        let cat = catalog();
        let m = model(&cat, Some(&config)).unwrap();
        assert_eq!(m.base_settings["weld_tolerance"], json!(2.5));
        let drafting = apply_preset(&cat, Some(&config), &m.base_settings, Preset2::Drafting);
        assert_eq!(drafting["keep_lv_poles"], b(true));
        assert!(!drafting.contains_key("not_a_setting"));
        let sketch = apply_preset(&cat, Some(&config), &m.base_settings, Preset2::Sketch);
        assert_eq!(
            sketch["keep_lv_poles"],
            b(true),
            "a mistyped project row cannot leak in"
        );
    }

    #[test]
    fn the_collapse_follows_the_engine_including_the_five_property_keep_keys() {
        let mut s = Settings::new();
        for key in [
            KEEP_LV_LINES_TOPOLOGY,
            KEEP_LV_LINES_LENGTH,
            KEEP_LV_LINES_SIZES,
            JOIN_LV_CABLES_OF_SAME_SIZE,
            KEEP_LV_POLES,
            CONNECT_UNCONNECTED_POLES,
            KEEP_SERVICE_CABLES,
            KEEP_SERVICE_CABLES_SIZES,
            EXTEND_SERVICE_CABLES,
            "keep_flying_stay",
            DESIRABLE_PROPERTIES,
        ] {
            s.insert(key.into(), b(false));
        }
        s.insert("keep_flying_stay".into(), b(true));
        s.insert(DESIRABLE_PROPERTIES.into(), b(true));
        let n = normalize(&s);
        assert_eq!(
            n[DESIRABLE_PROPERTIES],
            b(false),
            "keep_flying_stay alone preserves nothing"
        );
        s.insert(KEEP_LV_POLES.into(), b(true));
        assert_eq!(normalize(&s)[DESIRABLE_PROPERTIES], b(true));
        s.insert(KEEP_LV_LINES_TOPOLOGY.into(), b(false));
        s.insert(KEEP_LV_LINES_LENGTH.into(), b(true));
        let n = normalize(&s);
        assert_eq!(n[KEEP_LV_LINES_LENGTH], b(false));
        assert_eq!(n[KEEP_LV_LINES_SIZES], b(false));
        s.insert(KEEP_SERVICE_CABLES.into(), b(true));
        s.insert(EXTEND_SERVICE_CABLES.into(), b(true));
        let n = normalize(&s);
        assert_eq!(n[KEEP_SERVICE_CABLES_SIZES], b(true));
        assert_eq!(n[EXTEND_SERVICE_CABLES], b(false));
    }

    #[test]
    fn a_revealed_setting_reseeds_to_its_default_and_hidden_keys_leave_the_wire_named() {
        let cat = catalog();
        let base = model(&cat, None).unwrap().base_settings;
        let mut off = base.clone();
        off.insert(KEEP_LV_LINES_TOPOLOGY.into(), b(false));
        let off = normalize(&off);
        assert_eq!(off[KEEP_LV_LINES_SIZES], b(false));
        let mut changes = Settings::new();
        changes.insert(KEEP_LV_LINES_TOPOLOGY.into(), b(true));
        let on = change(&cat, &off, &changes);
        assert_eq!(
            on[KEEP_LV_LINES_SIZES],
            b(catalog_default(&cat, KEEP_LV_LINES_SIZES))
        );
        let mut dropped = Vec::new();
        let pruned = prune_hidden(&cat, &off, &mut dropped);
        // Hidden with a declared forced value: on the wire at that value, named.
        assert_eq!(pruned[KEEP_LV_LINES_SIZES], b(false));
        assert!(dropped.iter().any(|d| d.key == KEEP_LV_LINES_SIZES
            && d.why == "hidden_by_visibility_forced"
            && d.forced_to == Some(b(false))));
        // Hidden without one: omitted, named. The catalogue's numeric settings
        // never declare a forced value, so a hidden one is the case.
        let numeric_with_rules = cat
            .numeric_settings
            .iter()
            .find(|n| !n.visible_when.is_empty())
            .map(|n| n.key.clone());
        if let Some(key) = numeric_with_rules {
            let mut state = base.clone();
            for clause in visibility_rules(&cat, &key).unwrap() {
                // Break the first clause with a value of the same type.
                let broken = match &clause.equals {
                    Value::Bool(v) => b(!v),
                    other => json!(other.as_f64().unwrap_or(0.) + 1.),
                };
                state.insert(clause.key.clone(), broken);
            }
            let mut dropped = Vec::new();
            let pruned = prune_hidden(&cat, &state, &mut dropped);
            assert!(!pruned.contains_key(&key), "{key}");
            assert!(dropped
                .iter()
                .any(|d| d.key == key && d.why == "hidden_by_visibility"));
        }
    }

    #[test]
    fn the_wire_drops_lane_and_preset_hidden_keys_pins_geometry_and_stamps_sketch() {
        let cat = catalog();
        let base = model(&cat, None).unwrap().base_settings;
        let sketch = dialog_defaults(&apply_preset(&cat, None, &base, Preset2::Sketch));
        let (wire_fast, dropped) = wire(
            &cat,
            &sketch,
            DialogContext {
                lane: Lane::Fast,
                preset: Preset2::Sketch,
                use_firestore_design_data: false,
            },
        );
        assert_eq!(wire_fast[GEOMETRY_PROPERTIES], b(true));
        assert_eq!(wire_fast[CONNECT_UNCONNECTED_CUSTOMERS], b(false));
        for key in CUSTOMER_SOURCE_KEYS
            .iter()
            .chain(REPORT_ACTION_KEYS.iter())
            .chain(REPORT_DETAIL_KEYS.iter())
        {
            assert!(!wire_fast.contains_key(*key), "{key}");
        }
        assert!(dropped
            .iter()
            .any(|d| d.key == "export_reports_for_download"
                && d.why == "reporting_is_a_separate_fast_action"));
        let drafting = dialog_defaults(&apply_preset(&cat, None, &base, Preset2::Drafting));
        let (wire_std, _) = wire(
            &cat,
            &drafting,
            DialogContext {
                lane: Lane::Standard,
                preset: Preset2::Drafting,
                use_firestore_design_data: false,
            },
        );
        assert!(wire_std.contains_key("pull_customers_from_bldg"));
        assert!(
            !wire_std.contains_key(DESIRABLE_PROPERTIES),
            "browser-only flag never reaches Standard"
        );
        let groups = visible_groups(
            &model(&cat, None).unwrap().groups,
            DialogContext {
                lane: Lane::Fast,
                preset: Preset2::Sketch,
                use_firestore_design_data: true,
            },
        );
        assert!(groups
            .iter()
            .all(|g| !g.settings.iter().any(|s| s == INCLUDE_IN_COMBINED_REPORTS)));
    }

    #[test]
    fn the_default_preset_reads_the_template_first_then_the_project_type() {
        let config = json!({"sheets": {"project_settings": [{"parameter": "Default Process Preset", "value": "drafting"}]}});
        assert_eq!(
            default_preset(Some(&config), Some("assessment")),
            (Preset2::Drafting, "template")
        );
        assert_eq!(
            default_preset(None, Some("New Project")),
            (Preset2::Drafting, "project_type")
        );
        assert_eq!(
            default_preset(None, Some("as-built")),
            (Preset2::Sketch, "project_type")
        );
        assert_eq!(default_preset(None, None), (Preset2::Sketch, "fallback"));
    }

    #[test]
    fn canonical_names_keep_an_experimental_underscore() {
        assert_eq!(
            canonical_parameter_name(" Max feeder-cable size "),
            "max_feeder_cable_size"
        );
        assert_eq!(
            canonical_parameter_name("_experimental key"),
            "_experimental_key"
        );
        assert_eq!(canonical_parameter_name("___"), "");
    }

    #[test]
    fn the_door_refuses_what_it_should() {
        assert!(evaluate(br#"{"op":"normalize","settings":{"a":"x"}}"#).is_err());
        assert!(evaluate(br#"{"op":"teleport"}"#).is_err());
        let reply: Value = serde_json::from_str(&evaluate(br#"{"op":"normalize","settings":{"keep_lv_lines_topology":false,"keep_lv_lines_sizes":true}}"#).unwrap()).unwrap();
        assert_eq!(reply["settings"]["keep_lv_lines_sizes"], b(false));
    }
}
