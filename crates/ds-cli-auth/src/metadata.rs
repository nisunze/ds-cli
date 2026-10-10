//! Host adapter for the core's last-observed project metadata. These local
//! views never authorize a write, publication or remote artifact download.
use super::*;
use ds_project_data::metadata::{self as store, Kind, Scope};

fn scope(identity: &ProviderIdentity, project: &str) -> Scope {
    Scope {
        principal: identity.uid().into(),
        lane: identity.lane().into(),
        audience: identity.credential_audience_sha256().into(),
        project: project.into(),
    }
}
fn failed(message: String) -> Failure {
    Failure::failed("auth_response_unreadable", message)
        .remedy("refresh the project metadata and retry")
}
pub(super) fn invalidate(
    identity: &ProviderIdentity,
    project: &str,
    kind: Kind,
) -> Result<(), Failure> {
    store::invalidate(
        &ds_layer_store::default_root().map_err(failed)?,
        &scope(identity, project),
        kind,
    )
    .map_err(failed)
}
pub(super) fn invalidate_styles(identity: &ProviderIdentity) -> Result<(), Failure> {
    store::invalidate_all_styles(
        &ds_layer_store::default_root().map_err(failed)?,
        &scope(identity, "all"),
    )
    .map_err(failed)
}
fn acquired(
    lane: &str,
    project: &str,
    kind: Kind,
    refresh: bool,
    fetch: impl FnOnce(&ProviderIdentity) -> Result<Value, Failure>,
) -> Result<(ProviderIdentity, Value, bool), Failure> {
    let project = bounded_named_project(project)?;
    let identity = headless_identity_for_named_project(lane)?;
    let (value, cached) = store::acquire(
        &ds_layer_store::default_root().map_err(failed)?,
        &scope(&identity, &project),
        kind,
        refresh,
        || {
            let value = fetch(&identity)?;
            if headless_identity_for_named_project(lane)? != identity {
                return Err(failed(
                    "account changed while observing project metadata".into(),
                ));
            }
            Ok(value)
        },
    )
    .map_err(|error| match error {
        store::Error::Read(error) => error,
        store::Error::Store(error) => failed(error),
    })?;
    if headless_identity_for_named_project(lane)? != identity {
        return Err(failed(
            "account changed while reading project metadata".into(),
        ));
    }
    Ok((identity, value, cached))
}

/// Read a captured style catalogue; cold reads and explicit Refresh use the
/// authenticated API. Navigation/style authoring use `style_catalog` to replace it.
pub fn observed_style_catalog(
    lane: &str,
    project: &str,
    refresh: bool,
) -> Result<(HeadlessStyleSnapshot, bool), Failure> {
    let (_, document, cached) = acquired(lane, project, Kind::Styles, refresh, |expected| {
        let named = headless_named_project(
            lane,
            project,
            |device, project| device.style_catalog(project),
            |client, project| client.style_catalog(project, now()),
        )?;
        if named.identity() != expected {
            return Err(failed("style observation account changed".into()));
        }
        Ok(named.result.document().clone())
    })?;
    let result = StyleSnapshot::from_observation(project, document).map_err(map_client)?;
    Ok((
        HeadlessStyleSnapshot {
            lane: Lane::parse(lane)?.token(),
            project_id: project.into(),
            project_name: String::new(),
            project_status: String::new(),
            result,
        },
        cached,
    ))
}

/// Use already observed heads for read-only consumers. This never claims to
/// have observed a remote edit that has not arrived through Refresh/navigation.
pub fn observed_transformer_status_for_project(
    lane: &str,
    project: &str,
    refresh: bool,
) -> Result<(HeadlessNamedProject<TransformerStatusList>, bool), Failure> {
    let (identity, document, cached) = acquired(
        lane,
        project,
        Kind::TransformerStatus,
        refresh,
        |expected| {
            let requested =
                TransformerSet::new(std::iter::empty::<String>()).map_err(map_client)?;
            let named = headless_named_project(
                lane,
                project,
                |device, project| device.transformer_status(project, &requested),
                |client, project| client.transformer_status(project, &requested, now()),
            )?;
            if named.identity() != expected {
                return Err(failed("status observation account changed".into()));
            }
            Ok(
                json!({"user_email":named.user_email,"transformers":named.result.rows().iter().map(|row| row.row()).collect::<Vec<_>>()}),
            )
        },
    )?;
    let result = TransformerStatusList::from_observation(document["transformers"].clone())
        .map_err(map_client)?;
    Ok((
        HeadlessNamedProject {
            identity,
            user_email: document["user_email"].as_str().unwrap_or_default().into(),
            lane: Lane::parse(lane)?.token(),
            project_id: project.into(),
            result,
        },
        cached,
    ))
}
