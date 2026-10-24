//! Seed once per project: read the project's shared revision chain, replay
//! what this room lacks, acquire only what nobody published, and publish that
//! delta as the next revision.
//!
//! The loop around [`crate::ensure`] is unchanged; this module decides what
//! its provider answers. Three doors are lent by the host and none opens a
//! socket here:
//!
//! * [`RevisionHost`] — the compute artifact door: read the project's head,
//!   download one verified output, publish one revision fenced on the
//!   manifest it planned against. Brain authorizes and stores; a concurrent
//!   publication answers [`Publish::Stale`] and this seed re-plans.
//! * [`SourceHost`] — Core's own acquisition: publisher tiles for footprints,
//!   surface samples for contours, converted or derived offline by the
//!   Reporter engine into one complete page per cell.
//! * the room store (`ds-layer-store`), through `ensure`'s fenced verbs.
//!
//! What it guarantees: a second member's seed of covered ground performs no
//! source acquisition; extent growth acquires and publishes only the new
//! cells; a publisher or surface version change is reported and re-acquired
//! only by an explicit refresh; and a held room keeps answering offline.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};

use ds_command_kernel::project_dataset_cache::{
    acquisition_selects, feature_key, BufferPolicy, Coverage, Dataset, Extent, Scope,
};
use ds_command_kernel::project_dataset_revisions::{self as chain, Draft, Manifest};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::declared::DeclaredSource;
use crate::ensure::{self, Acquisition, EnsureOutcome, Hosts, Mode, Provider};
use crate::Failure;

/// How often one seed re-plans after a concurrent publication before it
/// stops and says so.
pub const MAX_STALE_RETRIES: u32 = 3;
/// The largest single page a prepared provider reads back.
const MAX_PAGE_BYTES: u64 = 512 * 1024 * 1024;

/// Why a shared seed stopped. A project failure keeps the crate's own codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SharedFailure {
    Project(Failure),
    /// The published chain is malformed or its bytes differ from their pins.
    Invalid(String),
    /// Other members kept publishing between every re-plan.
    Stale(String),
    /// The door refused this principal's publication or read.
    Refused(String),
    /// A host-owned typed refusal (an engine or surface the host declares).
    Host {
        code: &'static str,
        message: String,
    },
}

impl SharedFailure {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Project(failure) => failure.code(),
            Self::Invalid(_) => "project_dataset_revision_invalid",
            Self::Stale(_) => "project_dataset_revision_stale",
            Self::Refused(_) => "project_dataset_publication_refused",
            Self::Host { code, .. } => code,
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Self::Project(failure) => failure.message(),
            Self::Invalid(m) | Self::Stale(m) | Self::Refused(m) => m,
            Self::Host { message, .. } => message,
        }
    }
}

impl From<Failure> for SharedFailure {
    fn from(failure: Failure) -> Self {
        Self::Project(failure)
    }
}

impl std::fmt::Display for SharedFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

/// One project's shared head for one dataset on the compute artifact door.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Head {
    pub project: String,
    pub operation: String,
    pub variant: String,
}

impl Head {
    pub fn of(project: &str, dataset: &Dataset) -> Result<Self, SharedFailure> {
        Ok(Self {
            project: project.to_owned(),
            operation: chain::head_operation(dataset).map_err(SharedFailure::Invalid)?,
            variant: chain::head_variant(dataset),
        })
    }
}

