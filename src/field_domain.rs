//! What fields a project layer has, what values they carry, and what scalar
//! type each one is.
//!
//! # The question this module owns
//!
//! > *For this project layer, what fields exist, what values does each field
//! > carry, what scalar type does each field arrive as, and where did each of
//! > those answers come from?*
//!
//! It is answered over **canonical project data** — the design rooms, survey
//! entries and layer features a client already holds, plus the domains
//! ds-brain publishes. Never over rendered tiles. A renderer query answers a
//! different, lossy question ("what is painted at this pixel, at this zoom,
//! after tile simplification"), and off-screen features must exist here exactly
//! as on-screen features do.
//!
//! # Three answers, deliberately kept apart
//!
//! Conflating these is the defect this area already has, so every value the
//! report emits is tagged with which one it came from:
//!
//! | Answer | Owner | What it means |
//! |---|---|---|
//! | `declared` | **ds-brain** (`field_domains`), plus host-declared vocabularies (project known-column domains, sketch schemas) | the values this field is *allowed* to carry in this project. Passed through. This module never invents, widens or narrows a domain. |
//! | `present` | **ds-brain** (`present_values`, from tilestats) | the values the project's data is *known to contain*. Passed through. |
//! | `observed` | **this module** | what the canonical features the host holds actually carry. The **only** source of per-value counts, and the fallback source of a scalar type. |
//!
//! `field_domains` and `present_values` remain ds-brain's — contract
//! `ds-brain/docs/contracts/categorical-styling-foundation.md`. Nothing here
//! recomputes either.
//!
//! # Why the scalar type matters, and who owns it
//!
//! A MapLibre `match` compares its input and its labels **by type**: a `"1"`
//! label never hits a numeric `1` property, so the arm is dead and the feature
//! silently renders the fallback.
//!
//! ds-brain publishes a **declared** type — `FieldDomain.type`
//! (`categorical_string` | `categorical_int` | `categorical_number`) — but only
//! for fields that carry a known-column domain, which in practice means design
//! layers. Where it exists it **wins**, for the same reason `style_plan`'s
//! guided dimension already prefers it: a domain is what the field *is*, and
//! dirty data does not redefine it. Where it does not exist — survey layers,
//! uploaded `user_data`/`project_data`, any field without a catalog domain —
//! the type is derived from canonical features and tagged `observed`, and where
//! there are no canonical features it is **absent**, never guessed.
//!
//! `typeSource` reports which of the two answered, per field, and
//! `typeConflicts` names every field whose canonical data contradicts its
//! declared domain — that is dirty data worth surfacing, not a reason to
//! override the domain.
//!
//! # When the canonical data is not local
//!
//! A genuinely tiled layer has no in-memory feature set: its features live in
//! the tile pyramid, and the only thing that ever "saw" them was the renderer.
//! There is no correct local answer, so `observed` is a **typed refusal**
//! naming the reason. It never falls back to the renderer, and it never
//! fabricates a type. `declared` and `present` are still reported, because
//! ds-brain does publish those for tiled layers.

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

pub const REQUEST_SCHEMA: &str = "ds.field-domain.request/v1";
pub const REPORT_SCHEMA: &str = "ds.field-domain.report/v1";

/// Distinct values reported per field. Matches the bound the published
/// vocabulary already uses, so a report never widens what an editor can show.
pub const MAX_VALUES_PER_FIELD: usize = 200;
/// Fields reported. A layer past this is truncated, and says so in `more`.
pub const MAX_FIELDS: usize = 500;
/// Canonical features one call may observe. The host slices and sets
/// `truncated` when it holds more; the report repeats that verbatim.
pub const MAX_OBSERVED_FEATURES: usize = 20_000;
/// Request bound. Property bags only — hosts must not send geometry.
pub const MAX_BYTES: usize = 24 * 1024 * 1024;

/// Property keys that are geometry in disguise and never a style field.
const SKIP_KEYS: [&str; 2] = ["geometry", "geometry_wkt"];

/* ── scalar types ───────────────────────────────────────────────────────── */

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScalarType {
    String,
    Number,
    Boolean,
    /// Canonical features disagree. No label type is safe, so authoring
    /// surfaces must leave the authored labels alone.
    Mixed,
}

impl ScalarType {
    fn of(value: &Value) -> Option<Self> {
        match value {
            Value::String(_) => Some(Self::String),
            Value::Number(number) => number
                .as_f64()
                .filter(|n| n.is_finite())
                .map(|_| Self::Number),
            Value::Bool(_) => Some(Self::Boolean),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::String => "string",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Mixed => "mixed",
        }
    }
}

/// The text a scalar property presents as. `["get", f]` yields the value
/// itself; this is the stable key every surface addresses a value by, so it
/// must be the text the *product* uses — which is ECMAScript `String(value)`,
/// because that is what every client, style label and table cell already
/// shows. `1.0` is therefore `"1"`, not `"1.0"`.
fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Number(number) => {
            let float = number.as_f64()?;
            if !float.is_finite() {
                return None;
            }
            Some(js_number_text(float))
        }
        _ => None,
    }
}

