//! Server effect adapter for the Solar-owned native application. Each
//! authorized owner/project has an independent state and workspace authority.
mod output;
pub(crate) use output::command;
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

/// Governed snapshot/media reads hold captured native authority and check the
/// Server owner before each fetch. References use the verified local cache.
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

/// The Server binds the one completion retention its Desktop binds too
/// (`ds_solar_native::sync::CommonRetention`). Its authority is the Server
/// owner, rechecked around every retention against the captured session
/// identity; a retention wakes the Server's publication pump.
fn retention(
    auth: Arc<dyn ds_compute_runtime::Authorizer>,
    owner: String,
    database: PathBuf,
    identity: ds_compute_runtime::HostIdentity,
    project: &str,
    activity: Option<Arc<crate::solar_sync::SolarActivity>>,
) -> ds_solar_native::sync::CommonRetention {
    let captured = identity.owner.clone();
    let retention = ds_solar_native::sync::CommonRetention::new(
        database,
        identity,
        project,
        Arc::new(move || {
            auth.authorize(&owner)?;
            if captured != owner {
                return Err("Solar publication crosses its captured owner or project".into());
            }
            Ok(())
        }),
    );
    match activity {
        Some(activity) => {
            retention.with_completion(Arc::new(move || activity.local_publication_completed()))
        }
        None => retention,
    }
}

/// Closed selection persistence under this existing canonical Store. The
/// callback is the same protected native authority owner used by this endpoint.
struct WorkspaceSelections {
    database: PathBuf,
    scope: ds_sync_store::SolarWorkspaceScope,
    authorize: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
}
impl WorkspaceSelections {
    fn check(&self, project: &str) -> Result<(), String> {
        if project != self.scope.project {
            return Err("Solar workspace selection belongs to another project".into());
        }
        (self.authorize)()
    }
    fn project(
        row: ds_sync_store::SolarWorkspaceSelection,
    ) -> ds_solar_native::workspace_selection::Selection {
        ds_solar_native::workspace_selection::Selection {
            workspace_handle: row.workspace.workspace_handle,
            dir: row.workspace.dir,
            origin: match row.workspace.origin {
                ds_sync_store::SolarWorkspaceOrigin::Approved => {
                    ds_solar_native::workspace_selection::Origin::Approved
                }
                ds_sync_store::SolarWorkspaceOrigin::Default => {
                    ds_solar_native::workspace_selection::Origin::Default
                }
            },
            generation: row.generation,
        }
    }
}
impl ds_solar_native::workspace_selection::Provider for WorkspaceSelections {
    fn read(
        &self,
        project: &str,
    ) -> Result<Option<ds_solar_native::workspace_selection::Selection>, String> {
        self.check(project)?;
        let row = ds_sync_store::Store::open_read_only(&self.database)
            .map_err(|e| e.to_string())?
            .map(|store| store.solar_workspace_selection(&self.scope))
            .transpose()
            .map_err(|e| e.to_string())?
            .flatten();
        self.check(project)?;
        Ok(row.map(Self::project))
    }
    fn compare_set(
        &self,
        project: &str,
        expected_generation: Option<u64>,
        workspace: &ds_solar_native::workspace::NativeSolarWorkspace,
        origin: ds_solar_native::workspace_selection::Origin,
    ) -> Result<ds_solar_native::workspace_selection::Selection, String> {
        self.check(project)?;
        let choice = ds_sync_store::SolarWorkspaceChoice {
            workspace_handle: workspace.workspace_handle.clone(),
            dir: workspace.dir.clone(),
            origin: match origin {
                ds_solar_native::workspace_selection::Origin::Approved => {
                    ds_sync_store::SolarWorkspaceOrigin::Approved
                }
                ds_solar_native::workspace_selection::Origin::Default => {
                    ds_sync_store::SolarWorkspaceOrigin::Default
                }
            },
        };
        let row = ds_sync_store::Store::open(&self.database)
            .map_err(|e| e.to_string())?
            .set_solar_workspace_selection(&self.scope, &choice, expected_generation)
            .map_err(|e| e.to_string())?;
        self.check(project)?; // A late refusal must not adopt an old owner's result.
        Ok(Self::project(row))
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
        let selection_owner = app.connection.owner.clone();
        let selection_lane = app.connection.lane.clone();
        let selection_auth = app.auth.clone();
        let selection = Arc::new(WorkspaceSelections {
            database: app.database.clone(),
            scope: ds_sync_store::SolarWorkspaceScope {
                owner: selection_owner.clone(),
                lane: selection_lane.clone(),
                project: context.project.clone(),
            },
            authorize: Arc::new(move || {
                selection_auth.authorize(&selection_owner)?;
                if crate::auth::owner_fence(&selection_lane)? != selection_owner {
                    return Err("Server owner changed during Solar workspace selection".into());
                }
                Ok(())
            }),
        });
        let renderer = Arc::new(crate::solar_documents::NativeDocuments {
            directory: application_directory.join("document-tools"),
            owner: app.connection.owner.clone(),
            lane: app.connection.lane.clone(),
            auth: app.auth.clone(),
        });
        let host = app
            .solar
            .host(application_directory)?
            .with_engine_lane(&app.connection.lane)
            .map_err(|e| Failure::failed("server_refused", e))?
            .with_workspace_selection_provider(selection)
            .with_report_renderer(renderer)
            .with_media_provider(producer.clone())
            .with_publication_provider(Arc::new(retention(
                app.auth.clone(),
                app.connection.owner.clone(),
                app.database.clone(),
                app.sessions.identity().clone(),
                &context.project,
                app.activity.clone(),
            )))
            .with_snapshot_provider(producer);
        ds_solar_native::application::execute(host, &context.project, &body)
            .map(Json)
            .map_err(|e| Failure::invalid("server_refused", e))
    })
    .await
}

