//! Native file orchestration for single-source, reproducible terrain
//! sampling: the host half (DECOUPLING-013).
//!
//! The Network engine reads the route and survey files, plans, samples,
//! reduces and writes the artifacts; this host fetches the public Rwanda
//! rasters it names (with byte evidence) and moves the finished directory to
//! the fresh `--out` the receipt names.

use std::path::{Path, PathBuf};

use ds_elevation_host::{
    capture_engine, engine_error, run_job, ComparisonError, JobError, PublicRasters,
};
use ds_network_contracts::elevation::{ElevationResultV1, ElevationTaskV1, TERRAIN_ARTIFACT_DIR};
use serde_json::Value;
use thiserror::Error;

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
    #[error("authored terrain breaklines require constrained triangulation: {0}")]
    BreaklinesUnsupported(String),
    #[error("terrain provider acquisition failed: {0}")]
    ProviderFailed(String),
    #[error("terrain provider integrity failed: {0}")]
    IntegrityFailed(String),
    /// The Network engine that samples terrain is absent or too old.
    #[error("{code}: {message}")]
    Engine { code: &'static str, message: String },
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
            Self::BreaklinesUnsupported(_) => "terrain_breaklines_unsupported",
            Self::ProviderFailed(_) => "terrain_provider_failed",
            Self::IntegrityFailed(_) => "terrain_integrity_failed",
            Self::Engine { code, .. } => code,
        }
    }
}

/// The engine's refusal in this host's vocabulary (its codes are the
/// sampler's own), and public acquisition failures as provider failures.
fn from_comparison(error: ComparisonError) -> SamplingFileError {
    match error {
        ComparisonError::Engine { code, message } => SamplingFileError::Engine { code, message },
        ComparisonError::Integrity(message) => SamplingFileError::IntegrityFailed(message),
        ComparisonError::Io(message) => SamplingFileError::Io(message),
        other => SamplingFileError::ProviderFailed(other.to_string()),
    }
}

fn from_job(error: JobError) -> SamplingFileError {
    match error {
        JobError::Acquisition(error) => from_comparison(error),
        JobError::Engine(failure) => from_engine(failure),
    }
}

fn from_engine(failure: ds_network_client::NetworkFailure) -> SamplingFileError {
    let message = failure.message.clone();
    match failure.owner_code().unwrap_or(failure.code()) {
        "terrain_request_invalid" | "elevation_request_invalid" => {
            SamplingFileError::RequestInvalid(message)
        }
        "terrain_input_too_large" => SamplingFileError::InputTooLarge(message),
        "terrain_output_exists" => SamplingFileError::OutputExists(message),
        "terrain_io" => SamplingFileError::Io(message),
        "terrain_source_invalid" => SamplingFileError::SourceInvalid(message),
        "terrain_surface_failed" => SamplingFileError::SurfaceFailed(message),
        "terrain_sampling_failed" => SamplingFileError::SamplingFailed {
            code: "terrain_sampling_failed",
            detail: message,
        },
        "terrain_breaklines_unsupported" => SamplingFileError::BreaklinesUnsupported(message),
        "terrain_provider_failed" => SamplingFileError::ProviderFailed(message),
        "terrain_integrity_failed" => SamplingFileError::IntegrityFailed(message),
        _ => from_comparison(engine_error(failure)),
    }
}

/// Validate native inputs, sample one independently selected source, then
/// write a fresh local directory. A dry run validates routes and survey data
/// without provider acquisition, surface queries, or output creation.
pub fn sample_file(
    request_path: &Path,
    out: &Path,
    dry_run: bool,
) -> Result<Value, SamplingFileError> {
    let request = std::fs::canonicalize(request_path)
        .map_err(|error| SamplingFileError::Io(error.to_string()))?;
    if std::fs::symlink_metadata(out).is_ok() {
        return Err(SamplingFileError::OutputExists(
            out.to_string_lossy().into_owned(),
        ));
    }
    let parent = out
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent =
        std::fs::canonicalize(parent).map_err(|error| SamplingFileError::Io(error.to_string()))?;
    let output: PathBuf = parent.join(out.file_name().ok_or_else(|| {
        SamplingFileError::RequestInvalid("output must name a fresh directory".into())
    })?);
    let client = capture_engine(None).map_err(from_comparison)?;
    let mut rasters = PublicRasters::new().map_err(from_comparison)?;
    let completed = run_job(
        &client,
        &mut rasters,
        ElevationTaskV1::Terrain {
            request,
            output: output.clone(),
            dry_run,
            acquisition: Vec::new(),
        },
        &mut |request, evidence| {
            if let ElevationTaskV1::Terrain { acquisition, .. } = &mut request.task {
                *acquisition = evidence.to_vec();
            }
        },
    )
    .map_err(from_job)?;
    let ElevationResultV1::Terrain { receipt } = completed.result.clone() else {
        return Err(SamplingFileError::IntegrityFailed(
            "the Network engine answered another elevation task".into(),
        ));
    };
    if !dry_run {
        let written = completed.artifact(TERRAIN_ARTIFACT_DIR);
        // A fresh directory beside the engine's: same device, one rename.
        if std::fs::symlink_metadata(&output).is_ok() {
            return Err(SamplingFileError::OutputExists(
                output.to_string_lossy().into_owned(),
            ));
        }
        std::fs::rename(&written, &output)
            .or_else(|_| copy_dir(&written, &output))
            .map_err(|error| SamplingFileError::Io(error.to_string()))?;
    }
    Ok(receipt)
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        std::fs::copy(entry.path(), to.join(entry.file_name()))?;
    }
    Ok(())
}

/// Descriptor generated by the engine from its exact strict request types.
pub fn describe() -> Result<Value, SamplingFileError> {
    let client = capture_engine(None).map_err(from_comparison)?;
    let mut rasters = PublicRasters::new().map_err(from_comparison)?;
    let completed = run_job(
        &client,
        &mut rasters,
        ElevationTaskV1::TerrainDescribe {},
        &mut |_, _| {},
    )
    .map_err(from_job)?;
    match completed.result {
        ElevationResultV1::TerrainDescribe { descriptor } => Ok(descriptor),
        _ => Err(SamplingFileError::IntegrityFailed(
            "the Network engine answered another elevation task".into(),
        )),
    }
}
