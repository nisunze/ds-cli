//! What the layer drawer is true about, answered with the map closed.
//!
//! # The questions this module owns
//!
//! For one backend-authored layer document (the assembled `/layers`
//! response), the user's remembered toggles, the renderer mode and, when the
//! host has one, the camera zoom:
//!
//! * **identity** — which runtime layers form one canonical layer's *family*
//!   (a primary and its `__label` / `__boundary` / `__d3` companions), which
//!   role each plays, and which canonical id the reorder action accepts;
//! * **the plan** — which runtime layers mount, what each one's desired and
//!   runtime visibility is and why, and which of those the host records as
//!   the user's preference;
//! * **folds** — whether a set of layers reads as visible, partly visible or
//!   hidden, and what a parent toggle writes to its children;
//! * **transitions** — the preferences after a toggle, an explicit set, an
//!   isolation ("hide others") and its restore, and the runtime writes each
//!   implies;
//! * **projections** — the canonical catalogue and the matrix (by layer, by
//!   source) the drawer renders;
//! * **order** — whether an order request is acceptable against the document.
//!
//! The renderer is never consulted. Everything is decided from the document
//! and the preferences, so `ds` answers it identically with no map open.
//!
//! # Preferences
//!
//! `preferences` maps a runtime layer id to the visibility the user chose. A
//! layer without an entry inherits its family's remembered choice (the last
//! recorded member in document order, as the mount always did), then the
//! authored default (`layout.visibility`). After a mount plan every primary is
//! recorded; a companion is recorded once its family has a remembered choice.
//! Ids that are not in the document — *foreign*: temporary and analysis layers
//! toggled through the same store — keep their own entry and read as visible
//! when absent.
//!
//! An authored `visibility: none` is a hard hide the style author chose: a
//! remembered `true` still yields runtime `none`. A `__d3` companion renders
//! only in the 3D renderer with shapes enabled. Zoom is reported, never
//! applied: MapLibre already clips by `minzoom`/`maxzoom`, so `in_zoom_range`
//! is an answer for the drawer and for `ds`, not a runtime write.
//!
//! # Hosts
//!
//! The browser holds the document, the preferences and the isolation snapshot
//! in its stores, asks per gesture, persists the returned `preferences`, and
//! performs the returned `writes` on MapLibre when a map is mounted. `ds`
//! reads the document through the native client and the preferences through
//! the native layer store, then asks the same questions.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

pub const SCHEMA: &str = "ds.layer-state/v1";
pub const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_LAYERS: usize = 50_000;
pub const MAX_IDS: usize = 5_000;
pub const MAX_ORDERS: usize = 500;
pub const MAX_ORDER: i64 = 1_000_000;
pub const MAX_CATALOG_LIMIT: usize = 500;
/// Layers with no authored zoom range render at every zoom the map reaches.
pub const DEFAULT_MINZOOM: f64 = 0.0;
pub const DEFAULT_MAXZOOM: f64 = 25.0;

const COMPANION_SUFFIXES: [&str; 3] = ["__label", "__boundary", "__d3"];
const MATRIX_DOMAINS: [&str; 5] = ["survey", "design", "global", "user_data", "project_data"];

// ── the request ─────────────────────────────────────────────────────────

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    /// The project the caller believes it is asking about. The document must
    /// agree; a stale response after a project switch is refused here.
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    document: Option<Document>,
    /// The persisted preferences for this project.
    #[serde(default)]
    preferences: BTreeMap<String, bool>,
    /// The live toggles of the current session, if one is open. Used instead
    /// of `preferences` when it belongs to the same project.
    #[serde(default)]
    session: Option<Session>,
    #[serde(default)]
    mode: Mode,
    #[serde(default)]
    zoom: Option<f64>,
    #[serde(default)]
    isolation: Option<Isolation>,
    op: Op,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Session {
    project: String,
    preferences: BTreeMap<String, bool>,
}

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq)]
#[serde(deny_unknown_fields, default)]
pub struct Mode {
    pub renderer: Renderer,
    pub layer_3d_shapes: bool,
}

#[derive(Deserialize, Default, Clone, Copy, PartialEq, Eq)]
pub enum Renderer {
    #[default]
    #[serde(rename = "2d")]
    Flat,
    #[serde(rename = "3d")]
    Globe,
}

/// The map-wide "hide others" snapshot: which layers were managed when it was
/// taken and what each one's desired visibility was.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Isolation {
    pub layer_ids: Vec<String>,
    pub visibility: BTreeMap<String, bool>,
}

/// The backend-authored layer document. Unknown members are ignored so the
/// raw `/layers` response and a host's reduced projection both parse.
#[derive(Deserialize)]
struct Document {
    project_id: String,
    #[serde(default)]
    layers: Vec<LayerRow>,
    #[serde(default)]
    sources: Map<String, Value>,
    #[serde(default)]
    source_ids: Vec<String>,
    #[serde(default)]
    styles: Map<String, Value>,
    #[serde(default)]
    style_refs: Vec<String>,
    #[serde(default)]
    matrix: Value,
    #[serde(default)]
    platform_config: Value,
}

#[derive(Deserialize)]
struct LayerRow {
    #[serde(default)]
    id: String,
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(default)]
    source: String,
    #[serde(default, rename = "source-layer")]
    source_layer: String,
    #[serde(default)]
    style_ref: Value,
    #[serde(default)]
    metadata: Value,
    #[serde(default)]
    layout: Value,
    #[serde(default)]
    minzoom: Option<f64>,
    #[serde(default)]
    maxzoom: Option<f64>,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ScopeState {
    pub count: usize,
    pub any_visible: bool,
    pub all_visible: bool,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct Order {
    pub layer_id: String,
    pub order: i64,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Op {
    /// Identity of every document layer, plus the suffix-only role of any
    /// further runtime ids the host names.
    Classify {
        #[serde(default)]
        ids: Vec<String>,
    },
    /// The runtime ids a toggle on `ids` addresses: the ids themselves, their
    /// id-base siblings, their children and their style-family companions.
    Families { ids: Vec<String> },
    /// The mount plan: desired, runtime, mount and record per layer.
    Plan,
    /// How a set reads: count, any, all, and what a parent toggle writes.
    Scope {
        ids: Vec<String>,
        #[serde(default)]
        expand: bool,
    },
    /// The same fold over bare values the host holds for non-document layers.
    Fold { values: Vec<bool> },
    /// Combine independently owned scopes under one parent.
    Merge { scopes: Vec<ScopeState> },
    /// Flip a set: the family is expanded, and every member takes the value a
    /// parent toggle writes.
    Toggle { ids: Vec<String> },
    /// Set a set to an exact value.
    Set {
        ids: Vec<String>,
        visible: bool,
        #[serde(default)]
        expand: bool,
    },
    /// Hide every managed layer but `ids` and remember what was visible.
    Isolate { ids: Vec<String> },
    /// Return to the remembered visibility, if the snapshot still applies.
    Restore,
    /// Canonical rows with their families and folded visibility.
    Catalog {
        #[serde(default = "default_limit")]
        limit: usize,
    },
    /// The by-layer and by-source drawer projection per domain.
    Matrix,
    /// Whether an order request is acceptable against the document.
    Reorder { orders: Vec<Order> },
}

fn default_limit() -> usize {
    100
}

// ── the answer's vocabulary ─────────────────────────────────────────────

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Primary,
    Label,
    Boundary,
    D3,
    Companion,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SourceState {
    Present,
    Unknown,
    Missing,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    UserHidden,
    AuthoredHidden,
    #[serde(rename = "needs_3d_shapes")]
    Needs3dShapes,
}

// ── facts derived once per request ──────────────────────────────────────

struct Fact<'a> {
    id: &'a str,
    kind: &'a str,
    source: &'a str,
    canonical: Option<String>,
    parent: String,
    family: String,
    role: Role,
    companion: bool,
    authored_visible: bool,
    minzoom: f64,
    maxzoom: f64,
    source_state: SourceState,
    metadata: &'a Value,
    style_ref: String,
}

struct Facts<'a> {
    rows: Vec<Fact<'a>>,
    by_id: HashMap<&'a str, usize>,
    style_refs: BTreeSet<&'a str>,
}

