//! Pinned publisher objects -> one retained native analytical source.
//!
//! This crate verifies the operator's byte pins, stages the objects and
//! writes the canonical analytical artifact; the normalization of publisher
//! rows is the Reporter engine's (`prepare-publisher-footprints`), reached
//! through the converter the host lends. No socket, credential, provider
//! substitution or inferred coverage exists here.
use crate::local_source::{DigestAlgorithm, Kind, Origin, ProducerSource, Source};
use ds_command_kernel::project_dataset_cache::{
    self as policy, Coverage, Scope, BUILDINGS_DATASET_ID, REFERENCE_CACHE_PROVIDER,
};
use ds_command_kernel::project_dataset_revisions::tasks::{
    FootprintsRequest, FootprintsResult, Publisher, PublisherObject, FOOTPRINTS_REQUEST,
    FOOTPRINTS_RESULT, FOOTPRINT_CONVERSION,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
};

const MAX_COMPRESSED: u64 = 128 * 1024 * 1024;
const MAX_FEATURES: usize = 500_000;
const BATCH: usize = 512;
const MAX_PAGE_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Resource {
    /// Publisher tile/partition identity, independent of local filename.
    pub resource_id: String,
    pub path: PathBuf,
    pub sha256: String,
    /// Explicit publisher object coverage, not a feature-derived extent.
    pub coverage: Coverage,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub schema: String,
    pub project_id: String,
    pub asset_id: String,
    pub revision: String,
    pub actual_source: Origin,
    pub publisher_revision: String,
    pub coverage: Coverage,
    pub resources: Vec<Resource>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Invalid(String),
    Required(String),
    Malformed(String),
    Unsupported(String),
    TooLarge(String),
    Store(String),
    /// The conversion engine is missing, incompatible or refused for a
    /// reason of its own; the host names the code.
    Engine(&'static str, String),
}
impl Error {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Invalid(_) => "publisher_input_invalid",
            Self::Required(_) => "project_dataset_local_source_required",
            Self::Malformed(_) => "publisher_input_malformed",
            Self::Unsupported(_) => "publisher_geometry_unsupported",
            Self::TooLarge(_) => "publisher_input_too_large",
            Self::Store(_) => "project_dataset_store_failed",
            Self::Engine(code, _) => code,
        }
    }
    pub fn message(&self) -> &str {
        match self {
            Self::Invalid(m)
            | Self::Required(m)
            | Self::Malformed(m)
            | Self::Unsupported(m)
            | Self::TooLarge(m)
            | Self::Store(m)
            | Self::Engine(_, m) => m,
        }
    }
    /// The engine's typed refusal (`<code>: <message>`), as its own variant.
    pub fn from_engine(message: &str) -> Self {
        let (code, rest) = message.split_once(": ").unwrap_or(("", message));
        let rest = rest.to_owned();
        match code {
            "publisher_input_invalid" => Self::Invalid(rest),
            "publisher_input_malformed" => Self::Malformed(rest),
            "publisher_geometry_unsupported" => Self::Unsupported(rest),
            "publisher_input_too_large" => Self::TooLarge(rest),
            _ => Self::Engine("engine_refused", message.to_owned()),
        }
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}
impl std::error::Error for Error {}
fn store(e: impl std::fmt::Display) -> Error {
    Error::Store(e.to_string())
}
fn malformed(e: impl std::fmt::Display) -> Error {
    Error::Malformed(e.to_string())
}
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn text(v: &str) -> bool {
    !v.is_empty() && v.len() <= 200 && !v.chars().any(char::is_control)
}
fn digest(v: &str) -> bool {
    v.len() == 64
        && v.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}

