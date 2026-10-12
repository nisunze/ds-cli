//! Immutable offline version rooms. Hosts persist the returned manifest, rooms
//! and outbox intent in one transaction; no command performs network IO.
use crate::reconciliation as sync;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

const SCHEMA: &str = "ds.design.version/v1";
const MAX_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub owner: String,
    pub audience: String,
    pub lane: String,
    pub project: String,
    pub object_kind: String,
    pub object_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub stable_object_id: Option<String>,
    pub crs: Option<String>,
    pub location: Option<[f64; 2]>,
    /// Explicit operator/project extent only, never a viewport-derived guess.
    pub declared_bounds: Option<[f64; 4]>,
    pub source_digest: Option<String>,
    pub settings_digest: Option<String>,
    pub result_input_digest: Option<String>,
    pub current_input_digest: Option<String>,
    pub replacement_of: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: String,
    pub id: String,
    pub scope: Scope,
    pub operation: String,
    pub parent: Option<String>,
    /// Local display order. Never a server-assigned vN identity.
    pub local_sequence: usize,
    pub created_at: String,
    pub reason: String,
    pub rooms: BTreeMap<String, String>,
    pub room_bytes: usize,
    pub evidence: Evidence,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub manifest: Manifest,
    pub rooms: BTreeMap<String, Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub publication: Option<sync::Intent>,
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Prepare {
        scope: Scope,
        operation: String,
        parent: Option<String>,
        local_sequence: usize,
        created_at: String,
        reason: String,
        snapshot: Value,
        project_config: Option<Value>,
        #[serde(default)]
        evidence: Box<Evidence>,
    },
    Read {
        scope: Scope,
        id: String,
        bundle: Box<Bundle>,
    },
    Compare {
        scope: Scope,
        left: Box<Bundle>,
        right: Box<Bundle>,
        tolerance_m: f64,
    },
    ReadComparison {
        scope: Scope,
        artifact: Value,
    },
    PrepareComparisonPublication {
        scope: Scope,
        artifact: Value,
    },
    GovernedCatalogPlan {
        project: String,
        transformer: String,
    },
    GovernedCatalogAccept {
        project: String,
        transformer: String,
        response: Value,
    },
    PinnedRead {
        project: String,
        transformer: String,
        version_id: String,
        revision_id: String,
        response: Value,
    },
    ValidateTarget {
        id: String,
    },
    Commit {
        scope: Scope,
        manifest: Box<Manifest>,
        catalog: Vec<Manifest>,
        publication: Box<sync::Intent>,
        queue: Option<Box<sync::Queue>>,
        now_ms: u64,
    },
    Catalog {
        scope: Scope,
        catalog: Vec<Manifest>,
    },
}

