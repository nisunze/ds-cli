//! One exact native MV disk job in the existing account/project preview room.
//! The fixed Reporter adapter launches it; intentional consumers read status
//! and only materialize the ready page they actually select.
use crate::{
    mv_artifact, mv_preview::MvPreviewInputs, preview_artifact, HostFailure, ReportEngine,
};
use ds_command_kernel::printing::{mv_preview, preview_progress};
use ds_project_data::preview_room::Room;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const SCHEMA: &str = "ds.mv-preview-job/v1";
const MAX_JOBS: usize = 16;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub schema: String,
    pub project: String,
    pub authority_sha256: String,
    pub input_sha256: String,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub retention_ms: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    job: Job,
    inputs: Value,
    model_revision: String,
    layout_id: String,
    setup_sha256: String,
    request_sha256: String,
    error: Option<String>,
}

fn failure(message: impl Into<String>) -> HostFailure {
    HostFailure::new("print_preview_artifact_unavailable", message)
}
fn clock() -> Result<u64, HostFailure> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| failure(e.to_string()))?
        .as_millis()
        .try_into()
        .map_err(|_| failure("native preview clock exceeds its bound"))
}
fn base(room: &Room) -> Result<PathBuf, HostFailure> {
    let path = ds_project_data::room_hold::room_dir(&room.root, &room.scope)
        .join("preview-mv-jobs")
        .join(preview_artifact::authority(room)?);
    for ancestor in path.ancestors().take_while(|p| p.starts_with(&room.root)) {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
            Ok(_) => return Err(failure("native MV preview root is crossed")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(failure(e.to_string())),
        }
    }
    Ok(path)
}
fn directory(room: &Room, key: &str) -> Result<PathBuf, HostFailure> {
    if !preview_progress::digest(key) {
        return Err(failure("native MV job digest is invalid"));
    }
    Ok(base(room)?.join(key))
}
fn load(
    room: &Room,
    key: &str,
    clock: u64,
    expired: bool,
) -> Result<(Control, PathBuf), HostFailure> {
    let path = directory(room, key)?;
    for ancestor in path.ancestors().take_while(|p| p.starts_with(&room.root)) {
        let metadata = fs::symlink_metadata(ancestor).map_err(|e| failure(e.to_string()))?;
        if !metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err(failure(
                "native MV job ancestor is not a real private directory",
            ));
        }
    }
    let metadata = fs::symlink_metadata(&path).map_err(|e| failure(e.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(failure("native MV job is not a real private directory"));
    }
    let bytes = crate::read_bounded(
        &path.join("control.json"),
        preview_progress::MAX_MANIFEST_BYTES,
    )?
    .ok_or_else(|| failure("native MV job metadata is missing"))?;
    let control: Control = serde_json::from_slice(&bytes).map_err(|e| failure(e.to_string()))?;
    if control.job.schema != SCHEMA
        || control.job.project != room.scope.project
        || control.job.authority_sha256 != preview_artifact::authority(room)?
        || control.job.input_sha256 != key
        || control.inputs["project"] != room.scope.project
        || control.inputs["schema"] != SCHEMA
        || control.inputs["retention_ms"] != control.job.retention_ms
        || !preview_progress::digest(&control.request_sha256)
        || control.model_revision.is_empty()
        || control.model_revision.len() > 1024
        || control.layout_id.is_empty()
        || control.layout_id.len() > 256
        || !ds_command_kernel::printing::mv::valid_digest(&control.setup_sha256)
        || control
            .error
            .as_ref()
            .is_some_and(|error| error.is_empty() || error.len() > 8192)
        || ds_command_kernel::report_export::sha256_hex(
            &serde_json::to_vec(&control.inputs).map_err(|e| failure(e.to_string()))?,
        ) != key
        || ![
            preview_artifact::RETENTION_MS,
            preview_artifact::LONG_RETENTION_MS,
        ]
        .contains(&control.job.retention_ms)
        || control
            .job
            .created_at_unix_ms
            .checked_add(control.job.retention_ms)
            != Some(control.job.expires_at_unix_ms)
    {
        return Err(failure(
            "native MV job differs from its exact captured authority or inputs",
        ));
    }
    if !expired && clock >= control.job.expires_at_unix_ms {
        return Err(HostFailure::new(
            "print_preview_artifact_expired",
            "Native MV preview job expired; request the exact preview again.",
        ));
    }
    Ok((control, path))
}

/// Admit once and retain durable inputs before starting the fixed native task.
/// An exact repeated request joins the same job without launching an engine.
pub fn start(
    engine: &dyn ReportEngine,
    inputs: &MvPreviewInputs<'_>,
    room: &Room,
    retention: preview_artifact::Retention,
) -> Result<Value, crate::mv_preview::MvPreviewRefusal> {
    start_bound(engine, inputs, room, retention, None)
}

