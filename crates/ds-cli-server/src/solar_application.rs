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
    database: PathBuf,
    identity: ds_compute_runtime::HostIdentity,
    activity: Option<Arc<crate::solar_sync::SolarActivity>>,
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
            publication: Some(ds_solar_native::CapturedPublication {
                source_snapshot_sha256: snapshot.snapshot_sha256().into(),
                snapshot_receipt_id: snapshot.snapshot_receipt_id().into(),
                snapshot_receipt_expires_at: snapshot.snapshot_receipt_expires_at().into(),
            }),
        })
    }
}

impl ds_solar_native::PublicationProvider for NativeReferences {
    fn retain(&self, publication: ds_solar_native::sync::Publication) -> Result<Value, String> {
        use ds_sync_runtime::Producer;
        self.auth.authorize(&self.owner)?;
        if publication.project != self.project || self.identity.owner != self.owner {
            return Err("Solar publication crosses its captured owner or project".into());
        }
        let store = ds_sync_runtime::open_store(&self.database)?;
        let fence = ds_sync_runtime::solar_producer::fence_of(&self.identity);
        let producer = ds_sync_runtime::solar_producer::SolarProducer {
            database: &self.database,
            identity: &self.identity,
            project: self.project.clone(),
            failure: None,
        };
        let receipt = ds_solar_native::sync::retain(
            &store,
            &fence,
            publication,
            &|previous| {
                producer.reclaim(
                    &self.project,
                    &ds_sync_runtime::rows::local_row_from_store(previous),
                )
            },
        )?;
        if let Some(activity) = &self.activity {
            activity.local_publication_completed();
        }
        self.auth.authorize(&self.owner)?;
        Ok(receipt)
    }
    fn observe(&self, publication: &ds_solar_native::sync::Publication) -> Result<Value, String> {
        self.auth.authorize(&self.owner)?;
        if publication.project != self.project || self.identity.owner != self.owner {
            return Err("Solar publication crosses its captured owner or project".into());
        }
        let store = ds_sync_runtime::open_store(&self.database)?;
        let receipt = ds_solar_native::sync::recorded(
            &store,
            &ds_sync_runtime::solar_producer::fence_of(&self.identity),
            publication,
        )?;
        if let Some(activity) = &self.activity {
            activity.local_publication_completed();
        }
        Ok(receipt)
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
            database: app.database.clone(),
            identity: app.sessions.identity().clone(),
            activity: app.activity.clone(),
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
            .with_publication_provider(producer.clone())
            .with_snapshot_provider(producer);
        ds_solar_native::application::execute(host, &context.project, &body)
            .map(Json)
            .map_err(|e| Failure::invalid("server_refused", e))
    })
    .await
}

