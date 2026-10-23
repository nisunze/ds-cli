//! Native, bounded provider I/O for elevation work on this machine: the
//! host half (DECOUPLING-013).
//!
//! The governed Rwanda index and exact public TIFF ranges, and AWS Terrarium
//! tiles, are fetched here with byte evidence; the Network engine samples,
//! compares and writes. Both comparison sources are sampled at every input
//! XY and neither is a fallback for the other. URLs and the Rwanda version
//! are fixed.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use ds_geo::projection::GridModelCrs;
use ds_network_client::{CapturedNetworkClient, ElevationCompleted, Needed, NetworkFailure};
use ds_network_contracts::elevation::{
    COMPARISON_ARTIFACT, CompareRequestV1, ComparisonReceiptV1, DemAccessV1,
    ELEVATION_REQUEST_SCHEMA_V1, ElevationRequestV1, ElevationResultV1, ElevationTaskV1,
    HeldRastersV1, rwanda_block_file, terrarium_tile_file,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub use ds_network_contracts::elevation::FetchEvidenceV1 as FetchEvidence;
pub type ComparisonReceipt = ComparisonReceiptV1;

pub const POINT_LIMIT: usize = 4_000;
/// Single-source raster sampling batches have their own admission limit;
/// the comparison CSV's 4,000-row limit remains independent.
pub const RWANDA_BATCH_POINT_LIMIT: usize = 20_000;
pub const SOURCE_BYTE_LIMIT: u64 = 32 * 1024 * 1024;
const TILE_BYTE_LIMIT: usize = 2 * 1024 * 1024;
const BLOCK_BYTE_LIMIT: usize = 32 * 1024 * 1024;
const FETCH_BYTE_LIMIT: usize = 256 * 1024 * 1024;
const RWANDA_BASE: &str = "https://storage.googleapis.com/edcl/reference/rwanda_dem/2026-08-28";
const INDEX_SIZE: usize = 184_412;
const INDEX_SHA256: &str = "c0bdd9c9058a999cebb5b34e1f4f39b3056f24dcf28892227833983f923f10e7";
const TIFF_SIZE: u64 = 882_872_744;

#[derive(Debug, Clone, Copy)]
pub enum SourceCrs {
    Wgs84Lonlat,
    Projected(GridModelCrs),
}

impl SourceCrs {
    pub fn parse(value: &str) -> Result<Self, ComparisonError> {
        match value.trim().to_ascii_lowercase().as_str() {
            "wgs84_lonlat" | "epsg:4326" => Ok(Self::Wgs84Lonlat),
            "nix_itrf2005" => Ok(Self::Projected(GridModelCrs::RwandaTm)),
            _ => GridModelCrs::parse(value)
                .map(Self::Projected)
                .map_err(|error| ComparisonError::InvalidInput(error.to_string())),
        }
    }

    pub fn canonical(self) -> String {
        match self {
            Self::Wgs84Lonlat => "EPSG:4326".into(),
            Self::Projected(crs) => crs.canonical_definition(),
        }
    }

    fn wgs84(self, xy: [f64; 2]) -> Result<[f64; 2], ComparisonError> {
        let point = match self {
            Self::Wgs84Lonlat => xy,
            Self::Projected(crs) => crs
                .to_map(xy)
                .map_err(|error| ComparisonError::InvalidInput(error.to_string()))?,
        };
        if !point.iter().all(|value| value.is_finite())
            || !(-180.0..=180.0).contains(&point[0])
            || !(-90.0..=90.0).contains(&point[1])
        {
            return Err(ComparisonError::InvalidInput(
                "coordinates must resolve to finite WGS84 longitude/latitude".into(),
            ));
        }
        Ok(point)
    }
}

#[derive(Debug, Clone)]
pub struct ComparisonOptions {
    pub source_crs: SourceCrs,
    pub x_column: String,
    pub y_column: String,
    /// Nearest always remains available as the legacy sampling control.
    pub terrarium_bilinear: bool,
}

impl Default for ComparisonOptions {
    fn default() -> Self {
        Self {
            source_crs: SourceCrs::Wgs84Lonlat,
            x_column: "x".into(),
            y_column: "y".into(),
            terrarium_bilinear: true,
        }
    }
}

#[derive(Debug, Error)]
pub enum ComparisonError {
    #[error("invalid elevation input: {0}")]
    InvalidInput(String),
    #[error("elevation source exceeds {SOURCE_BYTE_LIMIT} bytes")]
    SourceByteLimit,
    #[error("elevation comparison exceeds {POINT_LIMIT} points")]
    PointLimit,
    #[error("Rwanda elevation batch exceeds {RWANDA_BATCH_POINT_LIMIT} points")]
    RwandaBatchPointLimit,
    #[error("elevation output already exists")]
    OutputExists,
    #[error("input column conflicts with comparison output: {0}")]
    OutputColumnCollision(String),
    #[error("elevation file I/O failed: {0}")]
    Io(String),
    #[error("elevation provider failed: {0}")]
    Provider(String),
    #[error("elevation source integrity failed: {0}")]
    Integrity(String),
    /// The Network engine that computes elevation is absent or too old.
    #[error("{code}: {message}")]
    Engine { code: &'static str, message: String },
}

impl ComparisonError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "elevation_invalid_input",
            Self::SourceByteLimit => "elevation_source_too_large",
            Self::PointLimit => "elevation_point_limit",
            Self::RwandaBatchPointLimit => "elevation_rwanda_batch_point_limit",
            Self::OutputExists => "elevation_output_exists",
            Self::OutputColumnCollision(_) => "elevation_output_column_collision",
            Self::Io(_) => "elevation_io",
            Self::Provider(_) => "elevation_provider_failed",
            Self::Integrity(_) => "elevation_integrity_failed",
            Self::Engine { code, .. } => code,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct SourceEvidence {
    pub id: &'static str,
    pub dataset: &'static str,
    pub interpolation: &'static str,
    pub vertical_datum: &'static str,
    pub nominal_resolution_m: Option<f64>,
    pub tile_zoom: Option<u32>,
}

fn io_error(error: std::io::Error) -> ComparisonError {
    ComparisonError::Io(error.to_string())
}
fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

/// One bounded provider answer.
pub struct Response {
    pub status: u16,
    pub content_range: Option<String>,
    pub body: Vec<u8>,
}

/// The fixed provider reads; tests substitute a fixture.
pub trait Transport {
    fn get(
        &mut self,
        url: &str,
        range: Option<(u64, u64)>,
        limit: usize,
    ) -> Result<Response, ComparisonError>;
}

pub struct HttpTransport {
    agent: ureq::Agent,
}
impl HttpTransport {
    fn new() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(Duration::from_secs(30)))
                .timeout_connect(Some(Duration::from_secs(10)))
                .build()
                .new_agent(),
        }
    }
}
impl Transport for HttpTransport {
    fn get(
        &mut self,
        url: &str,
        range: Option<(u64, u64)>,
        limit: usize,
    ) -> Result<Response, ComparisonError> {
        let mut request = self.agent.get(url).header("Accept-Encoding", "identity");
        let range_header =
            range.map(|(offset, length)| format!("bytes={offset}-{}", offset + length - 1));
        if let Some(value) = &range_header {
            request = request.header("Range", value);
        }
        let response = request
            .call()
            .map_err(|error| ComparisonError::Provider(error.to_string()))?;
        let status = response.status().as_u16();
        let content_range = response
            .headers()
            .get("Content-Range")
            .and_then(|value| value.to_str().ok())
            .map(str::to_string);
        // A server ignoring Range must never make us download the national TIFF.
        if status != if range.is_some() { 206 } else { 200 } {
            return Err(ComparisonError::Integrity(format!(
                "unexpected HTTP {status} for bounded provider read"
            )));
        }
        let mut body = Vec::new();
        response
            .into_body()
            .into_reader()
            .take(limit as u64 + 1)
            .read_to_end(&mut body)
            .map_err(io_error)?;
        if body.len() > limit {
            return Err(ComparisonError::Integrity(
                "provider response exceeded its byte bound".into(),
            ));
        }
        Ok(Response {
            status,
            content_range,
            body,
        })
    }
}

