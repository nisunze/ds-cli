//! The local Combined Report (`combined_transformer`), decided once for every
//! native host.
//!
//! Combined is a data-only deliverable: SHP, XLSX and GeoJSONSeq, plus GPKG or
//! Network Information when the project's output selection names them, and
//! never a PDF. DS Core acquires each active transformer's saved room through
//! its governed hold, stages the canonical LV layers the Reporter admits and
//! runs the Reporter's `export-combined-transformer-report` task. This module
//! decides, without IO: the run's formats (the shared
//! [`report_formats::plan`] for a combined run), the room projection, the
//! publication identity and digests, the engine requests, whether the
//! engine's answers fit them, the collision gate over the project's consumed
//! collision result, and the per-transformer voltage-drop warnings a receipt
//! carries. Hosts acquire, stage, run, verify files, seal and keep the clock.
//!
//! Voltage drop is never computed or required here: a combined run selects no
//! voltage-drop output, so a transformer whose saved analysis is missing,
//! stale or failed is a receipt warning and never a partial package.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use super::{
    engine_identity, exact_lower_sha256, jcs, reportable_transformer, sha256_hex,
    transformer_document_bytes, valid_project_id, valid_run_id, AdminBoundsEvidence, Blocker,
    EngineIdentity, InputReceipt, VerifiedArtifact, MAX_ARTIFACT_BYTES,
};
use crate::report_formats::{self, Output};

/// The Reporter task this module plans and verifies.
pub const TASK: &str = "export_combined_transformer_report";
/// The engine subcommand a host names for that task.
pub const SUBCOMMAND: &str = "export-combined-transformer-report";
/// The Reporter's read-only companion that names the media scope of staged rooms.
pub const MEDIA_TASK: &str = "discover_combined_media_references";
/// The engine subcommand a host names for the media-scope task.
pub const MEDIA_SUBCOMMAND: &str = "discover-combined-media-references";
/// The publication identity of the Combined Report: the operation is
/// `export-combined_transformer`, the variant `default`.
pub const IDENTITY: &str = "combined_transformer";
/// A combined publication spans many saved revisions, so its batch carries
/// no single transformer revision.
pub const PUBLICATION_REVISION: i64 = 0;
/// The Reporter's combined result document version this module reads.
pub const RESULT_SCHEMA_VERSION: u64 = 4;
/// The Reporter's media-scope result document version this module reads.
pub const MEDIA_SCOPE_SCHEMA_VERSION: u64 = 2;
/// The receipt a host writes beside the combined artifacts.
pub const RECEIPT_SCHEMA: &str = "ds.report-combined-run/v1";
/// The canonical preimage of a combined run's input fingerprint.
pub const INPUT_SCHEMA: &str = "ds.report-combined-input/v1";
/// The most rooms one combined run may stage: the project roster bound.
pub const MAX_TRANSFORMERS: usize = super::MAX_BATCH_TRANSFORMERS;
/// One staged room, as the Reporter bounds it.
pub const MAX_ROOM_BYTES: u64 = 64 * 1024 * 1024;
/// Every staged input together, as the Reporter bounds it.
pub const MAX_TOTAL_INPUT_BYTES: u64 = 512 * 1024 * 1024;
/// A media grant is one small document however many photos it covers.
pub const MAX_MEDIA_GRANT_BYTES: u64 = 64 * 1024;
/// The most projects one combined media scope may name.
pub const MAX_MEDIA_PROJECTS: usize = 32;
/// The largest combined result document a host reads.
pub const MAX_RESULT_BYTES: u64 = 16 * 1024 * 1024;
/// The largest media-scope result document a host reads.
pub const MAX_MEDIA_RESULT_BYTES: u64 = 64 * 1024;
/// The saved LV room layers the combined task admits and aggregates. Any
/// other saved layer (diagnostics, analysis, survey) is not report input.
pub const ROOM_LAYERS: &[&str] = &[
    "customers",
    "lv_poles",
    "field_notes",
    "lv_lines",
    "service_cables",
    "spans",
    "tr",
    "tapping_poles",
];
/// The authority fact the Reporter declares it cannot verify itself: the
/// cloud-owned optional `transformer_reviews` layer is not part of a local run.
pub const REVIEWS_REQUIREMENT: &str = "combined_transformer_reviews_absence";
/// The project setting that allows a combined report while collisions stand.
pub const COLLISION_ALLOW_SETTING: &str = "allow_combined_reports_with_collisions";
/// The project setting that bounds the collision rows a combined report admits.
pub const COLLISION_MAX_SETTING: &str = "max_collision_rows_for_combined_reports";
pub const DEFAULT_ALLOW_WITH_COLLISIONS: bool = true;
pub const DEFAULT_MAX_COLLISION_ROWS: u64 = 10;
/// The operator's sentence for a blocked collision gate, as the project's
/// Info Center has always shown it.
pub const COLLISION_MESSAGE: &str = "Too many transformers are overlapping. Please review collisions and resolve them to export combined STATS. Otherwise your results will not be accurate.";
/// The most voltage-drop warnings one receipt lists; the count is exact.
pub const MAX_VOLTAGE_DROP_WARNINGS: usize = 200;

/// The combined run's project facts and output plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scope {
    pub project: String,
    pub country: String,
    pub requires_admin_bounds: bool,
    pub sheets_sha256: String,
    pub reference_semantic_sha256: String,
    /// The shared combined plan, in the Reporter's format order.
    pub outputs: Vec<Output>,
}

impl Scope {
    pub fn formats(&self) -> Vec<String> {
        self.outputs
            .iter()
            .map(|output| output.output_id.clone())
            .collect()
    }
}

/// Plan one combined run from the project's server-minted receipt. The
/// formats are the shared combined plan of the sealed sheets; every print
/// output is outside a combined run.
pub fn scope(project: &str, receipt: &InputReceipt) -> Result<Scope, String> {
    if !valid_project_id(project) {
        return Err("A combined report requires one exact project id".into());
    }
    receipt.validate()?;
    if receipt.local_print_recipe.is_some() {
        return Err("A combined report cannot carry a local print recipe".into());
    }
    let outputs = report_formats::plan(&receipt.sheets()?, true)?;
    if outputs.is_empty()
        || outputs
            .iter()
            .any(|output| report_formats::is_printout(&output.format))
    {
        return Err("The combined output plan must be data outputs only".into());
    }
    Ok(Scope {
        project: project.to_owned(),
        country: receipt.country.clone(),
        requires_admin_bounds: receipt.requires_admin_bounds(),
        sheets_sha256: receipt.sheets_sha256.clone(),
        reference_semantic_sha256: receipt.reference_semantic_sha256.clone(),
        outputs,
    })
}

/// One saved room projected onto [`ROOM_LAYERS`]: the exact document a host
/// stages and the evidence its receipt carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedRoom {
    pub pin: RoomPin,
    pub document: Vec<u8>,
}

/// What a staged room is pinned to: the saved revision and content digest it
/// was read at, and the digest and size of the bytes the engine reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoomPin {
    pub transformer: String,
    pub revision: i64,
    pub content_digest: String,
    pub source_sha256: String,
    pub source_bytes: u64,
    pub layers: usize,
    pub features: usize,
}