fn digest(value: &impl Serialize) -> Result<String, String> {
    // Value normalizes all object-key ordering, including typed structs.
    let mut value = serde_json::to_value(value).map_err(|e| e.to_string())?;
    normalize_json_numbers(&mut value);
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&value).map_err(|e| e.to_string())?)
    ))
}
// JSON hosts represent 30.0 and 30 identically. Hash the same mathematical
// JSON number across Rust/JS round-trips, including negative zero.
fn normalize_json_numbers(value: &mut Value) {
    match value {
        Value::Number(n) if n.is_f64() => {
            if let Some(f) = n.as_f64() {
                if f.fract() == 0.0 && f.abs() <= 9_007_199_254_740_991.0 {
                    *value = json!(f as i64);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(normalize_json_numbers),
        Value::Object(map) => map.values_mut().for_each(normalize_json_numbers),
        _ => {}
    }
}
fn exact(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
fn validate_scope(scope: &Scope) -> Result<(), String> {
    if [
        &scope.owner,
        &scope.audience,
        &scope.lane,
        &scope.project,
        &scope.object_kind,
        &scope.object_id,
    ]
    .iter()
    .any(|s| !exact(s))
    {
        return Err(
            "version scope requires exact owner, audience, lane, project and object identity"
                .into(),
        );
    }
    Ok(())
}
pub fn valid_target(id: &str) -> bool {
    if let Some(hash) = id.strip_prefix("local-") {
        return hash.len() == 64
            && hash
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
    }
    id.strip_prefix('v')
        .is_some_and(|n| !n.starts_with('0') && n.parse::<u64>().is_ok_and(|n| n > 0))
}
fn manifest_id(manifest: &Manifest) -> Result<String, String> {
    let mut content = manifest.clone();
    content.id.clear();
    Ok(format!("local-{}", digest(&content)?))
}
pub fn reconciliation_scope(value: &Scope) -> sync::Scope {
    sync::Scope {
        principal: value.owner.clone(),
        audience: value.audience.clone(),
        lane: value.lane.clone(),
        project: value.project.clone(),
    }
}
fn content_reference(value: &impl Serialize) -> Result<sync::Content, String> {
    let mut value = serde_json::to_value(value).map_err(|e| e.to_string())?;
    normalize_json_numbers(&mut value);
    Ok(sync::Content {
        sha256: digest(&value)?,
        size: serde_json::to_vec(&value).map_err(|e| e.to_string())?.len() as u64,
    })
}
fn publication(
    manifest: &Manifest,
    rooms: &BTreeMap<String, Value>,
) -> Result<sync::Intent, String> {
    let artifacts = rooms
        .iter()
        .map(|(hash, room)| Ok((hash.clone(), content_reference(room)?)))
        .collect::<Result<_, String>>()?;
    Ok(sync::Intent {
        operation_id: format!("version:{}", manifest.operation),
        domain: sync::Domain::Design,
        object_id: manifest.scope.object_id.clone(),
        command: sync::Operation::DesignVersion,
        base: sync::BaseRevision::Immutable,
        input_fingerprint: manifest.id.trim_start_matches("local-").into(),
        payload: content_reference(manifest)?,
        artifacts,
        dependencies: Default::default(),
        // This identifies local capture semantics, not a grant to upload.
        policy_revision: "local-version-capture/v1".into(),
        tombstone: false,
    })
}
fn validate_publication(manifest: &Manifest, intent: &sync::Intent) -> Result<(), String> {
    let hashes = manifest
        .rooms
        .values()
        .collect::<std::collections::BTreeSet<_>>();
    if intent.operation_id != format!("version:{}", manifest.operation)
        || intent.command != sync::Operation::DesignVersion
        || intent.object_id != manifest.scope.object_id
        || intent.base != sync::BaseRevision::Immutable
        || !intent.dependencies.is_empty()
        || intent.policy_revision != "local-version-capture/v1"
        || intent.input_fingerprint != manifest.id.trim_start_matches("local-")
        || intent.payload != content_reference(manifest)?
        || intent.artifacts.len() != hashes.len()
        || intent
            .artifacts
            .iter()
            .any(|(name, content)| !hashes.contains(name) || &content.sha256 != name)
        || intent
            .artifacts
            .values()
            .try_fold(0u64, |total, c| total.checked_add(c.size))
            != Some(manifest.room_bytes as u64)
    {
        return Err("version publication does not pin its exact manifest and rooms".into());
    }
    Ok(())
}
fn layers(snapshot: &Value) -> Result<BTreeMap<String, Value>, String> {
    if snapshot["schema"] != "ds.transformer-version-snapshot/v1" {
        return Err("unsupported version snapshot schema".into());
    }
    let layers = snapshot["layers"]
        .as_object()
        .ok_or("snapshot layers are missing")?;
    if layers.is_empty() || layers.len() > 256 {
        return Err("snapshot requires 1..256 layer rooms".into());
    }
    for (name, layer) in layers {
        if !exact(name) || layer["type"] != "FeatureCollection" || !layer["features"].is_array() {
            return Err("snapshot has an invalid layer room".into());
        }
    }
    if snapshot.get("metadata").is_some_and(|v| !v.is_object()) {
        return Err("invalid snapshot metadata".into());
    }
    Ok(layers.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
}

pub fn read(scope: &Scope, id: &str, bundle: &Bundle) -> Result<Value, String> {
    validate_scope(scope)?;
    let m = &bundle.manifest;
    if &m.scope != scope || m.schema != SCHEMA || m.id != id || manifest_id(m)? != id {
        return Err("version identity, scope or manifest digest mismatch".into());
    }
    let mut layers = serde_json::Map::new();
    let mut metadata = json!({});
    let mut total = 0;
    let mut expected = std::collections::BTreeSet::new();
    for (name, hash) in &m.rooms {
        let room = bundle
            .rooms
            .get(hash)
            .ok_or("version room is not available locally")?;
        if digest(room)? != *hash {
            return Err("version room digest mismatch".into());
        }
        if expected.insert(hash) {
            total += serde_json::to_vec(room).map_err(|e| e.to_string())?.len();
        }
        if let Some(layer) = name.strip_prefix("layer:") {
            layers.insert(layer.into(), room.clone());
        } else if name == "metadata" {
            metadata = room.clone();
        } else if name != "project_config" {
            return Err("unknown version room role".into());
        }
    }
    if expected.len() != bundle.rooms.len() || total != m.room_bytes {
        return Err("version room inventory mismatch".into());
    }
    metadata["version_evidence"] = serde_json::to_value(&m.evidence).map_err(|e| e.to_string())?;
    let snapshot = json!({"schema":"ds.transformer-version-snapshot/v1", "layers":layers, "metadata":metadata});
    self::layers(&snapshot)?;
    Ok(snapshot)
}

fn read_comparison(scope: &Scope, mut artifact: Value) -> Result<Value, String> {
    validate_scope(scope)?;
    let id = artifact
        .get("id")
        .and_then(Value::as_str)
        .ok_or("comparison identity missing")?
        .to_owned();
    artifact
        .as_object_mut()
        .ok_or("invalid comparison room")?
        .remove("id");
    if artifact["schema"] != "ds.design.version-comparison/v1"
        || artifact["scope"] != serde_json::to_value(scope).map_err(|e| e.to_string())?
        || format!("comparison-{}", digest(&artifact)?) != id
    {
        return Err("comparison room identity or digest mismatch".into());
    }
    artifact["id"] = json!(id);
    Ok(artifact)
}
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.is_empty() || input.len() > MAX_BYTES {
        return Err("version command exceeds 64 MiB bound".into());
    }
    let cmd: Command = serde_json::from_slice(input).map_err(|e| e.to_string())?;
    let output = match cmd {
        Command::Catalog { scope, catalog } => {
            validate_catalog(&scope, &catalog)?;
            json!(catalog.iter().rev().map(record).collect::<Vec<_>>())
        }
        Command::Commit {
            scope,
            manifest,
            mut catalog,
            publication,
            queue,
            now_ms,
        } => {
            let manifest = *manifest;
            let publication = *publication;
            let queue = queue.map(|q| *q);
            validate_catalog(&scope, &catalog)?;
            if manifest.scope != scope || manifest_id(&manifest)? != manifest.id {
                return Err("invalid commit manifest".into());
            }
            if catalog
                .iter()
                .any(|old| old.operation == manifest.operation && old.id != manifest.id)
            {
                return Err("operation identity was reused with changed version content".into());
            }
            validate_publication(&manifest, &publication)?;
            let reconciled = sync::apply(sync::Request {
                schema: sync::SCHEMA.into(),
                scope: reconciliation_scope(&scope),
                expected_revision: queue.as_ref().map_or(0, |q| q.revision),
                now_ms,
                queue,
                command: sync::Action::Enqueue {
                    intent: publication,
                },
            })?;
            if let Some(old) = catalog.iter().find(|m| m.operation == manifest.operation) {
                if old.id != manifest.id {
                    return Err("operation identity was reused with changed version content".into());
                }
                json!({"catalog":catalog,"record":record(&manifest),"replayed":true,"queue":reconciled.queue})
            } else {
                if catalog.last().map(|m| &m.id) != manifest.parent.as_ref()
                    || manifest.local_sequence != catalog.len() + 1
                {
                    return Err("local version head moved; capture again".into());
                }
                if catalog.len() >= 10000 {
                    return Err(
                        "local version catalog reached its explicit 10000-version bound".into(),
                    );
                }
                catalog.push(manifest.clone());
                json!({"catalog":catalog,"record":record(&manifest),"replayed":false,"queue":reconciled.queue})
            }
        }
        Command::ReadComparison { scope, artifact } => read_comparison(&scope, artifact)?,
        Command::PrepareComparisonPublication { scope, artifact } => {
            let artifact = read_comparison(&scope, artifact)?;
            let id = artifact["id"]
                .as_str()
                .ok_or("comparison identity missing")?;
            let publication = sync::Intent {
                operation_id: id.into(),
                domain: sync::Domain::Design,
                object_id: scope.object_id.clone(),
                command: sync::Operation::DesignComparison,
                base: sync::BaseRevision::Immutable,
                input_fingerprint: id.trim_start_matches("comparison-").into(),
                payload: content_reference(&artifact)?,
                artifacts: Default::default(),
                dependencies: Default::default(),
                policy_revision: "local-comparison-capture/v1".into(),
                tombstone: false,
            };
            json!({"intent":publication})
        }
        Command::GovernedCatalogPlan {
            project,
            transformer,
        } => crate::design_version_catalog::Command::Catalog {
            object_id: transformer,
        }
        .payload(&project)?,
        Command::GovernedCatalogAccept {
            project,
            transformer,
            response,
        } => {
            let command = crate::design_version_catalog::Command::Catalog {
                object_id: transformer,
            };
            command.validate(&project)?;
            command.verify_response(&project, &response)?;
            response["catalog"].clone()
        }
        Command::PinnedRead {
            project,
            transformer,
            version_id,
            revision_id,
            response,
        } => {
            let command = crate::design_version_catalog::Command::Revision {
                object_id: transformer.clone(),
                version_id: version_id.clone(),
                revision_id: revision_id.clone(),
            };
            command.validate(&project)?;
            command.verify_response(&project, &response)?;
            let snapshot = crate::design_version_catalog::decode_pinned_snapshot(&response)?;
            layers(&snapshot)?;
            let m = &response["metadata"];
            if !matches!(
                m["snapshot_scope"].as_str(),
                Some("runtime_design" | "exported_design")
            ) {
                return Err("pinned snapshot scope unavailable".into());
            }
            let display = response["display_ordinal"]
                .as_u64()
                .filter(|n| *n <= 1_000_000)
                .ok_or("pinned catalog display ordinal absent")?;
            json!({"version":{"project_id":project,"transformer_id":transformer,
                "version_id":version_id,"revision_id":revision_id,"version":display,
                "source_revision":m["source_revision"],"reason":m["reason"],"created_at":m["created_at"],
                "created_by":m["created_by"],"playback_available":true,
                "snapshot_scope":m["snapshot_scope"],"source_mode":m["source_mode"]},"snapshot":snapshot})
        }
        Command::ValidateTarget { id } => json!({"valid":valid_target(&id)}),
        Command::Prepare {
            scope,
            operation,
            parent,
            local_sequence,
            created_at,
            reason,
            snapshot,
            project_config,
            mut evidence,
        } => {
            validate_scope(&scope)?;
            if !exact(&operation)
                || !exact(&created_at)
                || reason.trim().is_empty()
                || reason.len() > 1120
                || local_sequence == 0
            {
                return Err(
                    "version requires operation identity, creation time, reason and local sequence"
                        .into(),
                );
            }
            if parent.as_ref().is_some_and(|id| !valid_target(id)) {
                return Err("invalid parent version identity".into());
            }
            if evidence.settings_digest.is_none() {
                evidence.settings_digest = project_config.as_ref().map(digest).transpose()?;
            }
            if evidence.location.is_none() {
                if let Some(features) = snapshot["layers"]["tr"]["features"].as_array() {
                    if features.len() == 1 && features[0]["geometry"]["type"] == "Point" {
                        if let Some(c) = features[0]["geometry"]["coordinates"].as_array() {
                            if c.len() >= 2 {
                                evidence.location =
                                    c[0].as_f64().zip(c[1].as_f64()).map(|(x, y)| [x, y]);
                            }
                        }
                    }
                }
            }
            let mut references = BTreeMap::new();
            let mut rooms = BTreeMap::new();
            let mut add = |name: String, mut value: Value| -> Result<(), String> {
                normalize_json_numbers(&mut value);
                let hash = digest(&value)?;
                references.insert(name, hash.clone());
                rooms.insert(hash, value);
                Ok(())
            };
            for (name, layer) in layers(&snapshot)? {
                add(format!("layer:{name}"), layer)?;
            }
            add(
                "metadata".into(),
                snapshot.get("metadata").cloned().unwrap_or(json!({})),
            )?;
            if let Some(config) = project_config {
                add("project_config".into(), config)?;
            }
            let room_bytes = rooms.values().try_fold(0usize, |n, v| {
                serde_json::to_vec(v)
                    .map(|v| n + v.len())
                    .map_err(|e| e.to_string())
            })?;
            let mut manifest = Manifest {
                schema: SCHEMA.into(),
                id: String::new(),
                scope,
                operation,
                parent,
                local_sequence,
                created_at,
                reason,
                rooms: references,
                room_bytes,
                evidence: *evidence,
            };
            manifest.id = manifest_id(&manifest)?;
            let publication = Some(publication(&manifest, &rooms)?);
            serde_json::to_value(Bundle {
                manifest,
                rooms,
                publication,
            })
            .map_err(|e| e.to_string())?
        }
        Command::Read { scope, id, bundle } => read(&scope, &id, &bundle)?,
        Command::Compare {
            scope,
            left,
            right,
            tolerance_m,
        } => {
            if !tolerance_m.is_finite() || tolerance_m < 0.0 {
                return Err("comparison tolerance must be finite nonnegative metres".into());
            }
            let l = read(&scope, &left.manifest.id, &left)?;
            let r = read(&scope, &right.manifest.id, &right)?;
            let comparison = ds_geo_lite::comparison::compare_geojson_layers(
                ds_geo_lite::comparison::GeoJsonLayerComparisonInput {
                    local_layers: layers(&l)?,
                    cloud_layers: layers(&r)?,
                    layer_names: vec![],
                    ignored_property_keys: vec!["v_first".into(), "v_last".into()],
                    max_detail_features: 500,
                    detail_change_classes: vec![],
                },
            )?;
            let metadata_layer = |snapshot: &Value| {
                BTreeMap::from([(
                    "computation_metadata".into(),
                    json!({"type":"FeatureCollection","features":[{
                        "type":"Feature","id":"metadata","geometry":null,"properties":snapshot["metadata"]
                    }]}),
                )])
            };
            let metadata_comparison = ds_geo_lite::comparison::compare_geojson_layers(
                ds_geo_lite::comparison::GeoJsonLayerComparisonInput {
                    local_layers: metadata_layer(&l),
                    cloud_layers: metadata_layer(&r),
                    layer_names: vec![],
                    ignored_property_keys: vec![
                        "version_evidence".into(),
                        "captured_from".into(),
                        "source_server_version".into(),
                    ],
                    max_detail_features: 1,
                    detail_change_classes: vec![],
                },
            )?;
            let mut artifact = json!({"schema":"ds.design.version-comparison/v1","scope":scope,
                "left_revision":left.manifest.id,"right_revision":right.manifest.id,
                "comparison": comparison, "metadata_comparison":metadata_comparison, "consistency": consistency(&left.manifest.evidence, &right.manifest.evidence, tolerance_m)});
            artifact["id"] = json!(format!("comparison-{}", digest(&artifact)?));
            artifact
        }
    };
    serde_json::to_string(&output).map_err(|e| e.to_string())
}

fn consistency(left: &Evidence, right: &Evidence, tolerance_m: f64) -> Value {
    let mut findings = Vec::new();
    let mut add = |code: &str, status: &str, detail: Value| {
        findings.push(json!({"code":code,"status":status,"detail":detail}))
    };
    match (&left.stable_object_id, &right.stable_object_id) {
        (Some(a), Some(b)) => add(
            "object_identity",
            if a == b { "consistent" } else { "inconsistent" },
            json!({"from":a,"to":b}),
        ),
        _ => add(
            "object_identity",
            "unknown",
            json!("Stable identity was not captured; equal names do not establish continuity."),
        ),
    }
    let geographic =
        left.crs.as_deref() == Some("EPSG:4326") && right.crs.as_deref() == Some("EPSG:4326");
    let valid = |p: [f64; 2]| {
        p[0].is_finite()
            && p[1].is_finite()
            && (-180.0..=180.0).contains(&p[0])
            && (-90.0..=90.0).contains(&p[1])
    };
    match (left.location, right.location) {
        (Some(a), Some(b)) if geographic && valid(a) && valid(b) => {
            let distance = ds_geo_lite::haversine_m(a[0], a[1], b[0], b[1]);
            add(
                "relocation",
                if distance > tolerance_m {
                    "inconsistent"
                } else {
                    "consistent"
                },
                json!({"distance_m":distance,"tolerance_m":tolerance_m}),
            );
            match left.declared_bounds {
                Some([west, south, east, north])
                    if valid([west, south])
                        && valid([east, north])
                        && west <= east
                        && south <= north =>
                {
                    let inside = b[0] >= west && b[0] <= east && b[1] >= south && b[1] <= north;
                    add(
                        "outside_original_bounds",
                        if inside { "consistent" } else { "inconsistent" },
                        json!({"bounds":[west,south,east,north]}),
                    );
                }
                _ => add("outside_original_bounds", "unknown", Value::Null),
            }
        }
        _ => {
            add(
                "relocation",
                "unknown",
                json!("Comparable locations and explicit EPSG:4326 are required."),
            );
            add("outside_original_bounds", "unknown", Value::Null);
        }
    }
    for (code, a, b) in [
        ("source_inputs", &left.source_digest, &right.source_digest),
        (
            "project_settings",
            &left.settings_digest,
            &right.settings_digest,
        ),
        (
            "result_freshness",
            &right.current_input_digest,
            &right.result_input_digest,
        ),
    ] {
        add(
            code,
            match (a, b) {
                (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => {
                    if a == b {
                        "consistent"
                    } else {
                        "inconsistent"
                    }
                }
                _ => "unknown",
            },
            Value::Null,
        );
    }
    json!({"tolerance_m":tolerance_m,"findings":findings})
}

fn validate_catalog(scope: &Scope, catalog: &[Manifest]) -> Result<(), String> {
    validate_scope(scope)?;
    if catalog.len() > 10000
        || catalog
            .iter()
            .any(|m| &m.scope != scope || m.schema != SCHEMA)
    {
        return Err("invalid local version catalog scope or bound".into());
    }
    Ok(())
}
fn record(m: &Manifest) -> Value {
    json!({"project_id":m.scope.project,"transformer_id":m.scope.object_id,"version_id":m.id,
        "version":m.local_sequence,"source_revision":m.parent,"reason":m.reason,"created_at":m.created_at,
        "created_by":m.scope.owner,"playback_available":true,"storage":"local","sync_state":"pending_transport",
        "room_count":m.rooms.len(),"room_bytes":m.room_bytes})
}

#[cfg(test)]
mod tests {
    #[test]
    fn pinned_read_requires_exact_immutable_identity_and_snapshot_bytes() {
        use serde_json::json;
        let snapshot=json!({"schema":"ds.transformer-version-snapshot/v1","layers":{"tr":{"type":"FeatureCollection","features":[]}}}).to_string();
        let mut request = json!({"action":"pinned_read","project":"p","transformer":"t","version_id":"v0","revision_id":"v0",
            "response":{"display_ordinal":0,"metadata":{"project_id":"p","transformer_id":"t","version_id":"v0","revision_id":"v0",
                "revision_number":1,"content_digest":"a".repeat(64),"snapshot_scope":"exported_design","source_mode":"source_pinned_import"},
                "snapshot_json":snapshot,"snapshot_sha256":format!("{:x}",sha2::Sha256::digest(snapshot.as_bytes()))}});
        let out: serde_json::Value =
            serde_json::from_str(&super::evaluate(&serde_json::to_vec(&request).unwrap()).unwrap())
                .unwrap();
        assert_eq!(out["version"]["version"], 0);
        assert_eq!(out["version"]["snapshot_scope"], "exported_design");
        assert!(out["version"].get("restore_allowed").is_none());
        request["response"]["snapshot_json"] = json!("{}");
        assert!(super::evaluate(&serde_json::to_vec(&request).unwrap())
            .unwrap_err()
            .contains("digest"));
        request["response"]["metadata"]["revision_id"] = json!("v1");
        assert!(super::evaluate(&serde_json::to_vec(&request).unwrap()).is_err());
    }

    use super::*;
    fn scope() -> Scope {
        Scope {
            owner: "owner".into(),
            audience: "aud".into(),
            lane: "local".into(),
            project: "p".into(),
            object_kind: "lv_transformer".into(),
            object_id: "same-name".into(),
        }
    }
    fn call(v: Value) -> Result<Value, String> {
        serde_json::from_str(&evaluate(&serde_json::to_vec(&v).unwrap())?)
            .map_err(|e| e.to_string())
    }
    fn prepare(operation: &str) -> Bundle {
        serde_json::from_value(call(json!({"action":"prepare","scope":scope(),"operation":operation,"parent":null,"local_sequence":1,"created_at":"now","reason":"baseline",
            "snapshot":{"schema":"ds.transformer-version-snapshot/v1","layers":{"tr":{"type":"FeatureCollection","features":[{"type":"Feature","id":"one","geometry":{"type":"Point","coordinates":[30,-2]},"properties":{"id":"one"}}]}}},
            "project_config":{"actual":"prepared settings"}})).unwrap()).unwrap()
    }
    #[test]
    fn version_creation_requires_reason_and_exact_scope_without_a_server_revision() {
        let prepared = prepare("offline-capture");
        let snapshot = read(&scope(), &prepared.manifest.id, &prepared).unwrap();
        let request = json!({"action":"prepare","scope":scope(),"operation":"offline-capture",
            "parent":null,"local_sequence":1,"created_at":"now","reason":"Before site visit",
            "snapshot":snapshot,"project_config":null});
        let bundle: Bundle = serde_json::from_value(call(request.clone()).unwrap()).unwrap();
        assert_eq!(bundle.manifest.scope, scope());
        assert_eq!(bundle.manifest.reason, "Before site visit");
        assert_eq!(bundle.manifest.parent, None);
        let committed = call(
            json!({"action":"commit","scope":scope(),"manifest":bundle.manifest,
            "publication":bundle.publication,"queue":null,"now_ms":1000,"catalog":[]}),
        )
        .unwrap();
        assert_eq!(committed["record"]["transformer_id"], "same-name");
        assert_eq!(committed["record"]["reason"], "Before site visit");
        assert_eq!(committed["record"]["sync_state"], "pending_transport");
        for reason in ["", " \t\n"] {
            let mut invalid = request.clone();
            invalid["reason"] = json!(reason);
            assert!(call(invalid).unwrap_err().contains("reason"));
        }
        for field in [
            "owner",
            "audience",
            "lane",
            "project",
            "object_kind",
            "object_id",
        ] {
            for value in ["", " padded "] {
                let mut invalid = request.clone();
                invalid["scope"][field] = json!(value);
                assert!(call(invalid).unwrap_err().contains("exact"), "{field}");
            }
        }
        let mut legacy = request;
        legacy["version"] = json!({"reason":"legacy cloud payload"});
        assert!(call(legacy).unwrap_err().contains("unknown field"));
    }
    #[test]
    fn commit_replay_and_parent_fence_are_exact() {
        let a = prepare("operation");
        let first =
            call(json!({"action":"commit","scope":scope(),"manifest":a.manifest,"publication":a.publication,"queue":null,"now_ms":1000,"catalog":[]}))
                .unwrap();
        assert_eq!(first["record"]["sync_state"], "pending_transport");
        let replay=call(json!({"action":"commit","scope":scope(),"manifest":a.manifest,"publication":a.publication,"queue":first["queue"],"now_ms":1001,"catalog":first["catalog"]})).unwrap();
        assert_eq!(replay["replayed"], true);
        let b = prepare("other");
        assert!(call(json!({"action":"commit","scope":scope(),"manifest":b.manifest,"publication":b.publication,"queue":first["queue"],"now_ms":1001,"catalog":first["catalog"]})).unwrap_err().contains("head moved"));
        let mut changed = a.manifest.clone();
        changed.reason = "changed".into();
        changed.id = manifest_id(&changed).unwrap();
        assert!(call(
            json!({"action":"commit","scope":scope(),"manifest":changed,"publication":a.publication,"queue":first["queue"],"now_ms":1001,"catalog":first["catalog"]})
        )
        .unwrap_err()
        .contains("reused"));
    }
    #[test]
    fn immutable_rooms_survive_json_numeric_roundtrips_and_reject_missing_or_foreign_data() {
        let a = prepare("a");
        assert!(a.manifest.rooms.contains_key("project_config"));
        let mut other = scope();
        other.owner = "another".into();
        assert!(read(&other, &a.manifest.id, &a).is_err());
        let mut missing = a.clone();
        missing.rooms.clear();
        assert!(read(&scope(), &a.manifest.id, &missing).is_err());
        assert_eq!(
            digest(&json!({"coordinates":[30.0,-2.0,-0.0]})).unwrap(),
            digest(&json!({"coordinates":[30,-2,0]})).unwrap()
        );
        let compared =
            call(json!({"action":"compare","scope":scope(),"left":a,"right":a,"tolerance_m":0}))
                .unwrap();
        assert_eq!(compared["comparison"]["totals"]["unchanged"], 1);
        assert!(
            call(json!({"action":"read_comparison","scope":scope(),"artifact":compared})).is_ok()
        );
        let mut corrupt = compared;
        corrupt["comparison"]["totals"]["unchanged"] = json!(999);
        assert!(
            call(json!({"action":"read_comparison","scope":scope(),"artifact":corrupt})).is_err()
        );
    }
    #[test]
    fn spatial_policy_uses_existing_metric_and_distinguishes_unknown_from_safe() {
        let a = Evidence {
            stable_object_id: Some("one".into()),
            crs: Some("EPSG:4326".into()),
            location: Some([30.0, -2.0]),
            declared_bounds: Some([29.9, -2.1, 30.1, -1.9]),
            ..Default::default()
        };
        let b = Evidence {
            stable_object_id: Some("two".into()),
            location: Some([31.0, -2.0]),
            current_input_digest: Some("new".into()),
            result_input_digest: Some("old".into()),
            ..a.clone()
        };
        let rows = consistency(&a, &b, 1.0);
        for code in [
            "object_identity",
            "relocation",
            "outside_original_bounds",
            "result_freshness",
        ] {
            assert!(rows["findings"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["code"] == code && r["status"] == "inconsistent"));
        }
        let missing = consistency(&Evidence::default(), &Evidence::default(), 0.0);
        assert!(missing["findings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["status"] == "unknown"));
        let tiny = Evidence {
            location: Some([30.0000001, -2.0]),
            ..a.clone()
        };
        assert_eq!(
            consistency(&a, &tiny, 1.0)["findings"][1]["status"],
            "consistent"
        );
    }
}
