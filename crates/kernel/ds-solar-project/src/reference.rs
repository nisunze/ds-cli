//! Verified reference cache preparation for one captured local city.
use crate::{Workspace, files, timestamp};
use anyhow::{Result, ensure};
use ds_solar_contracts::{BundleBytes, ReferencePin, SolarCityInput, SolarErrorCode};
use ds_solar_host::{
    cache::{DirectoryReferenceCache, ReferenceCache},
    preparation::{AddonPreparation, PreparationTasks},
};
#[cfg(test)]
use ds_solar_reference_host::ReferencePreparer;
use serde_json::{Value, json};
use std::{path::Path, sync::Arc};

pub fn plan(workspace: &Path, city: &str, cache: &Path) -> Result<Value> {
    plan_with_tasks(
        workspace,
        city,
        cache,
        Arc::new(AddonPreparation::installed()),
    )
}
pub fn plan_with_tasks(
    workspace: &Path,
    city: &str,
    cache: &Path,
    tasks: Arc<dyn PreparationTasks>,
) -> Result<Value> {
    let ws = Workspace::open(workspace)?;
    let input = ws.cities(&[city.to_owned()])?.remove(0);
    plan_input(&ws, city, cache, input, tasks)
}
fn plan_input(
    ws: &Workspace,
    city: &str,
    cache: &Path,
    input: SolarCityInput,
    tasks: Arc<dyn PreparationTasks>,
) -> Result<Value> {
    let cache_root = cache.to_path_buf();
    let cache = DirectoryReferenceCache::new(cache);
    let pin = ReferencePin::current();
    let planned = tasks.plan(&input, pin.clone())?;
    let ready = match cache.get(&planned.key)? {
        None => false,
        Some(bundle) => match tasks.prepare(planned.city, pin, &bundle, &timestamp()) {
            Ok(_) => true,
            Err(error)
                if matches!(
                    error.code,
                    SolarErrorCode::SolarEngineUpdateRequired
                        | SolarErrorCode::Cancelled
                        | SolarErrorCode::ArtifactFailed
                ) =>
            {
                return Err(error.into());
            }
            Err(_) => false,
        },
    };
    Ok(
        json!({"project":ws.identity.project_id,"city":city,"input_digest":input.content_digest,
        "ready":ready,"request":planned.request,"cache":cache_root,"versioned_cache":cache.versioned_root()}),
    )
}
/// Export the owner's exact selected-city request, with no provider or API.
pub fn plan_file(
    workspace: &Path,
    project: &str,
    city: &str,
    cache: &Path,
    output: &Path,
) -> Result<Value> {
    plan_file_with_tasks(
        workspace,
        project,
        city,
        cache,
        output,
        Arc::new(AddonPreparation::installed()),
    )
}
pub fn plan_file_with_tasks(
    workspace: &Path,
    project: &str,
    city: &str,
    cache: &Path,
    output: &Path,
    tasks: Arc<dyn PreparationTasks>,
) -> Result<Value> {
    ensure!(
        std::fs::symlink_metadata(output).is_err(),
        "reference request output already exists"
    );
    let ws = scoped_workspace(workspace, project)?;
    let input = ws.cities(&[city.to_owned()])?.remove(0);
    let mut result = plan_input(&ws, city, cache, input, tasks)?;
    files::json(output, &result["request"])?;
    result
        .as_object_mut()
        .expect("plan receipt")
        .remove("request");
    result["request_file"] = json!(output);
    Ok(result)
}
fn scoped_workspace(workspace: &Path, project: &str) -> Result<Workspace> {
    let ws = Workspace::open(workspace)?;
    ensure!(
        ws.identity.project_id == project,
        "Solar workspace belongs to another project"
    );
    Ok(ws)
}
pub fn import(
    workspace: &Path,
    city: &str,
    cache: &Path,
    expected: &str,
    payload: &Value,
) -> Result<Value> {
    import_with_tasks(
        workspace,
        city,
        cache,
        expected,
        payload,
        Arc::new(AddonPreparation::installed()),
    )
}
pub fn import_with_tasks(
    workspace: &Path,
    city: &str,
    cache: &Path,
    expected: &str,
    payload: &Value,
    tasks: Arc<dyn PreparationTasks>,
) -> Result<Value> {
    let ws = Workspace::open(workspace)?;
    let input = ws.cities(&[city.to_owned()])?.remove(0);
    ensure!(
        input.content_digest == expected,
        "Solar city changed during reference acquisition"
    );
    let bundle = ds_solar_host::delivery::bundle_from_delivery(&ws.identity.project_id, payload)?;
    commit_bundle(&ws, city, cache, expected, input, bundle, tasks)
}
/// Read only the four declared local artifacts after project/revision admission.
pub fn import_files(
    workspace: &Path,
    project: &str,
    city: &str,
    cache: &Path,
    expected: &str,
    directory: &Path,
) -> Result<Value> {
    import_files_with_tasks(
        workspace,
        project,
        city,
        cache,
        expected,
        directory,
        Arc::new(AddonPreparation::installed()),
    )
}
pub fn import_files_with_tasks(
    workspace: &Path,
    project: &str,
    city: &str,
    cache: &Path,
    expected: &str,
    directory: &Path,
    tasks: Arc<dyn PreparationTasks>,
) -> Result<Value> {
    let ws = scoped_workspace(workspace, project)?;
    let input = ws.cities(&[city.to_owned()])?.remove(0);
    ensure!(
        input.content_digest == expected,
        "Solar city changed during reference acquisition"
    );
    let bundle = ds_solar_reference_host::reference_import::read_bundle(directory)?;
    commit_bundle(&ws, city, cache, expected, input, bundle, tasks)
}
fn commit_bundle(
    ws: &Workspace,
    city: &str,
    cache: &Path,
    expected: &str,
    input: SolarCityInput,
    bundle: BundleBytes,
    tasks: Arc<dyn PreparationTasks>,
) -> Result<Value> {
    let verified = ds_solar_reference_host::reference_import::VerifiedBundle::verify_with_tasks(
        input,
        bundle,
        &timestamp(),
        tasks,
    )?;
    ensure!(
        ws.cities(&[city.to_owned()])?.remove(0).content_digest == expected,
        "Solar city changed during reference verification"
    );
    let committed = verified.commit(cache)?;
    let cache_root = cache.to_path_buf();
    let cache = DirectoryReferenceCache::new(cache);
    let reused = committed.reused;
    let bytes_count = committed.bytes;

    Ok(
        json!({"project":ws.identity.project_id,"city":city,"input_digest":expected,"ready":true,
        "acquired":!reused,"reused":reused,"bytes":bytes_count,"cache":cache_root,"versioned_cache":cache.versioned_root(),
        "prepared_input_digest":committed.prepared_input_digest}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProjectIdentity;

    fn workspace(root: &Path) -> (std::path::PathBuf, String) {
        let path = root.join("project");
        let mut ws = Workspace::initialize(
            &path,
            ProjectIdentity {
                project_id: "p".into(),
                root: "eds_project/p/eds_solar".into(),
            },
        )
        .unwrap();
        let snapshot = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../ds-solar/fixtures/city/aderm_bere.snapshot.json");
        let input = ds_solar_contracts::SolarCityInput::from_city_snapshot(
            ds_solar_contracts::CityIdentity {
                project_id: "p".into(),
                root: "eds_project/p/eds_solar".into(),
                city_id: "aderm_bere".into(),
                display_name: "Bere".into(),
            },
            ds_solar_contracts::SnapshotRevision {
                source: "test".into(),
                captured_at: timestamp(),
                record_updated_at_ms: None,
                revision: None,
            },
            ds_solar_contracts::RunOptions {
                system_types: vec![ds_solar_contracts::SystemType::Hybrid],
                language: "fr".into(),
                demand_noise_seed: None,
            },
            &serde_json::from_slice::<Value>(&std::fs::read(snapshot).unwrap()).unwrap(),
        )
        .unwrap();
        let expected = input.content_digest.clone();
        ws.seed(&[input], &std::collections::BTreeMap::new())
            .unwrap();
        (path, expected)
    }
    fn bundle(root: &Path, city: &str) -> std::path::PathBuf {
        let directory = root.join(format!("bundle-{city}"));
        std::fs::create_dir(&directory).unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../ds-solar/fixtures/reference")
            .join(city);
        for name in ds_solar_host::cache::ARTIFACT_NAMES {
            std::fs::copy(fixture.join(name), directory.join(name)).unwrap();
        }
        directory
    }
    #[test]
    fn local_plan_exports_the_native_request_without_acquiring_missing_data() {
        let temp = tempfile::tempdir().unwrap();
        let (workspace, expected) = workspace(temp.path());
        let cache = temp.path().join("cache");
        let output = temp.path().join("request.json");
        let result = plan_file(&workspace, "p", "aderm_bere", &cache, &output).unwrap();
        assert_eq!(result["ready"], false);
        assert_eq!(result["input_digest"], expected);
        let document: Value = serde_json::from_slice(&std::fs::read(&output).unwrap()).unwrap();
        assert_eq!(
            document,
            plan(&workspace, "aderm_bere", &cache).unwrap()["request"]
        );
        assert!(
            !cache.exists(),
            "planning must not acquire or populate a cache"
        );
        assert!(plan_file(&workspace, "p", "aderm_bere", &cache, &output).is_err());
    }
    #[test]
    fn local_import_preserves_scope_and_reuses_the_identical_verified_generation() {
        let temp = tempfile::tempdir().unwrap();
        let (workspace, expected) = workspace(temp.path());
        let directory = bundle(temp.path(), "aderm_bere");
        let cache_path = temp.path().join("cache");
        let first = import_files(
            &workspace,
            "p",
            "aderm_bere",
            &cache_path,
            &expected,
            &directory,
        )
        .unwrap();
        assert_eq!(first["ready"], true);
        assert_eq!(first["acquired"], true);
        let input = Workspace::open(&workspace)
            .unwrap()
            .cities(&["aderm_bere".into()])
            .unwrap()
            .remove(0);
        assert_eq!(
            input.content_digest, expected,
            "reference import never changes authored city inputs"
        );
        let key = ReferencePreparer::new().reference_key(&input).unwrap();
        let cache = DirectoryReferenceCache::new(&cache_path);
        let generation = cache.entry_dir(&key);
        let second = import_files(
            &workspace,
            "p",
            "aderm_bere",
            &cache_path,
            &expected,
            &directory,
        )
        .unwrap();
        assert_eq!(second["reused"], true);
        assert_eq!(second["acquired"], false);
        assert_eq!(
            second["prepared_input_digest"],
            first["prepared_input_digest"]
        );
        assert_eq!(cache.entry_dir(&key), generation);
        assert_eq!(
            plan(&workspace, "aderm_bere", &cache_path).unwrap()["ready"],
            true
        );
    }
    #[test]
    fn project_and_revision_refuse_before_any_local_artifact_read() {
        let temp = tempfile::tempdir().unwrap();
        let (workspace, expected) = workspace(temp.path());
        let cache = temp.path().join("cache");
        let missing = temp.path().join("never-read");
        assert!(
            import_files(
                &workspace,
                "other",
                "aderm_bere",
                &cache,
                &expected,
                &missing
            )
            .unwrap_err()
            .to_string()
            .contains("another project")
        );
        assert!(
            import_files(&workspace, "p", "aderm_bere", &cache, "stale", &missing)
                .unwrap_err()
                .to_string()
                .contains("changed")
        );
        assert!(!cache.exists());
    }
    #[test]
    fn wrong_site_and_tampered_local_bytes_never_populate_the_cache() {
        let temp = tempfile::tempdir().unwrap();
        let (workspace, expected) = workspace(temp.path());
        let cache = temp.path().join("cache");
        let wrong_site = bundle(temp.path(), "aderm_gagal");
        assert!(
            import_files(
                &workspace,
                "p",
                "aderm_bere",
                &cache,
                &expected,
                &wrong_site
            )
            .is_err()
        );
        let directory = bundle(temp.path(), "aderm_bere");
        let member = directory.join("reference-unit.json");
        let mut bytes = std::fs::read(&member).unwrap();
        bytes.push(b'\n');
        std::fs::write(member, bytes).unwrap();
        assert!(
            import_files(&workspace, "p", "aderm_bere", &cache, &expected, &directory).is_err()
        );
        assert!(!cache.exists());
    }
    #[test]
    fn altered_reference_artifact_cannot_populate_cache() {
        let temp = tempfile::tempdir().unwrap();
        let ws_path = temp.path().join("project");
        let mut ws = Workspace::initialize(
            &ws_path,
            ProjectIdentity {
                project_id: "p".into(),
                root: "eds_project/p/eds_solar".into(),
            },
        )
        .unwrap();
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../../ds-solar/fixtures/city/aderm_bere.snapshot.json");
        let snap: Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
        let city = ds_solar_contracts::SolarCityInput::from_city_snapshot(
            ds_solar_contracts::CityIdentity {
                project_id: "p".into(),
                root: "eds_project/p/eds_solar".into(),
                city_id: "aderm_bere".into(),
                display_name: "Bere".into(),
            },
            ds_solar_contracts::SnapshotRevision {
                source: "test".into(),
                captured_at: timestamp(),
                record_updated_at_ms: None,
                revision: None,
            },
            ds_solar_contracts::RunOptions {
                system_types: vec![ds_solar_contracts::SystemType::Hybrid],
                language: "fr".into(),
                demand_noise_seed: None,
            },
            &snap,
        )
        .unwrap();
        let digest = city.content_digest.clone();
        ws.seed(&[city], &std::collections::BTreeMap::new())
            .unwrap();
        let cache = temp.path().join("cache");
        assert_eq!(
            plan(&ws_path, "aderm_bere", &cache).unwrap()["ready"],
            false
        );
        let artifacts = [
            "weather.json",
            "reference-unit.json",
            "reference-unit.parquet",
            "manifest.json",
        ]
        .map(|name| json!({"name":name,"bytes":1,"sha256":"0".repeat(64),"base64":"eA=="}));
        let payload = json!({"project":"p","artifacts":artifacts});
        assert!(
            import(&ws_path, "aderm_bere", &cache, &digest, &payload)
                .unwrap_err()
                .to_string()
                .contains("digest/size")
        );
        assert!(!cache.exists());
        assert!(
            import(&ws_path, "aderm_bere", &cache, "stale", &payload)
                .unwrap_err()
                .to_string()
                .contains("changed")
        );
    }
}
