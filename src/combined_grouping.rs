//! Which transformers make up which Combined Report archive when the operator
//! asks for one archive per tag group.
//!
//! Owner ruling, 2026-09-26: a grouped Combined Report is a custom report the
//! user requests explicitly through the CLI or MCP — per city, by tags, or by
//! administrative bounds — and the grouping may nest several levels. It is
//! read-only with respect to the project: nothing is saved, no tag, no
//! consumer grouping. Administrative levels are ordinary tag definitions after
//! governed enrichment, so ONE vocabulary covers all three cases and this
//! module never knows which kind of definition it is grouping by.
//!
//! The decision lives here, pure, so every host that asks it gets the same
//! leaf groups:
//!
//! * **Input** — the governed, digest-pinned tag projection
//!   (`ds-report.design-tags/v3`, the exact document the tag service exports
//!   for an explicit transformer scope), that scope itself, an ordered list of
//!   definition ids to group by (order is nesting: the first id is the outer
//!   folder), and `where` filters.
//! * **Output** — the ordered leaf groups: a path of `(definition id, value)`
//!   pairs and the transformers under it. Each leaf becomes one Combined
//!   Report archive over that explicit transformer scope.
//!
//! What it refuses, and why each is a refusal rather than a repair:
//!
//! * A transformer with no value for a group-by definition is placed under
//!   the explicit value [`UNASSIGNED`] — never dropped, never guessed from its
//!   name. A stored value spelled `_unassigned` would be indistinguishable
//!   from that bucket, so it refuses instead of merging.
//! * A group-by definition that can carry several values per transformer has
//!   no single folder to put one in; it refuses rather than copying the
//!   transformer into two archives.
//! * A filter value outside the definition's vocabulary refuses by name —
//!   `Bere` and `bere` are different values, and a filter that silently
//!   matched nothing is how an operator publishes nothing believing it done.
//! * No transformer left, or more leaf groups than [`MAX_LEAF_GROUPS`], is a
//!   typed refusal naming the count: two hundred archives is already a very
//!   large deliverable, and a mistaken `--group-by` on a per-transformer key
//!   would otherwise start a run of hundreds.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "ds.combined-grouping/v1";

/// The value a transformer carries at a level where it has no assignment.
pub const UNASSIGNED: &str = "_unassigned";

/// The most leaf groups — Combined Report archives — one request may plan.
pub const MAX_LEAF_GROUPS: usize = 200;
/// The deepest nesting one request may ask for. Five administrative levels
/// plus a city and a phase fit; more is a mistake, not a report.
pub const MAX_LEVELS: usize = 8;
/// The most `where` filters one request may carry.
pub const MAX_FILTERS: usize = 32;
/// The transformer scope bound, the tag projection's own.
pub const MAX_TRANSFORMERS: usize = 2000;

const MAX_DOCUMENT_BYTES: usize = 8 * 1024 * 1024;
const MAX_ID_CHARS: usize = 256;
/// How many names a refusal spells out before it stops.
const SHOWN: usize = 8;

const TAG_SCHEMA_CURRENT: &str = "ds-report.design-tags/v3";
const TAG_SCHEMA_LEGACY: &str = "ds-report.design-tags/v2";
const LV_TRANSFORMER: &str = "lv_transformer";
const SINGLE: &str = "single";

// ── refusals ─────────────────────────────────────────────────────────────

/// Why a grouped Combined Report is refused. One code per remedy.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RefusalCode {
    /// A group-by id or filter is blank, untrimmed, repeated or over a bound.
    RequestInvalid,
    /// The tag projection is unreadable, of another schema or project, or
    /// repeats an assignment.
    ProjectionInvalid,
    /// A group-by or filter definition id is not in the projection.
    KeyUnknown,
    /// A group-by definition can carry several values per transformer.
    KeyNotSingle,
    /// A filter value is not in its definition's vocabulary.
    ValueUnknown,
    /// A stored value is spelled like the reserved unassigned bucket.
    ValueReserved,
    /// No transformer is left to group.
    Empty,
    /// More leaf groups than [`MAX_LEAF_GROUPS`].
    TooMany,
}

impl RefusalCode {
    pub const fn token(self) -> &'static str {
        match self {
            Self::RequestInvalid => "combined_group_request_invalid",
            Self::ProjectionInvalid => "combined_group_projection_invalid",
            Self::KeyUnknown => "combined_group_key_unknown",
            Self::KeyNotSingle => "combined_group_key_not_single",
            Self::ValueUnknown => "combined_group_value_unknown",
            Self::ValueReserved => "combined_group_value_reserved",
            Self::Empty => "combined_groups_empty",
            Self::TooMany => "combined_groups_too_many",
        }
    }

    pub const ALL: [Self; 8] = [
        Self::RequestInvalid,
        Self::ProjectionInvalid,
        Self::KeyUnknown,
        Self::KeyNotSingle,
        Self::ValueUnknown,
        Self::ValueReserved,
        Self::Empty,
        Self::TooMany,
    ];
}