fn text(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => {
            let trimmed = s.trim();
            (!trimmed.is_empty()).then(|| trimmed.to_owned())
        }
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn first_text(values: &[&Value]) -> String {
    values.iter().find_map(|v| text(v)).unwrap_or_default()
}

/// `roads__label` → `roads`; `roads__label__2` → `roads`; anything else unchanged.
pub fn strip_companion_suffix(raw: &str) -> &str {
    for suffix in COMPANION_SUFFIXES {
        if let Some(base) = raw.strip_suffix(suffix) {
            return base;
        }
        let marker = format!("{suffix}__");
        if let Some(index) = raw.find(&marker) {
            return &raw[..index];
        }
    }
    raw
}

fn suffix_role(id: &str, style_ref: &str, parent: &str, kind: &str) -> Role {
    let has = |token: &str| {
        style_ref.ends_with(token) || id.ends_with(token) || id.contains(&format!("{token}__"))
    };
    let companion = !parent.is_empty()
        || strip_companion_suffix(style_ref) != style_ref
        || strip_companion_suffix(id) != id;
    if has("__d3") || (!parent.is_empty() && kind == "fill-extrusion") {
        Role::D3
    } else if style_ref.ends_with("__label")
        || id.ends_with("__label")
        || (!parent.is_empty() && kind == "symbol")
    {
        Role::Label
    } else if has("__boundary") {
        Role::Boundary
    } else if companion {
        Role::Companion
    } else {
        Role::Primary
    }
}

impl<'a> Facts<'a> {
    fn build(document: &'a Document) -> Result<Self, String> {
        if document.layers.len() > MAX_LAYERS {
            return Err("layer document exceeds its layer bound".into());
        }
        let known_sources: Option<HashSet<&str>> = {
            let set: HashSet<&str> = document
                .sources
                .keys()
                .map(String::as_str)
                .chain(document.source_ids.iter().map(String::as_str))
                .collect();
            (!set.is_empty()).then_some(set)
        };
        let style_refs: BTreeSet<&str> = document
            .styles
            .keys()
            .map(String::as_str)
            .chain(document.style_refs.iter().map(String::as_str))
            .collect();
        let mut rows = Vec::with_capacity(document.layers.len());
        let mut by_id = HashMap::with_capacity(document.layers.len());
        for (index, row) in document.layers.iter().enumerate() {
            let id = row.id.as_str();
            if id.is_empty() {
                return Err("layer document has a layer without a runtime id".into());
            }
            if by_id.insert(id, index).is_some() {
                return Err(format!("layer document repeats the runtime id {id}"));
            }
            let metadata = &row.metadata;
            let style_ref = first_text(&[&row.style_ref, &metadata["style_ref"]]);
            let parent = text(&metadata["parent_layer"]).unwrap_or_default();
            let source_key = text(&metadata["source_key"]).unwrap_or_else(|| {
                format!("{}\u{1f}{}", row.source.trim(), row.source_layer.trim())
            });
            let logical = text(&metadata["layer_key"])
                .or_else(|| text(&metadata["entry_key"]))
                .or_else(|| {
                    let base = strip_companion_suffix(&style_ref);
                    (!base.is_empty()).then(|| base.to_owned())
                })
                .unwrap_or_else(|| strip_companion_suffix(id.trim()).to_owned());
            let role = suffix_role(id, &style_ref, &parent, &row.kind);
            let companion = role != Role::Primary;
            let source_state = match &known_sources {
                None => SourceState::Unknown,
                Some(set) if set.contains(row.source.as_str()) => SourceState::Present,
                Some(_) => SourceState::Missing,
            };
            rows.push(Fact {
                id,
                kind: &row.kind,
                source: &row.source,
                canonical: text(&metadata["config_layer_id"]),
                parent,
                family: format!("{source_key}\u{1f}{logical}"),
                role,
                companion,
                authored_visible: row.layout["visibility"] != "none",
                minzoom: row.minzoom.unwrap_or(DEFAULT_MINZOOM),
                maxzoom: row.maxzoom.unwrap_or(DEFAULT_MAXZOOM),
                source_state,
                metadata,
                style_ref,
            });
        }
        Ok(Self {
            rows,
            by_id,
            style_refs,
        })
    }

    fn get(&self, id: &str) -> Option<&Fact<'a>> {
        self.by_id.get(id).map(|index| &self.rows[*index])
    }

    /// `expandLayerIds`: the given ids, then every document layer that shares
    /// an id base, names a wanted parent, or is a companion in a wanted family.
    fn families(&self, ids: &[String]) -> (Vec<String>, Vec<String>) {
        let wanted: HashSet<&str> = ids
            .iter()
            .map(String::as_str)
            .filter(|id| !id.is_empty())
            .collect();
        let wanted_bases: HashSet<&str> =
            wanted.iter().map(|id| strip_companion_suffix(id)).collect();
        let wanted_families: HashSet<&str> = self
            .rows
            .iter()
            .filter(|fact| wanted.contains(fact.id))
            .map(|fact| fact.family.as_str())
            .collect();
        let mut seen = HashSet::new();
        let mut expanded = Vec::new();
        let mut foreign = Vec::new();
        for id in ids.iter().filter(|id| !id.is_empty()) {
            if seen.insert(id.as_str()) {
                expanded.push(id.clone());
                if !self.by_id.contains_key(id.as_str()) {
                    foreign.push(id.clone());
                }
            }
        }
        for fact in &self.rows {
            let member = wanted.contains(fact.id)
                || wanted_bases.contains(strip_companion_suffix(fact.id))
                || (!fact.parent.is_empty() && wanted.contains(fact.parent.as_str()))
                || (fact.companion && wanted_families.contains(fact.family.as_str()));
            if member && seen.insert(fact.id) {
                expanded.push(fact.id.to_owned());
            }
        }
        (expanded, foreign)
    }

    fn managed_ids(&self) -> Vec<String> {
        self.rows.iter().map(|fact| fact.id.to_owned()).collect()
    }
}

// ── remembered and effective visibility ────────────────────────────────

struct Remembered<'a> {
    preferences: &'a BTreeMap<String, bool>,
    /// The family's remembered choice: the last recorded member in document order.
    families: HashMap<String, bool>,
}

impl<'a> Remembered<'a> {
    fn new(facts: &Facts<'_>, preferences: &'a BTreeMap<String, bool>) -> Self {
        let mut families = HashMap::new();
        for fact in &facts.rows {
            if let Some(value) = preferences.get(fact.id) {
                families.insert(fact.family.clone(), *value);
            }
        }
        Self {
            preferences,
            families,
        }
    }
    fn remembered(&self, fact: &Fact<'_>) -> Option<bool> {
        self.preferences
            .get(fact.id)
            .copied()
            .or_else(|| self.families.get(fact.family.as_str()).copied())
    }
    fn desired(&self, fact: &Fact<'_>) -> bool {
        self.remembered(fact).unwrap_or(fact.authored_visible)
    }
    fn foreign_desired(&self, id: &str) -> bool {
        self.preferences.get(id).copied().unwrap_or(true)
    }
    fn desired_of(&self, facts: &Facts<'_>, id: &str) -> bool {
        match facts.get(id) {
            Some(fact) => self.desired(fact),
            None => self.foreign_desired(id),
        }
    }
}

fn runtime_reasons(fact: &Fact<'_>, desired: bool, mode: Mode) -> Vec<Reason> {
    let mut reasons = Vec::new();
    if !desired {
        reasons.push(Reason::UserHidden);
    }
    if !fact.authored_visible {
        reasons.push(Reason::AuthoredHidden);
    }
    if fact.role == Role::D3 && !(mode.renderer == Renderer::Globe && mode.layer_3d_shapes) {
        reasons.push(Reason::Needs3dShapes);
    }
    reasons
}

