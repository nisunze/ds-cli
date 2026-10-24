//! Typed addon preparation; Core owns acquisition and storage effects only.
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use crate::cancel::CancelToken;
use ds_cli_exec::solar::{Solar, Task};
use ds_solar_contracts::reference_request::ReferenceRequest;
use ds_solar_contracts::{
    BundleBytes, PreparedSolarCityInput, ReferenceKey, ReferencePin, SolarCityInput, SolarError,
    SolarErrorCode, SolarResult,
};

#[derive(Debug, Clone)]
pub struct ReferencePlan {
    pub city: SolarCityInput,
    pub request: ReferenceRequest,
    pub key: ReferenceKey,
}

/// A selected Solar capability; no host implements the calculation policy.
pub trait PreparationTasks: Send + Sync {
    fn plan(&self, city: &SolarCityInput, pin: ReferencePin) -> SolarResult<ReferencePlan>;
    fn prepare(
        &self,
        city: SolarCityInput,
        pin: ReferencePin,
        bundle: &BundleBytes,
        prepared_at: &str,
    ) -> SolarResult<PreparedSolarCityInput>;
}

type Capture = dyn Fn() -> SolarResult<Arc<Solar>> + Send + Sync;
struct Selection {
    capture: Box<Capture>,
    engine: OnceLock<SolarResult<Arc<Solar>>>,
}

/// Capture is lazy and retained for this preparation scope. A saved prepared
/// input never calls this capability and never discovers a preparation addon.
#[derive(Clone)]
pub struct AddonPreparation {
    selection: Arc<Selection>,
    scratch: PathBuf,
    cancel: CancelToken,
}

impl AddonPreparation {
    pub fn installed() -> Self {
        Self::from_factory(std::env::temp_dir(), || {
            let path =
                crate::process::installed_engine_path().map_err(SolarError::artifact_failed)?;
            Solar::capture_pinned(&path).map_err(failure)
        })
    }

    /// The native owner captures its managed lane; authored requests cannot
    /// choose an executable, lane or capture factory.
    pub fn from_factory(
        scratch: impl Into<PathBuf>,
        capture: impl Fn() -> SolarResult<Solar> + Send + Sync + 'static,
    ) -> Self {
        Self {
            selection: Arc::new(Selection {
                capture: Box::new(move || capture().map(Arc::new)),
                engine: OnceLock::new(),
            }),
            scratch: scratch.into(),
            cancel: CancelToken::new(),
        }
    }

    /// Reuse the exact managed capture already held by the run/calculation.
    pub fn from_client(scratch: impl Into<PathBuf>, engine: Arc<Solar>) -> Self {
        Self {
            selection: Arc::new(Selection {
                capture: Box::new(move || Ok(engine.clone())),
                engine: OnceLock::new(),
            }),
            scratch: scratch.into(),
            cancel: CancelToken::new(),
        }
    }

    #[must_use]
    pub fn with_cancel(mut self, cancel: CancelToken) -> Self {
        self.cancel = cancel;
        self
    }

    fn invoke(
        &self,
        operation: ds_solar_contracts::preparation_task::ReferencePreparationOperation,
        bundle: Option<&BundleBytes>,
    ) -> SolarResult<ds_solar_contracts::preparation_task::ReferencePreparationOutput> {
        if self.cancel.is_cancelled() {
            return Err(SolarError::cancelled("Solar preparation cancelled"));
        }
        let engine = self
            .selection
            .engine
            .get_or_init(|| (self.selection.capture)())
            .as_ref()
            .map_err(Clone::clone)?;
        engine
            .with_task_executable(Task::PrepareReference, |path| {
                crate::process::preparation::invoke(
                    path,
                    &self.scratch,
                    operation,
                    bundle,
                    &self.cancel,
                )
            })
            .map_err(failure)?
            .map_err(|error| {
                if error.kind == crate::process::ProcessFailureKind::Cancelled {
                    SolarError::cancelled(error.message)
                } else {
                    error
                        .refusal
                        .unwrap_or_else(|| SolarError::artifact_failed(error.message))
                }
            })
    }
}

impl PreparationTasks for AddonPreparation {
    fn plan(&self, city: &SolarCityInput, pin: ReferencePin) -> SolarResult<ReferencePlan> {
        use ds_solar_contracts::preparation_task::{
            ReferencePreparationOperation as Op, ReferencePreparationOutput as Out,
        };
        match self.invoke(
            Op::Plan {
                city: city.clone(),
                pin,
            },
            None,
        )? {
            Out::Plan { city, request, key } => Ok(ReferencePlan {
                city: *city,
                request: *request,
                key: *key,
            }),
            Out::Prepared { .. } => Err(SolarError::artifact_failed(
                "Solar returned the wrong preparation operation",
            )),
        }
    }

    fn prepare(
        &self,
        city: SolarCityInput,
        pin: ReferencePin,
        bundle: &BundleBytes,
        prepared_at: &str,
    ) -> SolarResult<PreparedSolarCityInput> {
        use ds_solar_contracts::preparation_task::{
            ReferencePreparationOperation as Op, ReferencePreparationOutput as Out,
        };
        let artifacts = crate::process::preparation::descriptors(bundle)?;
        match self.invoke(
            Op::Prepare {
                city,
                pin,
                prepared_at: prepared_at.into(),
                artifacts,
            },
            Some(bundle),
        )? {
            Out::Prepared { input } => Ok(*input),
            Out::Plan { .. } => Err(SolarError::artifact_failed(
                "Solar returned the wrong preparation operation",
            )),
        }
    }
}

fn failure(error: ds_core_contracts::Failure) -> SolarError {
    if matches!(
        error.code(),
        "solar_engine_update_required" | "unknown_task"
    ) {
        SolarError::new(
            SolarErrorCode::SolarEngineUpdateRequired,
            "solar_engine_update_required: install a Solar addon for the captured lane that declares the requested preparation task",
        )
    } else {
        SolarError::artifact_failed(format!("{}: {error}", error.code()))
    }
}
