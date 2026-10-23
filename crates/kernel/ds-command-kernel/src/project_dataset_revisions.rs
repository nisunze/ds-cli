//! Shared project dataset revisions: the governed chain a project's seeded
//! building footprints and contours are published as, so a project is seeded
//! once and every member's machine reuses it.
//!
//! Each project room (`project_dataset_cache`) is still one principal's local
//! holding. What this module adds is the project's SHARED answer: an ordered
//! chain of revisions, each completing an exact coverage with a verified delta
//! artifact, published through the compute artifact door (engine
//! [`ENGINE`]). A reader replays revisions it has not applied into its own
//! room; a seeder acquires only what neither its room nor the chain covers and
//! publishes that delta as the next revision, fenced on the manifest it planned
//! against ([`base_fingerprint`]). Brain authorizes and stores; it computes
//! nothing.
//!
//! Decisions only. No I/O, no clock, no credential: hosts read and publish,
//! engines convert, ds-project-data orchestrates.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::project_dataset_cache::{
    assess, extend, freshness, validate_source_receipt, Coverage, Dataset, Freshness, HeldVersion,
    Quality,
};

pub const SCHEMA: &str = "ds.project-dataset-revisions/v1";
/// The compute artifact engine a project dataset head is published under.
pub const ENGINE: &str = "project_dataset";
pub const MANIFEST_OUTPUT: &str = "manifest";
pub const MANIFEST_FORMAT: &str = "json";
pub const MANIFEST_CONTENT_TYPE: &str = "application/json";
/// One revision's features: gzip-compressed GeoJSON lines.
pub const DELTA_FORMAT: &str = "geojsonl_gz";
pub const DELTA_CONTENT_TYPE: &str = "application/gzip";
/// One head declares at most 100 outputs, the manifest among them.
pub const MAX_REVISIONS: usize = 99;
/// The door's per-output byte bound.
pub const MAX_DELTA_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_DELTA_FEATURES: u64 = 500_000;
pub const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_TEXT: usize = 512;

/// The dataset identity every member shares. The room's provider token and
/// the principal stay out of it: they address one machine's holding, never
/// the project's data.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct SharedDataset {
    pub id: String,
    pub quality: Quality,
    #[serde(default)]
    pub parameters: BTreeMap<String, Value>,
    pub crs: String,
}

impl SharedDataset {
    pub fn of(dataset: &Dataset) -> Self {
        Self {
            id: dataset.id.clone(),
            quality: dataset.quality,
            parameters: dataset.parameters.clone(),
            crs: dataset.crs.clone(),
        }
    }
}

/// One published completion of exact coverage. Its delta replaces, inside its
/// cells, every row an earlier revision's cells select there.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Revision {
    /// One-based and contiguous.
    pub revision: u32,
    pub output_id: String,
    pub coverage: Coverage,
    /// Publisher or surface version the rows were produced from.
    pub source_version: String,
    pub source_receipt: Value,
    pub feature_count: u64,
    pub sha256: String,
    pub size_bytes: u64,
    /// An explicit re-acquisition of coverage an earlier revision held.
    #[serde(default)]
    pub refresh: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Manifest {
    pub schema: String,
    pub project: String,
    pub dataset: SharedDataset,
    pub revisions: Vec<Revision>,
}

/// What a seeder declares for its next revision.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub coverage: Coverage,
    pub source_version: String,
    pub source_receipt: Value,
    pub feature_count: u64,
    pub sha256: String,
    pub size_bytes: u64,
    pub refresh: bool,
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn lower_sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn artifact_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.as_bytes()[0].is_ascii_alphanumeric()
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn bounded_text(value: &str) -> bool {
    !value.is_empty() && value.len() <= MAX_TEXT && !value.chars().any(char::is_control)
}

/// The head operation: the canonical dataset id.
pub fn head_operation(dataset: &Dataset) -> Result<String, String> {
    if !artifact_id(&dataset.id) {
        return Err(format!(
            "dataset `{}` cannot name a shared project revision head",
            dataset.id
        ));
    }
    Ok(dataset.id.clone())
}

/// The head variant: a digest of the shared identity, so two contour
/// interval choices are two chains and never one chain answering both.
pub fn head_variant(dataset: &Dataset) -> String {
    let identity = serde_json::to_vec(&SharedDataset::of(dataset))
        .expect("a dataset identity always serializes");
    format!("p{}", &sha(&identity)[..40])
}

/// The optimistic fence a publication carries: the digest of the manifest it
/// planned against, or of no bytes when the project has published nothing.
pub fn base_fingerprint(manifest_bytes: Option<&[u8]>) -> String {
    sha(manifest_bytes.unwrap_or_default())
}

