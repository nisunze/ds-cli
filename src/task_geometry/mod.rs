//! Task geometry from DS objects — typed references become a task's one
//! geometry and the links that trace it.
//!
//! `docs/contracts/task-geometry-from-objects.md` is the contract. The owner's
//! case: a consultant's comment names DS Grid structures 74, 76 and 77 in a
//! swamp, and the task that answers it has to say WHERE. The intelligence
//! (a person, or an agent reading the comment) supplies the structure numbers
//! it read as a typed reference; this module resolves them deterministically
//! and shapes the geometry; the CLI shows the proposal; a person confirms.
//! No prose is parsed anywhere in the stack.
//!
//! **This module never opens a `.dsgrid`.** Reading a model is engine work
//! (`ds-network`), which the kernel does not link
//! (`docs/contracts/ds-kernel-network-boundary.md`). A host that does link it
//! — `ds-cli-dsgrid` today, the desktop's open session later — produces an
//! [`ObjectIndex`]: every structure and alignment of one model in EPSG:4326
//! with the model's identity. What a reference MEANS against that index —
//! which object `74` is, whether it is ambiguous, what shape three poles
//! become, every bound and every refusal — is decided here, once, for the
//! CLI, MCP, the Server and the desktop alike.
//!
//! Pure by the crate's contract: no clock, no IO, no network.

pub mod grammar;
pub mod shape;

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub use grammar::{Object, Reference, Source};
use shape::{Position, Vertex};

/// The request/reply schema of this module.
pub const SCHEMA: &str = "ds.task-geometry/v1";

/// A whole model's index is a few hundred kilobytes; a request past this is
/// something other than a proposal.
const MAX_INPUT_BYTES: usize = 32 * 1024 * 1024;

/// The most objects one proposal resolves. A task carries at most 128 links
/// (ds-brain's bound) and every object becomes one; this leaves room for the
/// links a task already has.
pub const MAX_OBJECTS: usize = 100;
/// ds-brain's bound on a task's links.
pub const MAX_LINKS: usize = 128;
/// ds-brain's bound on a geometry's positions.
pub const MAX_POSITIONS: usize = 2_000;
/// The buffer a `buffered_hull` takes when the caller names none.
pub const DEFAULT_BUFFER_M: f64 = 25.0;
/// The smallest and largest buffer a proposal admits. Under a metre is a
/// line pretending to be an area; over half a kilometre is a corridor study,
/// not a task's where.
pub const MIN_BUFFER_M: f64 = 1.0;
pub const MAX_BUFFER_M: f64 = 500.0;

// ── Refusals ────────────────────────────────────────────────────────────

pub const REFERENCE_INVALID: &str = "reference_invalid";
pub const MODEL_UNKNOWN: &str = "model_unknown";
pub const OBJECT_UNRESOLVED: &str = "object_unresolved";
pub const OBJECT_AMBIGUOUS: &str = "object_ambiguous";
pub const REFERENCES_INCOMPATIBLE: &str = "references_incompatible";
pub const TOO_MANY_OBJECTS: &str = "too_many_objects";
pub const GEOMETRY_TOO_LARGE: &str = "geometry_too_large";
pub const BUFFER_OUT_OF_RANGE: &str = "buffer_out_of_range";
pub const LINKS_BOUND_EXCEEDED: &str = "links_bound_exceeded";

/// The closed vocabulary, for a host that documents every refusal it can
/// relay.
pub const REFUSALS: &[&str] = &[
    REFERENCE_INVALID,
    MODEL_UNKNOWN,
    OBJECT_UNRESOLVED,
    OBJECT_AMBIGUOUS,
    REFERENCES_INCOMPATIBLE,
    TOO_MANY_OBJECTS,
    GEOMETRY_TOO_LARGE,
    BUFFER_OUT_OF_RANGE,
    LINKS_BOUND_EXCEEDED,
];

/// Why a proposal was not made. `detail` names the ids, numbers and bounds
/// the rule is about, so a caller can act without parsing the sentence.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Refusal {
    pub code: &'static str,
    pub message: String,
    pub detail: Value,
}

impl Refusal {
    pub fn new(code: &'static str, message: impl Into<String>, detail: Value) -> Self {
        Self {
            code,
            message: message.into(),
            detail,
        }
    }
}

// ── The object index a host supplies ─────────────────────────────────────

/// One structure of a model, as the engine placed it, in EPSG:4326.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexedStructure {
    /// The model's stable `StructureId`.
    pub id: String,
    /// The engineering number a consultant writes, when authored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    /// The alignment that owns the structure's primary placement, when routed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alignment: Option<String>,
    /// Chainage on that alignment, metres, when routed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station_m: Option<f64>,
    /// `[lon, lat]`.
    pub position: Position,
}

/// One alignment of a model: its route polyline with the engine's chainage
/// at every vertex, in EPSG:4326.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexedAlignment {
    pub id: String,
    pub label: String,
    /// `[lon, lat, station_m]` per vertex, in route order.
    pub vertices: Vec<[f64; 3]>,
}

/// Everything a host read from one model that this module needs, keyed by
/// the reference source it answers (`local-<id>` or `package`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObjectIndex {
    /// `local-<id>` or `package`.
    pub r#ref: String,
    /// The native manifest identity (`manifest.model.model_id`), stable across
    /// revisions and pinned by the project catalogue as `manifest_model_id`.
    pub model_id: String,
    pub model_revision: u64,
    /// `manifest.model.snapshot_fingerprint`.
    pub fingerprint: String,
    /// The CRS the package declares; positions here are already EPSG:4326.
    pub crs: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default)]
    pub structures: Vec<IndexedStructure>,
    #[serde(default)]
    pub alignments: Vec<IndexedAlignment>,
}

