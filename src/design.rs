//! Map-independent LV document and edit contracts. No storage, transport or compute.
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub const SCHEMA: &str = "ds.design.snapshot/v1";
pub const MAX_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_FEATURES: usize = 100_000;
pub const MAX_EDITS: usize = 5_000;

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