fn foreign_reasons(id: &str, desired: bool, mode: Mode) -> Vec<Reason> {
    let mut reasons = Vec::new();
    if !desired {
        reasons.push(Reason::UserHidden);
    }
    if id.ends_with("__d3") && !(mode.renderer == Renderer::Globe && mode.layer_3d_shapes) {
        reasons.push(Reason::Needs3dShapes);
    }
    reasons
}

fn runtime_word(reasons: &[Reason]) -> &'static str {
    if reasons.is_empty() {
        "visible"
    } else {
        "none"
    }
}

fn write(facts: &Facts<'_>, id: &str, desired: bool, mode: Mode) -> Value {
    let reasons = match facts.get(id) {
        Some(fact) => runtime_reasons(fact, desired, mode),
        None => foreign_reasons(id, desired, mode),
    };
    json!({"id": id, "runtime": runtime_word(&reasons), "reasons": reasons})
}

// ── folds ───────────────────────────────────────────────────────────────

pub fn fold(values: impl IntoIterator<Item = bool>) -> ScopeState {
    let mut count = 0;
    let mut visible = 0;
    for value in values {
        count += 1;
        if value {
            visible += 1;
        }
    }
    ScopeState {
        count,
        any_visible: visible > 0,
        all_visible: count > 0 && visible == count,
    }
}

pub fn merge(scopes: &[ScopeState]) -> ScopeState {
    let mut count = 0;
    let mut any_visible = false;
    let mut all_visible = true;
    for scope in scopes {
        if scope.count == 0 {
            continue;
        }
        count += scope.count;
        any_visible |= scope.any_visible;
        all_visible &= scope.all_visible;
    }
    ScopeState {
        count,
        any_visible,
        all_visible: count > 0 && all_visible,
    }
}

/// What a parent toggle writes to its children: a partly visible or fully
/// visible scope turns off; an off scope turns on; an empty scope writes nothing.
pub fn next_visibility(state: ScopeState) -> bool {
    state.count > 0 && !state.any_visible
}

fn scope_value(state: ScopeState) -> Value {
    json!({"count": state.count, "any_visible": state.any_visible, "all_visible": state.all_visible, "next": next_visibility(state)})
}

// ── the operations ──────────────────────────────────────────────────────

struct Context<'a> {
    facts: Facts<'a>,
    remembered: Remembered<'a>,
    preferences_source: &'static str,
    mode: Mode,
    zoom: Option<f64>,
    isolation: Option<&'a Isolation>,
}

impl<'a> Context<'a> {
    fn same_set(&self, snapshot: &Isolation) -> bool {
        let managed: HashSet<&str> = self.facts.rows.iter().map(|fact| fact.id).collect();
        snapshot.layer_ids.len() == managed.len()
            && snapshot
                .layer_ids
                .iter()
                .all(|id| managed.contains(id.as_str()))
    }

    fn isolation_active(&self) -> bool {
        self.isolation
            .is_some_and(|snapshot| self.same_set(snapshot))
    }

    fn in_zoom_range(&self, fact: &Fact<'_>) -> Value {
        match self.zoom {
            Some(zoom) => json!(zoom >= fact.minzoom && zoom < fact.maxzoom),
            None => Value::Null,
        }
    }

    fn classify(&self, ids: &[String]) -> Value {
        let layers: Vec<Value> = self
            .facts
            .rows
            .iter()
            .map(|fact| {
                json!({
                    "id": fact.id,
                    "canonical_id": fact.canonical,
                    "family": fact.family,
                    "role": fact.role,
                    "parent": if fact.parent.is_empty() { Value::Null } else { json!(fact.parent) },
                    "companion": fact.companion,
                    "authored_visible": fact.authored_visible,
                    "source": fact.source,
                    "source_state": fact.source_state,
                    "style_ref": if fact.style_ref.is_empty() { Value::Null } else { json!(fact.style_ref) },
                })
            })
            .collect();
        let named: Vec<Value> = ids
            .iter()
            .map(|id| match self.facts.get(id) {
                Some(fact) => json!({"id": id, "role": fact.role, "known": true}),
                None => json!({"id": id, "role": suffix_role(id, "", "", ""), "known": false}),
            })
            .collect();
        json!({"layers": layers, "ids": named})
    }

    fn plan(&self) -> Value {
        let mut layers = Vec::with_capacity(self.facts.rows.len());
        let mut preferences = BTreeMap::new();
        let mut writes = Vec::new();
        let mut visible_count = 0;
        let mut hidden_count = 0;
        for fact in &self.facts.rows {
            let remembered = self.remembered.remembered(fact);
            let desired = remembered.unwrap_or(fact.authored_visible);
            let reasons = runtime_reasons(fact, desired, self.mode);
            let hidden_label =
                fact.role == Role::Label && !fact.authored_visible && remembered.is_none();
            // A layer whose source is absent is neither mounted nor recorded; its
            // `source_state` says why, and `runtime` stays the visibility decision.
            let source_missing = fact.source_state == SourceState::Missing;
            let mount = !hidden_label && !source_missing;
            let record = mount && (!fact.companion || remembered.is_some());
            if record {
                preferences.insert(fact.id.to_owned(), desired);
            }
            let runtime = runtime_word(&reasons);
            if mount {
                writes.push(json!({"id": fact.id, "runtime": runtime, "reasons": reasons}));
                if runtime == "visible" {
                    visible_count += 1;
                } else {
                    hidden_count += 1;
                }
            }
            layers.push(json!({
                "id": fact.id,
                "canonical_id": fact.canonical,
                "family": fact.family,
                "role": fact.role,
                "desired": desired,
                "remembered": remembered,
                "runtime": runtime,
                "reasons": reasons,
                "mount": mount,
                "record": record,
                "source_state": fact.source_state,
                "in_zoom_range": self.in_zoom_range(fact),
            }));
        }
        json!({
            "preferences_source": self.preferences_source,
            "layers": layers,
            "preferences": preferences,
            "writes": writes,
            "visible_count": visible_count,
            "hidden_count": hidden_count,
            "isolation_active": self.isolation_active(),
        })
    }

    fn scope(&self, ids: &[String], expand: bool) -> Value {
        let (members, foreign) = if expand {
            self.facts.families(ids)
        } else {
            let mut seen = HashSet::new();
            let members: Vec<String> = ids
                .iter()
                .filter(|id| !id.is_empty() && seen.insert(id.as_str()))
                .cloned()
                .collect();
            let foreign = members
                .iter()
                .filter(|id| self.facts.get(id).is_none())
                .cloned()
                .collect();
            (members, foreign)
        };
        let state = fold(
            members
                .iter()
                .map(|id| self.remembered.desired_of(&self.facts, id)),
        );
        let mut value = scope_value(state);
        value["ids"] = json!(members);
        value["foreign"] = json!(foreign);
        value
    }

    fn apply(
        &self,
        members: &[String],
        foreign: Vec<String>,
        visible: bool,
        outcome: &str,
    ) -> Value {
        let mut preferences = self.remembered.preferences.clone();
        let mut changed = Vec::new();
        for id in members {
            if self.remembered.desired_of(&self.facts, id) != visible {
                changed.push(id.clone());
            }
            preferences.insert(id.clone(), visible);
        }
        let writes: Vec<Value> = members
            .iter()
            .map(|id| write(&self.facts, id, visible, self.mode))
            .collect();
        json!({
            "outcome": outcome,
            "visible": visible,
            "ids": members,
            "foreign": foreign,
            "preferences": preferences,
            "writes": writes,
            "changed": changed,
        })
    }