struct Fetcher<T> {
    transport: T,
    evidence: Vec<FetchEvidence>,
    bytes: usize,
}
impl<T: Transport> Fetcher<T> {
    fn get(
        &mut self,
        url: &str,
        range: Option<(u64, u64)>,
        limit: usize,
    ) -> Result<Response, ComparisonError> {
        let response = self.transport.get(url, range, limit)?;
        if response.body.len() > limit
            || self.bytes.saturating_add(response.body.len()) > FETCH_BYTE_LIMIT
        {
            return Err(ComparisonError::Integrity(
                "comparison fetch byte budget exceeded".into(),
            ));
        }
        self.bytes += response.body.len();
        self.evidence.push(FetchEvidence {
            url: url.into(),
            range: range.map(|(offset, length)| format!("bytes={offset}-{}", offset + length - 1)),
            status: response.status,
            bytes: response.body.len(),
            sha256: sha256(&response.body),
        });
        Ok(response)
    }
}

fn validate_range(
    response: &Response,
    offset: u64,
    length: u64,
    total: u64,
) -> Result<(), ComparisonError> {
    let end = offset
        .checked_add(length)
        .and_then(|value| value.checked_sub(1))
        .filter(|value| *value < total)
        .ok_or_else(|| ComparisonError::Integrity("invalid TIFF byte range".into()))?;
    let expected = format!("bytes {offset}-{end}/{total}");
    if response.status != 206
        || response.content_range.as_deref() != Some(expected.as_str())
        || response.body.len() as u64 != length
    {
        return Err(ComparisonError::Integrity(format!(
            "TIFF read requires exact HTTP 206, {expected}, and {length} bytes"
        )));
    }
    Ok(())
}

