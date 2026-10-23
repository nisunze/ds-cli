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
/// The combined result revision this module was written against. A reply's
/// revision is provenance, never a gate: the family (task and prefix) and the
/// members this module reads decide compatibility.
pub const RESULT_SCHEMA_VERSION: u64 = 4;
/// The media-scope result revision this module was written against; provenance only.
pub const MEDIA_SCOPE_SCHEMA_VERSION: u64 = 2;

/// A reply's required revision member: present as a positive integer. Any
/// same-family revision is admitted; its members are checked by their reader.
fn reply_revision(reply: &Value) -> Option<u64> {
    reply["schema_version"]
        .as_u64()
        .filter(|revision| *revision > 0)
}
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
    if reply_revision(&value).is_none() {
        return Err("The combined media scope omits its schema_version member".into());
    }
    if value["task"] != MEDIA_TASK {
        return Err("The combined media scope belongs to another Reporter task family".into());
    }
    if value["status"] != "completed"
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
    /// The facts the workbook itemizes as Dirty Categories; `None` from a
    /// Reporter that predates them.
    pub dirty_facts: Option<super::engineering::DirtyFactsSummary>,
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
    // The task and prefix name the family; a revision of it is provenance.
    // Every member this reader uses is checked below, and added ones are ignored.
    if reply_revision(&result).is_none() {
        return Err(CombinedRefusal::invalid(
            "The combined result omits its schema_version member",
        ));
    }
    if result["task"] != TASK || result["prefix"] != IDENTITY {
        return Err(CombinedRefusal::invalid(
            "The combined result belongs to another Reporter task family (task or prefix)",
        ));
    }
    if result["country"] != scope.country.as_str() {
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
        dirty_facts: dirty_facts(&result)?,
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
    let mut receipt = json!({
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
        // Missing or invalid engineering facts the workbook itemizes on its
        // Dirty Categories sheet: counted findings of a complete set.
        // An older Reporter counted them as `blockers`; a current one refuses
        // on any blocker, so the sum is exact for both.
        "engineering_findings": facts.result.engineering_blockers
            + facts.result.dirty_facts.as_ref().map_or(0, |facts| facts.features as u64),
        // The Reporter declares that the cloud-owned optional review layer is
        // not part of a local run; the receipt says so instead of implying it.
        "transformer_reviews": "not_included",
    });
    // Only a Reporter that itemizes them adds the member.
    if let Some(dirty) = &facts.result.dirty_facts {
        receipt["dirty_facts"] = json!(dirty);
    }
    Ok(receipt)
}

/// The facts the result itemizes as Dirty Categories, summarized, or `None`
/// from a Reporter that predates them. A malformed member refuses: Core reads it.
fn dirty_facts(
    result: &Value,
) -> Result<Option<super::engineering::DirtyFactsSummary>, CombinedRefusal> {
    let Some(value) = result["engineering_preflight"].get("dirty_facts") else {
        return Ok(None);
    };
    let facts: super::engineering::DirtyFacts = serde_json::from_value(value.clone())
        .map_err(|_| CombinedRefusal::invalid("The combined result dirty facts are malformed"))?;
    if !facts.valid() {
        return Err(CombinedRefusal::invalid(
            "The combined result dirty facts are invalid or exceed their bounds",
        ));
    }
    Ok(Some(facts.summary()))
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
#[path = "combined_tests.rs"]
mod tests;
