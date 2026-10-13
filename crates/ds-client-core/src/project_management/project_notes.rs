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
) -> Result<Value, ClientError> {
    let payload = command.payload(project).map_err(|_| {
        ClientError::new(ErrorKind::InvalidInput, "Project Notes request is invalid")
    })?;
    let value = super::send(profile, transport, authority, payload)?;
    command.verify_response(project,&value).map_err(|_|ClientError::new(ErrorKind::UnreadableResponse,"Project Notes response differs from its captured project, identities or complete receipt"))?;
    Ok(value)
}
