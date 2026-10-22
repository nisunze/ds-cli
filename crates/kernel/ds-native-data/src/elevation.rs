//! Shared native application elevation interpolation.
//!
//! Source bytes and results stay on this machine. Rwanda samples come from a
//! verified installed COG or exact public GCS byte ranges; AWS Terrarium is an
//! explicit fallback provider. No DS API, Cloud Run route, or project upload is
//! reachable from this module.

use crate::RequestContext;
use ds_elevation::{
    assess_admission, attach_elevation, stamp_point_elevation, ElevationExecutionHost,
    ElevationLayer, ElevationOptions, ElevationProvider, ElevationResult, RwandaBlockCache,
    RwandaBlockProvider, RwandaBlockRequest, RwandaCoverage, RwandaDem, RwandaDemAccess,
    SurfaceCoverageAccumulator,
};
use ds_geo::projection::{nix_itrf2005_to_wgs84, wgs84_to_nix_itrf2005};
use ds_io::{
    geojson_text_to_layer, kml_text_to_layer, kmz_bytes_to_layer, layers_to_geojson_string,
    shp_zip_bytes_to_layer, source_inspect, source_to_layer, ColumnRename, GeoLayerExportInput,
    GeoLayerImportEnvelope, GeoLayerInput, GeoTextImportInput, SourceInspectInput,
    SourceLayerInput,
};
use image::ImageFormat;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

const MAX_MATERIALIZED_SOURCE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_MATERIALIZED_POINTS: usize = 1_000_000;
const STREAM_BATCH_POINTS: usize = 4_096;
const TERRARIUM_ZOOM: u32 = 15;
const MAX_TERRARIUM_TILE_BYTES: u64 = 2 * 1024 * 1024;
pub(crate) const HTTP_TIMEOUT: Duration = Duration::from_secs(30);
const TASK: &str = "native_elevation_interpolate";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeElevationSourceCrs {
    NixItrf2005,
    Wgs84Lonlat,
}

impl NativeElevationSourceCrs {
    fn ds_io_id(self) -> &'static str {
        match self {
            Self::NixItrf2005 => "nix_itrf2005",
            Self::Wgs84Lonlat => "wgs84_lonlat",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeElevationRequest {
    source_path: String,
    output_path: String,
    #[serde(default = "default_x_column")]
    x_column: String,
    #[serde(default = "default_y_column")]
    y_column: String,
    #[serde(default = "default_source_crs")]
    source_crs: NativeElevationSourceCrs,
    #[serde(default)]
    separator: Option<String>,
    #[serde(default)]
    common_column: Option<String>,
    #[serde(default = "default_true")]
    use_terrarium_fallback: bool,
}

impl NativeElevationRequest {
    pub(crate) fn validate(&self) -> Result<(), String> {
        crate::bounded_text(&self.source_path, "sourcePath", 4096)?;
        crate::bounded_text(&self.output_path, "outputPath", 4096)?;
        crate::bounded_text(&self.x_column, "xColumn", 200)?;
        crate::bounded_text(&self.y_column, "yColumn", 200)?;
        if let Some(column) = &self.common_column {
            crate::bounded_text(column, "commonColumn", 200)?;
        }
        if let Some(separator) = &self.separator {
            if separator.chars().count() != 1 {
                return Err("separator must be exactly one character".into());
            }
        }
        Ok(())
    }
}

fn default_x_column() -> String {
    "x".to_string()
}

fn default_y_column() -> String {
    "y".to_string()
}

fn default_source_crs() -> NativeElevationSourceCrs {
    NativeElevationSourceCrs::NixItrf2005
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeElevationEvidence {
    task: &'static str,
    execution_host: &'static str,
    source_path: String,
    source_bytes: u64,
    output_path: String,
    output_bytes: u64,
    output_sha256: String,
    output_format: &'static str,
    point_count: usize,
    interpolated_count: usize,
    rwanda_count: usize,
    fallback_count: usize,
    unresolved_count: usize,
    fallback_required_count: usize,
    rwanda_coverage: String,
    rwanda_source: String,
    // Absent, never null, when no fallback ran: the desktop evidence contract declares
    // `fallbackSource?: 'aws_terrarium_30m'`, and a null was refused as mismatched evidence.
    #[serde(skip_serializing_if = "Option::is_none")]
    fallback_source: Option<String>,
    dem_access: &'static str,
    direct_public_range_reads: bool,
    full_local_required_above_points: u32,
    cloud_service_used: bool,
    admin_enrichment: &'static str,
    layers: Vec<ds_elevation::ElevationLayerStats>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeElevationRefusalCode {
    FullLocalDemRequired,
    MaterializedFormatBound,
    StreamGroupBound,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeElevationRefusal {
    code: NativeElevationRefusalCode,
    message: String,
    point_count: Option<usize>,
    point_limit: Option<usize>,
    source_bytes: Option<u64>,
    source_byte_limit: Option<u64>,
    remedy: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum NativeElevationOutcome {
    Completed { evidence: NativeElevationEvidence },
    Refused { refusal: NativeElevationRefusal },
}

fn absolute_regular_source(raw: &str) -> Result<(PathBuf, u64), String> {
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err("Elevation source_path must be absolute".to_string());
    }
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|error| format!("inspect elevation source: {error}"))?;
    if ds_update_runtime::transfer::metadata_is_link_or_reparse(&metadata) || !metadata.is_file() {
        return Err("Elevation source must be a regular file, not a link".to_string());
    }
    if metadata.len() == 0 {
        return Err("Elevation source must not be empty".to_string());
    }
    Ok((path, metadata.len()))
}

pub(crate) fn absolute_new_output(raw: &str, source: &Path) -> Result<PathBuf, String> {
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err("Elevation output_path must be absolute".to_string());
    }
    if path == source {
        return Err("Elevation output_path must differ from source_path".to_string());
    }
    if path.exists() {
        return Err("Elevation output already exists; overwrite is refused".to_string());
    }
    if path.extension().and_then(|value| value.to_str()) != Some("geojson") {
        return Err("Elevation output_path must end in .geojson".to_string());
    }
    let parent = path
        .parent()
        .ok_or_else(|| "Elevation output has no parent directory".to_string())?;
    let metadata = std::fs::symlink_metadata(parent)
        .map_err(|error| format!("inspect elevation output directory: {error}"))?;
    if ds_update_runtime::transfer::metadata_is_link_or_reparse(&metadata) || !metadata.is_dir() {
        return Err("Elevation output directory must be a real directory".to_string());
    }
    Ok(path)
}

fn parse_source(
    path: &Path,
    bytes: &[u8],
    request: &NativeElevationRequest,
) -> Result<Vec<ElevationLayer>, String> {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| "Elevation source file name is not valid UTF-8".to_string())?;
    let lower = name.to_ascii_lowercase();
    let envelope = if lower.ends_with(".csv") || lower.ends_with(".tsv") || lower.ends_with(".tab")
    {
        let inspection = source_inspect(
            name,
            bytes,
            SourceInspectInput {
                separator: request.separator.clone(),
            },
        )?;
        let sheet = inspection
            .sheets
            .first()
            .ok_or_else(|| "Elevation table has no readable sheet".to_string())?;
        let materialized = source_to_layer(
            name,
            bytes,
            SourceLayerInput {
                sheet_key: sheet.key.clone(),
                separator: request.separator.clone(),
                header_row: None,
                x_column: Some(request.x_column.trim().to_string()),
                y_column: Some(request.y_column.trim().to_string()),
                crs: request.source_crs.ds_io_id().to_string(),
                utm_zone: None,
                utm_southern: None,
                attributes_only: false,
                layer_name: None,
                column_renames: Vec::<ColumnRename>::new(),
                drop_columns: Vec::new(),
            },
        )?;
        materialized.envelope
    } else if lower.ends_with(".geojson") || lower.ends_with(".json") {
        let text =
            std::str::from_utf8(bytes).map_err(|_| "GeoJSON source must be UTF-8".to_string())?;
        geojson_text_to_layer(GeoTextImportInput {
            name: name.to_string(),
            text: text.to_string(),
        })?
    } else if lower.ends_with(".kml") {
        let text =
            std::str::from_utf8(bytes).map_err(|_| "KML source must be UTF-8".to_string())?;
        kml_text_to_layer(GeoTextImportInput {
            name: name.to_string(),
            text: text.to_string(),
        })?
    } else if lower.ends_with(".kmz") {
        kmz_bytes_to_layer(name, bytes)?
    } else if lower.ends_with(".shp.zip") || lower.ends_with(".zip") {
        shp_zip_bytes_to_layer(name, bytes)?
    } else {
        return Err(
            "Elevation source must be CSV/TSV, GeoJSON, KML, KMZ, or a zipped shapefile"
                .to_string(),
        );
    };
    point_layers(envelope)
}

fn point_layers(envelope: GeoLayerImportEnvelope) -> Result<Vec<ElevationLayer>, String> {
    let mut layers = Vec::new();
    for input in envelope.layers {
        let mut features = Vec::new();
        for feature in input.features {
            let geometry = feature.get("geometry").ok_or_else(|| {
                format!("Layer '{}' contains a feature without geometry", input.name)
            })?;
            match geometry.get("type").and_then(Value::as_str) {
                Some("Point") => features.push(feature),
                Some("MultiPoint") => {
                    let coordinates = geometry
                        .get("coordinates")
                        .and_then(Value::as_array)
                        .ok_or_else(|| {
                            format!("Layer '{}' has an invalid MultiPoint", input.name)
                        })?;
                    for coordinate in coordinates {
                        let mut exploded = feature.clone();
                        exploded["geometry"] = json!({
                            "type": "Point",
                            "coordinates": coordinate,
                        });
                        features.push(exploded);
                    }
                }
                other => {
                    return Err(format!(
                        "Layer '{}' contains unsupported {:?} geometry; elevation accepts points only",
                        input.name, other
                    ));
                }
            }
        }
        if !features.is_empty() {
            layers.push(ElevationLayer {
                name: input.name,
                features,
            });
        }
    }
    if layers.is_empty() {
        return Err("Elevation source contains no point features".to_string());
    }
    Ok(layers)
}

fn is_streamed_text(path: &Path) -> bool {
    path.extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "csv" | "tsv" | "tab"))
}