pub fn start_bound(
    engine: &dyn ReportEngine,
    inputs: &MvPreviewInputs<'_>,
    room: &Room,
    retention: preview_artifact::Retention,
    source: Option<&ds_command_kernel::printing::mv_models::SourceBinding>,
) -> Result<Value, crate::mv_preview::MvPreviewRefusal> {
    crate::mv_preview::validate_source(inputs, source)?;
    if room.scope.project != inputs.project_id {
        return Err(failure("MV job belongs to another project").into());
    }
    for (name, bytes) in [("scene", inputs.scene), ("plan", inputs.plan)] {
        if bytes.is_empty() || bytes.len() > crate::mv_preview::MAX_PROJECTION_BYTES {
            return Err(failure(format!(
                "MV {name} projection is empty or exceeds its native bound"
            ))
            .into());
        }
    }
    let now = clock()?;
    prune_at(room, now, 16)?;
    let identity = crate::engine_identity_now(engine)?;
    let scene: Value = serde_json::from_slice(inputs.scene).map_err(|e| failure(e.to_string()))?;
    let revision = scene["model_revision"]
        .as_str()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| failure("native MV scene has no exact model revision"))?;
    let lifetime = match retention {
        preview_artifact::Retention::Hours24 => preview_artifact::RETENTION_MS,
        preview_artifact::Retention::Days7 => preview_artifact::LONG_RETENTION_MS,
    };
    let key_inputs = json!({"source_binding":source,"schema":SCHEMA,"project":inputs.project_id,"layout":inputs.layout,
        "sheets":inputs.sheets,"styles":inputs.table,"project_crs":inputs.project_crs,
        "model_crs":inputs.model_crs,"engine":identity,"retention_ms":lifetime,
        "scene_sha256":ds_command_kernel::report_export::sha256_hex(inputs.scene),
        "plan_sha256":ds_command_kernel::report_export::sha256_hex(inputs.plan)});
    let key = ds_command_kernel::report_export::sha256_hex(
        &serde_json::to_vec(&key_inputs).map_err(|e| failure(e.to_string()))?,
    );
    let root = base(room)?;
    let _launch = ds_layer_store::exclusive_lock(&root.join("launch.lock")).map_err(failure)?;
    let _flight =
        ds_layer_store::exclusive_lock(&root.join(format!("{key}.lock"))).map_err(failure)?;
    let destination = directory(room, &key)?;
    if fs::symlink_metadata(&destination).is_ok() {
        return read_locked(room, &key, Some(1), now).map_err(Into::into);
    }
    let retained = fs::read_dir(&root)
        .map_err(|e| failure(e.to_string()))?
        .filter_map(Result::ok)
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(preview_progress::digest)
        })
        .count();
    if retained >= MAX_JOBS {
        return Err(failure("The native MV preview room is full; expire admitted completed jobs before starting another preview.").into());
    }
    let mut active = 0;
    for entry in fs::read_dir(&root).map_err(|e| failure(e.to_string()))? {
        let entry = entry.map_err(|e| failure(e.to_string()))?;
        let name = entry.file_name();
        let Some(key) = name.to_str().filter(|key| preview_progress::digest(key)) else {
            continue;
        };
        let Ok((control, path)) = load(room, key, now, false) else {
            continue;
        };
        let progress = read_locked(room, key, None, now)?;
        let stage = progress["progress"]["stage"].as_str();
        let producer = path.join("producer.lock");
        let running = if producer.exists() {
            let metadata = fs::symlink_metadata(&producer).map_err(|e| failure(e.to_string()))?;
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                return Err(failure("MV producer lock is crossed").into());
            }
            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(producer)
                .map_err(|e| failure(e.to_string()))?;
            file.try_lock().is_err()
        } else {
            false
        };
        if running || (control.error.is_none() && !matches!(stage, Some("complete" | "failed"))) {
            active += 1;
        }
    }
    if active >= 2 {
        return Err(HostFailure::new("print_preview_job_capacity","Two native MV preview jobs are active; consume a ready page or wait for one to finish before starting another.").into());
    }
    let staged = crate::Staging {
        path: root.join(format!(".stage-{}", crate::random_run_id())),
    };
    crate::create_private_dir(&staged.path)?;
    let paths = mv_preview::Paths {
        scene: &destination.join("scene.json"),
        plan: &destination.join("plan.json"),
        out_dir: &destination.join("output"),
    };
    let preview = mv_preview::request(
        inputs.project_id,
        inputs.sheets,
        inputs.layout,
        inputs.table,
        inputs.project_crs,
        inputs.model_crs,
        paths,
    )
    .map_err(crate::mv_preview::MvPreviewRefusal::Kernel)?;
    let target = preview_progress::Target {
        directory: destination.join("progress"),
        input_sha256: key.clone(),
    };
    let preview = mv_preview::all_pages(preview, &target)
        .map_err(crate::mv_preview::MvPreviewRefusal::Kernel)?;
    let request_bytes = serde_json::to_vec(&preview.request).map_err(|e| failure(e.to_string()))?;
    let control = Control {
        job: Job {
            schema: SCHEMA.into(),
            project: inputs.project_id.into(),
            authority_sha256: preview_artifact::authority(room)?,
            input_sha256: key.clone(),
            created_at_unix_ms: now,
            expires_at_unix_ms: now
                .checked_add(lifetime)
                .ok_or_else(|| failure("MV preview expiry exceeds its bound"))?,
            retention_ms: lifetime,
        },
        inputs: key_inputs,
        model_revision: revision.into(),
        layout_id: inputs.layout.id.clone(),
        setup_sha256: preview.setup.sha256,
        request_sha256: ds_command_kernel::report_export::sha256_hex(&request_bytes),
        error: None,
    };
    for (name, bytes) in [("scene.json", inputs.scene), ("plan.json", inputs.plan)] {
        if bytes.is_empty() || bytes.len() > crate::mv_preview::MAX_PROJECTION_BYTES {
            return Err(failure("MV projection is empty or exceeds its native bound").into());
        }
        crate::write_new_private(&staged.path.join(name), bytes)?;
    }
    for (name, value) in [
        ("request.json", preview.request),
        (
            "control.json",
            serde_json::to_value(&control).map_err(|e| failure(e.to_string()))?,
        ),
    ] {
        let bytes = serde_json::to_vec(&value).map_err(|e| failure(e.to_string()))?;
        if bytes.len() as u64 > preview_progress::MAX_MANIFEST_BYTES {
            return Err(failure("Native MV job metadata exceeds its bounded manifest.").into());
        }
        crate::write_new_private(&staged.path.join(name), &bytes)?;
    }
    crate::promote_report(&staged.path, &destination)?;
    let launch = crate::engine_identity_now(engine).and_then(|current| {
        if current != identity {
            return Err(HostFailure::new(
                crate::CODE_ENGINE_IDENTITY_CHANGED,
                "The Reporter build changed before its native preview job could start.",
            ));
        }
        engine.launch_grid_preview(
            &destination.join("request.json"),
            &destination.join("result.json"),
            &key,
        )
    });
    if let Err(error) = launch {
        let mut failed = control;
        failed.error = Some(error.to_string().chars().take(2048).collect());
        ds_project_data::private::write_atomic(
            &destination.join("control.json"),
            serde_json::to_vec(&failed).map_err(|e| failure(e.to_string()))?,
        )
        .map_err(|e| failure(e.to_string()))?;
        return Err(error.into());
    }
    read_locked(room, &key, Some(1), now).map_err(Into::into)
}