/// Project one saved room for the combined task. `Ok(None)` is a room with
/// nothing to aggregate — no feature in any report layer — which a run names
/// rather than stages. The saved content digest, when the service gave one,
/// must be the digest of the layers as read.
pub fn stage_room(
    transformer: &str,
    revision: i64,
    content_digest: Option<&str>,
    layers: &BTreeMap<String, Value>,
) -> Result<Option<StagedRoom>, String> {
    reportable_transformer(transformer)?;
    if revision <= 0 {
        return Err(format!("{transformer} has no positive saved revision"));
    }
    let computed = jcs::layers_content_digest(layers)?;
    if content_digest.is_some_and(|expected| expected != computed) {
        return Err(format!(
            "{transformer}: the saved content digest does not match its layers"
        ));
    }
    let projected: BTreeMap<String, Value> = layers
        .iter()
        .filter(|(name, _)| ROOM_LAYERS.contains(&name.as_str()))
        .map(|(name, collection)| (name.clone(), collection.clone()))
        .collect();
    let mut features = 0_usize;
    for (name, collection) in &projected {
        features = features
            .checked_add(super::validate_feature_collection(name, collection)?)
            .ok_or("Combined room feature count overflowed")?;
    }
    if features == 0 {
        return Ok(None);
    }
    let document = transformer_document_bytes(&projected)?;
    if document.len() as u64 > MAX_ROOM_BYTES {
        return Err(format!(
            "{transformer} exceeds the {MAX_ROOM_BYTES}-byte combined room bound"
        ));
    }
    Ok(Some(StagedRoom {
        pin: RoomPin {
            transformer: transformer.to_owned(),
            revision,
            content_digest: computed,
            source_sha256: sha256_hex(&document),
            source_bytes: document.len() as u64,
            layers: projected.len(),
            features,
        },
        document,
    }))
}

/// The pins of one run, sorted as the Reporter sorts its inputs and bounded.
pub fn sorted_pins(mut pins: Vec<RoomPin>) -> Result<Vec<RoomPin>, String> {
    if pins.is_empty() {
        return Err("A combined report needs at least one room with report features".into());
    }
    if pins.len() > MAX_TRANSFORMERS {
        return Err(format!(
            "A combined report stages at most {MAX_TRANSFORMERS} rooms"
        ));
    }
    pins.sort_by(|left, right| left.transformer.cmp(&right.transformer));
    let mut total = 0_u64;
    for (index, pin) in pins.iter().enumerate() {
        reportable_transformer(&pin.transformer)?;
        if index > 0 && pins[index - 1].transformer == pin.transformer {
            return Err(format!("{} is staged twice", pin.transformer));
        }
        if pin.revision <= 0
            || !exact_lower_sha256(&pin.content_digest)
            || !exact_lower_sha256(&pin.source_sha256)
            || pin.source_bytes == 0
            || pin.source_bytes > MAX_ROOM_BYTES
            || pin.layers == 0
            || pin.features == 0
        {
            return Err(format!(
                "{} has an invalid staged room pin",
                pin.transformer
            ));
        }
        total = total
            .checked_add(pin.source_bytes)
            .filter(|total| *total <= MAX_TOTAL_INPUT_BYTES)
            .ok_or_else(|| {
                format!("The staged rooms exceed the {MAX_TOTAL_INPUT_BYTES}-byte combined bound")
            })?;
    }
    Ok(pins)
}

/// The run's input fingerprint: the project's sealed receipt, the plan and
/// every staged saved revision, so an identical rerun is one publication.
pub fn input_base_fingerprint(
    scope: &Scope,
    pins: &[RoomPin],
    transformer_natures: Option<&BTreeMap<String, String>>,
) -> Result<String, String> {
    jcs::sha256(&json!({
        "schema": INPUT_SCHEMA,
        "project": scope.project,
        "country": scope.country,
        "sheets_sha256": scope.sheets_sha256,
        "reference_semantic_sha256": scope.reference_semantic_sha256,
        "formats": scope.formats(),
        "transformers": pins.iter().map(|pin| json!({
            "transformer": pin.transformer,
            "revision": pin.revision,
            "content_digest": pin.content_digest,
        })).collect::<Vec<_>>(),
        "transformer_natures": transformer_natures,
    }))
}

/// The digest of the exact staged room bytes, the combined counterpart of an
/// individual run's `room_content_sha256`.
pub fn room_content_sha256(pins: &[RoomPin]) -> Result<String, String> {
    jcs::sha256(&json!(pins
        .iter()
        .map(|pin| json!({
            "transformer": pin.transformer,
            "revision": pin.revision,
            "content_digest": pin.content_digest,
            "source_sha256": pin.source_sha256,
            "source_bytes": pin.source_bytes,
        }))
        .collect::<Vec<_>>()))
}

/// Bytes and digest of one staged file, as the Reporter receipts it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileReceipt {
    pub bytes: u64,
    pub sha256: String,
}

impl FileReceipt {
    pub fn of(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.len() as u64,
            sha256: sha256_hex(bytes),
        }
    }
}

/// What a host staged for one run, by path, with the receipts it proved.
#[derive(Debug, Clone)]
pub struct Staged<'a> {
    pub network_config: (&'a Path, &'a FileReceipt),
    /// Sorted pins ([`sorted_pins`]) and the file each one was written to.
    pub rooms: &'a [(PathBuf, RoomPin)],
    pub out_dir: &'a Path,
    /// The verified installed reference: path and its exact byte receipt.
    pub admin_bounds: Option<(&'a Path, &'a FileReceipt)>,
    pub media_grant: Option<(&'a Path, &'a FileReceipt)>,
    pub transformer_natures: Option<&'a BTreeMap<String, String>>,
}

impl Staged<'_> {
    fn total_input_bytes(&self) -> Result<u64, String> {
        let mut total = self.network_config.1.bytes;
        for bytes in self
            .admin_bounds
            .iter()
            .chain(self.media_grant.iter())
            .map(|(_, receipt)| receipt.bytes)
            .chain(self.rooms.iter().map(|(_, pin)| pin.source_bytes))
        {
            total = total
                .checked_add(bytes)
                .ok_or("Combined input bytes overflowed")?;
        }
        if total > MAX_TOTAL_INPUT_BYTES {
            return Err(format!(
                "The combined inputs exceed the Reporter's {MAX_TOTAL_INPUT_BYTES}-byte bound"
            ));
        }
        Ok(total)
    }

    fn validate(&self, scope: &Scope) -> Result<(), String> {
        let pins: Vec<RoomPin> = self.rooms.iter().map(|(_, pin)| pin.clone()).collect();
        if sorted_pins(pins.clone())? != pins {
            return Err("Combined rooms must be staged in the Reporter's sorted order".into());
        }
        if scope.requires_admin_bounds != self.admin_bounds.is_some() {
            return Err(if scope.requires_admin_bounds {
                "A Rwanda combined report requires the verified installed administrative reference"
                    .into()
            } else {
                "Only a Rwanda combined report stages the administrative reference".into()
            });
        }
        if let Some((_, grant)) = self.media_grant {
            if grant.bytes == 0 || grant.bytes > MAX_MEDIA_GRANT_BYTES {
                return Err("A combined media grant is a document of 1 to 65536 bytes".into());
            }
        }
        if let Some(natures) = self.transformer_natures {
            if natures.len() > MAX_TRANSFORMERS
                || natures
                    .iter()
                    .any(|(name, nature)| name.is_empty() || nature.is_empty())
            {
                return Err("Combined transformer natures are malformed".into());
            }
        }
        self.total_input_bytes().map(|_| ())
    }
}