fn text_delimiter(path: &Path, requested: Option<&str>) -> Result<u8, String> {
    match requested.map(str::trim).filter(|value| !value.is_empty()) {
        Some(",") | Some("comma") => Ok(b','),
        Some(";") | Some("semicolon") => Ok(b';'),
        Some("\\t") | Some("tab") => Ok(b'\t'),
        Some("|") | Some("pipe") => Ok(b'|'),
        Some(other) => Err(format!("Unsupported elevation table separator '{other}'")),
        None if path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "tsv" | "tab")) =>
        {
            Ok(b'\t')
        }
        None => Ok(b','),
    }
}

#[derive(Clone)]
struct StreamPoint {
    feature: Value,
    wgs84: [f64; 2],
    nix: [f64; 2],
}

fn for_each_text_batch<F>(
    path: &Path,
    request: &NativeElevationRequest,
    mut visit: F,
) -> Result<usize, String>
where
    F: FnMut(&mut Vec<StreamPoint>) -> Result<(), String>,
{
    let delimiter = text_delimiter(path, request.separator.as_deref())?;
    let file = File::open(path).map_err(|error| format!("open elevation table: {error}"))?;
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(false)
        .from_reader(file);
    let headers = reader
        .headers()
        .map_err(|error| format!("read elevation table header: {error}"))?
        .iter()
        .map(|value| value.trim().to_string())
        .collect::<Vec<_>>();
    let column = |wanted: &str| {
        headers
            .iter()
            .position(|header| header.eq_ignore_ascii_case(wanted.trim()))
    };
    let x_index = column(&request.x_column)
        .ok_or_else(|| format!("Elevation table has no '{}' column", request.x_column))?;
    let y_index = column(&request.y_column)
        .ok_or_else(|| format!("Elevation table has no '{}' column", request.y_column))?;
    if x_index == y_index {
        return Err("Elevation x and y columns must differ".to_string());
    }
    let mut batch = Vec::with_capacity(STREAM_BATCH_POINTS);
    let mut count = 0_usize;
    for record in reader.records() {
        let record = record.map_err(|error| format!("read elevation table row: {error}"))?;
        let x = record
            .get(x_index)
            .ok_or_else(|| "Elevation x value is missing".to_string())?
            .trim()
            .parse::<f64>()
            .map_err(|_| format!("Elevation x must be numeric at row {}", count + 2))?;
        let y = record
            .get(y_index)
            .ok_or_else(|| "Elevation y value is missing".to_string())?
            .trim()
            .parse::<f64>()
            .map_err(|_| format!("Elevation y must be numeric at row {}", count + 2))?;
        if !x.is_finite() || !y.is_finite() {
            return Err(format!(
                "Elevation coordinates must be finite at row {}",
                count + 2
            ));
        }
        let (wgs84, nix) = match request.source_crs {
            NativeElevationSourceCrs::NixItrf2005 => {
                let (lon, lat) = nix_itrf2005_to_wgs84(x, y);
                ([lon, lat], [x, y])
            }
            NativeElevationSourceCrs::Wgs84Lonlat => {
                if !(-180.0..=180.0).contains(&x) || !(-90.0..=90.0).contains(&y) {
                    return Err(format!(
                        "WGS84 coordinate is out of range at row {}",
                        count + 2
                    ));
                }
                let (nix_x, nix_y) = wgs84_to_nix_itrf2005(x, y);
                ([x, y], [nix_x, nix_y])
            }
        };
        let properties = headers
            .iter()
            .zip(record.iter())
            .map(|(key, value)| (key.clone(), Value::String(value.to_string())))
            .collect::<serde_json::Map<_, _>>();
        batch.push(StreamPoint {
            feature: json!({
                "type": "Feature",
                "geometry": { "type": "Point", "coordinates": wgs84 },
                "properties": properties,
            }),
            wgs84,
            nix,
        });
        count += 1;
        if batch.len() == STREAM_BATCH_POINTS {
            visit(&mut batch)?;
            batch.clear();
        }
    }
    if !batch.is_empty() {
        visit(&mut batch)?;
    }
    if count == 0 {
        return Err("Elevation table has no data rows".to_string());
    }
    Ok(count)
}