struct IndexContract<'a> {
    size: usize,
    digest: &'a str,
    tiff_size: u64,
}

const INDEX_CONTRACT: IndexContract<'static> = IndexContract {
    size: INDEX_SIZE,
    digest: INDEX_SHA256,
    tiff_size: TIFF_SIZE,
};

/// The public rasters one job holds: the pinned index, the exact TIFF ranges
/// and the Terrarium tiles the engine named, each with fetch evidence.
pub struct PublicRasters<T: Transport = HttpTransport> {
    fetcher: Fetcher<T>,
    dir: tempfile::TempDir,
    contract: IndexContract<'static>,
    index: bool,
}

impl PublicRasters<HttpTransport> {
    /// An empty holding; the index is fetched when a job first reads Rwanda.
    pub fn new() -> Result<Self, ComparisonError> {
        Self::with(HttpTransport::new(), INDEX_CONTRACT)
    }
}

impl<T: Transport> PublicRasters<T> {
    fn with(transport: T, contract: IndexContract<'static>) -> Result<Self, ComparisonError> {
        let dir = tempfile::Builder::new()
            .prefix("ds-elevation-rasters-")
            .tempdir()
            .map_err(io_error)?;
        for sub in ["blocks", "tiles"] {
            fs::create_dir(dir.path().join(sub)).map_err(io_error)?;
        }
        Ok(Self {
            fetcher: Fetcher {
                transport,
                evidence: Vec::new(),
                bytes: 0,
            },
            dir,
            contract,
            index: false,
        })
    }