pub fn output_id(revision: u32) -> String {
    format!("rev-{revision:06}")
}

pub fn encode(manifest: &Manifest) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(manifest).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_MANIFEST_BYTES {
        return Err("the project dataset revision manifest exceeds its byte bound".into());
    }
    Ok(bytes)
}

/// Admit a published manifest for one project and dataset. Additive fields
/// a later publisher writes are accepted; anything that breaks the chain's
/// identity, order, bounds or digests is refused whole.
pub fn decode(bytes: &[u8], project: &str, dataset: &Dataset) -> Result<Manifest, String> {
    if bytes.is_empty() || bytes.len() > MAX_MANIFEST_BYTES {
        return Err("the project dataset revision manifest is empty or exceeds its bound".into());
    }
    let manifest: Manifest = serde_json::from_slice(bytes)
        .map_err(|error| format!("the project dataset revision manifest is unreadable: {error}"))?;
    if manifest.schema != SCHEMA {
        return Err(format!(
            "the project dataset revision manifest is {:?}, not {SCHEMA}",
            manifest.schema
        ));
    }
    if manifest.project != project || manifest.dataset != SharedDataset::of(dataset) {
        return Err(
            "the project dataset revision manifest belongs to another project or dataset".into(),
        );
    }
    validate_chain(&manifest.revisions)?;
    Ok(manifest)
}

fn validate_chain(revisions: &[Revision]) -> Result<(), String> {
    if revisions.is_empty() || revisions.len() > MAX_REVISIONS {
        return Err(format!(
            "a project dataset chain holds 1 to {MAX_REVISIONS} revisions"
        ));
    }
    let mut outputs = BTreeSet::new();
    for (index, revision) in revisions.iter().enumerate() {
        let expected = index as u32 + 1;
        if revision.revision != expected
            || revision.output_id != output_id(expected)
            || !outputs.insert(revision.output_id.as_str())
        {
            return Err(format!(
                "project dataset revision {} is out of order or misnamed",
                revision.revision
            ));
        }
        revision.coverage.rects()?;
        if revision.coverage.is_empty()
            || !bounded_text(&revision.source_version)
            || !lower_sha(&revision.sha256)
            || revision.size_bytes == 0
            || revision.size_bytes > MAX_DELTA_BYTES
            || revision.feature_count > MAX_DELTA_FEATURES
        {
            return Err(format!(
                "project dataset revision {} has an invalid coverage, version, digest or bound",
                revision.revision
            ));
        }
        validate_source_receipt(&revision.source_receipt)
            .map_err(|error| format!("project dataset revision {}: {error}", revision.revision))?;
    }
    Ok(())
}

/// The next manifest: the previous chain plus one revision.
pub fn next(
    previous: Option<&Manifest>,
    project: &str,
    dataset: &Dataset,
    draft: Draft,
) -> Result<Manifest, String> {
    let mut revisions = previous.map(|m| m.revisions.clone()).unwrap_or_default();
    if let Some(previous) = previous {
        if previous.project != project || previous.dataset != SharedDataset::of(dataset) {
            return Err("the previous manifest belongs to another project or dataset".into());
        }
    }
    if revisions.len() >= MAX_REVISIONS {
        return Err(format!(
            "this project dataset already holds {MAX_REVISIONS} revisions; it needs a compacted chain before another is published"
        ));
    }
    let number = revisions.len() as u32 + 1;
    revisions.push(Revision {
        revision: number,
        output_id: output_id(number),
        coverage: draft.coverage,
        source_version: draft.source_version,
        source_receipt: draft.source_receipt,
        feature_count: draft.feature_count,
        sha256: draft.sha256,
        size_bytes: draft.size_bytes,
        refresh: draft.refresh,
    });
    validate_chain(&revisions)?;
    let manifest = Manifest {
        schema: SCHEMA.into(),
        project: project.into(),
        dataset: SharedDataset::of(dataset),
        revisions,
    };
    encode(&manifest)?;
    Ok(manifest)
}

/// Everything the chain completed.
pub fn coverage(manifest: &Manifest) -> Result<Coverage, String> {
    manifest
        .revisions
        .iter()
        .try_fold(Coverage::default(), |held, revision| {
            extend(&held, &revision.coverage)
        })
}

fn exact_gaps(held: &Coverage, wanted: &Coverage) -> Result<Coverage, String> {
    let assessment = assess(held, wanted)?;
    if assessment.simplified {
        return Err(
            "the shared coverage arithmetic exceeded its exact representation; narrow the seed"
                .into(),
        );
    }
    Ok(assessment.gaps)
}