fn preflight_text_count(path: &Path, request: &NativeElevationRequest) -> Result<usize, String> {
    for_each_text_batch(path, request, |_| Ok(()))
}

pub(crate) trait RangeFetcher {
    fn fetch(&mut self, url: &str, offset: u64, length: u64, total: u64)
        -> Result<Vec<u8>, String>;
}

pub(crate) struct HttpRangeFetcher {
    agent: ureq::Agent,
    check_online: crate::OnlineCheck,
}

impl HttpRangeFetcher {
    pub(crate) fn new(check_online: crate::OnlineCheck) -> Self {
        Self {
            check_online,
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(HTTP_TIMEOUT))
                .timeout_per_call(Some(HTTP_TIMEOUT))
                .timeout_connect(Some(Duration::from_secs(10)))
                .timeout_recv_response(Some(HTTP_TIMEOUT))
                .timeout_recv_body(Some(HTTP_TIMEOUT))
                .build()
                .new_agent(),
        }
    }
}

fn validate_range_response(
    status: u16,
    content_range: Option<&str>,
    offset: u64,
    length: u64,
    total: u64,
) -> Result<(), String> {
    let end = offset
        .checked_add(length)
        .and_then(|value| value.checked_sub(1))
        .ok_or_else(|| "Rwanda DEM byte range overflowed".to_string())?;
    let expected = format!("bytes {offset}-{end}/{total}");
    if status != 206 {
        return Err(format!(
            "Rwanda DEM range server returned HTTP {status}; exact HTTP 206 is required"
        ));
    }
    if content_range != Some(expected.as_str()) {
        return Err(format!(
            "Rwanda DEM range server returned a mismatched Content-Range; expected {expected}"
        ));
    }
    Ok(())
}

impl RangeFetcher for HttpRangeFetcher {
    fn fetch(
        &mut self,
        url: &str,
        offset: u64,
        length: u64,
        total: u64,
    ) -> Result<Vec<u8>, String> {
        (self.check_online)()?;
        let end = offset
            .checked_add(length)
            .and_then(|value| value.checked_sub(1))
            .ok_or_else(|| "Rwanda DEM byte range overflowed".to_string())?;
        let response = self
            .agent
            .get(url)
            .header("Range", &format!("bytes={offset}-{end}"))
            .call()
            .map_err(|error| format!("Rwanda DEM public range transport failed: {error}"))?;
        let status = response.status().as_u16();
        let content_range = response
            .headers()
            .get("content-range")
            .and_then(|value| value.to_str().ok());
        validate_range_response(status, content_range, offset, length, total)?;
        let mut reader = response.into_body().into_reader().take(length + 1);
        let mut bytes = Vec::with_capacity(length as usize);
        reader
            .read_to_end(&mut bytes)
            .map_err(|error| format!("read Rwanda DEM public byte range: {error}"))?;
        if bytes.len() != length as usize {
            return Err(format!(
                "Rwanda DEM range returned {} bytes; expected {length}",
                bytes.len()
            ));
        }
        Ok(bytes)
    }
}

pub(crate) struct CachedRangeProvider<F> {
    pub(crate) url: String,
    pub(crate) total: u64,
    pub(crate) cache: PathBuf,
    pub(crate) max_bytes: u64,
    pub(crate) max_entries: u32,
    pub(crate) fetcher: F,
}

impl<F: RangeFetcher> CachedRangeProvider<F> {
    fn cache_path(&self, request: &RwandaBlockRequest) -> PathBuf {
        self.cache.join(format!(
            "block-{}-{}-{}-{}.bin",
            request.key.i, request.key.j, request.offset, request.length
        ))
    }

    fn read_cache(path: &Path, expected: u64) -> Result<Option<Vec<u8>>, String> {
        let Ok(metadata) = std::fs::symlink_metadata(path) else {
            return Ok(None);
        };
        if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() != expected {
            std::fs::remove_file(path)
                .map_err(|error| format!("remove invalid Rwanda DEM cache entry: {error}"))?;
            return Ok(None);
        }
        std::fs::read(path)
            .map(Some)
            .map_err(|error| format!("read Rwanda DEM block cache: {error}"))
    }

