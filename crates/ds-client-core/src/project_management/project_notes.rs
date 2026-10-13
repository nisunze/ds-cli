//! One fixed existing PM door for the closed shared note owner.
use crate::{AuthorizedTransport, ClientError, ClientProfile, ErrorKind, authorized::Authority};
pub use ds_command_kernel::project_management::project_notes::Command;
use serde_json::Value;
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
    authoring_model: Option<
        &ds_command_kernel::project_management::authoring_model::AuthoringModel,
    >,
) -> Result<Value, ClientError> {
    let mut payload = command.payload(project).map_err(|_| {
        ClientError::new(ErrorKind::InvalidInput, "Project Notes request is invalid")
    })?;
    if let Some(model) = authoring_model {
        payload["authoring_model"] =
            serde_json::to_value(model).expect("validated authoring declaration");
    }
    let value = super::send(profile, transport, authority, payload)?;
    command.verify_response(project,&value).map_err(|_|ClientError::new(ErrorKind::UnreadableResponse,"Project Notes response differs from its captured project, identities or complete receipt"))?;
    Ok(value)
}