/// What a seeder publishes: the wanted coverage its room completed that the
/// chain does not already hold. A refresh publishes all of it again.
pub fn delta(
    wanted: &Coverage,
    held: &Coverage,
    published: &Coverage,
    refresh: bool,
) -> Result<Coverage, String> {
    let missing = exact_gaps(held, wanted)?;
    let completed = exact_gaps(&missing, wanted)?;
    if refresh {
        return Ok(completed);
    }
    exact_gaps(published, &completed)
}

/// Revisions a reader has not applied yet, in chain order.
pub fn pending<'a>(manifest: &'a Manifest, applied: &BTreeSet<u32>) -> Vec<&'a Revision> {
    manifest
        .revisions
        .iter()
        .filter(|revision| !applied.contains(&revision.revision))
        .collect()
}

/// Which published coverage was produced from a version other than the one
/// the acquisition owner reads now. Reported, never acted on: only an
/// explicit refresh re-acquires it.
pub fn chain_freshness(manifest: &Manifest, current: &str) -> Result<Freshness, String> {
    let mut held = Vec::with_capacity(manifest.revisions.len());
    for (index, revision) in manifest.revisions.iter().enumerate() {
        let later = manifest.revisions[index + 1..]
            .iter()
            .try_fold(Coverage::default(), |acc, next| {
                extend(&acc, &next.coverage)
            })?;
        held.push(HeldVersion {
            source_version: revision.source_version.clone(),
            coverage: exact_gaps(&later, &revision.coverage)?,
        });
    }
    freshness(current, &held)
}

/// A bounded summary of a chain for receipts and status rows.
pub fn summary(manifest: &Manifest) -> Value {
    let last = manifest.revisions.last();
    json!({
        "revisions": manifest.revisions.len(),
        "latest_revision": last.map(|r| r.revision),
        "source_versions": manifest.revisions.iter().map(|r| r.source_version.as_str()).collect::<BTreeSet<_>>(),
        "features_published": manifest.revisions.iter().map(|r| r.feature_count).sum::<u64>(),
        "coverage_cells": manifest.revisions.iter().map(|r| r.coverage.cells.len()).sum::<usize>(),
    })
}

/// Admit one revision's `open` declaration on the native publication door:
/// the shared head, the converting engine's release, and exactly the
/// manifest plus one delta. Nothing else may ride on this engine.
pub fn validate_open(object: &serde_json::Map<String, Value>) -> Result<(), String> {
    let text = |key: &str| object.get(key).and_then(Value::as_str).unwrap_or_default();
    let variant = text("variant");
    if text("engine") != ENGINE
        || !artifact_id(text("operation"))
        || variant.len() != 41
        || !variant.starts_with('p')
        || !lower_sha_prefix(&variant[1..])
        || !text("engine_version").starts_with("ds-network-reporter@")
        || !lower_sha(text("engine_build_manifest_sha256"))
        || !lower_sha(text("input_base_fingerprint"))
        || [
            "source_snapshot_sha256",
            "snapshot_receipt_id",
            "printing_preview",
        ]
        .iter()
        .any(|key| object.contains_key(*key))
    {
        return Err("invalid project dataset revision publication".into());
    }
    let outputs = object
        .get("outputs")
        .and_then(Value::as_array)
        .filter(|outputs| outputs.len() == 2)
        .ok_or("a revision publishes its manifest and one delta")?;
    let mut ids = BTreeSet::new();
    for output in outputs {
        let declaration: crate::compute_artifact_inventory::Declaration =
            serde_json::from_value(output.clone()).map_err(|_| "invalid output declaration")?;
        crate::compute_artifact_inventory::validate(ENGINE, &declaration)?;
        let expected =
            if declaration.output_id == MANIFEST_OUTPUT {
                MANIFEST_FORMAT
            } else if declaration.output_id.strip_prefix("rev-").is_some_and(|n| {
                n.len() == 6 && n.bytes().all(|b| b.is_ascii_digit()) && n != "000000"
            }) {
                DELTA_FORMAT
            } else {
                return Err("a revision output is the manifest or one rev-NNNNNN delta".into());
            };
        if declaration.format != expected
            || declaration.size_bytes > MAX_DELTA_BYTES
            || declaration.filename.is_some()
            || !ids.insert(declaration.output_id.clone())
        {
            return Err("a revision output has the wrong format, bound or identity".into());
        }
    }
    if !ids.contains(MANIFEST_OUTPUT) {
        return Err("a revision publishes its manifest".into());
    }
    Ok(())
}

fn lower_sha_prefix(value: &str) -> bool {
    value
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
// The publisher tiling, the Reporter task contract and the S2 cell token are
// shared with the Reporter (ds-report-contracts `project_datasets`).
pub use ds_report_contracts::project_datasets::{google_open_buildings, s2_token, tasks};

#[cfg(test)]
#[path = "project_dataset_revisions_tests.rs"]
mod tests;
