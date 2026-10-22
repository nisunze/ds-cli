//! Shared native application elevation point-cloud extraction.
//!
//! A DIFFERENT operation from `native_elevation_interpolate`, which attaches
//! elevation to a point file the operator already has. This one takes an AREA —
//! drawn on the map, or a local area file, or a bounding box — and produces the
//! points. Planning a grading exercise needs the second; the first cannot
//! substitute for it.
//!
//! Everything stays on this machine. Areas are read from local files, elevation
//! comes from the verified installed COG or exact public byte ranges, and the
//! result is written locally. No DS API, Cloud Run route, or project upload is
//! reachable from here.
//!
//! Generation is the shared `ds_elevation::point_cloud` kernel — the same code
//! the browser previews with and the same code the cloud lane runs — so a count
//! shown before Apply is the count Apply produces on every lane.
//!
//! Two commands, because they answer different questions at different costs:
//! `plan` counts and touches nothing; `extract` samples the DEM and writes
//! files. Above 4,000 points the plan reports that the full local component is
//! required BEFORE the extract spends a single range read discovering it.

use crate::RequestContext;
use ds_elevation::point_cloud::{
    generate_point_cloud, plan_point_cloud, AreaFeature, AreaPolygon, GeneratedPointCloud,
    PointCloudError, PointCloudPlan, RandomTarget, SamplingSpec,
};
use ds_elevation::point_cloud_nix::NixItrf2005;
use ds_elevation::{
    assess_admission, stamp_point_elevation, ElevationProvider, RwandaBlockCache,
    RwandaBlockProvider, RwandaDem, RwandaDemAccess,
};
use ds_geo::projection::wgs84_to_nix_itrf2005;
use ds_io::{
    geojson_text_to_layer, kml_text_to_layer, kmz_bytes_to_layer, shp_zip_bytes_to_layer,
    GeoLayerImportEnvelope, GeoTextImportInput,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::elevation::{
    absolute_new_output, write_atomic, CachedRangeProvider, HttpRangeFetcher, LocalCogProvider,
    TerrariumProvider,
};

const TASK: &str = "native_elevation_point_cloud";

/// Desktop's own extraction ceiling. It is not an admission limit — Desktop
/// supports large jobs — it is the bound on what one call will hold in memory
/// and write in a single artifact. Beyond it the honest answer is to split the
/// area rather than to consume the machine.
const MAX_DESKTOP_POINTS: usize = 2_000_000;

/// Samples per DEM pass. Memory stays bounded by this batch plus the block
/// cache's explicit capacity, no matter how many points were generated.
const SAMPLE_BATCH: usize = 4_096;

/// Bounds on the REQUEST, so a pathological boundary cannot consume the shell
/// inside containment tests before any point exists.
const MAX_AREA_FEATURES: usize = 2_000;
const MAX_AREA_POSITIONS: usize = 2_000_000;
const MAX_AREA_FILE_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativePointCloudSampling {
    mode: String,
    #[serde(default)]
    spacing_m: Option<f64>,
    #[serde(default)]
    seed: Option<u64>,
    #[serde(default)]
    count: Option<u64>,
    #[serde(default)]
    density_per_km2: Option<f64>,
}

impl NativePointCloudSampling {
    fn resolve(&self) -> Result<SamplingSpec, String> {
        match self.mode.trim() {
            "grid" => Ok(SamplingSpec::Grid {
                spacing_m: self
                    .spacing_m
                    .ok_or("Grid sampling requires an explicit spacing in metres.")?,
            }),
            "seeded_random" => {
                let seed = self.seed.ok_or_else(|| {
                    "Seeded-random sampling requires an explicit seed so the result is reproducible."
                        .to_string()
                })?;
                let target = match (self.count, self.density_per_km2) {
                    (Some(_), Some(_)) => {
                        return Err("Give either a point count or a density, not both.".to_string());
                    }
                    (Some(count), None) => RandomTarget::Count(count),
                    (None, Some(density)) => RandomTarget::DensityPerKm2(density),
                    (None, None) => {
                        return Err(
                            "Seeded-random sampling requires a point count or a density."
                                .to_string(),
                        );
                    }
                };
                Ok(SamplingSpec::SeededRandom { seed, target })
            }
            other => Err(format!(
                "Sampling mode must be grid or seeded_random, got '{other}'."
            )),
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativePointCloudRequest {
    /// Inline GeoJSON — how the map's drawn or picked area arrives.
    #[serde(default)]
    area_geojson: Option<String>,
    /// An absolute local area file: GeoJSON, KML/KMZ, or a zipped Shapefile.
    /// This is the map-independent path a CLI caller uses.
    #[serde(default)]
    area_path: Option<String>,
    /// `[west, south, east, north]`, also map-independent.
    #[serde(default)]
    bbox: Option<[f64; 4]>,
    sampling: NativePointCloudSampling,
    /// Absolute path for a new `.geojson`. The CSV sibling is written beside it.
    #[serde(default)]
    output_path: Option<String>,
    #[serde(default = "default_true")]
    use_terrarium_fallback: bool,
}

impl NativePointCloudRequest {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if let Some(path) = &self.area_path {
            crate::bounded_text(path, "areaPath", 4096)?;
        }
        if let Some(path) = &self.output_path {
            crate::bounded_text(path, "outputPath", 4096)?;
        }
        if self
            .area_geojson
            .as_ref()
            .is_some_and(|v| v.len() > MAX_AREA_FILE_BYTES as usize)
        {
            return Err("inline area exceeds the native area byte bound".into());
        }
        self.sampling.resolve()?;
        Ok(())
    }
}

fn default_true() -> bool {
    true
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePointCloudAreaStats {
    area_index: usize,
    area_id: String,
    part_count: usize,
    area_km2: f64,
    point_count: usize,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePointCloudPlanEvidence {
    task: &'static str,
    execution_host: &'static str,
    sampling: String,
    point_count: usize,
    area_count: usize,
    total_area_km2: f64,
    /// What the point count means for routing, decided here so the UI and the
    /// CLI reflect one answer instead of each re-deriving the boundary.
    cloud_admissible: bool,
    max_cloud_points: usize,
    full_local_dem_required: bool,
    dem_access: &'static str,
    cloud_service_used: bool,
    areas: Vec<NativePointCloudAreaStats>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePointCloudEvidence {
    task: &'static str,
    execution_host: &'static str,
    sampling: String,
    output_path: String,
    output_bytes: u64,
    output_sha256: String,
    csv_path: String,
    csv_bytes: u64,
    csv_sha256: String,
    output_formats: Vec<String>,
    point_count: usize,
    interpolated_count: usize,
    rwanda_count: usize,
    fallback_count: usize,
    unresolved_count: usize,
    rwanda_coverage: &'static str,
    rwanda_source: &'static str,
    fallback_source: Option<&'static str>,
    dem_access: &'static str,
    direct_public_range_reads: bool,
    full_local_required_above_points: u32,
    cloud_service_used: bool,
    renamed_attributes: BTreeMap<String, String>,
    areas: Vec<NativePointCloudAreaStats>,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativePointCloudRefusalCode {
    /// More points than public range reads may serve; install the component.
    FullLocalDemRequired,
    /// Legal, but beyond what one Desktop call will materialize.
    PointBudgetExceeded,
    /// So large the plan itself cannot be walked; widen the spacing.
    AreaTooLargeToPlan,
    /// The operator cancelled. Partial output has already been removed.
    Cancelled,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePointCloudRefusal {
    code: NativePointCloudRefusalCode,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    point_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    point_limit: Option<usize>,
    remedy: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum NativePointCloudOutcome {
    Completed {
        evidence: Box<NativePointCloudEvidence>,
    },
    Refused {
        refusal: NativePointCloudRefusal,
    },
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum NativePointCloudPlanOutcome {
    Planned {
        evidence: NativePointCloudPlanEvidence,
    },
    Refused {
        refusal: NativePointCloudRefusal,
    },
}

/// Coarse progress for a long extraction. Published on batch boundaries only —
/// per-point publishing would make the readout the expensive part.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePointCloudProgress {
    running: bool,
    stage: String,
    done: usize,
    total: usize,
    message: String,
}

impl Default for NativePointCloudProgress {
    fn default() -> Self {
        Self {
            running: false,
            stage: "idle".to_string(),
            done: 0,
            total: 0,
            message: "No elevation point-cloud extraction is running".to_string(),
        }
    }
}

/// Shared by the command handlers and the blocking worker.
///
/// Cloneable over `Arc`s on purpose: `State<'_, T>` cannot cross into
/// `spawn_blocking`, and a worker publishing into a throwaway copy would leave
/// the progress readout permanently idle and the cancel button inert.
#[derive(Clone, Default)]
pub struct NativePointCloudState {
    progress: Arc<Mutex<NativePointCloudProgress>>,
    active_request: Arc<Mutex<Option<RequestContext>>>,
    cancelled: Arc<Mutex<Option<Arc<AtomicBool>>>>,
}

struct ExtractionAdmission<'a>(&'a NativePointCloudState);
impl Drop for ExtractionAdmission<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.0.active_request.lock() {
            *active = None;
        }
    }
}

impl NativePointCloudState {
    fn admit(&self, context: &RequestContext) -> Result<ExtractionAdmission<'_>, String> {
        context.validate()?;
        let mut active = self
            .active_request
            .lock()
            .map_err(|_| "point-cloud admission state is unavailable")?;
        if active.is_some() {
            return Err("an elevation extraction is already running in this local host".into());
        }
        *active = Some(context.clone());
        Ok(ExtractionAdmission(self))
    }

    fn begin(&self, total: usize) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        if let Ok(mut current) = self.cancelled.lock() {
            // A previous run's flag must not survive to cancel this one.
            *current = Some(Arc::clone(&flag));
        }
        self.publish(true, "generating", 0, total, "Generating points");
        flag
    }

    fn publish(&self, running: bool, stage: &str, done: usize, total: usize, message: &str) {
        if let Ok(mut current) = self.progress.lock() {
            *current = NativePointCloudProgress {
                running,
                stage: stage.to_string(),
                done,
                total,
                message: message.to_string(),
            };
        }
    }

    fn finish(&self, message: &str, total: usize) {
        if let Ok(mut current) = self.cancelled.lock() {
            *current = None;
        }
        self.publish(false, "complete", total, total, message);
    }

    #[cfg(test)]
    fn snapshot(&self) -> NativePointCloudProgress {
        self.progress
            .lock()
            .map(|current| current.clone())
            .unwrap_or_default()
    }
}

fn refusal_from(error: PointCloudError) -> Result<NativePointCloudRefusal, String> {
    match error {
        PointCloudError::PointBudgetExceeded {
            point_count,
            ceiling,
        } => Ok(NativePointCloudRefusal {
            code: NativePointCloudRefusalCode::PointBudgetExceeded,
            message: format!(
                "This area and sampling choice generates {point_count} points, above the {ceiling} one extraction materializes"
            ),
            point_count: Some(point_count),
            point_limit: Some(ceiling),
            remedy:
                "Widen the spacing, lower the density, or split the area into smaller extractions"
                    .to_string(),
        }),
        PointCloudError::AreaTooLargeToPlan {
            estimated,
            candidate_limit,
        } => Ok(NativePointCloudRefusal {
            code: NativePointCloudRefusalCode::AreaTooLargeToPlan,
            message: format!(
                "This area and sampling choice would examine about {estimated} candidate positions, above the {candidate_limit} one plan may walk"
            ),
            point_count: None,
            point_limit: None,
            remedy:
                "Widen the spacing, lower the density, or split the area into smaller extractions"
                    .to_string(),
        }),
        // Everything else is the caller describing an impossible request, which
        // is an error rather than a routing outcome.
        other => Err(other.to_string()),
    }
}

fn absolute_regular_file(raw: &str) -> Result<(PathBuf, u64), String> {
    let path = PathBuf::from(raw);
    if !path.is_absolute() {
        return Err(format!("{raw} must be an absolute local path"));
    }
    let metadata = std::fs::symlink_metadata(&path)
        .map_err(|error| format!("inspect area source {}: {error}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("{} is not a regular file", path.display()));
    }
    if metadata.len() > MAX_AREA_FILE_BYTES {
        return Err(format!(
            "{} is {} bytes, above the {MAX_AREA_FILE_BYTES} an area source may hold",
            path.display(),
            metadata.len()
        ));
    }
    Ok((path, metadata.len()))
}

/// Read an area file through the same `ds-io` readers the import path uses. No
/// second parser, and no format this application cannot already open.
fn areas_from_file(path: &Path) -> Result<Vec<AreaFeature>, String> {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_default();
    let extension = path
        .extension()
        .map(|value| value.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|error| format!("read area source: {error}"))?;
    let envelope: GeoLayerImportEnvelope = match extension.as_str() {
        "geojson" | "json" => {
            let text = String::from_utf8(bytes)
                .map_err(|error| format!("area GeoJSON is not UTF-8: {error}"))?;
            geojson_text_to_layer(GeoTextImportInput {
                name: name.clone(),
                text,
            })
            .map_err(|error| format!("read area GeoJSON: {error}"))?
        }
        "kml" => {
            let text = String::from_utf8(bytes)
                .map_err(|error| format!("area KML is not UTF-8: {error}"))?;
            kml_text_to_layer(GeoTextImportInput {
                name: name.clone(),
                text,
            })
            .map_err(|error| format!("read area KML: {error}"))?
        }
        "kmz" => {
            kmz_bytes_to_layer(&name, &bytes).map_err(|error| format!("read area KMZ: {error}"))?
        }
        "zip" => shp_zip_bytes_to_layer(&name, &bytes)
            .map_err(|error| format!("read area Shapefile: {error}"))?,
        other => {
            return Err(format!(
                "'{other}' is not an area format this application reads; use GeoJSON, KML/KMZ, or a zipped Shapefile"
            ));
        }
    };
    let mut areas = Vec::new();
    for layer in envelope.layers {
        for feature in &layer.features {
            collect_geojson_areas(feature, &mut areas)?;
        }
    }
    Ok(areas)
}

fn collect_geojson_areas(value: &Value, areas: &mut Vec<AreaFeature>) -> Result<(), String> {
    match value.get("type").and_then(Value::as_str) {
        Some("FeatureCollection") => {
            let features = value
                .get("features")
                .and_then(Value::as_array)
                .ok_or_else(|| "FeatureCollection has no features array".to_string())?;
            for feature in features {
                collect_geojson_areas(feature, areas)?;
            }
            Ok(())
        }
        Some("Feature") => {
            let geometry = value
                .get("geometry")
                .ok_or_else(|| "area feature has no geometry".to_string())?;
            let properties = match value.get("properties") {
                None | Some(Value::Null) => Map::new(),
                Some(Value::Object(map)) => map.clone(),
                Some(_) => return Err("area feature properties must be an object".to_string()),
            };
            let id = value
                .get("id")
                .map(|id| {
                    id.as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| id.to_string())
                })
                .or_else(|| {
                    properties
                        .get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
            areas.push(AreaFeature {
                id,
                properties,
                parts: polygon_parts(geometry)?,
            });
            Ok(())
        }
        Some(_) => {
            areas.push(AreaFeature {
                id: String::new(),
                properties: Map::new(),
                parts: polygon_parts(value)?,
            });
            Ok(())
        }
        None => Err("area must be GeoJSON with a type".to_string()),
    }
}

fn polygon_parts(geometry: &Value) -> Result<Vec<AreaPolygon>, String> {
    let kind = geometry.get("type").and_then(Value::as_str).unwrap_or("");
    let coordinates = geometry
        .get("coordinates")
        .ok_or_else(|| "area geometry has no coordinates".to_string())?;
    match kind {
        "Polygon" => Ok(vec![AreaPolygon {
            rings: rings(coordinates)?,
        }]),
        "MultiPolygon" => coordinates
            .as_array()
            .ok_or_else(|| "MultiPolygon coordinates must be an array".to_string())?
            .iter()
            .map(|polygon| {
                Ok(AreaPolygon {
                    rings: rings(polygon)?,
                })
            })
            .collect(),
        other => Err(format!(
            "An elevation point cloud needs an area: '{other}' has none"
        )),
    }
}

fn rings(value: &Value) -> Result<Vec<Vec<[f64; 2]>>, String> {
    value
        .as_array()
        .ok_or_else(|| "polygon coordinates must be an array".to_string())?
        .iter()
        .map(|ring| {
            ring.as_array()
                .ok_or_else(|| "polygon ring must be an array".to_string())?
                .iter()
                .map(|position| {
                    let values = position
                        .as_array()
                        .ok_or_else(|| "ring position must be an array".to_string())?;
                    let point = [
                        values.first().and_then(Value::as_f64).unwrap_or(f64::NAN),
                        values.get(1).and_then(Value::as_f64).unwrap_or(f64::NAN),
                    ];
                    if !point.iter().all(|value| value.is_finite()) {
                        return Err("ring position must be two finite numbers".to_string());
                    }
                    Ok(point)
                })
                .collect::<Result<Vec<_>, _>>()
        })
        .collect()
}

fn bbox_area(bbox: [f64; 4]) -> Result<AreaFeature, String> {
    let [west, south, east, north] = bbox;
    if !bbox.iter().all(|value| value.is_finite()) {
        return Err("bbox values must be finite".to_string());
    }
    if west >= east || south >= north {
        return Err(
            "bbox must be [west, south, east, north] with west < east and south < north"
                .to_string(),
        );
    }
    let mut properties = Map::new();
    properties.insert("ds_area_source".into(), json!("bbox"));
    Ok(AreaFeature {
        id: "bbox".into(),
        properties,
        parts: vec![AreaPolygon {
            rings: vec![vec![
                [west, south],
                [east, south],
                [east, north],
                [west, north],
            ]],
        }],
    })
}

pub(crate) fn collect_areas(request: &NativePointCloudRequest) -> Result<Vec<AreaFeature>, String> {
    let mut areas = Vec::new();
    if let Some(bbox) = request.bbox {
        areas.push(bbox_area(bbox)?);
    }
    if let Some(text) = request
        .area_geojson
        .as_deref()
        .map(str::trim)
        .filter(|text| !text.is_empty())
    {
        let value: Value = serde_json::from_str(text)
            .map_err(|error| format!("inline area is not valid GeoJSON: {error}"))?;
        collect_geojson_areas(&value, &mut areas)?;
    }
    if let Some(raw) = request
        .area_path
        .as_deref()
        .map(str::trim)
        .filter(|raw| !raw.is_empty())
    {
        let (path, _) = absolute_regular_file(raw)?;
        areas.extend(areas_from_file(&path)?);
    }
    if areas.is_empty() {
        return Err(
            "Supply an area: draw one on the map, choose an area file, or give a bbox.".to_string(),
        );
    }
    if areas.len() > MAX_AREA_FEATURES {
        return Err(format!(
            "At most {MAX_AREA_FEATURES} area features are accepted; this request has {}",
            areas.len()
        ));
    }
    let positions = areas
        .iter()
        .flat_map(|area| area.parts.iter())
        .flat_map(|part| part.rings.iter())
        .map(Vec::len)
        .sum::<usize>();
    if positions > MAX_AREA_POSITIONS {
        return Err(format!(
            "Area boundaries carry {positions} positions, above the {MAX_AREA_POSITIONS} accepted"
        ));
    }
    Ok(areas)
}

fn area_stats(plan: &[ds_elevation::AreaPlanRow]) -> Vec<NativePointCloudAreaStats> {
    plan.iter()
        .map(|row| NativePointCloudAreaStats {
            area_index: row.area_index,
            area_id: row.area_id.clone(),
            part_count: row.part_count,
            area_km2: row.area_km2,
            point_count: row.point_count,
        })
        .collect()
}

pub fn plan(
    context: &RequestContext,
    request: NativePointCloudRequest,
) -> Result<NativePointCloudPlanOutcome, String> {
    context.validate()?;
    request.validate()?;
    let areas = collect_areas(&request)?;
    let sampling = request.sampling.resolve()?;
    let plan: PointCloudPlan =
        match plan_point_cloud(&areas, &sampling, MAX_DESKTOP_POINTS, &NixItrf2005) {
            Ok(plan) => plan,
            Err(error) => {
                let refusal = refusal_from(error)?;
                return Ok(NativePointCloudPlanOutcome::Refused { refusal });
            }
        };

    // Routing is decided here, from the exact count, before any DEM byte.
    let installed_full = crate::rwanda_dem::installed_full_available();
    let access = if installed_full {
        RwandaDemAccess::InstalledFull
    } else {
        RwandaDemAccess::DirectRange
    };
    let full_local_dem_required = !matches!(
        assess_admission(
            plan.point_count,
            ds_elevation::ElevationExecutionHost::Desktop,
            access
        ),
        ds_elevation::ElevationAdmission::Accepted
    );
    let evidence = NativePointCloudPlanEvidence {
        task: TASK,
        execution_host: context.execution_host(),
        sampling: plan.sampling.clone(),
        point_count: plan.point_count,
        area_count: plan.area_count,
        total_area_km2: plan.total_area_km2,
        cloud_admissible: plan.point_count <= ds_elevation::CLOUD_POINT_LIMIT,
        max_cloud_points: ds_elevation::CLOUD_POINT_LIMIT,
        full_local_dem_required,
        dem_access: if installed_full {
            "installed_full"
        } else {
            "direct_range"
        },
        cloud_service_used: false,
        areas: area_stats(&plan.areas),
    };
    Ok(NativePointCloudPlanOutcome::Planned { evidence })
}

/// Sample the DEM in bounded batches, honouring cancellation between them.
///
/// Rwanda first, then whole-AREA fallback: an area partly outside the national
/// DEM moves to Terrarium entirely rather than producing a grading surface with
/// a seam through the middle of it. That is the interpolation lane's
/// all-or-nothing rule with the drawn area as the group.
#[allow(clippy::too_many_arguments)]
fn sample_and_stamp<P: RwandaBlockProvider>(
    state: &NativePointCloudState,
    cancelled: &AtomicBool,
    cloud: &mut GeneratedPointCloud,
    dem: &RwandaDem,
    provider: &mut P,
    mut fallback: Option<&mut dyn ElevationProvider>,
) -> Result<(usize, usize, usize, &'static str), String> {
    let total = cloud.features.len();
    let wgs84 = cloud
        .features
        .iter()
        .map(|feature| {
            let coordinates = &feature["geometry"]["coordinates"];
            [
                coordinates[0].as_f64().unwrap_or(f64::NAN),
                coordinates[1].as_f64().unwrap_or(f64::NAN),
            ]
        })
        .collect::<Vec<_>>();
    let nix = wgs84
        .iter()
        .map(|point| {
            let (x, y) = wgs84_to_nix_itrf2005(point[0], point[1]);
            [x, y]
        })
        .collect::<Vec<_>>();

    let mut cache = RwandaBlockCache::new(ds_elevation::DEFAULT_DECODED_BLOCK_CACHE)
        .map_err(|error| error.to_string())?;
    let mut rwanda: Vec<Option<f64>> = Vec::with_capacity(total);
    for (index, batch) in nix.chunks(SAMPLE_BATCH).enumerate() {
        if cancelled.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        let done = index * SAMPLE_BATCH;
        state.publish(true, "sampling", done, total, "Reading Rwanda elevation");
        rwanda.extend(
            dem.sample_with_cache(provider, &mut cache, batch)
                .map_err(|error| format!("read Rwanda DEM: {error}"))?,
        );
    }

    let coverage = match ds_elevation::RwandaCoverage::classify(&rwanda) {
        ds_elevation::RwandaCoverage::Full => "full",
        ds_elevation::RwandaCoverage::Partial => "partial",
        ds_elevation::RwandaCoverage::None => "none",
    };

    let mut area_complete = BTreeMap::<String, bool>::new();
    for (index, feature) in cloud.features.iter().enumerate() {
        let key = feature["properties"]["ds_area_id"].to_string();
        let covered = rwanda[index].is_some();
        area_complete
            .entry(key)
            .and_modify(|all| *all &= covered)
            .or_insert(covered);
    }
    let use_rwanda = cloud
        .features
        .iter()
        .map(|feature| {
            area_complete
                .get(&feature["properties"]["ds_area_id"].to_string())
                .copied()
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();

    let fallback_points = wgs84
        .iter()
        .zip(&use_rwanda)
        .filter_map(|(point, rwanda)| (!rwanda).then_some(*point))
        .collect::<Vec<_>>();
    let mut fallback_samples = Vec::with_capacity(fallback_points.len());
    if let Some(provider) = fallback.as_mut() {
        for (index, batch) in fallback_points.chunks(SAMPLE_BATCH).enumerate() {
            if cancelled.load(Ordering::Relaxed) {
                return Err("cancelled".to_string());
            }
            state.publish(
                true,
                "sampling_fallback",
                index * SAMPLE_BATCH,
                fallback_points.len(),
                "Reading fallback elevation",
            );
            fallback_samples.extend(
                provider
                    .sample_wgs84(batch)
                    .map_err(|error| format!("read fallback elevation: {error}"))?,
            );
        }
    } else {
        fallback_samples.resize(fallback_points.len(), None);
    }
    let fallback_source = fallback
        .as_ref()
        .map(|provider| provider.source_id().to_string());

    let mut interpolated = 0usize;
    let mut rwanda_count = 0usize;
    let mut fallback_count = 0usize;
    let mut fallback_cursor = 0usize;
    for (index, feature) in cloud.features.iter_mut().enumerate() {
        let (sample, source) = if use_rwanda[index] {
            rwanda_count += 1;
            (rwanda[index], rwanda[index].map(|_| "rwanda_dem_10m"))
        } else {
            let sample = fallback_samples
                .get(fallback_cursor)
                .copied()
                .flatten()
                .filter(|value| value.is_finite());
            fallback_cursor += 1;
            fallback_count += 1;
            (sample, sample.and(fallback_source.as_deref()))
        };
        if sample.is_some() {
            interpolated += 1;
        }
        stamp_point_elevation(feature, nix[index], sample, source)
            .map_err(|error| format!("stamp elevation: {error}"))?;
    }
    Ok((interpolated, rwanda_count, fallback_count, coverage))
}

/// Flatten the cloud to CSV. Field order is the sorted union of every point's
/// property names, so the header is stable across runs.
fn cloud_csv(features: &[Value]) -> String {
    let fields = features
        .iter()
        .filter_map(|feature| feature["properties"].as_object())
        .flat_map(|properties| properties.keys().cloned())
        .collect::<BTreeSet<_>>();
    let escape = |value: &str| {
        if value.contains([',', '"', '\n', '\r']) {
            format!("\"{}\"", value.replace('"', "\"\""))
        } else {
            value.to_string()
        }
    };
    let cell = |value: Option<&Value>| match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(text)) => escape(text),
        Some(other) => escape(&other.to_string()),
    };
    let mut out = String::new();
    out.push_str(
        &fields
            .iter()
            .map(|field| escape(field))
            .collect::<Vec<_>>()
            .join(","),
    );
    out.push('\n');
    for feature in features {
        let properties = feature["properties"].as_object();
        out.push_str(
            &fields
                .iter()
                .map(|field| cell(properties.and_then(|map| map.get(field))))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
    }
    out
}

fn csv_sibling(geojson: &Path) -> PathBuf {
    geojson.with_extension("csv")
}

/// Shared extraction application operation, including resource acquisition and
/// the exact full-local retry used by every native caller.
pub fn extract(
    context: &RequestContext,
    state: &NativePointCloudState,
    request: NativePointCloudRequest,
) -> Result<NativePointCloudOutcome, String> {
    let _admission = state.admit(context)?;
    request.validate()?;
    crate::rwanda_dem::retry_full_local(
        || extract_once(context, state, request.clone()),
        |outcome| {
            matches!(
                outcome,
                NativePointCloudOutcome::Refused {
                    refusal: NativePointCloudRefusal {
                        code: NativePointCloudRefusalCode::FullLocalDemRequired,
                        ..
                    }
                }
            )
        },
        || crate::rwanda_dem::ensure_available(context, true),
    )
}

fn extract_once(
    context: &RequestContext,
    state: &NativePointCloudState,
    request: NativePointCloudRequest,
) -> Result<NativePointCloudOutcome, String> {
    context.validate()?;
    request.validate()?;
    let areas = collect_areas(&request)?;
    let sampling = request.sampling.resolve()?;
    let output_raw = request
        .output_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| "Native extraction requires an explicit output path.".to_string())?;

    // Plan first: the count decides routing, and routing is decided before the
    // first DEM range read rather than discovered part way through one.
    let plan = match plan_point_cloud(&areas, &sampling, MAX_DESKTOP_POINTS, &NixItrf2005) {
        Ok(plan) => plan,
        Err(error) => {
            return Ok(NativePointCloudOutcome::Refused {
                refusal: refusal_from(error)?,
            });
        }
    };

    crate::rwanda_dem::ensure_available(context, false)?;
    let runtime = crate::rwanda_dem::verified_rwanda_dem_runtime()?;
    let access = if runtime.mode == "offline" {
        RwandaDemAccess::InstalledFull
    } else {
        RwandaDemAccess::DirectRange
    };
    if !matches!(
        assess_admission(
            plan.point_count,
            ds_elevation::ElevationExecutionHost::Desktop,
            access
        ),
        ds_elevation::ElevationAdmission::Accepted
    ) {
        return Ok(NativePointCloudOutcome::Refused {
            refusal: NativePointCloudRefusal {
                code: NativePointCloudRefusalCode::FullLocalDemRequired,
                message: format!(
                    "{} points exceed what public Rwanda DEM range reads serve",
                    plan.point_count
                ),
                point_count: Some(plan.point_count),
                point_limit: Some(runtime.full_local_required_above_points as usize),
                remedy:
                    "Install and verify the full Rwanda DEM Desktop component, then retry unchanged"
                        .to_string(),
            },
        });
    }

    // Guarded against overwriting the area file it was derived from, and
    // against a CSV sibling that already exists — a half-refused write would
    // leave one honest file beside one stale one.
    let area_source = request
        .area_path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_default();
    let output = absolute_new_output(output_raw, &area_source)?;
    let csv_path = csv_sibling(&output);
    if csv_path.exists() {
        return Err(format!(
            "{} already exists; overwrite is refused",
            csv_path.display()
        ));
    }
    let cancelled = state.begin(plan.point_count);

    let outcome = (|| -> Result<NativePointCloudOutcome, String> {
        let mut cloud = generate_point_cloud(&areas, &sampling, MAX_DESKTOP_POINTS, &NixItrf2005)
            .map_err(|error| error.to_string())?;
        if cancelled.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        state.publish(
            true,
            "sampling",
            0,
            cloud.features.len(),
            "Reading Rwanda elevation",
        );

        let index = std::fs::read(&runtime.index_path)
            .map_err(|error| format!("read verified Rwanda DEM block index: {error}"))?;
        let dem = RwandaDem::from_index_json(&index)
            .map_err(|error| format!("parse verified Rwanda DEM block index: {error}"))?;
        let mut terrarium = request
            .use_terrarium_fallback
            .then(|| TerrariumProvider::new(runtime.terrarium_url_template, context.check_online));
        let fallback = terrarium
            .as_mut()
            .map(|provider| provider as &mut dyn ElevationProvider);

        let stats = if let Some(cog_path) = runtime.cog_path.as_ref() {
            let mut provider = LocalCogProvider {
                file: File::open(cog_path)
                    .map_err(|error| format!("open installed Rwanda DEM COG: {error}"))?,
            };
            sample_and_stamp(state, &cancelled, &mut cloud, &dem, &mut provider, fallback)?
        } else {
            let mut provider = CachedRangeProvider {
                url: runtime.cog_url.to_string(),
                total: runtime.cog_size_bytes,
                cache: runtime.cache_path.clone(),
                max_bytes: runtime.cache_max_bytes,
                max_entries: runtime.cache_max_entries,
                fetcher: HttpRangeFetcher::new(context.check_online),
            };
            sample_and_stamp(state, &cancelled, &mut cloud, &dem, &mut provider, fallback)?
        };
        let (interpolated, rwanda_count, fallback_count, coverage) = stats;

        if cancelled.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        state.publish(
            true,
            "writing",
            cloud.features.len(),
            cloud.features.len(),
            "Writing result",
        );

        let geojson = serde_json::to_string(&json!({
            "type": "FeatureCollection",
            "features": cloud.features,
        }))
        .map_err(|error| format!("render elevation point cloud: {error}"))?;
        let csv = cloud_csv(&cloud.features);
        write_atomic(&output, geojson.as_bytes())?;
        // The CSV is the second half of one result. If it fails, the GeoJSON
        // half must not be left behind claiming a complete extraction.
        if let Err(error) = write_atomic(&csv_path, csv.as_bytes()) {
            let _ = std::fs::remove_file(&output);
            return Err(error);
        }

        Ok(NativePointCloudOutcome::Completed {
            evidence: Box::new(NativePointCloudEvidence {
                task: TASK,
                execution_host: context.execution_host(),
                sampling: cloud.sampling.clone(),
                output_path: output.to_string_lossy().into_owned(),
                output_bytes: geojson.len() as u64,
                output_sha256: format!("{:x}", Sha256::digest(geojson.as_bytes())),
                csv_path: csv_path.to_string_lossy().into_owned(),
                csv_bytes: csv.len() as u64,
                csv_sha256: format!("{:x}", Sha256::digest(csv.as_bytes())),
                output_formats: vec!["geojson".to_string(), "csv".to_string()],
                point_count: cloud.features.len(),
                interpolated_count: interpolated,
                rwanda_count,
                fallback_count,
                unresolved_count: cloud.features.len() - interpolated,
                rwanda_coverage: coverage,
                rwanda_source: "rwanda_dem_10m",
                fallback_source: request
                    .use_terrarium_fallback
                    .then_some("aws_terrarium_30m"),
                dem_access: if runtime.mode == "offline" {
                    "installed_full"
                } else {
                    "direct_range"
                },
                direct_public_range_reads: runtime.mode != "offline",
                full_local_required_above_points: runtime.full_local_required_above_points,
                cloud_service_used: false,
                renamed_attributes: cloud.renamed_attributes.clone(),
                areas: area_stats(&cloud.areas),
            }),
        })
    })();

    match outcome {
        Ok(outcome) => {
            state.finish("Extraction complete", plan.point_count);
            Ok(outcome)
        }
        Err(error) if error == "cancelled" => {
            // Cancellation leaves nothing half-written: the atomic writer never
            // published, and any file it did publish is removed here.
            let _ = std::fs::remove_file(&output);
            let _ = std::fs::remove_file(&csv_path);
            state.finish("Extraction cancelled", 0);
            Ok(NativePointCloudOutcome::Refused {
                refusal: NativePointCloudRefusal {
                    code: NativePointCloudRefusalCode::Cancelled,
                    message: "The extraction was cancelled".to_string(),
                    point_count: Some(plan.point_count),
                    point_limit: None,
                    remedy: "Run it again when ready; nothing was written".to_string(),
                },
            })
        }
        Err(error) => {
            let _ = std::fs::remove_file(&output);
            let _ = std::fs::remove_file(&csv_path);
            state.finish("Extraction failed", 0);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(sampling: NativePointCloudSampling) -> NativePointCloudRequest {
        NativePointCloudRequest {
            area_geojson: None,
            area_path: None,
            bbox: Some([30.05, -1.95, 30.06, -1.94]),
            sampling,
            output_path: None,
            use_terrarium_fallback: true,
        }
    }

    fn grid(spacing_m: f64) -> NativePointCloudSampling {
        NativePointCloudSampling {
            mode: "grid".into(),
            spacing_m: Some(spacing_m),
            seed: None,
            count: None,
            density_per_km2: None,
        }
    }

    fn seeded(count: u64) -> NativePointCloudSampling {
        NativePointCloudSampling {
            mode: "seeded_random".into(),
            spacing_m: None,
            seed: Some(2_026),
            count: Some(count),
            density_per_km2: None,
        }
    }

    #[test]
    fn a_bbox_needs_no_map_and_no_file() {
        let areas = collect_areas(&request(grid(100.0))).expect("bbox is an area");
        assert_eq!(areas.len(), 1);
        assert_eq!(areas[0].id, "bbox");
        let plan = plan_point_cloud(&areas, &grid(100.0).resolve().unwrap(), 4_000, &NixItrf2005)
            .expect("plan");
        assert!(plan.point_count > 100);
    }

    #[test]
    fn desktop_supports_far_more_than_the_cloud_ceiling() {
        // The point of the Desktop lane: 4,001 is refused in the browser and
        // ordinary here.
        let areas = collect_areas(&request(seeded(50_000))).unwrap();
        let plan = plan_point_cloud(
            &areas,
            &seeded(50_000).resolve().unwrap(),
            MAX_DESKTOP_POINTS,
            &NixItrf2005,
        )
        .expect("Desktop admits large jobs");
        assert_eq!(plan.point_count, 50_000);
        assert!(plan.point_count > ds_elevation::CLOUD_POINT_LIMIT);
    }

    #[test]
    fn above_four_thousand_requires_the_installed_component() {
        // Proven against the shared admission kernel, so the Desktop lane and
        // the component manager cannot disagree about where the line is.
        assert!(matches!(
            assess_admission(
                4_000,
                ds_elevation::ElevationExecutionHost::Desktop,
                RwandaDemAccess::DirectRange
            ),
            ds_elevation::ElevationAdmission::Accepted
        ));
        assert!(matches!(
            assess_admission(
                4_001,
                ds_elevation::ElevationExecutionHost::Desktop,
                RwandaDemAccess::DirectRange
            ),
            ds_elevation::ElevationAdmission::InstalledDemRequired { .. }
        ));
        assert!(matches!(
            assess_admission(
                4_001,
                ds_elevation::ElevationExecutionHost::Desktop,
                RwandaDemAccess::InstalledFull
            ),
            ds_elevation::ElevationAdmission::Accepted
        ));
    }

    #[test]
    fn an_inline_area_keeps_its_attributes_per_area() {
        let mut inline = request(seeded(10));
        inline.bbox = None;
        inline.area_geojson = Some(
            json!({
                "type": "FeatureCollection",
                "features": [
                    {
                        "type": "Feature",
                        "id": "north",
                        "properties": { "sector": "Kacyiru", "phase": 1 },
                        "geometry": { "type": "Polygon", "coordinates": [[
                            [30.05, -1.95], [30.06, -1.95], [30.06, -1.94], [30.05, -1.94]
                        ]] }
                    },
                    {
                        "type": "Feature",
                        "id": "south",
                        "properties": { "sector": "Kicukiro", "owner": "EDCL" },
                        "geometry": { "type": "Polygon", "coordinates": [[
                            [30.08, -1.99], [30.09, -1.99], [30.09, -1.98], [30.08, -1.98]
                        ]] }
                    }
                ]
            })
            .to_string(),
        );
        let areas = collect_areas(&inline).expect("inline areas");
        assert_eq!(areas.len(), 2);
        let cloud = generate_point_cloud(
            &areas,
            &seeded(10).resolve().unwrap(),
            MAX_DESKTOP_POINTS,
            &NixItrf2005,
        )
        .expect("generate");
        assert_eq!(cloud.features.len(), 20);
        for feature in &cloud.features {
            let properties = feature["properties"].as_object().unwrap();
            match properties["ds_area_id"].as_str().unwrap() {
                "north" => {
                    assert_eq!(properties["sector"], json!("Kacyiru"));
                    assert_eq!(properties["phase"], json!(1));
                    assert!(!properties.contains_key("owner"));
                }
                "south" => {
                    assert_eq!(properties["sector"], json!("Kicukiro"));
                    assert_eq!(properties["owner"], json!("EDCL"));
                    assert!(!properties.contains_key("phase"));
                }
                other => panic!("unexpected area {other}"),
            }
        }
    }

    #[test]
    fn an_under_specified_sampling_choice_is_refused_rather_than_guessed() {
        for sampling in [
            NativePointCloudSampling {
                mode: "grid".into(),
                spacing_m: None,
                seed: None,
                count: None,
                density_per_km2: None,
            },
            NativePointCloudSampling {
                mode: "seeded_random".into(),
                spacing_m: None,
                seed: None,
                count: Some(5),
                density_per_km2: None,
            },
            NativePointCloudSampling {
                mode: "seeded_random".into(),
                spacing_m: None,
                seed: Some(1),
                count: Some(5),
                density_per_km2: Some(5.0),
            },
            NativePointCloudSampling {
                mode: "voronoi".into(),
                spacing_m: None,
                seed: None,
                count: None,
                density_per_km2: None,
            },
        ] {
            assert!(sampling.resolve().is_err());
        }
    }

    #[test]
    fn a_non_area_geometry_and_a_missing_area_are_both_refused() {
        let mut lines = request(grid(50.0));
        lines.bbox = None;
        lines.area_geojson = Some(
            json!({ "type": "LineString", "coordinates": [[30.05, -1.95], [30.06, -1.94]] })
                .to_string(),
        );
        assert!(collect_areas(&lines).is_err());

        let mut empty = request(grid(50.0));
        empty.bbox = None;
        assert!(collect_areas(&empty).is_err());
    }

    #[test]
    fn the_csv_header_is_the_stable_union_of_every_property() {
        let features = vec![
            json!({
                "properties": { "ds_point_id": "00000001", "z": 1500.5, "name": "a,b" }
            }),
            json!({ "properties": { "ds_point_id": "00000002", "sector": "Kacyiru" } }),
        ];
        let csv = cloud_csv(&features);
        let mut lines = csv.lines();
        assert_eq!(lines.next().unwrap(), "ds_point_id,name,sector,z");
        // A value containing a comma is quoted, not silently split into two
        // columns that would shift every field after it.
        assert_eq!(lines.next().unwrap(), "00000001,\"a,b\",,1500.5");
        assert_eq!(lines.next().unwrap(), "00000002,,Kacyiru,");
    }

    #[test]
    fn the_csv_is_written_beside_the_geojson() {
        assert_eq!(
            csv_sibling(Path::new("/data/kigali-elevation.geojson")),
            PathBuf::from("/data/kigali-elevation.csv")
        );
    }

    #[test]
    fn refusals_carry_a_remedy_and_never_hide_a_real_error() {
        let over = refusal_from(PointCloudError::PointBudgetExceeded {
            point_count: 3_000_000,
            ceiling: MAX_DESKTOP_POINTS,
        })
        .expect("a budget overrun is a routing outcome");
        assert!(matches!(
            over.code,
            NativePointCloudRefusalCode::PointBudgetExceeded
        ));
        assert_eq!(over.point_count, Some(3_000_000));
        assert!(!over.remedy.is_empty());

        // A malformed request is an error, not a polite refusal that would let
        // a caller believe the area was simply too big.
        assert!(refusal_from(PointCloudError::NoArea).is_err());
        assert!(refusal_from(PointCloudError::InvalidSpacing { got: 0.0 }).is_err());
    }

    #[test]
    fn progress_starts_idle_and_reports_what_it_is_doing() {
        let state = NativePointCloudState::default();
        assert!(!state.snapshot().running);
        state.begin(1_000);
        let running = state.snapshot();
        assert!(running.running);
        assert_eq!(running.total, 1_000);
        state.publish(true, "sampling", 512, 1_000, "Reading Rwanda elevation");
        assert_eq!(state.snapshot().done, 512);
        state.finish("Extraction complete", 1_000);
        let done = state.snapshot();
        assert!(!done.running);
        assert_eq!(done.stage, "complete");
    }

    #[test]
    fn heap_owned_completed_evidence_keeps_the_exact_receiver_wire() {
        let evidence = NativePointCloudEvidence {
            task: TASK,
            execution_host: context.execution_host(),
            sampling: "grid".into(),
            output_path: "/data/α.geojson".into(),
            output_bytes: 4096,
            output_sha256: "a".repeat(64),
            csv_path: "/data/α.csv".into(),
            csv_bytes: 2048,
            csv_sha256: "b".repeat(64),
            output_formats: vec!["geojson".into(), "csv".into()],
            point_count: 480,
            interpolated_count: 478,
            rwanda_count: 480,
            fallback_count: 0,
            unresolved_count: 2,
            rwanda_coverage: "partial",
            rwanda_source: "rwanda_dem_10m",
            fallback_source: Some("aws_terrarium_30m"),
            dem_access: "direct_range",
            direct_public_range_reads: true,
            full_local_required_above_points: 4000,
            cloud_service_used: false,
            renamed_attributes: BTreeMap::new(),
            areas: vec![NativePointCloudAreaStats {
                area_index: 0,
                area_id: "bbox".into(),
                part_count: 1,
                area_km2: 1.2,
                point_count: 480,
            }],
        };
        let expected = json!({"status":"completed", "evidence":&evidence});
        let outcome = NativePointCloudOutcome::Completed {
            evidence: Box::new(evidence),
        };
        assert_eq!(
            serde_json::to_vec(&outcome).unwrap(),
            serde_json::to_vec(&expected).unwrap()
        );
        let actual = serde_json::to_value(outcome).unwrap();
        assert_eq!(actual["status"], "completed");
        assert_eq!(actual["evidence"]["outputBytes"], 4096);
        assert_eq!(actual["evidence"]["cloudServiceUsed"], false);
        assert_eq!(actual["evidence"]["areas"][0]["pointCount"], 480);
        assert!(actual["evidence"].get("output_bytes").is_none());
    }
    #[test]
    fn a_running_extraction_retains_its_scope_instead_of_being_replaced() {
        let state = NativePointCloudState::default();
        let a = RequestContext::local_files(crate::ExecutionHost::Headless, crate::online);
        let b = RequestContext::local_files(crate::ExecutionHost::Desktop, crate::online);
        let admission = state.admit(&a).unwrap();
        assert!(state.admit(&b).is_err());
        assert_eq!(
            state
                .active_request
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .request_id(),
            a.request_id()
        );
        drop(admission);
        assert!(state.admit(&b).is_ok());
    }
}