fn path_text(path: &Path) -> Result<&str, String> {
    path.to_str()
        .filter(|text| !text.is_empty() && path.is_absolute())
        .ok_or_else(|| "Combined staging paths must be absolute Unicode paths".to_string())
}

fn room_inputs(staged: &Staged<'_>) -> Result<Vec<Value>, String> {
    staged
        .rooms
        .iter()
        .map(|(path, pin)| Ok(json!({"transformer": pin.transformer, "layers": path_text(path)?})))
        .collect()
}

/// The exact `export_combined_transformer_report` request.
pub fn engine_request(scope: &Scope, staged: &Staged<'_>) -> Result<Value, String> {
    staged.validate(scope)?;
    let mut request = Map::new();
    request.insert("transformers".into(), Value::Array(room_inputs(staged)?));
    request.insert(
        "network_config".into(),
        json!(path_text(staged.network_config.0)?),
    );
    request.insert("out_dir".into(), json!(path_text(staged.out_dir)?));
    request.insert("formats".into(), json!(scope.formats()));
    request.insert("country".into(), json!(scope.country));
    if let Some((path, receipt)) = staged.admin_bounds {
        request.insert("admin_bounds_asset".into(), json!(path_text(path)?));
        request.insert("admin_bounds_sha256".into(), json!(receipt.sha256));
    }
    if let Some((path, _)) = staged.media_grant {
        request.insert("media_grant".into(), json!(path_text(path)?));
    }
    if let Some(natures) = staged.transformer_natures {
        request.insert("transformer_natures".into(), json!(natures));
    }
    Ok(Value::Object(request))
}

/// The exact `discover_combined_media_references` request over the same rooms.
pub fn media_request(scope: &Scope, staged: &Staged<'_>) -> Result<Value, String> {
    staged.validate(scope)?;
    Ok(json!({
        "network_config": path_text(staged.network_config.0)?,
        "transformers": room_inputs(staged)?,
    }))
}

/// The media the staged rooms reference, folded to a bounded answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MediaScope {
    pub reference_count: u64,
    pub source_projects: Vec<String>,
    pub features_scanned: u64,
}

impl MediaScope {
    /// Source projects outside the named project: their photos need a grant
    /// this run cannot mint under the named project alone.
    pub fn foreign_projects(&self, project: &str) -> Vec<String> {
        self.source_projects
            .iter()
            .filter(|source| source.as_str() != project)
            .cloned()
            .collect()
    }
}

/// Prove one media-scope result answers the staged rooms.
pub fn verify_media_scope(document: &[u8], rooms: usize) -> Result<MediaScope, String> {
    if document.len() as u64 > MAX_MEDIA_RESULT_BYTES {
        return Err("The combined media scope exceeds its byte bound".into());
    }
    let value: Value = serde_json::from_slice(document)
        .map_err(|error| format!("The combined media scope is malformed: {error}"))?;
    let count = |key: &str| value[key].as_u64();
    let projects = value["source_projects"].as_array().map(|projects| {
        projects
            .iter()
            .map(|project| project.as_str().map(str::to_owned))
            .collect::<Option<Vec<_>>>()
    });
    let (Some(reference_count), Some(features_scanned), Some(Some(source_projects))) = (
        count("reference_count"),
        count("features_scanned"),
        projects,
    ) else {
        return Err("The combined media scope omits its counts or projects".into());
    };
    let unique: BTreeSet<&String> = source_projects.iter().collect();
    if count("schema_version") != Some(MEDIA_SCOPE_SCHEMA_VERSION)
        || value["task"] != MEDIA_TASK
        || value["status"] != "completed"
        || count("transformers_scanned") != Some(rooms as u64)
        || source_projects.len() > MAX_MEDIA_PROJECTS
        || unique.len() != source_projects.len()
        || unique.into_iter().cloned().collect::<Vec<_>>() != source_projects
        || source_projects
            .iter()
            .any(|project| !valid_project_id(project))
        || (reference_count == 0 && !source_projects.is_empty())
    {
        return Err("The combined media scope does not answer the staged rooms".into());
    }
    Ok(MediaScope {
        reference_count,
        source_projects,
        features_scanned,
    })
}

/// Why a combined result document is not a deliverable.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum CombinedRefusal {
    /// The engine built less than the contract: every planned format is part
    /// of the deliverable, so a partial or failed run is not one.
    Incomplete {
        status: String,
        blockers: Vec<Blocker>,
        artifacts: usize,
        /// The engineering facts that kept quantities out of the workbook,
        /// grouped per transformer, when the engine's preflight names them.
        engineering: Option<EngineeringGaps>,
    },
    /// The document does not answer this run.
    Invalid { message: String },
}

impl CombinedRefusal {
    fn invalid(message: impl Into<String>) -> Self {
        Self::Invalid {
            message: message.into(),
        }
    }
}

/// One transformer's missing or invalid engineering facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EngineeringGap {
    /// As the engine stamps it on report rows.
    pub transformer: String,
    pub findings: usize,
    /// `layer.field`, sorted and bounded.
    pub fields: Vec<String>,
}

/// The engine's engineering preflight folded per transformer: what an
/// operator fixes before the workbook can publish quantities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct EngineeringGaps {
    /// Every blocking fact the engine counted.
    pub blockers: usize,
    pub transformers: Vec<EngineeringGap>,
    /// Facts the engine counted without detailing them.
    pub omitted_details: usize,
}

/// The most transformers and fields one summary names; counts stay exact.
const MAX_GAP_TRANSFORMERS: usize = 50;
const MAX_GAP_FIELDS: usize = 16;

/// Fold the result's engineering preflight, or `None` when it names no
/// blocking fact or its counts disagree. A project-wide preflight carries far
/// more source and lineage rows than one transformer's, so only the parts
/// folded here are checked: the itemized findings, which the engine bounds,
/// and counts that account for every blocking fact.
pub fn engineering_gaps(result: &Value) -> Option<EngineeringGaps> {
    let preflight: super::engineering::EngineeringPreflight =
        serde_json::from_value(result.get("engineering_preflight")?.clone()).ok()?;
    let text = |value: &str| {
        !value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
    };
    if preflight.schema != "ds.report.engineering-preflight/v1"
        || preflight.blockers == 0
        || preflight.findings.len() > 100
        || preflight
            .findings
            .len()
            .checked_add(preflight.omitted_details)
            != Some(preflight.blockers)
        || preflight
            .finding_counts
            .values()
            .try_fold(0_usize, |sum, count| sum.checked_add(*count))
            != Some(preflight.blockers)
        || preflight.findings.iter().any(|finding| {
            !text(&finding.layer)
                || !text(&finding.field)
                || finding
                    .transformer
                    .as_deref()
                    .is_some_and(|name| !text(name))
        })
    {
        return None;
    }
    let mut grouped: BTreeMap<String, (usize, BTreeSet<String>)> = BTreeMap::new();
    for finding in &preflight.findings {
        let entry = grouped
            .entry(
                finding
                    .transformer
                    .clone()
                    .unwrap_or_else(|| "unattributed".into()),
            )
            .or_default();
        entry.0 += 1;
        entry
            .1
            .insert(format!("{}.{}", finding.layer, finding.field));
    }
    Some(EngineeringGaps {
        blockers: preflight.blockers,
        transformers: grouped
            .into_iter()
            .take(MAX_GAP_TRANSFORMERS)
            .map(|(transformer, (findings, fields))| EngineeringGap {
                transformer,
                findings,
                fields: fields.into_iter().take(MAX_GAP_FIELDS).collect(),
            })
            .collect(),
        omitted_details: preflight.omitted_details,
    })
}