// ── The proposal ─────────────────────────────────────────────────────────

/// One `ds_object` link the task will carry, before ds-brain stamps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub kind: &'static str,
    pub project_id: String,
    pub object_type: &'static str,
    pub entity_id: String,
    pub object_revision: String,
    pub label: String,
}

/// One resolved object, as the receipt shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedObject {
    pub kind: &'static str,
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<String>,
    pub label: String,
    pub source: String,
    pub model_id: String,
    pub model_revision: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alignment: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub station_m: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<Position>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub positions: Option<usize>,
}

/// The shaping rule a proposal applied.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Rule {
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub buffer_m: Option<f64>,
    pub positions: usize,
    pub objects: usize,
}

/// Which model a proposal read, as the receipt names it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ResolvedSource {
    pub r#ref: String,
    pub model_id: String,
    pub model_revision: u64,
    pub fingerprint: String,
    pub crs: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
}

/// What a dry run answers and a confirmed write writes.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Proposal {
    pub schema: &'static str,
    pub geometry: Value,
    pub rule: Rule,
    pub objects: Vec<ResolvedObject>,
    pub links: Vec<Link>,
    pub sources: Vec<ResolvedSource>,
}

/// The typed request behind `propose`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposeRequest {
    #[serde(default)]
    pub schema: String,
    #[serde(default)]
    pub action: String,
    /// The owning project every link names.
    pub project: String,
    pub references: Vec<String>,
    #[serde(default)]
    pub buffer_m: Option<f64>,
    /// The links the task already carries, so the bound is checked here and
    /// a link that already exists is not proposed twice.
    #[serde(default)]
    pub existing_links: Vec<Value>,
    pub index: IndexInput,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IndexInput {
    pub sources: Vec<ObjectIndex>,
}

// ── Resolution ───────────────────────────────────────────────────────────

enum Resolved<'a> {
    Structure(&'a ObjectIndex, &'a IndexedStructure),
    Alignment(&'a ObjectIndex, &'a IndexedAlignment),
    /// The alignment and the two bounding structures, station-ordered later.
    Range(
        &'a ObjectIndex,
        &'a IndexedAlignment,
        &'a IndexedStructure,
        &'a IndexedStructure,
    ),
}

fn find_source<'a>(
    reference: &Reference,
    sources: &'a [ObjectIndex],
) -> Result<&'a ObjectIndex, Refusal> {
    let key = reference.source.key();
    sources
        .iter()
        .find(|source| source.r#ref == key)
        .ok_or_else(|| {
            Refusal::new(
                MODEL_UNKNOWN,
                match &reference.source {
                    Source::Local { id } => format!(
                        "no DS Grid working copy `{id}` is held on this machine for this lane and account"
                    ),
                    Source::Package => {
                        "the reference names `package` but no --package <path> was given".to_owned()
                    }
                },
                json!({ "reference": reference.reference, "source": key }),
            )
        })
}

/// A structure by number first, then by id. Two structures under one number
/// is a fact about the model, not a guess this module makes.
fn find_structure<'a>(
    reference: &Reference,
    source: &'a ObjectIndex,
    selector: &str,
    on_alignment: Option<&str>,
) -> Result<&'a IndexedStructure, Refusal> {
    let placed = |structure: &&IndexedStructure| match on_alignment {
        Some(alignment) => structure.alignment.as_deref() == Some(alignment),
        None => true,
    };
    let by_number: Vec<&IndexedStructure> = source
        .structures
        .iter()
        .filter(placed)
        .filter(|structure| structure.number.as_deref() == Some(selector))
        .collect();
    match by_number.len() {
        1 => return Ok(by_number[0]),
        0 => {}
        _ => {
            return Err(Refusal::new(
                OBJECT_AMBIGUOUS,
                format!(
                    "structure number `{selector}` names {} structures in `{}`; use the structure id",
                    by_number.len(),
                    source.r#ref
                ),
                json!({
                    "reference": reference.reference,
                    "selector": selector,
                    "candidates": by_number.iter().map(|s| json!({
                        "id": s.id, "alignment": s.alignment, "station_m": s.station_m,
                    })).collect::<Vec<_>>(),
                }),
            ));
        }
    }
    source
        .structures
        .iter()
        .filter(placed)
        .find(|structure| structure.id == selector)
        .ok_or_else(|| {
            Refusal::new(
                OBJECT_UNRESOLVED,
                match on_alignment {
                    Some(alignment) => format!(
                        "no structure numbered or identified `{selector}` is placed on alignment `{alignment}` of `{}`",
                        source.r#ref
                    ),
                    None => format!(
                        "no structure numbered or identified `{selector}` exists in `{}` (model {} rev {})",
                        source.r#ref, source.model_id, source.model_revision
                    ),
                },
                json!({
                    "reference": reference.reference,
                    "unresolved": [selector],
                    "source": source.r#ref,
                }),
            )
        })
}

