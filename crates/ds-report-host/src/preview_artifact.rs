//! Explicit consumers of a native preview read one exact disk SVG. This owner
//! has no transport or credential refresh; hosts admit their captured context.
use crate::{HostFailure, PreviewPage, CODE_INPUTS_INVALID, CODE_RESULT_INVALID};
use ds_project_data::preview_room::Room;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const SCHEMA: &str = "ds.print-preview-artifact/v1";
pub const RETENTION_MS: u64 = 24 * 60 * 60 * 1000;
pub const LONG_RETENTION_MS: u64 = 7 * RETENTION_MS;
const MAX_MANIFEST_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub enum Retention {
    #[default]
    #[serde(rename = "24h")]
    Hours24,
    #[serde(rename = "7d")]
    Days7,
}
impl Retention {
    fn millis(self) -> u64 {
        match self {
            Self::Hours24 => RETENTION_MS,
            Self::Days7 => LONG_RETENTION_MS,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub schema: String,
    pub path: String,
    pub sha256: String,
    pub size_bytes: u64,
    pub content_type: String,
    pub authority_sha256: String,
    pub result_sha256: String,
    pub created_at_unix_ms: u64,
    pub expires_at_unix_ms: u64,
    pub retention_ms: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    artifact: Artifact,
    metadata: Value,
}

fn failed(message: impl Into<String>) -> HostFailure {
    HostFailure::new("print_preview_artifact_unavailable", message)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn digest_token(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn now() -> Result<u64, HostFailure> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| failed(e.to_string()))?
        .as_millis()
        .try_into()
        .map_err(|_| failed("preview clock exceeds its bound"))
}
pub(crate) fn authority(room: &Room) -> Result<String, HostFailure> {
    if !room.root.is_absolute()
        || room.scope.principal.trim().is_empty()
        || room.scope.project.trim().is_empty()
        || room.lane.trim().is_empty()
        || !digest_token(&room.credential_audience_sha256)
    {
        return Err(HostFailure::new(
            CODE_INPUTS_INVALID,
            "preview artifact requires exact native account/project/lane/audience",
        ));
    }
    Ok(digest(
        &serde_json::to_vec(&json!([
            SCHEMA,
            room.scope.principal,
            room.scope.project,
            room.lane,
            room.credential_audience_sha256
        ]))
        .map_err(|e| failed(e.to_string()))?,
    ))
}
fn root(room: &Room, authority: &str) -> PathBuf {
    ds_project_data::room_hold::room_dir(&room.root, &room.scope)
        .join("preview-svg")
        .join(authority)
}
fn metadata(page: &PreviewPage, retention: Retention) -> Result<Value, HostFailure> {
    // Serialize metadata directly; never serialize the SVG to discard it.
    Ok(
        json!({"transformer":page.transformer,"server_version":page.server_version,
        "sha256":page.sha256,"size_bytes":page.size_bytes,"content_type":page.content_type,
        "output_id":page.output_id,"layout_id":page.layout_id,"layout_sha256":page.layout_sha256,
        "format":page.format,"warnings":page.warnings,"print_diagnostics":page.print_diagnostics,
        "elapsed_ms":page.elapsed_ms,"engine":page.engine,"context_layers":page.context_layers,
        "context_omitted":page.context_omitted,"survey_forms_omitted":page.survey_forms_omitted,
        "context_sha256":page.context_sha256,"filename":page.filename,"receipt":page.receipt,
        "retention_ms":retention.millis()}),
    )
}
fn admitted_page(page: &PreviewPage) -> Result<(), HostFailure> {
    if page.size_bytes == 0
        || page.size_bytes > crate::MAX_PREVIEW_BYTES
        || page.size_bytes != page.page.len() as u64
        || !digest_token(&page.sha256)
        || digest(page.page.as_bytes()) != page.sha256
        || page.content_type != "image/svg+xml"
        || !page.page.trim_start().starts_with("<svg")
    {
        return Err(HostFailure::new(
            CODE_RESULT_INVALID,
            "preview SVG differs from its native size, digest or content type",
        ));
    }
    Ok(())
}
fn read_manifest(directory: &Path) -> Result<Manifest, HostFailure> {
    let metadata = fs::symlink_metadata(directory).map_err(|e| failed(e.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(failed("preview result directory must be a real directory"));
    }
    let path = directory.join("manifest.json");
    let bytes = crate::read_bounded(&path, MAX_MANIFEST_BYTES)?
        .ok_or_else(|| failed("preview artifact manifest is missing"))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| failed(format!("preview artifact manifest is unreadable: {e}")))
}
fn admit_manifest(room: &Room, manifest: &Manifest) -> Result<PathBuf, HostFailure> {
    let artifact = &manifest.artifact;
    let owner = authority(room)?;
    if artifact.schema != SCHEMA
        || artifact.authority_sha256 != owner
        || !digest_token(&artifact.result_sha256)
        || !digest_token(&artifact.sha256)
        || artifact.content_type != "image/svg+xml"
        || artifact.size_bytes == 0
        || artifact.size_bytes > crate::MAX_PREVIEW_BYTES
        || ![RETENTION_MS, LONG_RETENTION_MS].contains(&artifact.retention_ms)
        || artifact
            .created_at_unix_ms
            .checked_add(artifact.retention_ms)
            != Some(artifact.expires_at_unix_ms)
        || digest(&serde_json::to_vec(&manifest.metadata).map_err(|e| failed(e.to_string()))?)
            != artifact.result_sha256
        || manifest.metadata["sha256"] != artifact.sha256
        || manifest.metadata["retention_ms"] != artifact.retention_ms
        || manifest.metadata["size_bytes"] != artifact.size_bytes
    {
        return Err(failed(
            "preview artifact differs from its captured native authority or result",
        ));
    }
    let path = root(room, &owner)
        .join(&artifact.result_sha256)
        .join("page.svg");
    if path.to_string_lossy() != artifact.path {
        return Err(failed(
            "preview artifact path differs from its managed result",
        ));
    }
    Ok(path)
}

/// Materialize a proven result once. Reuse never extends its original expiry.
pub fn materialize(room: &Room, page: &PreviewPage) -> Result<Artifact, HostFailure> {
    materialize_retained(room, page, Retention::default())
}
pub fn materialize_retained(
    room: &Room,
    page: &PreviewPage,
    retention: Retention,
) -> Result<Artifact, HostFailure> {
    materialize_with_retention_at(room, page, retention, now()?)
}
#[cfg(test)]
fn materialize_at(room: &Room, page: &PreviewPage, clock: u64) -> Result<Artifact, HostFailure> {
    materialize_with_retention_at(room, page, Retention::default(), clock)
}
fn materialize_with_retention_at(
    room: &Room,
    page: &PreviewPage,
    retention: Retention,
    clock: u64,
) -> Result<Artifact, HostFailure> {
    admitted_page(page)?;
    materialize_svg_at(
        room,
        &page.page,
        metadata(page, retention)?,
        retention,
        clock,
    )
}

/// Persist another native SVG subject through the same artifact authority.
/// Provenance is authored by the admitted native renderer host, never a path
/// or artifact receipt supplied by a consumer.
pub fn materialize_svg_retained(
    room: &Room,
    svg: &str,
    provenance: Value,
    retention: Retention,
) -> Result<Artifact, HostFailure> {
    let metadata = json!({"provenance":provenance,"sha256":digest(svg.as_bytes()),
        "size_bytes":svg.len(),"content_type":"image/svg+xml","retention_ms":retention.millis()});
    materialize_svg_at(room, svg, metadata, retention, now()?)
}

fn materialize_svg_at(
    room: &Room,
    svg: &str,
    metadata: Value,
    retention: Retention,
    clock: u64,
) -> Result<Artifact, HostFailure> {
    let size_bytes = svg.len() as u64;
    if size_bytes == 0
        || size_bytes > crate::MAX_PREVIEW_BYTES
        || !(svg.trim_start().starts_with("<svg") || svg.trim_start().starts_with("<?xml"))
    {
        return Err(HostFailure::new(
            CODE_RESULT_INVALID,
            "native preview is empty, oversized or not an SVG document",
        ));
    }
    let sha256 = digest(svg.as_bytes());
    let owner = authority(room)?;
    let result = digest(&serde_json::to_vec(&metadata).map_err(|e| failed(e.to_string()))?);
    let base = root(room, &owner);
    ds_project_data::private::create_dir_all(&base).map_err(|e| failed(e.to_string()))?;
    prune_at(room, clock, 16)?;
    let _lock =
        ds_layer_store::exclusive_lock(&base.join(format!("{result}.lock"))).map_err(failed)?;
    let target = base.join(&result);
    if fs::symlink_metadata(&target).is_ok() {
        let manifest = read_manifest(&target)?;
        let path = admit_manifest(room, &manifest)?;
        if manifest.metadata != metadata {
            return Err(failed(
                "preview result identity differs from its retained manifest",
            ));
        }
        crate::verify_file(&path, size_bytes, &sha256)?;
        if clock < manifest.artifact.expires_at_unix_ms {
            return Ok(manifest.artifact);
        }
        // Only this admitted key's two owned files expire. Unknown files and
        // other result directories are never traversed or removed.
        remove_owned(&target, &manifest, &path)?;
    }
    let artifact = Artifact {
        schema: SCHEMA.into(),
        path: target.join("page.svg").to_string_lossy().into_owned(),
        sha256: sha256.clone(),
        size_bytes,
        content_type: "image/svg+xml".into(),
        authority_sha256: owner,
        result_sha256: result,
        created_at_unix_ms: clock,
        expires_at_unix_ms: clock
            .checked_add(retention.millis())
            .ok_or_else(|| failed("preview expiry exceeds its bound"))?,
        retention_ms: retention.millis(),
    };
    let staged = crate::Staging {
        path: base.join(format!(".stage-{}", crate::random_run_id())),
    };
    crate::create_private_dir(&staged.path)?;
    crate::write_new_private(&staged.path.join("page.svg"), svg.as_bytes())?;
    crate::verify_file(&staged.path.join("page.svg"), size_bytes, &sha256)?;
    let manifest = serde_json::to_vec(&Manifest {
        artifact: artifact.clone(),
        metadata,
    })
    .map_err(|e| failed(e.to_string()))?;
    if manifest.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(failed(
            "preview manifest exceeds its bounded transport metadata",
        ));
    }
    crate::write_new_private(&staged.path.join("manifest.json"), &manifest)?;
    crate::promote_report(&staged.path, &target)?;
    Ok(artifact)
}

fn remove_owned(directory: &Path, manifest: &Manifest, page: &Path) -> Result<(), HostFailure> {
    let names = fs::read_dir(directory)
        .map_err(|e| failed(e.to_string()))?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .map_err(|e| failed(e.to_string()))?;
    let expected = ["manifest.json", "page.svg"]
        .map(std::ffi::OsString::from)
        .into_iter()
        .collect();
    if names != expected {
        return Err(failed(
            "preview result contains foreign files; expiration left every file intact",
        ));
    }
    crate::verify_file(
        page,
        manifest.artifact.size_bytes,
        &manifest.artifact.sha256,
    )?;
    fs::remove_file(page).map_err(|e| failed(e.to_string()))?;
    fs::remove_file(directory.join("manifest.json")).map_err(|e| failed(e.to_string()))?;
    fs::remove_dir(directory).map_err(|e| failed(e.to_string()))
}
/// Bounded cleanup only at an explicit consuming write or explicit native
/// cleanup. It cannot acquire data, inspect remote freshness or traverse jobs.
pub fn prune_expired(room: &Room, limit: usize) -> Result<usize, HostFailure> {
    prune_at(room, now()?, limit)
}
fn prune_at(room: &Room, clock: u64, limit: usize) -> Result<usize, HostFailure> {
    if limit == 0 || limit > 128 {
        return Err(HostFailure::new(
            CODE_INPUTS_INVALID,
            "preview cleanup limit must be 1..128",
        ));
    }
    let base = root(room, &authority(room)?);
    let entries = match fs::read_dir(&base) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(failed(e.to_string())),
    };
    let mut candidates = 0;
    let mut removed = 0;
    for entry in entries {
        let entry = entry.map_err(|e| failed(e.to_string()))?;
        let key = entry.file_name();
        let Some(key) = key.to_str() else { continue };
        if !digest_token(key) {
            continue;
        }
        candidates += 1;
        if candidates > limit {
            break;
        }
        let _lock =
            ds_layer_store::exclusive_lock(&base.join(format!("{key}.lock"))).map_err(failed)?;
        let Ok(manifest) = read_manifest(&entry.path()) else {
            continue;
        };
        let Ok(page) = admit_manifest(room, &manifest) else {
            continue;
        };
        if page.parent() != Some(entry.path().as_path())
            || clock < manifest.artifact.expires_at_unix_ms
        {
            continue;
        }
        if remove_owned(&entry.path(), &manifest, &page).is_ok() {
            removed += 1;
        }
    }
    Ok(removed)
}

