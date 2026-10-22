//! Closed native application requests. Project attribution is supplied by the
//! authorized host, never inferred from mutable Desktop or browser selection.
use crate::{NativeHost, native::*, workspace};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

/// File transport covers the native 64 MiB artifact bound serialized as a
/// compact JSON u8 array (at most four bytes per element), plus 1 MiB envelope.
/// Ordinary stdout transport retains its separate 32 MiB response bound.
pub const MAX_FILE_RESPONSE_BYTES: usize = 257 * 1024 * 1024;

/// Server preparation accepts authored inputs; provider authority is host-owned.
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PreparationIntent {
    pub workspace_handle: String,
    pub project_id: String,
    pub root: String,
    pub cities: Vec<CitySnapshotInput>,
    pub concurrency: Option<usize>,
    #[serde(default)]
    pub overwrite: bool,
    pub language: Option<String>,
}
impl From<PreparationIntent> for PrepareCommand {
    fn from(intent: PreparationIntent) -> Self {
        Self {
            workspace_handle: intent.workspace_handle,
            project_id: intent.project_id,
            root: intent.root,
            cities: intent.cities,
            concurrency: intent.concurrency,
            overwrite: intent.overwrite,
            language: intent.language,
            weather_url: None,
            weather_token: None,
            weather_api_key: None,
        }
    }
}

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    ReferencePlan {
        command: crate::reference::PlanCommand,
    },
    ReferenceImport {
        command: crate::reference::ImportCommand,
    },
    ProjectClean {
        command: ds_solar_project::clean::Control,
    },
    RunSettings {
        command: crate::settings::RunSettings,
    },
    PipelineStatus {
        command: crate::pipeline::Snapshot,
    },
    CityFinalImport {
        command: city_finals::ImportCommand,
    },
    CityFinalInspect {
        command: city_finals::Selector,
    },
    PortfolioStart {
        command: PortfolioCalculateCommand,
    },
    PortfolioProgress {
        job_id: String,
    },
    PortfolioCancel {
        job_id: String,
    },
    ReportFinish {
        command: ReportFinishCommand,
    },
    PortfolioPublish {
        command: PortfolioBatchCommand,
    },
    PortfolioFinalImport {
        command: PortfolioFinalImportCommand,
    },
    PortfolioFinalInspect {
        command: PortfolioFinalSelector,
    },
    PortfolioReportBundle {
        command: PortfolioReportBundleCommand,
    },
    ReportBundle {
        command: ReportBundleCommand,
    },
    Dashboard {
        command: crate::dashboard::Command,
    },
    WorkspaceSelectionRead {},
    WorkspaceSelectionSet {
        workspace_handle: String,
        expected_generation: Option<u64>,
    },
    WorkspaceSelectionDefault {
        expected_generation: Option<u64>,
    },
    WorkspacePaths,
    WorkspaceDefault,
    WorkspaceChoose {
        path: PathBuf,
    },
    ImportPrepared {
        workspace_handle: String,
        file: PathBuf,
        expected: Option<String>,
    },
    Prepare {
        command: PreparationIntent,
    },
    PrepareCaptured {
        command: PrepareCapturedCommand,
    },
    PrepareCancel {
        job_id: String,
    },
    PrepareProgress {
        job_id: String,
    },
    Readiness {
        command: ReadinessCommand,
    },
    GenerationProfile {
        command: GenerationProfileCommand,
    },
    Calculate {
        command: CalculateCommand,
    },
    CalculateProgress {
        run_id: String,
    },
    Cancel {
        run_id: String,
    },
    Engine,
    Documents {
        command: RunDocumentsCommand,
    },
    PortfolioCalculate {
        command: PortfolioCalculateCommand,
    },
    PortfolioBatchArtifacts {
        command: PortfolioBatchCommand,
    },
    DocumentRead {
        command: DocumentCommand,
    },
    LoadProfileRead {
        command: LoadProfileCommand,
    },
    BatchArtifactRead {
        command: BatchArtifactCommand,
    },
    ExportBatchArtifact {
        command: BatchArtifactCommand,
        expected_content_digest: String,
        destination: PathBuf,
    },
    PortfolioBatchArtifactRead {
        command: PortfolioBatchArtifactCommand,
    },
    ExportPortfolioBatchArtifact {
        command: PortfolioBatchArtifactCommand,
        destination: PathBuf,
    },
    FinalImport {
        command: ImportFinalCommand,
    },
    FinalInspect {
        command: FinalSelector,
    },
}