    fn noop(&self, reason: &str) -> Value {
        json!({
            "outcome": "noop",
            "reason": reason,
            "ids": [],
            "foreign": [],
            "preferences": self.remembered.preferences,
            "writes": [],
            "changed": [],
        })
    }

    fn toggle(&self, ids: &[String]) -> Value {
        let (members, foreign) = self.facts.families(ids);
        if members.is_empty() {
            return self.noop("nothing_to_toggle");
        }
        let state = fold(
            members
                .iter()
                .map(|id| self.remembered.desired_of(&self.facts, id)),
        );
        self.apply(&members, foreign, next_visibility(state), "applied")
    }

    fn set(&self, ids: &[String], visible: bool, expand: bool) -> Value {
        let (members, foreign) = if expand {
            self.facts.families(ids)
        } else {
            let mut seen = HashSet::new();
            let members: Vec<String> = ids
                .iter()
                .filter(|id| !id.is_empty() && seen.insert(id.as_str()))
                .cloned()
                .collect();
            let foreign = members
                .iter()
                .filter(|id| self.facts.get(id).is_none())
                .cloned()
                .collect();
            (members, foreign)
        };
        if members.is_empty() {
            return self.noop("nothing_to_set");
        }
        self.apply(&members, foreign, visible, "applied")
    }

    fn isolate(&self, ids: &[String]) -> Value {
        let managed = self.facts.managed_ids();
        let (keep, _) = self.facts.families(ids);
        let keep: HashSet<&str> = keep.iter().map(String::as_str).collect();
        if managed.is_empty() || keep.is_empty() {
            let mut value = self.noop("nothing_to_isolate");
            value["isolation"] = json!(self.isolation);
            return value;
        }
        let snapshot = Isolation {
            layer_ids: managed.clone(),
            visibility: managed
                .iter()
                .map(|id| (id.clone(), self.remembered.desired_of(&self.facts, id)))
                .collect(),
        };
        let mut preferences = self.remembered.preferences.clone();
        let mut changed = Vec::new();
        let mut writes = Vec::new();
        for id in &managed {
            let visible = keep.contains(id.as_str());
            if self.remembered.desired_of(&self.facts, id) != visible {
                changed.push(id.clone());
            }
            preferences.insert(id.clone(), visible);
            writes.push(write(&self.facts, id, visible, self.mode));
        }
        json!({
            "outcome": "isolated",
            "isolation": snapshot,
            "ids": managed,
            "kept": keep.into_iter().collect::<BTreeSet<_>>(),
            "preferences": preferences,
            "writes": writes,
            "changed": changed,
        })
    }

    fn restore(&self) -> Value {
        let Some(snapshot) = self.isolation else {
            let mut value = self.noop("no_snapshot");
            value["outcome"] = json!("no_snapshot");
            value["isolation"] = Value::Null;
            return value;
        };
        if !self.same_set(snapshot) {
            let mut value = self.noop("stale_snapshot");
            value["outcome"] = json!("stale_snapshot");
            value["isolation"] = Value::Null;
            return value;
        }
        let mut preferences = self.remembered.preferences.clone();
        let mut changed = Vec::new();
        let mut writes = Vec::new();
        for id in &snapshot.layer_ids {
            let visible = snapshot.visibility.get(id).copied().unwrap_or(false);
            if self.remembered.desired_of(&self.facts, id) != visible {
                changed.push(id.clone());
            }
            preferences.insert(id.clone(), visible);
            writes.push(write(&self.facts, id, visible, self.mode));
        }
        json!({
            "outcome": "restored",
            "isolation": Value::Null,
            "ids": snapshot.layer_ids,
            "preferences": preferences,
            "writes": writes,
            "changed": changed,
        })
    }

    fn catalog(&self, limit: usize) -> Result<Value, String> {
        if !(1..=MAX_CATALOG_LIMIT).contains(&limit) {
            return Err(format!("catalogue limit must be 1..={MAX_CATALOG_LIMIT}"));
        }
        let mut order: Vec<&str> = Vec::new();
        let mut families: BTreeMap<&str, Vec<&Fact<'_>>> = BTreeMap::new();
        for fact in &self.facts.rows {
            let Some(canonical) = fact.canonical.as_deref() else {
                continue;
            };
            if !families.contains_key(canonical) {
                order.push(canonical);
            }
            families.entry(canonical).or_default().push(fact);
        }
        let rows: Vec<Value> = order
            .iter()
            .take(limit)
            .map(|canonical| {
                let family = &families[canonical];
                let primary = family
                    .iter()
                    .find(|fact| fact.parent.is_empty())
                    .unwrap_or(&family[0]);
                let metadata = primary.metadata;
                let field = |key: &str| text(&metadata[key]);
                let runtime_ids: Vec<&str> = family.iter().map(|fact| fact.id).collect();
                let state = fold(family.iter().map(|fact| self.remembered.desired(fact)));
                let roles: BTreeMap<&str, Role> = family.iter().map(|fact| (fact.id, fact.role)).collect();
                let source_state = family.iter().map(|fact| fact.source_state).max().unwrap_or(SourceState::Unknown);
                let in_zoom_range = match self.zoom {
                    Some(zoom) => json!(family.iter().any(|fact| zoom >= fact.minzoom && zoom < fact.maxzoom)),
                    None => Value::Null,
                };
                json!({
                    "id": canonical,
                    "label": field("label").unwrap_or_else(|| (*canonical).to_owned()),
                    "class": field("layer_class").or_else(|| field("layer_type")).unwrap_or_else(|| "unknown".into()),
                    "geometry": field("geometry_type").or_else(|| (!primary.kind.is_empty()).then(|| primary.kind.to_owned())),
                    "order": metadata["order"].as_i64().unwrap_or(0),
                    "runtime_ids": runtime_ids,
                    "style_ref": if primary.style_ref.is_empty() { Value::Null } else { json!(primary.style_ref) },
                    "roles": roles,
                    "visibility": scope_value(state),
                    "source_state": source_state,
                    "in_zoom_range": in_zoom_range,
                })
            })
            .collect();
        Ok(json!({
            "layer_count": order.len(),
            "layers": rows,
            "more": order.len() > limit,
            "isolation_active": self.isolation_active(),
        }))
    }

    fn resolve_style_ref(&self, row: &Value, ids: &[String]) -> String {
        if let Some(reference) = text(&row["style_ref"]) {
            if self.facts.style_refs.contains(reference.as_str()) {
                return reference;
            }
        }
        for reference in row["style_refs"].as_array().into_iter().flatten() {
            if let Some(reference) = text(reference) {
                if self.facts.style_refs.contains(reference.as_str()) {
                    return reference;
                }
            }
        }
        for id in ids {
            if let Some(fact) = self.facts.get(id) {
                if !fact.style_ref.is_empty()
                    && self.facts.style_refs.contains(fact.style_ref.as_str())
                {
                    return fact.style_ref.clone();
                }
            }
        }
        String::new()
    }

    fn folded(&self, ids: &[String]) -> Value {
        let (members, _) = self.facts.families(ids);
        scope_value(fold(
            members
                .iter()
                .map(|id| self.remembered.desired_of(&self.facts, id)),
        ))
    }