/// A complete, verified combined answer.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct VerifiedCombined {
    /// One artifact per planned output, in plan order.
    pub artifacts: Vec<VerifiedArtifact>,
    pub layers_aggregated: u64,
    pub features_aggregated: u64,
    pub admin_bounds: AdminBoundsEvidence,
    pub geometry_changed: bool,
    /// Engineering findings the workbook carries; a count, never a refusal.
    pub engineering_blockers: u64,
    pub phase_timings: Value,
    pub elapsed_ms: u64,
}

fn file_receipt(value: &Value) -> Option<FileReceipt> {
    serde_json::from_value::<FileReceipt>(value.clone())
        .ok()
        .filter(|receipt| exact_lower_sha256(&receipt.sha256))
}

fn absent(value: &Value) -> bool {
    value.is_null()
}

/// Prove the engine's result answers this exact run: its engine, plan,
/// staged inputs and every planned artifact.
pub fn verify_result(
    document: &[u8],
    scope: &Scope,
    staged: &Staged<'_>,
    engine: &EngineIdentity,
) -> Result<VerifiedCombined, CombinedRefusal> {
    staged.validate(scope).map_err(CombinedRefusal::invalid)?;
    if document.len() as u64 > MAX_RESULT_BYTES {
        return Err(CombinedRefusal::invalid(
            "The combined result exceeds its byte bound",
        ));
    }
    let result: Value = serde_json::from_slice(document).map_err(|error| {
        CombinedRefusal::invalid(format!("The combined result is malformed: {error}"))
    })?;
    if result["schema_version"].as_u64() != Some(RESULT_SCHEMA_VERSION)
        || result["task"] != TASK
        || result["prefix"] != IDENTITY
        || result["country"] != scope.country.as_str()
    {
        return Err(CombinedRefusal::invalid(
            "The combined result does not answer this request",
        ));
    }
    match engine_identity(&result["engine"]) {
        Ok(identity) if identity == *engine => {}
        _ => {
            return Err(CombinedRefusal::invalid(
                "The combined result names another Reporter build",
            ))
        }
    }
    let blockers: Vec<Blocker> = serde_json::from_value(result["blockers"].clone())
        .map_err(|_| CombinedRefusal::invalid("The combined result blockers are malformed"))?;
    let artifacts = result["artifacts"]
        .as_array()
        .ok_or_else(|| CombinedRefusal::invalid("The combined result lists no artifacts"))?;
    let status = result["status"].as_str().unwrap_or_default();
    if status != "completed" || !blockers.is_empty() {
        return Err(CombinedRefusal::Incomplete {
            status: status.to_owned(),
            blockers,
            artifacts: artifacts.len(),
            engineering: engineering_gaps(&result),
        });
    }
    let formats = scope.formats();
    if result["formats_requested"] != json!(formats) {
        return Err(CombinedRefusal::invalid(
            "The combined result answers other formats than the plan",
        ));
    }
    let requirements = result["upstream_admission_requirements"].as_array();
    if !requirements.is_some_and(|rows| {
        rows.len() == 1
            && rows[0]["code"] == REVIEWS_REQUIREMENT
            && rows[0]["forwarded_to_task"] == false
    }) {
        return Err(CombinedRefusal::invalid(
            "The combined result does not declare its closed reviews requirement",
        ));
    }
    let receipt = &result["input_receipt"];
    let expected_total = staged
        .total_input_bytes()
        .map_err(CombinedRefusal::invalid)?;
    let receipted = |value: &Value, staged: Option<&FileReceipt>| match staged {
        Some(expected) => file_receipt(value).as_ref() == Some(expected),
        None => absent(value),
    };
    if !receipted(&receipt["network_config"], Some(staged.network_config.1))
        || !receipted(
            &receipt["admin_bounds_asset"],
            staged.admin_bounds.map(|(_, receipt)| receipt),
        )
        || !receipted(
            &receipt["media_grant"],
            staged.media_grant.map(|(_, receipt)| receipt),
        )
        || !absent(&receipt["solar_portfolio_result"])
        || !absent(&receipt["design_tags"])
        || !absent(&receipt["applied_grouping"])
        || receipt["mv_models"]
            .as_array()
            .is_some_and(|models| !models.is_empty())
        || receipt["total_input_bytes"].as_u64() != Some(expected_total)
    {
        return Err(CombinedRefusal::invalid(
            "The combined input receipts do not match the staged inputs",
        ));
    }
    let rooms = receipt["transformers"]
        .as_array()
        .filter(|rooms| rooms.len() == staged.rooms.len())
        .ok_or_else(|| CombinedRefusal::invalid("The combined result omits staged rooms"))?;
    for (actual, (_, pin)) in rooms.iter().zip(staged.rooms) {
        let decoded = |key: &str| actual[key].as_u64().unwrap_or(0);
        if actual["transformer"] != pin.transformer.as_str()
            || file_receipt(&actual["source"])
                != Some(FileReceipt {
                    bytes: pin.source_bytes,
                    sha256: pin.source_sha256.clone(),
                })
            || decoded("layers_decoded") == 0
            || decoded("features_decoded") == 0
            || decoded("admin_features_matched") > decoded("admin_features_requested")
        {
            return Err(CombinedRefusal::invalid(format!(
                "The combined result's receipt for {} does not match its staged room",
                pin.transformer
            )));
        }
    }
    let admin_bounds: AdminBoundsEvidence = serde_json::from_value(result["admin_bounds"].clone())
        .map_err(|_| CombinedRefusal::invalid("The combined admin-bounds evidence is malformed"))?;
    if admin_bounds.features_matched > admin_bounds.features_requested
        || (scope.requires_admin_bounds
            && (!admin_bounds.configured || admin_bounds.villages_loaded == 0))
        || (!scope.requires_admin_bounds
            && (admin_bounds.configured
                || admin_bounds.villages_loaded != 0
                || admin_bounds.features_requested != 0))
    {
        return Err(CombinedRefusal::invalid(
            "The combined admin-bounds evidence does not fit the project's country",
        ));
    }
    if result["solar"]["configured"] != false {
        return Err(CombinedRefusal::invalid(
            "The combined result carries Solar evidence no input admitted",
        ));
    }
    let layers_aggregated = result["layers_aggregated"].as_u64().unwrap_or(0);
    let features_aggregated = result["features_aggregated"].as_u64().unwrap_or(0);
    if layers_aggregated == 0 || features_aggregated == 0 {
        return Err(CombinedRefusal::invalid(
            "The combined result aggregated no staged input",
        ));
    }
    if artifacts.len() != scope.outputs.len() {
        return Err(CombinedRefusal::invalid(
            "The combined result does not hold exactly one artifact per planned output",
        ));
    }
    let mut verified = Vec::with_capacity(scope.outputs.len());
    for output in &scope.outputs {
        let artifact = artifacts
            .iter()
            .find(|artifact| artifact["format"] == output.format.as_str())
            .ok_or_else(|| {
                CombinedRefusal::invalid(format!(
                    "The combined result has no {} artifact",
                    output.output_id
                ))
            })?;
        let filename = report_formats::report_filename(IDENTITY, &output.output_id)
            .map_err(CombinedRefusal::invalid)?;
        let size = artifact["bytes"].as_u64().unwrap_or(0);
        let sha256 = artifact["sha256"].as_str().unwrap_or_default();
        if artifact["filename"] != filename.as_str()
            || artifact["path"].as_str().map(Path::new) != Some(&staged.out_dir.join(&filename))
            || size == 0
            || size > MAX_ARTIFACT_BYTES
            || !exact_lower_sha256(sha256)
            || !absent(&artifact["paper_size"])
            || !absent(&artifact["presentation"])
        {
            return Err(CombinedRefusal::invalid(format!(
                "The combined {} artifact declaration is invalid",
                output.output_id
            )));
        }
        verified.push(VerifiedArtifact {
            output_id: output.output_id.clone(),
            format: output.format.clone(),
            filename,
            content_type: output.content_type,
            size_bytes: size,
            sha256: sha256.to_owned(),
            paper_size: None,
            presentation: None,
        });
    }
    Ok(VerifiedCombined {
        artifacts: verified,
        layers_aggregated,
        features_aggregated,
        admin_bounds,
        geometry_changed: result["geometry_changed"] == true,
        engineering_blockers: result["engineering_preflight"]["blockers"]
            .as_u64()
            .unwrap_or(0),
        phase_timings: result["phase_timings"].clone(),
        elapsed_ms: result["elapsed_ms"].as_u64().unwrap_or(0),
    })
}