/// Cleanup is an explicit native consumption action, never a timer or remote
/// freshness read. Active producers and any foreign file remain intact.
pub fn prune_expired(room: &Room, limit: usize) -> Result<usize, HostFailure> {
    prune_at(room, clock()?, limit)
}
fn prune_at(room: &Room, clock: u64, limit: usize) -> Result<usize, HostFailure> {
    if limit == 0 || limit > 128 {
        return Err(failure("native MV cleanup limit must be 1..128"));
    }
    let root = base(room)?;
    let entries = match fs::read_dir(&root) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(failure(e.to_string())),
    };
    let mut checked = 0;
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|e| failure(e.to_string()))?;
        let name = entry.file_name();
        let Some(key) = name.to_str().filter(|s| preview_progress::digest(s)) else {
            continue;
        };
        checked += 1;
        if checked > limit {
            break;
        }
        let _flight =
            ds_layer_store::exclusive_lock(&root.join(format!("{key}.lock"))).map_err(failure)?;
        let Ok((control, path)) = load(room, key, clock, true) else {
            continue;
        };
        if clock < control.job.expires_at_unix_ms {
            continue;
        }
        let producer = path.join("producer.lock");
        let lock = if producer.exists() {
            let Ok(metadata) = fs::symlink_metadata(&producer) else {
                continue;
            };
            if !metadata.is_file() || metadata.file_type().is_symlink() {
                continue;
            }
            let Ok(file) = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(&producer)
            else {
                continue;
            };
            if file.try_lock().is_err() {
                continue;
            }
            Some(file)
        } else {
            None
        };
        let Ok((files, mut directories)) = owned_inventory(&control, &path) else {
            continue;
        };
        // No deletion starts until every name and content fence has passed.
        // Removing exact names rather than a recursive tree never deletes a
        // foreign file inserted after admission; an added name prevents rmdir.
        drop(lock);
        for file in files {
            fs::remove_file(file).map_err(|e| failure(e.to_string()))?;
        }
        directories.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
        for directory in directories {
            fs::remove_dir(directory).map_err(|e| failure(e.to_string()))?;
        }
        removed += 1;
    }
    Ok(removed)
}

