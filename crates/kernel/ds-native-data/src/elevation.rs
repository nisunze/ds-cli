//! Shared native application elevation interpolation: the host half.
//!
//! Source bytes and results stay on this machine. Interpolation runs in the
//! Network engine (DECOUPLING-013) over the verified Rwanda DEM resource this
//! host holds — the installed COG, or exact public ranges it fetches into its
//! block cache — and, only when the request names it, the AWS Terrarium tiles
//! it fetches. No DS API, Cloud Run route, or project upload is reachable
//! from this module.

use crate::bridge::CommandFailure;
use crate::engine_host::{place, EngineJob};
use crate::RequestContext;
use ds_network_contracts::elevation::{ElevationResultV1, ElevationTaskV1, ATTACH_ARTIFACT};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub use ds_network_contracts::elevation::{
    AttachEvidenceV1 as NativeElevationEvidence, AttachOutcomeV1 as NativeElevationOutcome,
    AttachRefusalCodeV1 as NativeElevationRefusalCode, AttachRefusalV1 as NativeElevationRefusal,
    AttachRequestV1 as NativeElevationRequest, AttachSourceCrsV1 as NativeElevationSourceCrs,
};

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

/// The byte count and SHA-256 of one artifact the engine wrote.
pub(crate) fn sha256_file(path: &Path) -> Result<(u64, String), String> {
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

/// An artifact is placed only when its bytes are the ones the engine named.
pub(crate) fn place_verified(
    artifact: &Path,
    destination: &Path,
    bytes: u64,
    sha256: &str,
) -> Result<(), CommandFailure> {
    let (actual_bytes, actual_sha256) = sha256_file(artifact).map_err(CommandFailure::failed)?;
    if actual_bytes != bytes || actual_sha256 != sha256 {
        return Err(CommandFailure::failed(
            "the Network engine's artifact differs from its own evidence".into(),
        ));
    }
    place(artifact, destination).map_err(CommandFailure::failed)
}

fn requires_full(outcome: &NativeElevationOutcome) -> bool {
    matches!(
        outcome,
        NativeElevationOutcome::Refused {
            refusal: NativeElevationRefusal {
                code: NativeElevationRefusalCode::FullLocalDemRequired,
                ..
            }
        }
    )
}

/// One admitted local-file operation, including its required governed resource
/// and full-local retry. Every caller gets the same admission and recovery.
pub fn interpolate(
    context: &RequestContext,
    request: NativeElevationRequest,
) -> Result<NativeElevationOutcome, String> {
    interpolate_typed(context, request).map_err(|failure| failure.to_string())
}

/// [`interpolate`] with the native data refusal vocabulary kept.
pub(crate) fn interpolate_typed(
    context: &RequestContext,
    request: NativeElevationRequest,
) -> Result<NativeElevationOutcome, CommandFailure> {
    context.validate().map_err(CommandFailure::failed)?;
    request.validate().map_err(CommandFailure::failed)?;
    let outcome = interpolate_once(context, &request)?;
    if requires_full(&outcome) {
        crate::rwanda_dem::ensure_available(context, true).map_err(CommandFailure::failed)?;
        return interpolate_once(context, &request);
    }
    Ok(outcome)
}

fn interpolate_once(
    context: &RequestContext,
    request: &NativeElevationRequest,
) -> Result<NativeElevationOutcome, CommandFailure> {
    let (source, _) =
        absolute_regular_source(&request.source_path).map_err(CommandFailure::failed)?;
    let output =
        absolute_new_output(&request.output_path, &source).map_err(CommandFailure::failed)?;
    crate::rwanda_dem::ensure_available(context, false).map_err(CommandFailure::failed)?;
    let runtime =
        crate::rwanda_dem::verified_rwanda_dem_runtime().map_err(CommandFailure::failed)?;
    let job = EngineJob::open(output.parent().unwrap_or(Path::new("/")))?;
    let completed = job.run(
        context,
        Some(&runtime),
        ElevationTaskV1::Attach {
            request: request.clone(),
        },
        &|| false,
    )?;
    let ElevationResultV1::Attach { outcome } = completed.result.clone() else {
        return Err(CommandFailure::failed(
            "the Network engine answered another elevation task".into(),
        ));
    };
    if let NativeElevationOutcome::Completed { evidence } = &outcome {
        if Path::new(&evidence.output_path) != output {
            return Err(CommandFailure::failed(
                "the Network engine named another output path".into(),
            ));
        }
        place_verified(
            &completed.artifact(ATTACH_ARTIFACT),
            &output,
            evidence.output_bytes,
            &evidence.output_sha256,
        )?;
    }
    Ok(outcome)
}

#[cfg(test)]
#[path = "elevation_tests.rs"]
mod tests;