#[cfg(test)]
mod publication_tests {
    //! The Server's binding of the shared retention. The retention itself —
    //! offline sealing, replay after restart, refused bytes and new-build
    //! provenance — is proven where it lives, in `ds_solar_native::sync`.
    use super::*;
    use ds_solar_native::PublicationProvider;
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
    fn identity(owner: &str) -> ds_compute_runtime::HostIdentity {
        ds_compute_runtime::HostIdentity {
            owner: owner.into(),
            principal: ds_command_kernel::execution_context::Principal {
                uid: "account-a".into(),
                lane: "canary".into(),
                deployment: "https://gateway.example".into(),
                install_id: "install-a".into(),
            },
        }
    }
    fn publication() -> ds_solar_native::sync::Publication {
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
                project: "project-a",
                run: "report-run-a",
                operation: "report-city-a",
                variant: "network-draft-en",
                resource: "city-a",
                release: "ds-solar-engine@1.2.3",
                manifest: &"a".repeat(64),
                input_base: &"b".repeat(64),
            },
            vec![(output, bytes)],
        )
        .unwrap()
    }
    #[test]
    fn the_server_retains_under_its_captured_owner_without_a_gateway() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.sqlite");
        let host = retention(
            Arc::new(LocalAuth(Arc::new(AtomicBool::new(true)))),
            "owner-a".into(),
            path.clone(),
            identity("owner-a"),
            "project-a",
            None,
        );
        let receipt = host.retain(publication()).unwrap();
        assert_eq!(receipt["state"], "held");
        assert_eq!(receipt, host.observe(&publication()).unwrap());
        let store = ds_sync_runtime::open_store(&path).unwrap();
        let fence = ds_sync_runtime::solar_producer::fence_of(&identity("owner-a"));
        assert_eq!(
            store
                .lock()
                .unwrap()
                .snapshot(&fence, &ds_sync_runtime::rows::store_scope("project-a"))
                .unwrap()
                .artifacts
                .len(),
            1
        );
    }
    #[test]
    fn a_changed_server_owner_or_foreign_session_identity_retains_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("server.sqlite");
        let refused = retention(
            Arc::new(LocalAuth(Arc::new(AtomicBool::new(false)))),
            "owner-a".into(),
            path.clone(),
            identity("owner-a"),
            "project-a",
            None,
        );
        assert!(refused.retain(publication()).is_err());
        assert!(!path.exists());
        let foreign = retention(
            Arc::new(LocalAuth(Arc::new(AtomicBool::new(true)))),
            "owner-a".into(),
            path.clone(),
            identity("owner-b"),
            "project-a",
            None,
        );
        assert!(
            foreign
                .retain(publication())
                .unwrap_err()
                .contains("captured owner")
        );
        assert!(!path.exists());
    }
}