/// What a combined receipt is written from.
#[derive(Debug, Clone)]
pub struct ReceiptFacts<'a> {
    pub scope: &'a Scope,
    pub pins: &'a [RoomPin],
    pub client_run_id: &'a str,
    pub engine: &'a EngineIdentity,
    pub result: &'a VerifiedCombined,
    pub artifact_dir: &'a str,
    pub now_ms: u64,
    pub transformer_natures: Option<&'a BTreeMap<String, String>>,
    pub media_grant_sha256: Option<&'a str>,
}

/// The receipt one combined run leaves beside its artifacts. Its
/// `project_id`, `transformer`, `transformer_revision`,
/// `input_base_fingerprint` and `room_content_sha256` are the members a
/// publication seal reads.
pub fn run_receipt(facts: &ReceiptFacts<'_>) -> Result<Value, String> {
    if !valid_run_id(facts.client_run_id) {
        return Err("Combined run identity is invalid".into());
    }
    let pins = sorted_pins(facts.pins.to_vec())?;
    if pins != facts.pins {
        return Err("Combined receipt rooms must be in the staged order".into());
    }
    Ok(json!({
        "schema": RECEIPT_SCHEMA,
        "task": TASK,
        "status": "completed",
        "project_id": facts.scope.project,
        "transformer": IDENTITY,
        "transformer_revision": PUBLICATION_REVISION,
        "country": facts.scope.country,
        "client_run_id": facts.client_run_id,
        "produced_at_ms": facts.now_ms,
        "input_base_fingerprint": input_base_fingerprint(facts.scope, facts.pins, facts.transformer_natures)?,
        "room_content_sha256": room_content_sha256(facts.pins)?,
        "sheets_sha256": facts.scope.sheets_sha256,
        "reference_semantic_sha256": facts.scope.reference_semantic_sha256,
        "engine": {
            "engine_version": facts.engine.engine_version,
            "build_manifest_sha256": facts.engine.build_manifest_sha256,
        },
        "formats": facts.scope.formats(),
        "outputs": facts.result.artifacts.iter().map(|artifact| json!({
            "output_id": artifact.output_id,
            "format": artifact.format,
            "filename": artifact.filename,
            "path": format!("{}/{}", facts.artifact_dir, artifact.filename),
            "content_type": artifact.content_type,
            "sha256": artifact.sha256,
            "size_bytes": artifact.size_bytes,
        })).collect::<Vec<_>>(),
        "rooms": facts.pins,
        "layers_aggregated": facts.result.layers_aggregated,
        "features_aggregated": facts.result.features_aggregated,
        "admin_bounds": facts.result.admin_bounds,
        "transformer_natures_sha256": facts.transformer_natures.map(|natures| jcs::sha256(&json!(natures))).transpose()?,
        "media_grant_sha256": facts.media_grant_sha256,
        // The Reporter declares that the cloud-owned optional review layer is
        // not part of a local run; the receipt says so instead of implying it.
        "transformer_reviews": "not_included",
    }))
}

/// The project's combined-report collision policy, read from its sealed
/// project settings as the governed service always read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CollisionPolicy {
    pub allow_with_collisions: bool,
    pub max_collision_rows: u64,
}

fn settings_rows(sheets: &Value) -> Vec<&Value> {
    let top = sheets["project_settings"].as_array();
    let nested = sheets["parameters"]["project_settings"].as_array();
    nested
        .filter(|rows| !rows.is_empty())
        .or(top)
        .map(|rows| rows.iter().collect())
        .unwrap_or_default()
}

fn setting_bool(value: &Value, default: bool) -> bool {
    match value {
        Value::Bool(value) => *value,
        Value::Number(number) => number.as_f64().map_or(default, |number| number != 0.0),
        Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" | "y" | "on" => true,
            "false" | "no" | "0" | "n" | "off" | "" => false,
            _ => default,
        },
        _ => default,
    }
}

fn setting_count(value: &Value) -> Option<u64> {
    let number = match value {
        Value::Number(number) => number.as_i64().or_else(|| {
            number
                .as_f64()
                .filter(|value| value.is_finite() && value.fract() == 0.0)
                .map(|value| value as i64)
        })?,
        Value::String(text) => text.trim().parse::<i64>().ok()?,
        _ => return None,
    };
    Some(number.max(0) as u64)
}

pub fn collision_policy(sheets: &Value) -> CollisionPolicy {
    let mut policy = CollisionPolicy {
        allow_with_collisions: DEFAULT_ALLOW_WITH_COLLISIONS,
        max_collision_rows: DEFAULT_MAX_COLLISION_ROWS,
    };
    for row in settings_rows(sheets) {
        match report_formats::setting_parameter_key(row).as_deref() {
            Some(COLLISION_ALLOW_SETTING) => {
                policy.allow_with_collisions =
                    setting_bool(&row["value"], policy.allow_with_collisions);
            }
            Some(COLLISION_MAX_SETTING) => {
                if let Some(count) = setting_count(&row["value"]) {
                    policy.max_collision_rows = count;
                }
            }
            _ => {}
        }
    }
    policy
}

