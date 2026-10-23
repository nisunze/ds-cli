//! Explicit reference exchange against the governed native capture and workspace.
use crate::{
    NativeHost,
    native::{self, CitySnapshotInput},
    workspace,
};
use ds_solar_contracts::SolarCityInput;
use ds_solar_reference_host::{
    preparation::ReferencePreparer,
    reference_import::{self, VerifiedBundle},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::path::PathBuf;

#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PlanCommand {
    pub workspace_handle: String,
    pub city_id: String,
    pub language: Option<String>,
    pub output: PathBuf,
}
#[derive(Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ImportCommand {
    pub workspace_handle: String,
    pub city_id: String,
    pub language: Option<String>,
    pub expected_source_fingerprint: String,
    pub directory: PathBuf,
}
fn language(value: Option<&str>) -> Result<&str, String> {
    match value.unwrap_or("fr") {
        value @ ("fr" | "en") => Ok(value),
        _ => Err("Solar preparation language must be fr or en".into()),
    }
}
fn capture(
    host: &NativeHost,
    project: &str,
    city: &str,
    language: &str,
) -> Result<(SolarCityInput, String), String> {
    if !ds_solar_contracts::is_solar_id(city) {
        return Err("Invalid Solar city id".into());
    }
    let provider = host
        .snapshots
        .as_ref()
        .ok_or("This host has no governed Solar snapshot producer")?;
    let root = format!("eds_project/{project}/eds_solar");
    let selector = CitySnapshotInput {
        city_id: city.into(),
        display_name: String::new(),
        snapshot_json: String::new(),
        source: String::new(),
        captured_at: String::new(),
        record_updated_at_ms: None,
        revision: None,
    };
    let (captured, _) = native::captured_city(&selector, &root, provider.capture(city)?)?;
    let fingerprint = captured
        .revision
        .clone()
        .ok_or("Missing governed source fingerprint")?;
    Ok((
        native::normalize_city_input(&captured, project, &root, language)
            .map_err(|e| e.to_string())?,
        fingerprint,
    ))
}
fn receipt(
    project: &str,
    city: &str,
    fingerprint: &str,
    input: &SolarCityInput,
) -> Result<Value, String> {
    let preparer = ReferencePreparer::new();
    Ok(
        json!({"project":project,"city":city,"source_fingerprint":fingerprint,"input_digest":input.content_digest,"reference_key":preparer.reference_key(input).map_err(|e| e.to_string())?}),
    )
}
pub fn plan(host: &NativeHost, project: &str, command: PlanCommand) -> Result<Value, String> {
    workspace::resolve_workspace_handle(host, &command.workspace_handle)?;
    let parent = command
        .output
        .parent()
        .ok_or("Reference request needs a parent directory")?;
    workspace::secure_existing_private_directory(parent)?;
    match std::fs::symlink_metadata(&command.output) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        _ => return Err("reference request output already exists or is inaccessible".into()),
    }
    let (input, fingerprint) = capture(
        host,
        project,
        &command.city_id,
        language(command.language.as_deref())?,
    )?;
    let request = ReferencePreparer::new()
        .request_for(&input)
        .map_err(|e| e.to_string())?;
    let bytes = serde_json::to_vec_pretty(&request).map_err(|e| e.to_string())?;
    if bytes.len() > 8 * 1024 {
        return Err("Reference request exceeds its byte bound".into());
    }
    let mut result = receipt(project, &command.city_id, &fingerprint, &input)?;
    ds_solar_host::files::write(&command.output, &bytes).map_err(|e| e.to_string())?;
    ds_solar_host::files::sync_dir(parent).map_err(|e| e.to_string())?;
    result["request_file"] = json!(command.output);
    Ok(result)
}
pub fn import(host: &NativeHost, project: &str, command: ImportCommand) -> Result<Value, String> {
    let workspace = workspace::resolve_workspace_handle(host, &command.workspace_handle)?;
    let language = language(command.language.as_deref())?;
    let (input, fingerprint) = capture(host, project, &command.city_id, language)?;
    if fingerprint != command.expected_source_fingerprint {
        return Err("Solar city changed during reference acquisition".into());
    }
    let mut result = receipt(project, &command.city_id, &fingerprint, &input)?;
    let bundle = reference_import::read_bundle(&command.directory).map_err(|e| e.to_string())?;
    let verified = VerifiedBundle::verify(input, bundle, &chrono::Utc::now().to_rfc3339())
        .map_err(|e| e.to_string())?;
    let (_, current) = capture(host, project, &command.city_id, language)?;
    if current != fingerprint {
        return Err("Solar city changed during reference verification".into());
    }
    let committed = verified
        .commit(&native::weather_cache_dir(&workspace)?)
        .map_err(|e| e.to_string())?;
    result["ready"] = json!(true);
    result["acquired"] = json!(!committed.reused);
    result["reused"] = json!(committed.reused);
    result["bytes"] = json!(committed.bytes);
    result["prepared_input_digest"] = json!(committed.prepared_input_digest);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapturedCitySnapshot, SnapshotProvider, application::Request};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    const PROJECT: &str = "arjgpydw_aderm_loc7";
    struct Snapshots {
        calls: AtomicUsize,
        change_after: usize,
    }
    impl SnapshotProvider for Snapshots {
        fn capture(&self, city: &str) -> Result<CapturedCitySnapshot, String> {
            let call = self.calls.fetch_add(1, Ordering::SeqCst);
            let fingerprint = if call >= self.change_after { "c" } else { "b" }.repeat(64);
            let mut doc: Value = serde_json::from_slice(
                &std::fs::read(fixture("city/aderm_bere.snapshot.json")).unwrap(),
            )
            .unwrap();
            doc["_provenance"]["root"] = format!("eds_project/{PROJECT}/eds_solar").into();
            doc["_provenance"]["context"] = city.into();
            doc["_provenance"]["input_base_fingerprint"] = fingerprint.clone().into();
            Ok(CapturedCitySnapshot {
                snapshot_json: doc.to_string(),
                input_base_fingerprint: fingerprint,
                captured_at: format!("2026-09-17T00:00:{:02}Z", call % 60),
                publication: None,
            })
        }
    }
    struct ForbiddenReferenceProvider;
    impl crate::ReferenceBundleProvider for ForbiddenReferenceProvider {
        fn fetch(&self, _: &crate::ReferenceRequest) -> crate::SolarResult<crate::BundleBytes> {
            panic!("Explicit reference exchange must never acquire through a provider")
        }
    }
    fn fixture(path: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../ds-solar/fixtures")
            .join(path)
    }
    fn host(root: &std::path::Path, change_after: usize) -> (NativeHost, String, PathBuf) {
        let host = NativeHost::headless(root.join("authority"))
            .unwrap()
            .with_snapshot_provider(Arc::new(Snapshots {
                calls: AtomicUsize::new(0),
                change_after,
            }));
        let ws = workspace::solar_native_prepare_default_workspace(host.clone()).unwrap();
        (host, ws.workspace_handle, PathBuf::from(ws.dir))
    }
    fn execute(host: &NativeHost, value: Value) -> Result<Value, String> {
        serde_json::from_value::<Request>(value)
            .unwrap()
            .execute(host.clone(), PROJECT)
            .map(|receipt| receipt["value"].clone())
    }
    fn import_request(handle: &str, directory: &std::path::Path) -> Value {
        json!({"operation":"reference_import","command":{"workspace_handle":handle,"city_id":"aderm_bere","expected_source_fingerprint":"b".repeat(64),"directory":directory}})
    }
    #[test]
    fn application_schema_projects_closed_reference_exchange() {
        for operation in ["reference_plan", "reference_import"] {
            let schema = crate::application::schema(Some(operation))
                .unwrap()
                .to_string();
            assert!(schema.contains("workspace_handle"));
            assert!(schema.contains("city_id"));
            for forbidden in ["project_id", "cache_dir", "weather_url", "weather_token"] {
                assert!(!schema.contains(forbidden), "{forbidden} in {operation}");
            }
        }
    }
    #[test]
    fn governed_plan_import_and_explicit_prepare_share_one_native_cache() {
        let tmp = tempfile::tempdir().unwrap();
        let (host, handle, ws) = host(tmp.path(), usize::MAX);
        let host = host.with_reference_provider(Arc::new(ForbiddenReferenceProvider));
        let output = tmp.path().join("request.json");
        let result = execute(&host, json!({"operation":"reference_plan","command":{"workspace_handle":handle,"city_id":"aderm_bere","output":output}})).unwrap();
        let (input, _) = capture(&host, PROJECT, "aderm_bere", "fr").unwrap();
        let request: Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert_eq!(
            request,
            serde_json::to_value(ReferencePreparer::new().request_for(&input).unwrap()).unwrap()
        );
        assert_eq!(result["source_fingerprint"], "b".repeat(64));
        assert!(execute(&host, json!({"operation":"reference_plan","command":{"workspace_handle":handle,"city_id":"aderm_bere","output":output}})).unwrap_err().contains("already exists"));
        assert!(!ws.join("solar-rust/v1/weather-cache").exists());
        let request = import_request(&handle, &fixture("reference/aderm_bere"));
        let first = execute(&host, request.clone()).unwrap();
        assert_eq!(first["acquired"], true);
        let second = execute(&host, request).unwrap();
        assert_eq!(second["reused"], true);
        assert!(
            !ws.join("solar-rust/v1/prepared").exists(),
            "Import retains references only; preparation is explicit"
        );
        let job = native::solar_native_prepare_captured(
            host.clone(),
            &host.state(),
            native::PrepareCapturedCommand {
                workspace_handle: handle,
                project_id: PROJECT.into(),
                root: format!("eds_project/{PROJECT}/eds_solar"),
                city_ids: vec!["aderm_bere".into()],
                concurrency: Some(1),
                overwrite: false,
                language: None,
            },
        )
        .unwrap();
        let mut done = false;
        for _ in 0..1500 {
            let progress = native::preparation_progress(&host, PROJECT, &job.job_id).unwrap();
            if progress["done"] == true {
                assert_eq!(progress["cities"][0]["state"], "ready", "{progress}");
                done = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(done);
    }
    #[test]
    fn stale_scope_and_changed_source_refuse_before_commit() {
        let tmp = tempfile::tempdir().unwrap();
        let (host, handle, ws) = host(tmp.path(), usize::MAX);
        let mut request = import_request(&handle, &tmp.path().join("not-read"));
        request["command"]["expected_source_fingerprint"] = "stale".into();
        assert!(execute(&host, request).unwrap_err().contains("changed"));
        let request = import_request(&handle, &tmp.path().join("not-read"));
        assert!(
            serde_json::from_value::<Request>(request)
                .unwrap()
                .execute(host, "other")
                .unwrap_err()
                .contains("another city, project")
        );
        assert!(!ws.join("solar-rust/v1/weather-cache").exists());
        let tmp = tempfile::tempdir().unwrap();
        let (host, handle, ws) = self::host(tmp.path(), 1);
        assert!(
            execute(
                &host,
                import_request(&handle, &fixture("reference/aderm_bere"))
            )
            .unwrap_err()
            .contains("verification")
        );
        assert!(!ws.join("solar-rust/v1/weather-cache").exists());
    }
    #[test]
    fn wrong_site_tampered_bundle_and_unavailable_capture_refuse() {
        let tmp = tempfile::tempdir().unwrap();
        let (host, handle, ws) = host(tmp.path(), usize::MAX);
        assert!(
            execute(
                &host,
                import_request(&handle, &fixture("reference/aderm_gagal"))
            )
            .is_err()
        );
        let bundle = tmp.path().join("bundle");
        std::fs::create_dir(&bundle).unwrap();
        for name in ds_solar_host::cache::ARTIFACT_NAMES {
            std::fs::copy(
                fixture("reference/aderm_bere").join(name),
                bundle.join(name),
            )
            .unwrap();
        }
        std::fs::write(bundle.join("weather.json"), b"{}").unwrap();
        assert!(execute(&host, import_request(&handle, &bundle)).is_err());
        assert!(!ws.join("solar-rust/v1/weather-cache").exists());
        let bare = NativeHost::headless(tmp.path().join("bare")).unwrap();
        let ws = workspace::solar_native_prepare_default_workspace(bare.clone()).unwrap();
        let existing = tmp.path().join("existing.json");
        std::fs::write(&existing, b"retained").unwrap();
        assert!(execute(&bare, json!({"operation":"reference_plan","command":{"workspace_handle":ws.workspace_handle,"city_id":"aderm_bere","output":existing}})).unwrap_err().contains("already exists"));
        assert_eq!(std::fs::read(existing).unwrap(), b"retained");
        assert!(
            execute(&bare, import_request(&ws.workspace_handle, &bundle))
                .unwrap_err()
                .contains("no governed")
        );
        let mut request = import_request(&handle, &bundle);
        request["command"]["project_id"] = PROJECT.into();
        assert!(serde_json::from_value::<Request>(request).is_err());
    }
}
