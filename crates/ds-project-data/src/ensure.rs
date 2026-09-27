//! The governed acquisition stage for one project dataset — the port of the
//! desktop's `project-data/acquire.ts`.
//!
//! The order is always: plan the project's fused coverage → open (acquiring)
//! or read (never acquiring) the room → compare against what actually
//! COMPLETED → acquire only the gaps, one cell at a time → commit or fail →
//! assess again. Every decision is the kernel's; every write is the store's;
//! the provider is the host's.
//!
//! Three properties hold by construction:
//!
//! * `Mode::Read` contacts no provider and creates no room. Missing coverage is
//!   reported, never fetched inside a render.
//! * A failed cell records its failure on the room and leaves held data
//!   intact; the outcome is never `covered` while a gap remains.
//! * A second unchanged seed acquires nothing: the plan is deterministic, the
//!   room's completed coverage answers `assess` with no gaps, and the store's
//!   `begin` refuses a repeated digest anyway.

use ds_command_kernel::project_dataset_cache::{
    assess, bounds_area_m2, dataset_key, plan_focused, query_digest, BufferPolicy, Coverage,
    Dataset, Extent, Scope, MAX_ENVELOPE_M2,
};
use serde_json::{json, Value};
use std::path::Path;

use crate::bundle::{install_bundle, installed, BundleReceipt};
use crate::catalog::ReferenceResource;
use crate::datasets::at_version;
use crate::declared::DeclaredSource;
use crate::Failure;

/// One completed page from a provider, in the shape the room commits.
#[derive(Debug, Clone, PartialEq)]
pub struct Acquisition {
    pub features: Vec<Value>,
    /// The provider's own version for these rows. Never invented here.
    pub source_version: String,
    /// The provider's own receipt, bounded by the kernel before it is stored.
    pub source_receipt: Value,
}

/// The host's door to a governed provider (ds-brain `query_print_context`).
/// Called only from the explicit acquisition stage, once per gap cell.
pub trait Provider {
    fn acquire(&mut self, dataset: &Dataset, area: &Value) -> Result<Acquisition, Failure>;
}

/// What a host lends an acquiring call: the provider door and the byte
/// transfer for national bundles.
pub struct Hosts<'a> {
    pub provider: &'a mut dyn Provider,
    pub fetch: &'a mut dyn FnMut(&BundleReceipt, &Path) -> Result<(), String>,
}

/// Read what is held, or acquire what is missing.
pub enum Mode<'m, 'a> {
    Read,
    Acquire(&'m mut Hosts<'a>),
}

impl Mode<'_, '_> {
    pub const fn acquires(&self) -> bool {
        matches!(self, Mode::Acquire(_))
    }
}

#[derive(Debug, Clone)]
pub struct EnsureOutcome {
    pub dataset_key: String,
    /// The room's status after this call, as the store reports it.
    pub status: Value,
    /// The project's planned coverage for this dataset.
    pub coverage: Coverage,
    pub coverage_geometry: Option<Value>,
    /// Fused design clusters the plan covers.
    pub clusters: usize,
    /// Completed coverage answers the whole plan.
    pub covered: bool,
    /// Cells acquired in THIS call. Zero on a warm reuse.
    pub acquired: usize,
    /// Cells another process was already acquiring; read, not fetched.
    pub skipped: usize,
    pub features: i64,
    pub warnings: Vec<String>,
}

/// One room's address and fence, with the store's verbs.
struct Room<'a> {
    root: &'a Path,
    scope: &'a Scope,
    key: String,
}

impl Room<'_> {
    fn call(&self, mut request: Value) -> Result<Value, Failure> {
        request["dataset_key"] = json!(self.key);
        request["principal"] = json!(self.scope.principal);
        request["project"] = json!(self.scope.project);
        ds_layer_store::project_dataset_cache::execute_at(self.root, request).map_err(crate::store)
    }

    fn open(&self, dataset: &Dataset, policy: &BufferPolicy) -> Result<Value, Failure> {
        let document = serde_json::to_value(dataset).map_err(crate::store)?;
        self.call(json!({
            "action": "open",
            "dataset_id": document["id"],
            "provider": document["provider"],
            "quality": document["quality"],
            "parameters": document["parameters"],
            "crs": document["crs"],
            "source_version": "",
            "buffer_policy": policy,
        }))
    }

    fn status(&self) -> Result<Value, Failure> {
        self.call(json!({"action": "status"}))
    }

    fn begin(
        &self,
        digest: &str,
        area: &Value,
        cells: &Coverage,
        source_version: &str,
    ) -> Result<Value, Failure> {
        self.call(json!({
            "action": "begin",
            "query_digest": digest,
            "area": area,
            "cells": cells,
            "source_version": source_version,
            "wait_for_pending": true,
        }))
    }

    fn commit(
        &self,
        digest: &str,
        ticket: &str,
        cells: &Coverage,
        page: &Acquisition,
        fallback_version: &str,
    ) -> Result<Value, Failure> {
        let version = if page.source_version.is_empty() {
            fallback_version
        } else {
            &page.source_version
        };
        self.call(json!({
            "action": "commit",
            "query_digest": digest,
            "ticket": ticket,
            "cells": cells,
            "features": page.features,
            "source_version": version,
            "source_receipt": page.source_receipt,
        }))
    }

    fn fail(&self, digest: &str, ticket: &str, message: &str) -> Result<Value, Failure> {
        self.call(json!({
            "action": "fail",
            "query_digest": digest,
            "ticket": ticket,
            "message": message,
        }))
    }
}