fn names(path: &Path) -> Result<BTreeSet<std::ffi::OsString>, HostFailure> {
    let metadata = fs::symlink_metadata(path).map_err(|e| failure(e.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(failure("MV inventory directory is crossed"));
    }
    fs::read_dir(path)
        .map_err(|e| failure(e.to_string()))?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name())
                .map_err(|e| failure(e.to_string()))
        })
        .collect()
}
fn owned_inventory(
    control: &Control,
    path: &Path,
) -> Result<(Vec<PathBuf>, Vec<PathBuf>), HostFailure> {
    let mut expected: BTreeSet<std::ffi::OsString> =
        ["control.json", "scene.json", "plan.json", "request.json"]
            .map(Into::into)
            .into_iter()
            .collect();
    let mut files = expected
        .iter()
        .map(|name| path.join(name))
        .collect::<Vec<_>>();
    let mut directories = vec![path.to_owned()];
    for (name, key) in [("scene.json", "scene_sha256"), ("plan.json", "plan_sha256")] {
        let file = path.join(name);
        let metadata = fs::symlink_metadata(&file).map_err(|e| failure(e.to_string()))?;
        if metadata.len() == 0 || metadata.len() > crate::mv_preview::MAX_PROJECTION_BYTES as u64 {
            return Err(failure("owned MV projection exceeds its bound"));
        }
        let sha = control.inputs[key]
            .as_str()
            .filter(|s| preview_progress::digest(s))
            .ok_or_else(|| failure("owned MV projection digest is absent"))?;
        crate::verify_file(&file, metadata.len(), sha)?;
    }
    let request = crate::read_bounded(
        &path.join("request.json"),
        preview_progress::MAX_MANIFEST_BYTES,
    )?
    .ok_or_else(|| failure("MV request disappeared"))?;
    if ds_command_kernel::report_export::sha256_hex(&request) != control.request_sha256 {
        return Err(failure("owned MV request changed"));
    }
    let target = preview_progress::Target {
        directory: path.join("progress"),
        input_sha256: control.job.input_sha256.clone(),
    };
    let manifest = crate::read_bounded(
        &target.directory.join("manifest.json"),
        preview_progress::MAX_MANIFEST_BYTES,
    )?
    .map(|bytes| serde_json::from_slice::<preview_progress::Manifest>(&bytes))
    .transpose()
    .map_err(|e| failure(e.to_string()))?;
    if let Some(manifest) = &manifest {
        manifest
            .validate(
                &target,
                &control.job.project,
                &control.model_revision,
                &control.setup_sha256,
            )
            .map_err(failure)?;
    }
    if target.directory.exists() {
        let mut progress_names = BTreeSet::new();
        if manifest.is_some() {
            progress_names.insert("manifest.json".into());
            files.push(target.directory.join("manifest.json"));
        }
        if let Some(failed) =
            crate::read_bounded(&target.directory.join("failure.json"), 16 * 1024)?
        {
            let failed: preview_progress::Failure =
                serde_json::from_slice(&failed).map_err(|e| failure(e.to_string()))?;
            failed
                .validate(&target, &control.job.project)
                .map_err(failure)?;
            progress_names.insert("failure.json".into());
            files.push(target.directory.join("failure.json"));
        }
        if let Some(manifest) = &manifest {
            for page in &manifest.pages {
                crate::verify_file(&page.path, page.size_bytes, &page.sha256)?;
                progress_names.insert(
                    page.path
                        .file_name()
                        .ok_or_else(|| failure("MV page has no filename"))?
                        .to_owned(),
                );
                files.push(page.path.clone());
            }
        }
        if names(&target.directory)? != progress_names {
            return Err(failure("MV progress contains foreign files"));
        }
        expected.insert("progress".into());
        directories.push(target.directory.clone());
    }
    let result = crate::read_bounded(&path.join("result.json"), 8 * 1024 * 1024)?
        .map(|bytes| serde_json::from_slice::<Value>(&bytes))
        .transpose()
        .map_err(|e| failure(e.to_string()))?;
    if let Some(result) = &result {
        if result["project_id"] != control.job.project
            || result["model_revision"] != control.model_revision
            || result["mv_setup_sha256"] != control.setup_sha256
            || result["svg_only"] != true
            || manifest.as_ref().is_none_or(|manifest| {
                manifest.stage != preview_progress::Stage::Complete
                    || result["page_count"] != manifest.completed
                    || result["previews"]
                        .as_array()
                        .is_none_or(|pages| pages.len() != manifest.completed)
            })
        {
            return Err(failure("MV final result is crossed"));
        }
        expected.insert("result.json".into());
        files.push(path.join("result.json"));
    }
    let output = path.join("output");
    if output.exists() {
        let result = result
            .as_ref()
            .ok_or_else(|| failure("MV output has no final native receipt"))?;
        let manifest = manifest
            .as_ref()
            .ok_or_else(|| failure("MV output has no ordered native page inventory"))?;
        if names(&output)?
            != ["manifest.json", "booklet-plan.json", "preview"]
                .map(Into::into)
                .into_iter()
                .collect()
        {
            return Err(failure("MV output contains foreign files"));
        }
        for (name, value) in [
            ("manifest.json", result),
            ("booklet-plan.json", &result["booklet_plan"]),
        ] {
            let bytes = crate::read_bounded(&output.join(name), 8 * 1024 * 1024)?
                .ok_or_else(|| failure("MV output metadata is absent"))?;
            if serde_json::from_slice::<Value>(&bytes).map_err(|e| failure(e.to_string()))?
                != *value
            {
                return Err(failure("MV output metadata changed"));
            }
            files.push(output.join(name));
        }
        let preview = output.join("preview");
        let mut preview_names = BTreeSet::new();
        for (index, page) in manifest.pages.iter().enumerate() {
            let name = format!("sheet-{:03}.svg", index + 1);
            let file = preview.join(&name);
            if result["previews"][index] != file.to_string_lossy().as_ref() {
                return Err(failure("MV output path inventory changed"));
            }
            crate::verify_file(&file, page.size_bytes, &page.sha256)?;
            preview_names.insert(name.into());
            files.push(file);
        }
        if names(&preview)? != preview_names {
            return Err(failure("MV output preview contains foreign files"));
        }
        directories.extend([preview, output]);
        expected.insert("output".into());
    }
    for entry in names(path)? {
        let name = entry.to_string_lossy();
        if name == "producer.lock" {
            let metadata =
                fs::symlink_metadata(path.join(&entry)).map_err(|e| failure(e.to_string()))?;
            if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != 0 {
                return Err(failure("MV producer lock changed"));
            }
            expected.insert(entry.clone());
            files.push(path.join(entry));
        } else if let Some(number) = name
            .strip_prefix("selected-")
            .and_then(|s| s.strip_suffix(".json"))
            .and_then(|s| s.parse::<usize>().ok())
        {
            let source = manifest
                .as_ref()
                .and_then(|m| number.checked_sub(1).and_then(|n| m.pages.get(n)))
                .ok_or_else(|| failure("selected MV receipt has no native page"))?;
            let bytes = crate::read_bounded(&path.join(&entry), 64 * 1024)?
                .ok_or_else(|| failure("selected MV receipt is missing"))?;
            let artifact: preview_artifact::Artifact =
                serde_json::from_slice(&bytes).map_err(|e| failure(e.to_string()))?;
            if artifact.sha256 != source.sha256
                || artifact.size_bytes != source.size_bytes
                || artifact.authority_sha256 != control.job.authority_sha256
            {
                return Err(failure("selected MV receipt is crossed"));
            }
            expected.insert(entry.clone());
            files.push(path.join(entry));
        }
    }
    if names(path)? != expected {
        return Err(failure(
            "MV job contains foreign files; expiration left all files intact",
        ));
    }
    Ok((files, directories))
}

/// Read one intentional job consumer. No engine, API or freshness request is
/// made here, and unselected SVG page bytes are not read or copied.
pub fn read(room: &Room, key: &str, selected: Option<usize>) -> Result<Value, HostFailure> {
    if !preview_progress::digest(key) {
        return Err(failure("native MV job digest is invalid"));
    }
    let root = base(room)?;
    let _flight =
        ds_layer_store::exclusive_lock(&root.join(format!("{key}.lock"))).map_err(failure)?;
    read_locked(room, key, selected, clock()?)
}
/// A fixed native adapter records process termination under its admitted job
/// scope. No consumer-supplied path or rendering occurs here.
pub fn mark_failed(room: &Room, key: &str, message: &str) -> Result<(), HostFailure> {
    if !preview_progress::digest(key) || message.is_empty() {
        return Err(failure("native MV failure locator or message is invalid"));
    }
    let root = base(room)?;
    let _flight =
        ds_layer_store::exclusive_lock(&root.join(format!("{key}.lock"))).map_err(failure)?;
    let (mut control, path) = load(room, key, clock()?, true)?;
    control.error = Some(message.chars().take(2048).collect());
    ds_project_data::private::write_atomic(
        &path.join("control.json"),
        serde_json::to_vec(&control).map_err(|e| failure(e.to_string()))?,
    )
    .map_err(|e| failure(e.to_string()))
}