fn find_alignment<'a>(
    reference: &Reference,
    source: &'a ObjectIndex,
    selector: &str,
) -> Result<&'a IndexedAlignment, Refusal> {
    if let Some(alignment) = source.alignments.iter().find(|a| a.id == selector) {
        return Ok(alignment);
    }
    let by_label: Vec<&IndexedAlignment> = source
        .alignments
        .iter()
        .filter(|a| a.label == selector)
        .collect();
    match by_label.len() {
        1 => Ok(by_label[0]),
        0 => Err(Refusal::new(
            OBJECT_UNRESOLVED,
            format!(
                "no alignment identified or labelled `{selector}` exists in `{}`",
                source.r#ref
            ),
            json!({ "reference": reference.reference, "unresolved": [selector], "source": source.r#ref }),
        )),
        _ => Err(Refusal::new(
            OBJECT_AMBIGUOUS,
            format!(
                "alignment label `{selector}` names {} alignments in `{}`; use the alignment id",
                by_label.len(),
                source.r#ref
            ),
            json!({
                "reference": reference.reference,
                "selector": selector,
                "candidates": by_label.iter().map(|a| json!({ "id": a.id })).collect::<Vec<_>>(),
            }),
        )),
    }
}

/// Every unresolved selector of one structure reference, named together:
/// an agent that read three numbers wrong fixes them in one round, not three.
fn resolve_structures<'a>(
    reference: &Reference,
    source: &'a ObjectIndex,
    selectors: &[String],
) -> Result<Vec<&'a IndexedStructure>, Refusal> {
    let mut found = Vec::with_capacity(selectors.len());
    let mut unresolved = Vec::new();
    for selector in selectors {
        match find_structure(reference, source, selector, None) {
            Ok(structure) => found.push(structure),
            Err(refusal) if refusal.code == OBJECT_UNRESOLVED => unresolved.push(selector.clone()),
            Err(refusal) => return Err(refusal),
        }
    }
    if !unresolved.is_empty() {
        return Err(Refusal::new(
            OBJECT_UNRESOLVED,
            format!(
                "{} of {} structures named by `{}` do not exist in `{}` (model {} rev {}): {}",
                unresolved.len(),
                selectors.len(),
                reference.reference,
                source.r#ref,
                source.model_id,
                source.model_revision,
                unresolved.join(", ")
            ),
            json!({ "reference": reference.reference, "unresolved": unresolved, "source": source.r#ref }),
        ));
    }
    Ok(found)
}

fn resolve<'a>(
    references: &[Reference],
    sources: &'a [ObjectIndex],
) -> Result<Vec<Resolved<'a>>, Refusal> {
    let mut out = Vec::new();
    for reference in references {
        let source = find_source(reference, sources)?;
        match &reference.object {
            Object::Structure { selectors } => {
                for structure in resolve_structures(reference, source, selectors)? {
                    out.push(Resolved::Structure(source, structure));
                }
            }
            Object::Alignment { alignment } => {
                let alignment = find_alignment(reference, source, alignment)?;
                out.push(Resolved::Alignment(source, alignment));
            }
            Object::AlignmentRange {
                alignment,
                from,
                to,
            } => {
                let alignment = find_alignment(reference, source, alignment)?;
                let a = find_structure(reference, source, from, Some(&alignment.id))?;
                let b = find_structure(reference, source, to, Some(&alignment.id))?;
                out.push(Resolved::Range(source, alignment, a, b));
            }
        }
    }
    Ok(out)
}

// ── Links ────────────────────────────────────────────────────────────────

pub const OBJECT_TYPE_STRUCTURE: &str = "dsgrid_structure";
pub const OBJECT_TYPE_ALIGNMENT: &str = "dsgrid_alignment";

/// ds-brain's bound on a link's `entity_id`.
const MAX_ENTITY_ID_BYTES: usize = 256;

fn object_revision(source: &ObjectIndex) -> String {
    format!("dsgrid:r{}:{}", source.model_revision, source.fingerprint)
}

fn structure_label(structure: &IndexedStructure) -> String {
    match &structure.number {
        Some(number) => format!("Structure {number}"),
        None => format!("Structure {}", structure.id),
    }
}

fn link(
    project: &str,
    source: &ObjectIndex,
    object_type: &'static str,
    entity: &str,
    label: &str,
) -> Result<Link, Refusal> {
    let entity_id = format!("{}:{}", source.model_id, entity);
    if entity_id.len() > MAX_ENTITY_ID_BYTES {
        return Err(Refusal::new(
            REFERENCE_INVALID,
            format!(
                "the link identity `{entity_id}` is longer than the {MAX_ENTITY_ID_BYTES} bytes a task link holds"
            ),
            json!({ "part": "entity_id", "entity_id": entity_id }),
        ));
    }
    Ok(Link {
        kind: "ds_object",
        project_id: project.to_owned(),
        object_type,
        entity_id,
        object_revision: object_revision(source),
        label: label.chars().take(200).collect(),
    })
}

fn link_key(kind: &str, object_type: &str, entity_id: &str) -> String {
    format!("{kind}\u{0}{object_type}\u{0}{entity_id}")
}

// ── Shaping ──────────────────────────────────────────────────────────────

fn positions_json(positions: &[Position]) -> Value {
    Value::Array(
        positions
            .iter()
            .map(|p| json!([shape::round(p[0]), shape::round(p[1])]))
            .collect(),
    )
}

fn resolved_structure(source: &ObjectIndex, structure: &IndexedStructure) -> ResolvedObject {
    ResolvedObject {
        kind: OBJECT_TYPE_STRUCTURE,
        id: structure.id.clone(),
        number: structure.number.clone(),
        label: structure_label(structure),
        source: source.r#ref.clone(),
        model_id: source.model_id.clone(),
        model_revision: source.model_revision,
        alignment: structure.alignment.clone(),
        station_m: structure.station_m,
        position: Some(structure.position),
        positions: None,
    }
}