    /// The governed index, pinned by length and SHA-256.
    pub fn ensure_index(&mut self) -> Result<(), ComparisonError> {
        if self.index {
            return Ok(());
        }
        let index = self.fetcher.get(
            &format!("{RWANDA_BASE}/rwanda_dem10m_nix.blockindex.json"),
            None,
            self.contract.size,
        )?;
        if index.status != 200
            || index.body.len() != self.contract.size
            || format!("{:x}", Sha256::digest(&index.body)) != self.contract.digest
        {
            return Err(ComparisonError::Integrity(
                "governed Rwanda block index length or digest mismatch".into(),
            ));
        }
        fs::write(self.index_path(), &index.body).map_err(io_error)?;
        self.index = true;
        Ok(())
    }

    fn index_path(&self) -> PathBuf {
        self.dir.path().join("index.json")
    }

    pub fn held(&self) -> HeldRastersV1 {
        HeldRastersV1 {
            rwanda_index: self.index.then(|| self.index_path()),
            rwanda_cog: None,
            rwanda_blocks: Some(self.dir.path().join("blocks")),
            terrarium_tiles: Some(self.dir.path().join("tiles")),
            dem_access: DemAccessV1::DirectRange,
            full_local_required_above_points: RWANDA_BATCH_POINT_LIMIT as u32,
        }
    }

    /// Fetch exactly what one engine round named.
    pub fn acquire(&mut self, needed: &Needed) -> Result<(), ComparisonError> {
        if needed.rwanda_index {
            self.ensure_index()?;
        }
        for block in &needed.rwanda_blocks {
            let limit = usize::try_from(block.length)
                .map_err(|_| ComparisonError::Integrity("TIFF block length overflow".into()))?;
            if limit == 0 || limit > BLOCK_BYTE_LIMIT {
                return Err(ComparisonError::Integrity(
                    "TIFF block exceeds bounded range limit".into(),
                ));
            }
            let response = self.fetcher.get(
                &format!("{RWANDA_BASE}/rwanda_dem10m_nix.tif"),
                Some((block.offset, block.length)),
                limit,
            )?;
            validate_range(
                &response,
                block.offset,
                block.length,
                self.contract.tiff_size,
            )?;
            fs::write(
                self.dir
                    .path()
                    .join("blocks")
                    .join(rwanda_block_file(block)),
                &response.body,
            )
            .map_err(io_error)?;
        }
        // At most five tiles per input point; the engine bounds its decode.
        if needed.terrarium_tiles.len() > 512 {
            return Err(ComparisonError::Integrity(
                "Terrarium decoded tile budget exceeded (512 tiles)".into(),
            ));
        }
        for tile in &needed.terrarium_tiles {
            let response = self.fetcher.get(
                &format!(
                    "https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{}/{}/{}.png",
                    tile.z, tile.x, tile.y
                ),
                None,
                TILE_BYTE_LIMIT,
            )?;
            if response.status != 200 {
                return Err(ComparisonError::Integrity(
                    "Terrarium tile requires HTTP 200".into(),
                ));
            }
            fs::write(
                self.dir
                    .path()
                    .join("tiles")
                    .join(terrarium_tile_file(tile)),
                &response.body,
            )
            .map_err(io_error)?;
        }
        Ok(())
    }

    pub fn evidence(&self) -> &[FetchEvidence] {
        &self.fetcher.evidence
    }

    pub fn scratch(&self) -> &Path {
        self.dir.path()
    }
}