/// Read only the exact admitted managed file; no render, acquisition or refresh.
pub fn read(room: &Room, artifact: &Artifact) -> Result<Vec<u8>, HostFailure> {
    read_at(room, artifact, now()?)
}
/// Validate a retained locator without reading an unselected page's bytes.
/// The actual file consumer still calls `read` to verify its exact content.
pub fn admit_receipt(room: &Room, artifact: &Artifact) -> Result<(), HostFailure> {
    if !digest_token(&artifact.result_sha256) {
        return Err(failed("preview result locator is invalid"));
    }
    let manifest = read_manifest(&root(room, &authority(room)?).join(&artifact.result_sha256))?;
    admit_manifest(room, &manifest)?;
    if &manifest.artifact != artifact {
        return Err(failed(
            "preview receipt differs from its exact native manifest",
        ));
    }
    if now()? >= artifact.expires_at_unix_ms {
        return Err(HostFailure::new(
            "print_preview_artifact_expired",
            "Preview receipt expired; request its exact native preview again.",
        ));
    }
    Ok(())
}
fn read_at(room: &Room, artifact: &Artifact, clock: u64) -> Result<Vec<u8>, HostFailure> {
    if !digest_token(&artifact.result_sha256) {
        return Err(failed("preview result locator is invalid"));
    }
    let manifest = read_manifest(&root(room, &authority(room)?).join(&artifact.result_sha256))?;
    let path = admit_manifest(room, &manifest)?;
    if &manifest.artifact != artifact {
        return Err(failed("preview receipt differs from its managed manifest"));
    }
    if clock >= artifact.expires_at_unix_ms {
        return Err(HostFailure::new(
            "print_preview_artifact_expired",
            "Preview expired; request its exact native preview again.",
        ));
    }
    crate::verify_file(&path, artifact.size_bytes, &artifact.sha256)?;
    let bytes = crate::read_bounded(&path, artifact.size_bytes)?
        .ok_or_else(|| failed("preview SVG is missing"))?;
    if bytes.len() as u64 != artifact.size_bytes || digest(&bytes) != artifact.sha256 {
        return Err(failed("preview SVG changed while reading"));
    }
    Ok(bytes)
}

