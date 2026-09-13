//! Offline-first projection of MV and LV network facts into Solar city inputs.
//!
//! Hosts acquire bytes and publish the returned documents. This module alone
//! decides grouping, validates the fact boundary, and builds `network_data`.
//! The MV input carries route lengths only. Transformers, customers and LV
//! lengths come only from LV rooms; MV structure counts are not an input.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

pub const SOLAR_NETWORK_SEED_REQUEST_SCHEMA: &str = "ds-solar.network-seed-request/v1";
pub const SOLAR_NETWORK_SEED_PLAN_SCHEMA: &str = "ds-solar.network-seed-plan/v1";
pub const MV_ALIGNMENT_FACTS_SCHEMA: &str = "ds-grid.tagged-alignment-lengths/v1";
pub const MAX_SOLAR_NETWORK_SEED_BYTES: usize = 32 * 1024 * 1024;
const MAX_CITIES: usize = 200;
const MAX_TRANSFORMERS: usize = 2_000;
const MAX_ALIGNMENTS: usize = 20_000;

/// Discoverable closed request schema shared by native and browser hosts.
pub fn solar_network_seed_request_schema() -> Value {
    let room_schema = json!({
        "type": "object", "required": ["layers"],
        "description": "Wrap captured FastLV output.gdfs as room.layers. Only tr, customers and lv_lines are consumed; extra layers may be omitted.",
        "properties": {"layers": {
            "type": "object", "required": ["tr", "customers", "lv_lines"],
            "properties": {
                "tr": {"type": "object", "required": ["features"], "description": "One transformer feature; properties.tr_size (or kva) is positive integral kVA. A supplied transfo/tr_name must match the transformer identity ignoring case."},
                "customers": {"type": "object", "required": ["features"], "description": "Customer features, each with a nonempty properties.category. Every feature is one customer."},
                "lv_lines": {"type": "object", "required": ["features"], "description": "LV line features with positive properties.length or length_m in metres, or LineString/MultiLineString geometry in lv_geometry_crs."}
            }
        }}
    });
    json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "SolarNetworkSeedRequest",
        "description": "At most 32 MiB per request. For large projects, plan one city at a time with its exact tagged LV members and MV alignments; retain the original tag document and digest.",
        "type": "object",
        "additionalProperties": false,
        "required": ["schema", "project_id", "mv_city_definition_id", "mv_phase_definition_id", "lv_city_definition_id", "lv_phase_definition_id", "lv_geometry_crs", "mv", "lv_tag_projection", "lv_transformers", "cities"],
        "properties": {
            "schema": {"const": SOLAR_NETWORK_SEED_REQUEST_SCHEMA},
            "project_id": {"type": "string", "minLength": 1},
            "mv_city_definition_id": {"type": "string", "minLength": 1},
            "mv_phase_definition_id": {"type": "string", "minLength": 1},
            "lv_city_definition_id": {"type": "string", "minLength": 1},
            "lv_phase_definition_id": {"type": "string", "minLength": 1},
            "lv_geometry_crs": {"enum": ["geographic_wgs84", "projected_metric"]},
            "mv": {
                "type": "object", "additionalProperties": false,
                "required": ["schema", "model_id", "model_revision", "package_sha256", "alignments"],
                "properties": {
                    "schema": {"const": MV_ALIGNMENT_FACTS_SCHEMA},
                    "model_id": {"type": "string", "minLength": 1},
                    "model_revision": {"type": "string", "minLength": 1},
                    "package_sha256": {"type": "string", "pattern": "^sha256:[0-9a-f]{64}$"},
                    "alignments": {"type": "array", "maxItems": MAX_ALIGNMENTS, "items": {
                        "type": "object", "additionalProperties": false,
                        "required": ["alignment_id", "length_m", "tags"],
                        "properties": {
                            "alignment_id": {"type": "string", "minLength": 1},
                            "length_m": {"type": "number", "exclusiveMinimum": 0},
                            "tags": {"type": "object", "additionalProperties": {"type": "string", "minLength": 1}}
                        }
                    }}
                }
            },
            "lv_tag_projection": {
                "type": "object", "additionalProperties": false,
                "required": ["document", "sha256"],
                "properties": {
                    "document": {"type": "string", "minLength": 1},
                    "sha256": {"type": "string", "pattern": "^sha256:[0-9a-f]{64}$"}
                }
            },
            "lv_transformers": {"type": "array", "minItems": 1, "maxItems": MAX_TRANSFORMERS, "items": {
                "type": "object", "additionalProperties": false,
                "required": ["transformer", "room"],
                "properties": {
                    "transformer": {"type": "string", "minLength": 1},
                    "room": room_schema
                }
            }},
            "cities": {"type": "array", "minItems": 1, "maxItems": MAX_CITIES, "items": {
                "type": "object", "additionalProperties": false,
                "required": ["city_id", "display_name", "mv_city_values", "lv_city_values", "active_phases", "base_document"],
                "properties": {
                    "city_id": {"type": "string", "minLength": 1},
                    "display_name": {"type": "string"},
                    "mv_city_values": {"type": "array", "minItems": 1, "items": {"type": "string", "minLength": 1}, "uniqueItems": true},
                    "lv_city_values": {"type": "array", "minItems": 1, "items": {"type": "string", "minLength": 1}, "uniqueItems": true},
                    "active_phases": {"type": "array", "minItems": 1, "items": {"type": "string", "minLength": 1}, "uniqueItems": true},
                    "base_document": {"type": "object"}
                }
            }}
        }
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolarNetworkSeedError(String);

impl fmt::Display for SolarNetworkSeedError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for SolarNetworkSeedError {}

fn reject(code: &str, detail: impl fmt::Display) -> SolarNetworkSeedError {
    SolarNetworkSeedError(format!("{code}: {detail}"))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    schema: String,
    project_id: String,
    mv_city_definition_id: String,
    mv_phase_definition_id: String,
    lv_city_definition_id: String,
    lv_phase_definition_id: String,
    lv_geometry_crs: LvGeometryCrs,
    mv: MvFacts,
    lv_tag_projection: TagProjectionInput,
    lv_transformers: Vec<LvTransformerInput>,
    cities: Vec<CityBinding>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum LvGeometryCrs {
    GeographicWgs84,
    ProjectedMetric,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct TagProjectionInput {
    document: String,
    sha256: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MvFacts {
    schema: String,
    model_id: String,
    model_revision: String,
    package_sha256: String,
    alignments: Vec<MvAlignment>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct MvAlignment {
    alignment_id: String,
    length_m: f64,
    tags: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LvTransformerInput {
    transformer: String,
    room: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CityBinding {
    city_id: String,
    #[serde(default)]
    display_name: String,
    mv_city_values: Vec<String>,
    lv_city_values: Vec<String>,
    active_phases: Vec<String>,
    base_document: Value,
}

#[derive(Debug, Clone)]
struct LvFact {
    transformer: String,
    city: String,
    phase: String,
    kva: i64,
    lv_length_m: f64,
    customers: BTreeMap<String, i64>,
}

#[derive(Debug, Serialize)]
struct Plan {
    schema: &'static str,
    project_id: String,
    request_sha256: String,
    plan_digest: String,
    source: SourceReceipt,
    cities: Vec<CityPlan>,
    writes: Vec<LocalWrite>,
    excluded_facts: [&'static str; 1],
    mutated: bool,
}

#[derive(Debug, Serialize)]
struct SourceReceipt {
    mv_model_id: String,
    mv_model_revision: String,
    mv_package_sha256: String,
    lv_tag_projection_sha256: String,
    lv_transformer_count: usize,
    mv_alignment_count: usize,
}

#[derive(Debug, Serialize)]
struct CityPlan {
    city_id: String,
    display_name: String,
    active_phases: Vec<String>,
    transformer_count: usize,
    customer_count: i64,
    total_lv_length_m: f64,
    total_mv_length_km: f64,
    mv_length_by_phase_km: BTreeMap<String, f64>,
}

#[derive(Debug, Serialize)]
struct LocalWrite {
    city_id: String,
    document: Value,
    document_sha256: String,
}

#[derive(Debug, Deserialize)]
struct TagDocument {
    schema_version: String,
    project_id: String,
    groups: Vec<TagDefinition>,
    assignments: Vec<TagAssignment>,
}

#[derive(Debug, Deserialize)]
struct TagDefinition {
    definition_id: String,
    cardinality: String,
    #[serde(default = "active")]
    state: String,
    #[serde(default)]
    applies_to: Vec<String>,
}

fn active() -> String {
    "active".to_string()
}

#[derive(Debug, Deserialize)]
struct TagAssignment {
    object: TagObject,
    definition_id: String,
    values: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct TagObject {
    kind: String,
    id: String,
}

/// Validate one complete local fact set and return immutable Solar city writes.
/// No filesystem, browser, credential, network or clock is consulted.
pub fn plan_solar_network_seed(bytes: &[u8]) -> Result<String, SolarNetworkSeedError> {
    if bytes.is_empty() || bytes.len() > MAX_SOLAR_NETWORK_SEED_BYTES {
        return Err(reject(
            "solar_network_seed_request_bounded",
            "request must be within 1 byte..32 MiB",
        ));
    }
    let request: Request = serde_json::from_slice(bytes)
        .map_err(|error| reject("solar_network_seed_request_invalid", error))?;
    validate_request(&request)?;

    let tags = parse_lv_tags(&request)?;
    let lv = project_lv_facts(&request, &tags)?;
    let city_maps = city_maps(&request.cities)?;
    let mv_by_city = project_mv(&request, &city_maps.mv)?;
    let lv_by_city = group_lv(&lv, &city_maps.lv)?;

    let request_sha256 = sha256(bytes);
    let mut cities = Vec::with_capacity(request.cities.len());
    let mut writes = Vec::with_capacity(request.cities.len());
    for binding in &request.cities {
        let city_lv = lv_by_city.get(&binding.city_id).ok_or_else(|| {
            reject(
                "solar_network_seed_city_without_lv",
                format!("{} has no LV transformer facts", binding.city_id),
            )
        })?;
        let city_mv = mv_by_city.get(&binding.city_id).ok_or_else(|| {
            reject(
                "solar_network_seed_city_without_mv",
                format!("{} has no MV alignment facts", binding.city_id),
            )
        })?;
        let all_phases = city_lv
            .iter()
            .map(|fact| fact.phase.as_str())
            .collect::<BTreeSet<_>>();
        for phase in &binding.active_phases {
            if !all_phases.contains(phase.as_str()) {
                return Err(reject(
                    "solar_network_seed_active_phase_missing",
                    format!("{} selects absent LV phase {phase}", binding.city_id),
                ));
            }
        }

        let network_data = build_network_data(city_lv, city_mv, &binding.active_phases)?;
        let document = merge_network_data(&binding.city_id, &binding.base_document, network_data)?;
        let document_bytes = serde_json::to_vec(&document)
            .map_err(|error| reject("solar_network_seed_encode_failed", error))?;
        let customer_count = city_lv
            .iter()
            .flat_map(|fact| fact.customers.values())
            .try_fold(0_i64, |sum, value| sum.checked_add(*value))
            .ok_or_else(|| reject("solar_network_seed_customer_overflow", &binding.city_id))?;
        let total_lv_length_m = round3(city_lv.iter().map(|fact| fact.lv_length_m).sum());
        let total_mv_length_km = round3(city_mv.values().sum::<f64>() / 1_000.0);
        let mv_length_by_phase_km = city_mv
            .iter()
            .map(|(phase, metres)| (phase.clone(), round3(*metres / 1_000.0)))
            .collect();
        cities.push(CityPlan {
            city_id: binding.city_id.clone(),
            display_name: if binding.display_name.trim().is_empty() {
                binding.city_id.clone()
            } else {
                binding.display_name.clone()
            },
            active_phases: binding.active_phases.clone(),
            transformer_count: city_lv.len(),
            customer_count,
            total_lv_length_m,
            total_mv_length_km,
            mv_length_by_phase_km,
        });
        writes.push(LocalWrite {
            city_id: binding.city_id.clone(),
            document_sha256: sha256(&document_bytes),
            document,
        });
    }

    let source = SourceReceipt {
        mv_model_id: request.mv.model_id.clone(),
        mv_model_revision: request.mv.model_revision.clone(),
        mv_package_sha256: request.mv.package_sha256.clone(),
        lv_tag_projection_sha256: request.lv_tag_projection.sha256.clone(),
        lv_transformer_count: lv.len(),
        mv_alignment_count: request.mv.alignments.len(),
    };
    let digest_value = json!({
        "schema": SOLAR_NETWORK_SEED_PLAN_SCHEMA,
        "project_id": request.project_id,
        "request_sha256": request_sha256,
        "source": &source,
        "cities": &cities,
        "writes": &writes,
        "excluded_facts": ["mv_structure_counts"],
        "mutated": false,
    });
    let plan_digest = sha256(
        &serde_json::to_vec(&digest_value)
            .map_err(|error| reject("solar_network_seed_encode_failed", error))?,
    );
    serde_json::to_string(&Plan {
        schema: SOLAR_NETWORK_SEED_PLAN_SCHEMA,
        project_id: request.project_id,
        request_sha256,
        plan_digest,
        source,
        cities,
        writes,
        excluded_facts: ["mv_structure_counts"],
        mutated: false,
    })
    .map_err(|error| reject("solar_network_seed_encode_failed", error))
}

fn validate_request(request: &Request) -> Result<(), SolarNetworkSeedError> {
    if request.schema != SOLAR_NETWORK_SEED_REQUEST_SCHEMA {
        return Err(reject(
            "solar_network_seed_schema_unsupported",
            &request.schema,
        ));
    }
    for (field, value) in [
        ("project_id", request.project_id.as_str()),
        (
            "mv_city_definition_id",
            request.mv_city_definition_id.as_str(),
        ),
        (
            "mv_phase_definition_id",
            request.mv_phase_definition_id.as_str(),
        ),
        (
            "lv_city_definition_id",
            request.lv_city_definition_id.as_str(),
        ),
        (
            "lv_phase_definition_id",
            request.lv_phase_definition_id.as_str(),
        ),
        ("mv.model_id", request.mv.model_id.as_str()),
        ("mv.model_revision", request.mv.model_revision.as_str()),
        ("mv.package_sha256", request.mv.package_sha256.as_str()),
    ] {
        if value.trim().is_empty() {
            return Err(reject("solar_network_seed_field_blank", field));
        }
    }
    if request.mv.schema != MV_ALIGNMENT_FACTS_SCHEMA {
        return Err(reject(
            "solar_network_seed_mv_schema_unsupported",
            &request.mv.schema,
        ));
    }
    if request.mv_city_definition_id == request.mv_phase_definition_id
        || request.lv_city_definition_id == request.lv_phase_definition_id
    {
        return Err(reject(
            "solar_network_seed_definition_ids_invalid",
            "city and phase definition ids must be distinct within each source",
        ));
    }
    let package_digest = request
        .mv
        .package_sha256
        .strip_prefix("sha256:")
        .unwrap_or_default();
    if package_digest.len() != 64
        || !package_digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(reject(
            "solar_network_seed_mv_digest_invalid",
            "package_sha256 must be one lowercase prefixed SHA-256",
        ));
    }
    if sha256(request.lv_tag_projection.document.as_bytes()) != request.lv_tag_projection.sha256 {
        return Err(reject(
            "solar_network_seed_lv_tags_digest_mismatch",
            "the exact tag document bytes do not match their SHA-256",
        ));
    }
    if request.cities.is_empty() || request.cities.len() > MAX_CITIES {
        return Err(reject(
            "solar_network_seed_city_count_invalid",
            request.cities.len(),
        ));
    }
    if request.lv_transformers.is_empty() || request.lv_transformers.len() > MAX_TRANSFORMERS {
        return Err(reject(
            "solar_network_seed_transformer_count_invalid",
            request.lv_transformers.len(),
        ));
    }
    if request.mv.alignments.is_empty() || request.mv.alignments.len() > MAX_ALIGNMENTS {
        return Err(reject(
            "solar_network_seed_alignment_count_invalid",
            request.mv.alignments.len(),
        ));
    }
    Ok(())
}

struct CityMaps {
    mv: BTreeMap<String, String>,
    lv: BTreeMap<String, String>,
}

fn city_maps(bindings: &[CityBinding]) -> Result<CityMaps, SolarNetworkSeedError> {
    let mut city_ids = BTreeSet::new();
    let mut mv = BTreeMap::new();
    let mut lv = BTreeMap::new();
    for binding in bindings {
        if binding.city_id.trim().is_empty() || !city_ids.insert(binding.city_id.as_str()) {
            return Err(reject(
                "solar_network_seed_city_binding_invalid",
                &binding.city_id,
            ));
        }
        if binding.active_phases.is_empty()
            || binding
                .active_phases
                .iter()
                .any(|phase| phase.trim().is_empty())
            || binding.active_phases.iter().collect::<BTreeSet<_>>().len()
                != binding.active_phases.len()
        {
            return Err(reject(
                "solar_network_seed_active_phases_invalid",
                &binding.city_id,
            ));
        }
        insert_bindings(&mut mv, &binding.mv_city_values, &binding.city_id, "mv")?;
        insert_bindings(&mut lv, &binding.lv_city_values, &binding.city_id, "lv")?;
    }
    Ok(CityMaps { mv, lv })
}

fn insert_bindings(
    target: &mut BTreeMap<String, String>,
    values: &[String],
    city_id: &str,
    authority: &str,
) -> Result<(), SolarNetworkSeedError> {
    if values.is_empty() {
        return Err(reject(
            "solar_network_seed_city_values_missing",
            format!("{city_id} has no {authority} city value"),
        ));
    }
    for value in values {
        if value.trim().is_empty() || target.insert(value.clone(), city_id.to_string()).is_some() {
            return Err(reject(
                "solar_network_seed_city_value_ambiguous",
                format!("{authority} city value {value:?}"),
            ));
        }
    }
    Ok(())
}

fn parse_lv_tags(
    request: &Request,
) -> Result<BTreeMap<String, (String, String)>, SolarNetworkSeedError> {
    let document: TagDocument = serde_json::from_str(&request.lv_tag_projection.document)
        .map_err(|error| reject("solar_network_seed_lv_tags_invalid", error))?;
    if !matches!(
        document.schema_version.as_str(),
        "ds-report.design-tags/v2" | "ds-report.design-tags/v3"
    ) {
        return Err(reject(
            "solar_network_seed_lv_tags_schema_unsupported",
            document.schema_version,
        ));
    }
    if document.project_id != request.project_id {
        return Err(reject(
            "solar_network_seed_project_mismatch",
            format!(
                "tags={} request={}",
                document.project_id, request.project_id
            ),
        ));
    }
    for id in [
        request.lv_city_definition_id.as_str(),
        request.lv_phase_definition_id.as_str(),
    ] {
        let definition = document
            .groups
            .iter()
            .find(|definition| definition.definition_id == id)
            .ok_or_else(|| reject("solar_network_seed_lv_definition_missing", id))?;
        if definition.state != "active"
            || definition.cardinality != "single"
            || !definition
                .applies_to
                .iter()
                .any(|kind| kind == "lv_transformer")
        {
            return Err(reject("solar_network_seed_lv_definition_incompatible", id));
        }
    }
    let wanted = request
        .lv_transformers
        .iter()
        .map(|row| row.transformer.as_str())
        .collect::<BTreeSet<_>>();
    if wanted.len() != request.lv_transformers.len() {
        return Err(reject(
            "solar_network_seed_transformer_duplicate",
            "LV input repeats a transformer identity",
        ));
    }
    let mut values = BTreeMap::<(String, String), String>::new();
    for assignment in document.assignments {
        if assignment.object.kind != "lv_transformer"
            || !wanted.contains(assignment.object.id.as_str())
            || (assignment.definition_id != request.lv_city_definition_id
                && assignment.definition_id != request.lv_phase_definition_id)
        {
            continue;
        }
        let [value] = assignment.values.as_slice() else {
            return Err(reject(
                "solar_network_seed_lv_tag_not_scalar",
                format!("{}#{}", assignment.object.id, assignment.definition_id),
            ));
        };
        if value.trim().is_empty()
            || values
                .insert(
                    (
                        assignment.object.id.clone(),
                        assignment.definition_id.clone(),
                    ),
                    value.clone(),
                )
                .is_some()
        {
            return Err(reject(
                "solar_network_seed_lv_tag_duplicate",
                format!("{}#{}", assignment.object.id, assignment.definition_id),
            ));
        }
    }
    let mut tags = BTreeMap::new();
    for transformer in wanted {
        let city = values
            .remove(&(
                transformer.to_string(),
                request.lv_city_definition_id.clone(),
            ))
            .ok_or_else(|| reject("solar_network_seed_lv_city_missing", transformer))?;
        let phase = values
            .remove(&(
                transformer.to_string(),
                request.lv_phase_definition_id.clone(),
            ))
            .ok_or_else(|| reject("solar_network_seed_lv_phase_missing", transformer))?;
        tags.insert(transformer.to_string(), (city, phase));
    }
    Ok(tags)
}

fn project_lv_facts(
    request: &Request,
    tags: &BTreeMap<String, (String, String)>,
) -> Result<Vec<LvFact>, SolarNetworkSeedError> {
    request
        .lv_transformers
        .iter()
        .map(|input| {
            let (city, phase) = tags
                .get(&input.transformer)
                .ok_or_else(|| reject("solar_network_seed_lv_tags_missing", &input.transformer))?;
            let layers = input
                .room
                .get("layers")
                .and_then(Value::as_object)
                .ok_or_else(|| {
                    reject(
                        "solar_network_seed_lv_room_invalid",
                        format!("{}: expected room.layers object; wrap captured FastLV output.gdfs as room.layers", input.transformer),
                    )
                })?;
            let tr = features(layers, "tr", &input.transformer)?;
            if tr.len() != 1 {
                return Err(reject(
                    "solar_network_seed_transformer_marker_invalid",
                    format!("{} has {} tr features", input.transformer, tr.len()),
                ));
            }
            let props = properties(&tr[0], &input.transformer, "tr")?;
            for key in ["tr_name", "transfo"] {
                if let Some(marker) = props.get(key).and_then(Value::as_str) {
                    if !marker.trim().eq_ignore_ascii_case(&input.transformer) {
                        return Err(reject(
                            "solar_network_seed_transformer_identity_mismatch",
                            format!("{} marker {key} identifies {marker:?}", input.transformer),
                        ));
                    }
                }
            }
            let kva = numeric(props.get("tr_size").or_else(|| props.get("kva")))
                .filter(|value| *value > 0.0 && value.fract() == 0.0)
                .ok_or_else(|| {
                    reject(
                        "solar_network_seed_transformer_kva_invalid",
                        &input.transformer,
                    )
                })? as i64;
            let mut customers = BTreeMap::<String, i64>::new();
            for feature in features(layers, "customers", &input.transformer)? {
                let props = properties(feature, &input.transformer, "customers")?;
                let category = props
                    .get("category")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| {
                        reject(
                            "solar_network_seed_customer_category_missing",
                            &input.transformer,
                        )
                    })?;
                *customers.entry(category.to_string()).or_default() += 1;
            }
            if customers.is_empty() {
                return Err(reject(
                    "solar_network_seed_customers_missing",
                    &input.transformer,
                ));
            }
            let lv_length_m = features(layers, "lv_lines", &input.transformer)?
                .iter()
                .try_fold(0.0, |sum, feature| {
                    Ok::<_, SolarNetworkSeedError>(
                        sum + feature_length_m(
                            feature,
                            &input.transformer,
                            request.lv_geometry_crs,
                        )?,
                    )
                })?;
            if !lv_length_m.is_finite() || lv_length_m <= 0.0 {
                return Err(reject(
                    "solar_network_seed_lv_length_invalid",
                    &input.transformer,
                ));
            }
            Ok(LvFact {
                transformer: input.transformer.clone(),
                city: city.clone(),
                phase: phase.clone(),
                kva,
                lv_length_m: round3(lv_length_m),
                customers,
            })
        })
        .collect()
}

fn features<'a>(
    layers: &'a Map<String, Value>,
    name: &str,
    transformer: &str,
) -> Result<&'a Vec<Value>, SolarNetworkSeedError> {
    layers
        .get(name)
        .and_then(|layer| layer.get("features"))
        .and_then(Value::as_array)
        .ok_or_else(|| {
            reject(
                "solar_network_seed_lv_layer_missing",
                format!("{transformer}#{name}"),
            )
        })
}

fn properties<'a>(
    feature: &'a Value,
    transformer: &str,
    layer: &str,
) -> Result<&'a Map<String, Value>, SolarNetworkSeedError> {
    feature
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| {
            reject(
                "solar_network_seed_feature_properties_missing",
                format!("{transformer}#{layer}"),
            )
        })
}

fn numeric(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Number(value) => value.as_f64(),
        Value::String(value) => value.trim().parse().ok(),
        _ => None,
    }
    .filter(|value| value.is_finite())
}

fn feature_length_m(
    feature: &Value,
    transformer: &str,
    geometry_crs: LvGeometryCrs,
) -> Result<f64, SolarNetworkSeedError> {
    let props = properties(feature, transformer, "lv_lines")?;
    if let Some(length) = numeric(props.get("length").or_else(|| props.get("length_m"))) {
        if length > 0.0 {
            return Ok(length);
        }
    }
    let geometry = feature
        .get("geometry")
        .and_then(Value::as_object)
        .ok_or_else(|| reject("solar_network_seed_lv_geometry_missing", transformer))?;
    let kind = geometry.get("type").and_then(Value::as_str).unwrap_or("");
    let coordinates = geometry
        .get("coordinates")
        .ok_or_else(|| reject("solar_network_seed_lv_geometry_missing", transformer))?;
    match kind {
        "LineString" => line_coordinates_length(coordinates, geometry_crs),
        "MultiLineString" => coordinates
            .as_array()
            .ok_or_else(|| reject("solar_network_seed_lv_geometry_invalid", transformer))?
            .iter()
            .try_fold(0.0, |sum, line| {
                Ok(sum + line_coordinates_length(line, geometry_crs)?)
            }),
        _ => Err(reject(
            "solar_network_seed_lv_geometry_invalid",
            format!("{transformer} has {kind:?}"),
        )),
    }
}

fn line_coordinates_length(
    value: &Value,
    geometry_crs: LvGeometryCrs,
) -> Result<f64, SolarNetworkSeedError> {
    let coordinates = value
        .as_array()
        .ok_or_else(|| reject("solar_network_seed_lv_geometry_invalid", "coordinates"))?;
    if coordinates.len() < 2 {
        return Err(reject(
            "solar_network_seed_lv_geometry_invalid",
            "line has fewer than two coordinates",
        ));
    }
    let points = coordinates
        .iter()
        .map(|coordinate| {
            let pair = coordinate
                .as_array()
                .ok_or_else(|| reject("solar_network_seed_lv_geometry_invalid", "coordinate"))?;
            let x = pair
                .first()
                .and_then(Value::as_f64)
                .ok_or_else(|| reject("solar_network_seed_lv_geometry_invalid", "x"))?;
            let y = pair
                .get(1)
                .and_then(Value::as_f64)
                .ok_or_else(|| reject("solar_network_seed_lv_geometry_invalid", "y"))?;
            Ok((x, y))
        })
        .collect::<Result<Vec<_>, SolarNetworkSeedError>>()?;
    Ok(points
        .windows(2)
        .map(|pair| segment_length_m(pair[0], pair[1], geometry_crs))
        .sum())
}

fn segment_length_m(a: (f64, f64), b: (f64, f64), geometry_crs: LvGeometryCrs) -> f64 {
    if matches!(geometry_crs, LvGeometryCrs::GeographicWgs84) {
        let lat1 = a.1.to_radians();
        let lat2 = b.1.to_radians();
        let dlat = lat2 - lat1;
        let dlon = (b.0 - a.0).to_radians();
        let h = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
        2.0 * 6_371_008.8 * h.sqrt().asin()
    } else {
        (b.0 - a.0).hypot(b.1 - a.1)
    }
}

fn project_mv(
    request: &Request,
    city_values: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, BTreeMap<String, f64>>, SolarNetworkSeedError> {
    let mut ids = BTreeSet::new();
    let mut output = BTreeMap::<String, BTreeMap<String, f64>>::new();
    for alignment in &request.mv.alignments {
        if alignment.alignment_id.trim().is_empty() || !ids.insert(alignment.alignment_id.as_str())
        {
            return Err(reject(
                "solar_network_seed_mv_alignment_duplicate",
                &alignment.alignment_id,
            ));
        }
        if !alignment.length_m.is_finite() || alignment.length_m <= 0.0 {
            return Err(reject(
                "solar_network_seed_mv_length_invalid",
                &alignment.alignment_id,
            ));
        }
        let city_value = alignment
            .tags
            .get(&request.mv_city_definition_id)
            .ok_or_else(|| {
                reject(
                    "solar_network_seed_mv_city_missing",
                    &alignment.alignment_id,
                )
            })?;
        let phase = alignment
            .tags
            .get(&request.mv_phase_definition_id)
            .ok_or_else(|| {
                reject(
                    "solar_network_seed_mv_phase_missing",
                    &alignment.alignment_id,
                )
            })?;
        let city_id = city_values.get(city_value).ok_or_else(|| {
            reject(
                "solar_network_seed_mv_city_unbound",
                format!("{}={city_value:?}", request.mv_city_definition_id),
            )
        })?;
        *output
            .entry(city_id.clone())
            .or_default()
            .entry(phase.clone())
            .or_default() += alignment.length_m;
    }
    Ok(output)
}

fn group_lv(
    facts: &[LvFact],
    city_values: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, Vec<LvFact>>, SolarNetworkSeedError> {
    let mut output = BTreeMap::<String, Vec<LvFact>>::new();
    for fact in facts {
        let city_id = city_values.get(&fact.city).ok_or_else(|| {
            reject(
                "solar_network_seed_lv_city_unbound",
                format!("{}={:?}", fact.transformer, fact.city),
            )
        })?;
        output
            .entry(city_id.clone())
            .or_default()
            .push(fact.clone());
    }
    for city in output.values_mut() {
        city.sort_by(|left, right| left.transformer.cmp(&right.transformer));
    }
    Ok(output)
}

fn build_network_data(
    lv: &[LvFact],
    mv: &BTreeMap<String, f64>,
    active_phases: &[String],
) -> Result<Value, SolarNetworkSeedError> {
    let rows = lv.iter().map(transformer_row).collect::<Vec<_>>();
    let active = lv
        .iter()
        .filter(|fact| active_phases.contains(&fact.phase))
        .collect::<Vec<_>>();
    let all = lv.iter().collect::<Vec<_>>();
    let all_phases = lv
        .iter()
        .map(|fact| fact.phase.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let customers_full = aggregate_customers(&all)?;
    let customers_active = aggregate_customers(&active)?;
    let total_full = checked_customer_total(&customers_full)?;
    let total_active = checked_customer_total(&customers_active)?;
    let total_lv_full = round3(lv.iter().map(|fact| fact.lv_length_m).sum::<f64>() / 1_000.0);
    let total_lv_active = round3(active.iter().map(|fact| fact.lv_length_m).sum::<f64>() / 1_000.0);
    let total_mv_km = round3(mv.values().sum::<f64>() / 1_000.0);
    let mut phase_breakdown = Map::new();
    for phase in &all_phases {
        let phase_rows = lv
            .iter()
            .filter(|fact| &fact.phase == phase)
            .collect::<Vec<_>>();
        let customers = aggregate_customers(&phase_rows)?;
        phase_breakdown.insert(
            phase.clone(),
            json!({
                "is_active": active_phases.contains(phase),
                "n_transformers": phase_rows.len(),
                "total_bt_km": round3(phase_rows.iter().map(|fact| fact.lv_length_m).sum::<f64>() / 1_000.0),
                "total_clients": checked_customer_total(&customers)?,
                "customers_by_category": customers,
            }),
        );
    }
    let active_rows = active
        .iter()
        .map(|fact| transformer_row(fact))
        .collect::<Vec<_>>();
    let mut fleet = BTreeMap::<String, i64>::new();
    for fact in lv {
        *fleet.entry(fact.kva.to_string()).or_default() += 1;
    }
    Ok(json!({
        "customers": customers_full.iter().map(|(category, count)| json!({"category": category, "count": count})).collect::<Vec<_>>(),
        "proposed": {
            "active_network_phases": active_phases,
            "active_summary": {
                "active_network_phases": active_phases,
                "active_transformers": active_rows,
                "all_phases": all_phases,
                "customers_by_category_active": customers_active,
                "customers_by_category_full": customers_full,
                "is_phased_deployment": phase_breakdown.len() > 1,
                "n_active_transformers": active.len(),
                "n_total_transformers": lv.len(),
                "phase_breakdown": phase_breakdown,
                "total_bt_km_active": total_lv_active,
                "total_bt_km_full": total_lv_full,
                "total_clients_active": total_active,
                "total_clients_full": total_full,
                "total_mv_length_km": total_mv_km,
            },
            "customers": customers_full.iter().map(|(category, count)| json!({"category": category, "count": count})).collect::<Vec<_>>(),
            "num_transformers": lv.len(),
            "total_bt_km": total_lv_full,
            "total_mv_length_km": total_mv_km,
            "transformers": rows,
        },
        "material_quantities": {
            "derived_from": "01_city_inputs.network_data.proposed",
            "customers_by_category": customers_full,
            "total_customers": total_full,
            "total_lv_length_m": round3(lv.iter().map(|fact| fact.lv_length_m).sum()),
            "transformers_by_kva": fleet,
        },
    }))
}

fn transformer_row(fact: &LvFact) -> Value {
    let clients: i64 = fact.customers.values().sum();
    let mut row = Map::new();
    row.insert("name".into(), json!(fact.transformer));
    row.insert("phase".into(), json!(fact.phase));
    row.insert("kva".into(), json!(fact.kva));
    row.insert("bt_km".into(), json!(round3(fact.lv_length_m / 1_000.0)));
    row.insert("clients".into(), json!(clients));
    for (category, count) in &fact.customers {
        row.insert(category.clone(), json!(count));
    }
    Value::Object(row)
}

fn aggregate_customers(facts: &[&LvFact]) -> Result<BTreeMap<String, i64>, SolarNetworkSeedError> {
    let mut output = BTreeMap::<String, i64>::new();
    for fact in facts {
        for (category, count) in &fact.customers {
            let slot = output.entry(category.clone()).or_default();
            *slot = slot
                .checked_add(*count)
                .ok_or_else(|| reject("solar_network_seed_customer_overflow", category))?;
        }
    }
    Ok(output)
}

fn checked_customer_total(customers: &BTreeMap<String, i64>) -> Result<i64, SolarNetworkSeedError> {
    customers.values().try_fold(0_i64, |sum, count| {
        sum.checked_add(*count)
            .ok_or_else(|| reject("solar_network_seed_customer_overflow", "total"))
    })
}

fn merge_network_data(
    city_id: &str,
    base: &Value,
    generated: Value,
) -> Result<Value, SolarNetworkSeedError> {
    let mut document = base.clone();
    let city_inputs = document
        .get_mut("01_city_inputs")
        .and_then(Value::as_object_mut)
        .ok_or_else(|| reject("solar_network_seed_base_missing_city_inputs", city_id))?;
    let existing = city_inputs
        .entry("network_data")
        .or_insert_with(|| json!({}));
    let target = existing
        .as_object_mut()
        .ok_or_else(|| reject("solar_network_seed_base_network_invalid", city_id))?;
    let generated = generated
        .as_object()
        .expect("generated network data is an object");
    for (key, value) in generated {
        target.insert(key.clone(), value.clone());
    }
    Ok(document)
}

fn round3(value: f64) -> f64 {
    (value * 1_000.0).round() / 1_000.0
}

fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(kva: i64, category: &str, x: f64) -> Value {
        json!({
            "layers": {
                "tr": {"type":"FeatureCollection","features":[{
                    "type":"Feature","geometry":{"type":"Point","coordinates":[30.0,-2.0]},
                    "properties":{"tr_size":kva}
                }]},
                "customers": {"type":"FeatureCollection","features":[{
                    "type":"Feature","geometry":{"type":"Point","coordinates":[30.0,-2.0]},
                    "properties":{"category":category}
                }]},
                "lv_lines": {"type":"FeatureCollection","features":[{
                    "type":"Feature","geometry":{"type":"LineString","coordinates":[[x,0.0],[x+100.0,0.0]]},
                    "properties":{}
                }]}
            }
        })
    }

    fn base(city: &str) -> Value {
        json!({
            "_root": {"city_id":city},
            "00_templates": {},
            "01_city_inputs": {
                "city":{"city":city,"latitude":0,"longitude":0,"tz":"UTC"},
                "design_options":{"project_scope":"network"},
                "network_data":{"consider_inst":false,"minimum_load_profile":0.2,"maps":{"keep":"me"}}
            }
        })
    }

    fn request() -> Value {
        let tags = json!({
            "schema_version":"ds-report.design-tags/v3","project_id":"ader_loc7",
            "groups":[
                {"definition_id":"grp_city","cardinality":"single","state":"active","applies_to":["lv_transformer"]},
                {"definition_id":"grp_phase","cardinality":"single","state":"active","applies_to":["lv_transformer"]}
            ],
            "assignments":[
                {"object":{"kind":"lv_transformer","id":"BERE_01"},"definition_id":"grp_city","values":["Bere"]},
                {"object":{"kind":"lv_transformer","id":"BERE_01"},"definition_id":"grp_phase","values":["I"]},
                {"object":{"kind":"lv_transformer","id":"BERE_02"},"definition_id":"grp_city","values":["Bere"]},
                {"object":{"kind":"lv_transformer","id":"BERE_02"},"definition_id":"grp_phase","values":["II"]}
            ]
        });
        let tag_document = serde_json::to_string(&tags).unwrap();
        let tag_sha256 = sha256(tag_document.as_bytes());
        json!({
            "schema": SOLAR_NETWORK_SEED_REQUEST_SCHEMA,
            "project_id":"ader_loc7",
            "mv_city_definition_id":"city",
            "mv_phase_definition_id":"phasing",
            "lv_city_definition_id":"grp_city",
            "lv_phase_definition_id":"grp_phase",
            "lv_geometry_crs":"projected_metric",
            "mv":{
                "schema":MV_ALIGNMENT_FACTS_SCHEMA,
                "model_id":"mv","model_revision":"rev:1","package_sha256":format!("sha256:{}", "a".repeat(64)),
                "alignments":[
                    {"alignment_id":"a1","length_m":1200.0,"tags":{"city":"Béré","phasing":"I"}},
                    {"alignment_id":"a2","length_m":300.0,"tags":{"city":"Béré","phasing":"II"}}
                ]
            },
            "lv_tag_projection":{"document":tag_document,"sha256":tag_sha256},
            "lv_transformers":[
                {"transformer":"BERE_01","room":room(100,"Pauvre",1000.0)},
                {"transformer":"BERE_02","room":room(50,"Riche",1200.0)}
            ],
            "cities":[{
                "city_id":"aderm_bere","display_name":"Bere",
                "mv_city_values":["Béré"],"lv_city_values":["Bere"],"active_phases":["I"],
                "base_document":base("aderm_bere")
            }]
        })
    }

    #[test]
    fn mv_lengths_and_lv_facts_keep_their_separate_authorities() {
        let encoded = serde_json::to_vec(&request()).unwrap();
        let result: Value =
            serde_json::from_str(&plan_solar_network_seed(&encoded).unwrap()).unwrap();
        assert_eq!(result["schema"], SOLAR_NETWORK_SEED_PLAN_SCHEMA);
        assert_eq!(result["cities"][0]["total_mv_length_km"], 1.5);
        assert_eq!(result["cities"][0]["transformer_count"], 2);
        assert_eq!(result["cities"][0]["customer_count"], 2);
        assert_eq!(result["excluded_facts"], json!(["mv_structure_counts"]));
        let network = &result["writes"][0]["document"]["01_city_inputs"]["network_data"];
        assert_eq!(network["maps"]["keep"], "me");
        assert_eq!(
            network["proposed"]["active_summary"]["n_active_transformers"],
            1
        );
        assert_eq!(
            network["proposed"]["active_summary"]["total_mv_length_km"],
            1.5
        );
        assert_eq!(
            network["material_quantities"]["transformers_by_kva"],
            json!({"100":1,"50":1})
        );
        assert_eq!(network["material_quantities"]["total_lv_length_m"], 200.0);
        assert!(network["material_quantities"]
            .get("lv_pole_count")
            .is_none());
    }

    #[test]
    fn unbound_or_missing_tags_refuse_instead_of_guessing_names() {
        let mut unbound = request();
        unbound["cities"][0]["mv_city_values"] = json!(["Bere"]);
        let error = plan_solar_network_seed(&serde_json::to_vec(&unbound).unwrap()).unwrap_err();
        assert!(error
            .to_string()
            .starts_with("solar_network_seed_mv_city_unbound:"));

        let mut missing_phase = request();
        let mut tag_document: Value = serde_json::from_str(
            missing_phase["lv_tag_projection"]["document"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        tag_document["assignments"]
            .as_array_mut()
            .unwrap()
            .retain(|row| {
                !(row["object"]["id"] == "BERE_02" && row["definition_id"] == "grp_phase")
            });
        let tag_document = serde_json::to_string(&tag_document).unwrap();
        missing_phase["lv_tag_projection"] = json!({
            "sha256": sha256(tag_document.as_bytes()),
            "document": tag_document,
        });
        let error =
            plan_solar_network_seed(&serde_json::to_vec(&missing_phase).unwrap()).unwrap_err();
        assert!(error
            .to_string()
            .contains("solar_network_seed_lv_phase_missing"));
    }

    #[test]
    fn request_is_bounded_and_closed() {
        assert!(plan_solar_network_seed(&[])
            .unwrap_err()
            .to_string()
            .contains("bounded"));
        let mut unknown = request();
        unknown["mv"]["structure_count"] = json!(785);
        let error = plan_solar_network_seed(&serde_json::to_vec(&unknown).unwrap()).unwrap_err();
        assert!(error.to_string().contains("request_invalid"));

        let mut unwrapped = request();
        unwrapped["lv_transformers"][0]["room"] =
            unwrapped["lv_transformers"][0]["room"]["layers"].clone();
        let error = plan_solar_network_seed(&serde_json::to_vec(&unwrapped).unwrap()).unwrap_err();
        assert!(error
            .to_string()
            .contains("wrap captured FastLV output.gdfs as room.layers"));

        let mut stale_tags = request();
        stale_tags["lv_tag_projection"]["sha256"] = json!(format!("sha256:{}", "f".repeat(64)));
        let error = plan_solar_network_seed(&serde_json::to_vec(&stale_tags).unwrap()).unwrap_err();
        assert!(error.to_string().contains("lv_tags_digest_mismatch"));

        let mut casing = request();
        casing["lv_transformers"][0]["room"]["layers"]["tr"]["features"][0]["properties"]
            ["transfo"] = json!("bere_01");
        assert!(plan_solar_network_seed(&serde_json::to_vec(&casing).unwrap()).is_ok());

        let mut mismatched_room = request();
        mismatched_room["lv_transformers"][0]["room"]["layers"]["tr"]["features"][0]
            ["properties"]["transfo"] = json!("SOME_OTHER_TRANSFORMER");
        let error =
            plan_solar_network_seed(&serde_json::to_vec(&mismatched_room).unwrap()).unwrap_err();
        assert!(error.to_string().contains("transformer_identity_mismatch"));
    }
}