/// One output a published head declares.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    pub output_id: String,
    pub sha256: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Published {
    pub head_revision: u64,
    pub outputs: Vec<Output>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublishOutput {
    pub output_id: String,
    pub format: &'static str,
    pub content_type: &'static str,
    pub sha256: String,
    pub size_bytes: u64,
    pub path: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Publication {
    pub head: Head,
    /// The digest of the manifest this revision was planned against.
    pub base_fingerprint: String,
    /// Unique per attempt: a re-plan is new work, never a replay.
    pub client_run_id: String,
    pub outputs: Vec<PublishOutput>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Publish {
    Published {
        head_revision: u64,
    },
    /// Another revision landed after this one was planned.
    Stale {
        reason: String,
    },
}

/// The compute artifact door, lent by the host.
pub trait RevisionHost {
    /// The project's current head for this dataset, or `None` when nothing
    /// has been published.
    fn read(&mut self, head: &Head) -> Result<Option<Published>, SharedFailure>;
    /// One output's bytes into the fresh file `dest`, verified against its
    /// declared digest and size.
    fn download(&mut self, head: &Head, output: &Output, dest: &Path) -> Result<(), SharedFailure>;
    /// Open, transfer and finalize one revision.
    fn publish(&mut self, publication: &Publication) -> Result<Publish, SharedFailure>;
}

/// Complete pages for exactly the cells asked, from the acquisition owner.
#[derive(Clone, Debug, PartialEq)]
pub struct SourceBatch {
    pub source_version: String,
    pub receipt: Value,
    /// One GeoJSON-lines file per cell, holding every row the dataset's rule
    /// selects for that cell.
    pub pages: Vec<([f64; 4], PathBuf)>,
    /// Cells asked that the source has nothing to answer with (a surface
    /// that does not reach them). Every asked cell is in `pages` or here.
    /// They are never committed or published; the next seed asks again.
    pub uncovered: Vec<[f64; 4]>,
}

/// Core's own acquisition for a derived dataset, lent by the host.
pub trait SourceHost {
    /// The version acquisition would read now, known without contacting
    /// anything. Held rows from another version report as stale.
    fn current_version(&self, dataset: &Dataset) -> Option<String>;
    /// Acquire and convert exactly these cells. Called only when cells
    /// remain after the shared chain is replayed.
    fn acquire(
        &mut self,
        dataset: &Dataset,
        cells: &[[f64; 4]],
        scratch: &Path,
    ) -> Result<SourceBatch, SharedFailure>;
}

pub struct SharedSeed<'h> {
    pub revisions: &'h mut dyn RevisionHost,
    pub source: &'h mut dyn SourceHost,
    /// A private directory for this seed's transfers; the caller removes it.
    pub scratch: &'h Path,
    /// A run identity, unique per invocation.
    pub run_id: &'h str,
}

#[derive(Debug, Clone)]
pub struct SharedOutcome {
    pub ensure: EnsureOutcome,
    /// The project's head revision this seed last observed or produced.
    pub head_revision: Option<u64>,
    /// Shared revisions replayed into this room by this call.
    pub replayed: Vec<u32>,
    /// Cells acquired from the publisher or surface by this call.
    pub source_cells: usize,
    pub published: Option<Value>,
    pub stale_retries: u32,
    pub chain: Option<Value>,
    pub freshness: Option<Value>,
    /// Whether the room completed any of the planned coverage, now or
    /// before: false with `ensure.uncovered` set means the source reaches
    /// none of the project.
    pub answered: bool,
}

struct Chain {
    manifest: Manifest,
    bytes: Vec<u8>,
    head_revision: u64,
}

fn invalid(message: impl Into<String>) -> SharedFailure {
    SharedFailure::Invalid(message.into())
}

fn sha_file(path: &Path) -> Result<(String, u64), SharedFailure> {
    let mut file = File::open(path).map_err(|error| Failure::Store(error.to_string()))?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0_u8; 1 << 16];
    let mut bytes = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| Failure::Store(error.to_string()))?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        bytes += read as u64;
    }
    Ok((format!("{:x}", hasher.finalize()), bytes))
}

/// Download one output and prove its bytes here, whatever the host checked.
fn fetch(
    host: &mut dyn RevisionHost,
    head: &Head,
    output: &Output,
    dest: &Path,
) -> Result<(), SharedFailure> {
    if dest.exists() {
        std::fs::remove_file(dest).map_err(|error| Failure::Store(error.to_string()))?;
    }
    host.download(head, output, dest)?;
    let (digest, bytes) = sha_file(dest)?;
    if digest != output.sha256 || bytes != output.size_bytes {
        return Err(invalid(format!(
            "published output {} is not its pinned bytes ({bytes} bytes, sha256 {digest})",
            output.output_id
        )));
    }
    Ok(())
}