/// The collision gate over the project's consumed collision result. Nothing
/// is detected here: the count is the stored `collisions` row the edge
/// detection published, read in the shared precedence
/// ([`crate::collisions::project_count`]). A project never checked reads as
/// no collision rows, as the governed service always read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollisionGate {
    pub allowed: bool,
    /// Whether the project's status rows were read at all on this run.
    pub observed: bool,
    /// Whether the project holds a collision result.
    pub checked: bool,
    pub collision_rows: u64,
    pub policy: CollisionPolicy,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<&'static str>,
}

/// `collisions_row` is the project's `collisions` status row when the status
/// read answered, `None` inside `Some` when it answered without one; the
/// outer `None` is a run whose status could not be read.
pub fn collision_gate(sheets: &Value, collisions_row: Option<Option<&Value>>) -> CollisionGate {
    let policy = collision_policy(sheets);
    let row = collisions_row.flatten();
    let rows = row
        .and_then(|row| crate::collisions::project_count(row, &Value::Null))
        .unwrap_or(0);
    let mut gate = CollisionGate {
        allowed: true,
        observed: collisions_row.is_some(),
        checked: row.is_some(),
        collision_rows: rows,
        policy,
        reason: None,
    };
    if rows == 0 {
        return gate;
    }
    if !policy.allow_with_collisions {
        gate.allowed = false;
        gate.reason = Some("combined reports are disabled while collisions exist");
    } else if rows > policy.max_collision_rows {
        gate.allowed = false;
        gate.reason = Some("collision rows exceed the combined report threshold");
    }
    gate
}