/// ECMAScript `Number::toString`.
///
/// Not a stylistic choice. Rust's `Display` for `f64` never uses exponent
/// notation, so it spells `1e21` as `1000000000000000000000` and `1e-7` as
/// `0.0000001`, while every client, style label and table cell in the product
/// shows `1e+21` and `1e-7`. A value key that disagrees with the product's own
/// spelling is a value no authored `match` arm can address.
///
/// ECMAScript picks exponential form outside `[1e-6, 1e21)`; inside it, Rust's
/// shortest round-trip decimal already agrees digit for digit.
fn js_number_text(value: f64) -> String {
    if value == 0.0 {
        // Also catches -0.0, which ECMAScript spells "0".
        return "0".into();
    }
    let magnitude = value.abs();
    if (1e-6..1e21).contains(&magnitude) {
        return value.to_string();
    }
    let exponential = format!("{value:e}");
    match exponential.split_once('e') {
        // Rust omits the sign on a positive exponent; ECMAScript requires it.
        Some((mantissa, exponent)) if !exponent.starts_with('-') => {
            format!("{mantissa}e+{exponent}")
        }
        _ => exponential,
    }
}

/// A JSON number for a match label: integral values stay integers so a
/// retyped document is byte-comparable with the one ds-brain stores.
fn json_number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() <= i64::MAX as f64 {
        json!(value as i64)
    } else {
        serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
    }
}

/// The label a `match` arm must carry for `value` to hit a property of `type`.
///
/// This is a MapLibre comparison rule, not a formatting choice: an unknown or
/// `mixed` type keeps the string, because retyping a label the data may not
/// carry would create the dead arm this rule exists to prevent.
pub fn typed_match_label(value: &str, scalar: Option<ScalarType>) -> Value {
    match scalar {
        Some(ScalarType::Number) => {
            let trimmed = value.trim();
            match trimmed.parse::<f64>() {
                Ok(number) if number.is_finite() && !trimmed.is_empty() => {
                    match json_number(number) {
                        Value::Null => json!(value),
                        typed => typed,
                    }
                }
                _ => json!(value),
            }
        }
        Some(ScalarType::Boolean) if value == "true" => json!(true),
        Some(ScalarType::Boolean) if value == "false" => json!(false),
        _ => json!(value),
    }
}

/* ── request ────────────────────────────────────────────────────────────── */

/// Where the host's canonical features came from, or why there are none.
///
/// The host reports a fact; the kernel decides what that fact means. A host
/// that cannot say is refused rather than defaulted, because "no features"
/// and "features not loaded yet" and "this layer is tiled" have three
/// different correct answers.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
enum Canonical {
    /// The features in this request are the layer's canonical local set.
    #[serde(rename = "local")]
    Local {
        /// `design_room` | `survey_entries` | `sketch_layer` | `project_work`
        /// | `project_layer`. Free text: hosts name their own stores, and the
        /// kernel only echoes it as provenance.
        source: String,
        #[serde(default)]
        truncated: bool,
    },
    /// The layer renders from a vector-tile pyramid and holds no local
    /// feature set. There is no correct local answer.
    #[serde(rename = "tiled")]
    Tiled {
        #[serde(default)]
        source_layer: Option<String>,
    },
    /// The layer has a local store, but nothing is loaded into it yet.
    #[serde(rename = "not_loaded")]
    NotLoaded {
        #[serde(default)]
        source: Option<String>,
    },
}

/// One published or host-declared vocabulary. `values` is the declared domain;
/// `labels` are ds-brain's display labels for those values.
///
/// Deliberately NOT `deny_unknown_fields`: this is ds-brain's own `FieldDomain`
/// wire shape, which also carries `type`, `group_field`, `groups`, `aliases`,
/// `default` and `source`. Refusing those would refuse the contract.
#[derive(Debug, Clone, Default, Deserialize)]
struct Vocabulary {
    #[serde(default)]
    values: Vec<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "operation", deny_unknown_fields)]
enum Request {
    /// The full report over canonical project data.
    #[serde(rename = "observe")]
    Observe {
        schema: String,
        layer: String,
        canonical: Canonical,
        /// Property bags of the canonical features. Geometry is never sent.
        #[serde(default)]
        features: Vec<Value>,
        /// ds-brain `field_domains`, plus any host-declared vocabulary.
        #[serde(default)]
        declared: Map<String, Value>,
        /// ds-brain `present_values` (tilestats).
        #[serde(default)]
        present: Map<String, Value>,
        /// ds-brain `field_values` — the union it already publishes. Reported
        /// as declared, because a host cannot tell its two halves apart.
        #[serde(default)]
        published: Map<String, Value>,
        /// Fields the layer declares even with no values (`available_fields`).
        #[serde(default)]
        fields: Vec<Value>,
    },
    /// The typed match labels for values a report did not enumerate — an
    /// operator typing a value the canonical data does not yet carry.
    #[serde(rename = "labels")]
    Labels {
        schema: String,
        #[serde(default)]
        field_type: Option<ScalarType>,
        values: Vec<String>,
    },
    /// Whether an authored `["match", <field lookup>, …]` uses labels of a
    /// type the data never produces — every such arm is dead — and the
    /// repaired expression.
    #[serde(rename = "audit")]
    Audit {
        schema: String,
        expression: Value,
        #[serde(default)]
        types: Map<String, Value>,
    },
}

