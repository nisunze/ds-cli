//! Native, bounded provider I/O for an independent elevation comparison.
//!
//! Both sources are sampled at every input XY. Neither is a fallback for the
//! other. Raw samples are preserved: no datum conversion, correction, or model
//! mutation is implied by the difference. URLs and the Rwanda version are fixed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::time::Duration;

use ds_elevation::{
    DEFAULT_DECODED_BLOCK_CACHE, RwandaBlockCache, RwandaBlockProvider, RwandaBlockRequest,
    RwandaDem,
};
use ds_geo::projection::{GridModelCrs, wgs84_to_nix_itrf2005};
use image::{ImageFormat, RgbaImage};
use serde::Serialize;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const POINT_LIMIT: usize = 4_000;
/// Single-source raster sampling batches have their own admission limit;
/// the comparison CSV's 4,000-row limit remains independent.
pub const RWANDA_BATCH_POINT_LIMIT: usize = 20_000;
pub const SOURCE_BYTE_LIMIT: u64 = 32 * 1024 * 1024;
const TILE_BYTE_LIMIT: usize = 2 * 1024 * 1024;
const BLOCK_BYTE_LIMIT: usize = 32 * 1024 * 1024;
const FETCH_BYTE_LIMIT: usize = 256 * 1024 * 1024;
const ZOOM: u32 = 15;
const WORLD_PIXELS: i64 = (1_i64 << ZOOM) * 256;
const MERCATOR_LATITUDE_LIMIT: f64 = 85.05112878;
const RWANDA_BASE: &str = "https://storage.googleapis.com/edcl/reference/rwanda_dem/2026-08-28";
const INDEX_SIZE: usize = 184_412;
const INDEX_SHA256: &str = "c0bdd9c9058a999cebb5b34e1f4f39b3056f24dcf28892227833983f923f10e7";
const TIFF_SIZE: u64 = 882_872_744;
const COLUMNS: &[&str] = &[
    "comparison_longitude_deg",
    "comparison_latitude_deg",
    "rwanda_elevation_m",
    "terrarium_nearest_elevation_m",
    "terrarium_bilinear_elevation_m",
    "delta_nearest_minus_rwanda_m",
    "delta_bilinear_minus_rwanda_m",
];

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

    fn canonical(self) -> String {
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
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FetchEvidence {
    pub url: String,
    pub range: Option<String>,
    pub status: u16,
    pub bytes: usize,
    pub sha256: String,
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

#[derive(Debug, Serialize)]
pub struct ComparisonReceipt {
    pub schema: &'static str,
    pub source_path: String,
    pub source_bytes: usize,
    pub source_sha256: String,
    pub source_crs: String,
    pub output_path: String,
    pub output_bytes: usize,
    pub output_sha256: String,
    pub point_count: usize,
    pub rwanda_count: usize,
    pub terrarium_nearest_count: usize,
    pub terrarium_bilinear_count: Option<usize>,
    pub paired_nearest_count: usize,
    pub paired_bilinear_count: Option<usize>,
    pub rwanda_missing_count: usize,
    pub terrarium_nearest_missing_count: usize,
    pub terrarium_bilinear_missing_count: Option<usize>,
    pub sources: Vec<SourceEvidence>,
    pub correction_applied: bool,
    pub fallback_applied: bool,
    pub model_changed: bool,
    pub missing_representation: &'static str,
    pub difference_definition: &'static str,
    pub rwanda_tiff_full_object_bytes: u64,
    pub rwanda_tiff_full_object_sha256: Option<&'static str>,
    pub fetches: Vec<FetchEvidence>,
}

struct Input {
    bytes: Vec<u8>,
    headers: csv::StringRecord,
    rows: Vec<csv::StringRecord>,
    points: Vec<[f64; 2]>,
}

fn input(source: &Path, out: &Path, options: &ComparisonOptions) -> Result<Input, ComparisonError> {
    if fs::symlink_metadata(out).is_ok() {
        return Err(ComparisonError::OutputExists);
    }
    let metadata = fs::symlink_metadata(source).map_err(io_error)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(ComparisonError::InvalidInput(
            "source must be a regular CSV file".into(),
        ));
    }
    if metadata.len() > SOURCE_BYTE_LIMIT {
        return Err(ComparisonError::SourceByteLimit);
    }
    let mut bytes = Vec::new();
    File::open(source)
        .map_err(io_error)?
        .take(SOURCE_BYTE_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(io_error)?;
    if bytes.len() as u64 > SOURCE_BYTE_LIMIT {
        return Err(ComparisonError::SourceByteLimit);
    }
    let mut reader = csv::Reader::from_reader(bytes.as_slice());
    let headers = reader.headers().map_err(csv_error)?.clone();
    let mut seen = BTreeSet::new();
    for header in &headers {
        if !seen.insert(header) || header.is_empty() {
            return Err(ComparisonError::InvalidInput(
                "CSV headers must be unique and nonempty".into(),
            ));
        }
        if COLUMNS.contains(&header) {
            return Err(ComparisonError::OutputColumnCollision(header.into()));
        }
    }
    let column = |name: &str| {
        headers
            .iter()
            .position(|header| header == name)
            .ok_or_else(|| ComparisonError::InvalidInput(format!("CSV has no {name:?} column")))
    };
    let x = column(&options.x_column)?;
    let y = column(&options.y_column)?;
    if x == y {
        return Err(ComparisonError::InvalidInput(
            "X and Y must name different columns".into(),
        ));
    }
    let mut rows = Vec::new();
    let mut points = Vec::new();
    for (index, row) in reader.records().enumerate() {
        let row = row.map_err(csv_error)?;
        if rows.len() == POINT_LIMIT {
            return Err(ComparisonError::PointLimit);
        }
        let number = |column: usize| {
            row[column].trim().parse::<f64>().map_err(|_| {
                ComparisonError::InvalidInput(format!(
                    "CSV row {} has invalid coordinates",
                    index + 2
                ))
            })
        };
        points.push(options.source_crs.wgs84([number(x)?, number(y)?])?);
        rows.push(row);
    }
    if rows.is_empty() {
        return Err(ComparisonError::InvalidInput(
            "CSV has no point rows".into(),
        ));
    }
    Ok(Input {
        bytes,
        headers,
        rows,
        points,
    })
}

fn io_error(error: std::io::Error) -> ComparisonError {
    ComparisonError::Io(error.to_string())
}
fn csv_error(error: csv::Error) -> ComparisonError {
    ComparisonError::InvalidInput(error.to_string())
}
fn sha256(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

struct Response {
    status: u16,
    content_range: Option<String>,
    body: Vec<u8>,
}

trait Transport {
    fn get(
        &mut self,
        url: &str,
        range: Option<(u64, u64)>,
        limit: usize,
    ) -> Result<Response, ComparisonError>;
}

struct HttpTransport {
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

struct RwandaProvider<'a, T> {
    fetcher: &'a mut Fetcher<T>,
    total: u64,
    failure: Option<ComparisonError>,
}
impl<T: Transport> RwandaBlockProvider for RwandaProvider<'_, T> {
    fn read_compressed_block(
        &mut self,
        request: &RwandaBlockRequest,
    ) -> Result<Vec<u8>, ds_elevation::ElevationError> {
        let read = (|| {
            let limit = usize::try_from(request.length)
                .map_err(|_| ComparisonError::Integrity("TIFF block length overflow".into()))?;
            if limit == 0 || limit > BLOCK_BYTE_LIMIT {
                return Err(ComparisonError::Integrity(
                    "TIFF block exceeds bounded range limit".into(),
                ));
            }
            let response = self.fetcher.get(
                &format!("{RWANDA_BASE}/rwanda_dem10m_nix.tif"),
                Some((request.offset, request.length)),
                limit,
            )?;
            validate_range(&response, request.offset, request.length, self.total)?;
            Ok(response.body)
        })();
        read.map_err(|error: ComparisonError| {
            let message = error.to_string();
            self.failure = Some(error);
            ds_elevation::ElevationError::Provider {
                provider_id: "rwanda_dem_10m".into(),
                message,
            }
        })
    }
}

#[derive(Clone, Copy)]
struct PixelPlan {
    nearest: (i64, i64),
    corners: [(i64, i64); 4],
    dx: f64,
    dy: f64,
}
fn pixels(point: [f64; 2]) -> Option<PixelPlan> {
    if point[1].abs() > MERCATOR_LATITUDE_LIMIT {
        return None;
    }
    let lat = point[1].to_radians();
    let x = (point[0] + 180.0) / 360.0 * WORLD_PIXELS as f64;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI)
        * 0.5
        * WORLD_PIXELS as f64;
    let cx = x - 0.5;
    let cy = y - 0.5;
    let (ix, iy) = (cx.floor() as i64, cy.floor() as i64);
    Some(PixelPlan {
        nearest: (
            (x.floor() as i64).clamp(0, WORLD_PIXELS - 1),
            (y.floor() as i64).clamp(0, WORLD_PIXELS - 1),
        ),
        corners: [(ix, iy), (ix + 1, iy), (ix, iy + 1), (ix + 1, iy + 1)],
        dx: cx - cx.floor(),
        dy: cy - cy.floor(),
    })
}
fn tile(pixel: (i64, i64)) -> Option<((u32, u32), (u32, u32))> {
    if !(0..WORLD_PIXELS).contains(&pixel.1) {
        return None;
    }
    let x = pixel.0.rem_euclid(WORLD_PIXELS) as u32;
    let y = pixel.1 as u32;
    Some(((x / 256, y / 256), (x % 256, y % 256)))
}
fn height(pixel: &image::Rgba<u8>) -> Option<f64> {
    (pixel[3] != 0).then(|| {
        f64::from(pixel[0]) * 256.0 + f64::from(pixel[1]) + f64::from(pixel[2]) / 256.0 - 32768.0
    })
}

type TerrariumReadings = (Vec<Option<f64>>, Vec<Option<f64>>);

fn terrarium<T: Transport>(
    fetcher: &mut Fetcher<T>,
    points: &[[f64; 2]],
    bilinear: bool,
) -> Result<TerrariumReadings, ComparisonError> {
    let plans = points
        .iter()
        .map(|point| pixels(*point))
        .collect::<Vec<_>>();
    let mut requests = BTreeSet::new();
    for plan in plans.iter().flatten() {
        if let Some((key, _)) = tile(plan.nearest) {
            requests.insert(key);
        }
        if bilinear {
            for corner in plan.corners {
                if let Some((key, _)) = tile(corner) {
                    requests.insert(key);
                }
            }
        }
    }
    // At most five tiles per input point; hold only the tiles actually requested,
    // with an explicit decoded-memory admission bound.
    if requests.len() > 512 {
        return Err(ComparisonError::Integrity(
            "Terrarium decoded tile budget exceeded (512 tiles)".into(),
        ));
    }
    let mut tiles: BTreeMap<(u32, u32), RgbaImage> = BTreeMap::new();
    for (x, y) in requests {
        let response = fetcher.get(
            &format!("https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{ZOOM}/{x}/{y}.png"),
            None,
            TILE_BYTE_LIMIT,
        )?;
        if response.status != 200 {
            return Err(ComparisonError::Integrity(
                "Terrarium tile requires HTTP 200".into(),
            ));
        }
        let dimensions =
            image::ImageReader::with_format(Cursor::new(&response.body), ImageFormat::Png)
                .into_dimensions()
                .map_err(|error| {
                    ComparisonError::Integrity(format!("Terrarium PNG header: {error}"))
                })?;
        if dimensions != (256, 256) {
            return Err(ComparisonError::Integrity(
                "Terrarium tile must be 256 by 256 pixels".into(),
            ));
        }
        let image = image::load_from_memory_with_format(&response.body, ImageFormat::Png)
            .map_err(|error| ComparisonError::Integrity(format!("Terrarium PNG: {error}")))?
            .to_rgba8();
        tiles.insert((x, y), image);
    }
    let at = |pixel| {
        let (key, (x, y)) = tile(pixel)?;
        height(tiles.get(&key)?.get_pixel(x, y))
    };
    let nearest = plans
        .iter()
        .map(|plan| plan.and_then(|plan| at(plan.nearest)))
        .collect();
    let smooth = plans
        .iter()
        .map(|plan| {
            if !bilinear {
                return None;
            }
            let plan = (*plan)?;
            let [a, b, c, d] = plan.corners.map(at);
            let (a, b, c, d) = (a?, b?, c?, d?);
            Some(
                (a * (1.0 - plan.dx) + b * plan.dx) * (1.0 - plan.dy)
                    + (c * (1.0 - plan.dx) + d * plan.dx) * plan.dy,
            )
        })
        .collect();
    Ok((nearest, smooth))
}

struct IndexContract<'a> {
    size: usize,
    digest: &'a str,
    tiff_size: u64,
}