fn completed(status: &Value) -> Result<Coverage, Failure> {
    serde_json::from_value(status["completed"].clone()).map_err(crate::store)
}

fn polygon(cell: [f64; 4]) -> Value {
    let [w, s, e, n] = cell;
    json!({"type": "Polygon", "coordinates": [[[w, s], [e, s], [e, n], [w, n], [w, s]]]})
}

/// The most rows one national-index read returns before it says it truncated.
const CATALOG_QUERY_LIMIT: usize = 10_000;
/// How many times a truncated national-index read is quartered before the
/// layer is refused: 4⁴ = 256 parts of the tile it started from.
const MAX_SPLIT_DEPTH: u32 = 4;
/// The most tiles one gap cell may become. A district's overview — about 30
/// tiles for Gisagara — sits far below it; a cell needing more is not a
/// project's extent (a pole in the wrong hemisphere, a degree-wide buffer) and
/// is refused before the first provider round, never read as thousands of
/// sequential ones.
const MAX_CELL_TILES: usize = 400;

/// One gap cell as the tiles an acquisition asks for, none larger than a
/// provider read may be (`MAX_ENVELOPE_M2`, the bound the kernel's plan and a
/// cloud-resident read keep). A small cell is itself; a cell needing more than
/// `MAX_CELL_TILES` is refused.
fn tiles(cell: [f64; 4]) -> Result<Vec<[f64; 4]>, Failure> {
    let area = bounds_area_m2(cell).map_err(crate::store)?;
    let mut side = (area / MAX_ENVELOPE_M2).sqrt().ceil().max(1.0) as usize;
    loop {
        let [w, s, e, n] = cell;
        if side.saturating_mul(side) > MAX_CELL_TILES {
            return Err(Failure::AcquisitionFailed(format!(
                "the missing area [{w:.4}, {s:.4}, {e:.4}, {n:.4}] is {:.0} km², more than the {MAX_CELL_TILES} reads of {:.0} km² one cell may take; no project extent is this wide — narrow the seed to the transformers it is for, or correct the design extent that reaches this far",
                area / 1e6,
                MAX_ENVELOPE_M2 / 1e6,
            )));
        }
        let (dx, dy) = ((e - w) / side as f64, (n - s) / side as f64);
        let mut out = Vec::with_capacity(side * side);
        for row in 0..side {
            for column in 0..side {
                // The last row and column end exactly on the cell's own edge,
                // so the tiles join back to the cell with no sliver.
                let east = if column + 1 == side {
                    e
                } else {
                    w + dx * (column + 1) as f64
                };
                let north = if row + 1 == side {
                    n
                } else {
                    s + dy * (row + 1) as f64
                };
                out.push([w + dx * column as f64, s + dy * row as f64, east, north]);
            }
        }
        let mut largest = 0.0_f64;
        for tile in &out {
            largest = largest.max(bounds_area_m2(*tile).map_err(crate::store)?);
        }
        if largest <= MAX_ENVELOPE_M2 {
            return Ok(out);
        }
        side += 1;
    }
}

/// A cell's four quarters, for a national-index read that truncated.
fn quadrants(cell: [f64; 4]) -> [[f64; 4]; 4] {
    let [w, s, e, n] = cell;
    let (x, y) = ((w + e) / 2.0, (s + n) / 2.0);
    [[w, s, x, y], [x, s, e, y], [w, y, x, n], [x, y, e, n]]
}