/// A refusal, with the definition and the names it is about.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub code: RefusalCode,
    pub token: &'static str,
    /// One line a host may print as is.
    pub detail: String,
    /// The definition id concerned, when there is one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The values, filters or transformers concerned, bounded to a few.
    pub subjects: Vec<String>,
    /// How many subjects there are in all.
    pub subject_count: u32,
    /// The leaf-group count, for [`RefusalCode::TooMany`] and
    /// [`RefusalCode::Empty`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// The bound the count broke, for [`RefusalCode::TooMany`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bound: Option<u32>,
}

impl Refusal {
    fn new(code: RefusalCode, detail: impl Into<String>) -> Self {
        Self {
            code,
            token: code.token(),
            detail: detail.into(),
            key: None,
            subjects: Vec::new(),
            subject_count: 0,
            count: None,
            bound: None,
        }
    }

    fn key(mut self, key: &str) -> Self {
        self.key = Some(key.to_string());
        self
    }

    fn subjects(mut self, subjects: impl IntoIterator<Item = String>) -> Self {
        let all: Vec<String> = subjects.into_iter().collect();
        self.subject_count = small(all.len());
        self.subjects = all.into_iter().take(SHOWN).collect();
        self
    }
}

/// Every count here is bounded far below `u32::MAX`; narrowing keeps a
/// refusal small enough to travel by value.
fn small(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.token, self.detail)
    }
}

impl std::error::Error for Refusal {}

// ── the request ──────────────────────────────────────────────────────────

/// One `where` filter: keep transformers whose `key` value is `value`.
///
/// Filters on the SAME key are alternatives (`city=bere` or `city=kyabe`);
/// filters on different keys must all hold. `value` may be [`UNASSIGNED`] to
/// keep the transformers that carry no value for `key`.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Filter {
    pub key: String,
    pub value: String,
}

impl Filter {
    /// `key=value`, split at the first `=`. Nothing is trimmed or
    /// case-folded: the value is matched against the vocabulary by exact
    /// bytes, so a spelling that only differs in case is refused later by
    /// name rather than corrected here.
    pub fn parse(text: &str) -> Result<Self, Refusal> {
        let Some((key, value)) = text.split_once('=') else {
            return Err(Refusal::new(
                RefusalCode::RequestInvalid,
                format!("filter {text:?} is not <definition-id>=<value>"),
            )
            .subjects([text.to_string()]));
        };
        let filter = Self {
            key: key.to_string(),
            value: value.to_string(),
        };
        filter.validate()?;
        Ok(filter)
    }

    fn validate(&self) -> Result<(), Refusal> {
        if !is_id(&self.key) || !is_value(&self.value) {
            return Err(Refusal::new(
                RefusalCode::RequestInvalid,
                format!(
                    "filter {}={} needs a trimmed definition id and a non-empty trimmed value",
                    self.key, self.value
                ),
            )
            .subjects([format!("{}={}", self.key, self.value)]));
        }
        Ok(())
    }
}

/// Everything the decision reads.
#[derive(Debug, Clone, Copy)]
pub struct Request<'a> {
    /// The project the caller named; the projection must be of it.
    pub project_id: &'a str,
    /// The tag projection's exact text, as the tag service exported it for
    /// `transformers` and at least every definition named below.
    pub document: &'a str,
    /// The transformer scope — every name that must land in exactly one leaf
    /// group, or be filtered out.
    pub transformers: &'a [String],
    /// Definition ids to group by, outermost first.
    pub group_by: &'a [String],
    pub filters: &'a [Filter],
}

// ── the reply ────────────────────────────────────────────────────────────

/// One level of a leaf group's path.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct PathStep {
    pub key: String,
    pub value: String,
}

/// One leaf group: one Combined Report archive.
#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct LeafGroup {
    /// One step per group-by definition, in the requested order. Empty when
    /// the request only filters.
    pub path: Vec<PathStep>,
    /// The explicit transformer scope of this archive, in byte order.
    pub transformers: Vec<String>,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub schema: &'static str,
    pub project_id: String,
    pub group_by: Vec<String>,
    pub filters: Vec<Filter>,
    /// Transformers in the scope the caller handed in.
    pub scope_count: usize,
    /// Transformers the filters kept; each is in exactly one group.
    pub transformer_count: usize,
    /// Transformers the filters left out.
    pub filtered_out_count: usize,
    /// Transformers placed under [`UNASSIGNED`] at one level or more.
    pub unassigned_count: usize,
    pub group_count: usize,
    pub groups: Vec<LeafGroup>,
}

// ── the projection, as far as this question reads it ─────────────────────

#[derive(Deserialize)]
struct Document {
    schema_version: String,
    project_id: String,
    groups: Vec<Definition>,
    assignments: Vec<Assignment>,
}

#[derive(Deserialize)]
struct Definition {
    definition_id: String,
    #[serde(default)]
    cardinality: String,
    #[serde(default)]
    values: Vec<ValueOption>,
}

#[derive(Deserialize)]
struct ValueOption {
    value: String,
}

#[derive(Deserialize)]
struct Assignment {
    object: ObjectRef,
    definition_id: String,
    #[serde(default)]
    values: Vec<String>,
}

