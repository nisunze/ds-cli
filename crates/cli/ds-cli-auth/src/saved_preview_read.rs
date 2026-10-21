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

/// Exact published output acquisition under the native project authority. A
/// caller pins its selected revision and bytes; no newer head is substituted.
pub struct PublicationOutputSelection<'a> {
    pub engine: &'a str,
    pub operation: &'a str,
    pub variant: &'a str,
    pub output_id: &'a str,
    pub head_revision: u64,
    pub sha256: &'a str,
    pub size_bytes: u64,
}

pub fn publication_output_for_project(
    lane: &str,
    project: &str,
    selected: &PublicationOutputSelection<'_>,
    destination: &Path,
) -> Result<Head, Failure> {
    let PublicationOutputSelection {
        engine,
        operation,
        variant,
        output_id,
        head_revision,
        sha256,
        size_bytes,
    } = *selected;
    use ds_sync_runtime::{Gateway, SyncRoute};
    ds_client_core::validate_project_id(project).map_err(|e| unreadable(e.to_string()))?;
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
    guard()?;
    let raw=session.post(SyncRoute::ComputeArtifacts,&serde_json::json!({"action":"read","project_id":project,"engine":engine,"operation":operation,"variant":variant})).map_err(crate::sync::publication_read_error)?;
    let head = ds_command_kernel::compute_artifact_inventory::decode_head(
        &serde_json::to_vec(&raw).map_err(|e| unreadable(e.to_string()))?,
    )
    .map_err(unreadable)?;
    if head.engine != engine
        || head.operation != operation
        || head.variant != variant
        || head.head_revision != head_revision
        || !head
            .outputs
            .iter()
            .any(|o| o.output_id == output_id && o.sha256 == sha256 && o.size_bytes == size_bytes)
    {
        return Err(Failure::conflict(
            "publication_conflict",
            "The selected immutable publication changed; refresh inventory before selecting again.",
        ));
    }
    let reads = VerifiedReads::new(
        destination
            .parent()
            .ok_or_else(|| unreadable("Missing output parent."))?
            .into(),
    );
    ds_sync_runtime::retained_preview::read_output(
        &session,
        &reads,
        project,
        &head,
        output_id,
        destination,
        &guard,
    )
    .map_err(|error| match error {
        ReadError::Scope(e) => e,
        ReadError::Gateway(e) => crate::sync::publication_read_error(e),
        ReadError::Unreadable(e) => unreadable(e),
        ReadError::HeadMoved => Failure::conflict(
            "publication_conflict",
            "The publication moved while acquiring the selected output.",
        ),
    })?;
    Ok(head)
}