/// A Network failure in the comparison vocabulary: the engine's own refusal
/// code where it names one, else the engine's absence or age.
pub fn engine_error(failure: NetworkFailure) -> ComparisonError {
    let message = failure.message.clone();
    match failure.owner_code().unwrap_or(failure.code()) {
        "elevation_invalid_input" | "elevation_request_invalid" => {
            ComparisonError::InvalidInput(message)
        }
        "elevation_source_too_large" => ComparisonError::SourceByteLimit,
        "elevation_point_limit" => ComparisonError::PointLimit,
        "elevation_rwanda_batch_point_limit" => ComparisonError::RwandaBatchPointLimit,
        "elevation_output_exists" => ComparisonError::OutputExists,
        "elevation_output_column_collision" => ComparisonError::OutputColumnCollision(message),
        "elevation_io" => ComparisonError::Io(message),
        "network_engine_missing" | "module_removed" => ComparisonError::Engine {
            code: "network_engine_missing",
            message,
        },
        "network_engine_incompatible" => ComparisonError::Engine {
            code: "network_engine_incompatible",
            message,
        },
        _ => ComparisonError::Integrity(message),
    }
}

/// Why a job stopped: this host could not hold a raster, or the engine refused.
#[derive(Debug)]
pub enum JobError {
    Acquisition(ComparisonError),
    Engine(NetworkFailure),
}

impl From<JobError> for ComparisonError {
    fn from(error: JobError) -> Self {
        match error {
            JobError::Acquisition(error) => error,
            JobError::Engine(failure) => engine_error(failure),
        }
    }
}

/// Run one engine job to completion over public rasters.
pub fn run_job<T: Transport>(
    client: &CapturedNetworkClient,
    rasters: &mut PublicRasters<T>,
    task: ElevationTaskV1,
    on_round: &mut dyn FnMut(&mut ElevationRequestV1, &[FetchEvidence]),
) -> Result<ElevationCompleted, JobError> {
    let mut request = ElevationRequestV1 {
        schema: ELEVATION_REQUEST_SCHEMA_V1.into(),
        execution_host: "headless".into(),
        rasters: rasters.held(),
        task,
    };
    let scratch = rasters.scratch().to_path_buf();
    let mut failure = None;
    let completed =
        client.elevation(
            &mut request,
            &scratch,
            &|| false,
            &mut |request, needed| match rasters.acquire(needed) {
                Ok(()) => {
                    request.rasters = rasters.held();
                    on_round(request, rasters.evidence());
                    Ok(())
                }
                Err(error) => {
                    failure = Some(error);
                    Err(NetworkFailure::acquisition(
                        "a public raster could not be held",
                    ))
                }
            },
        );
    completed.map_err(|error| match failure.take() {
        Some(acquisition) => JobError::Acquisition(acquisition),
        None => JobError::Engine(error),
    })
}

/// Capture the Network engine for headless elevation work.
pub fn capture_engine(lane: Option<&str>) -> Result<CapturedNetworkClient, ComparisonError> {
    match lane {
        Some(lane) => CapturedNetworkClient::capture_for_lane(lane, 1, &|| false),
        None => CapturedNetworkClient::capture(1, &|| false),
    }
    .map_err(engine_error)
}

/// A single-source Rwanda raster session: bounded public acquisition held
/// across batches, sampled by the Network engine. It never requests
/// Terrarium or changes Z.
pub struct RwandaSampler {
    rasters: PublicRasters,
    client: CapturedNetworkClient,
    batch: usize,
}

impl RwandaSampler {
    pub fn new() -> Result<Self, ComparisonError> {
        Self::open(None)
    }

    /// A sampler whose engine is the one installed for `lane`.
    pub fn for_lane(lane: &str) -> Result<Self, ComparisonError> {
        Self::open(Some(lane))
    }

    fn open(lane: Option<&str>) -> Result<Self, ComparisonError> {
        let client = capture_engine(lane)?;
        let mut rasters = PublicRasters::new()?;
        rasters.ensure_index()?;
        Ok(Self {
            rasters,
            client,
            batch: 0,
        })
    }