/* ── observation ────────────────────────────────────────────────────────── */

#[derive(Default)]
struct Observed {
    order: Vec<String>,
    values: std::collections::HashMap<String, Vec<String>>,
    counts: std::collections::HashMap<String, Map<String, Value>>,
    types: std::collections::HashMap<String, ScalarType>,
    overflowed: bool,
}

impl Observed {
    /// Fold one canonical feature's property bag. Accepts either a bare
    /// property map or a `{properties: {...}}` feature, because design rooms
    /// and survey entries store the same layer both ways.
    fn add(&mut self, feature: &Value) {
        let bag = match feature.get("properties") {
            Some(Value::Object(properties)) => properties,
            _ => match feature.as_object() {
                Some(object) => object,
                None => return,
            },
        };
        for (field, raw) in bag {
            if SKIP_KEYS.contains(&field.as_str()) {
                continue;
            }
            let Some(scalar) = ScalarType::of(raw) else {
                continue;
            };
            let Some(text) = scalar_text(raw) else {
                continue;
            };
            match self.types.get(field) {
                None => {
                    if self.order.len() >= MAX_FIELDS {
                        self.overflowed = true;
                        continue;
                    }
                    self.order.push(field.clone());
                    self.types.insert(field.clone(), scalar);
                    self.values.insert(field.clone(), Vec::new());
                    self.counts.insert(field.clone(), Map::new());
                }
                Some(&known) if known != scalar && known != ScalarType::Mixed => {
                    self.types.insert(field.clone(), ScalarType::Mixed);
                }
                Some(_) => {}
            }
            let (Some(values), Some(counts)) =
                (self.values.get_mut(field), self.counts.get_mut(field))
            else {
                continue;
            };
            match counts.get_mut(&text) {
                Some(Value::Number(seen)) => {
                    let next = seen.as_u64().unwrap_or(0).saturating_add(1);
                    *seen = next.into();
                }
                _ => {
                    if values.len() >= MAX_VALUES_PER_FIELD {
                        self.overflowed = true;
                        continue;
                    }
                    values.push(text.clone());
                    counts.insert(text, json!(1));
                }
            }
        }
    }
}

/* ── report assembly ────────────────────────────────────────────────────── */

struct Union {
    order: Vec<String>,
    values: std::collections::HashMap<String, Vec<String>>,
    seen: std::collections::HashMap<String, std::collections::HashSet<String>>,
    provenance: std::collections::HashMap<String, [bool; 3]>,
    overflowed: bool,
}

const DECLARED: usize = 0;
const PRESENT: usize = 1;
const OBSERVED: usize = 2;

impl Union {
    fn new() -> Self {
        Self {
            order: Vec::new(),
            values: std::collections::HashMap::new(),
            seen: std::collections::HashMap::new(),
            provenance: std::collections::HashMap::new(),
            overflowed: false,
        }
    }

    fn field(&mut self, field: &str) -> bool {
        if self.values.contains_key(field) {
            return true;
        }
        if self.order.len() >= MAX_FIELDS {
            self.overflowed = true;
            return false;
        }
        self.order.push(field.to_string());
        self.values.insert(field.to_string(), Vec::new());
        self.seen
            .insert(field.to_string(), std::collections::HashSet::new());
        self.provenance.insert(field.to_string(), [false; 3]);
        true
    }

    fn value(&mut self, field: &str, text: &str, origin: usize) {
        if !self.field(field) {
            return;
        }
        if let Some(marks) = self.provenance.get_mut(field) {
            marks[origin] = true;
        }
        let (Some(values), Some(seen)) = (self.values.get_mut(field), self.seen.get_mut(field))
        else {
            return;
        };
        if seen.contains(text) {
            return;
        }
        if values.len() >= MAX_VALUES_PER_FIELD {
            self.overflowed = true;
            return;
        }
        seen.insert(text.to_string());
        values.push(text.to_string());
    }

    /// Fold a published vocabulary map (`{field: [values] | {values, labels}}`).
    fn vocabulary(&mut self, source: &Map<String, Value>, origin: usize) {
        for (field, raw) in source {
            let field = field.trim();
            if field.is_empty() {
                continue;
            }
            let vocabulary: Vocabulary = match raw {
                Value::Array(values) => Vocabulary {
                    values: values.clone(),
                },
                Value::Object(_) => serde_json::from_value(raw.clone()).unwrap_or_default(),
                _ => continue,
            };
            if !self.field(field) {
                continue;
            }
            for value in &vocabulary.values {
                if let Some(text) = scalar_text(value) {
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        self.value(field, &text, origin);
                    }
                }
            }
        }
    }
}