/// One transformer whose saved voltage-drop analysis is unavailable to a
/// voltage-drop consumer. A combined run selects no such output, so this is
/// a receipt warning and never a refusal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VoltageDropWarning {
    pub transformer: String,
    /// `missing`, `stale`, `invalid_receipt`, or `saved_json` with an
    /// uncalculated verdict.
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verdict: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// The voltage-drop warnings of the transformers a run combined, from the
/// status rows it read, classified by the one printing/status owner.
pub fn voltage_drop_warnings(
    rows: &BTreeMap<String, Value>,
    transformers: &[String],
) -> Vec<VoltageDropWarning> {
    transformers
        .iter()
        .filter_map(|name| {
            let row = rows.get(name)?.as_object()?;
            let preview = crate::printing::inventory::voltage_drop(row, false);
            let failed = preview.state == "saved_json"
                && matches!(preview.verdict.as_deref(), Some("refused" | "incomplete"));
            (preview.state != "saved_json" || failed).then(|| VoltageDropWarning {
                transformer: name.clone(),
                state: preview.state,
                verdict: preview.verdict,
                reason: preview.reason,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report_export::INPUT_RECEIPT_SCHEMA;

    fn receipt(country: &str, settings: Value) -> InputReceipt {
        let sheets_json = json!({"project_settings": settings}).to_string();
        InputReceipt {
            local_print_recipe: None,
            schema: INPUT_RECEIPT_SCHEMA,
            country: country.into(),
            sheets_sha256: sha256_hex(sheets_json.as_bytes()),
            sheets_json,
            reference_semantic_sha256: "b".repeat(64),
        }
    }

    fn layers(features: usize) -> BTreeMap<String, Value> {
        let feature = json!({"type": "Feature", "geometry": {"type": "Point", "coordinates": [30.0, -2.0]}, "properties": {"id": "p"}});
        serde_json::from_value(json!({
            "lv_poles": {"type": "FeatureCollection", "features": vec![feature; features]},
            "drafting_errors": {"type": "FeatureCollection", "features": [{"type": "Feature", "geometry": null, "properties": {}}]},
        }))
        .unwrap()
    }

    fn staged_room(name: &str) -> StagedRoom {
        stage_room(name, 3, None, &layers(2)).unwrap().unwrap()
    }

    #[test]
    fn the_plan_is_the_shared_data_only_combined_plan_and_names_no_print() {
        let scope = scope(
            "project-a",
            &receipt(
                "Rwanda",
                json!([{"parameter": "report_formats", "value": "pdf_a3, gpkg, voltage_drop"}]),
            ),
        )
        .unwrap();
        assert_eq!(scope.formats(), ["shp", "xlsx", "geojsonseq", "gpkg"]);
        assert!(scope.requires_admin_bounds);
        let plain = super::scope("project-a", &receipt("Burundi", json!([]))).unwrap();
        assert_eq!(plain.formats(), ["shp", "xlsx", "geojsonseq"]);
        assert!(!plain.requires_admin_bounds);
        assert!(super::scope("bad project", &receipt("Burundi", json!([]))).is_err());
    }

    #[test]
    fn a_room_stages_only_report_layers_and_pins_its_saved_digest() {
        let room = layers(2);
        let digest = jcs::layers_content_digest(&room).unwrap();
        let staged = stage_room("tx_a", 4, Some(&digest), &room)
            .unwrap()
            .unwrap();
        let document: Value = serde_json::from_slice(&staged.document).unwrap();
        assert_eq!(
            document["layers"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            ["lv_poles"]
        );
        assert_eq!(staged.pin.content_digest, digest);
        assert_eq!(staged.pin.features, 2);
        assert_eq!(staged.pin.source_sha256, sha256_hex(&staged.document));
        assert!(stage_room("tx_a", 4, Some(&"0".repeat(64)), &room)
            .unwrap_err()
            .contains("saved content digest"));
        assert!(stage_room("tx_a", 0, None, &room).is_err());
        for reserved in ["combined_transformer", "collisions", "mv_data"] {
            assert!(stage_room(reserved, 1, None, &room).is_err(), "{reserved}");
        }
        assert_eq!(stage_room("tx_a", 4, None, &layers(0)).unwrap(), None);
    }

    #[test]
    fn pins_sort_as_the_reporter_sorts_and_refuse_repeats_and_empty_runs() {
        let pins = vec![staged_room("tx_b").pin, staged_room("tx_a").pin];
        let sorted = sorted_pins(pins.clone()).unwrap();
        assert_eq!(sorted[0].transformer, "tx_a");
        assert!(sorted_pins(Vec::new()).is_err());
        assert!(sorted_pins(vec![pins[0].clone(), pins[0].clone()]).is_err());
        let first = input_base_fingerprint(
            &super::scope("p", &receipt("Burundi", json!([]))).unwrap(),
            &sorted,
            None,
        )
        .unwrap();
        let mut changed = sorted.clone();
        changed[0].revision += 1;
        let second = input_base_fingerprint(
            &super::scope("p", &receipt("Burundi", json!([]))).unwrap(),
            &changed,
            None,
        )
        .unwrap();
        assert_ne!(first, second);
        assert_ne!(
            room_content_sha256(&sorted).unwrap(),
            room_content_sha256(&changed).unwrap()
        );
    }

    struct Fixture {
        scope: Scope,
        rooms: Vec<(PathBuf, RoomPin)>,
        config: FileReceipt,
        reference: FileReceipt,
        out: PathBuf,
        engine: EngineIdentity,
    }

    impl Fixture {
        fn new(country: &str) -> Self {
            let scope = scope("project-a", &receipt(country, json!([]))).unwrap();
            let rooms = sorted_pins(vec![staged_room("tx_b").pin, staged_room("tx_a").pin])
                .unwrap()
                .into_iter()
                .enumerate()
                .map(|(index, pin)| (PathBuf::from(format!("/stage/rooms/{index}.json")), pin))
                .collect();
            Self {
                scope,
                rooms,
                config: FileReceipt::of(b"{}"),
                reference: FileReceipt {
                    bytes: 10,
                    sha256: "c".repeat(64),
                },
                out: PathBuf::from("/stage/out"),
                engine: EngineIdentity {
                    engine_version: format!("ds-network-reporter@0.1.0+{}", "e".repeat(40)),
                    build_manifest_sha256: "d".repeat(64),
                    package_version: "0.1.0".into(),
                    source_sha: "e".repeat(40),
                    profile: "release".into(),
                },
            }
        }
        fn staged(&self) -> Staged<'_> {
            Staged {
                network_config: (Path::new("/stage/config.json"), &self.config),
                rooms: &self.rooms,
                out_dir: &self.out,
                admin_bounds: self
                    .scope
                    .requires_admin_bounds
                    .then(|| (Path::new("/shared/asset.dsab"), &self.reference)),
                media_grant: None,
                transformer_natures: None,
            }
        }
        fn result(&self) -> Value {
            let rwanda = self.scope.requires_admin_bounds;
            let total = self.config.bytes
                + if rwanda { self.reference.bytes } else { 0 }
                + self
                    .rooms
                    .iter()
                    .map(|(_, pin)| pin.source_bytes)
                    .sum::<u64>();
            json!({
                "engineering_preflight": {"blockers": 2},
                "schema_version": 4, "task": TASK, "status": "completed",
                "engine": {"schema": "ds.engine-build/v1", "service": "ds-network-reporter", "binary": "ds-report",
                    "package_version": "0.1.0", "source_sha": "e".repeat(40), "engine_version": self.engine.engine_version,
                    "cargo_lock_sha256": "f".repeat(64), "features": [], "rustc": "rustc", "target": "x86_64", "profile": "release",
                    "schemas": ["s"], "build_manifest_sha256": "d".repeat(64)},
                "prefix": IDENTITY, "country": self.scope.country,
                "upstream_admission_requirements": [{"code": REVIEWS_REQUIREMENT, "signed_work_grant_field": "transformer_reviews_absent", "required_value": true, "must_bind_input_fingerprint": true, "forwarded_to_task": false, "message": "m"}],
                "input_receipt": {
                    "network_config": self.config,
                    "admin_bounds_asset": if rwanda { json!(self.reference) } else { Value::Null },
                    "transformers": self.rooms.iter().map(|(_, pin)| json!({"transformer": pin.transformer, "source": {"bytes": pin.source_bytes, "sha256": pin.source_sha256}, "layers_decoded": 1, "features_decoded": 2, "geometry_changed": false, "admin_features_requested": 2, "admin_features_matched": 2})).collect::<Vec<_>>(),
                    "total_input_bytes": total,
                },
                "layers_aggregated": 2, "features_aggregated": 4, "geometry_changed": false,
                "admin_bounds": {"configured": rwanda, "villages_loaded": if rwanda { 5 } else { 0 }, "features_matched": if rwanda { 4 } else { 0 }, "features_requested": if rwanda { 4 } else { 0 }},
                "solar": {"configured": false},
                "formats_requested": self.scope.formats(),
                "artifacts": self.scope.outputs.iter().map(|output| {
                    let filename = report_formats::report_filename(IDENTITY, &output.output_id).unwrap();
                    json!({"format": output.format, "filename": filename, "path": self.out.join(&filename), "bytes": 9, "sha256": "a".repeat(64)})
                }).collect::<Vec<_>>(),
                "tiles": {"status": "ready_for_background"},
                "blockers": [],
                "phase_timings": {"input_preparation_ms": 1, "format_compute_ms": 2, "artifact_finalize_ms": 3},
                "elapsed_ms": 7,
            })
        }
        fn verify(&self, result: &Value) -> Result<VerifiedCombined, CombinedRefusal> {
            verify_result(
                &serde_json::to_vec(result).unwrap(),
                &self.scope,
                &self.staged(),
                &self.engine,
            )
        }
    }

    #[test]
    fn the_request_names_every_staged_input_and_only_this_plan() {
        let fixture = Fixture::new("Rwanda");
        let request = engine_request(&fixture.scope, &fixture.staged()).unwrap();
        assert_eq!(request["formats"], json!(["shp", "xlsx", "geojsonseq"]));
        assert_eq!(request["transformers"][0]["transformer"], "tx_a");
        assert_eq!(request["admin_bounds_sha256"], "c".repeat(64));
        assert!(request.get("media_grant").is_none());
        let media = media_request(&fixture.scope, &fixture.staged()).unwrap();
        assert_eq!(media["transformers"], request["transformers"]);
        let mut unsorted = fixture.rooms.clone();
        unsorted.reverse();
        let staged = Staged {
            rooms: &unsorted,
            ..fixture.staged()
        };
        assert!(engine_request(&fixture.scope, &staged).is_err());
        let missing_reference = Staged {
            admin_bounds: None,
            ..fixture.staged()
        };
        assert!(engine_request(&fixture.scope, &missing_reference).is_err());
    }

    #[test]
    fn a_complete_answer_verifies_every_planned_artifact_and_receipt() {
        for country in ["Rwanda", "Burundi"] {
            let fixture = Fixture::new(country);
            let verified = fixture.verify(&fixture.result()).unwrap();
            assert_eq!(
                verified
                    .artifacts
                    .iter()
                    .map(|a| a.filename.as_str())
                    .collect::<Vec<_>>(),
                [
                    "combined_transformer.shp.zip",
                    "combined_transformer.xlsx",
                    "combined_transformer_geojsonl.zip"
                ]
            );
            assert_eq!(verified.engineering_blockers, 2);
            let receipt = run_receipt(&ReceiptFacts {
                scope: &fixture.scope,
                pins: &fixture
                    .rooms
                    .iter()
                    .map(|(_, pin)| pin.clone())
                    .collect::<Vec<_>>(),
                client_run_id: &format!("report-{}", "1".repeat(32)),
                engine: &fixture.engine,
                result: &verified,
                artifact_dir: IDENTITY,
                now_ms: 1,
                transformer_natures: None,
                media_grant_sha256: None,
            })
            .unwrap();
            assert_eq!(receipt["transformer"], IDENTITY);
            assert_eq!(receipt["transformer_revision"], 0);
            assert_eq!(receipt["project_id"], "project-a");
            assert!(exact_lower_sha256(
                receipt["input_base_fingerprint"].as_str().unwrap()
            ));
            assert_eq!(receipt["transformer_reviews"], "not_included");
        }
    }

    #[test]
    fn partial_crossed_or_unplanned_answers_never_become_a_deliverable() {
        let fixture = Fixture::new("Rwanda");
        let mut partial = fixture.result();
        partial["status"] = json!("partial");
        partial["artifacts"].as_array_mut().unwrap().pop();
        partial["blockers"] =
            json!([{"code": "artifact_stage_failed", "message": "disk", "format": "geojsonseq"}]);
        assert!(matches!(
            fixture.verify(&partial),
            Err(CombinedRefusal::Incomplete { artifacts: 2, .. })
        ));
        for corrupt in [
            ("country", json!("Burundi")),
            ("prefix", json!("tx_a")),
            ("formats_requested", json!(["shp", "xlsx"])),
            ("upstream_admission_requirements", json!([])),
            ("solar", json!({"configured": true})),
            ("layers_aggregated", json!(0)),
        ] {
            let mut result = fixture.result();
            result[corrupt.0] = corrupt.1.clone();
            assert!(
                matches!(
                    fixture.verify(&result),
                    Err(CombinedRefusal::Invalid { .. })
                ),
                "{}",
                corrupt.0
            );
        }
        let mut crossed_room = fixture.result();
        crossed_room["input_receipt"]["transformers"][0]["source"]["sha256"] =
            json!("0".repeat(64));
        assert!(fixture.verify(&crossed_room).is_err());
        let mut crossed_total = fixture.result();
        crossed_total["input_receipt"]["total_input_bytes"] = json!(1);
        assert!(fixture.verify(&crossed_total).is_err());
        let mut moved = fixture.result();
        moved["artifacts"][0]["path"] = json!("/elsewhere/combined_transformer.shp.zip");
        assert!(fixture.verify(&moved).is_err());
        let mut other_engine = fixture.result();
        other_engine["engine"]["build_manifest_sha256"] = json!("9".repeat(64));
        assert!(fixture.verify(&other_engine).is_err());
        let mut print = fixture.result();
        print["artifacts"][0]["paper_size"] = json!("A3");
        assert!(fixture.verify(&print).is_err());
    }

    #[test]
    fn a_refused_workbook_names_the_engineering_facts_per_transformer() {
        let fixture = Fixture::new("Burundi");
        let mut partial = fixture.result();
        partial["status"] = json!("partial");
        partial["artifacts"]
            .as_array_mut()
            .unwrap()
            .retain(|artifact| artifact["format"] != "xlsx");
        partial["blockers"] = json!([{"code": "report_engineering_facts_required", "message": "4 missing facts", "format": "xlsx"}]);
        let finding = |transformer: &str, layer: &str, field: &str| json!({"code": "engineering_attribute_missing", "layer": layer, "field": field, "transformer": transformer, "feature_id": "n1", "feature_index": 0, "recorded": null});
        // A project-wide preflight holds more source rows than one
        // transformer's bounds admit; it is still folded.
        let source_layers: Vec<Value> = (0..40)
            .map(|index| json!({"layer": format!("lv_poles_{index}"), "features": 1, "held_features_sha256": "a".repeat(64)}))
            .collect();
        partial["engineering_preflight"] = json!({
            "schema": "ds.report.engineering-preflight/v1", "source_layers": source_layers, "field_lineage": [],
            "blockers": 4, "finding_counts": {"engineering_attribute_missing": 4}, "coverage_counts": {},
            "findings": [
                finding("TX_B", "lv_poles", "struct_type"),
                finding("TX_B", "lv_poles", "stay"),
                finding("TX_A", "tapping_poles", "assembly_type"),
            ],
            "omitted_details": 1, "quantity_rows_total": 0, "quantity_lineage": [], "omitted_quantity_details": 0,
        });
        let Err(CombinedRefusal::Incomplete {
            engineering: Some(gaps),
            artifacts,
            ..
        }) = fixture.verify(&partial)
        else {
            panic!("a refused workbook names its engineering gaps");
        };
        assert_eq!(artifacts, 2);
        assert_eq!((gaps.blockers, gaps.omitted_details), (4, 1));
        assert_eq!(
            gaps.transformers,
            [
                EngineeringGap {
                    transformer: "TX_A".into(),
                    findings: 1,
                    fields: vec!["tapping_poles.assembly_type".into()],
                },
                EngineeringGap {
                    transformer: "TX_B".into(),
                    findings: 2,
                    fields: vec!["lv_poles.stay".into(), "lv_poles.struct_type".into()],
                },
            ]
        );
        // A preflight outside the closed document names nothing rather than guessing.
        partial["engineering_preflight"]["blockers"] = json!(5);
        assert!(matches!(
            fixture.verify(&partial),
            Err(CombinedRefusal::Incomplete {
                engineering: None,
                ..
            })
        ));
    }

    #[test]
    fn media_scope_answers_the_staged_rooms_and_names_foreign_projects() {
        let document = |value: Value| serde_json::to_vec(&value).unwrap();
        let answer = json!({"schema_version": 2, "task": MEDIA_TASK, "status": "completed", "reference_count": 3, "source_projects": ["other", "project-a"], "transformers_scanned": 2, "features_scanned": 9, "total_input_bytes": 1, "elapsed_ms": 1});
        let scope = verify_media_scope(&document(answer.clone()), 2).unwrap();
        assert_eq!(scope.foreign_projects("project-a"), ["other"]);
        let mut wrong = answer.clone();
        wrong["transformers_scanned"] = json!(1);
        assert!(verify_media_scope(&document(wrong), 2).is_err());
        let mut unsorted = answer.clone();
        unsorted["source_projects"] = json!(["project-a", "other"]);
        assert!(verify_media_scope(&document(unsorted), 2).is_err());
        let mut inconsistent = answer;
        inconsistent["reference_count"] = json!(0);
        assert!(verify_media_scope(&document(inconsistent), 2).is_err());
    }

    #[test]
    fn the_collision_gate_consumes_the_stored_result_under_the_project_policy() {
        let sheets = |rows: Value| json!({"project_settings": rows});
        let row = |count: u64| json!({"layers": {"collisions": count}});
        let default = collision_gate(&sheets(json!([])), Some(Some(&row(10))));
        assert!(default.allowed && default.checked && default.observed);
        let blocked = collision_gate(&sheets(json!([])), Some(Some(&row(11))));
        assert!(!blocked.allowed);
        assert_eq!(
            blocked.reason,
            Some("collision rows exceed the combined report threshold")
        );
        let strict =
            sheets(json!([{"parameter": "Allow_Combined_Reports_With_Collisions", "value": "no"}]));
        assert!(!collision_gate(&strict, Some(Some(&row(1)))).allowed);
        let tolerant = sheets(json!([{"parameter": COLLISION_MAX_SETTING, "value": "50"}]));
        assert!(collision_gate(&tolerant, Some(Some(&row(40)))).allowed);
        let zero = json!({"layers": {}, "report_metadata": {"outcomes": {"collision_detection": {"details": {"pairs": 0}}}}});
        assert!(collision_gate(&strict, Some(Some(&zero))).allowed);
        let never = collision_gate(&strict, Some(None));
        assert!(never.allowed && !never.checked && never.observed);
        let offline = collision_gate(&strict, None);
        assert!(offline.allowed && !offline.observed);
    }

    #[test]
    fn voltage_drop_states_become_warnings_never_refusals() {
        let mut rows = BTreeMap::new();
        rows.insert("tx_missing".to_string(), json!({"name": "tx_missing"}));
        rows.insert(
            "tx_stale".to_string(),
            json!({"voltage_drop_metadata": {"state": "stale"}}),
        );
        let warnings = voltage_drop_warnings(
            &rows,
            &["tx_missing".into(), "tx_stale".into(), "tx_unread".into()],
        );
        assert_eq!(
            warnings
                .iter()
                .map(|w| (w.transformer.as_str(), w.state.as_str()))
                .collect::<Vec<_>>(),
            [("tx_missing", "missing"), ("tx_stale", "stale")]
        );
    }
}