#[cfg(test)]
mod publication_tests {
    use super::*;
    use ds_solar_native::PublicationProvider;
    use ds_sync_runtime::Producer;
    use std::sync::atomic::{AtomicBool, Ordering};
    struct LocalAuth(Arc<AtomicBool>);
    impl ds_compute_runtime::Authorizer for LocalAuth {
        fn authorize(&self, owner: &str) -> Result<(), String> {
            if owner == "owner-a" && self.0.load(Ordering::SeqCst) {
                Ok(())
            } else {
                Err("owner changed".into())
            }
        }
    }
    fn adapter(path: PathBuf, authorized: Arc<AtomicBool>) -> NativeReferences {
        NativeReferences {
            lane: "canary".into(),
            project: "project-a".into(),
            owner: "owner-a".into(),
            auth: Arc::new(LocalAuth(authorized)),
            database: path,
            identity: ds_compute_runtime::HostIdentity {
                owner: "owner-a".into(),
                principal: ds_command_kernel::execution_context::Principal {
                    uid: "account-a".into(),
                    lane: "canary".into(),
                    deployment: "https://gateway.example".into(),
                    install_id: "install-a".into(),
                },
            },
            activity: None,
            session: Mutex::new(None),
        }
    }
    fn publication(project: &str, build: &str) -> ds_solar_native::sync::Publication {
        let bytes = b"# sealed Solar draft".to_vec();
        let output = ds_sync_runtime::LocalOutput {
            filename: None,
            paper_size: None,
            presentation: None,
            output_id: "network-draft-en".into(),
            format: "md".into(),
            content_type: "text/markdown".into(),
            sha256: ds_compute_runtime::digest(&bytes),
            size_bytes: bytes.len() as u64,
        };
        ds_solar_native::sync::close(
            ds_solar_native::sync::Source {
                project,
                run: "report-run-a",
                operation: "report-city-a",
                variant: "network-draft-en",
                resource: "city-a",
                release: "ds-solar-engine@1.2.3",
                manifest: build,
                input_base: &"b".repeat(64),
            },
            vec![(output, bytes)],
        )
        .unwrap()
    }
    #[test]
    fn server_retains_offline_and_replays_after_restart_without_opening_a_gateway_session() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.sqlite");
        let authorized = Arc::new(AtomicBool::new(true));
        let host = adapter(path.clone(), authorized.clone());
        assert!(
            host.observe(&publication("project-a", &"a".repeat(64)))
                .unwrap_err()
                .contains("publication_not_sealed")
        );
        let receipt = host
            .retain(publication("project-a", &"a".repeat(64)))
            .unwrap();
        assert_eq!(receipt["state"], "held");
        assert!(host.session.lock().unwrap().is_none());
        drop(host);
        let host = adapter(path, authorized);
        let retried = host
            .retain(publication("project-a", &"a".repeat(64)))
            .unwrap();
        assert_eq!(receipt, retried);
        assert_eq!(
            receipt,
            host.observe(&publication("project-a", &"a".repeat(64)))
                .unwrap()
        );
        assert!(host.session.lock().unwrap().is_none());
        let store = ds_sync_runtime::open_store(&host.database).unwrap();
        let fence = ds_sync_runtime::solar_producer::fence_of(&host.identity);
        let rows = store
            .lock()
            .unwrap()
            .snapshot(&fence, &ds_sync_runtime::rows::store_scope("project-a"))
            .unwrap()
            .artifacts;
        assert_eq!(rows.len(), 1);
        let row = ds_sync_runtime::rows::local_row_from_store(&rows[0]);
        assert_eq!(
            ds_sync_runtime::retained_publication::RetainedProducer {
                store: &store,
                fence: &fence,
                project: "project-a"
            }
            .output(&row, "network-draft-en")
            .unwrap(),
            b"# sealed Solar draft"
        );
        let mut declaration = ds_sync_runtime::rows::open_declaration("project-a", &row);
        ds_sync_runtime::solar_producer::SolarProducer {
            database: &host.database,
            identity: &host.identity,
            project: "project-a".into(),
            failure: None,
        }
        .amend_open_declaration("project-a", &mut declaration)
        .unwrap();
    }
    #[test]
    fn refused_owner_project_or_corrupted_payload_cannot_create_a_publication() {
        let dir = tempfile::tempdir().unwrap();
        let authorized = Arc::new(AtomicBool::new(false));
        let host = adapter(dir.path().join("server.sqlite"), authorized.clone());
        assert!(
            host.retain(publication("project-a", &"a".repeat(64)))
                .is_err()
        );
        assert!(!host.database.exists());
        authorized.store(true, Ordering::SeqCst);
        assert!(
            host.retain(publication("project-b", &"a".repeat(64)))
                .is_err()
        );
        assert!(!host.database.exists());
        let mut invalid = publication("project-a", &"a".repeat(64));
        invalid.outputs[0].bytes[0] = b'!';
        assert!(host.retain(invalid).is_err());
        let store = ds_sync_runtime::open_store(&host.database).unwrap();
        assert!(
            store
                .lock()
                .unwrap()
                .snapshot(
                    &ds_sync_runtime::solar_producer::fence_of(&host.identity),
                    &ds_sync_runtime::rows::store_scope("project-a")
                )
                .unwrap()
                .artifacts
                .is_empty()
        );
        assert!(host.session.lock().unwrap().is_none());
    }
    #[test]
    fn identical_bytes_from_a_new_build_keep_their_new_provenance_in_the_common_queue() {
        let dir = tempfile::tempdir().unwrap();
        let host = adapter(
            dir.path().join("server.sqlite"),
            Arc::new(AtomicBool::new(true)),
        );
        let old = host
            .retain(publication("project-a", &"a".repeat(64)))
            .unwrap();
        let current = host
            .retain(publication("project-a", &"c".repeat(64)))
            .unwrap();
        assert_ne!(old["client_publish_id"], current["client_publish_id"]);
        let store = ds_sync_runtime::open_store(&host.database).unwrap();
        let fence = ds_sync_runtime::solar_producer::fence_of(&host.identity);
        let rows = store
            .lock()
            .unwrap()
            .snapshot(&fence, &ds_sync_runtime::rows::store_scope("project-a"))
            .unwrap()
            .artifacts;
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].engine_build_manifest_sha256.as_deref(),
            Some("c".repeat(64).as_str())
        );
        assert!(
            store
                .lock()
                .unwrap()
                .publication_payload(
                    &fence,
                    "project-a",
                    old["client_publish_id"].as_str().unwrap()
                )
                .unwrap()
                .is_none()
        );
        assert!(host.session.lock().unwrap().is_none());
    }
}
