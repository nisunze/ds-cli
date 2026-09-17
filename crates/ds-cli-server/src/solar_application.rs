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
impl NativeReferences {
    fn execute(&self, command: &ds_cli_auth::SolarProjectCommand) -> Result<Value, String> {
        self.auth.authorize(&self.owner)?;
        let mut held = self
            .session
            .lock()
            .map_err(|_| "Solar acquisition state is poisoned")?;
        if held.is_none() {
            *held = Some(
                ds_cli_auth::solar_project_session_for_project(&self.lane, &self.project)
                    .map_err(|e| e.message().to_owned())?,
            );
        }
        let session = held.as_mut().ok_or("Solar authority is unavailable")?;
        let binding = session.binding();
        if crate::auth::fence(
            binding["uid"].as_str().unwrap_or(""),
            &self.lane,
            binding["audience"].as_str().unwrap_or(""),
        )? != self.owner
        {
            return Err("Server owner changed during Solar acquisition".into());
        }
        let receipt = session
            .execute(command)
            .map_err(|e| e.message().to_owned())?;
        if crate::auth::owner_fence(&self.lane)? != self.owner {
            return Err("Server owner changed during Solar acquisition".into());
        }
        Ok(receipt)
    }
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
        let receipt = self
            .execute(&ds_cli_auth::SolarProjectCommand::Reference {
                request: request.clone(),
            })
            .map_err(invalid)?;
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
impl ds_solar_native::MediaProvider for NativeReferences {
    fn fetch(&self, city: &str, reference: &str) -> Result<Vec<u8>, String> {
        use base64::Engine;
        let receipt = self.execute(&ds_cli_auth::SolarProjectCommand::ReadMedia {
            city: city.into(),
            reference: reference.into(),
        })?;
        if receipt["reference"] != reference
            || receipt["project_id"] != self.project
            || receipt["city_id"] != city
        {
            return Err("The governed image belongs to another Solar context".into());
        }
        let encoded = receipt["body_base64"]
            .as_str()
            .ok_or("The governed image has no verified bytes")?;
        if encoded.len() > (16usize << 20).div_ceil(3) * 4 {
            return Err("The governed image exceeds 16 MiB".into());
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|_| "The governed image has invalid encoding")?;
        if receipt["byte_count"].as_u64() != Some(bytes.len() as u64)
            || receipt["content_digest"] != format!("sha256:{}", ds_compute_runtime::digest(&bytes))
        {
            return Err("The governed image differs from its verified digest".into());
        }
        Ok(bytes)
    }
}

impl ds_solar_native::SnapshotProvider for NativeReferences {
    fn capture(&self, city: &str) -> Result<ds_solar_native::CapturedCitySnapshot, String> {
        self.auth.authorize(&self.owner)?;
        if crate::auth::owner_fence(&self.lane)? != self.owner {
            return Err("Server owner changed during Solar input capture".into());
        }
        let snapshot = ds_cli_auth::solar_snapshot_for_project(&self.lane, &self.project, city)
            .map_err(|e| e.message().to_owned())?;
        if snapshot.ds_project() != self.project
            || snapshot.template_id() != city
            || crate::auth::owner_fence(&self.lane)? != self.owner
        {
            return Err("Server owner or project changed during Solar input capture".into());
        }
        Ok(ds_solar_native::CapturedCitySnapshot {
            snapshot_json: snapshot.document_json().into(),
            input_base_fingerprint: snapshot.input_base_fingerprint().into(),
            captured_at: snapshot.firestore_read_time().into(),
        })
    }
}

impl ds_solar_native::PortfolioPublicationProvider for NativeReferences {
    fn publish(
        &self,
        result: Vec<u8>,
        outputs: Vec<ds_solar_native::PortfolioPublicationFile>,
    ) -> Result<Value, String> {
        let outputs = outputs
            .into_iter()
            .map(|file| ds_cli_auth::SolarProjectOutput {
                id: file.declaration.output_id,
                format: file.declaration.format,
                content_type: file.declaration.content_type,
                bytes: file.bytes,
            })
            .collect();
        self.execute(&ds_cli_auth::SolarProjectCommand::Portfolio(
            ds_cli_auth::SolarPortfolioCommand::Publish { result, outputs },
        ))
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
        let producer = Arc::new(NativeReferences {
            lane: app.connection.lane.clone(),
            project: context.project.clone(),
            owner: app.connection.owner.clone(),
            auth: app.auth.clone(),
            session: Mutex::new(None),
        });
        let application_directory = parent.join("solar-application").join(identity);
        let renderer = Arc::new(crate::solar_documents::NativeDocuments {
            directory: application_directory.join("document-tools"),
            owner: app.connection.owner.clone(),
            lane: app.connection.lane.clone(),
            auth: app.auth.clone(),
        });
        let host = app
            .solar
            .host(application_directory)?
            .with_report_renderer(renderer)
            .with_reference_provider(producer.clone())
            .with_media_provider(producer.clone())
            .with_portfolio_publication_provider(producer.clone())
            .with_snapshot_provider(producer);
        ds_solar_native::application::execute(host, &context.project, &body)
            .map(Json)
            .map_err(|e| Failure::invalid("server_refused", e))
    })
    .await
}