fn read_chain(
    host: &mut dyn RevisionHost,
    head: &Head,
    dataset: &Dataset,
    scratch: &Path,
) -> Result<Option<Chain>, SharedFailure> {
    let Some(published) = host.read(head)? else {
        return Ok(None);
    };
    let manifest_output = published
        .outputs
        .iter()
        .find(|output| output.output_id == chain::MANIFEST_OUTPUT)
        .ok_or_else(|| invalid("the published project dataset head declares no manifest"))?;
    let path = scratch.join(format!("manifest-{}.json", published.head_revision));
    fetch(host, head, manifest_output, &path)?;
    let bytes = std::fs::read(&path).map_err(|error| Failure::Store(error.to_string()))?;
    let manifest = chain::decode(&bytes, &head.project, dataset).map_err(SharedFailure::Invalid)?;
    for revision in &manifest.revisions {
        let declared = published
            .outputs
            .iter()
            .find(|output| output.output_id == revision.output_id);
        if declared.is_none_or(|output| {
            output.sha256 != revision.sha256 || output.size_bytes != revision.size_bytes
        }) {
            return Err(invalid(format!(
                "published revision {} differs from the output its head declares",
                revision.revision
            )));
        }
    }
    Ok(Some(Chain {
        manifest,
        bytes,
        head_revision: published.head_revision,
    }))
}

/// Every feature one revision's verified delta holds.
fn read_delta(path: &Path) -> Result<Vec<Value>, SharedFailure> {
    let file = File::open(path).map_err(|error| Failure::Store(error.to_string()))?;
    let mut reader = BufReader::new(flate2::read::GzDecoder::new(file).take(MAX_PAGE_BYTES + 1));
    let mut features = Vec::new();
    let mut line = String::new();
    let mut total = 0_u64;
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| invalid(format!("a published delta is not readable gzip: {error}")))?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > MAX_PAGE_BYTES {
            return Err(invalid("a published delta exceeds its expanded bound"));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let feature: Value = serde_json::from_str(trimmed)
            .map_err(|_| invalid("a published delta holds a row that is not JSON"))?;
        if feature["type"] != "Feature" {
            return Err(invalid(
                "a published delta holds a row that is not a Feature",
            ));
        }
        features.push(feature);
    }
    Ok(features)
}

fn polygon(cell: [f64; 4]) -> Value {
    let [w, s, e, n] = cell;
    json!({"type": "Polygon", "coordinates": [[[w, s], [e, s], [e, n], [w, n], [w, s]]]})
}