/// ds-brain's known-column domain type, as a MapLibre comparison type.
///
/// The vocabulary is `FieldDomain.type` — `categorical_string`,
/// `categorical_int`, `categorical_number` — plus the `numeric` and plain
/// scalar spellings host vocabularies use. `style_plan`'s guided dimension
/// already folds the same table; this is the one place it now lives.
fn declared_scalar(entry: &Value) -> Option<ScalarType> {
    match entry.get("type")?.as_str()? {
        "categorical_string" | "string" | "text" => Some(ScalarType::String),
        "categorical_int" | "categorical_number" | "numeric" | "number" | "int" | "integer"
        | "float" => Some(ScalarType::Number),
        "boolean" | "bool" => Some(ScalarType::Boolean),
        _ => None,
    }
}

fn declared_types(declared: &Map<String, Value>) -> std::collections::HashMap<String, ScalarType> {
    declared
        .iter()
        .filter_map(|(field, entry)| Some((field.trim().to_string(), declared_scalar(entry)?)))
        .filter(|(field, _)| !field.is_empty())
        .collect()
}

fn label_map(source: &Map<String, Value>) -> Map<String, Value> {
    let mut out = Map::new();
    for (field, raw) in source {
        if let Value::Object(entry) = raw {
            if let Some(Value::Object(labels)) = entry.get("labels") {
                if !labels.is_empty() {
                    out.insert(field.clone(), Value::Object(labels.clone()));
                }
            }
        }
    }
    out
}

/// The typed refusal for a layer whose canonical data is not local.
///
/// Named, not silent, and never a fallback to the renderer: a value outside
/// the viewport, below minzoom or dropped by tile simplification would be
/// invisible to such a fallback, so two operators would get two answers to a
/// question that is not about the picture.
fn refusal(canonical: &Canonical) -> Option<Value> {
    match canonical {
        Canonical::Local { .. } => None,
        Canonical::Tiled { source_layer } => Some(json!({
            "status": "refused",
            "code": "layer_is_tiled",
            "detail": "this layer renders from a vector-tile pyramid and holds no canonical local feature set, so no scalar type, value count or observed value can be derived for it",
            "remedy": "read the published domain and presence instead; a property type for a tiled layer needs a publisher, not a renderer scan",
            "sourceLayer": source_layer,
        })),
        Canonical::NotLoaded { source } => Some(json!({
            "status": "refused",
            "code": "canonical_features_not_loaded",
            "detail": "this layer's canonical features are not loaded on this host yet, so nothing can be observed from them",
            "remedy": "load the layer's canonical data, then ask again",
            "source": source,
        })),
    }
}

fn observe(
    layer: &str,
    canonical: &Canonical,
    features: &[Value],
    declared: &Map<String, Value>,
    present: &Map<String, Value>,
    published: &Map<String, Value>,
    fields: &[Value],
) -> Result<Value, String> {
    if features.len() > MAX_OBSERVED_FEATURES {
        return Err(format!(
            "a field-domain observation accepts at most {MAX_OBSERVED_FEATURES} canonical features per call"
        ));
    }
    let refused = refusal(canonical);
    if refused.is_some() && !features.is_empty() {
        return Err("canonical features were supplied for a layer declared as having none".into());
    }

    let mut union = Union::new();
    // Declared fields first: a field with a domain but no values is still a
    // field the operator may style by.
    for field in fields {
        if let Some(name) = field.as_str() {
            let name = name.trim();
            if !name.is_empty() {
                union.field(name);
            }
        }
    }
    union.vocabulary(declared, DECLARED);
    union.vocabulary(published, DECLARED);
    union.vocabulary(present, PRESENT);

    let mut observed = Observed::default();
    if refused.is_none() {
        for feature in features {
            observed.add(feature);
        }
        for field in &observed.order.clone() {
            for text in observed.values.get(field).cloned().unwrap_or_default() {
                union.value(field, &text, OBSERVED);
            }
        }
    }

    let mut values = Map::new();
    let mut provenance = Map::new();
    for field in &union.order {
        if let Some(list) = union.values.get(field) {
            if !list.is_empty() {
                values.insert(field.clone(), json!(list));
            }
        }
        let marks = union.provenance.get(field).copied().unwrap_or([false; 3]);
        provenance.insert(
            field.clone(),
            json!({
                "declared": marks[DECLARED],
                "present": marks[PRESENT],
                "observed": marks[OBSERVED],
            }),
        );
    }

    // The declared domain type wins wherever ds-brain publishes one; a field's
    // domain is what it IS, and dirty data does not redefine it.
    let published_types = declared_types(declared);
    let mut types = Map::new();
    let mut type_source = Map::new();
    let mut conflicts = Vec::new();
    let mut counts = Map::new();
    let mut labels = Map::new();
    for field in &union.order {
        let seen = observed.types.get(field).copied();
        let scalar = match (published_types.get(field).copied(), seen) {
            (Some(declared_type), Some(observed_type)) => {
                if declared_type != observed_type && observed_type != ScalarType::Mixed {
                    conflicts.push(json!({
                        "field": field,
                        "declared": declared_type.name(),
                        "observed": observed_type.name(),
                    }));
                }
                type_source.insert(field.clone(), json!("declared"));
                Some(declared_type)
            }
            (Some(declared_type), None) => {
                type_source.insert(field.clone(), json!("declared"));
                Some(declared_type)
            }
            (None, Some(observed_type)) => {
                type_source.insert(field.clone(), json!("observed"));
                Some(observed_type)
            }
            (None, None) => None,
        };
        if let Some(scalar) = scalar {
            types.insert(field.clone(), json!(scalar.name()));
        }
        if let Some(bucket) = observed.counts.get(field) {
            counts.insert(field.clone(), Value::Object(bucket.clone()));
        }
        // Every value the report offers gets its typed label, so an authoring
        // surface never has to re-derive one.
        let mut typed = Map::new();
        for text in union.values.get(field).into_iter().flatten() {
            typed.insert(text.clone(), typed_match_label(text, scalar));
        }
        if !typed.is_empty() {
            labels.insert(field.clone(), Value::Object(typed));
        }
    }

    let observation = match refused {
        Some(refusal) => refusal,
        None => {
            let (source, truncated) = match canonical {
                Canonical::Local { source, truncated } => (source.clone(), *truncated),
                _ => unreachable!("a refused canonical source never reaches here"),
            };
            json!({
                "status": "derived",
                "source": source,
                "features": features.len(),
                "truncated": truncated,
            })
        }
    };

    Ok(json!({
        "schema": REPORT_SCHEMA,
        "layer": layer,
        "fields": union.order,
        "values": values,
        "valueLabels": label_map(declared),
        "types": types,
        "typeSource": type_source,
        "typeConflicts": conflicts,
        "counts": counts,
        "matchLabels": labels,
        "provenance": provenance,
        "observed": observation,
        "more": union.overflowed || observed.overflowed,
    }))
}