#[derive(Deserialize)]
struct ObjectRef {
    kind: String,
    id: String,
}

fn is_id(text: &str) -> bool {
    !text.is_empty()
        && text.chars().count() <= MAX_ID_CHARS
        && text.trim() == text
        && !text.chars().any(char::is_control)
}

fn is_value(text: &str) -> bool {
    !text.is_empty() && text.trim() == text && !text.chars().any(char::is_control)
}

fn invalid(detail: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::RequestInvalid, detail)
}

fn validate_request(request: &Request<'_>) -> Result<Vec<Filter>, Refusal> {
    if request.transformers.len() > MAX_TRANSFORMERS {
        return Err(invalid(format!(
            "{} transformers in scope; the bound is {MAX_TRANSFORMERS}",
            request.transformers.len()
        )));
    }
    let mut seen = BTreeSet::new();
    let repeated: Vec<String> = request
        .transformers
        .iter()
        .filter(|name| !seen.insert(name.as_str()))
        .cloned()
        .collect();
    if !repeated.is_empty() {
        return Err(invalid("the transformer scope repeats a name").subjects(repeated));
    }
    check_shape(request.group_by, request.filters)
}

/// The group-by and filter checks that need nothing read, with the filters
/// de-duplicated.
fn check_shape(group_by: &[String], request_filters: &[Filter]) -> Result<Vec<Filter>, Refusal> {
    if group_by.len() > MAX_LEVELS {
        return Err(invalid(format!(
            "{} group-by levels; the bound is {MAX_LEVELS}",
            group_by.len()
        )));
    }
    let mut seen = BTreeSet::new();
    for key in group_by {
        if !is_id(key) {
            return Err(
                invalid(format!("group-by {key:?} is not a trimmed definition id"))
                    .subjects([key.clone()]),
            );
        }
        if !seen.insert(key.as_str()) {
            return Err(invalid(format!(
                "group-by {key} is named twice; each level is one definition"
            ))
            .key(key));
        }
    }
    if request_filters.len() > MAX_FILTERS {
        return Err(invalid(format!(
            "{} filters; the bound is {MAX_FILTERS}",
            request_filters.len()
        )));
    }
    let mut filters: Vec<Filter> = Vec::new();
    for filter in request_filters {
        filter.validate()?;
        // The same filter twice asks the same question twice; the answer is
        // identical, so the repeat is folded rather than refused.
        if !filters.contains(filter) {
            filters.push(filter.clone());
        }
    }
    Ok(filters)
}

/// Group-by definitions first, then filter definitions not already named.
fn ordered_definitions<'a>(group_by: &'a [String], filters: &'a [Filter]) -> Vec<&'a str> {
    let mut named: Vec<&str> = group_by.iter().map(String::as_str).collect();
    for filter in filters {
        if !named.contains(&filter.key.as_str()) {
            named.push(filter.key.as_str());
        }
    }
    named
}

/// The definition ids a host must export the tag projection for, in order —
/// group-by first, then filter keys not already named — once the request's
/// shape is checked. A host calls this BEFORE it reads anything, so a
/// malformed or over-deep request costs no round trip.
pub fn projection_definitions(
    group_by: &[String],
    filters: &[Filter],
) -> Result<Vec<String>, Refusal> {
    let filters = check_shape(group_by, filters)?;
    Ok(ordered_definitions(group_by, &filters)
        .into_iter()
        .map(str::to_string)
        .collect())
}

fn parse_document(request: &Request<'_>) -> Result<Document, Refusal> {
    let projection = |detail: String| Refusal::new(RefusalCode::ProjectionInvalid, detail);
    if request.document.len() > MAX_DOCUMENT_BYTES {
        return Err(projection(format!(
            "the tag projection is {} bytes; the bound is {MAX_DOCUMENT_BYTES}",
            request.document.len()
        )));
    }
    let document: Document = serde_json::from_str(request.document)
        .map_err(|error| projection(format!("the tag projection is unreadable: {error}")))?;
    if document.schema_version != TAG_SCHEMA_CURRENT && document.schema_version != TAG_SCHEMA_LEGACY
    {
        return Err(projection(format!(
            "the tag projection is {}; this kernel reads {TAG_SCHEMA_CURRENT}",
            document.schema_version
        )));
    }
    if document.project_id != request.project_id {
        return Err(projection(format!(
            "the tag projection is of project {}, not {}",
            document.project_id, request.project_id
        )));
    }
    Ok(document)
}

/// What `transformer` carries for `key`: its values, or [`UNASSIGNED`].
fn value_of<'a>(
    values: &BTreeMap<(&'a str, &'a str), Vec<&'a str>>,
    transformer: &'a str,
    key: &'a str,
) -> Vec<&'a str> {
    match values.get(&(transformer, key)) {
        Some(assigned) if !assigned.is_empty() => assigned.clone(),
        _ => vec![UNASSIGNED],
    }
}

/// Sort key for one level: assigned values in byte order, then
/// [`UNASSIGNED`] last, so the bucket that needs attention closes the list.
type LevelKey = (bool, String);