/// A project subset of an installed national bundle: rows that intersect the
/// cell, read from the machine's own index. No provider.
///
/// The index answers exactly the rule the room replaces the cell's rows under
/// (`acquisition_selects` for the reference cache is exact intersection), so a
/// commit deletes only rows this page puts back — rows an earlier seed holds
/// included. `None` when the index truncated the read: the caller splits the cell.
fn acquire_catalog(
    root: &Path,
    resource: &ReferenceResource,
    area: &Value,
) -> Result<Option<Acquisition>, Failure> {
    if !installed(root, &resource.identity())? {
        return Err(Failure::BundleUnavailable(format!(
            "{} ({}) is not installed on this machine",
            resource.label, resource.layer
        )));
    }
    let result = ds_layer_store::reference_cache::execute_at(
        root,
        json!({"action": "query", "identity": resource.identity(), "area": area, "limit": CATALOG_QUERY_LIMIT}),
    )
    .map_err(crate::store)?;
    if result["truncated"] == Value::Bool(true) {
        return Ok(None);
    }
    if result["cache"] != "local" || result["version"] != resource.version {
        return Err(Failure::AcquisitionFailed(
            "the project subset came from a different national seed version".into(),
        ));
    }
    let features = result["features"].as_array().cloned().unwrap_or_default();
    Ok(Some(Acquisition {
        source_version: resource.version.clone(),
        source_receipt: json!({
            "kind": "national_seed_subset",
            "resource_id": resource.id,
            "resource_version": resource.version,
            "bundle_sha256": resource.bundle_sha256,
            "expanded_sha256": resource.expanded_sha256,
            "area": area,
            "feature_count": features.len(),
        }),
        features,
    }))
}

/// Ensure the project holds this dataset over the coverage its design needs.
///
/// `extents` is the roster the plan fuses (every transformer, or the one being
/// printed); `focus` narrows acquisition to the clusters holding the named
/// transformers so a print pays for its own transformer only.
#[allow(clippy::too_many_arguments)]
pub fn ensure(
    root: &Path,
    scope: &Scope,
    dataset: &Dataset,
    source: &DeclaredSource,
    policy: &BufferPolicy,
    extents: &[Extent],
    focus: &[String],
    mut mode: Mode<'_, '_>,
) -> Result<EnsureOutcome, Failure> {
    let plan = plan_focused(scope, dataset, policy, extents, focus).map_err(crate::store)?;
    let mut warnings = Vec::new();
    if !plan.skipped.is_empty() {
        let shown: Vec<&str> = plan.skipped.iter().take(5).map(String::as_str).collect();
        warnings.push(format!(
            "{} transformer(s) have no design extent yet and were not covered: {}{}",
            plan.skipped.len(),
            shown.join(", "),
            if plan.skipped.len() > 5 { "…" } else { "" }
        ));
    }
    let room = Room {
        root,
        scope,
        key: dataset_key(scope, dataset),
    };
    if let (DeclaredSource::Catalog(resource), Mode::Acquire(hosts)) = (source, &mut mode) {
        // A national subset cannot be extracted before its seed is installed,
        // and installing one is a download: it happens only inside the
        // explicit acquiring stage, never during a read.
        if !installed(root, &resource.identity())? {
            let receipt = resource.bundle().ok_or_else(|| {
                Failure::BundleUnavailable(format!(
                    "{} has no verified published bundle; publish it before seeding",
                    resource.label
                ))
            })?;
            install_bundle(root, &receipt, &mut *hosts.fetch)?;
        }
    }
    let mut status = if mode.acquires() {
        room.open(dataset, policy)?
    } else {
        room.status()?
    };
    if plan.empty {
        warnings
            .push("This project has no design extent yet, so there is nothing to acquire.".into());
        return Ok(EnsureOutcome {
            dataset_key: room.key,
            clusters: plan.clusters.len(),
            covered: true,
            acquired: 0,
            skipped: 0,
            features: status["feature_count"].as_i64().unwrap_or(0),
            coverage_geometry: plan.coverage_geometry,
            coverage: plan.coverage,
            status,
            warnings,
        });
    }
    let mut assessment = assess(&completed(&status)?, &plan.coverage).map_err(crate::store)?;
    let (mut acquired, mut skipped) = (0_usize, 0_usize);
    if !assessment.covered {
        if let Mode::Acquire(hosts) = &mut mode {
            // A gap is acquired in tiles no provider read may exceed: a whole
            // project's overview is one district-sized gap, and asking for it
            // in one read is what the provider refuses and the national index
            // truncates. Each tile is its own ticket and commit; the room's
            // exact coverage joins them again.
            let mut pending: Vec<([f64; 4], u32)> = Vec::new();
            for cell in assessment.gaps.cells.clone() {
                pending.extend(tiles(cell)?.into_iter().rev().map(|tile| (tile, 0)));
            }
            while let Some((cell, depth)) = pending.pop() {
                let cells = Coverage { cells: vec![cell] };
                let area = polygon(cell);
                // A national subset is read from this machine's own index
                // first: a read the index truncates is split, never stored.
                let local = match source {
                    DeclaredSource::Catalog(resource) => {
                        Some(acquire_catalog(root, resource, &area))
                    }
                    DeclaredSource::Buildings
                    | DeclaredSource::Contours(_)
                    | DeclaredSource::Cloud(_) => None,
                };
                if let (Some(Ok(None)), DeclaredSource::Catalog(resource)) = (&local, source) {
                    if depth >= MAX_SPLIT_DEPTH {
                        return Err(Failure::AcquisitionFailed(format!(
                            "the national-cache query for {} still exceeds {CATALOG_QUERY_LIMIT} features in a {:.4}° cell after {MAX_SPLIT_DEPTH} splits",
                            resource.layer,
                            cell[2] - cell[0],
                        )));
                    }
                    pending.extend(quadrants(cell).into_iter().map(|part| (part, depth + 1)));
                    continue;
                }
                let held_version = status["source_version"].as_str().unwrap_or("").to_owned();
                let digest =
                    query_digest(scope, &at_version(dataset, &held_version), policy, &cells);
                let ticket = room.begin(&digest, &area, &cells, &held_version)?;
                if ticket["accepted"] != Value::Bool(true) {
                    // Another process owns this exact work, or the room already
                    // completed it. Declining is the saving.
                    skipped += 1;
                    status = room.status()?;
                    continue;
                }
                let ticket = ticket["ticket"].as_str().unwrap_or("").to_owned();
                let page = match local {
                    Some(read) => read.map(|page| page.expect("a truncated read was split above")),
                    None => hosts.provider.acquire(dataset, &area),
                };
                match page {
                    Ok(page) => {
                        status = room.commit(&digest, &ticket, &cells, &page, &held_version)?;
                        acquired += 1;
                    }
                    Err(error) => {
                        // Record the failure without touching held data. A
                        // partial acquisition must never read as ready.
                        let _ = room.fail(&digest, &ticket, error.message());
                        return Err(match error {
                            Failure::ProviderUnavailable(m) => Failure::ProviderUnavailable(m),
                            Failure::BundleUnavailable(m) => Failure::BundleUnavailable(m),
                            other => Failure::AcquisitionFailed(other.message().to_owned()),
                        });
                    }
                }
            }
            assessment = assess(&completed(&status)?, &plan.coverage).map_err(crate::store)?;
            if !assessment.covered {
                warnings.push(
                    "Another acquisition is still preparing this project area. Coverage is not complete yet."
                        .into(),
                );
            }
        }
    }
    Ok(EnsureOutcome {
        dataset_key: room.key,
        clusters: plan.clusters.len(),
        covered: assessment.covered,
        acquired,
        skipped,
        features: status["feature_count"].as_i64().unwrap_or(0),
        coverage_geometry: plan.coverage_geometry,
        coverage: plan.coverage,
        status,
        warnings,
    })
}

