//! Shared native application elevation point-cloud extraction: the host half.
//!
//! A DIFFERENT operation from `native_elevation_interpolate`, which attaches
//! elevation to a point file the operator already has. This one takes an AREA —
//! drawn on the map, or a local area file, or a bounding box — and produces the
//! points. Planning a grading exercise needs the second; the first cannot
//! substitute for it.
//!
//! Generation and sampling run in the Network engine (DECOUPLING-013), the
//! shared `ds_elevation::point_cloud` kernel, over the verified Rwanda DEM
//! resource this host holds and the exact ranges or tiles it fetches. The
//! host admits one extraction at a time, reports progress, and places the
//! two files. No DS API, Cloud Run route, or project upload is reachable.
//!
//! Two commands, because they answer different questions at different costs:
//! `plan` counts and touches nothing; `extract` samples the DEM and writes
//! files. Above 4,000 points the plan reports that the full local component is
//! required BEFORE the extract spends a single range read discovering it.

use crate::RequestContext;
use crate::bridge::CommandFailure;
use crate::elevation::{absolute_new_output, place_verified};
use crate::engine_host::EngineJob;
use ds_network_contracts::elevation::{
    ElevationResultV1, ElevationTaskV1, POINT_CLOUD_CSV_ARTIFACT, POINT_CLOUD_GEOJSON_ARTIFACT,
    PointCloudTaskV1,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub use ds_network_contracts::elevation::{
    PointCloudAreaStatsV1 as NativePointCloudAreaStats,
    PointCloudEvidenceV1 as NativePointCloudEvidence,
    PointCloudOutcomeV1 as NativePointCloudOutcome,
    PointCloudPlanEvidenceV1 as NativePointCloudPlanEvidence,
    PointCloudPlanOutcomeV1 as NativePointCloudPlanOutcome,
    PointCloudRefusalCodeV1 as NativePointCloudRefusalCode,
    PointCloudRefusalV1 as NativePointCloudRefusal, PointCloudRequestV1 as NativePointCloudRequest,
    PointCloudSamplingV1 as NativePointCloudSampling,
};

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

/// An inline area travels to the engine as a file in the job's directory, so
/// the request it receives stays small.
fn engine_task(
    request: &NativePointCloudRequest,
    job_dir: &Path,
) -> Result<PointCloudTaskV1, CommandFailure> {
    let mut request = request.clone();
    let inline_area = match request.area_geojson.take() {
        Some(text) if !text.trim().is_empty() => {
            let path = job_dir.join("inline-area.geojson");
            crate::elevation::write_atomic(&path, text.as_bytes())
                .map_err(CommandFailure::failed)?;
            Some(path)
        }
        _ => None,
    };
    Ok(PointCloudTaskV1 {
        request,
        inline_area,
    })
}

fn plan_job_dir() -> Result<tempfile::TempDir, CommandFailure> {
    tempfile::Builder::new()
        .prefix("ds-point-cloud-plan-")
        .tempdir()
        .map_err(|error| CommandFailure::failed(format!("create point-cloud plan: {error}")))
}

pub fn plan(
    context: &RequestContext,
    request: NativePointCloudRequest,
) -> Result<NativePointCloudPlanOutcome, String> {
    plan_typed(context, request).map_err(|failure| failure.to_string())
}

/// Count an area's cloud. Routing is decided from the exact count, before any
/// DEM byte: the plan reads no raster at all.
pub(crate) fn plan_typed(
    context: &RequestContext,
    request: NativePointCloudRequest,
) -> Result<NativePointCloudPlanOutcome, CommandFailure> {
    context.validate().map_err(CommandFailure::failed)?;
    request.validate().map_err(CommandFailure::failed)?;
    let dir = plan_job_dir()?;
    let job = EngineJob::open(dir.path())?;
    let task = engine_task(&request, dir.path())?;
    let completed = job.run(
        context,
        None,
        ElevationTaskV1::PointCloudPlan { request: task },
        &|| false,
    )?;
    match completed.result {
        ElevationResultV1::PointCloudPlan { outcome } => Ok(outcome),
        _ => Err(CommandFailure::failed(
            "the Network engine answered another elevation task".into(),
        )),
    }
}

/// Shared extraction application operation, including resource acquisition and
/// the exact full-local retry used by every native caller.
pub fn extract(
    context: &RequestContext,
    state: &NativePointCloudState,
    request: NativePointCloudRequest,
) -> Result<NativePointCloudOutcome, String> {
    extract_typed(context, state, request).map_err(|failure| failure.to_string())
}

pub(crate) fn extract_typed(
    context: &RequestContext,
    state: &NativePointCloudState,
    request: NativePointCloudRequest,
) -> Result<NativePointCloudOutcome, CommandFailure> {
    let _admission = state.admit(context).map_err(CommandFailure::failed)?;
    request.validate().map_err(CommandFailure::failed)?;
    let outcome = extract_once(context, state, &request)?;
    if matches!(
        &outcome,
        NativePointCloudOutcome::Refused {
            refusal: NativePointCloudRefusal {
                code: NativePointCloudRefusalCode::FullLocalDemRequired,
                ..
            }
        }
    ) {
        crate::rwanda_dem::ensure_available(context, true).map_err(CommandFailure::failed)?;
        return extract_once(context, state, &request);
    }
    Ok(outcome)
}

fn extract_once(
    context: &RequestContext,
    state: &NativePointCloudState,
    request: &NativePointCloudRequest,
) -> Result<NativePointCloudOutcome, CommandFailure> {
    context.validate().map_err(CommandFailure::failed)?;
    let output_raw = request
        .output_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| {
            CommandFailure::failed("Native extraction requires an explicit output path.".into())
        })?;
    // Guarded against overwriting the area file it was derived from, and
    // against a CSV sibling that already exists — a half-refused write would
    // leave one honest file beside one stale one.
    let area_source = request
        .area_path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_default();
    let output = absolute_new_output(output_raw, &area_source).map_err(CommandFailure::failed)?;
    let csv_path = output.with_extension("csv");
    if csv_path.exists() {
        return Err(CommandFailure::failed(format!(
            "{} already exists; overwrite is refused",
            csv_path.display()
        )));
    }
    crate::rwanda_dem::ensure_available(context, false).map_err(CommandFailure::failed)?;
    let runtime =
        crate::rwanda_dem::verified_rwanda_dem_runtime().map_err(CommandFailure::failed)?;
    let job = EngineJob::open(output.parent().unwrap_or(Path::new("/")))?;
    let task = engine_task(request, output.parent().unwrap_or(Path::new("/")))?;
    let inline = task.inline_area.clone();
    let cancelled = state.begin(0);
    state.publish(
        true,
        "sampling",
        0,
        0,
        "Sampling elevation in the Network engine",
    );
    let flag = Arc::clone(&cancelled);
    let completed = job.run(
        context,
        Some(&runtime),
        ElevationTaskV1::PointCloudExtract { request: task },
        &move || flag.load(Ordering::Relaxed),
    );
    if let Some(path) = inline {
        let _ = std::fs::remove_file(path);
    }
    let completed = match completed {
        Ok(completed) => completed,
        Err(_) if cancelled.load(Ordering::Relaxed) => {
            state.finish("Extraction cancelled", 0);
            return Ok(NativePointCloudOutcome::Refused {
                refusal: NativePointCloudRefusal {
                    code: NativePointCloudRefusalCode::Cancelled,
                    message: "The extraction was cancelled".to_string(),
                    point_count: None,
                    point_limit: None,
                    remedy: "Run it again when ready; nothing was written".to_string(),
                },
            });
        }
        Err(failure) => {
            state.finish("Extraction failed", 0);
            return Err(failure);
        }
    };
    let ElevationResultV1::PointCloudExtract { outcome } = completed.result.clone() else {
        state.finish("Extraction failed", 0);
        return Err(CommandFailure::failed(
            "the Network engine answered another elevation task".into(),
        ));
    };
    if let NativePointCloudOutcome::Completed { evidence } = &outcome {
        state.publish(
            true,
            "writing",
            evidence.point_count,
            evidence.point_count,
            "Writing result",
        );
        let placed = place_verified(
            &completed.artifact(POINT_CLOUD_GEOJSON_ARTIFACT),
            &output,
            evidence.output_bytes,
            &evidence.output_sha256,
        )
        .and_then(|()| {
            // The CSV is the second half of one result: if it cannot be
            // placed, the GeoJSON half must not stay behind.
            place_verified(
                &completed.artifact(POINT_CLOUD_CSV_ARTIFACT),
                &csv_path,
                evidence.csv_bytes,
                &evidence.csv_sha256,
            )
            .inspect_err(|_| {
                let _ = std::fs::remove_file(&output);
            })
        });
        if let Err(failure) = placed {
            state.finish("Extraction failed", 0);
            return Err(failure);
        }
        state.finish("Extraction complete", evidence.point_count);
    } else {
        state.finish("Extraction refused", 0);
    }
    Ok(outcome)
}

#[cfg(test)]
#[path = "point_cloud_tests.rs"]
mod tests;