fn resolved_alignment(source: &ObjectIndex, alignment: &IndexedAlignment) -> ResolvedObject {
    ResolvedObject {
        kind: OBJECT_TYPE_ALIGNMENT,
        id: alignment.id.clone(),
        number: None,
        label: alignment.label.clone(),
        source: source.r#ref.clone(),
        model_id: source.model_id.clone(),
        model_revision: source.model_revision,
        alignment: None,
        station_m: None,
        position: None,
        positions: Some(alignment.vertices.len()),
    }
}

fn buffer(requested: Option<f64>) -> Result<f64, Refusal> {
    let buffer_m = requested.unwrap_or(DEFAULT_BUFFER_M);
    if !buffer_m.is_finite() || !(MIN_BUFFER_M..=MAX_BUFFER_M).contains(&buffer_m) {
        return Err(Refusal::new(
            BUFFER_OUT_OF_RANGE,
            format!("a buffer of {buffer_m} m is outside {MIN_BUFFER_M}..={MAX_BUFFER_M} m"),
            json!({ "buffer_m": buffer_m, "min": MIN_BUFFER_M, "max": MAX_BUFFER_M }),
        ));
    }
    Ok(buffer_m)
}

fn too_large(count: usize) -> Refusal {
    Refusal::new(
        GEOMETRY_TOO_LARGE,
        format!("the geometry would carry {count} positions; a task's geometry holds at most {MAX_POSITIONS}"),
        json!({ "positions": count, "max": MAX_POSITIONS }),
    )
}