impl Request {
    /// The transport consumes the owner's bound for complete media/tool effects.
    pub fn response_timeout_seconds(&self) -> u64 {
        match self {
            Self::ReportFinish { .. }
            | Self::ReportBundle { .. }
            | Self::PortfolioReportBundle { .. }
            | Self::PortfolioPublish { .. } => 600,
            _ => 60,
        }
    }
    fn root(&self) -> Option<&str> {
        match self {
            Self::CityFinalImport { command } => Some(&command.batch.root),
            Self::CityFinalInspect { command } => Some(&command.batch.root),
            Self::ReportFinish { command } => Some(command.source.root()),
            Self::PortfolioPublish { command } => Some(&command.root),
            Self::PortfolioFinalImport { command } => Some(&command.batch.root),
            Self::PortfolioFinalInspect { command } => Some(&command.batch.root),
            Self::PortfolioReportBundle { command } => Some(&command.batch.root),
            Self::ReportBundle { command } => Some(&command.root),
            Self::Dashboard { command } => Some(&command.root),
            Self::Prepare { command } => Some(&command.root),
            Self::PrepareCaptured { command } => Some(&command.root),
            Self::Readiness { command } => Some(&command.root),
            Self::GenerationProfile { command } => Some(&command.root),
            Self::Calculate { command } => Some(&command.root),
            Self::Documents { command } => Some(&command.root),
            Self::PortfolioCalculate { command } => Some(&command.root),
            Self::PortfolioStart { command } => Some(&command.root),
            Self::PortfolioBatchArtifacts { command } => Some(&command.root),
            Self::DocumentRead { command } => Some(&command.root),
            Self::LoadProfileRead { command } => Some(&command.root),
            Self::BatchArtifactRead { command } => Some(&command.root),
            Self::ExportBatchArtifact { command, .. } => Some(&command.root),
            Self::PortfolioBatchArtifactRead { command } => Some(&command.root),
            Self::ExportPortfolioBatchArtifact { command, .. } => Some(&command.root),
            Self::FinalImport { command } => Some(&command.root),
            Self::FinalInspect { command } => Some(&command.root),
            _ => None,
        }
    }
    pub fn execute(self, host: NativeHost, project: &str) -> Result<Value, String> {
        if !ds_solar_contracts::is_solar_id(project) {
            return Err("An explicit canonical Solar project is required".into());
        }
        let root = format!("eds_project/{project}/eds_solar");
        if self.root().is_some_and(|r| r != root) {
            return Err("Solar application request belongs to another project".into());
        }
        match &self {
            Self::ProjectClean { command } if command.project_id != project => {
                return Err("Local cleanup belongs to another project".into());
            }
            Self::PrepareCaptured { command } if command.project_id != project => {
                return Err("Preparation belongs to another project".into());
            }
            Self::Prepare { command } if command.project_id != project => {
                return Err("Preparation belongs to another project".into());
            }
            Self::Calculate { command } if command.project_id != project => {
                return Err("Calculation belongs to another project".into());
            }
            Self::PortfolioCalculate { command } | Self::PortfolioStart { command }
                if command.project_id != project =>
            {
                return Err("Portfolio belongs to another project".into());
            }
            Self::FinalImport { command } if command.source_path.is_none() => {
                return Err("An explicit Markdown source path is required headlessly".into());
            }
            _ => (),
        }
        let state = host.state();
        let result = match self {
            Self::ReferencePlan { command } => crate::reference::plan(&host, project, command)?,
            Self::ReferenceImport { command } => crate::reference::import(&host, project, command)?,
            Self::ProjectClean { command } => {
                ds_solar_project::clean::execute(command).map_err(|error| error.to_string())?
            }
            Self::RunSettings { command } => json!(command.validate()?),
            Self::PipelineStatus { command } => crate::pipeline::project(command),
            Self::CityFinalImport { command } => json!(city_finals::import(host, command)?),
            Self::CityFinalInspect { command } => json!(city_finals::inspect(host, command)?),
            Self::PortfolioStart { command } => {
                json!(portfolio_jobs::start(host, project, command)?)
            }
            Self::PortfolioProgress { job_id } => {
                json!(portfolio_jobs::progress(&host, project, &job_id)?)
            }
            Self::PortfolioCancel { job_id } => {
                json!({"cancel_requested":portfolio_jobs::cancel(&host,project,&job_id)?})
            }
            Self::ReportFinish { command } => solar_native_finish_report(host, command)?,
            Self::PortfolioPublish { command } => solar_native_publish_portfolio(host, command)?,
            Self::PortfolioFinalImport { command } => {
                json!(solar_native_import_portfolio_final(host, command)?)
            }
            Self::PortfolioFinalInspect { command } => {
                json!(solar_native_inspect_portfolio_final(host, command)?)
            }
            Self::PortfolioReportBundle { command } => {
                solar_native_portfolio_report_bundle(host, command)?
            }
            Self::ReportBundle { command } => solar_native_report_bundle(host, command)?,
            Self::Dashboard { command } => crate::dashboard::compose(&host, project, command)?,
            Self::WorkspaceSelectionRead {} => {
                json!(crate::workspace_selection::read(&host, project)?)
            }
            Self::WorkspaceSelectionSet {
                workspace_handle,
                expected_generation,
            } => json!(crate::workspace_selection::select(
                &host,
                project,
                workspace_handle,
                expected_generation
            )?),
            Self::WorkspaceSelectionDefault {
                expected_generation,
            } => json!(crate::workspace_selection::select_default(
                &host,
                project,
                expected_generation
            )?),
            Self::WorkspacePaths => json!(workspace::solar_native_workspace_paths(host)?),
            Self::WorkspaceDefault => {
                json!(workspace::solar_native_prepare_default_workspace(host)?)
            }
            Self::WorkspaceChoose { path } => {
                json!(workspace::solar_native_choose_workspace(host, path)?)
            }
            Self::ImportPrepared {
                workspace_handle,
                file,
                expected,
            } => import_prepared(
                &host,
                &workspace_handle,
                project,
                &file,
                expected.as_deref(),
            )?,
            Self::Prepare { command } => json!(solar_native_prepare(host, &state, command.into())?),
            Self::PrepareCaptured { command } => {
                json!(solar_native_prepare_captured(host, &state, command)?)
            }
            Self::PrepareCancel { job_id } => {
                json!({"cancelled":preparation_cancel(&host,project,&job_id)?})
            }
            Self::PrepareProgress { job_id } => preparation_progress(&host, project, &job_id)?,
            Self::Readiness { command } => json!(solar_native_readiness(host, command)?),
            Self::GenerationProfile { command } => {
                json!(solar_native_generation_profile(host, command)?)
            }
            Self::Calculate { command } => json!(solar_native_calculate(host, &state, command)?),
            Self::CalculateProgress { run_id } => calculate_progress(&host, project, &run_id)?,
            Self::Cancel { run_id } => json!({"cancelled":solar_native_cancel(&state,run_id)?}),
            Self::Engine => json!(solar_native_engine_for_host(&host)?),
            Self::Documents { command } => json!(solar_native_run_documents(host, command)?),
            Self::PortfolioCalculate { command } => {
                json!(solar_native_calculate_portfolio(host, command)?)
            }
            Self::PortfolioBatchArtifacts { command } => {
                json!(solar_native_portfolio_batch_artifacts(host, command)?)
            }
            Self::DocumentRead { command } => json!(solar_native_read_document(host, command)?),
            Self::LoadProfileRead { command } => {
                json!(solar_native_read_load_profile(host, command)?)
            }
            Self::BatchArtifactRead { command } => {
                json!(solar_native_read_batch_artifact(host, command)?)
            }
            Self::ExportBatchArtifact {
                command,
                expected_content_digest,
                destination,
            } => {
                let bytes = solar_native_read_batch_artifact(host, command)?;
                export_bytes(&destination, &expected_content_digest, &bytes)?
            }
            Self::PortfolioBatchArtifactRead { command } => {
                json!(solar_native_read_portfolio_batch_artifact(host, command)?)
            }
            Self::ExportPortfolioBatchArtifact {
                command,
                destination,
            } => {
                let expected = command.expected_content_digest.clone();
                let bytes = solar_native_read_portfolio_batch_artifact(host, command)?;
                export_bytes(&destination, &expected, &bytes)?
            }
            Self::FinalImport { command } => json!(solar_native_import_final(host, command)?),
            Self::FinalInspect { command } => json!(solar_native_inspect_final(host, command)?),
        };
        Ok(
            json!({"schema":"ds-solar.application-receipt/v1","project_id":project,"root":root,"value":result}),
        )
    }
}

