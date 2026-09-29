//! Authenticated transport adapter only. The complete version workflow is kernel-owned.
use crate::{
    authorized::{self, Authority},
    AuthorizedTransport, ClientError, ClientProfile, DesignVersionsCall, ErrorKind, ServiceRefusal,
    TransportResponse,
};
use ds_command_kernel::design_versions_headless::{
    self as owner, BatchItem, Create, ObjectRef, Shared, Source,
};
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
        if response.status == 404 {
            return Err(ClientError::new(
                ErrorKind::ResourceNotFound,
                "the requested published design version does not exist",
            ));
        }
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
    kind: &str,
) -> Result<Value, ClientError> {
    let mut payload = json!({"action":if id.is_some() {"get_version"} else {"list_versions"},
        "project_id":project,"object":{"kind":kind,"id":transformer}});
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
    kind: &'a str,
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
            self.kind,
        )
    }
    fn head(&mut self, project: &str, transformer: &str) -> Result<Value, ClientError> {
        let observed = authorized::transformer_context(
            self.profile,
            self.transport,
            self.authority,
            project,
            transformer,
            crate::TransformerContextProjection::Full,
        )?;
        Ok(
            json!({"project":observed.ds_project(),"transformer":observed.transformer_name(),
            "snapshot":{"schema":"ds.transformer-version-snapshot/v1","layers":observed.layers()},
            "server_version":observed.metadata().version(),"content_digest":observed.metadata().content_digest()}),
        )
    }
    fn create(
        &mut self,
        project: &str,
        transformer: &str,
        create: &Create,
    ) -> Result<Value, ClientError> {
        let mut version = json!({
            "reason": create.reason,
            "idempotency_key": create.idempotency_key,
        });
        // Optional fields are omitted, never sent empty: the request a caller
        // who names neither sends is byte-identical to before.
        if let Some(milestone) = &create.milestone {
            version["milestone"] = json!(milestone);
        }
        if let Some(expected) = &create.expected_source {
            version["expected_source_revision"] = json!(expected);
        }
        let payload = json!({
            "action": "create_version",
            "project_id": project,
            "object": {"kind": self.kind, "id": transformer},
            "version": version,
        });
        decode(self.transport.design_versions(DesignVersionsCall {
            profile: self.profile,
            authority: self.authority.call(),
            payload,
        })?)
    }
    fn create_many(
        &mut self,
        project: &str,
        items: &[BatchItem],
        shared: &Shared,
    ) -> Result<Value, ClientError> {
        let mut payload = json!({
            "action": "create_versions",
            "project_id": project,
            "versions": items,
        });
        if shared.reason.is_some() || shared.milestone.is_some() {
            payload["version"] = json!(shared);
        }
        decode(self.transport.design_versions(DesignVersionsCall {
            profile: self.profile,
            authority: self.authority.call(),
            payload,
        })?)
    }
    fn summaries(&mut self, project: &str, objects: &[ObjectRef]) -> Result<Value, ClientError> {
        let payload = json!({
            "action": "list_version_summaries",
            "project_id": project,
            "objects": objects,
        });
        decode(self.transport.design_versions(DesignVersionsCall {
            profile: self.profile,
            authority: self.authority.call(),
            payload,
        })?)
    }
    fn current(&mut self, project: &str, transformer: &str) -> Result<Value, ClientError> {
        let payload = json!({
            "action": "get_head",
            "project_id": project,
            "object": {"kind": self.kind, "id": transformer},
        });
        decode(self.transport.design_versions(DesignVersionsCall {
            profile: self.profile,
            authority: self.authority.call(),
            payload,
        })?)
    }
    fn restore(
        &mut self,
        project: &str,
        transformer: &str,
        version: &str,
        reason: &str,
        expected_source_revision: &str,
        idempotency_key: &str,
    ) -> Result<Value, ClientError> {
        let payload = json!({
            "action": "restore_version",
            "project_id": project,
            "object": {"kind": self.kind, "id": transformer},
            "version_id": version,
            "version": {
                "reason": reason,
                "expected_source_revision": expected_source_revision,
                "idempotency_key": idempotency_key,
            },
        });
        decode(self.transport.design_versions(DesignVersionsCall {
            profile: self.profile,
            authority: self.authority.call(),
            payload,
        })?)
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
        object_kind: command.kind().into(),
        object_id: command.transformer().into(),
    };
    owner::run(
        &scope,
        &mut AuthenticatedSource {
            profile,
            transport,
            authority,
            kind: command.kind(),
        },
        command,
    )
    .map_err(|e| match e {
        owner::Error::Source(e) => e,
        owner::Error::VersionNotFound {
            transformer,
            requested,
            latest,
        } => {
            let message = format!(
                "transformer {transformer} has no published {requested}; latest is v{latest}"
            );
            ClientError::with_service_refusal(
                ErrorKind::ResourceNotFound,
                "the requested published design version does not exist",
                ServiceRefusal::new(404, Some("version_not_found"), Some(&message)),
            )
        }
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