/// Resolve and shape. The whole decision, in one call, for every host.
pub fn propose(request: &ProposeRequest) -> Result<Proposal, Refusal> {
    let project = request.project.trim();
    if project.is_empty() {
        return Err(Refusal::new(
            REFERENCE_INVALID,
            "a proposal needs the owning project every link names",
            json!({ "part": "project" }),
        ));
    }
    let references = grammar::parse_all(&request.references)?;
    let resolved = resolve(&references, &request.index.sources)?;

    let structures = resolved
        .iter()
        .filter(|r| matches!(r, Resolved::Structure(..)))
        .count();
    let lines = resolved.len() - structures;
    if lines > 1 || (lines == 1 && structures > 0) {
        return Err(Refusal::new(
            REFERENCES_INCOMPATIBLE,
            "one proposal shapes either structures (a point or an area) or ONE alignment or range (a line); name one or the other",
            json!({ "structures": structures, "lines": lines }),
        ));
    }

    let mut objects: Vec<ResolvedObject> = Vec::new();
    let mut links: Vec<Link> = Vec::new();
    let mut sources: BTreeMap<String, ResolvedSource> = BTreeMap::new();
    let mut note_source = |source: &ObjectIndex| {
        sources
            .entry(source.r#ref.clone())
            .or_insert_with(|| ResolvedSource {
                r#ref: source.r#ref.clone(),
                model_id: source.model_id.clone(),
                model_revision: source.model_revision,
                fingerprint: source.fingerprint.clone(),
                crs: source.crs.clone(),
                display_name: source.display_name.clone(),
            });
    };

    let (geometry, rule) = match resolved.as_slice() {
        [] => unreachable!("parse_all refuses an empty reference list"),
        [Resolved::Alignment(source, alignment)] => {
            note_source(source);
            if alignment.vertices.len() > MAX_POSITIONS {
                return Err(too_large(alignment.vertices.len()));
            }
            if alignment.vertices.len() < 2 {
                return Err(Refusal::new(
                    OBJECT_UNRESOLVED,
                    format!("alignment `{}` has no route to draw", alignment.id),
                    json!({ "unresolved": [alignment.id], "source": source.r#ref }),
                ));
            }
            objects.push(resolved_alignment(source, alignment));
            links.push(link(
                project,
                source,
                OBJECT_TYPE_ALIGNMENT,
                &alignment.id,
                &alignment.label,
            )?);
            let positions: Vec<Position> =
                alignment.vertices.iter().map(|v| [v[0], v[1]]).collect();
            (
                json!({ "type": "LineString", "coordinates": positions_json(&positions) }),
                Rule {
                    kind: "alignment_line",
                    buffer_m: None,
                    positions: positions.len(),
                    objects: 1,
                },
            )
        }
        [Resolved::Range(source, alignment, a, b)] => {
            note_source(source);
            let station = |structure: &IndexedStructure| -> Result<f64, Refusal> {
                structure.station_m.ok_or_else(|| {
                    Refusal::new(
                        OBJECT_UNRESOLVED,
                        format!(
                            "structure `{}` has no station on alignment `{}`",
                            structure.id, alignment.id
                        ),
                        json!({ "unresolved": [structure.id], "source": source.r#ref }),
                    )
                })
            };
            let (sa, sb) = (station(a)?, station(b)?);
            let (first, last) = if sa <= sb { (a, b) } else { (b, a) };
            let (s_first, s_last) = (sa.min(sb), sa.max(sb));
            let vertices: Vec<Vertex> = alignment
                .vertices
                .iter()
                .map(|v| Vertex {
                    position: [v[0], v[1]],
                    station_m: v[2],
                })
                .collect();
            let positions = shape::clip_by_station(
                &vertices,
                (first.position, s_first),
                (last.position, s_last),
            );
            if positions.len() > MAX_POSITIONS {
                return Err(too_large(positions.len()));
            }
            // Every structure of the range, in station order, is an object
            // of the proposal and a link on the task.
            let mut members: Vec<&IndexedStructure> = source
                .structures
                .iter()
                .filter(|s| s.alignment.as_deref() == Some(alignment.id.as_str()))
                .filter(|s| s.station_m.is_some_and(|st| st >= s_first && st <= s_last))
                .collect();
            members.sort_by(|x, y| {
                x.station_m
                    .partial_cmp(&y.station_m)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| x.id.cmp(&y.id))
            });
            if members.len() + 1 > MAX_OBJECTS {
                return Err(Refusal::new(
                    TOO_MANY_OBJECTS,
                    format!(
                        "the range spans {} structures; one proposal resolves at most {MAX_OBJECTS} objects",
                        members.len()
                    ),
                    json!({ "objects": members.len() + 1, "max": MAX_OBJECTS }),
                ));
            }
            objects.push(resolved_alignment(source, alignment));
            links.push(link(
                project,
                source,
                OBJECT_TYPE_ALIGNMENT,
                &alignment.id,
                &alignment.label,
            )?);
            for structure in &members {
                objects.push(resolved_structure(source, structure));
                links.push(link(
                    project,
                    source,
                    OBJECT_TYPE_STRUCTURE,
                    &structure.id,
                    &structure_label(structure),
                )?);
            }
            let count = positions.len();
            (
                json!({ "type": "LineString", "coordinates": positions_json(&positions) }),
                Rule {
                    kind: "alignment_range",
                    buffer_m: None,
                    positions: count,
                    objects: objects.len(),
                },
            )
        }
        all => {
            if all.len() > MAX_OBJECTS {
                return Err(Refusal::new(
                    TOO_MANY_OBJECTS,
                    format!(
                        "{} structures were named; one proposal resolves at most {MAX_OBJECTS} objects",
                        all.len()
                    ),
                    json!({ "objects": all.len(), "max": MAX_OBJECTS }),
                ));
            }
            let mut positions: Vec<Position> = Vec::with_capacity(all.len());
            for item in all {
                let Resolved::Structure(source, structure) = item else {
                    unreachable!("lines were counted above");
                };
                note_source(source);
                // The same structure named twice across references is one
                // object: one position, one link.
                if objects
                    .iter()
                    .any(|o| o.id == structure.id && o.model_id == source.model_id)
                {
                    continue;
                }
                objects.push(resolved_structure(source, structure));
                links.push(link(
                    project,
                    source,
                    OBJECT_TYPE_STRUCTURE,
                    &structure.id,
                    &structure_label(structure),
                )?);
                positions.push(structure.position);
            }
            if positions.len() == 1 {
                (
                    json!({ "type": "Point", "coordinates": [shape::round(positions[0][0]), shape::round(positions[0][1])] }),
                    Rule {
                        kind: "point",
                        buffer_m: None,
                        positions: 1,
                        objects: 1,
                    },
                )
            } else {
                let buffer_m = buffer(request.buffer_m)?;
                let ring = shape::buffered_hull(&positions, buffer_m);
                if ring.len() > MAX_POSITIONS {
                    return Err(too_large(ring.len()));
                }
                let count = ring.len();
                (
                    json!({ "type": "Polygon", "coordinates": [positions_json(&ring)] }),
                    Rule {
                        kind: "buffered_hull",
                        buffer_m: Some(buffer_m),
                        positions: count,
                        objects: objects.len(),
                    },
                )
            }
        }
    };

    // A link the task already carries is not proposed again; the total is
    // bounded here so the refusal names the number rather than arriving as
    // an engine violation after the round trip.
    let existing: Vec<String> = request
        .existing_links
        .iter()
        .map(|l| {
            link_key(
                l["kind"].as_str().unwrap_or_default(),
                l["object_type"].as_str().unwrap_or_default(),
                l["entity_id"].as_str().unwrap_or_default(),
            )
        })
        .collect();
    links.retain(|l| !existing.contains(&link_key(l.kind, l.object_type, &l.entity_id)));
    let total = request.existing_links.len() + links.len();
    if total > MAX_LINKS {
        return Err(Refusal::new(
            LINKS_BOUND_EXCEEDED,
            format!("the task would carry {total} links; a task holds at most {MAX_LINKS}"),
            json!({ "existing": request.existing_links.len(), "new": links.len(), "max": MAX_LINKS }),
        ));
    }

    Ok(Proposal {
        schema: SCHEMA,
        geometry,
        rule,
        objects,
        links,
        sources: sources.into_values().collect(),
    })
}

// ── The byte door ────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct Head {
    #[serde(default)]
    schema: String,
    #[serde(default)]
    action: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ParseRequest {
    #[allow(dead_code)]
    schema: String,
    #[allow(dead_code)]
    action: String,
    references: Vec<String>,
}

fn refusal_json(refusal: Refusal) -> String {
    serde_json::to_string(&refusal).unwrap_or(refusal.message)
}

/// Evaluate one request. `parse` answers the typed references and the
/// distinct sources a host must load; `propose` resolves and shapes.
///
/// A refusal is answered as the JSON of [`Refusal`] in the error, so a host
/// that only has the byte door still gets the code and the detail.
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT_BYTES {
        return Err("Task geometry request exceeds its bound.".into());
    }
    let head: Head = serde_json::from_slice(input)
        .map_err(|error| format!("Invalid task geometry request: {error}"))?;
    if head.schema != SCHEMA {
        return Err(format!(
            "Unsupported task geometry schema `{}`.",
            head.schema
        ));
    }
    match head.action.as_str() {
        "parse" => {
            let request: ParseRequest = serde_json::from_slice(input)
                .map_err(|error| format!("Invalid task geometry request: {error}"))?;
            let references = grammar::parse_all(&request.references).map_err(refusal_json)?;
            let mut sources: Vec<Source> = references.iter().map(|r| r.source.clone()).collect();
            sources.sort();
            sources.dedup();
            serde_json::to_string(&json!({
                "schema": SCHEMA,
                "references": references,
                "sources": sources.iter().map(|s| json!({ "kind": s, "ref": s.key() })).collect::<Vec<_>>(),
            }))
            .map_err(|error| error.to_string())
        }
        "propose" => {
            let request: ProposeRequest = serde_json::from_slice(input)
                .map_err(|error| format!("Invalid task geometry request: {error}"))?;
            let proposal = propose(&request).map_err(refusal_json)?;
            serde_json::to_string(&proposal).map_err(|error| error.to_string())
        }
        other => Err(format!("Unsupported task geometry action `{other}`.")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One synthetic model: an alignment of four vertices with six numbered
    /// structures along it, one detached structure, and a second alignment
    /// that reuses the number `1` — exactly the ambiguity a real model has.
    fn index(r#ref: &str) -> ObjectIndex {
        let structure = |id: &str,
                         number: Option<&str>,
                         alignment: Option<&str>,
                         station: Option<f64>,
                         lon: f64,
                         lat: f64| {
            IndexedStructure {
                id: id.into(),
                number: number.map(str::to_owned),
                alignment: alignment.map(str::to_owned),
                station_m: station,
                position: [lon, lat],
            }
        };
        ObjectIndex {
            r#ref: r#ref.into(),
            model_id: "m-nyam".into(),
            model_revision: 3,
            fingerprint: "abc123".into(),
            crs: "EPSG:32735".into(),
            display_name: Some("Nyamagabe MV".into()),
            structures: vec![
                structure(
                    "s-73",
                    Some("73"),
                    Some("aln-1"),
                    Some(200.0),
                    29.6020,
                    -2.3000,
                ),
                structure(
                    "s-74",
                    Some("74"),
                    Some("aln-1"),
                    Some(500.0),
                    29.6050,
                    -2.3000,
                ),
                structure(
                    "s-75",
                    Some("75"),
                    Some("aln-1"),
                    Some(1200.0),
                    29.6100,
                    -2.3020,
                ),
                structure(
                    "s-76",
                    Some("76"),
                    Some("aln-1"),
                    Some(1800.0),
                    29.6100,
                    -2.3080,
                ),
                structure(
                    "s-77",
                    Some("77"),
                    Some("aln-1"),
                    Some(2500.0),
                    29.6150,
                    -2.3100,
                ),
                structure(
                    "s-78",
                    Some("78"),
                    Some("aln-1"),
                    Some(2900.0),
                    29.6190,
                    -2.3100,
                ),
                structure("s-x", None, None, None, 29.6500, -2.3500),
                structure(
                    "s-b1",
                    Some("1"),
                    Some("aln-2"),
                    Some(0.0),
                    29.7000,
                    -2.4000,
                ),
                structure(
                    "s-a1",
                    Some("1"),
                    Some("aln-1"),
                    Some(0.0),
                    29.6000,
                    -2.3000,
                ),
            ],
            alignments: vec![
                IndexedAlignment {
                    id: "aln-1".into(),
                    label: "Line 1".into(),
                    vertices: vec![
                        [29.60, -2.30, 0.0],
                        [29.61, -2.30, 1000.0],
                        [29.61, -2.31, 2000.0],
                        [29.62, -2.31, 3000.0],
                    ],
                },
                IndexedAlignment {
                    id: "aln-2".into(),
                    label: "Line 1".into(),
                    vertices: vec![[29.70, -2.40, 0.0], [29.71, -2.40, 1000.0]],
                },
            ],
        }
    }

    fn request(references: &[&str], buffer_m: Option<f64>) -> ProposeRequest {
        ProposeRequest {
            schema: SCHEMA.into(),
            action: "propose".into(),
            project: "agct38sq_sample".into(),
            references: references.iter().map(|r| r.to_string()).collect(),
            buffer_m,
            existing_links: Vec::new(),
            index: IndexInput {
                sources: vec![index("local-abc"), index("package")],
            },
        }
    }

    #[test]
    fn the_owner_s_example_becomes_a_buffered_area_with_three_traceable_links() {
        let proposal =
            propose(&request(&["dsgrid:local-abc:structure:74,76,77"], None)).expect("proposes");
        assert_eq!(proposal.geometry["type"], "Polygon");
        let ring = proposal.geometry["coordinates"][0]
            .as_array()
            .expect("ring");
        assert_eq!(ring.first(), ring.last(), "closed");
        assert_eq!(proposal.rule.kind, "buffered_hull");
        assert_eq!(proposal.rule.buffer_m, Some(DEFAULT_BUFFER_M));
        assert_eq!(proposal.rule.positions, ring.len());
        assert_eq!(proposal.rule.objects, 3);
        let ids: Vec<&str> = proposal.objects.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["s-74", "s-76", "s-77"]);
        assert_eq!(proposal.objects[0].label, "Structure 74");
        assert_eq!(proposal.objects[0].source, "local-abc");
        assert_eq!(proposal.links.len(), 3);
        assert_eq!(
            proposal.links[0],
            Link {
                kind: "ds_object",
                project_id: "agct38sq_sample".into(),
                object_type: "dsgrid_structure",
                entity_id: "m-nyam:s-74".into(),
                object_revision: "dsgrid:r3:abc123".into(),
                label: "Structure 74".into(),
            }
        );
        assert_eq!(proposal.sources.len(), 1);
        assert_eq!(proposal.sources[0].model_id, "m-nyam");
        assert_eq!(
            proposal.sources[0].display_name.as_deref(),
            Some("Nyamagabe MV")
        );
        // Byte-for-byte deterministic.
        let again = propose(&request(&["dsgrid:local-abc:structure:74,76,77"], None)).unwrap();
        assert_eq!(proposal, again);
    }

    #[test]
    fn one_structure_is_a_point_and_an_id_works_where_a_number_does_not() {
        let proposal = propose(&request(&["dsgrid:package:structure:s-x"], None)).unwrap();
        assert_eq!(
            proposal.geometry,
            json!({ "type": "Point", "coordinates": [29.65, -2.35] })
        );
        assert_eq!(proposal.rule.kind, "point");
        assert_eq!(proposal.objects[0].label, "Structure s-x");
        assert_eq!(proposal.links[0].entity_id, "m-nyam:s-x");
        // Two references naming the same structure make one object.
        let twice = propose(&request(
            &[
                "dsgrid:package:structure:74",
                "dsgrid:package:structure:s-74",
            ],
            None,
        ))
        .unwrap();
        assert_eq!(twice.rule.kind, "point");
        assert_eq!(twice.links.len(), 1);
    }

    #[test]
    fn a_whole_alignment_and_a_range_are_lines_and_the_range_links_its_structures() {
        let line = propose(&request(&["dsgrid:local-abc:alignment:aln-1"], None)).unwrap();
        assert_eq!(line.geometry["type"], "LineString");
        assert_eq!(line.rule.kind, "alignment_line");
        assert_eq!(line.rule.positions, 4);
        assert_eq!(line.links.len(), 1);
        assert_eq!(line.links[0].object_type, "dsgrid_alignment");
        assert_eq!(line.links[0].label, "Line 1");

        let range = propose(&request(&["dsgrid:local-abc:alignment:aln-1:77..74"], None)).unwrap();
        assert_eq!(range.rule.kind, "alignment_range");
        assert_eq!(
            range.geometry["coordinates"],
            json!([
                [29.605, -2.3],
                [29.61, -2.3],
                [29.61, -2.31],
                [29.615, -2.31]
            ])
        );
        let ids: Vec<&str> = range.objects.iter().map(|o| o.id.as_str()).collect();
        assert_eq!(ids, ["aln-1", "s-74", "s-75", "s-76", "s-77"]);
        assert_eq!(range.links.len(), 5);
        assert_eq!(range.rule.objects, 5);
    }

    #[test]
    fn every_refusal_names_what_it_is_about() {
        let refuse =
            |refs: &[&str], buffer: Option<f64>| propose(&request(refs, buffer)).unwrap_err();

        let r = refuse(&["dsgrid:local-nope:structure:74"], None);
        assert_eq!(r.code, MODEL_UNKNOWN);
        assert_eq!(r.detail["source"], "local-nope");

        let r = refuse(&["dsgrid:local-abc:structure:74,99,100"], None);
        assert_eq!(r.code, OBJECT_UNRESOLVED);
        assert_eq!(r.detail["unresolved"], json!(["99", "100"]));

        let r = refuse(&["dsgrid:local-abc:structure:1"], None);
        assert_eq!(r.code, OBJECT_AMBIGUOUS);
        assert_eq!(r.detail["candidates"].as_array().map(Vec::len), Some(2));

        // On a named alignment the same number is not ambiguous.
        let range = propose(&request(&["dsgrid:local-abc:alignment:aln-1:1..74"], None)).unwrap();
        assert_eq!(range.objects[1].id, "s-a1");

        let r = refuse(&["dsgrid:local-abc:alignment:Line 1"], None);
        assert_eq!(r.code, OBJECT_AMBIGUOUS);
        let r = refuse(&["dsgrid:local-abc:alignment:aln-9"], None);
        assert_eq!(r.code, OBJECT_UNRESOLVED);
        // A structure not placed on the named alignment is unresolved there.
        let r = refuse(&["dsgrid:local-abc:alignment:aln-2:74..1"], None);
        assert_eq!(r.code, OBJECT_UNRESOLVED);

        let r = refuse(
            &[
                "dsgrid:local-abc:structure:74",
                "dsgrid:local-abc:alignment:aln-1",
            ],
            None,
        );
        assert_eq!(r.code, REFERENCES_INCOMPATIBLE);
        let r = refuse(
            &[
                "dsgrid:local-abc:alignment:aln-1",
                "dsgrid:local-abc:alignment:aln-2",
            ],
            None,
        );
        assert_eq!(r.code, REFERENCES_INCOMPATIBLE);

        for buffer in [0.5, 501.0, f64::NAN] {
            let r = refuse(&["dsgrid:local-abc:structure:74,76"], Some(buffer));
            assert_eq!(r.code, BUFFER_OUT_OF_RANGE, "{buffer}");
        }
        // One structure needs no buffer, so an out-of-range one is not checked.
        assert!(propose(&request(&["dsgrid:local-abc:structure:74"], Some(9_999.0))).is_ok());

        let r = refuse(&["transformer:TX-104"], None);
        assert_eq!(r.code, REFERENCE_INVALID);
        let mut blank = request(&["dsgrid:local-abc:structure:74"], None);
        blank.project = "  ".into();
        assert_eq!(propose(&blank).unwrap_err().code, REFERENCE_INVALID);
    }

    #[test]
    fn the_object_and_position_bounds_are_named_with_their_numbers() {
        let mut big = index("package");
        big.structures = (0..MAX_OBJECTS + 1)
            .map(|i| IndexedStructure {
                id: format!("s-{i}"),
                number: Some(i.to_string()),
                alignment: None,
                station_m: None,
                position: [29.6 + i as f64 * 1e-4, -2.3],
            })
            .collect();
        let selectors: Vec<String> = (0..MAX_OBJECTS + 1).map(|i| i.to_string()).collect();
        let mut req = request(&[], None);
        req.references = vec![format!("dsgrid:package:structure:{}", selectors.join(","))];
        req.index.sources = vec![big.clone()];
        let r = propose(&req).unwrap_err();
        assert_eq!(r.code, TOO_MANY_OBJECTS);
        assert_eq!(r.detail["max"], MAX_OBJECTS);

        let mut long = index("package");
        long.alignments[0].vertices = (0..MAX_POSITIONS + 1)
            .map(|i| [29.6 + i as f64 * 1e-5, -2.3, i as f64])
            .collect();
        req.references = vec!["dsgrid:package:alignment:aln-1".into()];
        req.index.sources = vec![long];
        let r = propose(&req).unwrap_err();
        assert_eq!(r.code, GEOMETRY_TOO_LARGE);
        assert_eq!(r.detail["positions"], MAX_POSITIONS + 1);
    }

    #[test]
    fn existing_links_are_not_proposed_twice_and_the_task_s_link_bound_holds() {
        let mut req = request(&["dsgrid:local-abc:structure:74,76,77"], None);
        req.existing_links = vec![json!({
            "kind": "ds_object", "project_id": "agct38sq_sample",
            "object_type": "dsgrid_structure", "entity_id": "m-nyam:s-76",
            "object_revision": "dsgrid:r2:old", "label": "Structure 76",
            "attached_by": "someone@ds.rw", "attached_at": "2026-09-01T00:00:00Z",
        })];
        let proposal = propose(&req).unwrap();
        // The geometry still uses all three; the links add only the two new.
        assert_eq!(proposal.rule.objects, 3);
        let entities: Vec<&str> = proposal
            .links
            .iter()
            .map(|l| l.entity_id.as_str())
            .collect();
        assert_eq!(entities, ["m-nyam:s-74", "m-nyam:s-77"]);

        req.existing_links = (0..MAX_LINKS - 1)
            .map(|i| json!({ "kind": "pm_task", "project_id": "p", "entity_id": format!("t-{i}") }))
            .collect();
        let r = propose(&req).unwrap_err();
        assert_eq!(r.code, LINKS_BOUND_EXCEEDED);
        assert_eq!(r.detail["new"], 3);
    }

    #[test]
    fn the_byte_door_parses_and_proposes_and_answers_a_refusal_as_json() {
        let parsed: Value = serde_json::from_str(
            &evaluate(
                serde_json::to_vec(&json!({
                    "schema": SCHEMA, "action": "parse",
                    "references": ["dsgrid:local-abc:structure:74,76", "dsgrid:package:alignment:aln-1:74..77"],
                }))
                .unwrap()
                .as_slice(),
            )
            .expect("parses"),
        )
        .unwrap();
        assert_eq!(parsed["references"].as_array().map(Vec::len), Some(2));
        assert_eq!(parsed["references"][0]["source"]["kind"], "local");
        assert_eq!(
            parsed["references"][0]["object"]["selectors"],
            json!(["74", "76"])
        );
        assert_eq!(parsed["references"][1]["object"]["kind"], "alignment_range");
        assert_eq!(
            parsed["sources"],
            json!([{ "kind": { "kind": "local", "id": "abc" }, "ref": "local-abc" }, { "kind": { "kind": "package" }, "ref": "package" }])
        );

        let mut body = serde_json::to_value(json!({
            "schema": SCHEMA, "action": "propose", "project": "p",
            "references": ["dsgrid:package:structure:74"],
        }))
        .unwrap();
        body["index"] = json!({ "sources": [index("package")] });
        let reply: Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&body).unwrap()).unwrap()).unwrap();
        assert_eq!(reply["rule"]["kind"], "point");
        assert_eq!(reply["links"][0]["object_type"], "dsgrid_structure");

        body["references"] = json!(["dsgrid:package:structure:404"]);
        let error = evaluate(&serde_json::to_vec(&body).unwrap()).unwrap_err();
        let refusal: Value = serde_json::from_str(&error).expect("a JSON refusal");
        assert_eq!(refusal["code"], OBJECT_UNRESOLVED);

        assert!(evaluate(b"{\"schema\":\"x\"}")
            .unwrap_err()
            .starts_with("Unsupported task geometry schema"));
        assert!(
            evaluate(br#"{"schema":"ds.task-geometry/v1","action":"draw"}"#)
                .unwrap_err()
                .starts_with("Unsupported task geometry action")
        );
        assert!(evaluate(
            br#"{"schema":"ds.task-geometry/v1","action":"parse","references":[],"extra":1}"#
        )
        .unwrap_err()
        .starts_with("Invalid task geometry request"));
    }
}
