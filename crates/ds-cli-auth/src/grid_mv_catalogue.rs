//! Consuming current-project head metadata through the native retained owner.
use super::*;
use ds_command_kernel::printing::retained_reads::Demand;
use ds_layer_store::grid_project_catalogue::{self as cache, Scope};

pub fn read(
    lane: &str,
    project: &str,
    demand: Demand,
    inventory: Option<ds_command_kernel::grid_held_models::Inventory>,
    source: Option<ds_command_kernel::printing::mv_models::SourceBinding>,
) -> Result<HeadlessNamedProject<Value>, Failure> {
    let captured = headless_named_project_with(
        lane,
        project,
        map_grid_publication,
        |device, project| {
            let scope = Scope {
                principal: device.context().uid().into(),
                project: project.into(),
                lane: lane.into(),
                audience_sha256: device.profile().credential_audience_sha256().into(),
            };
            Ok(cached(
                scope,
                demand,
                inventory.clone(),
                source.clone(),
                || {
                    ds_client_core::grid_models::catalogue::collect(project, |cursor| {
                        device.grid_models(
                            project,
                            &GridModelsCommand::List {
                                limit: 100,
                                cursor,
                                include_deleted: false,
                            },
                        )
                    })
                },
            ))
        },
        |client, project| {
            let Some(user) = client.user() else {
                return Ok(Err(cache::Failure::AccessRevoked));
            };
            let scope = Scope {
                principal: user.uid().into(),
                project: project.into(),
                lane: lane.into(),
                audience_sha256: client.profile().credential_audience_sha256().into(),
            };
            Ok(cached(
                scope,
                demand,
                inventory.clone(),
                source.clone(),
                || {
                    ds_client_core::grid_models::catalogue::collect(project, |cursor| {
                        client.grid_models(
                            project,
                            &GridModelsCommand::List {
                                limit: 100,
                                cursor,
                                include_deleted: false,
                            },
                            now(),
                        )
                    })
                },
            ))
        },
    )?;
    let result = captured.result.map_err(map_error)?;
    Ok(HeadlessNamedProject {
        identity: captured.identity,
        user_email: captured.user_email,
        lane: captured.lane,
        project_id: captured.project_id,
        result,
    })
}
fn unauthorized(error: &ClientError) -> bool {
    matches!(
        error.kind(),
        ErrorKind::SignedOut
            | ErrorKind::InvalidCredentials
            | ErrorKind::IdentityMismatch
            | ErrorKind::AuthenticationRejected
            | ErrorKind::PermanentlyRevoked
            | ErrorKind::AccountDisabled
    )
}
fn cached(
    scope: Scope,
    demand: Demand,
    inventory: Option<ds_command_kernel::grid_held_models::Inventory>,
    source: Option<ds_command_kernel::printing::mv_models::SourceBinding>,
    load: impl FnOnce() -> Result<Value, ClientError>,
) -> Result<Value, cache::Failure<ClientError>> {
    let root = ds_layer_store::default_root().map_err(cache::Failure::Storage)?;
    cache::consume(&root, &scope, demand, inventory, source, load, unauthorized)
}
fn map_error(error: cache::Failure<ClientError>) -> Failure {
    match error {
        // The kernel admission codes, named so their refusals stay declared.
        cache::Failure::Admission(code) => match code {
            "grid_catalog_version_stale" => Failure::conflict("grid_catalog_version_stale", code),
            "mv_model_source_stale" => Failure::conflict("mv_model_source_stale", code),
            _ => Failure::failed("mv_model_catalogue_invalid", code),
        },
        cache::Failure::Remote(error) => map_grid_publication(error),
        cache::Failure::AcquisitionRequired => Failure::unavailable(
            "project_model_catalogue_acquisition_required",
            "The project model catalogue requires intentional connected acquisition.",
        )
        .remedy("repeat this consuming read while connected"),
        cache::Failure::AccessRevoked => Failure::unauthorized(
            "auth_context_mismatch",
            "The project catalogue's access was refused. Deliberately refresh after authorization is restored.",
        ),
        cache::Failure::Storage(message) => Failure::failed("mv_model_catalogue_invalid", message),
    }
}

/// What a caller asks of the retained catalogue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogRead {
    /// This machine's Kernel SQLite rows only: no transport, no credential refresh.
    Held,
    /// One authorized listing and the Kernel's source-basis comparison.
    Refresh,
}

