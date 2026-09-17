//! Server effect adapter for the Solar-owned native application. Each
//! authorized owner/project has an independent state and workspace authority.
use crate::host::{App, ProjectQuery};
use axum::{
    Json,
    body::Bytes,
    extract::{Query, State},
    http::StatusCode,
};
use ds_cli_contract::Failure;
use ds_solar_native::NativeHost;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

/// Acquisition runs only on a cache miss. It holds captured native authority,
/// accepts no caller credentials and checks the Server owner before each fetch.
struct NativeReferences {
    lane: String,
    project: String,
    owner: String,
    auth: Arc<dyn ds_compute_runtime::Authorizer>,
    session: Mutex<Option<ds_cli_auth::NamedSolarProjectSession>>,
}
impl ds_solar_native::ReferenceBundleProvider for NativeReferences {
    fn fetch(
        &self,
        request: &ds_solar_native::ReferenceRequest,
    ) -> ds_solar_native::SolarResult<ds_solar_native::BundleBytes> {
        let invalid = |message: String| {
            ds_solar_native::SolarError::new(
                ds_solar_native::SolarErrorCode::ReferenceUnitInvalid,
                message,
            )
        };
        self.auth.authorize(&self.owner).map_err(invalid)?;
        let mut held = self
            .session
            .lock()
            .map_err(|_| invalid("Solar acquisition state is poisoned".into()))?;
        if held.is_none() {
            *held = Some(
                ds_cli_auth::solar_project_session_for_project(&self.lane, &self.project)
                    .map_err(|e| invalid(e.message().into()))?,
            );
        }
        let session = held
            .as_mut()
            .ok_or_else(|| invalid("Solar authority is unavailable".into()))?;
        let binding = session.binding();
        if crate::auth::fence(
            binding["uid"].as_str().unwrap_or(""),
            &self.lane,
            binding["audience"].as_str().unwrap_or(""),
        )
        .map_err(invalid)?
            != self.owner
        {
            return Err(invalid(
                "Server owner changed during Solar acquisition".into(),
            ));
        }
        let receipt = session
            .execute(&ds_cli_auth::SolarProjectCommand::Reference {
                request: request.clone(),
            })
            .map_err(|e| invalid(e.message().into()))?;
        if crate::auth::owner_fence(&self.lane).map_err(invalid)? != self.owner {
            return Err(invalid(
                "Server owner changed during Solar acquisition".into(),
            ));
        }
        if let Some(error) = receipt.get("reference_error") {
            return Err(invalid(
                error["message"]
                    .as_str()
                    .unwrap_or("Solar reference producer refused acquisition")
                    .into(),
            ));
        }
        ds_solar_native::bundle_from_delivery(&self.project, &receipt)
    }
}

#[derive(Default)]
pub struct Applications {
    hosts: Mutex<BTreeMap<PathBuf, NativeHost>>,
}
impl Applications {
    fn host(&self, path: PathBuf) -> Result<NativeHost, Failure> {
        let mut hosts = self.hosts.lock().map_err(|_| {
            Failure::internal("server_refused", "Solar application state is poisoned")
        })?;
        if let Some(host) = hosts.get(&path) {
            return Ok(host.clone());
        }
        if hosts.len() >= 64 {
            return Err(Failure::unavailable(
                "server_refused",
                "This Server already holds 64 Solar application contexts; restart it after active work settles",
            ));
        }
        let host =
            NativeHost::headless(path.clone()).map_err(|e| Failure::failed("server_refused", e))?;
        hosts.insert(path, host.clone());
        Ok(host)
    }
}

pub(crate) async fn invoke(
    State(app): State<App>,
    query: Option<Query<ProjectQuery>>,
    body: Bytes,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project = crate::host::project_query(query)?;
    crate::host::admitting(move || {
        app.auth
            .authorize(&app.connection.owner)
            .map_err(|e| Failure::unauthorized("server_owner_changed", e))?;
        let context = app.sessions.admit_read(
            ds_command_kernel::execution_context::SOLAR_PROCESSING,
            project.as_deref(),
            &body,
            ds_compute_runtime::now_ms(),
        )?;
        let identity = ds_compute_runtime::digest(
            &serde_json::to_vec(&(
                &app.connection.owner,
                &app.connection.lane,
                &context.project,
            ))
            .map_err(|_| {
                Failure::internal("server_refused", "Cannot bind Solar application context")
            })?,
        );
        let parent = app.database.parent().ok_or_else(|| {
            Failure::invalid("server_refused", "Server database has no protected parent")
        })?;
        let host = app
            .solar
            .host(parent.join("solar-application").join(identity))?
            .with_reference_provider(Arc::new(NativeReferences {
                lane: app.connection.lane.clone(),
                project: context.project.clone(),
                owner: app.connection.owner.clone(),
                auth: app.auth.clone(),
                session: Mutex::new(None),
            }));
        ds_solar_native::application::execute(host, &context.project, &body)
            .map(Json)
            .map_err(|e| Failure::invalid("server_refused", e))
    })
    .await
}