/* ── audit ──────────────────────────────────────────────────────────────── */

/// The field an expression reads, plus the aliases a `coalesce` also reads.
/// Mirrors `["get", f]`, `["coalesce", ["get", a], ["get", b], …]` and the
/// `["to-string", …]` wrapper the authoring surfaces emit.
fn field_lookup(value: &Value) -> Option<(String, Vec<String>)> {
    let array = value.as_array()?;
    match array.first()?.as_str()? {
        "get" => {
            let field = array.get(1)?.as_str()?.to_string();
            Some((field, Vec::new()))
        }
        "to-string" | "to-number" | "to-boolean" => field_lookup(array.get(1)?),
        "coalesce" => {
            let mut found: Vec<String> = Vec::new();
            for item in array.iter().skip(1) {
                if let Some((field, aliases)) = field_lookup(item) {
                    found.push(field);
                    found.extend(aliases);
                }
            }
            let mut iterator = found.into_iter();
            let first = iterator.next()?;
            Some((first, iterator.collect()))
        }
        _ => None,
    }
}

fn arm_label_type(label: &Value) -> Option<&'static str> {
    match label {
        Value::String(_) => Some("string"),
        Value::Number(_) => Some("number"),
        Value::Bool(_) => Some("boolean"),
        _ => None,
    }
}

fn parse_type(value: &Value) -> Option<ScalarType> {
    match value.as_str()? {
        "string" => Some(ScalarType::String),
        "number" => Some(ScalarType::Number),
        "boolean" => Some(ScalarType::Boolean),
        "mixed" => Some(ScalarType::Mixed),
        _ => None,
    }
}

/// A `match` expression's arms: `["match", input, l0, o0, l1, o1, …, fallback]`.
/// A well-formed one has an odd length of at least 5.
fn match_arms(expression: &Value) -> Option<&Vec<Value>> {
    let array = expression.as_array()?;
    (array.first()?.as_str()? == "match" && array.len() >= 5).then_some(array)
}

fn audit(expression: &Value, types: &Map<String, Value>) -> Value {
    let Some(array) = match_arms(expression) else {
        return json!({ "mismatch": null, "repaired": null });
    };
    let Some((field, aliases)) = field_lookup(&array[1]) else {
        return json!({ "mismatch": null, "repaired": null });
    };
    let expected = types.get(&field).and_then(parse_type).or_else(|| {
        aliases
            .iter()
            .find_map(|a| types.get(a).and_then(parse_type))
    });
    let Some(expected) = expected.filter(|t| *t != ScalarType::Mixed) else {
        return json!({ "mismatch": null, "repaired": null });
    };

    let mut authored: std::collections::BTreeSet<&'static str> = Default::default();
    let mut labels = 0_u64;
    let mut index = 2;
    while index < array.len() - 1 {
        let arm = &array[index];
        let group: Vec<&Value> = match arm.as_array() {
            Some(items) => items.iter().collect(),
            None => vec![arm],
        };
        for label in group {
            if let Some(kind) = arm_label_type(label) {
                authored.insert(kind);
                labels += 1;
            }
        }
        index += 2;
    }
    if authored.len() != 1 {
        return json!({ "mismatch": null, "repaired": null });
    }
    let actual = *authored.iter().next().expect("exactly one authored type");
    if actual == expected.name() {
        return json!({ "mismatch": null, "repaired": null });
    }
    json!({
        "mismatch": {
            "field": field,
            "expected": expected.name(),
            "actual": actual,
            "labels": labels,
        },
        "repaired": retype(expression, expected),
    })
}

