//! Fixed native transport for the pure catalog wire contract.
use crate::{
    AuthorizedTransport, ClientError, ClientProfile, DesignVersionsCall, ErrorKind,
    authorized::Authority,
};
pub use ds_command_kernel::design_version_catalog::{Command, Mutation};
use serde_json::Value;
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Value, ClientError> {
    let payload = command.payload(project).map_err(|_| {
        ClientError::new(ErrorKind::InvalidInput, "invalid version catalog request")
    })?;
    let response = transport.design_versions(DesignVersionsCall {
        profile,
        authority: authority.call(),
        payload,
    })?;
    let value = decode_authority_response(response)?;
    command.verify_response(project, &value).map_err(|_| {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "version catalog reply differs from requested scope or immutable identity",
        )
    })?;
    Ok(value)
}
pub(crate) fn decode_authority_response(
    response: crate::TransportResponse,
) -> Result<Value, ClientError> {
    if response.body.len() > 64 * 1024 * 1024 {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "version authority reply exceeds64MiB",
        ));
    }
    if response.status == 200 {
        return super::design_versions::decode(response);
    }
    let status = response.status;
    let error: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
    let refusal = if error["error"].is_object() {
        &error["error"]
    } else {
        &error
    };
    let code = refusal["code"].as_str().map(str::to_ascii_lowercase);
    let classified = crate::authorized::classify_tile_error(status);
    Err(ClientError::with_service_refusal(
        classified.kind(),
        "version authority refused the request",
        crate::ServiceRefusal::with_details(
            status,
            code.as_deref(),
            refusal["message"].as_str(),
            refusal["details"].as_object(),
        ),
    ))
}
