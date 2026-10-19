//! Installed Core retained Sync Center reads, independent of its daemon.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use ds_command_kernel::execution_context::Principal;
use ds_compute_runtime::HostIdentity;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

const MAX_BYTES: usize = 64 * 1024 * 1024;
const INVALID: Refusal = Refusal {
    code: "sync_read_invalid_input",
    when: "view, project or absolute output path is invalid",
    remedy: "select projects without project, or store with one exact project, and an absolute new output file",
};
const STORE: Refusal = Refusal {
    code: "sync_read_store_invalid",
    when: "the existing native store is corrupt, incompatible or unsafe",
    remedy: "retain the existing store and repair it through its native owner; this read never creates or repairs it",
};
const OUTPUT: Refusal = Refusal {
    code: "sync_read_output_invalid",
    when: "output exists, cannot be created privately or exceeds 64 MiB",
    remedy: "provide an absolute unused output file in a writable directory",
};
const CHANGED: Refusal = Refusal {
    code: "sync_read_identity_changed",
    when: "the protected account, deployment or installation changes during this read",
    remedy: "retry under the intended existing native identity",
};

pub static READ: Command = Command {
    id: "core.sync.read",
    path: &["core", "sync", "read"],
    contract: 1,
    summary: "Read retained Sync Center projects or one project's store offline.",
    purpose: "Read the existing lane's native SQLite store under its protected account, deployment and installation. No Core daemon, network, credential refresh, saved project, store initialization or repair is involved. Missing stores are valid empty views. Only the public native projection and retained compute activity are written to a new private file, bounded to 64 MiB; upload transport state and signed grant bytes remain private. Recheck the exact protected identity before acknowledging the file.",
    chapter: Chapter::Workstation,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        super::LANE,
        Arg::value(
            "view",
            "<projects|store>",
            "Retained project inventory or one exact project's public store projection.",
        )
        .choices(&["projects", "store"])
        .required(),
        super::PROJECT,
        Arg::value(
            "out",
            "<absolute-path>",
            "New owner-only JSON output file; an existing path is never replaced.",
        )
        .required(),
    ],
    output: "Receipt ds.sync-read-file/v1: path, sha256, size_bytes, lane, view, project (null for projects). File payload: ds.sync-projects/v1 with sorted unique projects, or the native ds.sync-store-read/v1 projection.",
    examples: &[],
    refusals: &[
        super::PLATFORM,
        INVALID,
        STORE,
        OUTPUT,
        CHANGED,
        super::CONTEXT_CORRUPT,
        super::PROJECT_REQUIRED,
    ],
    reference: Some("docs/reference/core.md"),
    search: &["offline", "retained", "sync", "projects"],
    requires: Requires::Server,
    availability: super::native_host_availability,
};
fn failure(refusal: Refusal, detail: impl ToString) -> Failure {
    Failure::failed(refusal.code, detail.to_string()).remedy(refusal.remedy)
}
fn selection(view: &str, project: Option<&str>) -> Result<Option<String>, Failure> {
    match (view, project) {
        ("projects", None) => Ok(None),
        ("store", Some(project)) => super::bounded_project(project).map(Some),
        ("store", None) => Err(Failure::invalid(
            super::PROJECT_REQUIRED.code,
            "store view requires --project",
        )
        .remedy(super::PROJECT_REQUIRED.remedy)),
        _ => Err(failure(
            INVALID,
            "projects forbids --project; view must be projects or store",
        )),
    }
}
fn identity(lane: &str) -> Result<HostIdentity, Failure> {
    let principal = ds_cli_auth::headless_principal_read_only(lane)?;
    Ok(HostIdentity {
        owner: principal.job_owner()?,
        principal: Principal {
            uid: principal.account_uid().into(),
            lane: lane.into(),
            deployment: principal.deployment().into(),
            install_id: principal.install_id().into(),
        },
    })
}
fn retained(
    database: &Path,
    identity: &HostIdentity,
    project: Option<&str>,
) -> Result<Value, Failure> {
    // Validate an existing file without tightening modes, repairing or creating it.
    match fs::symlink_metadata(database) {
        Ok(_) => {
            #[cfg(windows)]
            let file = ds_cli_auth::windows_host::read_live_store_file(database);
            #[cfg(not(windows))]
            let file = super::cache_recovery::private_file(database);
            drop(file.map_err(|e| failure(STORE, e))?);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(failure(STORE, error)),
    }
    let store = ds_sync_store::Store::open_read_only(database).map_err(|e| failure(STORE, e))?;
    match store.as_ref() {
        Some(store) => store
            .with_read_snapshot(|store| project_view(Some(store), identity, project))
            .map_err(|e| failure(STORE, e))?,
        None => project_view(None, identity, project),
    }
}
fn project_view(
    store: Option<&ds_sync_store::Store>,
    identity: &HostIdentity,
    project: Option<&str>,
) -> Result<Value, Failure> {
    let fence = ds_sync_runtime::solar_producer::fence_of(identity);
    match project {
        Some(project) => {
            let activities = match store {
                Some(store) => {
                    ds_sync_runtime::solar_producer::activity_from_store(store, identity, project)
                        .map_err(|e| failure(STORE, e))?
                }
                None => Vec::new(),
            };
            ds_sync_runtime::store_read_projection(
                store,
                &fence,
                project,
                activities,
                ds_sync_runtime::now_ms(),
            )
            .map_err(|e| failure(STORE, e))
        }
        None => {
            let mut projects = std::collections::BTreeSet::new();
            if let Some(store) = store {
                projects.extend(
                    store
                        .projects_of_fence(&fence)
                        .map_err(|e| failure(STORE, e))?,
                );
                projects.extend(
                    ds_sync_runtime::solar_producer::projects_from_store(store, identity)
                        .map_err(|e| failure(STORE, e))?,
                );
            }
            Ok(json!({"schema":"ds.sync-projects/v1", "projects":projects}))
        }
    }
}
struct BoundedBytes(Vec<u8>);
impl Write for BoundedBytes {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len().saturating_add(bytes.len()) > MAX_BYTES {
            return Err(std::io::Error::other("retained JSON exceeds 64 MiB"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
fn create_private(path: &Path) -> Result<fs::File, Failure> {
    #[cfg(windows)]
    {
        ds_cli_auth::windows_host::create_private_output_file(path).map_err(|e| failure(OUTPUT, e))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map_err(|e| failure(OUTPUT, e))
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(failure(OUTPUT, "private native files unavailable"))
    }
}
fn capture(
    database: &Path,
    lane: &str,
    view: &str,
    project: Option<&str>,
    out: &Path,
    observe: impl Fn() -> Result<HostIdentity, Failure>,
) -> Result<Value, Failure> {
    let project = selection(view, project)?;
    if !out.is_absolute() || out.to_str().is_none() {
        return Err(failure(INVALID, "--out must be an absolute Unicode path"));
    }
    let before = observe()?;
    let payload = retained(database, &before, project.as_deref())?;
    let mut bytes = BoundedBytes(Vec::new());
    serde_json::to_writer(&mut bytes, &payload).map_err(|e| failure(OUTPUT, e))?;
    let mut file = create_private(out)?;
    let result = (|| {
        file.write_all(&bytes.0)
            .and_then(|_| file.sync_all())
            .map_err(|e| failure(OUTPUT, e))?;
        if observe()? != before {
            return Err(failure(
                CHANGED,
                "protected native principal changed during read",
            ));
        }
        Ok(
            json!({"schema":"ds.sync-read-file/v1","path":out,"sha256":format!("{:x}",Sha256::digest(&bytes.0)),"size_bytes":bytes.0.len(),"lane":lane,"view":view,"project":project}),
        )
    })();
    drop(file);
    if result.is_err() {
        fs::remove_file(out).map_err(|e| {
            failure(
                OUTPUT,
                format!("failed to remove unacknowledged output: {e}"),
            )
        })?;
    }
    result
}
pub fn read(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    super::require_native_host()?;
    let lane = inputs.require("lane")?;
    let database = ds_compute_runtime::server_state_directory(lane, None)
        .map_err(|e| failure(INVALID, e))?
        .join("store.sqlite");
    capture(
        &database,
        lane,
        inputs.require("view")?,
        inputs.value("project"),
        &PathBuf::from(inputs.require("out")?),
        || identity(lane),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    #[cfg(windows)]
    struct FixtureDirectory(PathBuf);
    #[cfg(windows)]
    impl FixtureDirectory {
        fn path(&self) -> &Path {
            &self.0
        }
    }
    #[cfg(windows)]
    impl Drop for FixtureDirectory {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[cfg(windows)]
    fn private_temp() -> FixtureDirectory {
        let mut nonce = [0u8; 16];
        getrandom::getrandom(&mut nonce).unwrap();
        let nonce = nonce.iter().map(|b| format!("{b:02x}")).collect::<String>();
        // Retained stores still require a private parent; managed TEMP may
        // have foreign writers, so use the native current-user known folder.
        let path = ds_cli_auth::windows_host::local_application_data()
            .unwrap()
            .join(format!("ds-sync-read-test-{}-{nonce}", std::process::id()));
        ds_cli_auth::windows_host::prepare_directory(&path).unwrap();
        FixtureDirectory(path)
    }
    #[cfg(not(windows))]
    fn private_temp() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        dir
    }
    fn owner() -> HostIdentity {
        HostIdentity {
            owner: "owner".into(),
            principal: Principal {
                uid: "user".into(),
                lane: "stable".into(),
                deployment: "https://gateway.example".into(),
                install_id: "install".into(),
            },
        }
    }
    fn run(
        database: &Path,
        out: &Path,
        view: &str,
        project: Option<&str>,
    ) -> Result<Value, Failure> {
        capture(database, "stable", view, project, out, || Ok(owner()))
    }
    #[test]
    fn missing_store_is_empty_private_and_never_created() {
        let dir = private_temp();
        let db = dir.path().join("absent/store.sqlite");
        let out = dir.path().join("read.json");
        let receipt = run(&db, &out, "projects", None).unwrap();
        let bytes = fs::read(&out).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap(),
            json!({"schema":"ds.sync-projects/v1","projects":[]})
        );
        assert!(!db.parent().unwrap().exists());
        assert_eq!(receipt["sha256"], format!("{:x}", Sha256::digest(&bytes)));
        assert_eq!(receipt["size_bytes"], bytes.len());
        assert!(receipt["project"].is_null());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(fs::metadata(&out).unwrap().permissions().mode() & 0o077, 0);
        }
        let out2 = dir.path().join("store.json");
        run(&db, &out2, "store", Some("project")).unwrap();
        let data: Value = serde_json::from_slice(&fs::read(out2).unwrap()).unwrap();
        assert_eq!(data["schema"], "ds.sync-store-read/v1");
        assert_eq!(data["project"], "project");
        assert_eq!(data["activities"], json!([]));
        assert!(!db.exists());
    }
    #[test]
    fn invalid_selections_and_output_refuse_before_principal_or_creation() {
        let dir = private_temp();
        let db = dir.path().join("store.sqlite");
        let out = dir.path().join("read.json");
        for (view, project) in [
            ("other", None),
            ("store", None),
            ("projects", Some("project")),
            ("store", Some("../project")),
        ] {
            assert!(
                capture(&db, "stable", view, project, &out, || panic!(
                    "invalid request must not observe identity"
                ))
                .is_err()
            );
            assert!(!out.exists());
        }
        assert!(
            capture(
                &db,
                "stable",
                "projects",
                None,
                Path::new("relative"),
                || panic!("invalid path")
            )
            .is_err()
        );
    }
    #[test]
    fn output_is_create_new_and_identity_changes_remove_unacknowledged_bytes() {
        let dir = private_temp();
        let db = dir.path().join("store.sqlite");
        let out = dir.path().join("read.json");
        fs::write(&out, b"occupied").unwrap();
        assert!(run(&db, &out, "projects", None).is_err());
        assert_eq!(fs::read(&out).unwrap(), b"occupied");
        fs::remove_file(&out).unwrap();
        for field in ["uid", "deployment", "install"] {
            let calls = Cell::new(0);
            let error = capture(&db, "stable", "projects", None, &out, || {
                let mut identity = owner();
                if calls.replace(calls.get() + 1) > 0 {
                    match field {
                        "uid" => identity.principal.uid.push('x'),
                        "deployment" => identity.principal.deployment.push('x'),
                        _ => identity.principal.install_id.push('x'),
                    }
                }
                Ok(identity)
            })
            .unwrap_err();
            assert_eq!(error.code(), CHANGED.code);
            assert!(!out.exists());
            assert_eq!(calls.get(), 2);
        }
        let calls = Cell::new(0);
        assert!(
            capture(&db, "stable", "projects", None, &out, || {
                if calls.replace(calls.get() + 1) > 0 {
                    Err(failure(CHANGED, "signed out"))
                } else {
                    Ok(owner())
                }
            })
            .is_err()
        );
        assert!(!out.exists());
    }
    #[test]
    fn corrupt_and_incompatible_stores_are_refused_without_output_or_repair() {
        let dir = private_temp();
        let db = dir.path().join("store.sqlite");
        let out = dir.path().join("read.json");
        super::super::cache_recovery::create_private(&db, b"not SQLite").unwrap();
        let before = fs::read(&db).unwrap();
        assert_eq!(
            run(&db, &out, "projects", None).unwrap_err().code(),
            STORE.code
        );
        assert_eq!(fs::read(&db).unwrap(), before);
        assert!(!out.exists());
        fs::remove_file(&db).unwrap();
        drop(ds_sync_store::Store::open(&db).unwrap());
        let connection = rusqlite::Connection::open(&db).unwrap();
        connection
            .execute("UPDATE meta SET value=999 WHERE key='schema_version'", [])
            .unwrap();
        drop(connection);
        let before = fs::read(&db).unwrap();
        assert_eq!(
            run(&db, &out, "store", Some("project")).unwrap_err().code(),
            STORE.code
        );
        assert_eq!(fs::read(&db).unwrap(), before);
        assert!(!out.exists());
    }
    #[test]
    fn fenced_rows_and_public_projection_exclude_other_owners_and_transport_secrets() {
        use ds_sync_runtime::store::{ArtifactRow, Event, GrantRow, Scope};
        let dir = private_temp();
        let db = dir.path().join("store.sqlite");
        let out = dir.path().join("read.json");
        let mut store = ds_sync_store::Store::open(&db).unwrap();
        let fence = ds_sync_runtime::solar_producer::fence_of(&owner());
        let row:ArtifactRow=serde_json::from_value(json!({"scope":{"kind":"project","project":"project"},"engine":"network_reporter","operation":"report","variant":"main","sha256":"a".repeat(64),"size_bytes":10,"produced_at_ms":1,"engine_release":"r","client_publish_id":"publish","bytes_locator":"batch:1","state":"held","replay_key":"publish","updated_at_ms":1,"transfer_state":{"secret":"transport-secret"}})).unwrap();
        store
            .apply(&fence, 1, Event::LocalProduced { row: row.clone() })
            .unwrap();
        let grant = GrantRow {
            scope: row.scope.clone(),
            grant_id: "grant".into(),
            install_id: fence.install_id.clone(),
            engines: vec![],
            expires_at_ms: 999,
            raw: json!({"secret":"signed-grant-secret"}),
        };
        store
            .apply(&fence, 2, Event::GrantOpened { grant })
            .unwrap();
        for axis in ["account", "deployment", "install"] {
            let mut other = fence.clone();
            match axis {
                "account" => other.account.push('x'),
                "deployment" => other.deployment.push('x'),
                _ => other.install_id.push('x'),
            };
            let mut foreign = row.clone();
            foreign.scope = Scope::Project {
                project: format!("foreign-{axis}"),
            };
            store
                .apply(&other, 1, Event::LocalProduced { row: foreign })
                .unwrap();
        }
        drop(store);
        run(&db, &out, "projects", None).unwrap();
        let projects: Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
        assert_eq!(projects["projects"], json!(["project"]));
        fs::remove_file(&out).unwrap();
        run(&db, &out, "store", Some("project")).unwrap();
        let bytes = fs::read(&out).unwrap();
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(data["artifacts"].as_array().unwrap().len(), 1);
        assert!(data["artifacts"][0].get("transfer_state").is_none());
        assert_eq!(data["grants"][0]["grantId"], "grant");
        assert!(data["grants"][0].get("raw").is_none());
        let text = String::from_utf8(bytes).unwrap();
        assert!(!text.contains("transport-secret"));
        assert!(!text.contains("signed-grant-secret"));
        fs::remove_file(&out).unwrap();
        run(&db, &out, "store", Some("foreign-account")).unwrap();
        let data: Value = serde_json::from_slice(&fs::read(out).unwrap()).unwrap();
        assert_eq!(data["artifacts"], json!([]));
    }
    #[cfg(windows)]
    #[test]
    fn windows_retained_views_coexist_with_live_store_writer() {
        use ds_sync_runtime::store::{ArtifactRow, Event};
        let dir = private_temp();
        let db = dir.path().join("store.sqlite");
        let mut store = ds_sync_store::Store::open(&db).unwrap();
        let fence = ds_sync_runtime::solar_producer::fence_of(&owner());
        let row: ArtifactRow = serde_json::from_value(json!({
            "scope": {"kind": "project", "project": "project"},
            "engine": "network_reporter",
            "operation": "report",
            "variant": "main",
            "sha256": "a".repeat(64),
            "size_bytes": 10,
            "produced_at_ms": 1,
            "engine_release": "r",
            "client_publish_id": "publish",
            "bytes_locator": "batch:1",
            "state": "held",
            "replay_key": "publish",
            "updated_at_ms": 1,
            "transfer_state": {}
        }))
        .unwrap();
        store
            .apply(&fence, 1, Event::LocalProduced { row })
            .unwrap();

        // The credential reader retains its writer-excluding sharing fence.
        assert!(ds_cli_auth::windows_host::read_file(&db).is_err());
        let mut admission = ds_cli_auth::windows_host::read_live_store_file(&db).unwrap();
        assert!(admission.write_all(b"must not write").is_err());
        drop(admission);

        let projects_out = dir.path().join("projects.json");
        run(&db, &projects_out, "projects", None).unwrap();
        let projects: Value = serde_json::from_slice(&fs::read(projects_out).unwrap()).unwrap();
        assert_eq!(projects["projects"], json!(["project"]));
        let store_out = dir.path().join("store.json");
        run(&db, &store_out, "store", Some("project")).unwrap();
        let projection: Value = serde_json::from_slice(&fs::read(store_out).unwrap()).unwrap();
        assert_eq!(projection["project"], "project");
        assert_eq!(projection["artifacts"].as_array().unwrap().len(), 1);

        // Keep the actual writer connection live throughout both captures.
        assert_eq!(
            store.projects_of_fence(&fence).unwrap(),
            vec!["project".to_string()]
        );
        drop(store);
    }
    #[cfg(windows)]
    #[test]
    fn windows_retained_views_refuse_unsafe_store_files_and_acls_without_repair() {
        for unsafe_kind in ["hardlink", "file_acl", "parent_acl"] {
            let dir = private_temp();
            let db = dir.path().join("store.sqlite");
            drop(ds_sync_store::Store::open(&db).unwrap());
            match unsafe_kind {
                "hardlink" => fs::hard_link(&db, dir.path().join("alias.sqlite")).unwrap(),
                _ => {
                    let path = if unsafe_kind == "file_acl" {
                        db.as_path()
                    } else {
                        dir.path()
                    };
                    let result = std::process::Command::new("icacls.exe")
                        .arg(path)
                        .args(["/grant", "*S-1-1-0:(R)"])
                        .output()
                        .unwrap();
                    assert!(result.status.success(), "native ACL fixture failed");
                }
            }
            let before = fs::read(&db).unwrap();
            let out = dir.path().join("must-not-exist.json");
            for (view, project) in [("projects", None), ("store", Some("project"))] {
                assert_eq!(
                    run(&db, &out, view, project).unwrap_err().code(),
                    STORE.code,
                    "{unsafe_kind}"
                );
                assert!(!out.exists());
                assert_eq!(fs::read(&db).unwrap(), before);
                assert!(
                    ds_cli_auth::windows_host::read_live_store_file(&db).is_err(),
                    "refusal must preserve the unsafe file or ACL"
                );
            }
        }
    }
    #[test]
    fn retained_solar_jobs_are_real_activity_and_install_fenced() {
        use ds_command_kernel::{
            compute_jobs::{EngineKind, EngineLane, EnginePin, EnginePinSource, Job, Phase},
            execution_context::{ExecutionContext, Limits, SOLAR_PROCESSING},
        };
        let dir = private_temp();
        let db = dir.path().join("store.sqlite");
        let mut store = ds_sync_store::Store::open(&db).unwrap();
        store.initialize_jobs().unwrap();
        let fixture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/legacy-queue.sqlite");
        let fixture = rusqlite::Connection::open_with_flags(
            fixture,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let input: Vec<u8> = fixture
            .query_row(
                "SELECT input FROM compute_jobs WHERE row LIKE '%solar_prepared%' LIMIT 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let request: Value = serde_json::from_slice(&input).unwrap();
        let project = request["prepared"]["city"]["identity"]["project_id"]
            .as_str()
            .unwrap();
        let identity = owner();
        for (index, install) in ["install", "foreign-install"].into_iter().enumerate() {
            let id = format!("{}", index + 1).repeat(64);
            let input_sha256 = ds_compute_runtime::digest(&input);
            let context = ExecutionContext {
                principal_uid: identity.principal.uid.clone(),
                lane: "stable".into(),
                deployment: identity.principal.deployment.clone(),
                install_id: install.into(),
                project: project.into(),
                client: "cli:test".into(),
                operation: SOLAR_PROCESSING.into(),
                job_id: id.clone(),
                idempotency_key: format!("job-{index}"),
                input_sha256: input_sha256.clone(),
                admitted_at_ms: 10 + index as u64,
            };
            let pin = EnginePin {
                schema: "ds.compute.engine-pin/v1".into(),
                engine: EngineKind::SolarPrepared,
                owner: identity.owner.clone(),
                lane: EngineLane::Stable,
                context: context.clone(),
                source: EnginePinSource::NativeLaneCapture,
                executable: "/native-owned/immutable-test-engine".into(),
                executable_sha256: "a".repeat(64),
                identity: json!({"storage_test":true}),
                workers: 1,
                development_override: true,
            };
            let job = Job {
                id,
                owner: identity.owner.clone(),
                lane: "stable".into(),
                input_sha256,
                engine: EngineKind::SolarPrepared,
                input_tag: "ds.solar.calculate.prepared/v1".into(),
                phase: Phase::Queued,
                attempts: 0,
                created_at_ms: 10 + index as u64,
                updated_at_ms: 10 + index as u64,
                worker: None,
                lease_until_ms: 0,
                result_sha256: None,
                error: None,
                engine_pin: Some(pin),
                context: Some(context),
            };
            store
                .submit_job(
                    &job,
                    &input,
                    Limits {
                        global_running: 8,
                        per_project_running: 4,
                        per_project_queued: 16,
                        global_queued: 64,
                    },
                )
                .unwrap();
        }
        drop(store);
        let out = dir.path().join("activity.json");
        run(&db, &out, "store", Some(project)).unwrap();
        let data: Value = serde_json::from_slice(&fs::read(&out).unwrap()).unwrap();
        assert_eq!(data["activities"].as_array().unwrap().len(), 1);
        assert_eq!(data["activities"][0]["id"], "1".repeat(64));
        assert_eq!(data["activities"][0]["state"], "queued");
        fs::remove_file(&out).unwrap();
        run(&db, &out, "projects", None).unwrap();
        let data: Value = serde_json::from_slice(&fs::read(out).unwrap()).unwrap();
        assert_eq!(data["projects"], json!([project]));
    }
    #[test]
    fn bounded_serialization_refuses_overflow() {
        let mut writer = BoundedBytes(vec![0; MAX_BYTES]);
        assert!(writer.write_all(b"x").is_err());
        assert_eq!(writer.0.len(), MAX_BYTES);
    }
}