/// A single-source Rwanda raster session with bounded acquisition and decoded
/// block caching across batches. It never requests Terrarium or changes Z.
pub struct RwandaSampler {
    dem: RwandaDem,
    cache: RwandaBlockCache,
    fetcher: Fetcher<HttpTransport>,
}

impl RwandaSampler {
    pub fn new() -> Result<Self, ComparisonError> {
        let mut fetcher = Fetcher {
            transport: HttpTransport::new(),
            evidence: Vec::new(),
            bytes: 0,
        };
        let index = fetcher.get(
            &format!("{RWANDA_BASE}/rwanda_dem10m_nix.blockindex.json"),
            None,
            INDEX_SIZE,
        )?;
        if index.status != 200
            || index.body.len() != INDEX_SIZE
            || format!("{:x}", Sha256::digest(&index.body)) != INDEX_SHA256
        {
            return Err(ComparisonError::Integrity(
                "governed Rwanda block index length or digest mismatch".into(),
            ));
        }
        let dem = RwandaDem::from_index_json(&index.body)
            .map_err(|error| ComparisonError::Integrity(error.to_string()))?;
        let cache = RwandaBlockCache::new(DEFAULT_DECODED_BLOCK_CACHE)
            .map_err(|error| ComparisonError::Integrity(error.to_string()))?;
        Ok(Self {
            dem,
            cache,
            fetcher,
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
        let mut nix = Vec::with_capacity(points.len());
        for point in points {
            let point = SourceCrs::Wgs84Lonlat.wgs84(*point)?;
            let (x, y) = wgs84_to_nix_itrf2005(point[0], point[1]);
            nix.push([x, y]);
        }
        let mut provider = RwandaProvider {
            fetcher: &mut self.fetcher,
            total: TIFF_SIZE,
            failure: None,
        };
        self.dem
            .sample_with_cache(&mut provider, &mut self.cache, &nix)
            .map_err(|error| {
                provider
                    .failure
                    .take()
                    .unwrap_or_else(|| ComparisonError::Integrity(error.to_string()))
            })
    }

    pub fn acquisition_evidence(&self) -> &[FetchEvidence] {
        &self.fetcher.evidence
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
    compare_with(
        source,
        out,
        options,
        HttpTransport::new(),
        IndexContract {
            size: INDEX_SIZE,
            digest: INDEX_SHA256,
            tiff_size: TIFF_SIZE,
        },
    )
}

fn compare_with<T: Transport>(
    source: &Path,
    out: &Path,
    options: &ComparisonOptions,
    transport: T,
    contract: IndexContract<'_>,
) -> Result<ComparisonReceipt, ComparisonError> {
    let input = input(source, out, options)?;
    let mut fetcher = Fetcher {
        transport,
        evidence: Vec::new(),
        bytes: 0,
    };
    let index = fetcher.get(
        &format!("{RWANDA_BASE}/rwanda_dem10m_nix.blockindex.json"),
        None,
        contract.size,
    )?;
    if index.status != 200
        || index.body.len() != contract.size
        || format!("{:x}", Sha256::digest(&index.body)) != contract.digest
    {
        return Err(ComparisonError::Integrity(
            "governed Rwanda block index length or digest mismatch".into(),
        ));
    }
    let dem = RwandaDem::from_index_json(&index.body)
        .map_err(|error| ComparisonError::Integrity(error.to_string()))?;
    let nix = input
        .points
        .iter()
        .map(|point| {
            let (x, y) = wgs84_to_nix_itrf2005(point[0], point[1]);
            [x, y]
        })
        .collect::<Vec<_>>();
    let mut provider = RwandaProvider {
        fetcher: &mut fetcher,
        total: contract.tiff_size,
        failure: None,
    };
    let rwanda = dem.sample(&mut provider, &nix).map_err(|error| {
        provider
            .failure
            .take()
            .unwrap_or_else(|| ComparisonError::Integrity(error.to_string()))
    })?;
    let (nearest, bilinear) = terrarium(&mut fetcher, &input.points, options.terrarium_bilinear)?;
    let mut writer = csv::Writer::from_writer(Vec::new());
    let mut headers = input.headers.clone();
    for column in COLUMNS {
        headers.push_field(column);
    }
    writer.write_record(&headers).map_err(csv_error)?;
    let value = |value: Option<f64>| value.map(|value| value.to_string()).unwrap_or_default();
    for (index, row) in input.rows.iter().enumerate() {
        let mut row = row.clone();
        row.push_field(&input.points[index][0].to_string());
        row.push_field(&input.points[index][1].to_string());
        for sample in [
            rwanda[index],
            nearest[index],
            bilinear[index],
            nearest[index].zip(rwanda[index]).map(|(t, r)| t - r),
            bilinear[index].zip(rwanda[index]).map(|(t, r)| t - r),
        ] {
            row.push_field(&value(sample));
        }
        writer.write_record(&row).map_err(csv_error)?;
    }
    let output = writer
        .into_inner()
        .map_err(|error| ComparisonError::Io(error.to_string()))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ComparisonError::OutputExists
            } else {
                io_error(error)
            }
        })?;
    if let Err(error) = file.write_all(&output).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = fs::remove_file(out);
        return Err(io_error(error));
    }
    let count = input.points.len();
    let populated =
        |samples: &[Option<f64>]| samples.iter().filter(|sample| sample.is_some()).count();
    let paired = |samples: &[Option<f64>]| {
        samples
            .iter()
            .zip(&rwanda)
            .filter(|(a, b)| a.is_some() && b.is_some())
            .count()
    };
    let rc = populated(&rwanda);
    let nc = populated(&nearest);
    let bc = populated(&bilinear);
    let mut sources = vec![
        SourceEvidence {
            id: "rwanda_dem_10m",
            dataset: "Rwanda DEM 2026-08-28",
            interpolation: "bilinear_pixel_centres_four_valid_corners",
            vertical_datum: "unknown",
            nominal_resolution_m: Some(10.0),
            tile_zoom: None,
        },
        SourceEvidence {
            id: "aws_terrarium_30m",
            dataset: "elevation-tiles-prod/terrarium",
            interpolation: "nearest_legacy_containing_pixel",
            vertical_datum: "unknown",
            nominal_resolution_m: Some(30.0),
            tile_zoom: Some(ZOOM),
        },
    ];
    if options.terrarium_bilinear {
        sources.push(SourceEvidence {
            id: "aws_terrarium_30m",
            dataset: "elevation-tiles-prod/terrarium",
            interpolation: "bilinear_pixel_centres_four_valid_corners_across_tiles",
            vertical_datum: "unknown",
            nominal_resolution_m: Some(30.0),
            tile_zoom: Some(ZOOM),
        });
    }
    Ok(ComparisonReceipt {
        schema: "ds-elevation.compare/v1",
        source_path: source.display().to_string(),
        source_bytes: input.bytes.len(),
        source_sha256: sha256(&input.bytes),
        source_crs: options.source_crs.canonical(),
        output_path: out.display().to_string(),
        output_bytes: output.len(),
        output_sha256: sha256(&output),
        point_count: count,
        rwanda_count: rc,
        terrarium_nearest_count: nc,
        terrarium_bilinear_count: options.terrarium_bilinear.then_some(bc),
        paired_nearest_count: paired(&nearest),
        paired_bilinear_count: options.terrarium_bilinear.then_some(paired(&bilinear)),
        rwanda_missing_count: count - rc,
        terrarium_nearest_missing_count: count - nc,
        terrarium_bilinear_missing_count: options.terrarium_bilinear.then_some(count - bc),
        sources,
        correction_applied: false,
        fallback_applied: false,
        model_changed: false,
        missing_representation: "blank_csv_cells",
        difference_definition: "Terrarium minus Rwanda, metres; raw uncalibrated difference",
        rwanda_tiff_full_object_bytes: contract.tiff_size,
        rwanda_tiff_full_object_sha256: None,
        fetches: fetcher.evidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compression, write::ZlibEncoder};
    use image::{ExtendedColorType, ImageEncoder, codecs::png::PngEncoder};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    struct Temp(PathBuf);
    impl Temp {
        fn new(name: &str) -> Self {
            let root = std::env::var_os("TMPDIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("out/elevation-host-tests"));
            let path = root.join(format!(
                "{name}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn png(mut at: impl FnMut(u32, u32) -> Option<f64>) -> Vec<u8> {
        let mut raw = Vec::with_capacity(256 * 256 * 4);
        for y in 0..256 {
            for x in 0..256 {
                if let Some(z) = at(x, y) {
                    let packed = ((z + 32768.0) * 256.0).round() as u32;
                    raw.extend_from_slice(&[
                        ((packed >> 16) & 255) as u8,
                        ((packed >> 8) & 255) as u8,
                        (packed & 255) as u8,
                        255,
                    ]);
                } else {
                    raw.extend_from_slice(&[128, 0, 0, 0]);
                }
            }
        }
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(&raw, 256, 256, ExtendedColorType::Rgba8)
            .unwrap();
        bytes
    }

    struct Fixture {
        index: Vec<u8>,
        block: Vec<u8>,
        transparent: bool,
        fail_terrarium: bool,
        plane_origin: Option<(i64, i64)>,
    }
    impl Fixture {
        fn new() -> Self {
            let (x, y) = wgs84_to_nix_itrf2005(30.0, -2.0);
            let mut compressor = ZlibEncoder::new(Vec::new(), Compression::default());
            for value in [1600.0_f32, 1601.0, 1602.0, 1603.0] {
                compressor.write_all(&value.to_le_bytes()).unwrap();
            }
            let block = compressor.finish().unwrap();
            let index = serde_json::to_vec(&serde_json::json!({
                "width":2,"height":2,"geotransform":[x-5.0,10.0,0.0,y+5.0,0.0,-10.0],
                "nodata":-9999.0,"block":[2,2],"blocks":[{"i":0,"j":0,"offset":64,"length":block.len()}]
            })).unwrap();
            Self {
                index,
                block,
                transparent: false,
                fail_terrarium: false,
                plane_origin: None,
            }
        }
        fn contract(&self) -> (usize, String, u64) {
            (
                self.index.len(),
                format!("{:x}", Sha256::digest(&self.index)),
                64 + self.block.len() as u64,
            )
        }
    }

    #[test]
    fn single_source_rwanda_batch_limit_is_independent_and_precedes_acquisition() {
        let fixture = Fixture::new();
        let mut sampler = RwandaSampler {
            dem: RwandaDem::from_index_json(&fixture.index).unwrap(),
            cache: RwandaBlockCache::new(DEFAULT_DECODED_BLOCK_CACHE).unwrap(),
            fetcher: Fetcher {
                transport: HttpTransport::new(),
                evidence: Vec::new(),
                bytes: 0,
            },
        };
        // These valid WGS84 queries lie outside the fixture's Rwanda grid, so
        // its public API must return real coverage gaps without any HTTP.
        let outside = vec![[0.0, 0.0]; RWANDA_BATCH_POINT_LIMIT];
        let heights = sampler.sample_wgs84(&outside).unwrap();
        assert_eq!(heights.len(), RWANDA_BATCH_POINT_LIMIT);
        assert!(heights.iter().all(Option::is_none));
        assert!(sampler.acquisition_evidence().is_empty());
        let oversized = vec![[30.0, -2.0]; RWANDA_BATCH_POINT_LIMIT + 1];
        let error = sampler.sample_wgs84(&oversized).unwrap_err();
        assert_eq!(error.code(), "elevation_rwanda_batch_point_limit");
        assert!(error.to_string().contains("20000"));
        assert!(!error.to_string().contains("4000"));
        assert!(sampler.acquisition_evidence().is_empty());
    }
    impl Transport for Fixture {
        fn get(
            &mut self,
            url: &str,
            range: Option<(u64, u64)>,
            _limit: usize,
        ) -> Result<Response, ComparisonError> {
            if url.ends_with("blockindex.json") {
                assert!(range.is_none());
                return Ok(Response {
                    status: 200,
                    content_range: None,
                    body: self.index.clone(),
                });
            }
            if url.ends_with(".tif") {
                assert_eq!(range, Some((64, self.block.len() as u64)));
                let total = 64 + self.block.len() as u64;
                return Ok(Response {
                    status: 206,
                    content_range: Some(format!("bytes 64-{}/{total}", total - 1)),
                    body: self.block.clone(),
                });
            }
            assert!(url.starts_with("https://s3.amazonaws.com/elevation-tiles-prod/terrarium/15/"));
            assert!(range.is_none());
            if self.fail_terrarium {
                return Err(ComparisonError::Provider(
                    "fixture transport failure".into(),
                ));
            }
            let body = if self.transparent {
                png(|_, _| None)
            } else if let Some((ox, oy)) = self.plane_origin {
                let parts = url
                    .trim_end_matches(".png")
                    .rsplit('/')
                    .take(2)
                    .collect::<Vec<_>>();
                let ty = parts[0].parse::<i64>().unwrap();
                let tx = parts[1].parse::<i64>().unwrap();
                png(|x, y| {
                    Some(
                        100.0
                            + 2.0 * (tx * 256 + i64::from(x) - ox) as f64
                            + 3.0 * (ty * 256 + i64::from(y) - oy) as f64,
                    )
                })
            } else {
                png(|_, _| Some(1700.0))
            };
            Ok(Response {
                status: 200,
                content_range: None,
                body,
            })
        }
    }

    fn run_fixture(
        source: &Path,
        out: &Path,
        options: &ComparisonOptions,
        fixture: Fixture,
    ) -> Result<ComparisonReceipt, ComparisonError> {
        let (size, digest, tiff_size) = fixture.contract();
        compare_with(
            source,
            out,
            options,
            fixture,
            IndexContract {
                size,
                digest: &digest,
                tiff_size,
            },
        )
    }

    #[test]
    fn samples_sources_independently_and_preserves_original_columns() {
        let dir = Temp::new("independent");
        let source = dir.0.join("input.csv");
        let out = dir.0.join("output.csv");
        fs::write(
            &source,
            "id,alignment,station_m,x,y\na,route1,0,30,-2\nb,route2,10,30.1,-2.1\n",
        )
        .unwrap();
        let receipt =
            run_fixture(&source, &out, &ComparisonOptions::default(), Fixture::new()).unwrap();
        assert_eq!(receipt.point_count, 2);
        assert_eq!(receipt.rwanda_count, 1);
        assert_eq!(receipt.terrarium_nearest_count, 2);
        assert_eq!(receipt.terrarium_bilinear_count, Some(2));
        assert_eq!(receipt.paired_nearest_count, 1);
        assert!(!receipt.correction_applied && !receipt.fallback_applied && !receipt.model_changed);
        assert!(receipt.rwanda_tiff_full_object_sha256.is_none());
        let mut reader = csv::Reader::from_path(&out).unwrap();
        let headers = reader.headers().unwrap().clone();
        let records = reader.records().map(Result::unwrap).collect::<Vec<_>>();
        assert_eq!(records[0].get(0), Some("a"));
        assert_eq!(records[0].get(1), Some("route1"));
        assert_eq!(records[0].get(2), Some("0"));
        let at = |row: usize, name: &str| {
            records[row]
                .get(headers.iter().position(|header| header == name).unwrap())
                .unwrap()
        };
        assert_eq!(at(0, "comparison_longitude_deg"), "30");
        assert_eq!(at(0, "comparison_latitude_deg"), "-2");
        assert_eq!(at(0, "rwanda_elevation_m"), "1600");
        assert_eq!(at(0, "delta_nearest_minus_rwanda_m"), "100");
        assert_eq!(at(1, "rwanda_elevation_m"), "");
        assert_eq!(at(1, "terrarium_nearest_elevation_m"), "1700");
        assert_eq!(at(1, "delta_nearest_minus_rwanda_m"), "");
        assert!(receipt.fetches.iter().any(|fetch| fetch.range.is_some()));
        assert!(
            receipt
                .fetches
                .iter()
                .all(|fetch| fetch.sha256.starts_with("sha256:"))
        );
    }

    #[test]
    fn transparent_terrarium_is_missing_and_never_replaced_by_rwanda() {
        let dir = Temp::new("alpha");
        let source = dir.0.join("input.csv");
        let out = dir.0.join("output.csv");
        fs::write(&source, "x,y\n30,-2\n").unwrap();
        let mut fixture = Fixture::new();
        fixture.transparent = true;
        let receipt = run_fixture(&source, &out, &ComparisonOptions::default(), fixture).unwrap();
        assert_eq!(receipt.rwanda_count, 1);
        assert_eq!(receipt.terrarium_nearest_missing_count, 1);
        assert_eq!(receipt.terrarium_bilinear_missing_count, Some(1));
        assert_eq!(receipt.paired_nearest_count, 0);
    }

    #[test]
    fn provider_failure_produces_no_partial_comparison() {
        let dir = Temp::new("failure");
        let source = dir.0.join("input.csv");
        let out = dir.0.join("output.csv");
        fs::write(&source, "x,y\n30,-2\n").unwrap();
        let mut fixture = Fixture::new();
        fixture.fail_terrarium = true;
        assert_eq!(
            run_fixture(&source, &out, &ComparisonOptions::default(), fixture)
                .unwrap_err()
                .code(),
            "elevation_provider_failed"
        );
        assert!(!out.exists());
    }

    #[test]
    fn range_status_header_length_and_total_are_exact() {
        let valid = || Response {
            status: 206,
            content_range: Some("bytes 10-13/100".into()),
            body: vec![1; 4],
        };
        assert!(validate_range(&valid(), 10, 4, 100).is_ok());
        let mut response = valid();
        response.status = 200;
        assert!(validate_range(&response, 10, 4, 100).is_err());
        let mut response = valid();
        response.content_range = Some("bytes 10-13/101".into());
        assert!(validate_range(&response, 10, 4, 100).is_err());
        let mut response = valid();
        response.body.pop();
        assert!(validate_range(&response, 10, 4, 100).is_err());
        assert!(validate_range(&valid(), u64::MAX, 4, 100).is_err());
        assert!(validate_range(&valid(), 10, 0, 100).is_err());
    }

    fn pixel_to_wgs84(x: f64, y: f64) -> [f64; 2] {
        [
            x / WORLD_PIXELS as f64 * 360.0 - 180.0,
            (std::f64::consts::PI * (1.0 - 2.0 * y / WORLD_PIXELS as f64))
                .sinh()
                .atan()
                .to_degrees(),
        ]
    }

    #[test]
    fn bilinear_plane_uses_neighbour_tiles_in_both_directions() {
        let (ox, oy) = (19000 * 256, 17000 * 256);
        let point = pixel_to_wgs84(ox as f64 + 0.25, oy as f64 + 0.75);
        let mut fixture = Fixture::new();
        fixture.plane_origin = Some((ox, oy));
        let mut fetcher = Fetcher {
            transport: fixture,
            evidence: Vec::new(),
            bytes: 0,
        };
        let (nearest, bilinear) = terrarium(&mut fetcher, &[point], true).unwrap();
        // Raster centres are at integer world pixels + 0.5. The continuous
        // plane at this query is -0.25 X, +0.25 Y from pixel (ox,oy).
        assert!((bilinear[0].unwrap() - 100.25).abs() < 1.0e-6);
        assert_eq!(nearest[0], Some(100.0));
        assert_eq!(fetcher.evidence.len(), 2);
        assert!(
            fetcher
                .evidence
                .iter()
                .any(|fetch| fetch.url.contains("/18999/17000.png"))
        );
        // A query straddling both boundaries needs all four neighbouring tiles.
        let point = pixel_to_wgs84(ox as f64 + 0.25, oy as f64 + 0.25);
        let mut fixture = Fixture::new();
        fixture.plane_origin = Some((ox, oy));
        let mut fetcher = Fetcher {
            transport: fixture,
            evidence: Vec::new(),
            bytes: 0,
        };
        let (_, bilinear) = terrarium(&mut fetcher, &[point], true).unwrap();
        assert!((bilinear[0].unwrap() - 98.75).abs() < 1.0e-6);
        assert_eq!(fetcher.evidence.len(), 4);
    }

    #[test]
    fn output_collision_and_existing_file_fail_before_provider_work() {
        let dir = Temp::new("collision");
        let source = dir.0.join("input.csv");
        let out = dir.0.join("output.csv");
        fs::write(&source, "x,y,rwanda_elevation_m\n30,-2,99\n").unwrap();
        assert_eq!(
            run_fixture(&source, &out, &ComparisonOptions::default(), Fixture::new())
                .unwrap_err()
                .code(),
            "elevation_output_column_collision"
        );
        fs::write(&source, "x,y\n30,-2\n").unwrap();
        fs::write(&out, "original").unwrap();
        assert_eq!(
            run_fixture(&source, &out, &ComparisonOptions::default(), Fixture::new())
                .unwrap_err()
                .code(),
            "elevation_output_exists"
        );
        assert_eq!(fs::read_to_string(out).unwrap(), "original");
    }

    #[test]
    fn projection_uses_existing_crs_authority_and_limit_is_hard() {
        let crs = GridModelCrs::parse("EPSG:32735").unwrap();
        let xy = crs.from_map([30.0, -2.0]).unwrap();
        let converted = SourceCrs::parse("EPSG:32735").unwrap().wgs84(xy).unwrap();
        assert!((converted[0] - 30.0).abs() < 1.0e-7);
        assert!((converted[1] + 2.0).abs() < 1.0e-7);
        assert!(SourceCrs::parse("EPSG:4083").is_err());
        let dir = Temp::new("point-limit");
        let source = dir.0.join("input.csv");
        let out = dir.0.join("output.csv");
        fs::write(
            &source,
            format!("x,y\n{}", "30,-2\n".repeat(POINT_LIMIT + 1)),
        )
        .unwrap();
        assert_eq!(
            run_fixture(&source, &out, &ComparisonOptions::default(), Fixture::new())
                .unwrap_err()
                .code(),
            "elevation_point_limit"
        );
        assert!(!out.exists());
    }

    #[test]
    fn index_digest_mismatch_is_a_failure_without_output() {
        let dir = Temp::new("index");
        let source = dir.0.join("input.csv");
        let out = dir.0.join("output.csv");
        fs::write(&source, "x,y\n30,-2\n").unwrap();
        let fixture = Fixture::new();
        let (size, _, tiff_size) = fixture.contract();
        let error = compare_with(
            &source,
            &out,
            &ComparisonOptions::default(),
            fixture,
            IndexContract {
                size,
                digest: "incorrect",
                tiff_size,
            },
        )
        .unwrap_err();
        assert_eq!(error.code(), "elevation_integrity_failed");
        assert!(!out.exists());
    }
}
