//! Map-independent LV document and edit contracts. No storage, transport or compute.
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "ds.design.snapshot/v1";
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_FEATURES: usize = 100_000;
pub const MAX_EDITS: usize = 5_000;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProcessPlanRequest {
    targets: Vec<String>,
    workers: usize,
    resident_limit: usize,
    #[serde(default)]
    settings: Map<String, Value>,
    #[serde(default)]
    overrides: Map<String, Value>,
}

/// Offline processing scope and concurrency. Hosts supply measured worker
/// capacity and perform IO; cloud synchronization is never a completion gate.
pub fn process_plan(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > 512 * 1024 {
        return Err("Design process plan exceeds 512 KiB".into());
    }
    let request: ProcessPlanRequest = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    for (key, value) in request.settings.iter().chain(request.overrides.iter()) {
        if key.is_empty()
            || key.len() > 128
            || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            || !(value.is_boolean() || value.is_number())
        {
            return Err(
                "Process settings must have bounded names and boolean or numeric values".into(),
            );
        }
    }
    let mut settings = request.settings;
    settings.extend(request.overrides);
    if request.targets.len() > 2000
        || request.workers == 0
        || request.resident_limit == 0
        || request.resident_limit > 100
    {
        return Err("Invalid design process scope or worker budget".into());
    }
    let mut seen = BTreeSet::new();
    let mut names = Vec::new();
    for target in request.targets {
        let name = target.trim().to_ascii_lowercase();
        if name.is_empty() {
            continue;
        }
        transformer_name(&name)?;
        if seen.insert(name.clone()) {
            names.push(name);
        }
    }
    let concurrency = names.len().min(request.workers).min(request.resident_limit);
    Ok(
        json!({"schema":"ds.design.process-plan/v1", "names":names, "concurrency":concurrency,
        "input_source":"local", "completion":"local_commit", "cloud_save":"separate",
        "config_source":"cached_project", "settings":settings}),
    )
}

