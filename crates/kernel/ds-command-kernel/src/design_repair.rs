//! Evidence-backed, pure field reconciliation. Hosts authorize/acquire inputs
//! and persist the exact projected result; this owner never runs engineering
//! processing, normalizes retained facts, or performs an effect.
use crate::design::{self, Snapshot};
use crate::report_export::jcs;
use ds_geo_lite::identity::geojson_feature_identity_candidates;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_CHANGES: usize = design::MAX_EDITS;
pub const MAX_DETAILS: usize = 500;
pub const MAX_SOURCE_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PROPOSAL_BYTES: usize = 16 * 1024 * 1024;
pub const MAX_POLICY_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub project: String,
    pub principal: String,
    pub lane: String,
    pub audience: String,
}
impl Scope {
    pub fn validate(&self) -> Result<(), RepairError> {
        for value in [&self.project, &self.principal, &self.lane, &self.audience] {
            if value.is_empty()
                || value.trim() != value
                || value.len() > 512
                || value.chars().any(char::is_control)
            {
                return Err(error(
                    "repair_scope_invalid",
                    "Scope requires exact bounded project, principal, lane and audience.",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Cell {
    Missing,
    Value { value: Value },
}
impl Cell {
    fn read(feature: &Value, field: &str) -> Self {
        feature["properties"]
            .get(field)
            .map_or(Self::Missing, |value| Self::Value {
                value: value.clone(),
            })
    }
}

/// The source's logical digest is over its exact layer map. A raw file hash is
/// a separate host-verified pin and never substituted for a logical digest.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct EvidenceSource {
    pub schema: String,
    pub id: String,
    pub scope: Scope,
    pub content_digest: String,
    #[serde(default)]
    pub raw_file_sha256: Option<String>,
    #[serde(default)]
    pub revision: Option<String>,
    pub crs: Option<String>,
    #[serde(default)]
    pub units: BTreeMap<String, BTreeMap<String, String>>,
    pub layers: BTreeMap<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FieldType {
    String,
    Number,
    Boolean,
    Json,
}

/// This is supplied from the admitted model/cleaning policy. It does not infer
/// aliases, units or a wider domain from dirty observed values.
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldRule {
    pub field_type: FieldType,
    #[serde(default)]
    pub nullable: bool,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub allowed_values: Vec<Value>,
    #[serde(default)]
    pub unit: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ReferenceRule {
    pub layer: String,
    pub field: String,
    pub target_layers: Vec<String>,
    pub target_field: String,
    #[serde(default)]
    pub required: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeatureRef {
    pub layer: String,
    pub id: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FeatureIdentity {
    pub field: String,
    pub value: Value,
}

/// Exact source path/value for the first admitted durable identity. The caller
/// still checks alias uniqueness across the complete layer before applying it.
pub fn feature_identity(feature: &Value) -> Result<FeatureIdentity, RepairError> {
    let first = geojson_feature_identity_candidates(feature)
        .into_iter()
        .next()
        .ok_or_else(|| {
            error(
                "repair_identity_required",
                "Feature has no admitted stable identity.",
            )
        })?;
    for field in [
        "id",
        "properties.id",
        "properties.doc_id",
        "properties.feature_id",
    ] {
        let value = if field == "id" {
            feature.get("id")
        } else {
            feature["properties"].get(field.strip_prefix("properties.").unwrap())
        };
        let Some(value) = value else {
            continue;
        };
        let candidate = match value {
            Value::String(value) => value.trim().to_owned(),
            Value::Number(number) => number.to_string(),
            _ => continue,
        };
        if candidate == first {
            return Ok(FeatureIdentity {
                field: field.into(),
                value: value.clone(),
            });
        }
    }
    Err(error(
        "repair_identity_required",
        "Feature has no exact identity path.",
    ))
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RepairPropertyKind {
    TextOrNumber,
    /// A lifecycle status spelling (bounded text, never a number).
    Status,
    NonnegativeNumber,
    NonnegativeInteger,
    Approval,
}

/// Closed first-release repair catalogue shared with the transport gate.
/// CellAuthority owns these engineering spellings. This surface grants only
/// explicit factual field repair; it never changes cell locks or runs process.
pub fn repair_property_kind(layer: &str, field: &str) -> Option<RepairPropertyKind> {
    use RepairPropertyKind::*;
    if matches!(
        layer,
        "tr" | "lv_poles" | "tapping_poles" | "lv_lines" | "service_cables" | "customers"
    ) && field == "drafting_status"
    {
        return Some(Approval);
    }
    match (layer, field) {
        ("tr", "plan_kva" | "tr_size" | "transfo_size") => Some(NonnegativeNumber),
        ("tr", "main_cable_size") => Some(TextOrNumber),
        (
            "lv_poles" | "tapping_poles",
            "struct_type" | "material" | "assembly_type" | "stay" | "flying_stay" | "earthing",
        ) => Some(TextOrNumber),
        ("lv_lines" | "service_cables", "cable_size" | "cab_size" | "service_cable_size") => {
            Some(TextOrNumber)
        }
        ("lv_lines" | "service_cables", "length") => Some(NonnegativeNumber),
        (
            "customers",
            "meter_type"
            | "category"
            | "connection_pole_id"
            | "connection_override_pole_id"
            | "service_cable_size",
        ) => Some(TextOrNumber),
        ("customers", "peak_power_w") => Some(NonnegativeNumber),
        ("customers", "num_houses") => Some(NonnegativeInteger),
        // Owner 2026-10-23: a pole's existence status, which older designs
        // recorded as a struct_type marker, is a factual repair cell.
        ("lv_poles", "existing_status") => Some(Status),
        _ => None,
    }
}
pub fn validate_property_cell(layer: &str, field: &str, cell: &Cell) -> Result<(), RepairError> {
    let kind = repair_property_kind(layer, field).ok_or_else(|| {
        error(
            "repair_field_not_allowed",
            format!("{layer}.{field} is outside the native repair catalogue."),
        )
    })?;
    let valid_number = |value: &Value| {
        value
            .as_f64()
            .is_some_and(|n| n.is_finite() && (0.0..=9_007_199_254_740_991.0).contains(&n))
    };
    let valid = match (&kind, cell) {
        (RepairPropertyKind::Approval, Cell::Value { value }) => value == "approved",
        (RepairPropertyKind::Approval, _) => false,
        (_, Cell::Missing | Cell::Value { value: Value::Null }) => true,
        (
            RepairPropertyKind::TextOrNumber | RepairPropertyKind::Status,
            Cell::Value {
                value: Value::String(text),
            },
        ) => text.len() <= 2048 && !text.chars().any(char::is_control),
        (RepairPropertyKind::Status, Cell::Value { .. }) => false,
        (
            RepairPropertyKind::TextOrNumber | RepairPropertyKind::NonnegativeNumber,
            Cell::Value { value },
        ) => valid_number(value),
        (RepairPropertyKind::NonnegativeInteger, Cell::Value { value }) => {
            valid_number(value) && value.as_f64().is_some_and(|n| n.fract() == 0.0)
        }
    };
    if valid {
        Ok(())
    } else {
        Err(error("repair_value_invalid", "Repair cell contradicts its native field type/range; approval can only be explicitly assigned approved."))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ConnectivityPolicy {
    pub line_layers: Vec<String>,
    /// Deliberate roots, including admitted tapping roots. No guessed roots.
    pub roots: Vec<FeatureRef>,
    /// A completeness claim is checked by the source owner before this call.
    pub sources_complete: bool,
}

fn default_details() -> usize {
    100
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuditPolicy {
    pub scope: Scope,
    #[serde(default)]
    pub fields: BTreeMap<String, BTreeMap<String, FieldRule>>,
    #[serde(default)]
    pub references: Vec<ReferenceRule>,
    #[serde(default)]
    pub connectivity: Option<ConnectivityPolicy>,
    #[serde(default = "default_details")]
    pub max_details: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PositionDimensions {
    Xy,
    Xyz,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct KeyPair {
    pub source: String,
    pub target: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MatchSpec {
    ExactPosition {
        crs: String,
        dimensions: PositionDimensions,
    },
    Keys {
        pairs: Vec<KeyPair>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldMapping {
    pub source: String,
    pub target: String,
    /// Missing historical values do not erase current facts by default.
    #[serde(default)]
    pub remove_if_source_missing: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CompareRequest {
    pub scope: Scope,
    pub source_layer: String,
    pub target_layer: String,
    pub matching: MatchSpec,
    pub fields: Vec<FieldMapping>,
    #[serde(default = "default_details")]
    pub max_details: usize,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MatchCounts {
    pub source_rows: usize,
    pub target_rows: usize,
    pub matched: usize,
    pub unmatched_source: usize,
    pub unmatched_target: usize,
    pub ambiguous_source: usize,
    pub ambiguous_target: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RecordMatches {
    pub counts: MatchCounts,
    pub pairs: Vec<[usize; 2]>,
    pub unmatched_source: Vec<usize>,
    pub unmatched_target: Vec<usize>,
    pub ambiguous_source: Vec<usize>,
    pub ambiguous_target: Vec<usize>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FieldDifference {
    pub source_index: usize,
    pub target_index: usize,
    pub source_field: String,
    pub target_field: String,
    pub before: Cell,
    pub after: Cell,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Comparison {
    pub schema: String,
    pub source_digest: String,
    pub target_revision: String,
    pub counts: MatchCounts,
    pub different_cells: usize,
    pub differences: Vec<FieldDifference>,
    pub omitted_details: usize,
    pub mutated: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TargetFence {
    pub snapshot_revision: String,
    #[serde(default)]
    pub server_version: Option<u64>,
    #[serde(default)]
    pub layers_content_digest: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RepairOperation {
    Restore {
        source_layer: String,
        target_layer: String,
        matching: MatchSpec,
        fields: Vec<FieldMapping>,
    },
    Assign {
        target_layer: String,
        target_ids: Vec<String>,
        values: BTreeMap<String, Cell>,
    },
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ProposalRequest {
    pub scope: Scope,
    pub target: TargetFence,
    pub policy: AuditPolicy,
    pub allowlist: BTreeMap<String, BTreeSet<String>>,
    pub operations: Vec<RepairOperation>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SourceCellEvidence {
    pub layer: String,
    pub row: usize,
    pub feature_digest: String,
    pub field: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CellChange {
    pub id: String,
    pub layer: String,
    pub feature_id: String,
    pub before_feature_digest: String,
    pub field: String,
    pub before: Cell,
    pub after: Cell,
    pub evidence: Option<SourceCellEvidence>,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub schema: String,
    pub request: ProposalRequest,
    pub source_digest: Option<String>,
    pub match_counts: Vec<MatchCounts>,
    pub changes: Vec<CellChange>,
    pub target_after_revision: String,
    pub digest: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ApplySelection {
    pub scope: Scope,
    pub proposal_digest: String,
    pub change_ids: Vec<String>,
    pub confirmed: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AppliedRepair {
    pub schema: String,
    pub proposal_digest: String,
    pub selected_change_ids: Vec<String>,
    pub before_revision: String,
    pub after_revision: String,
    pub changed_cells: usize,
    pub preserved_outside_selection: bool,
    pub snapshot: Snapshot,
    pub persisted: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuditFinding {
    pub code: String,
    pub severity: String,
    pub transformer: String,
    pub layer: String,
    pub feature_index: Option<usize>,
    pub feature_id: Option<String>,
    pub field: Option<String>,
    pub evidence: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AuditReport {
    pub schema: String,
    pub scope: Scope,
    pub input_digest: String,
    pub policy_digest: String,
    pub transformers: usize,
    pub features: usize,
    pub finding_counts: BTreeMap<String, usize>,
    pub blockers: usize,
    pub candidates: usize,
    pub findings: Vec<AuditFinding>,
    pub omitted_details: usize,
    pub mutated: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RepairError {
    pub code: String,
    pub message: String,
}
impl std::fmt::Display for RepairError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for RepairError {}
fn error(code: &str, message: impl Into<String>) -> RepairError {
    RepairError {
        code: code.into(),
        message: message.into(),
    }
}
fn hash<T: Serialize + ?Sized>(value: &T) -> Result<String, RepairError> {
    // Preserve exact JSON integers as the snapshot revision contract does.
    // Remote layer content hashes use their separately named RFC8785 fence.
    design::digest(&value).map_err(|e| error("repair_document_invalid", e))
}
fn revision(target: &Snapshot) -> Result<String, RepairError> {
    design::digest(target).map_err(|e| error("repair_document_invalid", e))
}
fn bounded(value: &impl Serialize, max: usize) -> Result<(), RepairError> {
    if serde_json::to_vec(value)
        .map_err(|e| error("repair_document_invalid", e.to_string()))?
        .len()
        > max
    {
        return Err(error(
            "repair_bound_exceeded",
            "Document exceeds the admitted byte bound.",
        ));
    }
    Ok(())
}
fn name(value: &str) -> Result<(), RepairError> {
    if value.is_empty()
        || value.len() > 128
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(error(
            "repair_document_invalid",
            "Names must be exact, nonempty and at most 128 bytes.",
        ));
    }
    Ok(())
}
fn sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn rows(collection: &Value) -> Result<&[Value], RepairError> {
    if collection["type"] != "FeatureCollection" {
        return Err(error(
            "repair_document_invalid",
            "A layer must be a FeatureCollection.",
        ));
    }
    let rows = collection["features"]
        .as_array()
        .ok_or_else(|| error("repair_document_invalid", "features must be an array."))?;
    if rows.len() > design::MAX_FEATURES {
        return Err(error(
            "repair_bound_exceeded",
            "Layer exceeds 100,000 records.",
        ));
    }
    if rows
        .iter()
        .any(|f| f["type"] != "Feature" || !f["properties"].is_object())
    {
        return Err(error(
            "repair_document_invalid",
            "Records require Feature and object properties.",
        ));
    }
    Ok(rows)
}
fn layer<'a>(layers: &'a BTreeMap<String, Value>, key: &str) -> Result<&'a [Value], RepairError> {
    name(key)?;
    rows(
        layers
            .get(key)
            .ok_or_else(|| error("repair_layer_unknown", format!("Layer {key} is absent.")))?,
    )
}
fn validate_source(source: &EvidenceSource, scope: &Scope) -> Result<(), RepairError> {
    bounded(source, MAX_SOURCE_BYTES)?;
    source.scope.validate()?;
    name(&source.id)?;
    if source.schema != "ds.repair.evidence/v1" || &source.scope != scope {
        return Err(error(
            "repair_scope_mismatch",
            "Evidence schema/scope does not match this request.",
        ));
    }
    if source.layers.is_empty()
        || source.layers.len() > 64
        || source.content_digest != hash(&source.layers)?
    {
        return Err(error(
            "repair_source_changed",
            "Evidence layers differ from their recorded digest or bounds.",
        ));
    }
    if source.raw_file_sha256.as_ref().is_some_and(|s| !sha(s)) {
        return Err(error(
            "repair_document_invalid",
            "Raw file pin must be lowercase SHA-256.",
        ));
    }
    if source
        .revision
        .as_ref()
        .is_some_and(|s| s.is_empty() || s.len() > 512 || s.chars().any(char::is_control))
    {
        return Err(error(
            "repair_document_invalid",
            "Historical source revision is invalid.",
        ));
    }
    let mut count = 0usize;
    for (key, value) in &source.layers {
        name(key)?;
        let features = rows(value)?;
        count += features.len();
        for feature in features {
            if !feature["geometry"].is_null() {
                if source.crs.as_deref() != Some("EPSG:4326") {
                    return Err(error("repair_crs_required", "Geometry evidence requires admitted EPSG:4326; reproject explicitly before reconciliation."));
                }
                design::validate_geometry(&feature["geometry"])
                    .map_err(|e| error("repair_geometry_invalid", e))?;
            }
        }
    }
    if count > design::MAX_FEATURES {
        return Err(error(
            "repair_bound_exceeded",
            "Evidence exceeds 100,000 aggregate records.",
        ));
    }
    Ok(())
}
fn validate_policy(policy: &AuditPolicy) -> Result<(), RepairError> {
    bounded(policy, MAX_POLICY_BYTES)?;
    policy.scope.validate()?;
    if policy.max_details > MAX_DETAILS || policy.fields.len() > 64 || policy.references.len() > 64
    {
        return Err(error(
            "repair_bound_exceeded",
            "Policy/details exceed bounds.",
        ));
    }
    for (layer, fields) in &policy.fields {
        name(layer)?;
        if fields.len() > 256 {
            return Err(error("repair_bound_exceeded", "Too many declared fields."));
        }
        for (field, rule) in fields {
            name(field)?;
            if rule.allowed_values.len() > 2000
                || rule
                    .unit
                    .as_ref()
                    .is_some_and(|unit| unit.is_empty() || unit.len() > 128)
            {
                return Err(error(
                    "repair_bound_exceeded",
                    "Field domain/unit exceeds bounds.",
                ));
            }
        }
    }
    for reference in &policy.references {
        name(&reference.layer)?;
        name(&reference.field)?;
        name(&reference.target_field)?;
        if reference.target_layers.is_empty() || reference.target_layers.len() > 64 {
            return Err(error(
                "repair_document_invalid",
                "Reference targets require 1..64 exact layers.",
            ));
        }
        for layer in &reference.target_layers {
            name(layer)?;
        }
    }
    if let Some(connectivity) = &policy.connectivity {
        if connectivity.roots.len() > 2000
            || connectivity.line_layers.is_empty()
            || connectivity.line_layers.len() > 64
        {
            return Err(error(
                "repair_bound_exceeded",
                "Connectivity root/layer scope exceeds bounds.",
            ));
        }
        for layer in &connectivity.line_layers {
            name(layer)?;
        }
        for root in &connectivity.roots {
            name(&root.layer)?;
            name(&root.id)?;
        }
    }
    Ok(())
}
fn cell_allowed(cell: &Cell, rule: &FieldRule) -> bool {
    match cell {
        Cell::Missing => !rule.required,
        Cell::Value { value } if value.is_null() => rule.nullable,
        Cell::Value { value } => {
            let typed = match rule.field_type {
                FieldType::String => value.is_string(),
                FieldType::Number => value.as_f64().is_some_and(f64::is_finite),
                FieldType::Boolean => value.is_boolean(),
                FieldType::Json => true,
            };
            typed && (rule.allowed_values.is_empty() || rule.allowed_values.contains(value))
        }
    }
}
fn position_key(feature: &Value, dimensions: &PositionDimensions) -> Result<String, RepairError> {
    if feature["geometry"]["type"] != "Point" {
        return Err(error(
            "repair_geometry_invalid",
            "Exact-position matching requires Point geometry on every selected record.",
        ));
    }
    design::validate_geometry(&feature["geometry"])
        .map_err(|e| error("repair_geometry_invalid", e))?;
    let coordinates = feature["geometry"]["coordinates"].as_array().unwrap();
    let count = if matches!(dimensions, PositionDimensions::Xy) {
        2
    } else {
        3
    };
    if coordinates.len() < count {
        return Err(error(
            "repair_geometry_invalid",
            "XYZ comparison requires recorded Z on every point.",
        ));
    }
    jcs::sha256(&json!(&coordinates[..count])).map_err(|e| error("repair_geometry_invalid", e))
}
fn record_key(
    feature: &Value,
    spec: &MatchSpec,
    source: bool,
) -> Result<Option<String>, RepairError> {
    match spec {
        MatchSpec::ExactPosition { dimensions, .. } => position_key(feature, dimensions).map(Some),
        MatchSpec::Keys { pairs } => {
            let mut key = Vec::new();
            for pair in pairs {
                let value =
                    feature["properties"].get(if source { &pair.source } else { &pair.target });
                match value {
                    None | Some(Value::Null) => return Ok(None),
                    Some(value) if value.is_string() || value.is_number() || value.is_boolean() => {
                        key.push(value.clone())
                    }
                    _ => {
                        return Err(error(
                            "repair_key_invalid",
                            "Match keys require typed scalar values.",
                        ))
                    }
                }
            }
            hash(&key).map(Some)
        }
    }
}

/// Domain-neutral primitive also used by table-only workflows. Records are
/// Feature property rows; key matching needs no geometry or CRS.
pub fn match_records(
    source: &[Value],
    target: &[Value],
    spec: &MatchSpec,
    source_crs: Option<&str>,
    target_crs: Option<&str>,
) -> Result<RecordMatches, RepairError> {
    // Public callers may use the matcher directly, without an EvidenceSource
    // wrapper. Keep the same admitted byte bounds at that boundary too.
    bounded(&source, MAX_SOURCE_BYTES)?;
    bounded(&target, design::MAX_BYTES)?;
    if source.len().saturating_add(target.len()) > design::MAX_FEATURES {
        return Err(error(
            "repair_bound_exceeded",
            "Matching exceeds 100,000 aggregate records.",
        ));
    }
    if source
        .iter()
        .chain(target)
        .any(|r| !r["properties"].is_object())
    {
        return Err(error(
            "repair_document_invalid",
            "Match records require object properties.",
        ));
    }
    match spec {
        MatchSpec::ExactPosition { crs, .. }
            if crs != "EPSG:4326" || source_crs != Some(crs) || target_crs != Some(crs) =>
        {
            return Err(error("repair_crs_required", "Exact-position matching requires explicit common EPSG:4326 with no implicit reprojection."));
        }
        MatchSpec::Keys { pairs } => {
            if pairs.is_empty() || pairs.len() > 16 {
                return Err(error(
                    "repair_key_invalid",
                    "Declare 1..16 match key pairs.",
                ));
            }
            let mut seen = BTreeSet::new();
            for pair in pairs {
                name(&pair.source)?;
                name(&pair.target)?;
                if !seen.insert((&pair.source, &pair.target)) {
                    return Err(error("repair_key_invalid", "Repeated match key pair."));
                }
            }
        }
        _ => {}
    }
    let mut left: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut right: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    let mut result = RecordMatches {
        counts: MatchCounts {
            source_rows: source.len(),
            target_rows: target.len(),
            ..Default::default()
        },
        pairs: vec![],
        unmatched_source: vec![],
        unmatched_target: vec![],
        ambiguous_source: vec![],
        ambiguous_target: vec![],
    };
    for (records, side, index, missing) in [
        (source, true, &mut left, &mut result.unmatched_source),
        (target, false, &mut right, &mut result.unmatched_target),
    ] {
        for (row, feature) in records.iter().enumerate() {
            if let Some(key) = record_key(feature, spec, side)? {
                index.entry(key).or_default().push(row);
            } else {
                missing.push(row);
            }
        }
    }
    for key in left.keys().chain(right.keys()).collect::<BTreeSet<_>>() {
        let l = left.get(key).map(Vec::as_slice).unwrap_or(&[]);
        let r = right.get(key).map(Vec::as_slice).unwrap_or(&[]);
        if l.len() > 1 || r.len() > 1 {
            result.ambiguous_source.extend(l);
            result.ambiguous_target.extend(r);
        } else if l.len() == 1 && r.len() == 1 {
            result.pairs.push([l[0], r[0]]);
        } else {
            result.unmatched_source.extend(l);
            result.unmatched_target.extend(r);
        }
    }
    result.pairs.sort_unstable();
    result.unmatched_source.sort_unstable();
    result.unmatched_target.sort_unstable();
    result.ambiguous_source.sort_unstable();
    result.ambiguous_target.sort_unstable();
    result.counts.matched = result.pairs.len();
    result.counts.unmatched_source = result.unmatched_source.len();
    result.counts.unmatched_target = result.unmatched_target.len();
    result.counts.ambiguous_source = result.ambiguous_source.len();
    result.counts.ambiguous_target = result.ambiguous_target.len();
    Ok(result)
}

fn field_maps(fields: &[FieldMapping]) -> Result<(), RepairError> {
    if fields.is_empty() || fields.len() > 256 {
        return Err(error(
            "repair_bound_exceeded",
            "Select 1..256 field mappings.",
        ));
    }
    let mut seen = BTreeSet::new();
    for field in fields {
        name(&field.source)?;
        name(&field.target)?;
        if !seen.insert(&field.target) {
            return Err(error(
                "repair_field_invalid",
                "A target field is mapped more than once.",
            ));
        }
    }
    Ok(())
}
pub fn compare(
    source: &EvidenceSource,
    target: &Snapshot,
    request: &CompareRequest,
) -> Result<Comparison, RepairError> {
    request.scope.validate()?;
    validate_source(source, &request.scope)?;
    target
        .validate_retained()
        .map_err(|e| error("repair_target_invalid", e))?;
    field_maps(&request.fields)?;
    if request.max_details > MAX_DETAILS {
        return Err(error(
            "repair_bound_exceeded",
            "Comparison details exceed 500.",
        ));
    }
    let left = layer(&source.layers, &request.source_layer)?;
    let right = layer(&target.layers, &request.target_layer)?;
    let matched = match_records(
        left,
        right,
        &request.matching,
        source.crs.as_deref(),
        Some(&target.crs),
    )?;
    let mut differences = Vec::new();
    let mut total = 0;
    for [source_index, target_index] in matched.pairs {
        for field in &request.fields {
            let before = Cell::read(&right[target_index], &field.target);
            let after = Cell::read(&left[source_index], &field.source);
            if before != after {
                total += 1;
                if differences.len() < request.max_details {
                    differences.push(FieldDifference {
                        source_index,
                        target_index,
                        source_field: field.source.clone(),
                        target_field: field.target.clone(),
                        before,
                        after,
                    });
                }
            }
        }
    }
    Ok(Comparison {
        schema: "ds.repair.comparison/v1".into(),
        source_digest: hash(source)?,
        target_revision: revision(target)?,
        counts: matched.counts,
        different_cells: total,
        omitted_details: total - differences.len(),
        differences,
        mutated: false,
    })
}

fn protected(field: &str) -> bool {
    matches!(
        field,
        "id" | "doc_id"
            | "feature_id"
            | "transfo"
            | "transformer_name"
            | "geometry"
            | "geometry_wkt"
            | "geom"
            | "the_geom"
            | "wkt"
    ) || field.starts_with("_ds_")
        || field.starts_with("__ds_")
}
fn rule<'a>(
    request: &'a ProposalRequest,
    layer: &str,
    field: &str,
) -> Result<&'a FieldRule, RepairError> {
    if protected(field)
        || repair_property_kind(layer, field).is_none()
        || !request
            .allowlist
            .get(layer)
            .is_some_and(|fields| fields.contains(field))
    {
        return Err(error(
            "repair_field_not_allowed",
            format!("{layer}.{field} is protected or outside the reviewed allowlist."),
        ));
    }
    request
        .policy
        .fields
        .get(layer)
        .and_then(|fields| fields.get(field))
        .ok_or_else(|| {
            error(
                "repair_field_not_allowed",
                format!("{layer}.{field} has no admitted field policy."),
            )
        })
}
fn identities(features: &[Value]) -> BTreeMap<String, Vec<usize>> {
    let mut index: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (row, feature) in features.iter().enumerate() {
        for id in geojson_feature_identity_candidates(feature) {
            index.entry(id).or_default().push(row);
        }
    }
    index
}
fn stable_id(feature: &Value, index: &BTreeMap<String, Vec<usize>>) -> Result<String, RepairError> {
    let ids = geojson_feature_identity_candidates(feature);
    if ids.is_empty() {
        return Err(error(
            "repair_identity_required",
            "A mutation target has no stable identity; ordinal matching is not admitted.",
        ));
    }
    if ids
        .iter()
        .any(|id| index.get(id).is_none_or(|rows| rows.len() != 1))
    {
        return Err(error(
            "repair_identity_ambiguous",
            "A target identity alias belongs to multiple records.",
        ));
    }
    Ok(ids[0].clone())
}
fn add_change(
    changes: &mut Vec<CellChange>,
    seen: &mut BTreeSet<(String, String, String)>,
    request: &ProposalRequest,
    layer: &str,
    feature: &Value,
    id: String,
    field: &str,
    after: Cell,
    evidence: Option<SourceCellEvidence>,
) -> Result<(), RepairError> {
    let rule = rule(request, layer, field)?;
    validate_property_cell(layer, field, &after)?;
    if !cell_allowed(&after, rule) {
        return Err(error(
            "repair_value_invalid",
            format!("{layer}.{field} contradicts its admitted type/domain/null policy."),
        ));
    }
    let before = Cell::read(feature, field);
    if before == after {
        return Ok(());
    }
    if !seen.insert((layer.to_owned(), id.clone(), field.to_owned())) {
        return Err(error(
            "repair_change_ambiguous",
            "Two operations address the same target cell.",
        ));
    }
    if changes.len() >= MAX_CHANGES {
        return Err(error(
            "repair_bound_exceeded",
            "Proposal exceeds 5000 changed cells; narrow the explicit scope.",
        ));
    }
    let before_feature_digest = hash(feature)?;
    let change_id = hash(&json!([
        layer,
        id,
        before_feature_digest,
        field,
        before,
        after,
        evidence
    ]))?;
    changes.push(CellChange {
        id: change_id,
        layer: layer.into(),
        feature_id: id,
        before_feature_digest,
        field: field.into(),
        before,
        after,
        evidence,
    });
    Ok(())
}
fn check_target(target: &Snapshot, fence: &TargetFence) -> Result<(), RepairError> {
    target
        .validate_retained()
        .map_err(|e| error("repair_target_invalid", e))?;
    if !sha(&fence.snapshot_revision) || revision(target)? != fence.snapshot_revision {
        return Err(error(
            "repair_target_changed",
            "Target differs from the reviewed exact snapshot revision.",
        ));
    }
    if fence.server_version.is_some() != fence.layers_content_digest.is_some()
        || fence.server_version == Some(0)
    {
        return Err(error("repair_target_invalid", "Server version and layer digest must be present together and version must be positive."));
    }
    if let Some(expected) = &fence.layers_content_digest {
        let actual = jcs::layers_content_digest(&target.layers)
            .map_err(|e| error("repair_target_invalid", e))?;
        if !sha(expected) || expected != &actual {
            return Err(error(
                "repair_target_changed",
                "Target layer bytes differ from their captured server content digest.",
            ));
        }
    }
    Ok(())
}
fn proposal_hash(proposal: &Proposal) -> Result<String, RepairError> {
    let mut value = serde_json::to_value(proposal)
        .map_err(|e| error("repair_document_invalid", e.to_string()))?;
    value.as_object_mut().unwrap().remove("digest");
    hash(&value)
}
pub fn propose(
    source: Option<&EvidenceSource>,
    target: &Snapshot,
    request: &ProposalRequest,
) -> Result<Proposal, RepairError> {
    request.scope.validate()?;
    validate_policy(&request.policy)?;
    bounded(request, MAX_POLICY_BYTES)?;
    if request.scope != request.policy.scope {
        return Err(error(
            "repair_scope_mismatch",
            "Proposal policy belongs to a different scope.",
        ));
    }
    if request.operations.is_empty()
        || request.operations.len() > 64
        || request.allowlist.len() > 64
    {
        return Err(error(
            "repair_bound_exceeded",
            "Declare 1..64 repair operations.",
        ));
    }
    for (layer, fields) in &request.allowlist {
        name(layer)?;
        if fields.is_empty() || fields.len() > 256 {
            return Err(error(
                "repair_field_invalid",
                "Allowlist requires 1..256 fields per layer.",
            ));
        }
        for field in fields {
            name(field)?;
            rule(request, layer, field)?;
        }
    }
    check_target(target, &request.target)?;
    if let Some(source) = source {
        validate_source(source, &request.scope)?;
    }
    let mut changes = Vec::new();
    let mut seen = BTreeSet::new();
    let mut match_counts = Vec::new();
    for operation in &request.operations {
        match operation {
            RepairOperation::Restore {
                source_layer,
                target_layer,
                matching,
                fields,
            } => {
                field_maps(fields)?;
                let source = source.ok_or_else(|| {
                    error(
                        "repair_source_required",
                        "Restoration requires pinned source evidence.",
                    )
                })?;
                let left = layer(&source.layers, source_layer)?;
                let right = layer(&target.layers, target_layer)?;
                let matched = match_records(
                    left,
                    right,
                    matching,
                    source.crs.as_deref(),
                    Some(&target.crs),
                )?;
                if matched.counts.ambiguous_source + matched.counts.ambiguous_target > 0 {
                    return Err(error("repair_match_ambiguous", "Source/target matching is not one-to-one; review duplicate evidence before proposing changes."));
                }
                let index = identities(right);
                for field in fields {
                    let policy = rule(request, target_layer, &field.target)?;
                    if let Some(expected) = &policy.unit {
                        if source
                            .units
                            .get(source_layer)
                            .and_then(|fields| fields.get(&field.source))
                            != Some(expected)
                        {
                            return Err(error("repair_unit_mismatch", format!("{source_layer}.{} must declare unit {expected}; convert explicitly before reconciliation.", field.source)));
                        }
                    }
                }
                for [source_index, target_index] in matched.pairs {
                    let id = stable_id(&right[target_index], &index)?;
                    for field in fields {
                        let after = Cell::read(&left[source_index], &field.source);
                        if after == Cell::Missing && !field.remove_if_source_missing {
                            continue;
                        }
                        add_change(
                            &mut changes,
                            &mut seen,
                            request,
                            target_layer,
                            &right[target_index],
                            id.clone(),
                            &field.target,
                            after,
                            Some(SourceCellEvidence {
                                layer: source_layer.clone(),
                                row: source_index,
                                feature_digest: hash(&left[source_index])?,
                                field: field.source.clone(),
                            }),
                        )?;
                    }
                }
                match_counts.push(matched.counts);
            }
            RepairOperation::Assign {
                target_layer,
                target_ids,
                values,
            } => {
                if target_ids.is_empty()
                    || target_ids.len() > MAX_CHANGES
                    || values.is_empty()
                    || values.len() > 256
                {
                    return Err(error(
                        "repair_bound_exceeded",
                        "Assignment requires explicit bounded IDs and fields.",
                    ));
                }
                let right = layer(&target.layers, target_layer)?;
                let index = identities(right);
                let mut assigned = BTreeSet::new();
                for id in target_ids {
                    let rows = index.get(id).ok_or_else(|| {
                        error(
                            "repair_identity_required",
                            "An explicitly selected assignment target is absent.",
                        )
                    })?;
                    if rows.len() != 1 {
                        return Err(error(
                            "repair_identity_ambiguous",
                            "Assignment target identity is ambiguous.",
                        ));
                    }
                    let feature = &right[rows[0]];
                    let canonical = stable_id(feature, &index)?;
                    if !assigned.insert(canonical.clone()) {
                        return Err(error(
                            "repair_change_ambiguous",
                            "Repeated assignment target or identity alias.",
                        ));
                    }
                    for (field, after) in values {
                        add_change(
                            &mut changes,
                            &mut seen,
                            request,
                            target_layer,
                            feature,
                            canonical.clone(),
                            field,
                            after.clone(),
                            None,
                        )?;
                    }
                }
            }
        }
    }
    changes.sort_by(|a, b| {
        (&a.layer, &a.feature_id, &a.field).cmp(&(&b.layer, &b.feature_id, &b.field))
    });
    let projected = project_cells(target, &changes)?;
    let mut proposal = Proposal {
        schema: "ds.repair.proposal/v1".into(),
        request: request.clone(),
        source_digest: source.map(hash).transpose()?,
        match_counts,
        changes,
        target_after_revision: revision(&projected)?,
        digest: String::new(),
    };
    proposal.digest = proposal_hash(&proposal)?;
    bounded(&proposal, MAX_PROPOSAL_BYTES)?;
    Ok(proposal)
}
fn project_cells(target: &Snapshot, changes: &[CellChange]) -> Result<Snapshot, RepairError> {
    let mut next = target.clone();
    let mut offsets: BTreeMap<(String, String), usize> = BTreeMap::new();
    let mut layer_identities = BTreeMap::new();
    for change in changes {
        let key = (change.layer.clone(), change.feature_id.clone());
        if !offsets.contains_key(&key) {
            let features = layer(&target.layers, &change.layer)?;
            let index = layer_identities
                .entry(change.layer.clone())
                .or_insert_with(|| identities(features));
            let positions = index.get(&change.feature_id).ok_or_else(|| {
                error(
                    "repair_target_changed",
                    "Selected target identity disappeared.",
                )
            })?;
            if positions.len() != 1 {
                return Err(error(
                    "repair_identity_ambiguous",
                    "Selected identity is ambiguous.",
                ));
            }
            let offset = positions[0];
            stable_id(&features[offset], index)?;
            if hash(&features[offset])? != change.before_feature_digest {
                return Err(error(
                    "repair_target_changed",
                    "Selected feature differs from reviewed bytes.",
                ));
            }
            offsets.insert(key.clone(), offset);
        }
        let offset = offsets[&key];
        let original = &target.layers[&change.layer]["features"][offset];
        if Cell::read(original, &change.field) != change.before {
            return Err(error(
                "repair_target_changed",
                "Selected cell differs from its reviewed before value.",
            ));
        }
        let properties = next.layers.get_mut(&change.layer).unwrap()["features"][offset]
            ["properties"]
            .as_object_mut()
            .unwrap();
        match &change.after {
            Cell::Missing => {
                properties.remove(&change.field);
            }
            Cell::Value { value } => {
                properties.insert(change.field.clone(), value.clone());
            }
        }
    }
    // Verify by undoing only selected cells. Every other byte-valued fact,
    // including feature order, geometry, counts and snapshot inputs, must equal
    // the original. No normalizer is allowed on this path.
    let mut restored = next.clone();
    for change in changes {
        let offset = offsets[&(change.layer.clone(), change.feature_id.clone())];
        let properties = restored.layers.get_mut(&change.layer).unwrap()["features"][offset]
            ["properties"]
            .as_object_mut()
            .unwrap();
        match &change.before {
            Cell::Missing => {
                properties.remove(&change.field);
            }
            Cell::Value { value } => {
                properties.insert(change.field.clone(), value.clone());
            }
        }
    }
    if serde_json::to_value(&restored).unwrap() != serde_json::to_value(target).unwrap() {
        return Err(error(
            "repair_preservation_refused",
            "Projection changed facts outside the selected cells.",
        ));
    }
    next.validate_retained()
        .map_err(|e| error("repair_target_invalid", e))?;
    Ok(next)
}
pub fn apply_selected(
    source: Option<&EvidenceSource>,
    target: &Snapshot,
    proposal: &Proposal,
    selection: &ApplySelection,
) -> Result<AppliedRepair, RepairError> {
    bounded(proposal, MAX_PROPOSAL_BYTES)?;
    if !selection.confirmed {
        return Err(error(
            "repair_effect_unconfirmed",
            "Explicit selected apply requires confirmation.",
        ));
    }
    selection.scope.validate()?;
    if selection.scope != proposal.request.scope {
        return Err(error(
            "repair_scope_mismatch",
            "Selected apply belongs to a different project/principal/lane/audience.",
        ));
    }
    if proposal.schema != "ds.repair.proposal/v1"
        || proposal.digest != selection.proposal_digest
        || proposal_hash(proposal)? != proposal.digest
    {
        return Err(error(
            "repair_proposal_changed",
            "Proposal differs from the reviewed digest.",
        ));
    }
    // A hash is not evidence: independently rederive the whole proposal using
    // actual source/target facts and the recorded closed operation request.
    let fresh = propose(source, target, &proposal.request)?;
    if serde_json::to_value(&fresh).unwrap() != serde_json::to_value(proposal).unwrap() {
        return Err(error(
            "repair_proposal_changed",
            "Proposal is not the projection of its recorded evidence.",
        ));
    }
    if selection.change_ids.is_empty() || selection.change_ids.len() > MAX_CHANGES {
        return Err(error(
            "repair_selection_invalid",
            "Select 1..5000 exact proposed change IDs.",
        ));
    }
    let mut selected = BTreeSet::new();
    for id in &selection.change_ids {
        if !selected.insert(id.as_str()) {
            return Err(error(
                "repair_selection_invalid",
                "A change ID is repeated.",
            ));
        }
    }
    let changes = proposal
        .changes
        .iter()
        .filter(|change| selected.contains(change.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    if changes.len() != selected.len() {
        return Err(error(
            "repair_selection_invalid",
            "An explicitly selected change is absent from this proposal.",
        ));
    }
    let snapshot = project_cells(target, &changes)?;
    let after_revision = revision(&snapshot)?;
    if changes.len() == proposal.changes.len() && after_revision != proposal.target_after_revision {
        return Err(error(
            "repair_proposal_changed",
            "Projected result digest differs from proposal.",
        ));
    }
    Ok(AppliedRepair {
        schema: "ds.repair.projected/v1".into(),
        proposal_digest: proposal.digest.clone(),
        selected_change_ids: changes.iter().map(|change| change.id.clone()).collect(),
        before_revision: proposal.request.target.snapshot_revision.clone(),
        after_revision,
        changed_cells: changes.len(),
        preserved_outside_selection: true,
        snapshot,
        persisted: false,
    })
}

struct Findings {
    report: AuditReport,
    limit: usize,
}
impl Findings {
    fn add(
        &mut self,
        target: &Snapshot,
        layer: &str,
        index: Option<usize>,
        feature: Option<&Value>,
        field: Option<&str>,
        code: &str,
        severity: &str,
        evidence: Value,
    ) {
        *self.report.finding_counts.entry(code.into()).or_default() += 1;
        self.report.blockers += usize::from(severity == "blocker");
        self.report.candidates += usize::from(severity == "candidate");
        if self.report.findings.len() < self.limit {
            self.report.findings.push(AuditFinding {
                code: code.into(),
                severity: severity.into(),
                transformer: target.transformer.clone(),
                layer: layer.into(),
                feature_index: index,
                feature_id: feature.and_then(|feature| {
                    geojson_feature_identity_candidates(feature)
                        .into_iter()
                        .next()
                }),
                field: field.map(str::to_owned),
                evidence,
            });
        } else {
            self.report.omitted_details += 1;
        }
    }
}
pub fn audit(target: &Snapshot, policy: &AuditPolicy) -> Result<AuditReport, RepairError> {
    audit_scope(std::slice::from_ref(target), policy)
}
/// Cross-transformer collisions are candidates, never automatic deletions.
pub fn audit_scope(targets: &[Snapshot], policy: &AuditPolicy) -> Result<AuditReport, RepairError> {
    validate_policy(policy)?;
    if targets.is_empty() || targets.len() > 2000 {
        return Err(error(
            "repair_bound_exceeded",
            "Audit requires 1..2000 explicitly scoped snapshots.",
        ));
    }
    bounded(&targets, design::MAX_BYTES)?;
    let mut findings = Findings {
        limit: policy.max_details,
        report: AuditReport {
            schema: "ds.repair.audit/v1".into(),
            scope: policy.scope.clone(),
            input_digest: hash(&targets)?,
            policy_digest: hash(policy)?,
            transformers: targets.len(),
            features: 0,
            finding_counts: BTreeMap::new(),
            blockers: 0,
            candidates: 0,
            findings: vec![],
            omitted_details: 0,
            mutated: false,
        },
    };
    let mut owners = BTreeSet::new();
    let mut positions: BTreeMap<String, (String, String)> = BTreeMap::new();
    for target in targets {
        if target.schema != design::SCHEMA
            || target.crs != "EPSG:4326"
            || !owners.insert(&target.transformer)
            || target.layers.is_empty()
            || target.layers.len() > 64
        {
            return Err(error("repair_target_invalid", "Audit snapshots need distinct transformer owners, admitted schema/CRS and 1..64 layers."));
        }
        design::transformer_name(&target.transformer)
            .map_err(|e| error("repair_target_invalid", e))?;
        for (layer_name, collection) in &target.layers {
            name(layer_name)?;
            let features = rows(collection)?;
            findings.report.features += features.len();
            if findings.report.features > design::MAX_FEATURES {
                return Err(error(
                    "repair_bound_exceeded",
                    "Audit exceeds 100,000 aggregate records.",
                ));
            }
            let index = identities(features);
            for (i, feature) in features.iter().enumerate() {
                if design::validate_geometry(&feature["geometry"]).is_err() {
                    findings.add(
                        target,
                        layer_name,
                        Some(i),
                        Some(feature),
                        Some("geometry"),
                        "geometry_invalid",
                        "blocker",
                        json!({"recorded_type":feature["geometry"]["type"]}),
                    );
                } else if let Ok(expected) = design::authored_geometry(layer_name) {
                    if feature["geometry"]["type"] != expected {
                        findings.add(
                            target, layer_name, Some(i), Some(feature), Some("geometry"),
                            "geometry_class_invalid", "blocker",
                            json!({"recorded_type":feature["geometry"]["type"],"expected":expected}),
                        );
                    }
                }
                let aliases = geojson_feature_identity_candidates(feature);
                if aliases.is_empty() {
                    findings.add(
                        target,
                        layer_name,
                        Some(i),
                        Some(feature),
                        Some("id"),
                        "identity_missing",
                        "candidate",
                        json!({"mutation_requires_stable_identity":true}),
                    );
                } else if aliases.iter().any(|id| index[id].len() != 1) {
                    findings.add(
                        target,
                        layer_name,
                        Some(i),
                        Some(feature),
                        Some("id"),
                        "identity_ambiguous",
                        "blocker",
                        json!({"aliases":aliases}),
                    );
                }
                if let Some(owner) = feature["properties"].get("transfo") {
                    if owner != &json!(target.transformer) {
                        findings.add(
                            target,
                            layer_name,
                            Some(i),
                            Some(feature),
                            Some("transfo"),
                            "owner_reference_mismatch",
                            "blocker",
                            json!({"recorded":owner,"expected":target.transformer}),
                        );
                    }
                }
                if let Some(fields) = policy.fields.get(layer_name) {
                    for (field, rule) in fields {
                        let cell = Cell::read(feature, field);
                        if !cell_allowed(&cell, rule) {
                            findings.add(
                                target,
                                layer_name,
                                Some(i),
                                Some(feature),
                                Some(field),
                                "field_policy_invalid",
                                "blocker",
                                json!({"cell":cell,"declared_unit":rule.unit}),
                            );
                        }
                    }
                }
                if feature["geometry"]["type"] == "Point" {
                    if let Ok(key) = position_key(feature, &PositionDimensions::Xy) {
                        let key = format!("{layer_name}:{key}");
                        if let Some((owner, old_layer)) = positions.get(&key) {
                            if owner != &target.transformer {
                                findings.add(target, layer_name, Some(i), Some(feature), None, "cross_transformer_colocation", "candidate", json!({"other_transformer":owner,"other_layer":old_layer,"deletion_proven":false}));
                            }
                        } else {
                            positions.insert(key, (target.transformer.clone(), layer_name.clone()));
                        }
                    }
                }
            }
        }
        audit_references(target, policy, &mut findings)?;
        if let Some(connectivity) = &policy.connectivity {
            audit_connectivity(target, connectivity, &mut findings)?;
        }
    }
    Ok(findings.report)
}
fn audit_references(
    target: &Snapshot,
    policy: &AuditPolicy,
    findings: &mut Findings,
) -> Result<(), RepairError> {
    for rule in &policy.references {
        let mut referenced = BTreeMap::<String, usize>::new();
        for target_layer in &rule.target_layers {
            for feature in layer(&target.layers, target_layer)? {
                if let Some(value) = feature["properties"]
                    .get(&rule.target_field)
                    .filter(|v| !v.is_null())
                {
                    *referenced.entry(hash(value)?).or_default() += 1;
                }
            }
        }
        for (i, feature) in layer(&target.layers, &rule.layer)?.iter().enumerate() {
            let cell = Cell::read(feature, &rule.field);
            let issue = match &cell {
                Cell::Missing | Cell::Value { value: Value::Null } if rule.required => {
                    Some(("reference_missing", "blocker"))
                }
                Cell::Missing | Cell::Value { value: Value::Null } => None,
                Cell::Value { value } => match referenced.get(&hash(value)?) {
                    None => Some(("reference_unresolved", "candidate")),
                    Some(1) => None,
                    Some(_) => Some(("reference_ambiguous", "candidate")),
                },
            };
            if let Some((code, severity)) = issue {
                findings.add(target, &rule.layer, Some(i), Some(feature), Some(&rule.field), code, severity, json!({"cell":cell,"target_layers":rule.target_layers,"target_field":rule.target_field,"deletion_proven":false}));
            }
        }
    }
    Ok(())
}
fn audit_connectivity(
    target: &Snapshot,
    policy: &ConnectivityPolicy,
    findings: &mut Findings,
) -> Result<(), RepairError> {
    if !policy.sources_complete {
        findings.add(
            target,
            "",
            None,
            None,
            None,
            "connectivity_source_incomplete",
            "candidate",
            json!({"authoritative_orphan_verdict":false}),
        );
        return Ok(());
    }
    let mut graph: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut lines = Vec::new();
    for layer_name in &policy.line_layers {
        for (i, feature) in layer(&target.layers, layer_name)?.iter().enumerate() {
            let Some(points) = feature["geometry"]["coordinates"]
                .as_array()
                .filter(|_| feature["geometry"]["type"] == "LineString")
            else {
                continue;
            };
            let mut keys = Vec::new();
            for point in points {
                if let Some(point) = point.as_array().filter(|point| point.len() >= 2) {
                    keys.push(
                        jcs::sha256(&json!(&point[..2]))
                            .map_err(|e| error("repair_geometry_invalid", e))?,
                    );
                }
            }
            for pair in keys.windows(2) {
                graph
                    .entry(pair[0].clone())
                    .or_default()
                    .insert(pair[1].clone());
                graph
                    .entry(pair[1].clone())
                    .or_default()
                    .insert(pair[0].clone());
            }
            lines.push((layer_name, i, feature, keys));
        }
    }
    let mut pending = Vec::new();
    for root in &policy.roots {
        let features = layer(&target.layers, &root.layer)?;
        let index = identities(features);
        let positions = index
            .get(&root.id)
            .ok_or_else(|| error("repair_root_unknown", "A declared graph root is absent."))?;
        if positions.len() != 1 {
            return Err(error(
                "repair_identity_ambiguous",
                "Declared graph root is ambiguous.",
            ));
        }
        pending.push(position_key(
            &features[positions[0]],
            &PositionDimensions::Xy,
        )?);
    }
    if pending.is_empty() {
        return Err(error("repair_root_unknown", "Connectivity requires explicit accepted roots, including tapping roots where applicable."));
    }
    let mut reached = BTreeSet::new();
    while let Some(key) = pending.pop() {
        if reached.insert(key.clone()) {
            if let Some(neighbours) = graph.get(&key) {
                pending.extend(neighbours.iter().filter(|k| !reached.contains(*k)).cloned());
            }
        }
    }
    for (layer_name, i, feature, keys) in lines {
        if !keys.iter().any(|key| reached.contains(key)) {
            findings.add(target, layer_name, Some(i), Some(feature), None, "connectivity_unreached_candidate", "candidate", json!({"predicate":"exact_recorded_vertices","deletion_proven":false,"not_engineering_processing":true}));
        }
    }
    Ok(())
}

/// Closed JSON adapter shared by CLI/MCP and the existing typed workflow DAG.
/// Every operation is pure; apply_selected returns persisted=false.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct AuditInput {
    targets: Vec<Snapshot>,
    policy: AuditPolicy,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct CompareInput {
    source: EvidenceSource,
    target: Snapshot,
    request: CompareRequest,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ProposeInput {
    source: Option<EvidenceSource>,
    target: Snapshot,
    request: ProposalRequest,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
struct ApplyInput {
    source: Option<EvidenceSource>,
    target: Snapshot,
    proposal: Proposal,
    selection: ApplySelection,
}

pub fn evaluate(operation: &str, value: Value) -> Result<Value, RepairError> {
    bounded(&value, design::MAX_BYTES * 2)?;
    fn decode<T: serde::de::DeserializeOwned>(value: Value) -> Result<T, RepairError> {
        serde_json::from_value(value).map_err(|e| error("repair_document_invalid", e.to_string()))
    }
    fn encode<T: Serialize>(value: T) -> Result<Value, RepairError> {
        serde_json::to_value(value).map_err(|e| error("repair_document_invalid", e.to_string()))
    }
    match operation {
        "audit" | "design.repair.audit" => {
            let input: AuditInput = decode(value)?;
            encode(audit_scope(&input.targets, &input.policy)?)
        }
        "compare" | "design.repair.compare" => {
            let input: CompareInput = decode(value)?;
            encode(compare(&input.source, &input.target, &input.request)?)
        }
        "propose" | "design.repair.propose" => {
            let input: ProposeInput = decode(value)?;
            encode(propose(
                input.source.as_ref(),
                &input.target,
                &input.request,
            )?)
        }
        "apply_selected" | "design.repair.apply-selected" => {
            let input: ApplyInput = decode(value)?;
            encode(apply_selected(
                input.source.as_ref(),
                &input.target,
                &input.proposal,
                &input.selection,
            )?)
        }
        _ => Err(error(
            "repair_operation_unknown",
            "Choose audit, compare, propose or apply_selected.",
        )),
    }
}
pub fn describe() -> Value {
    fn schema<T: schemars::JsonSchema>() -> Value {
        let raw = serde_json::to_value(schemars::schema_for!(T)).expect("generated JSON schema");
        fn expand(value: &Value, root: &Value, depth: usize) -> Value {
            assert!(depth < 64, "repair schema must be non-recursive");
            if let Some(reference) = value["$ref"].as_str().and_then(|r| r.strip_prefix('#')) {
                return expand(
                    root.pointer(reference).expect("generated schema reference"),
                    root,
                    depth + 1,
                );
            }
            match value {
                Value::Object(map) => Value::Object(
                    map.iter()
                        .filter(|(key, _)| key.as_str() != "$defs")
                        .map(|(key, value)| (key.clone(), expand(value, root, depth + 1)))
                        .collect(),
                ),
                Value::Array(values) => Value::Array(
                    values
                        .iter()
                        .map(|value| expand(value, root, depth + 1))
                        .collect(),
                ),
                _ => value.clone(),
            }
        }
        expand(&raw, &raw, 0)
    }
    fn operation(id: &str, input: Value, output: Value) -> Value {
        json!({"id":id,"version":1,"status":"available","pure":true,"input_schema":input,"output_schema":output})
    }
    json!({"schema":"ds.repair.operations/v1","version":1,"pure":true,"effects":"none; apply_selected returns a projected snapshot, never a save","operations":[
        operation("design.repair.audit", schema::<AuditInput>(), schema::<AuditReport>()),
        operation("design.repair.compare", schema::<CompareInput>(), schema::<Comparison>()),
        operation("design.repair.propose", schema::<ProposeInput>(), schema::<Proposal>()),
        operation("design.repair.apply-selected", schema::<ApplyInput>(), schema::<AppliedRepair>())
    ],"bounds":{"source_bytes":MAX_SOURCE_BYTES,"proposal_bytes":MAX_PROPOSAL_BYTES,"changes":MAX_CHANGES,"details":MAX_DETAILS,"aggregate_records":design::MAX_FEATURES},"matching":["exact_position (explicit EPSG:4326, xy or xyz)","keys (typed scalar pairs, null/missing unmatched, duplicates ambiguous)"],"cell":{"missing":{"kind":"missing"},"null":{"kind":"value","value":null}},"contract":"docs/contracts/governed-design-repair.md"})
}