/// Rewrite every label of a field match to `scalar`. Arm outputs and the
/// fallback are untouched — only the comparison side is retyped.
pub fn retype(expression: &Value, scalar: ScalarType) -> Value {
    let Some(array) = match_arms(expression) else {
        return expression.clone();
    };
    if scalar == ScalarType::Mixed {
        return expression.clone();
    }
    let relabel = |label: &Value| match scalar_text(label) {
        Some(text) => typed_match_label(&text, Some(scalar)),
        None => label.clone(),
    };
    let mut next = vec![json!("match"), array[1].clone()];
    let mut index = 2;
    while index < array.len() - 1 {
        let arm = &array[index];
        next.push(match arm.as_array() {
            Some(items) => Value::Array(items.iter().map(relabel).collect()),
            None => relabel(arm),
        });
        next.push(array[index + 1].clone());
        index += 2;
    }
    next.push(array[array.len() - 1].clone());
    Value::Array(next)
}

/* ── entry point ────────────────────────────────────────────────────────── */

pub fn evaluate(input: &[u8]) -> Result<Value, String> {
    if input.is_empty() || input.len() > MAX_BYTES {
        return Err("field-domain request exceeds its 24 MiB bound".into());
    }
    let request: Request = serde_json::from_slice(input)
        .map_err(|_| "field-domain request does not match its closed contract".to_string())?;
    match request {
        Request::Observe {
            schema,
            layer,
            canonical,
            features,
            declared,
            present,
            published,
            fields,
        } => {
            if schema != REQUEST_SCHEMA {
                return Err("field-domain request schema is unsupported".into());
            }
            observe(
                &layer, &canonical, &features, &declared, &present, &published, &fields,
            )
        }
        Request::Labels {
            schema,
            field_type,
            values,
        } => {
            if schema != REQUEST_SCHEMA {
                return Err("field-domain request schema is unsupported".into());
            }
            if values.len() > MAX_VALUES_PER_FIELD {
                return Err(format!(
                    "a field-domain label request accepts at most {MAX_VALUES_PER_FIELD} values"
                ));
            }
            let mut labels = Map::new();
            for value in values {
                let typed = typed_match_label(&value, field_type);
                labels.insert(value, typed);
            }
            Ok(json!({ "schema": REPORT_SCHEMA, "labels": labels }))
        }
        Request::Audit {
            schema,
            expression,
            types,
        } => {
            if schema != REQUEST_SCHEMA {
                return Err("field-domain request schema is unsupported".into());
            }
            Ok(audit(&expression, &types))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(value: Value) -> Result<Value, String> {
        evaluate(&serde_json::to_vec(&value).unwrap())
    }

    fn local(features: Value) -> Result<Value, String> {
        command(json!({
            "schema": REQUEST_SCHEMA, "operation": "observe", "layer": "master/lv_poles",
            "canonical": {"kind": "local", "source": "design_room"},
            "features": features,
        }))
    }

    /// The corpus the deleted TypeScript scan pinned, answered from canonical
    /// features instead of from what the renderer happened to be painting.
    #[test]
    fn reports_fields_values_counts_and_the_scalar_type_canonical_data_carries() {
        let report = local(json!([
            {"properties": {"struct_type": "S140", "stay": 1, "earthing": 0, "pole_number": "P1", "geometry": "x"}},
            {"properties": {"struct_type": "S190", "stay": 0, "earthing": 0, "pole_number": "P2"}},
            {"properties": {"struct_type": "S140", "stay": 1, "earthing": 1, "flagged": true}},
            {"properties": Value::Null},
        ]))
        .unwrap();
        assert_eq!(
            report["fields"],
            json!(["struct_type", "stay", "earthing", "pole_number", "flagged"])
        );
        assert_eq!(report["values"]["stay"], json!(["1", "0"]));
        assert_eq!(
            report["types"],
            json!({"struct_type":"string","stay":"number","earthing":"number","pole_number":"string","flagged":"boolean"})
        );
        assert_eq!(report["counts"]["struct_type"], json!({"S140":2,"S190":1}));
        assert_eq!(report["counts"]["stay"], json!({"1":2,"0":1}));
        assert_eq!(report["observed"]["status"], "derived");
        assert_eq!(report["observed"]["features"], 4);
        // `geometry` is never a style field.
        assert!(!report["fields"]
            .as_array()
            .unwrap()
            .contains(&json!("geometry")));
    }

    #[test]
    fn a_field_whose_canonical_features_disagree_is_mixed_and_keeps_its_labels() {
        let report =
            local(json!([{"properties":{"level":1}}, {"properties":{"level":"two"}}])).unwrap();
        assert_eq!(report["types"]["level"], "mixed");
        assert_eq!(report["matchLabels"]["level"]["1"], json!("1"));
        assert_eq!(typed_match_label("1", Some(ScalarType::Mixed)), json!("1"));
    }

    #[test]
    fn types_match_labels_the_way_maplibre_compares_them() {
        assert_eq!(typed_match_label("1", Some(ScalarType::Number)), json!(1));
        assert_eq!(
            typed_match_label("abc", Some(ScalarType::Number)),
            json!("abc")
        );
        assert_eq!(typed_match_label("", Some(ScalarType::Number)), json!(""));
        assert_eq!(
            typed_match_label("true", Some(ScalarType::Boolean)),
            json!(true)
        );
        assert_eq!(
            typed_match_label("S140", Some(ScalarType::String)),
            json!("S140")
        );
        assert_eq!(typed_match_label("1", None), json!("1"));
        let labels = command(json!({
            "schema": REQUEST_SCHEMA, "operation": "labels",
            "field_type": "number", "values": ["0", "1"],
        }))
        .unwrap();
        assert_eq!(labels["labels"], json!({"0": 0, "1": 1}));
    }

    #[test]
    fn detects_dead_arms_and_repairs_them() {
        let types = local(json!([
            {"properties": {"stay": 1}},
            {"properties": {"stay": 0}}
        ]))
        .unwrap()["types"]
            .clone();
        let authored = json!(["match", ["get", "stay"], "1", 2.5, "0", 0, 0]);
        let verdict = command(json!({
            "schema": REQUEST_SCHEMA, "operation": "audit",
            "expression": authored, "types": types,
        }))
        .unwrap();
        assert_eq!(
            verdict["mismatch"],
            json!({"field":"stay","expected":"number","actual":"string","labels":2})
        );
        assert_eq!(
            verdict["repaired"],
            json!(["match", ["get", "stay"], 1, 2.5, 0, 0, 0])
        );
    }

    #[test]
    fn agreeing_types_unknown_fields_and_grouped_labels_are_quiet() {
        let types = json!({"stay": "number"});
        for expression in [
            json!(["match", ["get", "stay"], [1, 0], 2, 0]),
            json!(["match", ["get", "unknown"], "a", 1, 0]),
            json!(["match", ["get", "stay"], [1, "0"], 2, 0]),
            json!(["interpolate", ["linear"], ["get", "stay"], 0, 1]),
            json!("#ff0000"),
        ] {
            let verdict = command(json!({
                "schema": REQUEST_SCHEMA, "operation": "audit",
                "expression": expression, "types": types,
            }))
            .unwrap();
            assert_eq!(verdict["mismatch"], Value::Null, "{expression}");
            assert_eq!(verdict["repaired"], Value::Null);
        }
        let coalesce = command(json!({
            "schema": REQUEST_SCHEMA, "operation": "audit",
            "expression": ["match", ["coalesce", ["get", "stay"], ["get", "x"]], "1", 1, 0],
            "types": types,
        }))
        .unwrap();
        assert_eq!(coalesce["mismatch"]["field"], "stay");
    }

    #[test]
    fn a_tiled_layer_is_refused_by_name_never_scanned_and_never_guessed() {
        let report = command(json!({
            "schema": REQUEST_SCHEMA, "operation": "observe", "layer": "design_vt/lv_lines",
            "canonical": {"kind": "tiled", "source_layer": "lv_lines"},
            "declared": {
                "cable_size": {"type": "categorical_number", "values": ["35", "50"], "labels": {"35": "35 mm²"}},
                "installer": {"values": ["acme"]}
            },
            "present": {"cable_size": ["35"]},
        }))
        .unwrap();
        assert_eq!(report["observed"]["status"], "refused");
        assert_eq!(report["observed"]["code"], "layer_is_tiled");
        assert!(report["observed"]["remedy"].is_string());
        // The domain and the presence ds-brain publishes still reach the caller.
        assert_eq!(report["values"]["cable_size"], json!(["35", "50"]));
        assert_eq!(
            report["provenance"]["cable_size"],
            json!({"declared": true, "present": true, "observed": false})
        );
        assert_eq!(report["valueLabels"]["cable_size"], json!({"35": "35 mm²"}));
        // ds-brain's DECLARED type still answers, and its labels are typed by it.
        assert_eq!(report["types"]["cable_size"], "number");
        assert_eq!(report["typeSource"]["cable_size"], "declared");
        assert_eq!(
            report["matchLabels"]["cable_size"],
            json!({"35": 35, "50": 50})
        );
        // A field with no declared type on a tiled layer gets NO type at all —
        // there is no canonical local data to derive one from, and the renderer
        // is not consulted.
        assert!(report["types"].get("installer").is_none());
        assert!(report["typeSource"].get("installer").is_none());
        assert_eq!(report["counts"], json!({}));
    }

    #[test]
    fn a_declared_domain_type_outranks_what_dirty_canonical_data_carries() {
        let report = command(json!({
            "schema": REQUEST_SCHEMA, "operation": "observe", "layer": "master/lv_lines",
            "canonical": {"kind": "local", "source": "design_room"},
            "declared": {"cable_size": {"type": "categorical_number", "values": ["35", "50"]}},
            "features": [{"properties": {"cable_size": "35"}}, {"properties": {"cable_size": "50"}}],
        }))
        .unwrap();
        assert_eq!(report["types"]["cable_size"], "number");
        assert_eq!(report["typeSource"]["cable_size"], "declared");
        assert_eq!(
            report["typeConflicts"],
            json!([{"field": "cable_size", "declared": "number", "observed": "string"}])
        );
        assert_eq!(
            report["matchLabels"]["cable_size"],
            json!({"35": 35, "50": 50})
        );
    }

    #[test]
    fn a_layer_whose_features_are_not_loaded_says_so_rather_than_reporting_empty() {
        let report = command(json!({
            "schema": REQUEST_SCHEMA, "operation": "observe", "layer": "master/customers",
            "canonical": {"kind": "not_loaded", "source": "design_room"},
            "fields": ["category"],
        }))
        .unwrap();
        assert_eq!(report["observed"]["code"], "canonical_features_not_loaded");
        assert_eq!(report["fields"], json!(["category"]));
        assert_eq!(report["types"], json!({}));
    }

    #[test]
    fn declared_present_and_observed_stay_distinguishable() {
        let report = command(json!({
            "schema": REQUEST_SCHEMA, "operation": "observe", "layer": "master/customers",
            "canonical": {"kind": "local", "source": "design_room"},
            "declared": {"category": {"values": ["residential", "commercial", "industrial"]}},
            "present": {"category": ["residential"]},
            "features": [{"properties": {"category": "residential"}}, {"properties": {"category": "school"}}],
        }))
        .unwrap();
        assert_eq!(
            report["values"]["category"],
            json!(["residential", "commercial", "industrial", "school"])
        );
        assert_eq!(
            report["provenance"]["category"],
            json!({"declared": true, "present": true, "observed": true})
        );
        // `school` is off-domain: observed, never declared. The report says so
        // rather than laundering it into the domain.
        assert_eq!(
            report["counts"]["category"],
            json!({"residential":1,"school":1})
        );
    }

    #[test]
    fn bounds_are_reported_never_silently_truncated() {
        let many: Vec<Value> = (0..(MAX_VALUES_PER_FIELD + 25))
            .map(|index| json!({"properties": {"code": format!("C{index}")}}))
            .collect();
        let report = local(json!(many)).unwrap();
        assert_eq!(
            report["values"]["code"].as_array().unwrap().len(),
            MAX_VALUES_PER_FIELD
        );
        assert_eq!(report["more"], json!(true));

        let overflow: Vec<Value> = (0..(MAX_OBSERVED_FEATURES + 1))
            .map(|_| json!({}))
            .collect();
        assert!(local(json!(overflow)).is_err());
    }

    /// The value key must be spelled the way the product spells it, because
    /// that is the string every authored `match` arm addresses a value by.
    #[test]
    fn value_keys_are_spelled_the_way_ecmascript_spells_them() {
        let report = local(json!([{"properties": {
            "one": 1.0, "big": 1e21, "under": 1e20, "tiny": 1e-7, "small": 1e-6,
            "neg": -1234567.5, "zero": -0.0, "frac": 0.1
        }}]))
        .unwrap();
        for (field, expected) in [
            ("one", "1"),
            ("big", "1e+21"),
            ("under", "100000000000000000000"),
            ("tiny", "1e-7"),
            ("small", "0.000001"),
            ("neg", "-1234567.5"),
            ("zero", "0"),
            ("frac", "0.1"),
        ] {
            assert_eq!(report["values"][field], json!([expected]), "{field}");
        }
    }

    #[test]
    fn accepts_a_bare_property_bag_as_well_as_a_feature() {
        let report =
            local(json!([{"struct_type": "S140"}, {"properties": {"struct_type": "S190"}}]))
                .unwrap();
        assert_eq!(report["values"]["struct_type"], json!(["S140", "S190"]));
    }

    #[test]
    fn refuses_unknown_fields_a_wrong_schema_and_contradictory_facts() {
        assert!(command(json!({
            "schema": REQUEST_SCHEMA, "operation": "observe", "layer": "x",
            "canonical": {"kind": "local", "source": "design_room"}, "renderedFeatures": []
        }))
        .is_err());
        assert!(command(json!({
            "schema": "ds.field-domain.request/v0", "operation": "observe", "layer": "x",
            "canonical": {"kind": "local", "source": "design_room"}
        }))
        .is_err());
        // A host cannot claim a layer is tiled and hand over features anyway.
        assert!(command(json!({
            "schema": REQUEST_SCHEMA, "operation": "observe", "layer": "x",
            "canonical": {"kind": "tiled"}, "features": [{"properties": {"a": 1}}]
        }))
        .is_err());
        assert!(evaluate(b"").is_err());
    }
}