#[cfg(feature = "wasm")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = designProcessPlan)]
pub fn process_plan_wasm(input: &[u8]) -> Result<String, wasm_bindgen::JsValue> {
    process_plan(input)
        .and_then(|v| serde_json::to_string(&v).map_err(|e| e.to_string()))
        .map_err(|e| wasm_bindgen::JsValue::from_str(&e))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSelection {
    pub addresses: Vec<String>,
    pub labels: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceResolution {
    pub schema: String,
    pub kind: SourceKind,
    pub project: Option<SourceSelection>,
    pub user: Option<SourceSelection>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Customers,
    Poles,
}

/// Declared project settings win. An explicit empty pole list disables source
/// borrowing; an empty customer list is refused, never replaced silently.
pub fn resolve_sources(bytes: &[u8]) -> Result<Value, String> {
    if bytes.len() > 64 * 1024 {
        return Err("source selection exceeds 64 KiB".into());
    }
    let request: SourceResolution = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    if request.schema != "ds.design.source-resolution/v1" {
        return Err("unsupported source resolution schema".into());
    }
    let (selection, scope) = match (request.project, request.user) {
        (Some(selection), _) => (selection, "project"),
        (None, Some(selection)) => (selection, "user"),
        (None, None) => match request.kind {
            SourceKind::Customers => (
                SourceSelection {
                    addresses: vec!["design://layer/customers".into()],
                    labels: vec!["Design customers".into()],
                },
                "default",
            ),
            SourceKind::Poles => (
                SourceSelection {
                    addresses: vec![],
                    labels: vec![],
                },
                "default",
            ),
        },
    };
    if selection.addresses.len() > 64 || selection.addresses.len() != selection.labels.len() {
        return Err("expected at most 64 source addresses with matching labels".into());
    }
    let mut seen = BTreeSet::new();
    for address in &selection.addresses {
        if address.is_empty()
            || address.trim() != address
            || address.len() > 1024
            || address.chars().any(char::is_control)
            || !seen.insert(address)
        {
            return Err("source addresses must be nonempty, bounded and unique".into());
        }
    }
    if selection
        .labels
        .iter()
        .any(|s| s.len() > 1024 || s.chars().any(char::is_control))
    {
        return Err("source label exceeds bound".into());
    }
    if matches!(request.kind, SourceKind::Customers) && selection.addresses.is_empty() {
        return Err(
            "customer sources are explicitly empty; choose design customers or a dataset".into(),
        );
    }
    Ok(
        json!({"schema":"ds.design.source-selection/v1","addresses":selection.addresses,"labels":selection.labels,"scope":scope}),
    )
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub schema: String,
    pub transformer: String,
    pub crs: String,
    pub layers: BTreeMap<String, Value>,
    /// Explicit engine settings. Empty means the pinned engine's defaults.
    pub settings: Map<String, Value>,
    pub network_config: NetworkConfig,
    pub include_design_customers: bool,
    pub sources: Vec<Source>,
}

/// Compare immutable Design inputs without a renderer. Geometry/attribute
/// classification is the same owner used by retained transformer versions.
/// Full totals remain exact when detail rows reach the caller's bound.
pub fn compare_snapshots(left: &Snapshot, right: &Snapshot, limit: usize) -> Result<Value, String> {
    if left.transformer != right.transformer || left.crs != right.crs {
        return Err("comparison requires the same transformer and CRS".into());
    }
    if limit > 500 {
        return Err("comparison details must be bounded to at most 500 features".into());
    }
    let comparison = ds_geo_lite::comparison::compare_geojson_layers(
        ds_geo_lite::comparison::GeoJsonLayerComparisonInput {
            local_layers: left.layers.clone(),
            cloud_layers: right.layers.clone(),
            layer_names: vec![],
            ignored_property_keys: vec!["v_first".into(), "v_last".into()],
            max_detail_features: limit,
            detail_change_classes: vec![],
        },
    )?;
    let left_value = serde_json::to_value(left).map_err(|e| e.to_string())?;
    let right_value = serde_json::to_value(right).map_err(|e| e.to_string())?;
    let input_changes: Vec<&str> = [
        "settings",
        "network_config",
        "sources",
        "include_design_customers",
    ]
    .into_iter()
    .filter(|key| left_value[*key] != right_value[*key])
    .collect();
    Ok(
        json!({"schema":"ds.design.snapshot-comparison/v1", "transformer":left.transformer,
        "comparison":comparison,"input_changes":input_changes,"mutated":false}),
    )
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkConfig {
    pub sheets: Map<String, Value>,
    #[serde(default)]
    pub groups: Map<String, Value>,
    #[serde(default)]
    pub models: Map<String, Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub role: SourceRole,
    pub sha256: String,
    pub collection: Value,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceRole {
    Customers,
    Buildings,
    Poles,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub schema: String,
    pub transformer: String,
    pub expected_revision: String,
    pub operation_id: String,
    pub mutations: Vec<Mutation>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Mutation {
    Add {
        layer: String,
        features: Vec<Value>,
    },
    SetProperties {
        layer: String,
        ids: Vec<String>,
        values: Map<String, Value>,
    },
    ReplaceGeometry {
        layer: String,
        id: String,
        geometry: Value,
    },
    Delete {
        layer: String,
        ids: Vec<String>,
    },
}

pub fn digest(value: &impl Serialize) -> Result<String, String> {
    fn ordered(v: Value) -> Value {
        match v {
            Value::Object(m) => Value::Object(
                m.into_iter()
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .map(|(k, v)| (k, ordered(v)))
                    .collect(),
            ),
            Value::Array(a) => Value::Array(a.into_iter().map(ordered).collect()),
            v => v,
        }
    }
    let bytes = serde_json::to_vec(&ordered(
        serde_json::to_value(value).map_err(|e| e.to_string())?,
    ))
    .map_err(|e| e.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn identifier(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 120
        || value.trim() != value
        || value.chars().any(|c| c.is_control() || "/\\".contains(c))
        || matches!(value, "." | "..")
    {
        return Err(
            "identifier must be 1..120 bytes without path separators or control characters".into(),
        );
    }
    Ok(())
}

pub fn transformer_name(value: &str) -> Result<(), String> {
    identifier(value)?;
    if matches!(
        value,
        "mv_data" | "combined_transformer" | "all_transformers" | "collisions"
    ) || value.starts_with("collisions_")
    {
        return Err("reserved transformer identity".into());
    }
    Ok(())
}

pub fn decode(bytes: &[u8]) -> Result<Snapshot, String> {
    if bytes.len() > MAX_BYTES {
        return Err("snapshot exceeds 64 MiB".into());
    }
    let mut snapshot: Snapshot = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    snapshot.normalize()?;
    Ok(snapshot)
}

fn authored_geometry(layer: &str) -> Result<&'static str, String> {
    match layer {
        "tr" | "customers" | "lv_poles" | "tapping_poles" | "field_notes" => Ok("Point"),
        "lv_lines" | "service_cables" => Ok("LineString"),
        _ => Err(format!("{layer} is not an authorable LV layer")),
    }
}

fn position(v: &Value) -> bool {
    v.as_array().is_some_and(|a| {
        (2..=3).contains(&a.len())
            && a.iter().all(|v| v.as_f64().is_some_and(f64::is_finite))
            && a[0].as_f64().is_some_and(|v| (-180.0..=180.0).contains(&v))
            && a[1].as_f64().is_some_and(|v| (-90.0..=90.0).contains(&v))
    })
}
fn line(v: &Value) -> bool {
    v.as_array()
        .is_some_and(|a| a.len() >= 2 && a.iter().all(position))
}
fn polygon(v: &Value) -> bool {
    v.as_array().is_some_and(|a| {
        !a.is_empty()
            && a.iter().all(|r| {
                r.as_array().is_some_and(|a| {
                    a.len() >= 4 && a.first() == a.last() && a.iter().all(position)
                })
            })
    })
}
pub fn validate_geometry(v: &Value) -> Result<(), String> {
    let c = &v["coordinates"];
    let valid = match v["type"].as_str() {
        Some("Point") => position(c),
        Some("LineString") => line(c),
        Some("Polygon") => polygon(c),
        Some("MultiPoint") => c
            .as_array()
            .is_some_and(|a| !a.is_empty() && a.iter().all(position)),
        Some("MultiLineString") => c
            .as_array()
            .is_some_and(|a| !a.is_empty() && a.iter().all(line)),
        Some("MultiPolygon") => c
            .as_array()
            .is_some_and(|a| !a.is_empty() && a.iter().all(polygon)),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err("invalid or unsupported WGS84 geometry".into())
    }
}

fn features_mut(collection: &mut Value) -> Result<&mut Vec<Value>, String> {
    if collection["type"] != "FeatureCollection" {
        return Err("layer must be a FeatureCollection".into());
    }
    collection
        .get_mut("features")
        .and_then(Value::as_array_mut)
        .ok_or("features must be an array".into())
}

impl Snapshot {
    pub fn normalize(&mut self) -> Result<(), String> {
        if self.schema != SCHEMA || self.crs != "EPSG:4326" {
            return Err("expected ds.design.snapshot/v1 and EPSG:4326".into());
        }
        transformer_name(&self.transformer)?;
        if self.layers.is_empty() || self.layers.len() > 64 || self.sources.len() > 64 {
            return Err("expected 1..64 layers and at most 64 sources".into());
        }
        let mut count = 0;
        for (layer, collection) in &mut self.layers {
            identifier(layer)?;
            let rows = features_mut(collection)?;
            count += rows.len();
            let mut ids = BTreeSet::new();
            for (index, feature) in rows.iter_mut().enumerate() {
                if feature["type"] != "Feature" || !feature["properties"].is_object() {
                    return Err("expected Feature with object properties".into());
                }
                validate_geometry(&feature["geometry"])?;
                if feature.get("id").is_none() {
                    feature["id"] =
                        json!(format!("f-{}", digest(&json!([layer, index, feature]))?));
                }
                let id = feature["id"]
                    .as_str()
                    .ok_or("feature id must be a nonempty string")?;
                identifier(id)?;
                if !ids.insert(id.to_owned()) {
                    return Err(format!("duplicate feature identity in {layer}"));
                }
                feature["properties"]["transfo"] = json!(self.transformer);
            }
        }
        let mut source_ids = BTreeSet::new();
        for source in &mut self.sources {
            identifier(&source.id)?;
            if !source_ids.insert(&source.id) {
                return Err("duplicate source id".into());
            }
            if digest(&source.collection)? != source.sha256 {
                return Err(format!("source digest mismatch: {}", source.id));
            }
            let rows = features_mut(&mut source.collection)?;
            count += rows.len();
            for feature in rows {
                if feature["type"] != "Feature" || !feature["properties"].is_object() {
                    return Err("invalid source feature".into());
                }
                validate_geometry(&feature["geometry"])?;
                let kind = feature["geometry"]["type"].as_str().unwrap_or("");
                if matches!(source.role, SourceRole::Buildings)
                    && !matches!(kind, "Point" | "Polygon" | "MultiPolygon")
                {
                    return Err("building source requires point or polygon features".into());
                }
                if !matches!(source.role, SourceRole::Buildings) && kind != "Point" {
                    return Err("customer/pole source requires point features".into());
                }
            }
        }
        if count > MAX_FEATURES {
            return Err("snapshot exceeds 100,000 aggregate features".into());
        }
        if !self.include_design_customers
            && !self
                .sources
                .iter()
                .any(|s| matches!(s.role, SourceRole::Customers | SourceRole::Buildings))
        {
            return Err(
                "table-only customer mode requires an explicit source (which may be empty)".into(),
            );
        }
        if serde_json::to_vec(self).map_err(|e| e.to_string())?.len() > MAX_BYTES {
            return Err("snapshot exceeds 64 MiB".into());
        }
        Ok(())
    }
}

pub fn apply_edit(snapshot: &Snapshot, edit: &Edit) -> Result<Snapshot, String> {
    if edit.schema != "ds.design.edit/v1" || edit.transformer != snapshot.transformer {
        return Err("edit schema or transformer mismatch".into());
    }
    identifier(&edit.operation_id)?;
    if digest(snapshot)? != edit.expected_revision {
        return Err("revision conflict".into());
    }
    if edit.mutations.is_empty() || edit.mutations.len() > MAX_EDITS {
        return Err("expected 1..5000 mutations".into());
    }
    let mut next = snapshot.clone();
    let mut affected = 0;
    let mut indexes: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for (mutation_index, mutation) in edit.mutations.iter().enumerate() {
        let layer = match mutation {
            Mutation::Add { layer, .. }
            | Mutation::SetProperties { layer, .. }
            | Mutation::ReplaceGeometry { layer, .. }
            | Mutation::Delete { layer, .. } => layer,
        };
        let geometry_type = authored_geometry(layer)?;
        if matches!(mutation, Mutation::Add { .. }) {
            next.layers
                .entry(layer.clone())
                .or_insert_with(|| json!({"type":"FeatureCollection","features":[]}));
        }
        let rows = features_mut(
            next.layers
                .get_mut(layer)
                .ok_or("edit layer does not exist")?,
        )?;
        let index = indexes.entry(layer.clone()).or_insert_with(|| {
            rows.iter()
                .enumerate()
                .filter_map(|(i, f)| f["id"].as_str().map(|id| (id.to_owned(), i)))
                .collect()
        });
        match mutation {
            Mutation::Add { features, .. } => {
                affected += features.len();
                for (source_index, feature) in features.iter().enumerate() {
                    let mut feature = feature.clone();
                    if feature["geometry"]["type"] != geometry_type
                        || !feature["properties"].is_object()
                    {
                        return Err("new feature geometry or properties mismatch".into());
                    }
                    feature["id"] = json!(format!(
                        "f-{}",
                        digest(&json!([edit.operation_id, mutation_index, source_index]))?
                    ));
                    feature["properties"]["drafting_status"] = json!("draft");
                    let id = feature["id"]
                        .as_str()
                        .ok_or("new identity missing")?
                        .to_owned();
                    if index.insert(id, rows.len()).is_some() {
                        return Err("duplicate new feature identity".into());
                    }
                    rows.push(feature);
                }
            }
            Mutation::ReplaceGeometry { id, geometry, .. } => {
                affected += 1;
                if geometry["type"] != geometry_type {
                    return Err("replacement geometry type mismatch".into());
                }
                let feature = &mut rows[*index.get(id).ok_or("feature does not exist")?];
                feature["geometry"] = geometry.clone();
                feature["properties"]["drafting_status"] = json!("draft");
            }
            Mutation::SetProperties { ids, values, .. } => {
                if values.contains_key("transfo")
                    || values.contains_key("id")
                    || values.contains_key("doc_id")
                {
                    return Err("identity properties cannot be edited".into());
                }
                affected += ids.len();
                for id in unique_ids(ids)? {
                    let feature = &mut rows[*index.get(id).ok_or("feature does not exist")?];
                    feature["properties"]
                        .as_object_mut()
                        .ok_or("invalid properties")?
                        .extend(values.clone());
                }
            }
            Mutation::Delete { ids, .. } => {
                affected += ids.len();
                let ids = unique_ids(ids)?;
                for id in ids {
                    let offset = index.remove(id).ok_or("feature does not exist")?;
                    rows[offset] = Value::Null;
                }
            }
        }
        if affected > MAX_EDITS {
            return Err("edit exceeds 5000 affected features".into());
        }
    }
    for collection in next.layers.values_mut() {
        features_mut(collection)?.retain(|f| !f.is_null());
    }
    next.normalize()?;
    Ok(next)
}

fn unique_ids(ids: &[String]) -> Result<BTreeSet<&str>, String> {
    let set: BTreeSet<_> = ids.iter().map(String::as_str).collect();
    if set.len() != ids.len() || set.is_empty() {
        return Err("ids must be nonempty and unique".into());
    }
    Ok(set)
}

pub fn edit_json(snapshot: &[u8], request: &[u8]) -> Result<String, String> {
    if request.len() > MAX_BYTES {
        return Err("edit exceeds 64 MiB".into());
    }
    let snapshot = decode(snapshot)?;
    let edit = serde_json::from_slice(request).map_err(|e| e.to_string())?;
    serde_json::to_string(&apply_edit(&snapshot, &edit)?).map_err(|e| e.to_string())
}

#[cfg(test)]
mod process_plan_tests {
    use super::*;
    #[test]
    fn overrides_preserve_unmentioned_settings_and_false_values() {
        let input = json!({"targets":["tx_a"],"workers":2,"resident_limit":2,
            "settings":{"build_network":true,"assign_assemblies":true,"weld_tolerance":0.1},
            "overrides":{"build_network":false}});
        let plan = process_plan(input.to_string().as_bytes()).unwrap();
        assert_eq!(
            plan["settings"],
            json!({"build_network":false,"assign_assemblies":true,"weld_tolerance":0.1})
        );
        assert_eq!(plan["config_source"], "cached_project");
        let mut invalid = input;
        invalid["overrides"]["build_network"] = json!("false");
        assert!(process_plan(invalid.to_string().as_bytes()).is_err());
    }
    #[test]
    fn local_plan_caps_workers_and_preserves_order_without_cloud_completion() {
        let plan =
            process_plan(br#"{"targets":["TX_B","tx_a","tx_b"],"workers":32,"resident_limit":2}"#)
                .unwrap();
        assert_eq!(plan["names"], json!(["tx_b", "tx_a"]));
        assert_eq!(plan["concurrency"], 2);
        assert_eq!(plan["completion"], "local_commit");
        assert!(process_plan(
            br#"{"targets":["combined_transformer"],"workers":2,"resident_limit":2}"#
        )
        .is_err());
        assert!(process_plan(br#"{"targets":["tx_a"],"workers":0,"resident_limit":2}"#).is_err());
    }
}

// ── one row-identity vocabulary ─────────────────────────────────────────
//
// Three overlapping name sets decided what a status row IS: the browser's
// `SPECIAL_ROW_NAMES`, its `COMBINED_AGGREGATE_NAMES` (the same set minus
// `mv_data`), its `UNVERSIONABLE_NAMES` (a third membership that adds
// `__all__` and drops `mv_data`/`compounded_report`), plus two Rust copies —
// `project_control::is_special_status_row` and
// `report::is_reserved_report_identity`. This is the one home all of them
// became. The asymmetries are not accidents and are pinned by name below.

/// What a status row is, when it is not a transformer.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RowKind {
    /// A real LV transformer document.
    Individual,
    /// A synthetic selection over transformers — it has no document of its
    /// own and can never be an input to the flow that produces it.
    Aggregate,
    /// A document that belongs to the project rather than to one transformer:
    /// the MV network (`mv_data`) and the compounded report.
    ProjectDocument,
    /// Computed analysis output, full-replaced by its producer on every run.
    Analysis,
}

/// What a row may be a target of. Per-kind, with the two documented
/// exceptions the browser's three sets encoded by accident of membership.
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq)]
pub struct RowAllows {
    /// May be an input to "add to combined" / "combined + export".
    pub combinable: bool,
    /// May carry server-assigned vN versions.
    pub versionable: bool,
    /// May be a requested transformer of a report lane.
    pub reportable: bool,
    /// Has a document of its own that a delete would remove.
    pub deletable: bool,
}

/// The browser's `canonicalName` (`api/design.ts`) and the canonicalisation
/// `project_control`/`report` already applied, which are the same rule: trim,
/// lowercase, and every other byte becomes `_`.
pub fn canonical_row_name(name: &str) -> String {
    name.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_lowercase() || character.is_ascii_digit() || character == '_' {
                character
            } else {
                '_'
            }
        })
        .collect()
}

/// Classify a row by its canonical name alone — the only input this has.
pub fn row_kind(name: &str) -> RowKind {
    match canonical_row_name(name).as_str() {
        // `combined_transformers` is the plural spelling the report lane
        // already treated as the combined set; `__all__` is the whole-project
        // selection token. Both are selections, never documents.
        "combined_transformer" | "combined_transformers" | "all_transformers" | "__all__" => {
            RowKind::Aggregate
        }
        "mv_data" | "compounded_report" => RowKind::ProjectDocument,
        "collisions" => RowKind::Analysis,
        _ => RowKind::Individual,
    }
}

/// Not a transformer: the register pins it, gives it no sync badge, and keeps
/// it out of every per-fleet option list.
pub fn is_special_row(name: &str) -> bool {
    row_kind(name) != RowKind::Individual
}

pub fn row_allows(name: &str) -> RowAllows {
    let canonical = canonical_row_name(name);
    if canonical.is_empty() {
        // A nameless row is a target of nothing. `isVersionableTransformer`
        // said so first and it is true of all four.
        return RowAllows {
            combinable: false,
            versionable: false,
            reportable: false,
            deletable: false,
        };
    }
    match row_kind(&canonical) {
        RowKind::Individual => RowAllows {
            combinable: true,
            versionable: true,
            reportable: true,
            deletable: true,
        },
        // The two pinned asymmetries. `mv_data` is a PEER in the combined
        // flow, not the aggregate it feeds, so it may be combined — and it is
        // a document, so it versions. `compounded_report` is the project's
        // compounded output: never a combined input, but it was never in the
        // unversionable set either, and the report lane does not reserve its
        // name. Neither is a transformer a delete would remove.
        RowKind::ProjectDocument => RowAllows {
            combinable: canonical == "mv_data",
            versionable: true,
            reportable: true,
            deletable: false,
        },
        RowKind::Aggregate | RowKind::Analysis => RowAllows {
            combinable: false,
            versionable: false,
            reportable: false,
            deletable: false,
        },
    }
}

#[cfg(test)]
mod row_identity_tests {
    use super::*;

    #[test]
    fn reproduces_every_membership_of_the_three_sets_it_replaces() {
        for name in [
            "compounded_report",
            "combined_transformer",
            "all_transformers",
            "mv_data",
            "collisions",
        ] {
            assert!(is_special_row(name), "{name} was a special status row");
        }
        assert!(!is_special_row("tx_1"));
        // Cased and punctuated spellings canonicalise in.
        assert!(is_special_row(" Combined-Transformer "));

        // COMBINED_AGGREGATE_NAMES — the special set minus mv_data.
        for name in [
            "compounded_report",
            "combined_transformer",
            "all_transformers",
            "collisions",
        ] {
            assert!(!row_allows(name).combinable, "{name} is never combinable");
        }
        assert!(row_allows("mv_data").combinable);
        assert!(row_allows("tx_1").combinable);

        // UNVERSIONABLE_NAMES.
        for name in [
            "collisions",
            "__all__",
            "all_transformers",
            "combined_transformer",
        ] {
            assert!(!row_allows(name).versionable, "{name} is unversionable");
        }
        assert!(row_allows("mv_data").versionable);
        assert!(row_allows("compounded_report").versionable);
        assert!(row_allows("tx_1").versionable);
        assert!(!row_allows("  ").versionable);
    }

    #[test]
    fn the_declared_divergences_are_the_only_ones() {
        // `__all__` is the project-wide selection token: no register row ever
        // carries it, so classifying it as the aggregate it is costs nothing.
        assert!(is_special_row("__all__"));
        // The plural spelling the report lane already reserved.
        assert!(is_special_row("combined_transformers"));
        // Nothing else moved.
        assert!(!is_special_row("tx_1"));
        assert!(!is_special_row("mv_data_2"));
    }
}
