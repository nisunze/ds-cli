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
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

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
            .host(parent.join("solar-application").join(identity))?;
        ds_solar_native::application::execute(host, &context.project, &body)
            .map(Json)
            .map_err(|e| Failure::invalid("server_refused", e))
    })
    .await
}