/// Validate the explicit output before rendering. The managed result remains
/// authoritative; a copy never replaces or adopts a source or another output.
pub fn admit_output(raw: &str) -> Result<PathBuf, HostFailure> {
    let path = PathBuf::from(raw);
    if raw.is_empty()
        || raw.len() > 4096
        || raw.chars().any(char::is_control)
        || !path.is_absolute()
        || path.extension().and_then(|s| s.to_str()) != Some("svg")
    {
        return Err(HostFailure::new(
            CODE_INPUTS_INVALID,
            "preview output must be a new absolute .svg path",
        ));
    }
    if fs::symlink_metadata(&path).is_ok() {
        return Err(HostFailure::new(
            crate::CODE_OUTPUT_EXISTS,
            "preview output already exists",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| failed("preview output has no parent"))?;
    let metadata =
        fs::symlink_metadata(parent).map_err(|_| failed("preview output parent is unavailable"))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(failed("preview output parent must be a real directory"));
    }
    Ok(path)
}
pub fn copy(room: &Room, artifact: &Artifact, raw: &str) -> Result<PathBuf, HostFailure> {
    let destination = admit_output(raw)?;
    let bytes = read(room, artifact)?;
    let temporary = destination
        .parent()
        .unwrap()
        .join(format!(".preview-copy-{}", crate::random_run_id()));
    let staged = crate::Staging {
        path: temporary.clone(),
    };
    crate::create_private_dir(&temporary)?;
    let file = temporary.join("page.svg");
    crate::write_new_private(&file, &bytes)?;
    crate::verify_file(&file, artifact.size_bytes, &artifact.sha256)?;
    // A same-filesystem hard link publishes complete bytes with atomic
    // no-replace semantics on every supported native host.
    fs::hard_link(&file, &destination)
        .map_err(|e| failed(format!("could not publish new preview copy: {e}")))?;
    crate::sync_directory(destination.parent().unwrap())?;
    drop(staged);
    Ok(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn room(root: PathBuf) -> Room {
        Room {
            root,
            scope: ds_command_kernel::project_dataset_cache::Scope {
                principal: "owner".into(),
                project: "project".into(),
            },
            lane: "local".into(),
            credential_audience_sha256: "a".repeat(64),
        }
    }
    fn page(size: usize) -> PreviewPage {
        let svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\">{}</svg>",
            " ".repeat(size)
        );
        serde_json::from_value(json!({"transformer":"tx_a","server_version":7,"page":svg,
            "sha256":digest(svg.as_bytes()),"size_bytes":svg.len(),"content_type":"image/svg+xml",
            "output_id":"svg__a0","layout_id":"a0","layout_sha256":"b".repeat(64),"format":"svg",
            "warnings":[],"print_diagnostics":[],"elapsed_ms":1,"engine":{"engine_version":"dev","build_manifest_sha256":"c".repeat(64),
            "package_version":"1","source_sha":"d".repeat(40),"profile":"debug"},"context_layers":[],"context_omitted":[],
            "survey_forms_omitted":[],"context_sha256":null,"filename":"tx_a.svg","receipt":{"input":"native-pinned"}})).unwrap()
    }
    #[test]
    fn large_svg_is_disk_bytes_with_small_receipt_and_reuse_keeps_expiry() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let page = page(9 * 1024 * 1024);
        let artifact = materialize_at(&room, &page, 100).unwrap();
        assert!(artifact.size_bytes > 8 * 1024 * 1024);
        assert!(serde_json::to_vec(&artifact).unwrap().len() < 2048);
        assert_eq!(materialize_at(&room, &page, 500).unwrap(), artifact);
        assert_eq!(
            read_at(&room, &artifact, 500).unwrap(),
            page.page.as_bytes()
        );
        assert_eq!(artifact.expires_at_unix_ms, 100 + RETENTION_MS);
    }
    #[test]
    fn exact_scope_result_digest_path_and_bytes_are_admitted_on_every_read() {
        let dir = tempfile::tempdir().unwrap();
        let base = room(dir.path().into());
        let page = page(10);
        let artifact = materialize_at(&base, &page, 100).unwrap();
        for changed in [
            Room {
                scope: ds_command_kernel::project_dataset_cache::Scope {
                    principal: "other".into(),
                    ..base.scope.clone()
                },
                ..base.clone()
            },
            Room {
                scope: ds_command_kernel::project_dataset_cache::Scope {
                    project: "other".into(),
                    ..base.scope.clone()
                },
                ..base.clone()
            },
            Room {
                lane: "canary".into(),
                ..base.clone()
            },
            Room {
                credential_audience_sha256: "e".repeat(64),
                ..base.clone()
            },
        ] {
            assert!(read_at(&changed, &artifact, 101).is_err());
        }
        let mut changed = artifact.clone();
        changed.path = dir.path().join("foreign.svg").display().to_string();
        assert!(read_at(&base, &changed, 101).is_err());
        let mut changed = artifact.clone();
        changed.sha256 = "f".repeat(64);
        assert!(read_at(&base, &changed, 101).is_err());
        fs::write(&artifact.path, b"foreign").unwrap();
        assert!(read_at(&base, &artifact, 101).is_err());
    }
    #[test]
    fn expiry_republishes_only_exact_owned_result_and_leaves_other_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let page = page(10);
        let artifact = materialize_at(&room, &page, 100).unwrap();
        let mut other = page.clone();
        other.server_version += 1;
        let other = materialize_at(&room, &other, 100).unwrap();
        let foreign = dir.path().join("foreign-job");
        fs::write(&foreign, b"keep").unwrap();
        assert_eq!(
            read_at(&room, &artifact, 100 + RETENTION_MS)
                .unwrap_err()
                .code,
            "print_preview_artifact_expired"
        );
        let replacement = materialize_at(&room, &page, 100 + RETENTION_MS).unwrap();
        assert_eq!(replacement.path, artifact.path);
        assert_ne!(replacement.expires_at_unix_ms, artifact.expires_at_unix_ms);
        assert!(!Path::new(&other.path).exists());
        assert_eq!(fs::read(foreign).unwrap(), b"keep");
    }
    #[test]
    fn invalid_native_result_and_output_paths_cannot_write_or_replace() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let page = page(10);
        let mut bad = page.clone();
        bad.server_version += 1;
        bad.sha256 = "f".repeat(64);
        assert!(materialize_at(&room, &bad, 100).is_err());
        assert!(!root(&room, &authority(&room).unwrap()).exists());
        for path in [
            "relative.svg",
            "/missing-parent/preview.svg",
            dir.path().join("preview.pdf").to_str().unwrap(),
        ] {
            assert!(admit_output(path).is_err());
        }
        let artifact = materialize(&room, &page).unwrap();
        let output = dir.path().join("preview.svg");
        copy(&room, &artifact, output.to_str().unwrap()).unwrap();
        assert!(copy(&room, &artifact, output.to_str().unwrap()).is_err());
        assert_eq!(fs::read(output).unwrap(), page.page.as_bytes());
    }
    #[test]
    fn concurrent_consumers_publish_one_exact_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let page = page(10);
        let answers = std::thread::scope(|threads| {
            let jobs: Vec<_> = (0..4)
                .map(|_| threads.spawn(|| materialize_at(&room, &page, 100).unwrap()))
                .collect();
            jobs.into_iter()
                .map(|job| job.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(answers.iter().all(|value| value == &answers[0]));
        assert_eq!(
            fs::read_dir(Path::new(&answers[0].path).parent().unwrap())
                .unwrap()
                .count(),
            2
        );
    }
    #[test]
    fn seven_day_retention_and_foreign_files_are_preserved_by_bounded_prune() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let page = page(10);
        let short = materialize_at(&room, &page, 100).unwrap();
        let long = materialize_with_retention_at(&room, &page, Retention::Days7, 100).unwrap();
        assert_ne!(short.path, long.path);
        assert_eq!(long.expires_at_unix_ms, 100 + LONG_RETENTION_MS);
        assert_eq!(
            materialize_with_retention_at(&room, &page, Retention::Days7, 500).unwrap(),
            long
        );
        let foreign = Path::new(&short.path).parent().unwrap().join("other-job");
        fs::write(&foreign, b"keep").unwrap();
        assert_eq!(prune_at(&room, 100 + RETENTION_MS, 16).unwrap(), 0);
        assert!(Path::new(&short.path).is_file());
        assert!(foreign.is_file());
        assert_eq!(
            read_at(&room, &long, 100 + RETENTION_MS).unwrap(),
            page.page.as_bytes()
        );
        fs::remove_file(foreign).unwrap();
        assert_eq!(prune_at(&room, 100 + RETENTION_MS, 16).unwrap(), 1);
        assert!(!Path::new(&short.path).exists());
        assert!(Path::new(&long.path).is_file());
    }
    #[cfg(unix)]
    #[test]
    fn symlink_destination_parent_and_managed_result_refuse() {
        let dir = tempfile::tempdir().unwrap();
        let room = room(dir.path().into());
        let page = page(10);
        let artifact = materialize_at(&room, &page, 100).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(dir.path(), &link).unwrap();
        assert!(admit_output(link.join("preview.svg").to_str().unwrap()).is_err());
        let target = Path::new(&artifact.path).parent().unwrap();
        let moved = dir.path().join("moved");
        fs::rename(target, &moved).unwrap();
        std::os::unix::fs::symlink(&moved, target).unwrap();
        assert!(read_at(&room, &artifact, 101).is_err());
        assert!(materialize_at(&room, &page, 101).is_err());
    }
}