impl Input {
    pub fn validate(&self, scope: &Scope) -> Result<(), Error> {
        if self.schema != "ds.publisher-footprints/v1"
            || self.project_id != scope.project
            || !text(&self.asset_id)
            || !text(&self.revision)
            || !text(&self.publisher_revision)
            || !matches!(
                self.actual_source,
                Origin::GoogleOpenBuildings | Origin::MicrosoftBuildings
            )
            || self.resources.is_empty()
            || self.resources.len() > 16
            || self.coverage.is_empty()
        {
            return Err(Error::Invalid("select one exact project/asset/revision and Google CSV.gz or Microsoft GeoJSONL.gz publisher input (1–16 objects)".into()));
        }
        self.coverage.rects().map_err(malformed)?;
        if self.coverage.cells.len() > 256 {
            return Err(Error::TooLarge(
                "publisher extraction exceeds 256 coverage cells".into(),
            ));
        }
        let mut ids = std::collections::BTreeSet::new();
        let mut available = Coverage::default();
        for r in &self.resources {
            if !text(&r.resource_id)
                || !ids.insert(&r.resource_id)
                || !r.path.is_absolute()
                || !digest(&r.sha256)
                || r.coverage.is_empty()
                || r.coverage.cells.len() > 256
            {
                return Err(Error::Invalid("each publisher object needs a unique stable id, absolute path, exact SHA-256 and explicit coverage".into()));
            }
            r.coverage.rects().map_err(malformed)?;
            available.cells.extend(&r.coverage.cells);
        }
        if !policy::assess(&available, &self.coverage)
            .map_err(malformed)?
            .covered
        {
            return Err(Error::Required("selected publisher objects do not cover the requested extract; supply the missing pinned objects explicitly".into()));
        }
        Ok(())
    }
    /// Paths are transport addresses. Source family/version, stable object ids,
    /// byte pins and extraction coverage alone identify this conversion.
    pub fn identity(&self) -> String {
        sha(self.pins().to_string().as_bytes())
    }
    fn pins(&self) -> Value {
        let mut resources: Vec<_> = self
            .resources
            .iter()
            .map(|r| json!({"resource_id":r.resource_id,"sha256":r.sha256,"coverage":r.coverage}))
            .collect();
        resources.sort_by_key(|r| r["resource_id"].as_str().unwrap_or("").to_owned());
        json!({"schema":self.schema,"project_id":self.project_id,"asset_id":self.asset_id,"revision":self.revision,
            "actual_source":self.actual_source,"publisher_revision":self.publisher_revision,"coverage":self.coverage,"resources":resources,
            "conversion":FOOTPRINT_CONVERSION})
    }
    fn publisher(&self) -> Publisher {
        if self.actual_source == Origin::GoogleOpenBuildings {
            Publisher::GoogleCsv
        } else {
            Publisher::MicrosoftGeojsonl
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Produced {
    pub source: Source,
    pub source_manifest: PathBuf,
    pub receipt: Value,
    pub reused: bool,
}

/// The engine door the host lends: one typed conversion request, answered
/// with page files and digests.
pub type Convert<'a> = dyn FnMut(&FootprintsRequest) -> Result<FootprintsResult, Error> + 'a;

/// Explicitly acquire from already admitted publisher objects. A repeated
/// unchanged acquisition consumes its retained manifest without opening the
/// original objects, the engine, or any remote freshness.
pub fn produce(
    root: &Path,
    scope: &Scope,
    input: &Input,
    convert: &mut Convert<'_>,
) -> Result<Produced, Error> {
    input.validate(scope)?;
    let destination = root.join("publisher-footprints").join(input.identity());
    let parent = destination
        .parent()
        .ok_or_else(|| store("publisher destination has no parent"))?
        .to_path_buf();
    crate::private::create_dir_all(&parent).map_err(store)?;
    // The store owns advisory locks. Concurrent consumers of the same pinned
    // conversion join its completed output rather than re-reading objects.
    let _lease =
        ds_layer_store::exclusive_lock(&destination.with_extension("lock")).map_err(store)?;
    if destination.exists() {
        return retained(&destination, scope, input);
    }
    let stage = tempfile::tempdir_in(&parent).map_err(store)?;
    let mut objects = Vec::with_capacity(input.resources.len());
    let mut compressed_bytes = 0_u64;
    let mut resources: Vec<_> = input.resources.iter().collect();
    resources.sort_by_key(|r| &r.resource_id);
    for (i, resource) in resources.into_iter().enumerate() {
        let local = stage.path().join(format!("object-{i}.gz"));
        let bytes = copy_verified(resource, &local)?;
        compressed_bytes += bytes;
        objects.push(PublisherObject {
            object_id: resource.resource_id.clone(),
            path: local,
            sha256: resource.sha256.clone(),
            size_bytes: bytes,
        });
    }
    let request = FootprintsRequest {
        schema: FOOTPRINTS_REQUEST.into(),
        // A local analytical source is indexed and queried exactly; its
        // rows are selected under that same rule.
        dataset_id: BUILDINGS_DATASET_ID.into(),
        provider: REFERENCE_CACHE_PROVIDER.into(),
        publisher: input.publisher(),
        objects,
        cells: input.coverage.cells.clone(),
        out_dir: stage.path().join("pages"),
    };
    let converted = convert(&request)?;
    let features = merge_pages(&request, &converted)?;
    let artifact = stage.path().join("footprints.parquet");
    let pins = input.pins();
    let producer_digest = sha(pins.to_string().as_bytes());
    let provenance = ds_columnar::geoparquet::Provenance {
        source_digest: Some(producer_digest.clone()),
        conversion: Some(input.identity()),
        ..Default::default()
    };
    let mut writer = ds_columnar::ConversionStream::create(&artifact, provenance).map_err(store)?;
    let mut batch = Vec::with_capacity(BATCH);
    for feature in features.into_values() {
        batch.push(feature);
        if batch.len() == BATCH {
            writer.append(&batch, &[]).map_err(malformed)?;
            batch.clear();
        }
    }
    writer.append(&batch, &[]).map_err(malformed)?;
    let conversion = writer.finish(&input.asset_id).map_err(store)?;
    crate::private::prepare_file(&artifact).map_err(store)?;
    for page in &converted.pages {
        let _ = std::fs::remove_file(&page.path);
    }
    for object in &request.objects {
        let _ = std::fs::remove_file(&object.path);
    }
    let source = Source {
        schema: "ds.local-analytical-source/v1".into(),
        project_id: input.project_id.clone(),
        asset_id: input.asset_id.clone(),
        revision: input.revision.clone(),
        actual_source: input.actual_source,
        producer_source: ProducerSource {
            revision: input.publisher_revision.clone(),
            digest_algorithm: DigestAlgorithm::Sha256,
            digest: producer_digest,
        },
        content: Kind::Footprints,
        artifact_path: destination.join("footprints.parquet"),
        artifact_sha256: ds_columnar::digest_file(&artifact).map_err(store)?,
        completed_coverage: input.coverage.clone(),
    };
    source.validate(scope).map_err(malformed)?;
    let receipt = json!({"schema":"ds.publisher-footprints-receipt/v1","pins":pins,"source":source,
        "raw_rows":converted.raw_rows,"feature_count":conversion.feature_count,"duplicates":converted.duplicates,
        "compressed_bytes":compressed_bytes,"artifact_bytes":conversion.byte_count,
        "engine_conversion":converted.conversion,"network_requests":0,"brain_compute_requests":0});
    write_json(
        &stage.path().join("source.json"),
        &serde_json::to_value(&source).map_err(store)?,
    )?;
    write_json(&stage.path().join("receipt.json"), &receipt)?;
    // Never replace an existing producer revision. The receipt is published
    // last; interrupted output cannot claim an admitted source.
    match crate::private::create_dir(&destination) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return retained(&destination, scope, input)
        }
        Err(e) => return Err(store(e)),
    }
    for name in ["footprints.parquet", "source.json", "receipt.json"] {
        std::fs::hard_link(stage.path().join(name), destination.join(name)).map_err(store)?;
    }
    Ok(Produced {
        source,
        source_manifest: destination.join("source.json"),
        receipt,
        reused: false,
    })
}

/// The engine's pages, verified against their digests and joined by stable
/// identity: one footprint once, and two different rows under one identity
/// refuse the whole conversion.
fn merge_pages(
    request: &FootprintsRequest,
    result: &FootprintsResult,
) -> Result<BTreeMap<String, Value>, Error> {
    if result.schema != FOOTPRINTS_RESULT
        || result.conversion != FOOTPRINT_CONVERSION
        || result.pages.len() != request.cells.len()
        || result
            .pages
            .iter()
            .zip(&request.cells)
            .any(|(page, cell)| page.cell != *cell || !page.path.starts_with(&request.out_dir))
    {
        return Err(Error::Engine(
            "engine_response_invalid",
            "the conversion engine answered another request or conversion".into(),
        ));
    }
    let mut features: BTreeMap<String, Value> = BTreeMap::new();
    for page in &result.pages {
        let file = File::open(&page.path).map_err(store)?;
        let mut hasher = Sha256::new();
        let mut rows = 0_u64;
        for line in BufReader::new(file.take(MAX_PAGE_BYTES)).split(b'\n') {
            let line = line.map_err(store)?;
            hasher.update(&line);
            hasher.update(b"\n");
            if line.is_empty() {
                continue;
            }
            rows += 1;
            let feature: Value = serde_json::from_slice(&line).map_err(malformed)?;
            let id = feature["id"]
                .as_str()
                .ok_or_else(|| malformed("an engine footprint has no stable identity"))?
                .to_owned();
            match features.get(&id) {
                Some(previous) if *previous != feature => {
                    return Err(malformed(
                        "publisher objects contain conflicting rows for one stable footprint id",
                    ))
                }
                Some(_) => {}
                None => {
                    features.insert(id, feature);
                }
            }
            if features.len() > MAX_FEATURES {
                return Err(Error::TooLarge(
                    "publisher extract exceeds 500000 footprints; select a smaller coverage".into(),
                ));
            }
        }
        if rows != page.features || format!("{:x}", hasher.finalize()) != page.sha256 {
            return Err(Error::Engine(
                "engine_response_invalid",
                "a conversion page differs from its declared digest or count".into(),
            ));
        }
    }
    Ok(features)
}

fn retained(dir: &Path, scope: &Scope, input: &Input) -> Result<Produced, Error> {
    let metadata = std::fs::symlink_metadata(dir).map_err(store)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(store("publisher output is not a private directory"));
    }
    let receipt = read_json(&dir.join("receipt.json"))?;
    if receipt["schema"] != "ds.publisher-footprints-receipt/v1" || receipt["pins"] != input.pins()
    {
        return Err(store(
            "retained publisher output has no matching completed receipt",
        ));
    }
    let source: Source =
        serde_json::from_value(read_json(&dir.join("source.json"))?).map_err(store)?;
    source.validate(scope).map_err(malformed)?;
    if receipt["source"] != serde_json::to_value(&source).map_err(store)?
        || source.artifact_path != dir.join("footprints.parquet")
    {
        return Err(store(
            "retained publisher manifest differs from its completed receipt",
        ));
    }
    Ok(Produced {
        source,
        source_manifest: dir.join("source.json"),
        receipt,
        reused: true,
    })
}
fn write_json(path: &Path, value: &Value) -> Result<(), Error> {
    let bytes = serde_json::to_vec_pretty(value).map_err(store)?;
    if bytes.len() > 128 * 1024 {
        return Err(Error::TooLarge(
            "publisher receipt exceeds 128 KiB; use fewer coverage cells".into(),
        ));
    }
    let mut file = crate::private::open_options()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(store)?;
    file.write_all(&bytes).map_err(store)?;
    file.sync_all().map_err(store)
}
fn read_json(path: &Path) -> Result<Value, Error> {
    let meta = std::fs::symlink_metadata(path).map_err(store)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 128 * 1024 {
        return Err(store(
            "publisher receipt must be a regular bounded JSON file",
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(store)?
        .take(128 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(store)?;
    if bytes.len() > 128 * 1024 {
        return Err(store("publisher receipt grew beyond its bound"));
    }
    serde_json::from_slice(&bytes).map_err(store)
}
fn copy_verified(resource: &Resource, destination: &Path) -> Result<u64, Error> {
    let meta = std::fs::symlink_metadata(&resource.path).map_err(|_| {
        Error::Required(format!(
            "publisher object {} is missing; explicitly acquire its pinned revision",
            resource.resource_id
        ))
    })?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.len() == 0
        || meta.len() > MAX_COMPRESSED
    {
        return Err(Error::TooLarge(
            "publisher object must be a regular nonempty gzip file of at most 128 MiB".into(),
        ));
    }
    #[cfg(test)]
    counts::read();
    let mut input = File::open(&resource.path).map_err(store)?;
    let mut output = crate::private::open_options()
        .write(true)
        .create_new(true)
        .open(destination)
        .map_err(store)?;
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = input.read(&mut buf).map_err(store)?;
        if n == 0 {
            break;
        }
        bytes += n as u64;
        if bytes > MAX_COMPRESSED {
            return Err(Error::TooLarge(
                "publisher gzip grew beyond its byte bound".into(),
            ));
        }
        hash.update(&buf[..n]);
        output.write_all(&buf[..n]).map_err(store)?;
    }
    if format!("{:x}", hash.finalize()) != resource.sha256 {
        return Err(Error::Invalid(format!(
            "publisher object {} differs from its exact SHA-256 pin",
            resource.resource_id
        )));
    }
    output.sync_all().map_err(store)?;
    Ok(bytes)
}

#[cfg(test)]
mod counts {
    use std::cell::Cell;
    thread_local! {static READS:Cell<usize>=const{Cell::new(0)};}
    pub fn read() {
        READS.with(|r| r.set(r.get() + 1));
    }
    pub fn take() -> usize {
        READS.with(|r| r.replace(0))
    }
}

#[cfg(test)]
#[path = "publisher_footprints_tests.rs"]
mod tests;