pub(crate) fn export_bytes(
    destination: &std::path::Path,
    expected: &str,
    bytes: &[u8],
) -> Result<Value, String> {
    use std::io::Write;
    let digest = ds_solar_contracts::digest::sha256_hex(bytes);
    if digest != expected {
        return Err("Solar export content does not match the selected digest".into());
    }
    let parent = destination
        .parent()
        .ok_or("An absolute export destination is required")?;
    let name = destination
        .file_name()
        .and_then(|s| s.to_str())
        .filter(|s| workspace::valid_solar_segment(s))
        .ok_or("Invalid Solar export filename")?;
    let parent = workspace::secure_existing_private_directory(parent)?;
    let path = parent.join(name);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .map_err(|e| format!("Cannot create new Solar export: {e}"))?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(&path);
        return Err(format!("Solar export did not complete: {error}"));
    }
    Ok(
        json!({"destination":path,"byte_count":bytes.len(),"content_digest":digest,
        "publication":"not_requested"}),
    )
}

pub fn execute(host: NativeHost, project: &str, bytes: &[u8]) -> Result<Value, String> {
    if bytes.is_empty() || bytes.len() > 32 * 1024 * 1024 {
        return Err("Solar application request exceeds 32 MiB".into());
    }
    let request: Request =
        serde_json::from_slice(bytes).map_err(|_| "Invalid closed Solar application request")?;
    request.execute(host, project)
}