/// Read every held feature for one area from the room, paged and fenced on
/// the room's generation. Performs no acquisition, by design.
pub fn read_features(
    root: &Path,
    scope: &Scope,
    dataset: &Dataset,
    area: &Value,
) -> Result<(Vec<Value>, String), Failure> {
    let room = Room {
        root,
        scope,
        key: dataset_key(scope, dataset),
    };
    let status = room.status()?;
    if status["exists"] != Value::Bool(true) {
        return Err(Failure::NotHeld(format!(
            "{} has not been seeded for this project",
            dataset.id
        )));
    }
    let mut features = Vec::new();
    let mut after_id: Option<i64> = None;
    let mut generation: Option<i64> = None;
    let mut version = String::new();
    loop {
        let mut request = json!({"action": "query", "area": area, "limit": 50_000});
        if let (Some(after), Some(generation)) = (after_id, generation) {
            request["after_id"] = json!(after);
            request["generation"] = json!(generation);
        }
        let page = room.call(request).map_err(|error| match error {
            Failure::Store(message) if message.contains("beyond completed coverage") => {
                Failure::NotHeld(format!("{}: {message}", dataset.id))
            }
            other => other,
        })?;
        let page_generation = page["generation"].as_i64().unwrap_or(-1);
        let page_version = page["source_version"].as_str().unwrap_or("").to_owned();
        if let Some(seen) = generation {
            if seen != page_generation || version != page_version {
                return Err(Failure::Store(
                    "project reference data changed during print capture; repeat this read".into(),
                ));
            }
        }
        generation = Some(page_generation);
        version = page_version;
        if let Some(rows) = page["features"].as_array() {
            features.extend(rows.iter().cloned());
        }
        if page["truncated"] != Value::Bool(true) {
            break;
        }
        let next = page["next_after_id"].as_i64();
        match next {
            Some(next) if next > after_id.unwrap_or(-1) => after_id = Some(next),
            _ => {
                return Err(Failure::Store(
                    "project reference data did not advance its page cursor".into(),
                ))
            }
        }
    }
    Ok((features, version))
}