    fn trim_cache(&self, incoming: u64) -> Result<(), String> {
        let mut entries = Vec::<(PathBuf, u64, SystemTime)>::new();
        for entry in std::fs::read_dir(&self.cache)
            .map_err(|error| format!("inspect Rwanda DEM block cache: {error}"))?
        {
            let entry =
                entry.map_err(|error| format!("inspect Rwanda DEM cache entry: {error}"))?;
            let metadata = entry
                .metadata()
                .map_err(|error| format!("inspect Rwanda DEM cache metadata: {error}"))?;
            if metadata.is_file() {
                entries.push((
                    entry.path(),
                    metadata.len(),
                    metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                ));
            }
        }
        entries.sort_by_key(|entry| entry.2);
        let mut bytes = entries.iter().map(|entry| entry.1).sum::<u64>();
        let mut count = entries.len() as u32;
        for (path, size, _) in entries {
            if bytes.saturating_add(incoming) <= self.max_bytes && count < self.max_entries {
                break;
            }
            std::fs::remove_file(&path)
                .map_err(|error| format!("evict Rwanda DEM cache entry: {error}"))?;
            bytes = bytes.saturating_sub(size);
            count = count.saturating_sub(1);
        }
        if incoming > self.max_bytes || self.max_entries == 0 {
            return Err("Rwanda DEM block exceeds the governed cache bound".to_string());
        }
        Ok(())
    }

    fn write_cache(&self, path: &Path, bytes: &[u8]) -> Result<(), String> {
        self.trim_cache(bytes.len() as u64)?;
        let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
        let mut file = ds_layer_store::private::open_options()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| format!("create Rwanda DEM cache entry: {error}"))?;
        let result = (|| {
            file.write_all(bytes)
                .map_err(|error| format!("write Rwanda DEM cache entry: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("sync Rwanda DEM cache entry: {error}"))?;
            std::fs::rename(&temp, path)
                .map_err(|error| format!("admit Rwanda DEM cache entry: {error}"))
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        result
    }
}

impl<F: RangeFetcher> RwandaBlockProvider for CachedRangeProvider<F> {
    fn read_compressed_block(
        &mut self,
        request: &RwandaBlockRequest,
    ) -> Result<Vec<u8>, ds_elevation::ElevationError> {
        let path = self.cache_path(request);
        if let Some(bytes) = Self::read_cache(&path, request.length).map_err(|message| {
            ds_elevation::ElevationError::Provider {
                provider_id: "rwanda_dem_public_range".to_string(),
                message,
            }
        })? {
            return Ok(bytes);
        }
        let bytes = self
            .fetcher
            .fetch(&self.url, request.offset, request.length, self.total)
            .map_err(|message| ds_elevation::ElevationError::Provider {
                provider_id: "rwanda_dem_public_range".to_string(),
                message,
            })?;
        self.write_cache(&path, &bytes).map_err(|message| {
            ds_elevation::ElevationError::Provider {
                provider_id: "rwanda_dem_public_range".to_string(),
                message,
            }
        })?;
        Ok(bytes)
    }
}

pub(crate) struct LocalCogProvider {
    pub(crate) file: File,
}

impl RwandaBlockProvider for LocalCogProvider {
    fn read_compressed_block(
        &mut self,
        request: &RwandaBlockRequest,
    ) -> Result<Vec<u8>, ds_elevation::ElevationError> {
        self.file
            .seek(SeekFrom::Start(request.offset))
            .map_err(|error| ds_elevation::ElevationError::Provider {
                provider_id: "rwanda_dem_installed_full".to_string(),
                message: error.to_string(),
            })?;
        let mut bytes = vec![0_u8; request.length as usize];
        self.file.read_exact(&mut bytes).map_err(|error| {
            ds_elevation::ElevationError::Provider {
                provider_id: "rwanda_dem_installed_full".to_string(),
                message: error.to_string(),
            }
        })?;
        Ok(bytes)
    }
}

pub(crate) struct TerrariumProvider {
    agent: ureq::Agent,
    check_online: crate::OnlineCheck,
    url_template: String,
}

impl TerrariumProvider {
    pub(crate) fn new(url_template: &str, check_online: crate::OnlineCheck) -> Self {
        Self {
            check_online,
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(HTTP_TIMEOUT))
                .timeout_per_call(Some(HTTP_TIMEOUT))
                .timeout_connect(Some(Duration::from_secs(10)))
                .timeout_recv_response(Some(HTTP_TIMEOUT))
                .timeout_recv_body(Some(HTTP_TIMEOUT))
                .build()
                .new_agent(),
            url_template: url_template.to_string(),
        }
    }
}

fn terrarium_tile_pixel(lon: f64, lat: f64, zoom: u32) -> (u32, u32, u8, u8) {
    let scale = f64::from(1_u32 << zoom);
    let world_x = (lon + 180.0) / 360.0 * scale;
    let lat_rad = lat.clamp(-85.05112878, 85.05112878).to_radians();
    let world_y =
        (1.0 - (lat_rad.tan() + 1.0 / lat_rad.cos()).ln() / std::f64::consts::PI) * 0.5 * scale;
    let tile_x = world_x.floor().clamp(0.0, scale - 1.0) as u32;
    let tile_y = world_y.floor().clamp(0.0, scale - 1.0) as u32;
    let px = ((world_x - f64::from(tile_x)) * 256.0)
        .floor()
        .clamp(0.0, 255.0) as u8;
    let py = ((world_y - f64::from(tile_y)) * 256.0)
        .floor()
        .clamp(0.0, 255.0) as u8;
    (tile_x, tile_y, px, py)
}

fn decode_terrarium_rgb(rgb: [u8; 3]) -> f64 {
    f64::from(rgb[0]) * 256.0 + f64::from(rgb[1]) + f64::from(rgb[2]) / 256.0 - 32768.0
}

impl ElevationProvider for TerrariumProvider {
    fn source_id(&self) -> &str {
        "aws_terrarium_30m"
    }

    fn sample_wgs84(
        &mut self,
        points: &[[f64; 2]],
    ) -> Result<Vec<Option<f64>>, ds_elevation::ElevationError> {
        let mut grouped = BTreeMap::<(u32, u32), Vec<(usize, u8, u8)>>::new();
        for (index, point) in points.iter().enumerate() {
            let (x, y, px, py) = terrarium_tile_pixel(point[0], point[1], TERRARIUM_ZOOM);
            grouped.entry((x, y)).or_default().push((index, px, py));
        }
        let mut output = vec![None; points.len()];
        for ((x, y), samples) in grouped {
            let url = self
                .url_template
                .replace("{z}", &TERRARIUM_ZOOM.to_string())
                .replace("{x}", &x.to_string())
                .replace("{y}", &y.to_string());
            (self.check_online)().map_err(|message| ds_elevation::ElevationError::Provider {
                provider_id: self.source_id().to_string(),
                message,
            })?;
            let response = self.agent.get(&url).call().map_err(|error| {
                ds_elevation::ElevationError::Provider {
                    provider_id: self.source_id().to_string(),
                    message: format!("Terrarium transport failed: {error}"),
                }
            })?;
            if response.status().as_u16() != 200 {
                return Err(ds_elevation::ElevationError::Provider {
                    provider_id: self.source_id().to_string(),
                    message: format!("Terrarium returned HTTP {}", response.status()),
                });
            }
            let mut reader = response
                .into_body()
                .into_reader()
                .take(MAX_TERRARIUM_TILE_BYTES + 1);
            let mut bytes = Vec::new();
            reader.read_to_end(&mut bytes).map_err(|error| {
                ds_elevation::ElevationError::Provider {
                    provider_id: self.source_id().to_string(),
                    message: format!("read Terrarium tile: {error}"),
                }
            })?;
            if bytes.len() as u64 > MAX_TERRARIUM_TILE_BYTES {
                return Err(ds_elevation::ElevationError::Provider {
                    provider_id: self.source_id().to_string(),
                    message: "Terrarium tile exceeded the bounded response size".to_string(),
                });
            }
            let pixels = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
                .map_err(|error| ds_elevation::ElevationError::Provider {
                    provider_id: self.source_id().to_string(),
                    message: format!("decode Terrarium PNG: {error}"),
                })?
                .to_rgb8();
            if pixels.width() != 256 || pixels.height() != 256 {
                return Err(ds_elevation::ElevationError::Provider {
                    provider_id: self.source_id().to_string(),
                    message: "Terrarium tile must be exactly 256 by 256 pixels".to_string(),
                });
            }
            for (index, px, py) in samples {
                let rgb = pixels.get_pixel(u32::from(px), u32::from(py)).0;
                output[index] = Some(decode_terrarium_rgb(rgb));
            }
        }
        Ok(output)
    }
}

fn sha256_file(path: &Path) -> Result<(u64, String), String> {
    let mut file = File::open(path).map_err(|error| format!("open elevation evidence: {error}"))?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; 1024 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("read elevation evidence: {error}"))?;
        if count == 0 {
            break;
        }
        total += count as u64;
        hasher.update(&buffer[..count]);
    }
    Ok((total, format!("{:x}", hasher.finalize())))
}