/// Discover the exact serde contract without opening a workspace or restoring
/// authentication. Selecting one operation retains only its reachable types.
pub fn schema(operation: Option<&str>) -> Result<Value, String> {
    let schema = serde_json::to_value(schemars::schema_for!(Request))
        .map_err(|_| "Cannot encode Solar application schema")?;
    let variants = schema["oneOf"]
        .as_array()
        .ok_or("Missing Solar application variants")?;
    let name = |v: &Value| {
        v["properties"]["operation"]["const"]
            .as_str()
            .or_else(|| v["properties"]["operation"]["enum"][0].as_str())
            .map(str::to_owned)
    };
    let Some(operation) = operation else {
        return Ok(
            json!({"schema":"ds-solar.application-schema/v1","operations":variants.iter().filter_map(name).collect::<Vec<_>>(),"next":"repeat with --operation <exact-name> for its closed request schema"}),
        );
    };
    let mut selected = variants
        .iter()
        .find(|v| name(v).as_deref() == Some(operation))
        .cloned()
        .ok_or("Unknown native Solar operation")?;
    fn references(value: &Value, found: &mut std::collections::BTreeSet<String>) {
        match value {
            Value::Object(o) => {
                if let Some(r) = o
                    .get("$ref")
                    .and_then(Value::as_str)
                    .and_then(|r| r.strip_prefix("#/$defs/"))
                {
                    found.insert(r.into());
                }
                for v in o.values() {
                    references(v, found);
                }
            }
            Value::Array(a) => {
                for v in a {
                    references(v, found);
                }
            }
            _ => (),
        }
    }
    let mut pending = std::collections::BTreeSet::new();
    let mut defs = serde_json::Map::new();
    references(&selected, &mut pending);
    while let Some(key) = pending.pop_first() {
        if defs.contains_key(&key) {
            continue;
        }
        let definition = schema["$defs"]
            .get(&key)
            .ok_or("Missing native Solar schema definition")?;
        references(definition, &mut pending);
        defs.insert(key, definition.clone());
    }
    selected["$defs"] = Value::Object(defs);
    Ok(json!({"schema":"ds-solar.application-schema/v1","operation":operation,"request":selected}))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_clean_json_matches_its_owner_and_refuses_crossed_host_context() {
        use ds_solar_project::{
            ProjectIdentity, Workspace,
            clean::{self, Control},
        };
        let directory = tempfile::tempdir().unwrap();
        let workspace = directory.path().join("workspace");
        Workspace::initialize(
            &workspace,
            ProjectIdentity {
                project_id: "project".into(),
                root: "eds_project/project/eds_solar".into(),
            },
        )
        .unwrap();
        let control = Control {
            workspace,
            project_id: "project".into(),
        };
        let packet =
            serde_json::to_vec(&json!({"operation":"project_clean","command":control})).unwrap();
        let owner = clean::execute(control).unwrap();
        let host = NativeHost::headless(directory.path().join("native")).unwrap();
        assert_eq!(
            execute(host.clone(), "project", &packet).unwrap()["value"],
            owner
        );
        assert!(
            execute(host, "other", &packet)
                .unwrap_err()
                .contains("another project")
        );
    }
    #[test]
    fn request_schema_is_bounded_and_keeps_only_the_selected_operation_types() {
        let catalog = schema(None).unwrap();
        assert!(
            catalog["operations"]
                .as_array()
                .unwrap()
                .contains(&json!("calculate"))
        );
        let selected = schema(Some("calculate")).unwrap();
        assert!(
            selected["request"]["$defs"]
                .get("CalculateCommand")
                .is_some()
        );
        assert!(selected["request"]["$defs"].get("PrepareCommand").is_none());
        let concurrency =
            &selected["request"]["$defs"]["CalculateCommand"]["properties"]["concurrency"];
        assert_eq!(concurrency["minimum"], 0);
        assert_eq!(concurrency["maximum"], 32);
        assert!(
            concurrency["description"]
                .as_str()
                .unwrap()
                .contains("automatic")
        );
        assert!(serde_json::to_vec(&selected).unwrap().len() < 15_000);
        assert!(schema(Some("arbitrary_api")).is_err());
        let invalid = json!({"operation":"calculate","command":{"workspace_handle":"handle","root":"root","project_id":"project","city_ids":["city"],"run_id":"run","extra":true}});
        assert!(serde_json::from_value::<Request>(invalid).is_err());
    }
    #[test]
    fn selection_requests_are_closed_and_never_accept_authority_or_paths() {
        for value in [
            json!({"operation":"workspace_selection_read","owner":"foreign"}),
            json!({"operation":"workspace_selection_set","workspace_handle":"approval","expected_generation":null,"dir":"/arbitrary"}),
            json!({"operation":"workspace_selection_default","expected_generation":null,"sql":"no"}),
        ] {
            assert!(serde_json::from_value::<Request>(value).is_err());
        }
        for operation in [
            "workspace_selection_read",
            "workspace_selection_set",
            "workspace_selection_default",
        ] {
            assert!(schema(Some(operation)).is_ok());
        }
    }
    #[test]
    fn calculation_concurrency_is_owned_by_rust() {
        let automatic = calculation_concurrency(None).unwrap().get();
        assert_eq!(calculation_concurrency(Some(0)).unwrap().get(), automatic);
        for explicit in [1, 32] {
            assert_eq!(
                calculation_concurrency(Some(explicit)).unwrap().get(),
                explicit
            );
        }
        assert!(calculation_concurrency(Some(33)).is_err());
        assert!(calculation_concurrency(Some(1024)).is_err());
    }
    #[test]
    fn exports_preserve_exact_bytes_and_refuse_overwrite_or_unpinned_content() {
        let root = std::env::temp_dir().join(format!("solar-export-{}", uuid::Uuid::new_v4()));
        let root = workspace::secure_private_directory(&root).unwrap();
        let destination = root.join("draft.md");
        let bytes = b"# Draft\n\nExact source bytes.\n";
        let expected = ds_solar_contracts::digest::sha256_hex(bytes);
        assert!(export_bytes(&destination, "sha256:wrong", bytes).is_err());
        assert!(!destination.exists());
        let receipt = export_bytes(&destination, &expected, bytes).unwrap();
        assert_eq!(receipt["publication"], "not_requested");
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        assert!(export_bytes(&destination, &expected, bytes).is_err());
        assert_eq!(std::fs::read(&destination).unwrap(), bytes);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn application_rejects_foreign_project_before_any_effect() {
        let path = std::env::temp_dir().join(format!("solar-app-{}", uuid::Uuid::new_v4()));
        let host = NativeHost::headless(path.clone()).unwrap();
        let request = json!({"operation":"calculate","command":{"workspace_handle":"bad","root":"eds_project/b/eds_solar","project_id":"b","city_ids":["city"],"run_id":"run"}});
        let err = execute(host, "a", &serde_json::to_vec(&request).unwrap()).unwrap_err();
        assert!(err.contains("another project"));
        assert!(!path.join("solar-workspaces").exists());
        std::fs::remove_dir_all(path).unwrap();
    }
    #[test]
    fn server_preparation_rejects_caller_provider_authority_before_workspace_effects() {
        let path = std::env::temp_dir().join(format!("solar-provider-{}", uuid::Uuid::new_v4()));
        let host = NativeHost::headless(path.clone()).unwrap();
        let request = json!({"operation":"prepare","command":{
            "workspace_handle":"bad","root":"eds_project/p/eds_solar","project_id":"p",
            "cities":[],"weather_url":"https://caller.invalid"
        }});
        let error = execute(host, "p", &serde_json::to_vec(&request).unwrap()).unwrap_err();
        assert!(error.contains("Invalid closed"));
        let schema = schema(Some("prepare")).unwrap().to_string();
        assert!(!schema.contains("weather_url"));
        assert!(!schema.contains("weather_token"));
        assert!(!path.join("solar-workspaces").exists());
        std::fs::remove_dir_all(path).unwrap();
    }
}
