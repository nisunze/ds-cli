//! Fixed native transport for the pure catalog wire contract.
use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, DesignVersionsCall,
    ErrorKind,
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
    if let Command::Compare { object_id, request } = command {
        let scope = ds_command_kernel::design_versions::Scope {
            owner: authority.uid.into(),
            audience: profile.credential_audience_sha256().into(),
            lane: profile.lane().token().into(),
            project: project.into(),
            object_kind: "lv_transformer".into(),
            object_id: object_id.clone(),
        };
        return ds_command_kernel::design_revision_comparison::run(
            &scope,
            &mut RevisionSource {
                profile,
                transport,
                authority,
            },
            request,
        )
        .map_err(|e| match e {
            ds_command_kernel::design_revision_comparison::Error::Source(e) => e,
            ds_command_kernel::design_revision_comparison::Error::Invalid(message) => {
                ClientError::with_service_refusal(
                    ErrorKind::InvalidInput,
                    "immutable revision comparison refused",
                    crate::ServiceRefusal::new(
                        400,
                        Some("revision_comparison_refused"),
                        Some(&message),
                    ),
                )
            }
        });
    }
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

struct RevisionSource<'a, T> {
    profile: &'a ClientProfile,
    transport: &'a mut T,
    authority: Authority<'a>,
}
impl<T: AuthorizedTransport> ds_command_kernel::design_revision_comparison::Source
    for RevisionSource<'_, T>
{
    type Error = ClientError;
    fn read(
        &mut self,
        project: &str,
        object: &str,
        pin: &ds_command_kernel::design_revision_comparison::Pin,
    ) -> Result<Value, ClientError> {
        let command = Command::Snapshot {
            object_id: object.into(),
            version_id: pin.version_id.clone(),
            revision_id: pin.revision_id.clone(),
        };
        let payload = command.payload(project).map_err(|_| {
            ClientError::new(ErrorKind::InvalidInput, "invalid exact immutable read")
        })?;
        // The pure consuming workflow validates scope/digests and decodes once.
        decode_authority_response(self.transport.design_versions(DesignVersionsCall {
            profile: self.profile,
            authority: self.authority.call(),
            payload,
        })?)
    }
}