fn stream_text_elevation<P: RwandaBlockProvider>(
    source: &Path,
    output: &Path,
    request: &NativeElevationRequest,
    point_count: usize,
    dem: &RwandaDem,
    provider: &mut P,
    mut fallback: Option<&mut dyn ElevationProvider>,
) -> Result<(ElevationResult, u64, String), String> {
    let mut decoded = RwandaBlockCache::new(ds_elevation::DEFAULT_DECODED_BLOCK_CACHE)
        .map_err(|error| format!("create Rwanda DEM decoded cache: {error}"))?;
    let mut coverage = SurfaceCoverageAccumulator::new(request.common_column.as_deref());
    let mut covered_count = 0_usize;
    for_each_text_batch(source, request, |batch| {
        let nix = batch.iter().map(|point| point.nix).collect::<Vec<_>>();
        let samples = dem
            .sample_with_cache(provider, &mut decoded, &nix)
            .map_err(|error| format!("sample Rwanda DEM coverage: {error}"))?;
        for (point, sample) in batch.iter().zip(samples) {
            let covered = sample.is_some();
            covered_count += usize::from(covered);
            coverage.observe(0, &point.feature, covered);
        }
        Ok(())
    })?;
    let plan = coverage.finish();
    let temp = output.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|error| format!("create streamed elevation result: {error}"))?;
    file.write_all(b"{\n  \"type\": \"FeatureCollection\",\n  \"features\": [\n")
        .map_err(|error| format!("write streamed elevation header: {error}"))?;
    let mut wrote = 0_usize;
    let mut interpolated = 0_usize;
    let mut rwanda_count = 0_usize;
    let mut fallback_count = 0_usize;
    let mut unresolved_count = 0_usize;
    let mut fallback_required_count = 0_usize;
    let stream_result = for_each_text_batch(source, request, |batch| {
        let nix = batch.iter().map(|point| point.nix).collect::<Vec<_>>();
        let rwanda = dem
            .sample_with_cache(provider, &mut decoded, &nix)
            .map_err(|error| format!("sample Rwanda DEM output: {error}"))?;
        let choices = batch
            .iter()
            .map(|point| plan.use_rwanda(0, &point.feature))
            .collect::<Vec<_>>();
        let fallback_points = batch
            .iter()
            .zip(&choices)
            .filter_map(|(point, use_rwanda)| (!use_rwanda).then_some(point.wgs84))
            .collect::<Vec<_>>();
        fallback_required_count += fallback_points.len();
        let fallback_samples = if let Some(fallback) = fallback.as_mut() {
            fallback
                .sample_wgs84(&fallback_points)
                .map_err(|error| format!("sample Terrarium fallback: {error}"))?
        } else {
            vec![None; fallback_points.len()]
        };
        let fallback_source = fallback.as_ref().map(|provider| provider.source_id());
        let mut fallback_cursor = 0_usize;
        for (index, point) in batch.iter_mut().enumerate() {
            let (sample, source_id) = if choices[index] {
                let sample = rwanda[index];
                rwanda_count += usize::from(sample.is_some());
                (sample, sample.map(|_| ds_elevation::RWANDA_DEM_SOURCE))
            } else {
                let sample = fallback_samples[fallback_cursor].filter(|value| value.is_finite());
                fallback_cursor += 1;
                fallback_count += usize::from(sample.is_some());
                (sample, sample.and(fallback_source))
            };
            interpolated += usize::from(sample.is_some());
            unresolved_count += usize::from(sample.is_none());
            stamp_point_elevation(&mut point.feature, point.nix, sample, source_id)
                .map_err(|error| format!("stamp elevation result: {error}"))?;
            point.feature["properties"]["_layer"] = Value::String("elevation_points".to_string());
            if wrote > 0 {
                file.write_all(b",\n")
                    .map_err(|error| format!("write streamed elevation separator: {error}"))?;
            }
            serde_json::to_writer(&mut file, &point.feature)
                .map_err(|error| format!("write streamed elevation feature: {error}"))?;
            wrote += 1;
        }
        Ok(())
    });
    if let Err(error) = stream_result {
        drop(file);
        let _ = std::fs::remove_file(&temp);
        return Err(error);
    }
    file.write_all(b"\n  ]\n}\n")
        .and_then(|_| file.sync_all())
        .map_err(|error| format!("finish streamed elevation result: {error}"))?;
    drop(file);
    let (output_bytes, output_sha256) = sha256_file(&temp)?;
    std::fs::rename(&temp, output).map_err(|error| format!("admit elevation result: {error}"))?;
    let rwanda_coverage = if covered_count == 0 {
        RwandaCoverage::None
    } else if covered_count == point_count {
        RwandaCoverage::Full
    } else {
        RwandaCoverage::Partial
    };
    let fallback_source = fallback
        .as_ref()
        .map(|provider| provider.source_id().to_string());
    let stats = ds_elevation::ElevationLayerStats {
        layer: "elevation_points".to_string(),
        point_count,
        interpolated_count: interpolated,
        rwanda_count,
        fallback_count,
        unresolved_count,
    };
    Ok((
        ElevationResult {
            layers: Vec::new(),
            point_count,
            interpolated_count: interpolated,
            rwanda_count,
            fallback_count,
            unresolved_count,
            fallback_required_count,
            rwanda_source: ds_elevation::RWANDA_DEM_SOURCE.to_string(),
            rwanda_coverage,
            fallback_source,
            crs: ds_elevation::OUTPUT_CRS.to_string(),
            stats: vec![stats],
        },
        output_bytes,
        output_sha256,
    ))
}