    fn matrix(&self, document: &Document) -> Value {
        let labels: HashMap<&str, &str> = document.platform_config["domain_labels"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|entry| Some((entry["key"].as_str()?, entry["label"].as_str()?)))
            .collect();
        let mut domains = Vec::new();
        let mut total_rows = 0;
        for key in MATRIX_DOMAINS {
            let domain = &document.matrix[key];
            let Some(rows) = domain["rows"].as_array().filter(|rows| !rows.is_empty()) else {
                continue;
            };
            total_rows += rows.len();
            let row_ids = |row: &Value| -> Vec<String> {
                row["ids"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|id| id.as_str().filter(|id| !id.is_empty()).map(str::to_owned))
                    .collect()
            };
            let entries: Vec<Value> = rows
                .iter()
                .filter_map(|row| {
                    let ids = row_ids(row);
                    if ids.is_empty() {
                        return None;
                    }
                    let row_key = row["key"].as_str().unwrap_or_default();
                    let label = row["label"]
                        .as_str()
                        .filter(|label| !label.is_empty())
                        .unwrap_or(row_key);
                    Some(json!({
                        "key": row_key,
                        "label": label,
                        "layer_ids": ids,
                        "style_ref": self.resolve_style_ref(row, &ids),
                        "geometry_type": row["geometry_type"].as_str().unwrap_or_default(),
                        "visibility": self.folded(&ids),
                    }))
                })
                .collect();
            let sources: Vec<Value> = domain["sources"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
                .filter_map(|(index, source)| {
                    let source_key = source["key"].as_str().unwrap_or_default();
                    let provenance = |value: &mut Value| {
                        for field in ["created_at", "created_by", "file_name"] {
                            if !source[field].is_null() {
                                value[field] = source[field].clone();
                            }
                        }
                    };
                    let entries: Vec<Value> = rows
                        .iter()
                        .filter_map(|row| {
                            let id = row["ids"][index].as_str().filter(|id| !id.is_empty())?;
                            let row_key = row["key"].as_str().unwrap_or_default();
                            let style_ref = text(&row["style_refs"][index])
                                .filter(|reference| self.facts.style_refs.contains(reference.as_str()))
                                .unwrap_or_else(|| self.resolve_style_ref(row, &[id.to_owned()]));
                            let mut entry = json!({
                                "key": format!("{source_key}:{row_key}"),
                                "label": row["label"].as_str().filter(|label| !label.is_empty()).unwrap_or(row_key),
                                "layer_ids": [id],
                                "style_ref": style_ref,
                                "geometry_type": row["geometry_type"].as_str().unwrap_or_default(),
                                "visibility": self.folded(&[id.to_owned()]),
                            });
                            provenance(&mut entry);
                            Some(entry)
                        })
                        .collect();
                    if entries.is_empty() {
                        return None;
                    }
                    let layer_ids: Vec<String> = entries
                        .iter()
                        .flat_map(|entry| entry["layer_ids"].as_array().into_iter().flatten())
                        .filter_map(|id| id.as_str().map(str::to_owned))
                        .collect();
                    let mut group = json!({
                        "key": source_key,
                        "label": format_label(source["label"].as_str().filter(|label| !label.is_empty()).unwrap_or(source_key)),
                        "layer_ids": layer_ids,
                        "entries": entries,
                        "visibility": self.folded(&layer_ids),
                    });
                    provenance(&mut group);
                    Some(group)
                })
                .collect();
            domains.push(json!({
                "key": key,
                "label": labels.get(key).copied().unwrap_or(key),
                "row_count": rows.len(),
                "entries": entries,
                "sources": sources,
                "grouping_choice": sources.len() > 1,
            }));
        }
        json!({"domains": domains, "total_rows": total_rows, "has_data": !domains.is_empty()})
    }

    fn reorder(&self, orders: &[Order]) -> Value {
        let refuse = |code: &str, message: &str, ids: Vec<&str>| json!({"outcome": "refused", "code": code, "message": message, "ids": ids});
        if orders.is_empty() || orders.len() > MAX_ORDERS {
            return refuse(
                "invalid_order",
                &format!("an order request lists 1..={MAX_ORDERS} canonical layers"),
                vec![],
            );
        }
        let mut seen = HashSet::new();
        let mut duplicates = Vec::new();
        let mut invalid = Vec::new();
        for row in orders {
            let id = row.layer_id.as_str();
            if id.is_empty()
                || id.trim() != id
                || id.chars().count() > 500
                || id.chars().any(char::is_control)
                || !(-MAX_ORDER..=MAX_ORDER).contains(&row.order)
            {
                invalid.push(id);
            } else if !seen.insert(id) {
                duplicates.push(id);
            }
        }
        if !invalid.is_empty() {
            return refuse(
                "invalid_order",
                "layer ids must be trimmed, bounded text and orders bounded integers",
                invalid,
            );
        }
        if !duplicates.is_empty() {
            return refuse(
                "duplicate_layer",
                "each canonical layer takes one order",
                duplicates,
            );
        }
        let canonical: BTreeSet<&str> = self
            .facts
            .rows
            .iter()
            .filter_map(|fact| fact.canonical.as_deref())
            .collect();
        let unknown: Vec<&str> = orders
            .iter()
            .map(|row| row.layer_id.as_str())
            .filter(|id| !canonical.contains(id))
            .collect();
        if !unknown.is_empty() {
            return refuse("unknown_layer", "these ids are not canonical layers of the selected project; copy ids from the catalogue", unknown);
        }
        let unlisted: Vec<&str> = canonical
            .iter()
            .copied()
            .filter(|id| !seen.contains(id))
            .collect();
        json!({
            "outcome": "accepted",
            "orders": orders,
            "canonical_count": canonical.len(),
            "complete": unlisted.is_empty(),
            "unlisted": unlisted,
        })
    }
}

/// `formatLabel`: underscores to spaces, whitespace collapsed, ASCII words capitalised.
pub fn format_label(value: &str) -> String {
    let spaced: String = value
        .replace('_', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut out = String::with_capacity(spaced.len());
    let mut previous_word = false;
    for ch in spaced.chars() {
        let word = ch.is_ascii_alphanumeric();
        if word && !previous_word {
            out.extend(ch.to_uppercase());
        } else {
            out.push(ch);
        }
        previous_word = word;
    }
    out
}

pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("layer state request exceeds its bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|error| format!("invalid layer state request: {error}"))?;
    if request.schema != SCHEMA {
        return Err(format!("unsupported layer state schema; expected {SCHEMA}"));
    }
    let value = match &request.op {
        Op::Fold { values } => {
            if values.len() > MAX_LAYERS {
                return Err("fold exceeds its bound".into());
            }
            scope_value(fold(values.iter().copied()))
        }
        Op::Merge { scopes } => {
            if scopes.len() > MAX_LAYERS {
                return Err("merge exceeds its bound".into());
            }
            scope_value(merge(scopes))
        }
        op => {
            let document = request
                .document
                .as_ref()
                .ok_or("this layer state question needs the layer document")?;
            let project = request
                .project
                .as_deref()
                .ok_or("this layer state question needs the expected project")?;
            if document.project_id != project {
                return Err(format!(
                    "project_context_changed: the layer document belongs to {}, not {project}",
                    document.project_id
                ));
            }
            let facts = Facts::build(document)?;
            let (preferences, preferences_source) = match &request.session {
                Some(session) if session.project == project => (&session.preferences, "session"),
                _ => (&request.preferences, "persisted"),
            };
            let context = Context {
                remembered: Remembered::new(&facts, preferences),
                facts,
                preferences_source,
                mode: request.mode,
                zoom: request.zoom.filter(|zoom| zoom.is_finite()),
                isolation: request.isolation.as_ref(),
            };
            let ids_bound = |ids: &[String]| -> Result<(), String> {
                (ids.len() <= MAX_IDS)
                    .then_some(())
                    .ok_or_else(|| format!("at most {MAX_IDS} layer ids per question"))
            };
            match op {
                Op::Classify { ids } => {
                    ids_bound(ids)?;
                    context.classify(ids)
                }
                Op::Families { ids } => {
                    ids_bound(ids)?;
                    let (members, foreign) = context.facts.families(ids);
                    json!({"ids": members, "foreign": foreign})
                }
                Op::Plan => context.plan(),
                Op::Scope { ids, expand } => {
                    ids_bound(ids)?;
                    context.scope(ids, *expand)
                }
                Op::Toggle { ids } => {
                    ids_bound(ids)?;
                    context.toggle(ids)
                }
                Op::Set {
                    ids,
                    visible,
                    expand,
                } => {
                    ids_bound(ids)?;
                    context.set(ids, *visible, *expand)
                }
                Op::Isolate { ids } => {
                    ids_bound(ids)?;
                    context.isolate(ids)
                }
                Op::Restore => context.restore(),
                Op::Catalog { limit } => context.catalog(*limit)?,
                Op::Matrix => context.matrix(document),
                Op::Reorder { orders } => context.reorder(orders),
                Op::Fold { .. } | Op::Merge { .. } => unreachable!("handled above"),
            }
        }
    };
    serde_json::to_string(&value).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(id: &str, kind: &str, source: &str, metadata: Value) -> Value {
        json!({"id": id, "type": kind, "source": source, "source-layer": "s", "style_ref": id, "metadata": metadata, "paint": {"line-color": "#000"}})
    }

    /// Two canonical layers with the wire shape ds-brain's assembler emits: a
    /// primary and its `__label` / `__boundary` / `__d3` companions.
    fn document() -> Value {
        json!({
            "project_id": "p1",
            "sources": {"design_vt": {"type": "vector"}, "survey_geo": {"type": "geojson"}},
            "styles": {"roads": {}, "roads__label": {}, "roads__d3": {}, "poles": {}, "poles__boundary": {}, "lost": {}},
            "layers": [
                layer("roads", "line", "design_vt", json!({"config_layer_id": "design/roads", "label": "Roads", "layer_class": "design_tile", "geometry_type": "LineString", "order": 4, "source_key": "vt", "layer_key": "design/roads"})),
                layer("roads__label", "symbol", "design_vt", json!({"config_layer_id": "design/roads", "parent_layer": "roads", "label": "Roads Labels", "source_key": "vt", "layer_key": "design/roads"})).tap(|v| { v["layout"] = json!({"visibility": "none"}); }),
                layer("roads__d3", "fill-extrusion", "design_vt", json!({"config_layer_id": "design/roads", "parent_layer": "roads", "source_key": "vt", "layer_key": "design/roads"})),
                layer("poles", "circle", "survey_geo", json!({"config_layer_id": "survey/poles", "label": "Poles", "layer_class": "survey", "geometry_type": "Point", "order": 2, "source_key": "geojson", "layer_key": "survey/poles"})).tap(|v| { v["minzoom"] = json!(12); v["maxzoom"] = json!(20); }),
                layer("poles__boundary", "line", "survey_geo", json!({"config_layer_id": "survey/poles", "parent_layer": "poles", "source_key": "geojson", "layer_key": "survey/poles"})),
                layer("lost", "circle", "gone_source", json!({"config_layer_id": "survey/lost", "label": "Lost", "order": 9})),
                layer("notes", "circle", "survey_geo", json!({}))
            ],
            "matrix": {
                "design": {"rows": [{"key": "design/roads", "label": "Roads", "ids": ["roads"], "style_ref": "roads", "geometry_type": "LineString"}], "sources": [{"key": "vt", "label": "vector_tiles"}]},
                "survey": {"rows": [
                    {"key": "survey/poles", "label": "Poles", "ids": ["poles", null], "style_refs": ["poles", null], "geometry_type": "Point"},
                    {"key": "survey/empty", "label": "Empty", "ids": [null, null]}
                ], "sources": [{"key": "geojson", "label": "survey entries", "created_at": 5, "created_by": "a@b.c", "file_name": "x.geojson"}, {"key": "tiles", "label": "tiles"}]},
                "global": {"rows": []}
            },
            "platform_config": {"domain_labels": [{"key": "survey", "label": "Field survey"}]}
        })
    }

    trait Tap {
        fn tap(self, f: impl FnOnce(&mut Value)) -> Value;
    }
    impl Tap for Value {
        fn tap(mut self, f: impl FnOnce(&mut Value)) -> Value {
            f(&mut self);
            self
        }
    }

    fn ask(extra: Value, op: Value) -> Value {
        let mut request =
            json!({"schema": SCHEMA, "project": "p1", "document": document(), "op": op});
        for (key, value) in extra.as_object().unwrap() {
            request[key] = value.clone();
        }
        let out =
            evaluate(&serde_json::to_vec(&request).unwrap()).unwrap_or_else(|e| panic!("{e}"));
        serde_json::from_str(&out).unwrap()
    }

    fn refuse(request: Value) -> String {
        evaluate(&serde_json::to_vec(&request).unwrap()).unwrap_err()
    }

    fn ids(value: &Value) -> Vec<&str> {
        value
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect()
    }

    #[test]
    fn identity_classifies_roles_families_and_suffixes() {
        let out = ask(
            json!({}),
            json!({"kind": "classify", "ids": ["sketch__d3", "sketch__label__2", "plain"]}),
        );
        let layers = out["layers"].as_array().unwrap();
        let by = |id: &str| layers.iter().find(|l| l["id"] == id).unwrap();
        assert_eq!(by("roads")["role"], "primary");
        assert_eq!(by("roads__label")["role"], "label");
        assert_eq!(by("roads__d3")["role"], "d3");
        assert_eq!(by("poles__boundary")["role"], "boundary");
        assert_eq!(by("roads")["family"], by("roads__label")["family"]);
        assert_eq!(by("roads")["canonical_id"], "design/roads");
        assert_eq!(by("notes")["canonical_id"], Value::Null);
        assert_eq!(by("lost")["source_state"], "missing");
        assert_eq!(by("roads")["source_state"], "present");
        assert_eq!(by("roads__label")["authored_visible"], false);
        assert_eq!(
            out["ids"][0],
            json!({"id": "sketch__d3", "role": "d3", "known": false})
        );
        assert_eq!(
            out["ids"][1]["role"], "companion",
            "an infix label marker is a companion, not a label (matches the TypeScript predicate)"
        );
        assert_eq!(out["ids"][2]["role"], "primary");
        assert_eq!(strip_companion_suffix("roads__label__2"), "roads");
        assert_eq!(strip_companion_suffix("roads__boundary"), "roads");
        assert_eq!(strip_companion_suffix("roads"), "roads");
    }

    #[test]
    fn families_expand_by_id_base_parent_and_style_family_and_name_foreign_ids() {
        let out = ask(
            json!({}),
            json!({"kind": "families", "ids": ["roads", "", "ghost"]}),
        );
        assert_eq!(
            ids(&out["ids"]),
            ["roads", "ghost", "roads__label", "roads__d3"]
        );
        assert_eq!(ids(&out["foreign"]), ["ghost"]);
        // toggling a companion reaches its siblings through the id base
        let out = ask(
            json!({}),
            json!({"kind": "families", "ids": ["poles__boundary"]}),
        );
        assert_eq!(ids(&out["ids"]), ["poles__boundary", "poles"]);
    }

    #[test]
    fn the_plan_mounts_records_and_hides_as_the_mount_always_did() {
        let out = ask(json!({}), json!({"kind": "plan"}));
        let layers = out["layers"].as_array().unwrap();
        let by = |id: &str| layers.iter().find(|l| l["id"] == id).unwrap();
        // an authored-hidden label with nothing remembered is neither mounted nor recorded
        assert_eq!(by("roads__label")["mount"], false);
        assert_eq!(by("roads__label")["record"], false);
        assert_eq!(by("roads__label")["runtime"], "none");
        assert_eq!(
            by("roads__label")["reasons"],
            json!(["user_hidden", "authored_hidden"])
        );
        // a d3 companion mounts hidden in the flat renderer
        assert_eq!(by("roads__d3")["mount"], true);
        assert_eq!(by("roads__d3")["runtime"], "none");
        assert_eq!(by("roads__d3")["reasons"], json!(["needs_3d_shapes"]));
        assert_eq!(by("roads__d3")["record"], false);
        // a layer whose source is missing is skipped and says why
        assert_eq!(by("lost")["mount"], false);
        assert_eq!(by("lost")["source_state"], "missing");
        assert_eq!(
            by("lost")["runtime"],
            "visible",
            "runtime is the visibility decision; the source fact has its own field"
        );
        // primaries are always recorded, companions never without memory
        assert_eq!(
            out["preferences"],
            json!({"roads": true, "poles": true, "notes": true})
        );
        assert_eq!(out["preferences_source"], "persisted");
        assert_eq!(out["isolation_active"], false);
        assert!(out["writes"]
            .as_array()
            .unwrap()
            .iter()
            .all(|w| w["id"] != "lost" && w["id"] != "roads__label"));

        // a remembered primary carries its family: the hidden label now mounts and is recorded
        let out = ask(
            json!({"preferences": {"roads": false}}),
            json!({"kind": "plan"}),
        );
        let layers = out["layers"].as_array().unwrap();
        let by = |id: &str| layers.iter().find(|l| l["id"] == id).unwrap();
        assert_eq!(by("roads__label")["mount"], true);
        assert_eq!(by("roads__label")["desired"], false);
        assert_eq!(by("roads__label")["remembered"], false);
        assert_eq!(by("roads__label")["record"], true);
        assert_eq!(by("roads__d3")["record"], true);
        assert_eq!(out["preferences"]["roads__d3"], false);
        // a remembered true never beats an authored none
        let out = ask(
            json!({"preferences": {"roads__label": true}}),
            json!({"kind": "plan"}),
        );
        let label = out["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == "roads__label")
            .unwrap()
            .clone();
        assert_eq!(label["desired"], true);
        assert_eq!(label["runtime"], "none");
        assert_eq!(label["reasons"], json!(["authored_hidden"]));
    }

    #[test]
    fn the_session_wins_over_persisted_only_for_the_same_project_and_zoom_is_reported() {
        let out = ask(
            json!({"preferences": {"poles": false}, "session": {"project": "p1", "preferences": {"poles": true}}, "zoom": 10.0}),
            json!({"kind": "plan"}),
        );
        assert_eq!(out["preferences_source"], "session");
        let poles = out["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == "poles")
            .unwrap()
            .clone();
        assert_eq!(poles["desired"], true);
        assert_eq!(poles["in_zoom_range"], false);
        assert_eq!(
            poles["runtime"], "visible",
            "zoom is an answer, not a runtime write"
        );
        let out = ask(
            json!({"preferences": {"poles": false}, "session": {"project": "other", "preferences": {"poles": true}}, "zoom": 12.0}),
            json!({"kind": "plan"}),
        );
        assert_eq!(out["preferences_source"], "persisted");
        let poles = out["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == "poles")
            .unwrap()
            .clone();
        assert_eq!(poles["desired"], false);
        assert_eq!(poles["in_zoom_range"], true);
        let out = ask(
            json!({"mode": {"renderer": "3d", "layer_3d_shapes": true}}),
            json!({"kind": "plan"}),
        );
        let d3 = out["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["id"] == "roads__d3")
            .unwrap()
            .clone();
        assert_eq!(d3["runtime"], "visible");
    }

    #[test]
    fn folds_read_a_scope_and_say_what_a_parent_writes() {
        assert_eq!(
            fold([]),
            ScopeState {
                count: 0,
                any_visible: false,
                all_visible: false
            }
        );
        assert_eq!(
            fold([true, false]),
            ScopeState {
                count: 2,
                any_visible: true,
                all_visible: false
            }
        );
        assert!(!next_visibility(fold([true, false])));
        assert!(next_visibility(fold([false, false])));
        assert!(!next_visibility(fold([])));
        let merged = merge(&[fold([]), fold([true]), fold([false, false])]);
        assert_eq!(
            merged,
            ScopeState {
                count: 3,
                any_visible: true,
                all_visible: false
            }
        );
        let out = ask(
            json!({"preferences": {"roads": false, "roads__d3": false}}),
            json!({"kind": "scope", "ids": ["roads"], "expand": true}),
        );
        assert_eq!(ids(&out["ids"]), ["roads", "roads__label", "roads__d3"]);
        assert_eq!(out["count"], 3);
        assert_eq!(
            out["any_visible"], false,
            "the authored-hidden label follows its family, it is not visible by default"
        );
        assert_eq!(out["next"], true);
        let out = ask(
            json!({}),
            json!({"kind": "scope", "ids": ["ghost", "roads"]}),
        );
        assert_eq!(out["all_visible"], true);
        assert_eq!(ids(&out["foreign"]), ["ghost"]);
        let bare: Value = serde_json::from_str(
            &evaluate(
                &serde_json::to_vec(
                    &json!({"schema": SCHEMA, "op": {"kind": "fold", "values": [true, true]}}),
                )
                .unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(bare["all_visible"], true);
        assert_eq!(bare["next"], false);
    }

    #[test]
    fn toggle_set_isolate_and_restore_move_preferences_and_name_the_writes() {
        let out = ask(json!({}), json!({"kind": "toggle", "ids": ["roads"]}));
        assert_eq!(out["outcome"], "applied");
        assert_eq!(out["visible"], false);
        assert_eq!(
            out["preferences"],
            json!({"roads": false, "roads__label": false, "roads__d3": false})
        );
        assert_eq!(
            ids(&out["changed"]),
            ["roads", "roads__d3"],
            "the authored-hidden label was already hidden; the d3 companion was desired-visible"
        );
        assert_eq!(
            out["writes"][0],
            json!({"id": "roads", "runtime": "none", "reasons": ["user_hidden"]})
        );
        let out = ask(
            json!({"preferences": {"roads": false, "roads__label": false, "roads__d3": false}}),
            json!({"kind": "toggle", "ids": ["roads"]}),
        );
        assert_eq!(out["visible"], true);
        assert_eq!(
            out["writes"][1],
            json!({"id": "roads__label", "runtime": "none", "reasons": ["authored_hidden"]})
        );
        assert_eq!(out["writes"][2]["reasons"], json!(["needs_3d_shapes"]));
        let out = ask(json!({}), json!({"kind": "toggle", "ids": [""]}));
        assert_eq!(out["outcome"], "noop");
        // a foreign id is toggled on its own and keeps the d3 suffix rule
        let out = ask(
            json!({"preferences": {"sketch__d3": true}}),
            json!({"kind": "toggle", "ids": ["sketch__d3"]}),
        );
        assert_eq!(out["preferences"]["sketch__d3"], false);
        assert_eq!(ids(&out["foreign"]), ["sketch__d3"]);
        let out = ask(
            json!({}),
            json!({"kind": "set", "ids": ["poles"], "visible": false}),
        );
        assert_eq!(
            ids(&out["ids"]),
            ["poles"],
            "set does not expand unless asked"
        );
        let out = ask(
            json!({}),
            json!({"kind": "set", "ids": ["poles"], "visible": false, "expand": true}),
        );
        assert_eq!(ids(&out["ids"]), ["poles", "poles__boundary"]);

        let out = ask(
            json!({"preferences": {"poles": false}}),
            json!({"kind": "isolate", "ids": ["roads"]}),
        );
        assert_eq!(out["outcome"], "isolated");
        let snapshot = out["isolation"].clone();
        assert_eq!(
            ids(&snapshot["layer_ids"]),
            [
                "roads",
                "roads__label",
                "roads__d3",
                "poles",
                "poles__boundary",
                "lost",
                "notes"
            ]
        );
        assert_eq!(snapshot["visibility"]["poles"], false);
        assert_eq!(
            snapshot["visibility"]["roads__label"], false,
            "the snapshot records the effective desired value"
        );
        assert_eq!(out["preferences"]["notes"], false);
        assert_eq!(out["preferences"]["roads__d3"], true);
        assert_eq!(out["preferences"]["poles__boundary"], false);
        assert_eq!(ids(&out["kept"]), ["roads", "roads__d3", "roads__label"]);
        let restored = ask(
            json!({"preferences": out["preferences"].clone(), "isolation": snapshot.clone()}),
            json!({"kind": "restore"}),
        );
        assert_eq!(restored["outcome"], "restored");
        assert_eq!(restored["isolation"], Value::Null);
        assert_eq!(restored["preferences"]["poles"], false);
        assert_eq!(restored["preferences"]["notes"], true);
        assert_eq!(restored["preferences"]["roads__label"], false);
        let plan = ask(
            json!({"isolation": snapshot.clone()}),
            json!({"kind": "plan"}),
        );
        assert_eq!(plan["isolation_active"], true);
        let mut stale = snapshot.clone();
        stale["layer_ids"].as_array_mut().unwrap().pop();
        let out = ask(json!({"isolation": stale}), json!({"kind": "restore"}));
        assert_eq!(out["outcome"], "stale_snapshot");
        assert_eq!(out["writes"], json!([]));
        let out = ask(json!({}), json!({"kind": "restore"}));
        assert_eq!(out["outcome"], "no_snapshot");
        // isolating a foreign layer (an analysis result) hides every managed layer and keeps it
        let out = ask(json!({}), json!({"kind": "isolate", "ids": ["ghost"]}));
        assert_eq!(out["outcome"], "isolated");
        assert!(out["preferences"]
            .as_object()
            .unwrap()
            .values()
            .all(|v| v == false));
        assert_eq!(ids(&out["kept"]), ["ghost"]);
        let out = ask(json!({}), json!({"kind": "isolate", "ids": [""]}));
        assert_eq!(out["outcome"], "noop");
        assert_eq!(out["reason"], "nothing_to_isolate");
    }

    #[test]
    fn the_catalogue_groups_families_folds_visibility_and_truncates() {
        let out = ask(
            json!({"preferences": {"roads": false}, "zoom": 8.0}),
            json!({"kind": "catalog", "limit": 2}),
        );
        assert_eq!(out["layer_count"], 3);
        assert_eq!(out["more"], true);
        let roads = &out["layers"][0];
        assert_eq!(roads["id"], "design/roads");
        assert_eq!(roads["label"], "Roads");
        assert_eq!(roads["class"], "design_tile");
        assert_eq!(roads["order"], 4);
        assert_eq!(
            ids(&roads["runtime_ids"]),
            ["roads", "roads__label", "roads__d3"]
        );
        assert_eq!(roads["roles"]["roads__d3"], "d3");
        assert_eq!(roads["visibility"]["any_visible"], false);
        assert_eq!(roads["visibility"]["next"], true);
        assert_eq!(roads["source_state"], "present");
        assert_eq!(roads["in_zoom_range"], true);
        let poles = &out["layers"][1];
        assert_eq!(poles["visibility"]["all_visible"], true);
        assert_eq!(
            poles["in_zoom_range"], true,
            "the boundary companion has no zoom range, so the family renders at zoom 8"
        );
        let out = ask(json!({}), json!({"kind": "catalog", "limit": 10}));
        assert_eq!(out["layers"][2]["source_state"], "missing");
        assert_eq!(out["layers"][2]["class"], "unknown");
        assert!(refuse(json!({"schema": SCHEMA, "project": "p1", "document": document(), "op": {"kind": "catalog", "limit": 0}})).contains("limit"));
    }

    #[test]
    fn the_matrix_projects_domains_in_order_with_resolved_styles_and_provenance() {
        let out = ask(
            json!({"preferences": {"poles": false}}),
            json!({"kind": "matrix"}),
        );
        assert_eq!(out["has_data"], true);
        assert_eq!(out["total_rows"], 3);
        let domains = out["domains"].as_array().unwrap();
        assert_eq!(
            domains
                .iter()
                .map(|d| d["key"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["survey", "design"]
        );
        let survey = &domains[0];
        assert_eq!(survey["label"], "Field survey");
        assert_eq!(
            survey["entries"].as_array().unwrap().len(),
            1,
            "a row with no ids is dropped"
        );
        assert_eq!(survey["entries"][0]["style_ref"], "poles");
        assert_eq!(
            survey["entries"][0]["visibility"]["any_visible"], false,
            "the boundary follows its family"
        );
        let sources = survey["sources"].as_array().unwrap();
        assert_eq!(
            sources.len(),
            1,
            "a source column with no entries is dropped"
        );
        assert_eq!(sources[0]["label"], "Survey Entries");
        assert_eq!(sources[0]["created_by"], "a@b.c");
        assert_eq!(sources[0]["entries"][0]["key"], "geojson:survey/poles");
        assert_eq!(sources[0]["entries"][0]["file_name"], "x.geojson");
        assert_eq!(survey["grouping_choice"], false);
        assert_eq!(domains[1]["label"], "design");
        assert_eq!(domains[1]["sources"][0]["label"], "Vector Tiles");
        assert_eq!(format_label("lv_lines  extra"), "Lv Lines Extra");
        assert_eq!(format_label("  a-b "), "A-B");
    }

    #[test]
    fn reorder_names_invalid_duplicate_unknown_and_partial_outcomes() {
        let out = ask(
            json!({}),
            json!({"kind": "reorder", "orders": [{"layer_id": "design/roads", "order": 10}]}),
        );
        assert_eq!(out["outcome"], "accepted");
        assert_eq!(out["complete"], false);
        assert_eq!(ids(&out["unlisted"]), ["survey/lost", "survey/poles"]);
        assert_eq!(out["canonical_count"], 3);
        let out = ask(
            json!({}),
            json!({"kind": "reorder", "orders": [{"layer_id": "roads", "order": 1}]}),
        );
        assert_eq!(out["outcome"], "refused");
        assert_eq!(out["code"], "unknown_layer");
        assert_eq!(
            ids(&out["ids"]),
            ["roads"],
            "a runtime id is never an order key"
        );
        let out = ask(
            json!({}),
            json!({"kind": "reorder", "orders": [{"layer_id": "design/roads", "order": 1}, {"layer_id": "design/roads", "order": 2}]}),
        );
        assert_eq!(out["code"], "duplicate_layer");
        let out = ask(
            json!({}),
            json!({"kind": "reorder", "orders": [{"layer_id": " design/roads", "order": 1}]}),
        );
        assert_eq!(out["code"], "invalid_order");
        let out = ask(
            json!({}),
            json!({"kind": "reorder", "orders": [{"layer_id": "design/roads", "order": 1_000_001}]}),
        );
        assert_eq!(out["code"], "invalid_order");
        let out = ask(json!({}), json!({"kind": "reorder", "orders": []}));
        assert_eq!(out["code"], "invalid_order");
    }

    #[test]
    fn fences_refuse_a_stale_project_a_wrong_schema_and_a_missing_document() {
        let stale = refuse(
            json!({"schema": SCHEMA, "project": "p2", "document": document(), "op": {"kind": "plan"}}),
        );
        assert!(stale.starts_with("project_context_changed"), "{stale}");
        assert!(
            refuse(json!({"schema": "ds.other/v1", "op": {"kind": "fold", "values": []}}))
                .contains("schema")
        );
        assert!(refuse(json!({"schema": SCHEMA, "op": {"kind": "plan"}})).contains("document"));
        assert!(
            refuse(json!({"schema": SCHEMA, "document": document(), "op": {"kind": "plan"}}))
                .contains("project")
        );
        let mut duplicated = document();
        duplicated["layers"].as_array_mut().unwrap().push(layer(
            "roads",
            "line",
            "design_vt",
            json!({}),
        ));
        assert!(refuse(json!({"schema": SCHEMA, "project": "p1", "document": duplicated, "op": {"kind": "plan"}})).contains("repeats"));
        assert!(refuse(json!({"schema": SCHEMA, "project": "p1", "document": document(), "op": {"kind": "plan"}, "extra": 1})).contains("unknown field"));
        assert!(evaluate(&vec![b'x'; MAX_INPUT_BYTES + 1])
            .unwrap_err()
            .contains("bound"));
    }
}
