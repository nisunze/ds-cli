//! Controlled source previews through the established native credential fence
//! and common file room. Offline consumes no restored session or source grant.
use crate::{
    LayerScopeFence, NativeTransport, capture_layer_scope_fence_for_project,
    headless_identity_for_named_project, headless_named_project,
    verify_layer_scope_fence_for_project,
};
use ds_cli_contract::Failure;
use ds_client_core::asset_preview_source::{Grant, TransferControl, TransferError};
use ds_command_kernel::asset_preview::{Request, SourceProof};
use ds_project_data::preview_room::Room;
use ds_report_host::{
    HostFailure,
    asset_preview::{self, Control, Phase, Receipt, SourceFailure, SourceProvider},
};
use std::{
    cell::RefCell,
    fs::File,
    sync::atomic::{AtomicBool, Ordering},
};

pub struct CapturedControl<'a> {
    lane: &'a str,
    project: &'a str,
    fence: LayerScopeFence,
    cancelled: &'a AtomicBool,
    on_progress: &'a dyn Fn(Phase, u64, Option<u64>),
    original_failure: RefCell<Option<Failure>>,
}
impl CapturedControl<'_> {
    pub fn uid(&self) -> &str {
        self.fence.uid()
    }
    pub fn audience(&self) -> &str {
        self.fence.audience()
    }
}
fn host(error: Failure) -> HostFailure {
    HostFailure::new(error.code(), error.message())
}
impl Control for CapturedControl<'_> {
    fn check_current(&self) -> Result<(), HostFailure> {
        if self.cancelled.load(Ordering::SeqCst) {
            return Err(HostFailure::new(
                "asset_preview_cancelled",
                "The intentional preview was cancelled.",
            ));
        }
        verify_layer_scope_fence_for_project(self.lane, &self.fence, self.fence.uid(), self.project)
            .map_err(|error| {
                *self.original_failure.borrow_mut() = Some(error.clone());
                host(error)
            })
    }
    fn progress(&self, phase: Phase, received: u64, total: Option<u64>) {
        (self.on_progress)(phase, received, total);
    }
}
struct ByteControl<'a>(&'a dyn Control);
impl TransferControl for ByteControl<'_> {
    fn check_current(&self) -> Result<(), TransferError> {
        self.0.check_current().map_err(|e| {
            if e.code == "asset_preview_cancelled" {
                TransferError::Cancelled
            } else {
                TransferError::ContextChanged
            }
        })
    }
    fn progress(&self, received: u64, total: u64) {
        self.0.progress(Phase::Acquiring, received, Some(total));
    }
}
struct Provider<'a> {
    lane: &'a str,
    project: &'a str,
    uid: &'a str,
    audience: &'a str,
    original_failure: &'a RefCell<Option<Failure>>,
    grant: Option<Grant>,
}
impl SourceProvider for Provider<'_> {
    fn authorize(
        &mut self,
        request: &Request,
        control: &dyn Control,
    ) -> Result<SourceProof, SourceFailure> {
        control.check_current()?;
        let named = headless_named_project(
            self.lane,
            self.project,
            |device, project| device.authorize_asset_preview(project, request),
            |client, project| client.authorize_asset_preview(project, request, crate::now()),
        )
        .map_err(|error| {
            // Only an actual route-authored denial/deletion removes held data;
            // an outage, absent route or malformed proof never grants fallback.
            let denied = error.detail_value().is_some_and(|detail| {
                let status = detail["http_status"].as_u64();
                let code = detail["service_code"].as_str();
                // A code-bearing 401/403/404 is the fixed source authority's
                // refusal. Bare gateway errors/outages cannot revoke a file.
                matches!(status, Some(401 | 403 | 404)) && code.is_some()
            });
            *self.original_failure.borrow_mut() = Some(error.clone());
            SourceFailure {
                failure: host(error),
                retained_forbidden: denied,
            }
        })?;
        control.check_current()?;
        if named.identity().uid() != self.uid
            || named.identity().credential_audience_sha256() != self.audience
            || named.project_id() != self.project
        {
            return Err(HostFailure::new(
                "project_context_changed",
                "The source grant has another native account, audience or project.",
            )
            .into());
        }
        let grant = named.into_result();
        let proof = grant.proof().clone();
        self.grant = Some(grant);
        Ok(proof)
    }
    fn acquire(
        &mut self,
        source: &SourceProof,
        writer: &mut File,
        control: &dyn Control,
    ) -> Result<(), HostFailure> {
        control.check_current()?;
        let grant = self
            .grant
            .as_ref()
            .filter(|g| g.proof() == source)
            .ok_or_else(|| {
                HostFailure::new(
                    "asset_preview_source_invalid",
                    "No matching generation grant was captured.",
                )
            })?;
        let result = grant.acquire(&mut NativeTransport, writer, &ByteControl(control));
        control.check_current()?;
        result.map_err(|error| match error {
            TransferError::SourceChanged => HostFailure::new(
                "asset_preview_source_changed",
                "Storage returned a different generation or byte count.",
            ),
            TransferError::Cancelled => HostFailure::new(
                "asset_preview_cancelled",
                "The intentional preview was cancelled.",
            ),
            TransferError::ContextChanged => HostFailure::new(
                "project_context_changed",
                "The native preview authority changed.",
            ),
            TransferError::TimedOut => HostFailure::new(
                "asset_preview_acquire_failed",
                "The intentional source read exceeded its bounded native timeout.",
            ),
            _ => HostFailure::new(
                "asset_preview_acquire_failed",
                "The exact source read could not complete; no preview was committed.",
            ),
        })
    }
}
pub(crate) fn captured_room<'a>(
    lane: &'a str,
    project: &'a str,
    cancelled: &'a AtomicBool,
    on_progress: &'a dyn Fn(Phase, u64, Option<u64>),
) -> Result<(Room, CapturedControl<'a>), Failure> {
    let fence = capture_layer_scope_fence_for_project(lane, project)?;
    let identity = headless_identity_for_named_project(lane)?;
    let control = CapturedControl {
        lane,
        project,
        fence,
        cancelled,
        on_progress,
        original_failure: RefCell::new(None),
    };
    control.check_current().map_err(failure)?;
    if identity.uid() != control.uid()
        || identity.credential_audience_sha256() != control.audience()
    {
        return Err(Failure::conflict(
            "project_context_changed",
            "The native account changed while capturing its document room.",
        ));
    }
    let room = Room {
        root: ds_layer_store::default_root()
            .map_err(|e| Failure::unavailable("print_preview_artifact_unavailable", e))?,
        scope: ds_command_kernel::project_dataset_cache::Scope {
            principal: identity.uid().into(),
            project: project.into(),
        },
        lane: identity.lane().into(),
        credential_audience_sha256: identity.credential_audience_sha256().into(),
    };
    Ok((room, control))
}
pub fn acquire_controlled(
    lane: &str,
    project: &str,
    request: &Request,
    cancelled: &AtomicBool,
    on_progress: &dyn Fn(Phase, u64, Option<u64>),
) -> Result<Receipt, Failure> {
    ds_client_core::asset_preview_source::validate_request(project, request).map_err(|_| {
        Failure::invalid(
            "asset_preview_pins_invalid",
            "The retained preview project/source reference or pins are invalid.",
        )
        .remedy(
            "Use one exact selected document; offline needs its retained revision, generation and SHA.",
        )
    })?;
    let (room, control) = captured_room(lane, project, cancelled, on_progress)?;
    let mut provider = Provider {
        lane,
        project,
        uid: control.uid(),
        audience: control.audience(),
        original_failure: &control.original_failure,
        grant: None,
    };
    asset_preview::acquire(&room, request, &mut provider, &control).map_err(|error| {
        control
            .original_failure
            .borrow_mut()
            .take()
            .unwrap_or_else(|| failure(error))
    })
}
pub fn acquire_for_named_project(
    lane: &str,
    project: &str,
    request: &Request,
) -> Result<Receipt, Failure> {
    acquire_controlled(
        lane,
        project,
        request,
        &AtomicBool::new(false),
        &|_, _, _| {},
    )
}
fn failure(error: HostFailure) -> Failure {
    // Original protected/source refusals return unchanged above. These are
    // only the closed native IO/admission causes, never service-body text.
    let remedy =
        "Retry the exact selected source online; offline needs its full retained native pins.";
    match error.code.as_str() {
        "asset_preview_pins_invalid" => {
            Failure::invalid("asset_preview_pins_invalid", error.message).remedy(remedy)
        }
        "asset_preview_source_invalid" => {
            Failure::invalid("asset_preview_source_invalid", error.message).remedy(remedy)
        }
        "asset_preview_source_changed" => {
            Failure::conflict("asset_preview_source_changed", error.message).remedy(remedy)
        }
        "asset_preview_retained_invalid" => {
            Failure::unavailable("asset_preview_retained_invalid", error.message).remedy(remedy)
        }
        "asset_preview_offline_absent" => {
            Failure::unavailable("asset_preview_offline_absent", error.message).remedy(remedy)
        }
        "asset_preview_acquire_failed" => {
            Failure::unavailable("asset_preview_acquire_failed", error.message).remedy(remedy)
        }
        "asset_preview_decode_failed" => {
            Failure::invalid("asset_preview_decode_failed", error.message).remedy(remedy)
        }
        "asset_preview_cancelled" => Failure::failed("asset_preview_cancelled", error.message)
            .remedy("Open the selected source again when you want its preview."),
        "project_context_changed" => {
            Failure::conflict("project_context_changed", error.message).remedy(remedy)
        }
        "report_inputs_invalid" => {
            Failure::invalid("report_inputs_invalid", error.message).remedy(remedy)
        }
        "print_preview_artifact_unavailable" => {
            Failure::unavailable("print_preview_artifact_unavailable", error.message).remedy(remedy)
        }
        _ => Failure::internal(
            "assets_unreadable",
            "The native preview owner returned an undeclared refusal.",
        )
        .remedy("Preserve the exact native request and report the owner gap."),
    }
}
