//! Explicit external-document handover and derivative consumption through the
//! existing protected native room and typed Assets client. No extractor spawn.
use crate::{asset_preview_source, project_assets_for_project};
use ds_cli_contract::Failure;
use ds_client_core::project_assets::Command as Assets;
use ds_command_kernel::{
    asset_preview::Request,
    assets::semantic::{self, Command, Extractor, Prepare},
};
use ds_report_host::{
    HostFailure,
    asset_preview::Control,
    document_semantic as host,
    preview_artifact::{self, Retention},
};
use serde_json::{Value, json};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("asset_semantic_invalid",message).remedy("Use the unchanged native semantic-export manifest and one selected document/UTF-8 Markdown file.")
}
fn failure(error: HostFailure) -> Failure {
    match error.code.as_str() {
        "asset_semantic_invalid" => invalid(error.message),
        "report_output_exists" => Failure::conflict("report_output_exists", error.message)
            .remedy("Choose a new output path; existing files are preserved."),
        "project_context_changed" => Failure::conflict("project_context_changed", error.message)
            .remedy("Repeat under the intended native account and project."),
        "asset_preview_offline_absent" => {
            Failure::unavailable("asset_preview_offline_absent", error.message)
                .remedy("Acquire the exact selected document online first.")
        }
        "print_preview_artifact_expired" => {
            Failure::unavailable("print_preview_artifact_expired", error.message)
                .remedy("Explicitly read the selected derivative online again.")
        }
        _ => Failure::unavailable("asset_semantic_retained_invalid", error.message)
            .remedy("Use the exact native source/export again; preserve foreign files."),
    }
}
fn input_path(raw: &str) -> Result<PathBuf, Failure> {
    let path = PathBuf::from(raw);
    if raw.is_empty()
        || raw.len() > 4096
        || raw.chars().any(char::is_control)
        || !path.is_absolute()
    {
        return Err(invalid("Name one absolute local file."));
    }
    let metadata = fs::symlink_metadata(&path).map_err(|e| invalid(e.to_string()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(invalid("The selected input is not one regular local file."));
    }
    Ok(path)
}
fn new_path(raw: &str, extension: &str) -> Result<PathBuf, Failure> {
    let path = PathBuf::from(raw);
    if raw.is_empty()
        || raw.len() > 4096
        || raw.chars().any(char::is_control)
        || !path.is_absolute()
        || path.extension().and_then(|s| s.to_str()) != Some(extension)
    {
        return Err(invalid(format!(
            "Choose one new absolute .{extension} output path."
        )));
    }
    if fs::symlink_metadata(&path).is_ok() {
        return Err(
            Failure::conflict("report_output_exists", "Output already exists.")
                .remedy("Choose a new output path; existing files are preserved."),
        );
    }
    let metadata = fs::symlink_metadata(
        path.parent()
            .ok_or_else(|| invalid("Output has no parent."))?,
    )
    .map_err(|e| invalid(e.to_string()))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(invalid("Output parent must be a real existing directory."));
    }
    Ok(path)
}
/// Export's typed native result is the only CLI manifest input; callers never
/// assemble source or output receipts by hand.
pub fn read_export(project: &str, raw: &str) -> Result<host::Export, Failure> {
    let path = input_path(raw)?;
    let metadata = fs::metadata(&path).map_err(|e| invalid(e.to_string()))?;
    if metadata.len() == 0 || metadata.len() > 64 * 1024 {
        return Err(invalid(
            "Export manifest must be nonempty and at most 64 KiB.",
        ));
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|e| invalid(e.to_string()))?
        .take(64 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| invalid(e.to_string()))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(invalid("Export manifest changed while read."));
    }
    let export: host::Export = serde_json::from_slice(&bytes)
        .map_err(|_| invalid("File is not an unchanged typed native semantic-export manifest."))?;
    host::validate_export(project, &export).map_err(failure)?;
    Ok(export)
}
fn write_export(path: &Path, export: &host::Export) -> Result<(), Failure> {
    let bytes = serde_json::to_vec_pretty(export)
        .map_err(|_| invalid("Native export manifest could not be encoded."))?;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(|e| invalid(e.to_string()))?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|e| invalid(e.to_string()))
}
pub fn export(
    lane: &str,
    project: &str,
    request: &Request,
    extractor: Extractor,
    out: &str,
    manifest: &str,
) -> Result<Value, Failure> {
    // Local pins and output admission happen before source acquisition.
    request.validate().map_err(|e| invalid(e.message))?;
    extractor.validate().map_err(invalid)?;
    new_path(out, "pdf")?;
    let manifest_path = new_path(manifest, "json")?;
    let receipt = asset_preview_source::acquire_for_named_project(lane, project, request)?;
    let cancelled = AtomicBool::new(false);
    let progress = |_, _, _| {};
    let (room, control) =
        asset_preview_source::captured_room(lane, project, &cancelled, &progress)?;
    let export = host::export(&room, &receipt, extractor, out, &control).map_err(failure)?;
    control.check_current().map_err(failure)?;
    write_export(&manifest_path, &export)?;
    control.check_current().map_err(failure)?;
    Ok(
        json!({"selection":export.selection,"pdf":export.path,"manifest":manifest_path,"extraction":"external; use the pinned MarkItDown version and options, then semantic import"}),
    )
}
pub fn lookup(lane: &str, project: &str, manifest: &str) -> Result<Value, Failure> {
    let export = read_export(project, manifest)?;
    project_assets_for_project(
        lane,
        project,
        &Assets::Semantic(Command::Lookup(export.selection)),
        None,
    )
    .map(|v| v.into_result())
}
fn revoke_if_denied(
    room: &ds_project_data::preview_room::Room,
    selection: &semantic::Selection,
    error: &Failure,
) {
    if error.detail_value().is_some_and(|detail| {
        matches!(detail["http_status"].as_u64(), Some(401 | 403 | 404))
            && detail["service_code"].as_str().is_some()
    }) {
        let _ = host::forget_source(room, selection);
    }
}
fn answer(
    room: &ds_project_data::preview_room::Room,
    receipt: Value,
    artifact: preview_artifact::Artifact,
    reused: bool,
    out: Option<&str>,
) -> Result<Value, Failure> {
    let bytes = preview_artifact::read(room, &artifact).map_err(failure)?;
    let text =
        std::str::from_utf8(&bytes).map_err(|_| invalid("Retained derivative is not UTF-8."))?;
    let inline: String = text.chars().take(4096).collect();
    let more = inline.len() < text.len();
    let copied = out
        .map(|path| host::copy_markdown(room, &artifact, path))
        .transpose()
        .map_err(failure)?;
    Ok(
        json!({"receipt":receipt,"retained":artifact,"reused":reused,"markdown":inline,"more":more,"output":copied}),
    )
}
pub fn read(
    lane: &str,
    project: &str,
    manifest: &str,
    offline: bool,
    refresh: bool,
    out: Option<&str>,
    retention: Retention,
) -> Result<Value, Failure> {
    if offline && refresh {
        return Err(invalid("Offline and refresh cannot be combined."));
    }
    let export = read_export(project, manifest)?;
    if let Some(path) = out {
        new_path(path, "md")?;
    }
    let cancelled = AtomicBool::new(false);
    let progress = |_, _, _| {};
    let (room, control) =
        asset_preview_source::captured_room(lane, project, &cancelled, &progress)?;
    control.check_current().map_err(failure)?;
    if !refresh
        && let Some((receipt, artifact)) =
            host::read_retained(&room, &export.selection, retention).map_err(failure)?
    {
        let result = answer(&room, receipt, artifact, true, out)?;
        control.check_current().map_err(failure)?;
        return Ok(result);
    }
    if offline {
        return Err(Failure::unavailable(
            "asset_semantic_offline_absent",
            "The exact derivative is absent, expired or revoked in this native room.",
        )
        .remedy("Explicitly read it online after import; offline performs no source request."));
    }
    let result = project_assets_for_project(
        lane,
        project,
        &Assets::Semantic(Command::Read(export.selection.clone())),
        None,
    )
    .inspect_err(|e| revoke_if_denied(&room, &export.selection, e))?
    .into_result();
    control.check_current().map_err(failure)?;
    let bytes = result["markdown"]
        .as_str()
        .ok_or_else(|| invalid("Native semantic read has no checked Markdown."))?
        .as_bytes();
    let receipt = result["receipt"].clone();
    let prepare = Prepare {
        selection: export.selection,
        output: semantic::Output {
            sha256: receipt["output"]["sha256"]
                .as_str()
                .unwrap_or_default()
                .into(),
            bytes: receipt["output"]["bytes"].as_u64().unwrap_or_default(),
        },
    };
    let artifact =
        host::retain_ready(&room, &prepare, &receipt, bytes, retention).map_err(failure)?;
    control.check_current().map_err(failure)?;
    answer(&room, receipt, artifact, false, out)
}
pub fn import(
    lane: &str,
    project: &str,
    manifest: &str,
    markdown: &str,
    retention: Retention,
) -> Result<Value, Failure> {
    let export = read_export(project, manifest)?;
    let path = input_path(markdown)?;
    if path.extension().and_then(|value| value.to_str()) != Some("md") {
        return Err(invalid(
            "External Markdown must name one absolute .md file.",
        ));
    }
    let (prepare, bytes) =
        host::prepare_file(project, export.selection.clone(), &path).map_err(failure)?;
    let cancelled = AtomicBool::new(false);
    let progress = |_, _, _| {};
    let (room, control) =
        asset_preview_source::captured_room(lane, project, &cancelled, &progress)?;
    let selection = host::selection(
        &room,
        &export.native_source,
        export.selection.extractor.clone(),
        &control,
    )
    .map_err(failure)?;
    if selection != prepare.selection {
        return Err(invalid("Native source differs from its export manifest."));
    }
    control.check_current().map_err(failure)?;
    let mut reader = &bytes[..];
    let receipt = project_assets_for_project(
        lane,
        project,
        &Assets::Semantic(Command::Prepare(prepare.clone())),
        Some(&mut reader),
    )
    .inspect_err(|e| revoke_if_denied(&room, &prepare.selection, e))?
    .into_result();
    control.check_current().map_err(failure)?;
    let artifact =
        host::retain_ready(&room, &prepare, &receipt, &bytes, retention).map_err(failure)?;
    control.check_current().map_err(failure)?;
    Ok(
        json!({"receipt":receipt,"retained":artifact,"extraction":"external; no source or catalogue version changed"}),
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            let suffix = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("ds-semantic-cli-{}-{suffix}", std::process::id()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn exported() -> host::Export {
        let measured = "a".repeat(64);
        let source = ds_command_kernel::asset_preview::SourceProof {
            project_id: "project".into(),
            asset_id: "sys:pm_attachment:pma_pdf".into(),
            source_kind: "pm_attachment".into(),
            source_id: "pma_pdf".into(),
            revision: "pm-version:3".into(),
            generation: "29".into(),
            advertised_sha256: None,
            size_bytes: 7,
            content_type: "application/pdf".into(),
            sensitivity: "internal".into(),
            name: "Meeting.pdf".into(),
        };
        host::Export {
            selection: semantic::Selection {
                source: semantic::SourcePin {
                    kind: semantic::SourceKind::PmAttachment,
                    id: "pma_pdf".into(),
                    version: 3,
                    generation: 29,
                    size_bytes: 7,
                    content_type: "application/pdf".into(),
                    measured_sha256: measured.clone(),
                },
                extractor: Extractor {
                    name: "markitdown".into(),
                    version: "0.1.8".into(),
                    options_sha256: semantic::digest(b"{}"),
                },
            },
            path: "/selected/Meeting.pdf".into(),
            native_source: ds_report_host::asset_preview::Receipt {
                schema: ds_command_kernel::asset_preview::RECEIPT_SCHEMA.into(),
                source,
                measured_sha256: measured.clone(),
                document: json!({}),
                artifact: preview_artifact::Artifact {
                    schema: preview_artifact::SCHEMA.into(),
                    path: "/held/page.pdf".into(),
                    sha256: measured.clone(),
                    size_bytes: 7,
                    content_type: "application/pdf".into(),
                    authority_sha256: "b".repeat(64),
                    result_sha256: "c".repeat(64),
                    created_at_unix_ms: 1,
                    expires_at_unix_ms: 2,
                    retention_ms: preview_artifact::RETENTION_MS,
                },
                cache: preview_artifact::RequestReceipt {
                    input_sha256: "d".repeat(64),
                    reused: false,
                    created_at_unix_ms: 1,
                    expires_at_unix_ms: 2,
                    retention_ms: preview_artifact::RETENTION_MS,
                },
            },
        }
    }
    #[test]
    fn typed_native_manifest_roundtrips_and_foreign_changed_extra_inputs_refuse_before_auth() {
        let scratch = Scratch::new();
        let path = scratch.0.join("export.json");
        let export = exported();
        write_export(&path, &export).unwrap();
        assert_eq!(
            read_export("project", path.to_str().unwrap())
                .unwrap()
                .selection,
            export.selection
        );
        assert_eq!(
            read_export("foreign", path.to_str().unwrap())
                .unwrap_err()
                .code(),
            "asset_semantic_invalid"
        );
        let mut changed = serde_json::to_value(&export).unwrap();
        changed["selection"]["source"]["generation"] = json!(30);
        fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
        assert!(read_export("project", path.to_str().unwrap()).is_err());
        let mut extra = serde_json::to_value(&export).unwrap();
        extra["url"] = json!("https://example.test/source");
        fs::write(&path, serde_json::to_vec(&extra).unwrap()).unwrap();
        assert!(read_export("project", path.to_str().unwrap()).is_err());
        assert_eq!(
            lookup("canary", "project", "relative.json")
                .unwrap_err()
                .code(),
            "asset_semantic_invalid"
        );
        assert_eq!(
            read(
                "canary",
                "project",
                "relative.json",
                true,
                true,
                None,
                Retention::Hours24
            )
            .unwrap_err()
            .code(),
            "asset_semantic_invalid"
        );
    }
    #[test]
    fn explicit_outputs_preserve_existing_files_and_closed_extensions() {
        let scratch = Scratch::new();
        let path = scratch.0.join("manifest.json");
        new_path(path.to_str().unwrap(), "json").unwrap();
        write_export(&path, &exported()).unwrap();
        let before = fs::read(&path).unwrap();
        assert_eq!(
            new_path(path.to_str().unwrap(), "json").unwrap_err().code(),
            "report_output_exists"
        );
        assert!(write_export(&path, &exported()).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(new_path(scratch.0.join("wrong.pdf").to_str().unwrap(), "json").is_err());
        assert!(new_path("relative.json", "json").is_err());
    }
}