/// Replay every published revision this room has not applied, in order.
#[allow(clippy::too_many_arguments)]
fn replay(
    root: &Path,
    scope: &Scope,
    dataset: &Dataset,
    policy: &BufferPolicy,
    head: &Head,
    shared: &mut SharedSeed<'_>,
    chain: &Chain,
) -> Result<Vec<u32>, SharedFailure> {
    let applied = ensure::applied_revisions(root, scope, dataset, &head.variant)?;
    let manifest_sha = chain::base_fingerprint(Some(&chain.bytes));
    let mut replayed = Vec::new();
    for revision in chain::pending(&chain.manifest, &applied) {
        let path = shared
            .scratch
            .join(format!("{}.geojsonl.gz", revision.output_id));
        let output = Output {
            output_id: revision.output_id.clone(),
            sha256: revision.sha256.clone(),
            size_bytes: revision.size_bytes,
        };
        fetch(&mut *shared.revisions, head, &output, &path)?;
        let features = read_delta(&path)?;
        if features.len() as u64 != revision.feature_count {
            return Err(invalid(format!(
                "published revision {} holds {} features, not the {} its manifest declares",
                revision.revision,
                features.len(),
                revision.feature_count
            )));
        }
        let mut pages = Vec::with_capacity(revision.coverage.cells.len());
        for cell in &revision.coverage.cells {
            let area = polygon(*cell);
            let mut page = Vec::new();
            for feature in &features {
                if acquisition_selects(&dataset.id, &dataset.provider, feature, &area)
                    .map_err(SharedFailure::Invalid)?
                {
                    page.push(feature.clone());
                }
            }
            pages.push((
                *cell,
                Acquisition {
                    features: page,
                    source_version: revision.source_version.clone(),
                    source_receipt: json!({
                        "kind": "project_dataset_revision",
                        "revision": revision.revision,
                        "sha256": revision.sha256,
                        "source_version": revision.source_version,
                        "cell": cell,
                    }),
                },
            ));
        }
        ensure::replace_cells(root, scope, dataset, policy, pages)?;
        ensure::mark_applied(
            root,
            scope,
            dataset,
            &head.variant,
            revision.revision,
            &manifest_sha,
        )?;
        let _ = std::fs::remove_file(&path);
        replayed.push(revision.revision);
    }
    Ok(replayed)
}

/// Pages a source batch prepared, answered cell by cell to `ensure`.
struct Prepared {
    pages: BTreeMap<String, PathBuf>,
    uncovered: std::collections::BTreeSet<String>,
    version: String,
    receipt: Value,
}

fn cell_key(cell: [f64; 4]) -> String {
    format!("{:?}", cell)
}

fn bounds(area: &Value) -> Option<[f64; 4]> {
    let ring = area["coordinates"][0].as_array()?;
    let point = |index: usize| -> Option<[f64; 2]> {
        let position = ring.get(index)?.as_array()?;
        Some([position.first()?.as_f64()?, position.get(1)?.as_f64()?])
    };
    let [w, s] = point(0)?;
    let [e, n] = point(2)?;
    Some([w, s, e, n])
}

impl Provider for Prepared {
    fn uncovered(&self, cell: [f64; 4]) -> bool {
        self.uncovered.contains(&cell_key(cell))
    }

    fn acquire(&mut self, dataset: &Dataset, area: &Value) -> Result<Acquisition, Failure> {
        let cell = bounds(area)
            .ok_or_else(|| Failure::Store("an acquisition area is not a cell".into()))?;
        let path = self.pages.get(&cell_key(cell)).ok_or_else(|| {
            Failure::Store(format!(
                "{} asked for a cell its prepared acquisition did not plan",
                dataset.id
            ))
        })?;
        let file = File::open(path).map_err(|error| Failure::Store(error.to_string()))?;
        let mut features = Vec::new();
        for line in BufReader::new(file.take(MAX_PAGE_BYTES)).lines() {
            let line = line.map_err(|error| Failure::AcquisitionFailed(error.to_string()))?;
            if line.trim().is_empty() {
                continue;
            }
            features.push(serde_json::from_str(&line).map_err(|_| {
                Failure::AcquisitionFailed("an engine page row is not JSON".into())
            })?);
        }
        let mut receipt = self.receipt.clone();
        if let Some(object) = receipt.as_object_mut() {
            object.insert("cell".into(), json!(cell));
        }
        Ok(Acquisition {
            features,
            source_version: self.version.clone(),
            source_receipt: receipt,
        })
    }
}

/// A refusing bundle fetch: derived datasets never install a national bundle.
fn no_bundle(_: &crate::BundleReceipt, _: &Path) -> Result<(), String> {
    Err("derived project datasets install no national bundle".into())
}