/// The project's retained model catalogue (`ds.grid-project-catalog/v1`), the
/// same projection the browser computes over its own SQLite. A head seen for
/// the first time is persisted at once; a failed refresh returns the held rows
/// labelled, with `refresh_error` naming the owning refusal. A revoked
/// catalogue is never served from held rows.
pub fn catalog(
    lane: &str,
    project: &str,
    read: CatalogRead,
) -> Result<(ProviderIdentity, Value), Failure> {
    let root = ds_layer_store::default_root().map_err(storage)?;
    if read == CatalogRead::Held {
        return held_catalog(lane, project, |_, _| Ok(()));
    }
    let refresh = Demand {
        online: true,
        acquire: true,
        refresh: true,
    };
    let captured = headless_named_project_with(
        lane,
        project,
        map_grid_publication,
        |device, project| {
            let scope = Scope {
                principal: device.context().uid().into(),
                project: project.into(),
                lane: lane.into(),
                audience_sha256: device.profile().credential_audience_sha256().into(),
            };
            Ok(cache::catalog(
                &root,
                &scope,
                refresh,
                || {
                    ds_client_core::grid_models::catalogue::collect(project, |cursor| {
                        device.grid_models(
                            project,
                            &GridModelsCommand::List {
                                limit: 100,
                                cursor,
                                include_deleted: false,
                            },
                        )
                    })
                },
                unauthorized,
            ))
        },
        |client, project| {
            let Some(user) = client.user() else {
                return Ok(Err(cache::Failure::AccessRevoked));
            };
            let scope = Scope {
                principal: user.uid().into(),
                project: project.into(),
                lane: lane.into(),
                audience_sha256: client.profile().credential_audience_sha256().into(),
            };
            Ok(cache::catalog(
                &root,
                &scope,
                refresh,
                || {
                    ds_client_core::grid_models::catalogue::collect(project, |cursor| {
                        client.grid_models(
                            project,
                            &GridModelsCommand::List {
                                limit: 100,
                                cursor,
                                include_deleted: false,
                            },
                            now(),
                        )
                    })
                },
                unauthorized,
            ))
        },
    )?;
    let identity = captured.identity.clone();
    let read = captured.result.map_err(map_error)?;
    let mut catalog = read.catalog;
    if let Some(failure) = read.refresh_failure {
        let failure = map_error(failure);
        catalog["refresh_error"] = json!({"code": failure.code(), "message": failure.message()});
    }
    Ok((identity, catalog))
}

/// Record, offline, the exact version whose verified bytes this host's bytes
/// owner holds for one held head; answers the held catalogue after it.
pub fn retain_catalog_version(
    lane: &str,
    project: &str,
    model: &str,
    version: Value,
) -> Result<(ProviderIdentity, Value), Failure> {
    held_catalog(lane, project, |root, scope| {
        cache::retain_version(root, scope, model, version)
            .map(|_| ())
            .map_err(|error| match error {
                cache::Failure::Admission("grid_catalog_invalid") => Failure::failed(
                    "mv_model_catalogue_invalid",
                    "The retained catalogue refused the version as invalid.",
                ),
                cache::Failure::Admission(_) => Failure::conflict(
                    "grid_catalog_version_stale",
                    "The retained version does not name the held head of that model.",
                )
                .remedy("refresh the catalogue, then retain the exact current head version"),
                cache::Failure::AccessRevoked => revoked(),
                cache::Failure::Storage(message) => storage(message),
                cache::Failure::AcquisitionRequired | cache::Failure::Remote(()) => {
                    storage("the held catalogue could not record the retained version")
                }
            })
    })
}

fn held_catalog(
    lane: &str,
    project: &str,
    before: impl FnOnce(&std::path::Path, &Scope) -> Result<(), Failure>,
) -> Result<(ProviderIdentity, Value), Failure> {
    let root = ds_layer_store::default_root().map_err(storage)?;
    let project = bounded_named_project(project)?;
    let identity = headless_identity_for_named_project(lane)?;
    let scope = Scope {
        principal: identity.uid().into(),
        project: project.clone(),
        lane: identity.lane().into(),
        audience_sha256: identity.credential_audience_sha256().into(),
    };
    before(&root, &scope)?;
    let held = cache::catalog(
        &root,
        &scope,
        Demand {
            online: false,
            acquire: false,
            refresh: false,
        },
        || -> Result<Value, ClientError> { unreachable!("held reads never acquire") },
        |_| false,
    )
    .map_err(map_error)?;
    if headless_identity_for_named_project(lane)? != identity {
        return Err(Failure::failed(
            "project_context_changed",
            "The account changed while reading the project model catalogue.",
        ));
    }
    if held.catalog["observation"]["state"] == "revoked" {
        return Err(revoked());
    }
    Ok((identity, held.catalog))
}
fn storage(message: impl Into<String>) -> Failure {
    Failure::failed("mv_model_catalogue_invalid", message.into())
        .remedy("refresh the project model catalogue; report the store error if it repeats")
}
fn revoked() -> Failure {
    Failure::unauthorized(
        "auth_context_mismatch",
        "The project catalogue's access was refused. Deliberately refresh after authorization is restored.",
    )
    .remedy("deliberately refresh after authorization is restored")
}
