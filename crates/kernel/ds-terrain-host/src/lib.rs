//! Native file orchestration for single-source, reproducible terrain sampling.
//! Geometry and sampling decisions belong to their portable owners. This host
//! admits paths and bytes, records provenance, and writes new local artifacts.

mod input;
mod output;
mod surface;

use std::path::{Path, PathBuf};

use ds_terrain_sampling::{SamplingError, SamplingOptions, SamplingResult};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;

pub const MAX_INPUT_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_OUTPUT_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_RECEIPT_BYTES: usize = 8 * 1024;
pub const MAX_METADATA_PATH_BYTES: usize = 512;
pub const DERIVED_ROW_CLASS: &str = "derived_interpolated";

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SamplingFileRequest {
    /// RFC 7946 WGS84 longitude/latitude GeoJSON. Relative paths are resolved
    /// from the request file's directory.
    pub route_file: PathBuf,
    /// Required characterized metric model frame, never EPSG:4326.
    pub projected_crs: String,
    pub source: SamplingSource,
    #[serde(default)]
    pub settings: SamplingOptions,
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum SamplingSource {
    RwandaTiff {},
    SurveyTin {
        csv_file: PathBuf,
        source_crs: String,
        x_column: String,
        y_column: String,
        z_column: String,
        max_triangle_edge_m: f64,
        #[serde(default)]
        breaklines: Vec<DeclaredBreakline>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeclaredBreakline {
    pub vertices: Vec<DeclaredTerrainPoint>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeclaredTerrainPoint {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

#[derive(Debug, Error)]
pub enum SamplingFileError {
    #[error("invalid terrain request: {0}")]
    RequestInvalid(String),
    #[error("terrain resource limit exceeded: {0}")]
    InputTooLarge(String),
    #[error("terrain output already exists: {0}")]
    OutputExists(String),
    #[error("terrain file I/O failed: {0}")]
    Io(String),
    #[error("invalid declared terrain source: {0}")]
    SourceInvalid(String),
    #[error("terrain surface failed: {0}")]
    SurfaceFailed(String),
    #[error("terrain sampling failed ({code}): {detail}")]
    SamplingFailed { code: &'static str, detail: String },
    #[error("authored terrain breaklines require constrained triangulation; {count} supplied")]
    BreaklinesUnsupported { count: usize },
    #[error("terrain provider acquisition failed: {0}")]
    ProviderFailed(String),
    #[error("terrain provider integrity failed: {0}")]
    IntegrityFailed(String),
}

impl SamplingFileError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::RequestInvalid(_) => "terrain_request_invalid",
            Self::InputTooLarge(_) => "terrain_input_too_large",
            Self::OutputExists(_) => "terrain_output_exists",
            Self::Io(_) => "terrain_io",
            Self::SourceInvalid(_) => "terrain_source_invalid",
            Self::SurfaceFailed(_) => "terrain_surface_failed",
            Self::SamplingFailed { .. } => "terrain_sampling_failed",
            Self::BreaklinesUnsupported { .. } => "terrain_breaklines_unsupported",
            Self::ProviderFailed(_) => "terrain_provider_failed",
            Self::IntegrityFailed(_) => "terrain_integrity_failed",
        }
    }
}

impl From<SamplingError> for SamplingFileError {
    fn from(error: SamplingError) -> Self {
        Self::SamplingFailed {
            code: error.code(),
            detail: error.to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct InputEvidence {
    pub role: String,
    pub path: String,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ArtifactEvidence {
    /// Filename relative to output_directory.
    pub file: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct RoutePreview {
    pub route_id: String,
    pub feature_index: usize,
    pub part_index: usize,
    pub input_vertices: usize,
    pub query_count: usize,
    pub supported_profile_samples: Option<usize>,
    pub missing_profile_samples: Option<usize>,
}

/// Bounded CLI receipt. Full settings, route results, acquisition evidence and
/// discrete error/density records are delivered in files, never inline here.
#[derive(Debug, Serialize)]
pub struct SamplingFileReceipt {
    pub schema: &'static str,
    pub dry_run: bool,
    pub output_directory: String,
    pub projected_crs: String,
    pub source: Value,
    pub inputs: Vec<InputEvidence>,
    pub settings_sha256: String,
    pub seed: u64,
    pub route_count: usize,
    pub input_route_vertices: usize,
    pub planned_query_count: usize,
    pub query_count: usize,
    pub derived_row_class: &'static str,
    pub source_observations_mutated: bool,
    pub source_measurements_independently_verified: bool,
    pub route_preview: Vec<RoutePreview>,
    pub more_routes: usize,
    pub files: Vec<ArtifactEvidence>,
}

struct RouteResult {
    route: input::InputRoute,
    result: SamplingResult,
}

/// Validate native inputs, sample one independently selected source, then
/// write a fresh local directory. A dry run validates routes and survey data
/// without provider acquisition, surface queries, or output creation.
pub fn sample_file(
    request_path: &Path,
    out: &Path,
    dry_run: bool,
) -> Result<SamplingFileReceipt, SamplingFileError> {
    let prepared = input::prepare(request_path, out)?;
    let settings_bytes = serde_json::to_vec(&prepared.request.settings)
        .map_err(|error| SamplingFileError::RequestInvalid(error.to_string()))?;
    let mut receipt = SamplingFileReceipt {
        schema: "ds-terrain-sampling-file/v1",
        dry_run,
        output_directory: prepared.out.to_string_lossy().into_owned(),
        projected_crs: prepared.projection.canonical_definition(),
        source: prepared.source_evidence.clone(),
        inputs: prepared.inputs.clone(),
        settings_sha256: input::digest(&settings_bytes),
        seed: prepared.request.settings.seed,
        route_count: prepared.routes.len(),
        input_route_vertices: prepared.routes.iter().map(|route| route.points.len()).sum(),
        planned_query_count: prepared.planned_queries,
        query_count: 0,
        derived_row_class: DERIVED_ROW_CLASS,
        source_observations_mutated: false,
        source_measurements_independently_verified: false,
        route_preview: prepared
            .routes
            .iter()
            .take(5)
            .map(|route| RoutePreview {
                route_id: route.route_id.clone(),
                feature_index: route.feature_index,
                part_index: route.part_index,
                input_vertices: route.points.len(),
                query_count: route.planned_queries,
                supported_profile_samples: None,
                missing_profile_samples: None,
            })
            .collect(),
        more_routes: prepared.routes.len().saturating_sub(5),
        files: Vec::new(),
    };
    output::check_receipt(&receipt)?;
    if dry_run {
        return Ok(receipt);
    }

    // Every route and the aggregate query budget were validated before this
    // constructor may acquire pinned provider metadata.
    let mut surface = surface::FileSurface::new(prepared.surface, prepared.projection)?;
    let mut results = Vec::with_capacity(prepared.routes.len());
    for route in prepared.routes {
        let result = ds_terrain_sampling::sample_route(
            &route.points,
            &mut surface,
            &prepared.request.settings,
        )
        .map_err(|error| surface.take_failure().unwrap_or_else(|| error.into()))?;
        receipt.query_count += result.query_count;
        if let Some(preview) = receipt.route_preview.get_mut(results.len()) {
            let (supported, missing) = output::profile_counts(&result);
            preview.supported_profile_samples = Some(supported);
            preview.missing_profile_samples = Some(missing);
        }
        results.push(RouteResult { route, result });
    }
    output::write_artifacts(
        &prepared.out,
        &prepared.request,
        prepared.projection,
        &prepared.inputs,
        &prepared.source_evidence,
        &results,
        surface.acquisition_evidence(),
        prepared.survey_bytes.as_deref(),
        &mut receipt,
    )?;
    output::check_receipt(&receipt)?;
    Ok(receipt)
}

/// Descriptor generated from the exact strict serde request type.
pub fn describe() -> Value {
    json!({
        "schema": "ds-terrain-sampling-descriptor/v1",
        "request_schema": schemars::schema_for!(SamplingFileRequest),
        "defaults": {"settings": SamplingOptions::default()},
        "sources": ["rwanda_tiff", "survey_tin"],
        "semantics": {
            "route": "WGS84 longitude/latitude GeoJSON LineString, MultiLineString, Feature or FeatureCollection; IDs and part indices retained; projected by ds-geo",
            "input_paths": "relative input paths resolve from the request JSON directory",
            "surface": "one source only; no Terrarium, blending, datum conversion, height adjustment or smoothing",
            "survey": "declared CSV bytes retained verbatim; measurement authenticity and vertical datum remain unverified/unknown",
            "breaklines": "nonempty authored breaklines refuse until constrained triangulation is supported",
            "profiles": "centerline is mandatory; side_offsets_m defaults empty and explicit side profiles are queried at their exact nominal offsets",
            "randomness": "seeded longitudinal/observation-position randomness never changes source heights or profile offsets",
            "coverage": "actual unsupported surface queries are gaps; acquisition/integrity failures refuse all output",
            "accuracy": "height error is discrete against the declared baseline queries; no continuous accuracy guarantee or survey/design compliance claim",
            "density": "supplied engineering intervals/weight spans govern declared density; absent context is explicitly limited",
            "output": "fresh local directory only; every sampled row is derived_interpolated; full results/settings/evidence in artifacts, bounded receipt inline",
            "dry_run": "validate local route/settings/survey surface and output freshness; no provider acquisition, surface queries or file writes"
        },
        "limits": {
            "input_file_bytes": MAX_INPUT_BYTES,
            "aggregate_route_vertices": ds_terrain_sampling::MAX_ROUTE_VERTICES,
            "survey_points": ds_geo_lite::projected_tin::MAX_PROJECTED_TIN_POINTS,
            "aggregate_surface_queries": ds_terrain_sampling::MAX_SURFACE_QUERIES,
            "provider_batch_queries": ds_terrain_sampling::MAX_QUERY_CHUNK,
            "provider_acquisition_bytes": 256 * 1024 * 1024,
            "decoded_rwanda_blocks": 64,
            "output_bytes": MAX_OUTPUT_BYTES,
            "receipt_bytes": MAX_RECEIPT_BYTES,
            "route_preview": 5
        },
        "refusal_codes": ["terrain_request_invalid", "terrain_input_too_large", "terrain_output_exists", "terrain_io", "terrain_source_invalid", "terrain_surface_failed", "terrain_sampling_failed", "terrain_breaklines_unsupported", "terrain_provider_failed", "terrain_integrity_failed"]
    })
}
