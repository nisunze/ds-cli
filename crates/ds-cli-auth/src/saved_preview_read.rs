//! One fixed retained A4 head and verified byte download. Opening never reads
//! current engineering inputs or renders, and never wakes publication.
use ds_cli_contract::Failure;
use ds_command_kernel::compute_artifact_inventory::Head;
use ds_sync_runtime::{
    VerifiedReads,
    retained_preview::{ReadError, read_a4},
};
use std::path::Path;

pub fn retained_a4_for_project(
    lane: &str,
    project: &str,
    transformer: &str,
    destination: &Path,
) -> Result<Head, Failure> {
    ds_client_core::validate_project_id(project).map_err(|error| unreadable(error.to_string()))?;
    ds_command_kernel::report_export::reportable_transformer(transformer).map_err(unreadable)?;
    let fence = crate::capture_layer_scope_fence_for_project(lane, project)?;
    let principal = crate::headless_identity_for_named_project(lane)?;
    let session = crate::sync::NativeSyncSession::open(
        crate::profile::Lane::parse(lane)?,
        principal,
        project.into(),
        crate::runtime_credential_binding(lane)?,
    )
    .map_err(|detail| Failure::unavailable("publication_unavailable", detail))?;
    let guard = || crate::verify_layer_scope_fence_for_project(lane, &fence, fence.uid(), project);
    let reads = VerifiedReads::new(
        destination
            .parent()
            .ok_or_else(|| unreadable("The destination has no parent."))?
            .into(),
    );
    read_a4(&session, &reads, project, transformer, destination, &guard).map_err(
        |error| match error {
            ReadError::Scope(error) => error,
            ReadError::Gateway(error) => crate::sync::publication_read_error(error),
            ReadError::Unreadable(message) => unreadable(message),
            ReadError::HeadMoved => Failure::conflict(
                "publication_conflict",
                "The retained head moved while its download ticket was being read.",
            )
            .remedy("Retry the same retained read; do not render or substitute a ticket."),
        },
    )
}

fn unreadable(message: impl Into<String>) -> Failure {
    Failure::unavailable("publication_unreadable", message).remedy("Preserve the shared head and retry the same read; do not recalculate or bypass a refused download.")
}