/// The whole decision.
///
/// An empty scope refuses before the projection is read — there is nothing to
/// export a projection for — so a host holding no transformer may pass an
/// empty document.
pub fn plan(request: &Request<'_>) -> Result<Plan, Refusal> {
    let filters = validate_request(request)?;
    if request.transformers.is_empty() {
        return Err(Refusal {
            count: Some(0),
            ..Refusal::new(
                RefusalCode::Empty,
                "the scope holds no transformer to group",
            )
        });
    }
    let document = parse_document(request)?;

    // Every definition the request names must be one the projection carries.
    let definitions: BTreeMap<&str, &Definition> = document
        .groups
        .iter()
        .map(|definition| (definition.definition_id.as_str(), definition))
        .collect();
    let named = ordered_definitions(request.group_by, &filters);
    let unknown: Vec<String> = named
        .iter()
        .filter(|key| !definitions.contains_key(*key))
        .map(|key| key.to_string())
        .collect();
    if !unknown.is_empty() {
        return Err(Refusal::new(
            RefusalCode::KeyUnknown,
            format!(
                "the tag projection has no definition {}",
                unknown.join(", ")
            ),
        )
        .key(&unknown[0])
        .subjects(unknown));
    }
    for key in request.group_by {
        let definition = definitions[key.as_str()];
        if definition.cardinality != SINGLE {
            return Err(Refusal::new(
                RefusalCode::KeyNotSingle,
                format!(
                    "{key} is a {} definition; a transformer can only sit in one folder",
                    if definition.cardinality.is_empty() {
                        "cardinality-less"
                    } else {
                        definition.cardinality.as_str()
                    }
                ),
            )
            .key(key));
        }
    }
    for filter in &filters {
        let vocabulary = &definitions[filter.key.as_str()].values;
        if filter.value == UNASSIGNED
            || vocabulary.is_empty()
            || vocabulary.iter().any(|option| option.value == filter.value)
        {
            continue;
        }
        let near: Vec<&str> = vocabulary
            .iter()
            .map(|option| option.value.as_str())
            .filter(|value| value.to_lowercase() == filter.value.to_lowercase())
            .collect();
        let mut detail = format!(
            "{} is not a value of {}; values are matched by exact bytes",
            filter.value, filter.key
        );
        if !near.is_empty() {
            detail.push_str(&format!(" (the vocabulary spells it {})", near.join(", ")));
        }
        return Err(Refusal::new(RefusalCode::ValueUnknown, detail)
            .key(&filter.key)
            .subjects([filter.value.clone()]));
    }

    // Transformer → definition → values, over the scope and the named
    // definitions only.
    let scope: BTreeSet<&str> = request.transformers.iter().map(String::as_str).collect();
    let mut values: BTreeMap<(&str, &str), Vec<&str>> = BTreeMap::new();
    for assignment in &document.assignments {
        if assignment.object.kind != LV_TRANSFORMER
            || !scope.contains(assignment.object.id.as_str())
            || !named.contains(&assignment.definition_id.as_str())
        {
            continue;
        }
        let key = (
            assignment.object.id.as_str(),
            assignment.definition_id.as_str(),
        );
        if values.contains_key(&key) {
            return Err(Refusal::new(
                RefusalCode::ProjectionInvalid,
                format!(
                    "the tag projection assigns {} to {} twice",
                    assignment.definition_id, assignment.object.id
                ),
            )
            .key(&assignment.definition_id)
            .subjects([assignment.object.id.clone()]));
        }
        let assigned: Vec<&str> = assignment
            .values
            .iter()
            .map(String::as_str)
            .filter(|value| !value.trim().is_empty())
            .collect();
        if let Some(reserved) = assigned.iter().find(|value| **value == UNASSIGNED) {
            return Err(Refusal::new(
                RefusalCode::ValueReserved,
                format!(
                    "{} carries the value {reserved} for {}, which is the reserved unassigned bucket",
                    assignment.object.id, assignment.definition_id
                ),
            )
            .key(&assignment.definition_id)
            .subjects([assignment.object.id.clone()]));
        }
        if assigned.len() > 1 && request.group_by.contains(&assignment.definition_id) {
            return Err(Refusal::new(
                RefusalCode::KeyNotSingle,
                format!(
                    "{} carries {} values for {}; a transformer can only sit in one folder",
                    assignment.object.id,
                    assigned.len(),
                    assignment.definition_id
                ),
            )
            .key(&assignment.definition_id)
            .subjects([assignment.object.id.clone()]));
        }
        values.insert(key, assigned);
    }

    // Filters: alternatives within a key, all keys must hold.
    let mut wanted: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for filter in &filters {
        wanted
            .entry(filter.key.as_str())
            .or_default()
            .insert(filter.value.as_str());
    }
    let kept: Vec<&str> = request
        .transformers
        .iter()
        .map(String::as_str)
        .filter(|transformer| {
            wanted.iter().all(|(key, accepted)| {
                value_of(&values, transformer, key)
                    .iter()
                    .any(|value| accepted.contains(value))
            })
        })
        .collect();
    if kept.is_empty() {
        let detail = format!(
            "no transformer of {} matches the filters",
            request.transformers.len()
        );
        return Err(Refusal {
            count: Some(0),
            ..Refusal::new(RefusalCode::Empty, detail).subjects(
                filters
                    .iter()
                    .map(|filter| format!("{}={}", filter.key, filter.value)),
            )
        });
    }

    let mut leaves: BTreeMap<Vec<LevelKey>, Vec<String>> = BTreeMap::new();
    let mut unassigned_count = 0;
    for transformer in &kept {
        let path: Vec<LevelKey> = request
            .group_by
            .iter()
            .map(|key| {
                // Group-by definitions are single-valued (checked above).
                let value = value_of(&values, transformer, key)[0];
                (value == UNASSIGNED, value.to_string())
            })
            .collect();
        if path.iter().any(|(unassigned, _)| *unassigned) {
            unassigned_count += 1;
        }
        leaves
            .entry(path)
            .or_default()
            .push(transformer.to_string());
    }
    if leaves.len() > MAX_LEAF_GROUPS {
        return Err(Refusal {
            count: Some(small(leaves.len())),
            bound: Some(small(MAX_LEAF_GROUPS)),
            ..Refusal::new(
                RefusalCode::TooMany,
                format!(
                    "{} leaf groups; the bound is {MAX_LEAF_GROUPS} Combined Report archives per request",
                    leaves.len()
                ),
            )
            .subjects(request.group_by.iter().cloned())
        });
    }

    let groups: Vec<LeafGroup> = leaves
        .into_iter()
        .map(|(path, mut transformers)| {
            transformers.sort();
            LeafGroup {
                path: request
                    .group_by
                    .iter()
                    .zip(path)
                    .map(|(key, (_, value))| PathStep {
                        key: key.clone(),
                        value,
                    })
                    .collect(),
                transformers,
            }
        })
        .collect();
    Ok(Plan {
        schema: SCHEMA,
        project_id: request.project_id.to_string(),
        group_by: request.group_by.to_vec(),
        filters,
        scope_count: request.transformers.len(),
        transformer_count: kept.len(),
        filtered_out_count: request.transformers.len() - kept.len(),
        unassigned_count,
        group_count: groups.len(),
        groups,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    const PROJECT: &str = "p1";

    fn definition(id: &str, cardinality: &str, values: &[&str]) -> Value {
        json!({
            "definition_id": id, "name": id, "cardinality": cardinality,
            "state": "active", "management": "project", "applies_to": ["lv_transformer"],
            "values": values.iter().map(|value| json!({"value": value})).collect::<Vec<_>>(),
        })
    }

    fn assignment(transformer: &str, id: &str, values: &[&str]) -> Value {
        json!({
            "object": {"kind": "lv_transformer", "id": transformer},
            "definition_id": id, "values": values,
        })
    }

    fn document(groups: Vec<Value>, assignments: Vec<Value>) -> String {
        json!({
            "schema_version": TAG_SCHEMA_CURRENT, "project_id": PROJECT,
            "selected_definition_ids": groups.iter().map(|g| g["definition_id"].clone()).collect::<Vec<_>>(),
            "groups": groups, "assignments": assignments,
        })
        .to_string()
    }

    /// Two districts, three cities, two phases, and `t6` with no city.
    fn rwanda() -> (String, Vec<String>) {
        let rows = [
            ("t1", "gasabo", "kigali", "i"),
            ("t2", "gasabo", "kigali", "ii"),
            ("t3", "gasabo", "kimironko", "i"),
            ("t4", "huye", "butare", "i"),
            ("t5", "huye", "butare", "ii"),
            ("t6", "huye", "", "i"),
        ];
        let mut assignments = Vec::new();
        for (transformer, district, city, phase) in rows {
            assignments.push(assignment(transformer, "district", &[district]));
            if !city.is_empty() {
                assignments.push(assignment(transformer, "city", &[city]));
            }
            assignments.push(assignment(transformer, "phase", &[phase]));
        }
        let doc = document(
            vec![
                definition("district", "single", &["gasabo", "huye"]),
                definition("city", "single", &["kigali", "kimironko", "butare"]),
                definition("phase", "single", &["i", "ii"]),
            ],
            assignments,
        );
        let names = rows.iter().map(|row| row.0.to_string()).collect();
        (doc, names)
    }

    fn keys(ids: &[&str]) -> Vec<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    fn filters(texts: &[&str]) -> Vec<Filter> {
        texts
            .iter()
            .map(|text| Filter::parse(text).expect("filter parses"))
            .collect()
    }

    fn run(doc: &str, scope: &[String], group_by: &[&str], wh: &[&str]) -> Result<Plan, Refusal> {
        plan(&Request {
            project_id: PROJECT,
            document: doc,
            transformers: scope,
            group_by: &keys(group_by),
            filters: &filters(wh),
        })
    }

    /// `(path as "k=v/k=v", transformers)` — the whole answer, compared at once.
    fn shape(plan: &Plan) -> Vec<(String, Vec<String>)> {
        plan.groups
            .iter()
            .map(|group| {
                (
                    group
                        .path
                        .iter()
                        .map(|step| format!("{}={}", step.key, step.value))
                        .collect::<Vec<_>>()
                        .join("/"),
                    group.transformers.clone(),
                )
            })
            .collect()
    }

    fn row(path: &str, names: &[&str]) -> (String, Vec<String>) {
        (path.to_string(), keys(names))
    }

    #[test]
    fn one_level_groups_every_transformer_once() {
        let (doc, scope) = rwanda();
        let plan = run(&doc, &scope, &["district"], &[]).expect("plans");
        assert_eq!(
            shape(&plan),
            vec![
                row("district=gasabo", &["t1", "t2", "t3"]),
                row("district=huye", &["t4", "t5", "t6"]),
            ]
        );
        assert_eq!(plan.group_count, 2);
        assert_eq!(plan.transformer_count, 6);
        assert_eq!(plan.unassigned_count, 0);
        assert_eq!(plan.schema, SCHEMA);
    }

    /// Order is nesting: the first definition is the outer folder, and the
    /// reverse order is a different answer, not the same one resorted.
    #[test]
    fn two_levels_nest_in_the_order_asked() {
        let (doc, scope) = rwanda();
        let plan = run(&doc, &scope, &["district", "phase"], &[]).expect("plans");
        assert_eq!(
            shape(&plan),
            vec![
                row("district=gasabo/phase=i", &["t1", "t3"]),
                row("district=gasabo/phase=ii", &["t2"]),
                row("district=huye/phase=i", &["t4", "t6"]),
                row("district=huye/phase=ii", &["t5"]),
            ]
        );
        let reversed = run(&doc, &scope, &["phase", "district"], &[]).expect("plans");
        assert_eq!(
            shape(&reversed),
            vec![
                row("phase=i/district=gasabo", &["t1", "t3"]),
                row("phase=i/district=huye", &["t4", "t6"]),
                row("phase=ii/district=gasabo", &["t2"]),
                row("phase=ii/district=huye", &["t5"]),
            ]
        );
    }

    /// A transformer with no value is placed, visibly, under `_unassigned` —
    /// last at its level — and never dropped.
    #[test]
    fn a_missing_value_lands_in_an_explicit_unassigned_group() {
        let (doc, scope) = rwanda();
        let plan = run(&doc, &scope, &["district", "city"], &[]).expect("plans");
        assert_eq!(
            shape(&plan),
            vec![
                row("district=gasabo/city=kigali", &["t1", "t2"]),
                row("district=gasabo/city=kimironko", &["t3"]),
                row("district=huye/city=butare", &["t4", "t5"]),
                row("district=huye/city=_unassigned", &["t6"]),
            ]
        );
        assert_eq!(plan.unassigned_count, 1);
        let placed: usize = plan.groups.iter().map(|g| g.transformers.len()).sum();
        assert_eq!(placed, scope.len(), "every transformer is in one group");

        // A scope name the projection carries nothing for at all is the same case.
        let mut wider = scope.clone();
        wider.push("t7".into());
        let plan = run(&doc, &wider, &["city"], &[]).expect("plans");
        assert_eq!(
            shape(&plan).last().expect("a group"),
            &row("city=_unassigned", &["t6", "t7"])
        );
    }

    /// Filters on one key are alternatives; filters on different keys all hold;
    /// `_unassigned` selects the untagged.
    #[test]
    fn where_filters_narrow_the_scope_before_grouping() {
        let (doc, scope) = rwanda();
        let plan = run(&doc, &scope, &["city"], &["phase=i"]).expect("plans");
        assert_eq!(
            shape(&plan),
            vec![
                row("city=butare", &["t4"]),
                row("city=kigali", &["t1"]),
                row("city=kimironko", &["t3"]),
                row("city=_unassigned", &["t6"]),
            ]
        );
        assert_eq!(plan.filtered_out_count, 2);

        let either = run(&doc, &scope, &["city"], &["city=kigali", "city=butare"]).expect("plans");
        assert_eq!(
            shape(&either),
            vec![
                row("city=butare", &["t4", "t5"]),
                row("city=kigali", &["t1", "t2"])
            ]
        );

        let both = run(&doc, &scope, &["phase"], &["district=huye", "phase=ii"]).expect("plans");
        assert_eq!(shape(&both), vec![row("phase=ii", &["t5"])]);

        let untagged = run(&doc, &scope, &["district"], &["city=_unassigned"]).expect("plans");
        assert_eq!(shape(&untagged), vec![row("district=huye", &["t6"])]);

        // Filtering alone is one archive over the matching scope.
        let only = run(&doc, &scope, &[], &["district=gasabo"]).expect("plans");
        assert_eq!(shape(&only), vec![row("", &["t1", "t2", "t3"])]);
    }

    #[test]
    fn nothing_left_to_group_is_a_typed_refusal() {
        let (doc, scope) = rwanda();
        let refusal = run(&doc, &scope, &["city"], &["district=gasabo", "city=butare"])
            .expect_err("no transformer is in gasabo AND butare");
        assert_eq!(refusal.code, RefusalCode::Empty);
        assert_eq!(refusal.token, "combined_groups_empty");
        assert_eq!(refusal.count, Some(0));
        assert_eq!(refusal.subjects, keys(&["district=gasabo", "city=butare"]));

        let refusal = run(&doc, &[], &["city"], &[]).expect_err("an empty scope");
        assert_eq!(refusal.code, RefusalCode::Empty);
        // ...before the projection is read: a host with no transformer has
        // no projection to hand in.
        let refusal = run("", &[], &["city"], &[]).expect_err("no document needed");
        assert_eq!(refusal.code, RefusalCode::Empty);
    }

    /// A host learns what to export — and whether the request is well formed
    /// at all — before it reads anything.
    #[test]
    fn the_projection_definitions_are_known_before_any_read() {
        assert_eq!(
            projection_definitions(
                &keys(&["district", "city"]),
                &filters(&["phase=i", "city=kigali", "phase=ii"])
            )
            .expect("well formed"),
            keys(&["district", "city", "phase"])
        );
        let refusal = projection_definitions(&keys(&["city", "city"]), &[]).expect_err("repeat");
        assert_eq!(refusal.code, RefusalCode::RequestInvalid);
        let refusal = projection_definitions(&keys(&[" city"]), &[]).expect_err("untrimmed");
        assert_eq!(refusal.code, RefusalCode::RequestInvalid);
    }

    #[test]
    fn more_leaf_groups_than_the_bound_refuse_with_the_count() {
        let names: Vec<String> = (0..=MAX_LEAF_GROUPS).map(|n| format!("t{n:03}")).collect();
        let values: Vec<String> = (0..=MAX_LEAF_GROUPS).map(|n| format!("v{n:03}")).collect();
        let vocabulary: Vec<&str> = values.iter().map(String::as_str).collect();
        let doc = document(
            vec![definition("site", "single", &vocabulary)],
            names
                .iter()
                .zip(&values)
                .map(|(name, value)| assignment(name, "site", &[value]))
                .collect(),
        );
        let refusal = run(&doc, &names, &["site"], &[]).expect_err("201 groups");
        assert_eq!(refusal.code, RefusalCode::TooMany);
        assert_eq!(refusal.token, "combined_groups_too_many");
        assert_eq!(refusal.count, Some(201));
        assert_eq!(refusal.bound, Some(200));
        assert!(refusal.detail.contains("201"), "{}", refusal.detail);

        // Exactly the bound plans.
        let plan = run(&doc, &names[..MAX_LEAF_GROUPS], &["site"], &[]).expect("200 groups");
        assert_eq!(plan.group_count, MAX_LEAF_GROUPS);
    }

    #[test]
    fn a_definition_the_projection_lacks_refuses_by_id() {
        let (doc, scope) = rwanda();
        let refusal = run(&doc, &scope, &["District"], &[]).expect_err("ids are exact");
        assert_eq!(refusal.code, RefusalCode::KeyUnknown);
        assert_eq!(refusal.key.as_deref(), Some("District"));
        let refusal = run(&doc, &scope, &["city"], &["sector=x"]).expect_err("filter key");
        assert_eq!(refusal.code, RefusalCode::KeyUnknown);
        assert_eq!(refusal.subjects, keys(&["sector"]));
    }

    #[test]
    fn a_filter_value_outside_the_vocabulary_refuses_and_names_the_spelling() {
        let (doc, scope) = rwanda();
        let refusal = run(&doc, &scope, &["city"], &["district=Huye"]).expect_err("case differs");
        assert_eq!(refusal.code, RefusalCode::ValueUnknown);
        assert_eq!(refusal.key.as_deref(), Some("district"));
        assert!(
            refusal.detail.contains("spells it huye"),
            "{}",
            refusal.detail
        );
    }

    #[test]
    fn a_multi_valued_group_by_refuses_rather_than_copying_a_transformer() {
        let doc = document(
            vec![definition("crew", "multiple", &["a", "b"])],
            vec![assignment("t1", "crew", &["a", "b"])],
        );
        let refusal = run(&doc, &keys(&["t1"]), &["crew"], &[]).expect_err("multiple");
        assert_eq!(refusal.code, RefusalCode::KeyNotSingle);
        // Filtering on it is fine: any value may match.
        let doc = document(
            vec![
                definition("crew", "multiple", &["a", "b"]),
                definition("phase", "single", &["i"]),
            ],
            vec![
                assignment("t1", "crew", &["a", "b"]),
                assignment("t1", "phase", &["i"]),
            ],
        );
        let plan = run(&doc, &keys(&["t1"]), &["phase"], &["crew=b"]).expect("plans");
        assert_eq!(shape(&plan), vec![row("phase=i", &["t1"])]);
    }

    #[test]
    fn a_stored_value_spelled_like_the_bucket_refuses() {
        let doc = document(
            vec![definition("city", "single", &["_unassigned"])],
            vec![assignment("t1", "city", &["_unassigned"])],
        );
        let refusal = run(&doc, &keys(&["t1"]), &["city"], &[]).expect_err("reserved");
        assert_eq!(refusal.code, RefusalCode::ValueReserved);
    }

    #[test]
    fn requests_and_projections_are_checked_before_grouping() {
        let (doc, scope) = rwanda();
        for bad in ["city", "=x", "city=", " city=x", "city= x"] {
            let refusal = Filter::parse(bad).expect_err(bad);
            assert_eq!(refusal.code, RefusalCode::RequestInvalid, "{bad}");
        }
        assert_eq!(
            Filter::parse("note=a=b")
                .expect("value keeps its '='")
                .value,
            "a=b"
        );
        let refusal = run(&doc, &scope, &["city", "city"], &[]).expect_err("repeat");
        assert_eq!(refusal.code, RefusalCode::RequestInvalid);
        let deep: Vec<&str> = ["a", "b", "c", "d", "e", "f", "g", "h", "i"].to_vec();
        let refusal = run(&doc, &scope, &deep, &[]).expect_err("too deep");
        assert_eq!(refusal.code, RefusalCode::RequestInvalid);

        let other = plan(&Request {
            project_id: "p2",
            document: &doc,
            transformers: &scope,
            group_by: &keys(&["city"]),
            filters: &[],
        })
        .expect_err("another project's projection");
        assert_eq!(other.code, RefusalCode::ProjectionInvalid);
        let v9 = doc.replace(TAG_SCHEMA_CURRENT, "ds-report.design-tags/v9");
        let refusal = run(&v9, &scope, &["city"], &[]).expect_err("schema");
        assert_eq!(refusal.code, RefusalCode::ProjectionInvalid);
    }

    #[test]
    fn every_code_has_its_own_token() {
        let tokens: BTreeSet<&str> = RefusalCode::ALL.iter().map(|code| code.token()).collect();
        assert_eq!(tokens.len(), RefusalCode::ALL.len());
    }

    // ── real data ────────────────────────────────────────────────────────

    /// The exact bytes the tag service exported on 2026-09-26 for project
    /// `arjgpydw_aderm_loc7` (69 active transformers, definitions `city`
    /// then `phase`); sha256 f9b49bea…e774f4 matches the service's receipt.
    const ADERM_LOC7: &str =
        include_str!("../tests/fixtures/combined-grouping-aderm-loc7-tags.json");

    fn aderm_scope() -> Vec<String> {
        let document: Value = serde_json::from_str(ADERM_LOC7).expect("fixture parses");
        let mut names: Vec<String> = document["assignments"]
            .as_array()
            .expect("assignments")
            .iter()
            .filter_map(|row| row["object"]["id"].as_str().map(str::to_string))
            .collect();
        names.sort();
        names.dedup();
        names
    }

    fn aderm(group_by: &[&str], wh: &[&str]) -> Plan {
        plan(&Request {
            project_id: "arjgpydw_aderm_loc7",
            document: ADERM_LOC7,
            transformers: &aderm_scope(),
            group_by: &keys(group_by),
            filters: &filters(wh),
        })
        .expect("the real projection plans")
    }

    fn counts(plan: &Plan) -> Vec<(String, usize)> {
        shape(plan)
            .into_iter()
            .map(|(path, names)| (path, names.len()))
            .collect()
    }

    fn count(path: &str, n: usize) -> (String, usize) {
        (path.to_string(), n)
    }

    /// Per city: seven archives. The counts were read independently from the
    /// same bytes with `jq` before this test was written.
    #[test]
    fn aderm_loc7_per_city_is_seven_archives() {
        assert_eq!(aderm_scope().len(), 69);
        let plan = aderm(&["city"], &[]);
        assert_eq!(
            counts(&plan),
            vec![
                count("city=beinamar", 8),
                count("city=bere", 12),
                count("city=gagal", 9),
                count("city=kyabe", 16),
                count("city=massenya", 4),
                count("city=mbaye", 8),
                count("city=moissaia", 12),
            ]
        );
        assert_eq!(plan.transformer_count, 69);
        assert_eq!(plan.unassigned_count, 0);
        assert_eq!(
            plan.groups[1].transformers,
            (1..=12).map(|n| format!("bere_{n:02}")).collect::<Vec<_>>()
        );
    }

    /// City then phase: thirteen archives — Massenya has no phase II.
    #[test]
    fn aderm_loc7_city_then_phase_is_thirteen_archives() {
        let plan = aderm(&["city", "phase"], &[]);
        assert_eq!(
            counts(&plan),
            vec![
                count("city=beinamar/phase=i", 6),
                count("city=beinamar/phase=ii", 2),
                count("city=bere/phase=i", 9),
                count("city=bere/phase=ii", 3),
                count("city=gagal/phase=i", 4),
                count("city=gagal/phase=ii", 5),
                count("city=kyabe/phase=i", 12),
                count("city=kyabe/phase=ii", 4),
                count("city=massenya/phase=i", 4),
                count("city=mbaye/phase=i", 6),
                count("city=mbaye/phase=ii", 2),
                count("city=moissaia/phase=i", 9),
                count("city=moissaia/phase=ii", 3),
            ]
        );
        let phase_two = aderm(&["city"], &["phase=ii"]);
        assert_eq!(phase_two.group_count, 6);
        assert_eq!(phase_two.transformer_count, 19);
        assert_eq!(phase_two.filtered_out_count, 50);
    }
}