#[cfg(test)]
mod workspace_selection_tests {
    use super::*;
    use ds_solar_native::workspace_selection::{Origin, Provider};
    use std::sync::atomic::{AtomicBool, Ordering};
    fn provider(
        path: PathBuf,
        authorize: Arc<dyn Fn() -> Result<(), String> + Send + Sync>,
    ) -> WorkspaceSelections {
        WorkspaceSelections {
            database: path,
            scope: ds_sync_store::SolarWorkspaceScope {
                owner: "a".repeat(64),
                lane: "canary".into(),
                project: "project-a".into(),
            },
            authorize,
        }
    }
    #[test]
    fn actual_solar_application_selection_reopens_the_same_sqlite_custom_receipt() {
        let temp = tempfile::tempdir().unwrap();
        let database = temp.path().join("core.sqlite");
        let data = temp.path().join("application");
        let effects = Arc::new(provider(database.clone(), Arc::new(|| Ok(()))));
        let host = NativeHost::headless(data.clone())
            .unwrap()
            .with_workspace_selection_provider(effects.clone());
        let custom = temp.path().join("approved");
        std::fs::create_dir(&custom).unwrap();
        let approved =
            ds_solar_native::workspace::solar_native_choose_workspace(host.clone(), custom.clone())
                .unwrap();
        let request=serde_json::to_vec(&serde_json::json!({"operation":"workspace_selection_set","workspace_handle":approved.workspace_handle,"expected_generation":null})).unwrap();
        let written = ds_solar_native::application::execute(host, "project-a", &request).unwrap();
        assert_eq!(written["value"]["origin"], "approved");
        assert_eq!(written["value"]["generation"], 1);
        let reopened = NativeHost::headless(data)
            .unwrap()
            .with_workspace_selection_provider(Arc::new(provider(database, Arc::new(|| Ok(())))));
        let read = ds_solar_native::application::execute(
            reopened,
            "project-a",
            br#"{"operation":"workspace_selection_read"}"#,
        )
        .unwrap();
        assert_eq!(read["value"], written["value"]);
        assert!(!custom.join("solar-rust").exists());
    }
    #[test]
    fn missing_selection_read_and_foreign_or_signed_out_scope_create_no_store() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("core.sqlite");
        let allowed = provider(path.clone(), Arc::new(|| Ok(())));
        assert!(allowed.read("project-a").unwrap().is_none());
        assert!(allowed.read("project-b").is_err());
        assert!(!path.exists());
        let denied = provider(path.clone(), Arc::new(|| Err("owner changed".into())));
        assert!(denied.read("project-a").is_err());
        assert!(!path.exists());
    }
    #[test]
    fn late_owner_refusal_does_not_adopt_another_scopes_selection() {
        let temp = tempfile::tempdir().unwrap();
        let allowed = Arc::new(AtomicBool::new(true));
        let current = allowed.clone();
        let effects = provider(
            temp.path().join("core.sqlite"),
            Arc::new(move || {
                if current.swap(false, Ordering::SeqCst) {
                    Ok(())
                } else {
                    Err("owner changed".into())
                }
            }),
        );
        assert!(effects.read("project-a").is_err());
        assert!(!effects.database.exists());
        let path = temp.path().join("retained.sqlite");
        let seeded = provider(path.clone(), Arc::new(|| Ok(())));
        let workspace = ds_solar_native::workspace::NativeSolarWorkspace {
            workspace_handle: format!("solar-workspace-{}", "b".repeat(32)),
            dir: "/approved/custom".into(),
        };
        let initial = seeded
            .compare_set("project-a", None, &workspace, Origin::Approved)
            .unwrap();
        let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let held = count.clone();
        let refusing = provider(
            path,
            Arc::new(move || {
                if held.fetch_add(1, Ordering::SeqCst) == 0 {
                    Ok(())
                } else {
                    Err("owner changed".into())
                }
            }),
        );
        assert!(refusing.read("project-a").is_err());
        assert_eq!(seeded.read("project-a").unwrap(), Some(initial));
    }

    fn approved_host() -> (
        tempfile::TempDir,
        NativeHost,
        Arc<WorkspaceSelections>,
        ds_solar_native::workspace::NativeSolarWorkspace,
    ) {
        let temp = tempfile::tempdir().unwrap();
        let effects = Arc::new(provider(
            temp.path().join("core.sqlite"),
            Arc::new(|| Ok(())),
        ));
        let host = NativeHost::headless(temp.path().join("native"))
            .unwrap()
            .with_workspace_selection_provider(effects.clone());
        let custom = temp.path().join("custom");
        std::fs::create_dir(&custom).unwrap();
        let approval =
            ds_solar_native::workspace::solar_native_choose_workspace(host.clone(), custom)
                .unwrap();
        (temp, host, effects, approval)
    }
    #[test]
    fn native_selection_absence_and_closed_requests_have_no_default_or_sqlite_effect() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("core.sqlite");
        let effects = Arc::new(provider(path.clone(), Arc::new(|| Ok(()))));
        let host = NativeHost::headless(temp.path().join("native"))
            .unwrap()
            .with_workspace_selection_provider(effects);
        let reply = ds_solar_native::application::execute(
            host,
            "project-a",
            br#"{"operation":"workspace_selection_read"}"#,
        )
        .unwrap();
        assert!(reply["value"].is_null());
        assert!(!path.exists());
        assert!(!temp.path().join("native/solar-workspaces").exists());
        for request in [
            serde_json::json!({"operation":"workspace_selection_read","owner":"foreign"}),
            serde_json::json!({"operation":"workspace_selection_set","workspace_handle":"approval","expected_generation":null,"dir":"/foreign"}),
            serde_json::json!({"operation":"workspace_selection_default","expected_generation":null,"sql":"no"}),
        ] {
            assert!(
                serde_json::from_value::<ds_solar_native::application::Request>(request).is_err()
            );
        }
        for operation in [
            "workspace_selection_read",
            "workspace_selection_set",
            "workspace_selection_default",
        ] {
            assert!(ds_solar_native::application::schema(Some(operation)).is_ok());
        }
    }
    #[test]
    fn missing_selected_receipt_preserves_sqlite_choice_and_never_chooses_a_default() {
        let (temp, host, effects, approval) = approved_host();
        let selected = ds_solar_native::workspace_selection::select(
            &host,
            "project-a",
            approval.workspace_handle,
            None,
        )
        .unwrap();
        std::fs::remove_file(
            temp.path()
                .join("native/solar-workspaces")
                .join(format!("{}.json", selected.workspace_handle)),
        )
        .unwrap();
        assert!(ds_solar_native::workspace_selection::read(&host, "project-a").is_err());
        assert_eq!(effects.read("project-a").unwrap(), Some(selected));
        assert!(
            !temp
                .path()
                .join("native/solar-workspaces/workspace")
                .exists()
        );
        assert!(!PathBuf::from(approval.dir).join("solar-rust").exists());
    }
    #[test]
    fn stale_default_refuses_before_creation_but_deliberate_default_preserves_custom_inputs() {
        let (temp, host, effects, approval) = approved_host();
        let custom = PathBuf::from(&approval.dir);
        std::fs::write(custom.join("authored-input"), b"keep these exact inputs").unwrap();
        let selected = ds_solar_native::workspace_selection::select(
            &host,
            "project-a",
            approval.workspace_handle,
            None,
        )
        .unwrap();
        assert!(
            ds_solar_native::workspace_selection::select_default(&host, "project-a", None).is_err()
        );
        assert!(
            !temp
                .path()
                .join("native/solar-workspaces/workspace")
                .exists()
        );
        assert_eq!(effects.read("project-a").unwrap(), Some(selected.clone()));
        let successor = ds_solar_native::workspace_selection::select_default(
            &host,
            "project-a",
            Some(selected.generation),
        )
        .unwrap();
        assert_eq!(successor.origin, Origin::Default);
        assert_eq!(successor.generation, 2);
        assert_eq!(
            ds_solar_native::workspace_selection::read(&host, "project-a").unwrap(),
            Some(successor)
        );
        assert_eq!(
            std::fs::read(custom.join("authored-input")).unwrap(),
            b"keep these exact inputs"
        );
        assert!(!custom.join("solar-rust").exists());
    }
    #[test]
    fn native_zero_city_readiness_is_readonly_and_crossed_project_cannot_select() {
        let (_temp, host, effects, approval) = approved_host();
        let directory = PathBuf::from(&approval.dir);
        #[cfg(unix)]
        let before = {
            use std::os::unix::fs::MetadataExt;
            let m = std::fs::metadata(&directory).unwrap();
            (m.mode(), m.ctime(), m.ctime_nsec())
        };
        let readiness = ds_solar_native::native::solar_native_readiness(
            host.clone(),
            ds_solar_native::native::ReadinessCommand {
                workspace_handle: approval.workspace_handle.clone(),
                root: "eds_project/project-a/eds_solar".into(),
                city_ids: Vec::new(),
            },
        )
        .unwrap();
        assert!(readiness.is_empty());
        assert!(!directory.join("solar-rust").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let m = std::fs::metadata(&directory).unwrap();
            assert_eq!((m.mode(), m.ctime(), m.ctime_nsec()), before);
        }
        assert!(
            ds_solar_native::workspace_selection::select(
                &host,
                "project-b",
                approval.workspace_handle,
                None
            )
            .is_err()
        );
        assert!(effects.read("project-a").unwrap().is_none());
        assert!(!effects.database.exists());
    }
    #[test]
    fn native_rejects_false_origin_directory_handle_or_generation_before_adopting_receipt() {
        struct Crossed {
            inner: Arc<WorkspaceSelections>,
            field: &'static str,
        }
        impl Provider for Crossed {
            fn read(
                &self,
                project: &str,
            ) -> Result<Option<ds_solar_native::workspace_selection::Selection>, String>
            {
                self.inner.read(project)
            }
            fn compare_set(
                &self,
                project: &str,
                expected: Option<u64>,
                workspace: &ds_solar_native::workspace::NativeSolarWorkspace,
                origin: Origin,
            ) -> Result<ds_solar_native::workspace_selection::Selection, String> {
                let mut row = self
                    .inner
                    .compare_set(project, expected, workspace, origin)?;
                match self.field {
                    "origin" => row.origin = Origin::Default,
                    "dir" => row.dir = "/foreign/directory".into(),
                    "handle" => {
                        row.workspace_handle = format!("solar-workspace-{}", "c".repeat(32))
                    }
                    "generation" => row.generation += 1,
                    _ => unreachable!(),
                };
                Ok(row)
            }
        }
        for field in ["origin", "dir", "handle", "generation"] {
            let (_temp, host, effects, approval) = approved_host();
            let host = host.with_workspace_selection_provider(Arc::new(Crossed {
                inner: effects.clone(),
                field,
            }));
            assert!(
                ds_solar_native::workspace_selection::select(
                    &host,
                    "project-a",
                    approval.workspace_handle.clone(),
                    None
                )
                .unwrap_err()
                .contains("exact approved intent"),
                "{field}"
            );
            let retained = effects.read("project-a").unwrap().unwrap();
            assert_eq!(retained.workspace_handle, approval.workspace_handle);
            assert_eq!(retained.dir, approval.dir);
            assert_eq!(retained.origin, Origin::Approved);
            assert_eq!(retained.generation, 1);
        }
    }
}