/// Build the next revision's delta from what the room holds for `delta`.
fn build_delta(
    root: &Path,
    scope: &Scope,
    dataset: &Dataset,
    delta: &Coverage,
    path: &Path,
) -> Result<(u64, String, u64), SharedFailure> {
    let mut rows: BTreeMap<String, String> = BTreeMap::new();
    for cell in &delta.cells {
        let (features, _) = ensure::read_selected(root, scope, dataset, &polygon(*cell))?;
        for feature in features {
            rows.insert(feature_key(&feature), feature.to_string());
        }
    }
    if rows.len() as u64 > chain::MAX_DELTA_FEATURES {
        return Err(SharedFailure::Project(Failure::TooLarge(format!(
            "this revision would publish {} features, over the {} one revision admits; seed fewer transformers at a time",
            rows.len(),
            chain::MAX_DELTA_FEATURES
        ))));
    }
    // Deterministic bytes: rows in feature-key order, no gzip timestamp.
    let file = File::create(path).map_err(|error| Failure::Store(error.to_string()))?;
    let mut encoder = flate2::GzBuilder::new()
        .mtime(0)
        .write(file, flate2::Compression::default());
    for row in rows.values() {
        encoder
            .write_all(row.as_bytes())
            .and_then(|()| encoder.write_all(b"\n"))
            .map_err(|error| Failure::Store(error.to_string()))?;
    }
    encoder
        .finish()
        .and_then(|file| file.sync_all())
        .map_err(|error| Failure::Store(error.to_string()))?;
    let (digest, bytes) = sha_file(path)?;
    if bytes > chain::MAX_DELTA_BYTES {
        return Err(SharedFailure::Project(Failure::TooLarge(format!(
            "this revision's delta is {bytes} bytes, over the door's {} byte bound; seed fewer transformers at a time",
            chain::MAX_DELTA_BYTES
        ))));
    }
    Ok((rows.len() as u64, digest, bytes))
}

fn chain_text(version: &str) -> Result<(), SharedFailure> {
    if version.is_empty() || version.len() > 512 || version.chars().any(char::is_control) {
        return Err(SharedFailure::Project(Failure::AcquisitionFailed(
            "the acquisition owner named no bounded source version".into(),
        )));
    }
    Ok(())
}

/// A revision receipt is provenance, never payload: past its bound the
/// source detail is replaced by its digest rather than trimmed.
fn bounded_receipt(mut receipt: Value) -> Value {
    let size = serde_json::to_vec(&receipt)
        .map(|bytes| bytes.len())
        .unwrap_or(usize::MAX);
    if size > ds_command_kernel::project_dataset_cache::MAX_SOURCE_RECEIPT_BYTES / 2 {
        let digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&receipt["source"]).unwrap_or_default())
        );
        receipt["source"] = json!({"sha256": digest, "omitted": "the source receipt exceeded half the revision receipt bound"});
    }
    receipt
}

fn version_of(status: &Value) -> (String, Value) {
    let held = status["source_version"].as_str().unwrap_or("").to_owned();
    let versions = status["source_versions"].clone();
    if held.is_empty() {
        // Rows from more than one version are published as such: freshness
        // then reports them, and a refresh replaces them with one.
        ("mixed".to_owned(), versions)
    } else {
        (held, versions)
    }
}

