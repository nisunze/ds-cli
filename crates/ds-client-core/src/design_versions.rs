//! Authenticated transport adapter only. The complete version workflow is kernel-owned.
use crate::{
    authorized::{self, Authority},
    AuthorizedTransport, ClientError, ClientProfile, DesignVersionsCall, ErrorKind,
    TransportResponse,
};
use ds_command_kernel::design_versions_headless::{self as owner, Source};
pub use owner::Command;
use serde_json::{json, Value};
fn bad(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::UnreadableResponse, message)
}
fn decode(response: TransportResponse) -> Result<Value, ClientError> {
    if response.body.len() > 64 * 1024 * 1024 {
        return Err(bad("version response exceeds 64 MiB"));
    }
    if response.status != 200 {
        return Err(authorized::classify_tile_error(response.status));
    }
    let value: Value =
        serde_json::from_slice(&response.body).map_err(|_| bad("unreadable version response"))?;
    if value["success"] != true || !value["data"].is_object() {
        return Err(bad("missing version response data"));
    }
    Ok(value["data"].clone())
}
fn fetch<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    transformer: &str,
    id: Option<&str>,
) -> Result<Value, ClientError> {
    let mut payload = json!({"action":if id.is_some() {"get_version"} else {"list_versions"},
        "project_id":project,"object":{"kind":"lv_transformer","id":transformer}});
    if let Some(id) = id {
        payload["version_id"] = json!(id);
    }
    decode(transport.design_versions(DesignVersionsCall {
        profile,
        authority: authority.call(),
        payload,
    })?)
}

struct AuthenticatedSource<'a, T> {
    profile: &'a ClientProfile,
    transport: &'a mut T,
    authority: Authority<'a>,
}
impl<T: AuthorizedTransport> Source for AuthenticatedSource<'_, T> {
    type Error = ClientError;
    fn versions(
        &mut self,
        project: &str,
        transformer: &str,
        version: Option<&str>,
    ) -> Result<Value, ClientError> {
        fetch(
            self.profile,
            self.transport,
            self.authority,
            project,
            transformer,
            version,
        )
    }
    fn head(&mut self, project: &str, transformer: &str) -> Result<Value, ClientError> {
        let observed = authorized::transformer_context(
            self.profile,
            self.transport,
            self.authority,
            project,
            transformer,
        )?;
        Ok(
            json!({"project":observed.ds_project(),"transformer":observed.transformer_name(),
            "snapshot":{"schema":"ds.transformer-version-snapshot/v1","layers":observed.layers()},
            "server_version":observed.metadata().version(),"content_digest":observed.metadata().content_digest()}),
        )
    }
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Value, ClientError> {
    let scope = ds_command_kernel::design_versions::Scope {
        owner: authority.uid.into(),
        audience: profile.credential_audience_sha256().into(),
        lane: profile.lane().token().into(),
        project: project.into(),
        object_kind: "lv_transformer".into(),
        object_id: command.transformer().into(),
    };
    owner::run(
        &scope,
        &mut AuthenticatedSource {
            profile,
            transport,
            authority,
        },
        command,
    )
    .map_err(|e| match e {
        owner::Error::Source(e) => e,
        owner::Error::Invalid(message) if message.starts_with("playback_unavailable") => {
            ClientError::new(
                ErrorKind::InvalidInput,
                "playback_unavailable: choose a published version with playback_available=true",
            )
        }
        owner::Error::Invalid(_) => {
            bad("kernel refused version identity, snapshot integrity or comparison inputs")
        }
    })
}
