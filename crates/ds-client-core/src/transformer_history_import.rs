//! Fixed Bulk import transport; source preparation remains the native file owner's job.
use crate::{
    AuthorizedTransport, ClientError, ClientProfile, DesignVersionsCall, ErrorKind,
    authorized::Authority,
};
pub use ds_command_kernel::transformer_history_import::{Import, Prepared, Source};
use serde_json::Value;
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    input: &Import,
) -> Result<Value, ClientError> {
    let payload = input.payload(project).map_err(|_| {
        ClientError::new(ErrorKind::InvalidInput, "invalid pinned historical import")
    })?;
    let response = transport.design_versions(DesignVersionsCall {
        profile,
        authority: authority.call(),
        payload,
    })?;
    let value = super::design_version_catalog::decode_authority_response(response)?;
    input.verify_response(project, &value).map_err(|_| {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "historical import acknowledgement differs from exact archival pins",
        )
    })?;
    Ok(value)
}