/// Seed one derived dataset for the project through its shared revisions.
///
/// `refresh` re-acquires every planned cell from the source, whatever the
/// room or the chain holds, and publishes the result as a refresh revision.
#[allow(clippy::too_many_arguments)]
pub fn seed(
    root: &Path,
    scope: &Scope,
    dataset: &Dataset,
    source: &DeclaredSource,
    policy: &BufferPolicy,
    extents: &[Extent],
    focus: &[String],
    refresh: bool,
    shared: &mut SharedSeed<'_>,
) -> Result<SharedOutcome, SharedFailure> {
    if !matches!(
        source,
        DeclaredSource::Buildings | DeclaredSource::Contours(_)
    ) {
        return Err(SharedFailure::Project(Failure::Unsupported(format!(
            "{} is not seeded through shared project revisions",
            dataset.id
        ))));
    }
    let head = Head::of(&scope.project, dataset)?;
    let mut current = read_chain(&mut *shared.revisions, &head, dataset, shared.scratch)?;
    let mut replayed = Vec::new();
    if !refresh {
        if let Some(chain) = &current {
            replayed = replay(root, scope, dataset, policy, &head, shared, chain)?;
        }
    }
    let available = shared.source.current_version(dataset);
    if let Some(version) = available.as_deref() {
        ensure::observe_available(root, scope, dataset, policy, version)?;
    }
    let (wanted, cells) =
        ensure::planned_cells(root, scope, dataset, policy, extents, focus, refresh)?;
    let mut prepared = Prepared {
        pages: BTreeMap::new(),
        uncovered: std::collections::BTreeSet::new(),
        version: String::new(),
        receipt: json!({}),
    };
    if !cells.is_empty() {
        let batch = shared.source.acquire(dataset, &cells, shared.scratch)?;
        chain_text(&batch.source_version)?;
        prepared.version = batch.source_version;
        prepared.receipt = batch.receipt;
        for (cell, path) in batch.pages {
            prepared.pages.insert(cell_key(cell), path);
        }
        for cell in batch.uncovered {
            prepared.uncovered.insert(cell_key(cell));
        }
        if cells.iter().any(|cell| {
            let key = cell_key(*cell);
            !prepared.pages.contains_key(&key) && !prepared.uncovered.contains(&key)
        }) {
            return Err(SharedFailure::Project(Failure::AcquisitionFailed(
                "the acquisition owner answered fewer cells than were asked".into(),
            )));
        }
    }
    let mut fetch = no_bundle;
    let mut hosts = Hosts {
        provider: &mut prepared,
        fetch: &mut fetch,
    };
    let outcome = if refresh {
        crate::refresh(
            root, scope, dataset, source, policy, extents, focus, &mut hosts,
        )?
    } else {
        crate::ensure(
            root,
            scope,
            dataset,
            source,
            policy,
            extents,
            focus,
            Mode::Acquire(&mut hosts),
        )?
    };
    let mut published = None;
    let mut retries = 0_u32;
    let mut head_revision = current.as_ref().map(|chain| chain.head_revision);
    loop {
        let status = ensure::room_status(root, scope, dataset)?;
        let held: Coverage = serde_json::from_value(status["completed"].clone())
            .map_err(|error| Failure::Store(error.to_string()))?;
        let chain_coverage = match &current {
            Some(chain) => chain::coverage(&chain.manifest).map_err(SharedFailure::Invalid)?,
            None => Coverage::default(),
        };
        let delta = chain::delta(&wanted, &held, &chain_coverage, refresh)
            .map_err(|error| SharedFailure::Project(Failure::Store(error)))?;
        if delta.is_empty() {
            break;
        }
        let number = current
            .as_ref()
            .map_or(1, |chain| chain.manifest.revisions.len() as u32 + 1);
        let delta_path = shared
            .scratch
            .join(format!("publish-{}.geojsonl.gz", chain::output_id(number)));
        let (features, digest, bytes) = build_delta(root, scope, dataset, &delta, &delta_path)?;
        let (version, versions) = version_of(&status);
        let receipt = json!({
            "kind": "project_dataset_revision",
            "source": prepared.receipt,
            "room_source_versions": versions,
            "cells": delta.cells.len(),
            "conversion": "rows selected from the publishing member's project room under the dataset's acquisition rule",
        });
        let receipt = bounded_receipt(receipt);
        let manifest = chain::next(
            current.as_ref().map(|chain| &chain.manifest),
            &scope.project,
            dataset,
            Draft {
                coverage: delta.clone(),
                source_version: version,
                source_receipt: receipt,
                feature_count: features,
                sha256: digest.clone(),
                size_bytes: bytes,
                refresh,
            },
        )
        .map_err(SharedFailure::Invalid)?;
        let manifest_bytes = chain::encode(&manifest).map_err(SharedFailure::Invalid)?;
        let manifest_path = shared
            .scratch
            .join(format!("publish-manifest-{number}.json"));
        std::fs::write(&manifest_path, &manifest_bytes)
            .map_err(|error| Failure::Store(error.to_string()))?;
        let manifest_sha = chain::base_fingerprint(Some(&manifest_bytes));
        let publication = Publication {
            head: head.clone(),
            base_fingerprint: chain::base_fingerprint(
                current.as_ref().map(|chain| chain.bytes.as_slice()),
            ),
            client_run_id: format!("{}-{number}-{retries}", shared.run_id),
            outputs: vec![
                PublishOutput {
                    output_id: chain::MANIFEST_OUTPUT.into(),
                    format: chain::MANIFEST_FORMAT,
                    content_type: chain::MANIFEST_CONTENT_TYPE,
                    sha256: manifest_sha.clone(),
                    size_bytes: manifest_bytes.len() as u64,
                    path: manifest_path,
                },
                PublishOutput {
                    output_id: chain::output_id(number),
                    format: chain::DELTA_FORMAT,
                    content_type: chain::DELTA_CONTENT_TYPE,
                    sha256: digest.clone(),
                    size_bytes: bytes,
                    path: delta_path,
                },
            ],
        };
        match shared.revisions.publish(&publication)? {
            Publish::Published {
                head_revision: next,
            } => {
                ensure::mark_applied(root, scope, dataset, &head.variant, number, &manifest_sha)?;
                head_revision = Some(next);
                published = Some(json!({
                    "revision": number,
                    "head_revision": next,
                    "cells": delta.cells.len(),
                    "features": features,
                    "sha256": digest,
                    "refresh": refresh,
                }));
                current = Some(Chain {
                    manifest,
                    bytes: manifest_bytes,
                    head_revision: next,
                });
                break;
            }
            Publish::Stale { reason } => {
                retries += 1;
                if retries > MAX_STALE_RETRIES {
                    return Err(SharedFailure::Stale(format!(
                        "other members published {retries} times while this seed re-planned ({reason}); held data is kept — seed again"
                    )));
                }
                // Re-plan against what landed: replay it here, then publish
                // only what it still lacks. A refresh republishes its cells.
                current = read_chain(&mut *shared.revisions, &head, dataset, shared.scratch)?;
                head_revision = current.as_ref().map(|chain| chain.head_revision);
                if !refresh {
                    if let Some(chain) = &current {
                        replayed
                            .extend(replay(root, scope, dataset, policy, &head, shared, chain)?);
                    }
                }
            }
        }
    }
    let (chain_summary, freshness) = match &current {
        Some(chain) => {
            let summary = chain::summary(&chain.manifest);
            let freshness = match available.as_deref() {
                Some(version) => {
                    let report = chain::chain_freshness(&chain.manifest, version)
                        .map_err(SharedFailure::Invalid)?;
                    Some(json!({
                        "current_version": version,
                        "fresh": report.fresh,
                        "stale_cells": report.stale_count,
                        "versions": report.versions,
                    }))
                }
                None => None,
            };
            (Some(summary), freshness)
        }
        None => (None, None),
    };
    let mut settled = outcome;
    settled.status = ensure::room_status(root, scope, dataset)?;
    settled.features = settled.status["feature_count"]
        .as_i64()
        .unwrap_or(settled.features);
    let held: Coverage = serde_json::from_value(settled.status["completed"].clone())
        .map_err(|error| Failure::Store(error.to_string()))?;
    let answered = !chain::delta(&wanted, &held, &Coverage::default(), true)
        .map_err(|error| SharedFailure::Project(Failure::Store(error)))?
        .is_empty();
    Ok(SharedOutcome {
        ensure: settled,
        head_revision,
        replayed,
        source_cells: cells.len(),
        published,
        stale_retries: retries,
        chain: chain_summary,
        freshness,
        answered,
    })
}

#[cfg(test)]
#[path = "revisions_tests.rs"]
mod tests;