fn render_geojson(result: &ElevationResult) -> Result<String, String> {
    layers_to_geojson_string(GeoLayerExportInput {
        layers: result
            .layers
            .iter()
            .map(|layer| GeoLayerInput {
                name: layer.name.clone(),
                geojson: json!({
                    "type": "FeatureCollection",
                    "features": layer.features,
                }),
            })
            .collect(),
    })
}

pub(crate) fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|error| format!("create elevation result: {error}"))?;
    let result = (|| {
        file.write_all(bytes)
            .map_err(|error| format!("write elevation result: {error}"))?;
        file.sync_all()
            .map_err(|error| format!("sync elevation result: {error}"))?;
        std::fs::rename(&temp, path).map_err(|error| format!("admit elevation result: {error}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

fn execute_with_provider<P: RwandaBlockProvider>(
    layers: Vec<ElevationLayer>,
    options: &ElevationOptions,
    dem: &RwandaDem,
    provider: &mut P,
    fallback: Option<&mut dyn ElevationProvider>,
) -> Result<ElevationResult, String> {
    attach_elevation(layers, options, dem, provider, fallback)
        .map_err(|error| format!("native elevation interpolation failed: {error}"))
}

/// One admitted local-file operation, including its required governed resource
/// and full-local retry. Every caller gets the same admission and recovery.
pub fn interpolate(
    context: &RequestContext,
    request: NativeElevationRequest,
) -> Result<NativeElevationOutcome, String> {
    context.validate()?;
    request.validate()?;
    crate::rwanda_dem::retry_full_local(
        || interpolate_once(context, request.clone()),
        |outcome| {
            matches!(
                outcome,
                NativeElevationOutcome::Refused {
                    refusal: NativeElevationRefusal {
                        code: NativeElevationRefusalCode::FullLocalDemRequired,
                        ..
                    }
                }
            )
        },
        || crate::rwanda_dem::ensure_available(context, true),
    )
}

fn interpolate_once(
    context: &RequestContext,
    request: NativeElevationRequest,
) -> Result<NativeElevationOutcome, String> {
    context.validate()?;
    request.validate()?;
    let (source, source_bytes) = absolute_regular_source(&request.source_path)?;
    let output = absolute_new_output(&request.output_path, &source)?;
    let streamed_text = is_streamed_text(&source);
    let (point_count, layers) = if streamed_text {
        (preflight_text_count(&source, &request)?, None)
    } else {
        if source_bytes > MAX_MATERIALIZED_SOURCE_BYTES {
            return Ok(NativeElevationOutcome::Refused {
                refusal: NativeElevationRefusal {
                    code: NativeElevationRefusalCode::MaterializedFormatBound,
                    message:
                        "This archive/geometry format exceeds the bounded materialization lane"
                            .to_string(),
                    point_count: None,
                    point_limit: Some(MAX_MATERIALIZED_POINTS),
                    source_bytes: Some(source_bytes),
                    source_byte_limit: Some(MAX_MATERIALIZED_SOURCE_BYTES),
                    remedy: "Convert the point source to CSV/TSV for the streamed Desktop lane"
                        .to_string(),
                },
            });
        }
        let bytes =
            std::fs::read(&source).map_err(|error| format!("read elevation source: {error}"))?;
        let parsed = parse_source(&source, &bytes, &request)?;
        let count = parsed
            .iter()
            .map(|layer| layer.features.len())
            .sum::<usize>();
        if count > MAX_MATERIALIZED_POINTS {
            return Ok(NativeElevationOutcome::Refused {
                refusal: NativeElevationRefusal {
                    code: NativeElevationRefusalCode::MaterializedFormatBound,
                    message:
                        "This archive/geometry format exceeds the bounded materialization lane"
                            .to_string(),
                    point_count: Some(count),
                    point_limit: Some(MAX_MATERIALIZED_POINTS),
                    source_bytes: Some(source_bytes),
                    source_byte_limit: Some(MAX_MATERIALIZED_SOURCE_BYTES),
                    remedy: "Convert the point source to CSV/TSV for the streamed Desktop lane"
                        .to_string(),
                },
            });
        }
        (count, Some(parsed))
    };
    if streamed_text
        && point_count > MAX_MATERIALIZED_POINTS
        && request
            .common_column
            .as_deref()
            .is_some_and(|column| !column.trim().is_empty())
    {
        return Ok(NativeElevationOutcome::Refused {
            refusal: NativeElevationRefusal {
                code: NativeElevationRefusalCode::StreamGroupBound,
                message: "A high-cardinality common-column plan cannot be held inside the bounded streaming lane"
                    .to_string(),
                point_count: Some(point_count),
                point_limit: Some(MAX_MATERIALIZED_POINTS),
                source_bytes: Some(source_bytes),
                source_byte_limit: None,
                remedy: "Run without commonColumn for one all-or-nothing layer surface, or split the table into explicit surface groups"
                    .to_string(),
            },
        });
    }

    crate::rwanda_dem::ensure_available(context, false)?;
    let runtime = crate::rwanda_dem::verified_rwanda_dem_runtime()?;
    let access = if runtime.mode == "offline" {
        RwandaDemAccess::InstalledFull
    } else {
        RwandaDemAccess::DirectRange
    };
    match assess_admission(point_count, ElevationExecutionHost::Desktop, access) {
        ds_elevation::ElevationAdmission::Accepted => {}
        ds_elevation::ElevationAdmission::InstalledDemRequired { .. } => {
            return Ok(NativeElevationOutcome::Refused {
                refusal: NativeElevationRefusal {
                    code: NativeElevationRefusalCode::FullLocalDemRequired,
                    message: format!(
                        "{point_count} points exceed the public-range Desktop admission limit"
                    ),
                    point_count: Some(point_count),
                    point_limit: Some(runtime.full_local_required_above_points as usize),
                    source_bytes: Some(source_bytes),
                    source_byte_limit: None,
                    remedy: "Install and verify the full Rwanda DEM Desktop component, then retry"
                        .to_string(),
                },
            });
        }
        ds_elevation::ElevationAdmission::DesktopRequired { .. } => {
            return Err(
                "Desktop elevation admission returned an invalid cloud-only decision".to_string(),
            );
        }
    }

    let index = std::fs::read(&runtime.index_path)
        .map_err(|error| format!("read verified Rwanda DEM block index: {error}"))?;
    let dem = RwandaDem::from_index_json(&index)
        .map_err(|error| format!("parse verified Rwanda DEM block index: {error}"))?;
    let options = ElevationOptions {
        common_column: request
            .common_column
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
    };
    let mut terrarium = request
        .use_terrarium_fallback
        .then(|| TerrariumProvider::new(runtime.terrarium_url_template, context.check_online));
    let fallback = terrarium
        .as_mut()
        .map(|provider| provider as &mut dyn ElevationProvider);

    let (result, output_bytes, output_sha256) = if let Some(cog_path) = runtime.cog_path.as_ref() {
        let metadata = std::fs::symlink_metadata(cog_path)
            .map_err(|error| format!("inspect installed Rwanda DEM COG: {error}"))?;
        if ds_update_runtime::transfer::metadata_is_link_or_reparse(&metadata)
            || !metadata.is_file()
            || metadata.len() != runtime.cog_size_bytes
        {
            return Err("Installed Rwanda DEM COG failed runtime file verification".to_string());
        }
        let mut provider = LocalCogProvider {
            file: File::open(cog_path)
                .map_err(|error| format!("open installed Rwanda DEM COG: {error}"))?,
        };
        if streamed_text {
            stream_text_elevation(
                &source,
                &output,
                &request,
                point_count,
                &dem,
                &mut provider,
                fallback,
            )?
        } else {
            let result = execute_with_provider(
                layers.expect("materialized layers were established"),
                &options,
                &dem,
                &mut provider,
                fallback,
            )?;
            let rendered = render_geojson(&result)?;
            write_atomic(&output, rendered.as_bytes())?;
            (
                result,
                rendered.len() as u64,
                format!("{:x}", Sha256::digest(rendered.as_bytes())),
            )
        }
    } else {
        let mut provider = CachedRangeProvider {
            url: runtime.cog_url.to_string(),
            total: runtime.cog_size_bytes,
            cache: runtime.cache_path.clone(),
            max_bytes: runtime.cache_max_bytes,
            max_entries: runtime.cache_max_entries,
            fetcher: HttpRangeFetcher::new(context.check_online),
        };
        if streamed_text {
            stream_text_elevation(
                &source,
                &output,
                &request,
                point_count,
                &dem,
                &mut provider,
                fallback,
            )?
        } else {
            let result = execute_with_provider(
                layers.expect("materialized layers were established"),
                &options,
                &dem,
                &mut provider,
                fallback,
            )?;
            let rendered = render_geojson(&result)?;
            write_atomic(&output, rendered.as_bytes())?;
            (
                result,
                rendered.len() as u64,
                format!("{:x}", Sha256::digest(rendered.as_bytes())),
            )
        }
    };

    Ok(NativeElevationOutcome::Completed {
        evidence: NativeElevationEvidence {
            task: TASK,
            execution_host: context.execution_host(),
            source_path: source.to_string_lossy().into_owned(),
            source_bytes,
            output_path: output.to_string_lossy().into_owned(),
            output_bytes,
            output_sha256,
            output_format: "geojson",
            point_count: result.point_count,
            interpolated_count: result.interpolated_count,
            rwanda_count: result.rwanda_count,
            fallback_count: result.fallback_count,
            unresolved_count: result.unresolved_count,
            fallback_required_count: result.fallback_required_count,
            rwanda_coverage: format!("{:?}", result.rwanda_coverage).to_ascii_lowercase(),
            rwanda_source: result.rwanda_source,
            fallback_source: result.fallback_source,
            dem_access: if runtime.mode == "offline" {
                "installed_full"
            } else {
                "direct_range"
            },
            direct_public_range_reads: runtime.mode != "offline",
            full_local_required_above_points: runtime.full_local_required_above_points,
            cloud_service_used: false,
            admin_enrichment: "required_next_via_reference_asset_enrich_admin_bounds",
            layers: result.stats,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use ds_elevation::{ElevationAdmission, RwandaBlockKey};

    #[test]
    fn desktop_admission_boundary_requires_full_component_at_4001() {
        assert_eq!(
            assess_admission(
                4_000,
                ElevationExecutionHost::Desktop,
                RwandaDemAccess::DirectRange,
            ),
            ElevationAdmission::Accepted
        );
        assert!(matches!(
            assess_admission(
                4_001,
                ElevationExecutionHost::Desktop,
                RwandaDemAccess::DirectRange,
            ),
            ElevationAdmission::InstalledDemRequired { .. }
        ));
        assert_eq!(
            assess_admission(
                4_001,
                ElevationExecutionHost::Desktop,
                RwandaDemAccess::InstalledFull,
            ),
            ElevationAdmission::Accepted
        );
    }

    #[test]
    fn exact_http_range_contract_refuses_full_or_wrong_response() {
        assert!(validate_range_response(206, Some("bytes 10-14/100"), 10, 5, 100).is_ok());
        assert!(validate_range_response(200, Some("bytes 10-14/100"), 10, 5, 100).is_err());
        assert!(validate_range_response(206, Some("bytes 10-15/100"), 10, 5, 100).is_err());
        assert!(validate_range_response(206, None, 10, 5, 100).is_err());
    }

    struct FakeFetcher {
        calls: usize,
        bytes: Vec<u8>,
    }

    impl RangeFetcher for FakeFetcher {
        fn fetch(
            &mut self,
            _url: &str,
            _offset: u64,
            _length: u64,
            _total: u64,
        ) -> Result<Vec<u8>, String> {
            self.calls += 1;
            Ok(self.bytes.clone())
        }
    }

    #[test]
    fn exact_compressed_ranges_are_persisted_and_reused_from_disk_cache() {
        let cache = std::env::temp_dir().join(format!("ds-dem-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&cache).expect("cache");
        let request = RwandaBlockRequest {
            key: RwandaBlockKey { i: 1, j: 2 },
            offset: 11,
            length: 4,
        };
        let mut provider = CachedRangeProvider {
            url: "https://example.invalid/cog".to_string(),
            total: 100,
            cache: cache.clone(),
            max_bytes: 100,
            max_entries: 4,
            fetcher: FakeFetcher {
                calls: 0,
                bytes: vec![1, 2, 3, 4],
            },
        };
        assert_eq!(
            provider.read_compressed_block(&request).unwrap(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(provider.fetcher.calls, 1);
        assert_eq!(
            provider.read_compressed_block(&request).unwrap(),
            vec![1, 2, 3, 4]
        );
        assert_eq!(provider.fetcher.calls, 1);
        std::fs::remove_dir_all(cache).expect("cleanup");
    }

    #[test]
    fn terrarium_rgb_formula_is_exact() {
        assert_eq!(decode_terrarium_rgb([128, 0, 0]), 0.0);
        assert_eq!(decode_terrarium_rgb([128, 1, 128]), 1.5);
        assert_eq!(decode_terrarium_rgb([127, 255, 0]), -1.0);
    }

    #[test]
    fn absolute_output_refuses_overwrite_and_non_geojson() {
        let root = std::env::temp_dir().join(format!("ds-elevation-out-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("root");
        let source = root.join("source.csv");
        std::fs::write(&source, b"x,y\n1,2\n").expect("source");
        assert!(absolute_new_output(root.join("result.zip").to_str().unwrap(), &source).is_err());
        let output = root.join("result.geojson");
        std::fs::write(&output, b"existing").expect("existing");
        assert!(absolute_new_output(output.to_str().unwrap(), &source).is_err());
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn streamed_text_lane_never_materializes_more_than_one_batch() {
        let root =
            std::env::temp_dir().join(format!("ds-elevation-stream-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("root");
        let source = root.join("points.csv");
        let mut file = File::create(&source).expect("source");
        writeln!(file, "x,y,group").expect("header");
        for index in 0..10_001 {
            writeln!(file, "{}, {},city", 500_000.0 + index as f64, 4_780_000.0).expect("row");
        }
        drop(file);
        let request = NativeElevationRequest {
            source_path: source.to_string_lossy().into_owned(),
            output_path: root.join("out.geojson").to_string_lossy().into_owned(),
            x_column: "x".to_string(),
            y_column: "y".to_string(),
            source_crs: NativeElevationSourceCrs::NixItrf2005,
            separator: None,
            common_column: Some("group".to_string()),
            use_terrarium_fallback: false,
        };
        let mut largest = 0_usize;
        let count = for_each_text_batch(&source, &request, |batch| {
            largest = largest.max(batch.len());
            Ok(())
        })
        .expect("stream");
        assert_eq!(count, 10_001);
        assert_eq!(largest, STREAM_BATCH_POINTS);
        std::fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn geojson_result_is_valid_for_the_admin_enrichment_input_contract() {
        let result = ElevationResult {
            layers: vec![ElevationLayer {
                name: "points".to_string(),
                features: vec![json!({
                    "type": "Feature",
                    "geometry": { "type": "Point", "coordinates": [30.0, -2.0] },
                    "properties": { "x": 500000.0, "y": 4780000.0, "z": 1500.0,
                        "z_source": "rwanda_dem_10m" },
                })],
            }],
            point_count: 1,
            interpolated_count: 1,
            rwanda_count: 1,
            fallback_count: 0,
            unresolved_count: 0,
            fallback_required_count: 0,
            rwanda_source: "rwanda_dem_10m".to_string(),
            rwanda_coverage: RwandaCoverage::Full,
            fallback_source: None,
            crs: "NIX/ITRF_2005".to_string(),
            stats: Vec::new(),
        };
        let text = render_geojson(&result).expect("render");
        let parsed = geojson_text_to_layer(GeoTextImportInput {
            name: "elevation.geojson".to_string(),
            text,
        })
        .expect("same GeoJSON parser used by the local admin-enrichment input lane");
        assert_eq!(parsed.layers[0].features.len(), 1);
        assert_eq!(parsed.layers[0].features[0]["properties"]["z"], 1500.0);
    }

    #[test]
    fn completed_evidence_omits_the_fallback_source_when_no_fallback_ran() {
        let evidence = |fallback_source: Option<&str>| NativeElevationEvidence {
            task: TASK,
            execution_host: context.execution_host(),
            source_path: "/work/points.geojson".to_string(),
            source_bytes: 10,
            output_path: "/work/points-z.geojson".to_string(),
            output_bytes: 20,
            output_sha256: "0".repeat(64),
            output_format: "geojson",
            point_count: 6,
            interpolated_count: 6,
            rwanda_count: 6,
            fallback_count: 0,
            unresolved_count: 0,
            fallback_required_count: 0,
            rwanda_coverage: "full".to_string(),
            rwanda_source: "rwanda_dem_10m".to_string(),
            fallback_source: fallback_source.map(str::to_string),
            dem_access: "installed_full",
            direct_public_range_reads: false,
            full_local_required_above_points: 4_000,
            cloud_service_used: false,
            admin_enrichment: "required_next_via_reference_asset_enrich_admin_bounds",
            layers: Vec::new(),
        };
        let none = serde_json::to_value(evidence(None)).expect("serialize");
        assert!(
            none.get("fallbackSource").is_none(),
            "no-fallback evidence must not carry a null: {none}"
        );
        let terrarium =
            serde_json::to_value(evidence(Some("aws_terrarium_30m"))).expect("serialize");
        assert_eq!(terrarium["fallbackSource"], "aws_terrarium_30m");
    }
}