fn read_locked(
    room: &Room,
    key: &str,
    selected: Option<usize>,
    clock: u64,
) -> Result<Value, HostFailure> {
    let (control, path) = load(room, key, clock, false)?;
    let target = preview_progress::Target {
        directory: path.join("progress"),
        input_sha256: key.into(),
    };
    let bytes = crate::read_bounded(
        &target.directory.join("manifest.json"),
        preview_progress::MAX_MANIFEST_BYTES,
    )?;
    let manifest: Option<preview_progress::Manifest> = bytes
        .map(|bytes| serde_json::from_slice(&bytes))
        .transpose()
        .map_err(|e| failure(e.to_string()))?;
    if let Some(manifest) = &manifest {
        manifest
            .validate(
                &target,
                &control.job.project,
                &control.model_revision,
                &control.setup_sha256,
            )
            .map_err(failure)?;
    }
    let failed = crate::read_bounded(&target.directory.join("failure.json"), 16 * 1024)?
        .map(|bytes| serde_json::from_slice::<preview_progress::Failure>(&bytes))
        .transpose()
        .map_err(|e| failure(e.to_string()))?;
    if let Some(failed) = &failed {
        failed
            .validate(&target, &control.job.project)
            .map_err(failure)?;
    }
    let error = control
        .error
        .clone()
        .or_else(|| failed.map(|failed| failed.error))
        .or_else(|| {
            manifest
                .as_ref()
                .and_then(|manifest| manifest.error.clone())
        });
    let mut page = None;
    if let Some(number) = selected {
        if number == 0 || number > preview_progress::MAX_PAGES {
            return Err(failure(
                "MV preview page number is outside its native bound",
            ));
        }
        if let Some(source) = manifest
            .as_ref()
            .and_then(|manifest| manifest.pages.get(number - 1))
        {
            let selected_path = path.join(format!("selected-{number:04}.json"));
            let held = crate::read_bounded(&selected_path, 64 * 1024)?
                .and_then(|bytes| serde_json::from_slice::<preview_artifact::Artifact>(&bytes).ok())
                .filter(|artifact| {
                    artifact.sha256 == source.sha256
                        && artifact.size_bytes == source.size_bytes
                        && preview_artifact::admit_receipt(room, artifact).is_ok()
                });
            let artifact = if let Some(held) = held {
                held
            } else {
                crate::verify_file(&source.path, source.size_bytes, &source.sha256)?;
                let bytes = crate::read_bounded(&source.path, source.size_bytes)?
                    .ok_or_else(|| failure("ready native MV page is missing"))?;
                if ds_command_kernel::report_export::sha256_hex(&bytes) != source.sha256 {
                    return Err(failure("ready native MV page changed while consuming"));
                }
                let svg = std::str::from_utf8(&bytes).map_err(|e| failure(e.to_string()))?;
                let retention = if control.job.retention_ms == preview_artifact::LONG_RETENTION_MS {
                    preview_artifact::Retention::Days7
                } else {
                    preview_artifact::Retention::Hours24
                };
                let artifact = preview_artifact::materialize_svg_retained(
                    room,
                    svg,
                    json!({"job":control.job,
                    "model_revision":control.model_revision,"layout_id":control.layout_id,"setup_sha256":control.setup_sha256,
                    "source_sheet_number":source.source_sheet_number,"alignment_id":source.alignment_id,
                    "from_station_m":source.from_station_m,"to_station_m":source.to_station_m}),
                    retention,
                )?;
                ds_project_data::private::write_atomic(
                    &selected_path,
                    serde_json::to_vec(&artifact).map_err(|e| failure(e.to_string()))?,
                )
                .map_err(|e| failure(e.to_string()))?;
                artifact
            };
            page = Some(mv_artifact::Sheet {
                source_sheet_number: source.source_sheet_number as u64,
                alignment_id: source.alignment_id.clone(),
                from_station_m: source.from_station_m,
                to_station_m: source.to_station_m,
                page: artifact,
            });
        }
    }
    Ok(
        json!({"schema":SCHEMA,"project":control.job.project,"job":control.job,
        "layout_id":control.layout_id,"model_revision":control.model_revision,"setup_sha256":control.setup_sha256,
        "progress":manifest.as_ref().map(|manifest|json!({"completed":manifest.completed,"total":manifest.total,"stage":if error.is_some(){preview_progress::Stage::Failed}else{manifest.stage},"revision":manifest.revision}))
            .unwrap_or_else(||json!({"completed":0,"total":0,"stage":if error.is_some(){"failed"}else{"preparing"},"revision":0})),
        "full_page_count":manifest.as_ref().map_or(0,|manifest|manifest.full_page_count),
        "pages":manifest.as_ref().map(|manifest|manifest.pages.iter().map(|page|json!({"page_number":page.page_number,
            "source_sheet_number":page.source_sheet_number,"alignment_id":page.alignment_id,
            "from_station_m":page.from_station_m,"to_station_m":page.to_station_m,
            "sha256":page.sha256,"size_bytes":page.size_bytes})).collect::<Vec<_>>()).unwrap_or_default(),
        "selected_page":page,"error":error}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineExit;
    use ds_command_kernel::printing::{self, mv};
    use ds_command_kernel::style_resolution::{self, Binding, Key, Snapshot};
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };
    fn layout() -> printing::Layout {
        serde_json::from_str(include_str!(
            "../../ds-command-kernel/tests/fixtures/mv-global-canonical.json"
        ))
        .unwrap()
    }

    fn sheets() -> Value {
        json!({"project_settings": [], "printing_defaults": {"schema": "ds.printing-defaults/v1",
            "mv_selection": {"schema": mv::SELECTION_SCHEMA, "layout_id": "default", "revision": "a".repeat(64),
                "external_version": "1", "issue_date": "2026-10-02"}}})
    }

    fn table(layout: &printing::Layout) -> Snapshot {
        let binding = |key: Key, name: &str, document: Value| Binding {
            key,
            style_ref: name.into(),
            revision_id: "b".repeat(64),
            content_sha256: ds_command_kernel::style_plan::style_digest(&document),
            scope: "global".into(),
            presentation_tier: None,
            document,
        };
        let defaults: Value = serde_json::from_str(include_str!(
            "../../ds-command-kernel/tests/fixtures/governed-renderer-defaults.json"
        ))
        .unwrap();
        serde_json::from_value(json!({"schema": style_resolution::SCHEMA, "project_id": "template_project",
            "revision_id": "c".repeat(64), "bindings": [
                binding(mv::paper_style_key(&layout.mv.as_ref().unwrap().settings), "fixture/mv_print",
                    json!({"type":"line","paint":{"line-color":"#123456","line-width":1}})),
                binding(printing::renderer_defaults::key(), "fixture/renderer_print",
                    json!({"type":"line","paint":{},"metadata":{"print_renderer_defaults": defaults}})),
            ]}))
        .unwrap()
    }

    struct Engine {
        launches: AtomicUsize,
        reads: AtomicUsize,
        early_failure: bool,
        task: Mutex<Option<Value>>,
    }
    impl Engine {
        fn new() -> Self {
            Self {
                launches: AtomicUsize::new(0),
                reads: AtomicUsize::new(0),
                early_failure: false,
                task: Mutex::new(None),
            }
        }
    }
    impl ReportEngine for Engine {
        fn build_info(&self) -> Result<Value, HostFailure> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            crate::tests::FakeEngine::release().build_info()
        }
        fn export_transformer_report(&self, _: &Path, _: &Path) -> Result<EngineExit, HostFailure> {
            panic!("MV jobs never export a transformer")
        }
        fn launch_grid_preview(
            &self,
            request: &Path,
            _: &Path,
            key: &str,
        ) -> Result<(), HostFailure> {
            self.launches.fetch_add(1, Ordering::SeqCst);
            let task: Value = serde_json::from_slice(&fs::read(request).unwrap()).unwrap();
            assert!(task["sample_pages"].is_null());
            assert_eq!(task["svg_only"], true);
            let target: preview_progress::Target =
                serde_json::from_value(task["preview_progress"].clone()).unwrap();
            assert_eq!(target.input_sha256, key);
            fs::create_dir(&target.directory).unwrap();
            if self.early_failure {
                let failure = preview_progress::Failure {
                    schema: preview_progress::FAILURE_SCHEMA.into(),
                    input_sha256: key.into(),
                    project: task["project_id"].as_str().unwrap().into(),
                    error: "native input refused before pagination".into(),
                };
                fs::write(
                    target.directory.join("failure.json"),
                    serde_json::to_vec(&failure).unwrap(),
                )
                .unwrap();
            } else {
                let scene: Value = serde_json::from_slice(
                    &fs::read(task["scene_path"].as_str().unwrap()).unwrap(),
                )
                .unwrap();
                let svg = b"<svg xmlns='http://www.w3.org/2000/svg'/>";
                let page = target.directory.join("page-0001.svg");
                fs::write(&page, svg).unwrap();
                let manifest = preview_progress::Manifest {
                    schema: preview_progress::SCHEMA.into(),
                    input_sha256: key.into(),
                    project: task["project_id"].as_str().unwrap().into(),
                    model_revision: scene["model_revision"].as_str().unwrap().into(),
                    setup_sha256: task["mv_setup"]["sha256"].as_str().unwrap().into(),
                    revision: 3,
                    stage: preview_progress::Stage::Rendering,
                    completed: 1,
                    total: 100,
                    full_page_count: 100,
                    pages: vec![preview_progress::Page {
                        page_number: 1,
                        source_sheet_number: 1,
                        alignment_id: "alignment-one".into(),
                        from_station_m: 0.0,
                        to_station_m: 500.0,
                        path: page,
                        sha256: ds_command_kernel::report_export::sha256_hex(svg),
                        size_bytes: svg.len() as u64,
                    }],
                    error: None,
                };
                fs::write(
                    target.directory.join("manifest.json"),
                    serde_json::to_vec(&manifest).unwrap(),
                )
                .unwrap();
            }
            *self.task.lock().unwrap() = Some(task);
            Ok(())
        }
    }
    struct Fixture {
        hold: tempfile::TempDir,
        layout: printing::Layout,
        table: Snapshot,
        sheets: Value,
        scene: Vec<u8>,
    }
    impl Fixture {
        fn new() -> Self {
            let layout = layout();
            let table = table(&layout);
            Self {
                hold: tempfile::tempdir().unwrap(),
                layout,
                table,
                sheets: sheets(),
                scene: br#"{"model_revision":"revision-one"}"#.to_vec(),
            }
        }
        fn room(&self) -> Room {
            Room {
                root: self.hold.path().into(),
                scope: ds_command_kernel::project_dataset_cache::Scope {
                    principal: "owner".into(),
                    project: "template_project".into(),
                },
                lane: "local".into(),
                credential_audience_sha256: "a".repeat(64),
            }
        }
        fn inputs(&self) -> MvPreviewInputs<'_> {
            MvPreviewInputs {
                project_id: "template_project",
                sheets: &self.sheets,
                layout: &self.layout,
                table: &self.table,
                project_crs: None,
                model_crs: None,
                scene: &self.scene,
                plan: b"{}",
                staging: self.hold.path(),
            }
        }
    }
    fn key(answer: &Value) -> &str {
        answer["job"]["input_sha256"].as_str().unwrap()
    }
    fn expire(room: &Room, key: &str) {
        let (mut control, path) = load(room, key, clock().unwrap(), true).unwrap();
        control.job.created_at_unix_ms = 1;
        control.job.expires_at_unix_ms = 1 + control.job.retention_ms;
        fs::write(
            path.join("control.json"),
            serde_json::to_vec(&control).unwrap(),
        )
        .unwrap();
    }
    #[test]
    fn first_ready_page_is_consumable_at_one_of_one_hundred_without_another_engine_call() {
        let fixture = Fixture::new();
        let room = fixture.room();
        let engine = Engine::new();
        let answer = start(
            &engine,
            &fixture.inputs(),
            &room,
            preview_artifact::Retention::Days7,
        )
        .unwrap();
        assert_eq!(answer["progress"]["completed"], 1);
        assert_eq!(answer["progress"]["total"], 100);
        assert_eq!(answer["progress"]["stage"], "rendering");
        let artifact: preview_artifact::Artifact =
            serde_json::from_value(answer["selected_page"]["page"].clone()).unwrap();
        assert_eq!(artifact.retention_ms, preview_artifact::LONG_RETENTION_MS);
        assert!(preview_artifact::read(&room, &artifact)
            .unwrap()
            .starts_with(b"<svg"));
        assert!(serde_json::to_vec(&answer).unwrap().len() < 8192);
        assert!(!serde_json::to_string(&answer).unwrap().contains("<svg"));
        let identity_reads = engine.reads.load(Ordering::SeqCst);
        assert_eq!(read(&room, key(&answer), Some(1)).unwrap(), answer);
        assert!(read(&room, key(&answer), Some(100)).unwrap()["selected_page"].is_null());
        assert_eq!(engine.reads.load(Ordering::SeqCst), identity_reads);
        assert_eq!(engine.launches.load(Ordering::SeqCst), 1);
        // Status never opens an unused SVG. A returning page uses its already
        // admitted artifact; the final native disk reader still verifies bytes.
        let path = directory(&room, key(&answer)).unwrap();
        fs::remove_file(path.join("progress/page-0001.svg")).unwrap();
        assert!(read(&room, key(&answer), None).unwrap()["selected_page"].is_null());
        assert_eq!(
            read(&room, key(&answer), Some(1)).unwrap()["selected_page"]["page"],
            answer["selected_page"]["page"]
        );
    }
    #[test]
    fn concurrent_intentional_consumers_launch_once_and_navigation_release_keeps_the_job() {
        let fixture = Fixture::new();
        let room = fixture.room();
        let engine = Engine::new();
        let inputs = fixture.inputs();
        let answers = std::thread::scope(|scope| {
            let workers = (0..4)
                .map(|_| {
                    scope.spawn(|| {
                        start(
                            &engine,
                            &inputs,
                            &room,
                            preview_artifact::Retention::default(),
                        )
                        .unwrap()
                    })
                })
                .collect::<Vec<_>>();
            workers
                .into_iter()
                .map(|worker| worker.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(answers.windows(2).all(|pair| pair[0] == pair[1]));
        assert_eq!(engine.launches.load(Ordering::SeqCst), 1);
        let expected = answers[0].clone();
        drop(answers);
        assert_eq!(read(&room, key(&expected), Some(1)).unwrap(), expected);
    }
    #[test]
    fn exact_project_model_source_is_hashed_and_crossed_source_never_launches() {
        let fixture = Fixture::new();
        let room = fixture.room();
        let engine = Engine::new();
        let source = ds_command_kernel::printing::mv_models::SourceBinding {
            project_id: room.scope.project.clone(),
            model_id: "logical-one".into(),
            revision_id: "head-one".into(),
            content_sha256: "a".repeat(64),
            version: 1,
            worker_id: "local-one".into(),
            worker_model_id: "native-one".into(),
            worker_revision: "revision-one".into(),
            history_pin: "pin".into(),
            coordinate_system: "EPSG:32736".into(),
            read_only: false,
        };
        let first = start_bound(
            &engine,
            &fixture.inputs(),
            &room,
            preview_artifact::Retention::default(),
            Some(&source),
        )
        .unwrap();
        assert_eq!(
            start_bound(
                &engine,
                &fixture.inputs(),
                &room,
                preview_artifact::Retention::default(),
                Some(&source)
            )
            .unwrap(),
            first
        );
        let mut crossed = source.clone();
        crossed.project_id = "foreign".into();
        assert!(start_bound(
            &engine,
            &fixture.inputs(),
            &room,
            preview_artifact::Retention::default(),
            Some(&crossed)
        )
        .is_err());
        assert_eq!(engine.launches.load(Ordering::SeqCst), 1);
        let mut second_source = source.clone();
        second_source.model_id = "logical-two".into();
        second_source.revision_id = "head-two".into();
        let second = start_bound(
            &engine,
            &fixture.inputs(),
            &room,
            preview_artifact::Retention::default(),
            Some(&second_source),
        )
        .unwrap();
        assert_ne!(key(&first), key(&second));
        assert_eq!(engine.launches.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn native_termination_is_scoped_and_visible_without_another_engine_call() {
        let fixture = Fixture::new();
        let room = fixture.room();
        let engine = Engine::new();
        let first = start(
            &engine,
            &fixture.inputs(),
            &room,
            preview_artifact::Retention::default(),
        )
        .unwrap();
        let reads = engine.reads.load(Ordering::SeqCst);
        let mut foreign = room.clone();
        foreign.scope.project = "foreign_project".into();
        assert!(mark_failed(&foreign, key(&first), "wrong scope").is_err());
        assert!(mark_failed(&room, "../foreign", "wrong locator").is_err());
        assert!(mark_failed(&room, key(&first), "").is_err());
        assert!(read(&room, key(&first), None).unwrap()["error"].is_null());
        mark_failed(
            &room,
            key(&first),
            "renderer terminated before its first page",
        )
        .unwrap();
        let failed = read(&room, key(&first), None).unwrap();
        assert_eq!(failed["progress"]["stage"], "failed");
        assert_eq!(failed["error"], "renderer terminated before its first page");
        assert_eq!(engine.launches.load(Ordering::SeqCst), 1);
        assert_eq!(engine.reads.load(Ordering::SeqCst), reads);
    }

    #[test]
    fn early_failure_is_visible_before_a_page_plan_and_retries_do_not_launch_again() {
        let fixture = Fixture::new();
        let room = fixture.room();
        let mut engine = Engine::new();
        engine.early_failure = true;
        let first = start(
            &engine,
            &fixture.inputs(),
            &room,
            preview_artifact::Retention::default(),
        )
        .unwrap();
        assert_eq!(first["progress"]["stage"], "failed");
        assert_eq!(first["progress"]["completed"], 0);
        assert_eq!(first["error"], "native input refused before pagination");
        assert_eq!(
            start(
                &engine,
                &fixture.inputs(),
                &room,
                preview_artifact::Retention::default()
            )
            .unwrap(),
            first
        );
        assert_eq!(engine.launches.load(Ordering::SeqCst), 1);
    }
    #[test]
    fn authority_and_revision_crossing_refuse_without_launching_or_reading_an_engine() {
        let mut fixture = Fixture::new();
        let room = fixture.room();
        let engine = Engine::new();
        let first = start(
            &engine,
            &fixture.inputs(),
            &room,
            preview_artifact::Retention::default(),
        )
        .unwrap();
        let identity_reads = engine.reads.load(Ordering::SeqCst);
        for variant in 0..4 {
            let mut foreign = room.clone();
            match variant {
                0 => foreign.scope.principal = "peer".into(),
                1 => foreign.scope.project = "peer".into(),
                2 => foreign.lane = "peer".into(),
                _ => foreign.credential_audience_sha256 = "b".repeat(64),
            };
            assert!(read(&foreign, key(&first), Some(1)).is_err());
        }
        assert!(read(&room, "../foreign", None).is_err());
        assert_eq!(engine.reads.load(Ordering::SeqCst), identity_reads);
        assert_eq!(engine.launches.load(Ordering::SeqCst), 1);
        fixture.scene = br#"{"model_revision":"revision-two"}"#.to_vec();
        let second = start(
            &engine,
            &fixture.inputs(),
            &room,
            preview_artifact::Retention::default(),
        )
        .unwrap();
        assert_ne!(key(&first), key(&second));
        assert_eq!(engine.launches.load(Ordering::SeqCst), 2);
        let path = directory(&room, key(&second)).unwrap();
        let manifest = path.join("progress/manifest.json");
        let mut document: Value = serde_json::from_slice(&fs::read(&manifest).unwrap()).unwrap();
        document["model_revision"] = json!("foreign");
        fs::write(manifest, serde_json::to_vec(&document).unwrap()).unwrap();
        assert!(read(&room, key(&second), None).is_err());
    }
    #[test]
    fn expiration_reclaims_a_superseded_owned_job_and_keeps_the_current_one() {
        let mut fixture = Fixture::new();
        let room = fixture.room();
        let engine = Engine::new();
        let old = start(
            &engine,
            &fixture.inputs(),
            &room,
            preview_artifact::Retention::default(),
        )
        .unwrap();
        fixture.scene = br#"{"model_revision":"revision-two"}"#.to_vec();
        let current = start(
            &engine,
            &fixture.inputs(),
            &room,
            preview_artifact::Retention::default(),
        )
        .unwrap();
        expire(&room, key(&old));
        assert!(read(&room, key(&old), None).is_err());
        assert_eq!(prune_expired(&room, 16).unwrap(), 1);
        assert!(!directory(&room, key(&old)).unwrap().exists());
        assert!(directory(&room, key(&current)).unwrap().exists());
        assert_eq!(engine.launches.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn completed_owned_output_expires_as_a_whole_and_a_third_active_job_is_refused() {
        let mut fixture = Fixture::new();
        let room = fixture.room();
        let engine = Engine::new();
        let first = start(
            &engine,
            &fixture.inputs(),
            &room,
            preview_artifact::Retention::default(),
        )
        .unwrap();
        fixture.scene = br#"{"model_revision":"revision-two"}"#.to_vec();
        let second = start(
            &engine,
            &fixture.inputs(),
            &room,
            preview_artifact::Retention::default(),
        )
        .unwrap();
        fixture.scene = br#"{"model_revision":"revision-three"}"#.to_vec();
        assert!(
            matches!(start(&engine,&fixture.inputs(),&room,preview_artifact::Retention::default()),Err(crate::mv_preview::MvPreviewRefusal::Host(error)) if error.code=="print_preview_job_capacity")
        );
        assert_eq!(engine.launches.load(Ordering::SeqCst), 2);
        let (control, path) = load(&room, key(&first), clock().unwrap(), true).unwrap();
        let progress = path.join("progress");
        let file = progress.join("manifest.json");
        let mut manifest: preview_progress::Manifest =
            serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
        let original = manifest.pages[0].clone();
        let svg = fs::read(&original.path).unwrap();
        for number in 2..=100 {
            let mut page = original.clone();
            page.page_number = number;
            page.source_sheet_number = number;
            page.path = progress.join(format!("page-{number:04}.svg"));
            fs::write(&page.path, &svg).unwrap();
            manifest.pages.push(page);
        }
        manifest.completed = 100;
        manifest.stage = preview_progress::Stage::Complete;
        manifest.revision += 1;
        fs::write(file, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let output = path.join("output");
        fs::create_dir_all(output.join("preview")).unwrap();
        let mut paths = Vec::new();
        for (index, page) in manifest.pages.iter().enumerate() {
            let file = output
                .join("preview")
                .join(format!("sheet-{:03}.svg", index + 1));
            fs::write(&file, fs::read(&page.path).unwrap()).unwrap();
            paths.push(file);
        }
        let result = json!({"project_id":control.job.project,"model_revision":control.model_revision,"mv_setup_sha256":control.setup_sha256,"svg_only":true,"page_count":100,"previews":paths,"booklet_plan":{}});
        for file in [path.join("result.json"), output.join("manifest.json")] {
            fs::write(file, serde_json::to_vec(&result).unwrap()).unwrap();
        }
        fs::write(output.join("booklet-plan.json"), b"{}").unwrap();
        expire(&room, key(&first));
        assert_eq!(prune_expired(&room, 16).unwrap(), 1);
        assert!(!path.exists());
        assert!(directory(&room, key(&second)).unwrap().exists());
    }
    #[test]
    fn expired_active_or_foreign_jobs_are_never_partly_deleted() {
        let fixture = Fixture::new();
        let room = fixture.room();
        let engine = Engine::new();
        let old = start(
            &engine,
            &fixture.inputs(),
            &room,
            preview_artifact::Retention::default(),
        )
        .unwrap();
        expire(&room, key(&old));
        let path = directory(&room, key(&old)).unwrap();
        let active = fs::OpenOptions::new()
            .create_new(true)
            .read(true)
            .write(true)
            .open(path.join("producer.lock"))
            .unwrap();
        active.lock().unwrap();
        assert_eq!(prune_expired(&room, 16).unwrap(), 0);
        assert!(path.join("scene.json").exists());
        drop(active);
        fs::write(path.join("foreign.txt"), b"owner bytes").unwrap();
        assert_eq!(prune_expired(&room, 16).unwrap(), 0);
        assert_eq!(fs::read(path.join("foreign.txt")).unwrap(), b"owner bytes");
        assert!(path.join("scene.json").exists());
        assert!(path.join("progress/page-0001.svg").exists());
    }
}
