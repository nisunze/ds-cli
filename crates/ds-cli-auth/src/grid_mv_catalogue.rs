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
        cache::Failure::Admission(code) => Failure::conflict(code, code),
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