    /// WGS84 longitude/latitude at actual query XY; absent raster coverage
    /// remains None, whereas failed acquisition refuses the whole batch.
    pub fn sample_wgs84(
        &mut self,
        points: &[[f64; 2]],
    ) -> Result<Vec<Option<f64>>, ComparisonError> {
        if points.len() > RWANDA_BATCH_POINT_LIMIT {
            return Err(ComparisonError::RwandaBatchPointLimit);
        }
        for point in points {
            SourceCrs::Wgs84Lonlat.wgs84(*point)?;
        }
        self.batch += 1;
        let path = self
            .rasters
            .scratch()
            .join(format!("points-{}.json", self.batch));
        fs::write(
            &path,
            serde_json::to_vec(points).map_err(|error| ComparisonError::Io(error.to_string()))?,
        )
        .map_err(io_error)?;
        let completed = run_job(
            &self.client,
            &mut self.rasters,
            ElevationTaskV1::Sample {
                points: path.clone(),
            },
            &mut |_, _| {},
        );
        let _ = fs::remove_file(&path);
        match completed.map_err(ComparisonError::from)?.result {
            ElevationResultV1::Sample { samples } if samples.len() == points.len() => Ok(samples),
            _ => Err(ComparisonError::Integrity(
                "the Network engine answered another sampling request".into(),
            )),
        }
    }

    pub fn acquisition_evidence(&self) -> &[FetchEvidence] {
        self.rasters.evidence()
    }

    pub fn source_evidence() -> SourceEvidence {
        SourceEvidence {
            id: "rwanda_dem_10m",
            dataset: "Rwanda DEM 2026-08-28",
            interpolation: "bilinear_pixel_centres_four_valid_corners",
            vertical_datum: "unknown",
            nominal_resolution_m: Some(10.0),
            tile_zoom: None,
        }
    }
}

/// Compare two fixed providers independently and write one new CSV. No file is
/// written unless all provider requests and their integrity checks succeed.
pub fn compare_file(
    source: &Path,
    out: &Path,
    options: &ComparisonOptions,
) -> Result<ComparisonReceipt, ComparisonError> {
    if fs::symlink_metadata(out).is_ok() {
        return Err(ComparisonError::OutputExists);
    }
    let client = capture_engine(None)?;
    let mut rasters = PublicRasters::new()?;
    rasters.ensure_index()?;
    compare_with(&client, &mut rasters, source, out, options)
}

fn compare_with<T: Transport>(
    client: &CapturedNetworkClient,
    rasters: &mut PublicRasters<T>,
    source: &Path,
    out: &Path,
    options: &ComparisonOptions,
) -> Result<ComparisonReceipt, ComparisonError> {
    let completed = run_job(
        client,
        rasters,
        ElevationTaskV1::Compare {
            request: CompareRequestV1 {
                source: source.to_path_buf(),
                output: out.to_path_buf(),
                source_crs: options.source_crs.canonical(),
                x_column: options.x_column.clone(),
                y_column: options.y_column.clone(),
                terrarium_bilinear: options.terrarium_bilinear,
            },
        },
        &mut |_, _| {},
    )
    .map_err(ComparisonError::from)?;
    let ElevationResultV1::Compare { receipt } = completed.result.clone() else {
        return Err(ComparisonError::Integrity(
            "the Network engine answered another elevation task".into(),
        ));
    };
    let mut receipt = *receipt;
    let artifact = completed.artifact(COMPARISON_ARTIFACT);
    let mut bytes = Vec::new();
    fs::File::open(&artifact)
        .and_then(|file| file.take(SOURCE_BYTE_LIMIT * 4 + 1).read_to_end(&mut bytes))
        .map_err(io_error)?;
    if bytes.len() != receipt.output_bytes || sha256(&bytes) != receipt.output_sha256 {
        return Err(ComparisonError::Integrity(
            "the engine's comparison differs from its own evidence".into(),
        ));
    }
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .and_then(|mut file| {
            use std::io::Write;
            file.write_all(&bytes).and_then(|()| file.sync_all())
        })
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ComparisonError::OutputExists
            } else {
                io_error(error)
            }
        })?;
    receipt.fetches = rasters.evidence().to_vec();
    Ok(receipt)
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
