//! Exact-byte export/import preparation for an external extractor over the
//! existing native document room. No transport, extraction or second cache.
use crate::{asset_preview, preview_artifact, HostFailure};
use ds_command_kernel::{
    asset_preview::{Request, REQUEST_SCHEMA},
    assets::semantic::{self, Extractor, Output, Prepare, Selection, SourceKind, SourcePin},
};
use ds_project_data::preview_room::Room;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub selection: Selection,
    pub path: String,
    pub native_source: asset_preview::Receipt,
}
fn fail(message: impl Into<String>) -> HostFailure {
    HostFailure::new("asset_semantic_invalid", message)
}
struct Offline;
impl asset_preview::SourceProvider for Offline {
    fn authorize(
        &mut self,
        _: &Request,
        _: &dyn asset_preview::Control,
    ) -> Result<ds_command_kernel::asset_preview::SourceProof, asset_preview::SourceFailure> {
        Err(fail("semantic retained export cannot authorize an unselected source").into())
    }
    fn acquire(
        &mut self,
        _: &ds_command_kernel::asset_preview::SourceProof,
        _: &mut fs::File,
        _: &dyn asset_preview::Control,
    ) -> Result<(), HostFailure> {
        Err(fail(
            "semantic retained export cannot fetch an unselected source",
        ))
    }
}
fn admitted_source(
    room: &Room,
    receipt: &asset_preview::Receipt,
    control: &dyn asset_preview::Control,
) -> Result<asset_preview::Receipt, HostFailure> {
    let retention = match receipt.artifact.retention_ms {
        preview_artifact::RETENTION_MS => preview_artifact::Retention::Hours24,
        preview_artifact::LONG_RETENTION_MS => preview_artifact::Retention::Days7,
        _ => return Err(fail("retained source expiry is not admitted")),
    };
    let request = Request {
        schema: REQUEST_SCHEMA.into(),
        asset_id: receipt.source.asset_id.clone(),
        expected_revision: Some(receipt.source.revision.clone()),
        expected_generation: Some(receipt.source.generation.clone()),
        expected_sha256: Some(receipt.measured_sha256.clone()),
        offline: true,
        retention,
    };
    let held = asset_preview::acquire(room, &request, &mut Offline, control)?;
    if held.source != receipt.source
        || held.measured_sha256 != receipt.measured_sha256
        || held.artifact != receipt.artifact
    {
        return Err(fail(
            "source receipt differs from its exact admitted disk manifest",
        ));
    }
    Ok(held)
}
/// Reuse the native source's sealed request/result, measuring no substitute
/// input and making zero source calls. Hosts capture current native authority.
pub fn selection(
    room: &Room,
    receipt: &asset_preview::Receipt,
    extractor: Extractor,
    control: &dyn asset_preview::Control,
) -> Result<Selection, HostFailure> {
    let held = admitted_source(room, receipt, control)?;
    let (kind, prefix) = match held.source.source_kind.as_str() {
        "catalogue_asset" => (SourceKind::CatalogueAsset, "catalogue-version:"),
        "pm_attachment" => (SourceKind::PmAttachment, "pm-version:"),
        _ => return Err(fail("source kind has no semantic owner")),
    };
    let version = held
        .source
        .revision
        .strip_prefix(prefix)
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or_else(|| fail("source has no actual native version pin"))?;
    let generation = held
        .source
        .generation
        .parse::<u64>()
        .map_err(|_| fail("source has no actual storage generation"))?;
    let selection = Selection {
        source: SourcePin {
            kind,
            id: held.source.source_id,
            version,
            generation,
            size_bytes: held.source.size_bytes,
            content_type: held.source.content_type,
            measured_sha256: held.measured_sha256,
        },
        extractor,
    };
    selection.validate(&room.scope.project).map_err(fail)?;
    Ok(selection)
}
pub fn export(
    room: &Room,
    receipt: &asset_preview::Receipt,
    extractor: Extractor,
    out: &str,
    control: &dyn asset_preview::Control,
) -> Result<Export, HostFailure> {
    control.check_current()?;
    let selection = selection(room, receipt, extractor, control)?;
    let path = preview_artifact::copy(room, &receipt.artifact, out)?;
    control.check_current()?;
    Ok(Export {
        selection,
        path: path.to_string_lossy().into(),
        native_source: receipt.clone(),
    })
}
fn markdown(bytes: &[u8]) -> Result<Output, HostFailure> {
    if bytes.len() as u64 > semantic::MAX_OUTPUT_BYTES
        || !std::str::from_utf8(bytes).is_ok_and(|s| !s.trim().is_empty())
    {
        return Err(fail(
            "external output must be bounded nonempty UTF-8 Markdown",
        ));
    }
    Ok(Output {
        sha256: semantic::digest(bytes),
        bytes: bytes.len() as u64,
    })
}
/// A local file is selected deliberately, never a URI or an extractor command.
pub fn prepare_file(
    project: &str,
    selection: Selection,
    path: &Path,
) -> Result<(Prepare, Vec<u8>), HostFailure> {
    selection.validate(project).map_err(fail)?;
    if !path.is_absolute() {
        return Err(fail("semantic output must name one absolute local file"));
    }
    let metadata = fs::symlink_metadata(path).map_err(|e| fail(e.to_string()))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() == 0
        || metadata.len() > semantic::MAX_OUTPUT_BYTES
    {
        return Err(fail("semantic output is not one bounded regular file"));
    }
    let mut file = fs::File::open(path).map_err(|e| fail(e.to_string()))?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(semantic::MAX_OUTPUT_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| fail(e.to_string()))?;
    if bytes.len() as u64 != metadata.len() {
        return Err(fail("semantic output changed while it was measured"));
    }
    let output = markdown(&bytes)?;
    Ok((Prepare { selection, output }, bytes))
}

fn inputs(project: &str, selection: &Selection) -> Value {
    let asset = match selection.source.kind {
        SourceKind::CatalogueAsset => selection.source.id.clone(),
        SourceKind::PmAttachment => format!("sys:pm_attachment:{}", selection.source.id),
    };
    json!({"schema":semantic::SCHEMA,"project_id":project,"asset_id":asset,"selection":selection})
}
fn provenance(project: &str, prepare: &Prepare, receipt: &Value) -> Result<Value, HostFailure> {
    Ok(
        json!({"schema":semantic::SCHEMA,"project_id":project,"selection":prepare.selection,"manifest_sha256":prepare.manifest_sha256(project).map_err(fail)?,"output_generation":receipt["output_generation"]}),
    )
}
/// Selected offline/repeated consumers join the same native request index;
/// this never checks cloud freshness or authorizes an unselected source.
pub fn read_retained(
    room: &Room,
    selection: &Selection,
    retention: preview_artifact::Retention,
) -> Result<Option<(Value, preview_artifact::Artifact)>, HostFailure> {
    selection.validate(&room.scope.project).map_err(fail)?;
    let lease =
        preview_artifact::begin_request(room, inputs(&room.scope.project, selection), retention)?;
    let Some(held) = lease.retained() else {
        return Ok(None);
    };
    let receipt = held["receipt"].clone();
    let output = Output {
        sha256: receipt["output"]["sha256"]
            .as_str()
            .unwrap_or_default()
            .into(),
        bytes: receipt["output"]["bytes"].as_u64().unwrap_or_default(),
    };
    let prepare = Prepare {
        selection: selection.clone(),
        output,
    };
    semantic::Command::Prepare(prepare.clone())
        .verify_response(&room.scope.project, &receipt)
        .map_err(fail)?;
    if receipt["state"] != "ready" {
        return Err(fail("partial semantic upload is not reusable"));
    }
    let artifact: preview_artifact::Artifact =
        serde_json::from_value(held["artifact"].clone()).map_err(|e| fail(e.to_string()))?;
    preview_artifact::verify(
        room,
        &artifact,
        &provenance(&room.scope.project, &prepare, &receipt)?,
    )?;
    if artifact.sha256 != prepare.output.sha256 || artifact.size_bytes != prepare.output.bytes {
        return Err(fail(
            "retained semantic file differs from its exact output pins",
        ));
    }
    Ok(Some((receipt, artifact)))
}
/// A real source-owner denial revokes all source-bound retained documents
/// through the existing shared manifest guard before cleanup can fail.
pub fn forget_source(room: &Room, selection: &Selection) -> Result<usize, HostFailure> {
    let data = inputs(&room.scope.project, selection);
    preview_artifact::forget_asset_requests(
        room,
        data["asset_id"]
            .as_str()
            .ok_or_else(|| fail("source identity is missing"))?,
    )
}
/// Admit only a ready server receipt and exact checked bytes. An upload or a
/// failed finalize never enters the reusable semantic request index.
pub fn retain_ready(
    room: &Room,
    prepare: &Prepare,
    receipt: &Value,
    bytes: &[u8],
    retention: preview_artifact::Retention,
) -> Result<preview_artifact::Artifact, HostFailure> {
    semantic::Command::Prepare(prepare.clone())
        .verify_response(&room.scope.project, receipt)
        .map_err(fail)?;
    if receipt["state"] != "ready" || markdown(bytes)? != prepare.output {
        return Err(fail("only exact ready semantic bytes can be retained"));
    }
    let provenance = provenance(&room.scope.project, prepare, receipt)?;
    let mut lease = preview_artifact::begin_request(
        room,
        inputs(&room.scope.project, &prepare.selection),
        retention,
    )?;
    if let Some(held) = lease.retained() {
        let artifact: preview_artifact::Artifact =
            serde_json::from_value(held["artifact"].clone()).map_err(|e| fail(e.to_string()))?;
        preview_artifact::verify(room, &artifact, &provenance)?;
        if artifact.sha256 != prepare.output.sha256 || artifact.size_bytes != prepare.output.bytes {
            return Err(fail("retained semantic bytes differ from their manifest"));
        }
        return Ok(artifact);
    }
    let artifact =
        preview_artifact::materialize_markdown_retained(room, bytes, provenance, retention)?;
    lease.commit(json!({"receipt":receipt,"artifact":artifact}), &artifact)?;
    Ok(artifact)
}
pub fn copy_markdown(
    room: &Room,
    artifact: &preview_artifact::Artifact,
    out: &str,
) -> Result<PathBuf, HostFailure> {
    preview_artifact::copy(room, artifact, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::Write,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
    };
    const PDF: &[u8] = b"%PDF-1.7 native semantic test bytes\n";
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
    struct Guard;
    impl asset_preview::Control for Guard {
        fn check_current(&self) -> Result<(), HostFailure> {
            Ok(())
        }
        fn progress(&self, _: asset_preview::Phase, _: u64, _: Option<u64>) {}
    }
    struct Provider {
        calls: Arc<AtomicUsize>,
    }
    impl asset_preview::SourceProvider for Provider {
        fn authorize(
            &mut self,
            _: &Request,
            _: &dyn asset_preview::Control,
        ) -> Result<ds_command_kernel::asset_preview::SourceProof, asset_preview::SourceFailure>
        {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(ds_command_kernel::asset_preview::SourceProof {
                project_id: "project".into(),
                asset_id: "sys:pm_attachment:pdf-one".into(),
                source_kind: "pm_attachment".into(),
                source_id: "pdf-one".into(),
                revision: "pm-version:7".into(),
                generation: "29".into(),
                advertised_sha256: None,
                size_bytes: PDF.len() as u64,
                content_type: "application/pdf".into(),
                sensitivity: "internal".into(),
                name: "Meeting.pdf".into(),
            })
        }
        fn acquire(
            &mut self,
            _: &ds_command_kernel::asset_preview::SourceProof,
            file: &mut fs::File,
            _: &dyn asset_preview::Control,
        ) -> Result<(), HostFailure> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            file.write_all(PDF).map_err(|e| fail(e.to_string()))
        }
    }
    fn extractor() -> Extractor {
        Extractor {
            name: "markitdown".into(),
            version: "0.1.8".into(),
            options_sha256: semantic::digest(b"{}"),
        }
    }
    fn source(room: &Room, calls: Arc<AtomicUsize>) -> asset_preview::Receipt {
        asset_preview::acquire(
            room,
            &Request {
                schema: REQUEST_SCHEMA.into(),
                asset_id: "sys:pm_attachment:pdf-one".into(),
                expected_revision: None,
                expected_generation: None,
                expected_sha256: None,
                offline: false,
                retention: preview_artifact::Retention::Hours24,
            },
            &mut Provider { calls },
            &Guard,
        )
        .unwrap()
    }
    fn ready(room: &Room, p: &Prepare) -> Value {
        json!({"schema":semantic::SCHEMA,"project_id":room.scope.project,"derivative_id":p.selection.derivative_id(&room.scope.project).unwrap(),"manifest_sha256":p.manifest_sha256(&room.scope.project).unwrap(),"source":p.selection.source,"extractor":p.selection.extractor,"output":{"sha256":p.output.sha256,"bytes":p.output.bytes,"content_type":semantic::MARKDOWN_TYPE},"output_generation":31,"state":"ready","source_sha256_basis":"native_measured"})
    }
    #[test]
    fn export_is_exact_retained_source_with_zero_repeat_authorization_or_download() {
        let dir = tempfile::tempdir().unwrap();
        let r = room(dir.path().into());
        let calls = Arc::new(AtomicUsize::new(0));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let held = source(&r, calls.clone());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let out = dir.path().join("export.pdf");
        let result = export(&r, &held, extractor(), out.to_str().unwrap(), &Guard).unwrap();
        assert_eq!(fs::read(out).unwrap(), PDF);
        assert_eq!(result.selection.source.version, 7);
        assert_eq!(result.selection.source.generation, 29);
        assert_eq!(
            result.selection.source.measured_sha256,
            semantic::digest(PDF)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let mut forged = held.clone();
        forged.source.name = "Other.pdf".into();
        assert!(selection(&r, &forged, extractor(), &Guard).is_err());
        let mut foreign = r.clone();
        foreign.scope.principal = "other".into();
        assert!(selection(&foreign, &held, extractor(), &Guard).is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
    #[test]
    fn ready_markdown_reuses_shared_room_and_denial_revokes_its_source() {
        let dir = tempfile::tempdir().unwrap();
        let r = room(dir.path().into());
        let calls = Arc::new(AtomicUsize::new(0));
        let held = source(&r, calls.clone());
        let selected = selection(&r, &held, extractor(), &Guard).unwrap();
        let bytes = "# Meeting\nExact extraction with Unicode: caf\u{e9}.\n".as_bytes();
        let p = Prepare {
            selection: selected.clone(),
            output: markdown(bytes).unwrap(),
        };
        let receipt = ready(&r, &p);
        let artifact = retain_ready(
            &r,
            &p,
            &receipt,
            bytes,
            preview_artifact::Retention::Hours24,
        )
        .unwrap();
        assert_eq!(artifact.content_type, semantic::MARKDOWN_TYPE);
        assert_eq!(preview_artifact::read(&r, &artifact).unwrap(), bytes);
        let second = retain_ready(
            &r,
            &p,
            &receipt,
            bytes,
            preview_artifact::Retention::Hours24,
        )
        .unwrap();
        assert_eq!(artifact, second);
        assert_eq!(
            read_retained(&r, &selected, preview_artifact::Retention::Hours24)
                .unwrap()
                .unwrap()
                .1,
            artifact
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let out = dir.path().join("extract.md");
        copy_markdown(&r, &artifact, out.to_str().unwrap()).unwrap();
        assert_eq!(fs::read(out).unwrap(), bytes);
        let mut changed = selected.clone();
        changed.extractor.version = "0.1.9".into();
        assert!(
            read_retained(&r, &changed, preview_artifact::Retention::Hours24)
                .unwrap()
                .is_none()
        );
        assert!(forget_source(&r, &selected).unwrap() > 0);
        assert!(preview_artifact::read(&r, &artifact).is_err());
        assert!(preview_artifact::read(&r, &held.artifact).is_err());
    }
    #[test]
    fn partial_corrupt_and_empty_outputs_do_not_enter_reusable_derivative_index() {
        let dir = tempfile::tempdir().unwrap();
        let r = room(dir.path().into());
        let held = source(&r, Arc::new(AtomicUsize::new(0)));
        let selected = selection(&r, &held, extractor(), &Guard).unwrap();
        let bytes = b"# Ready\n";
        let p = Prepare {
            selection: selected.clone(),
            output: markdown(bytes).unwrap(),
        };
        let mut receipt = ready(&r, &p);
        receipt["state"] = json!("uploading");
        assert!(retain_ready(
            &r,
            &p,
            &receipt,
            bytes,
            preview_artifact::Retention::Hours24
        )
        .is_err());
        assert!(
            read_retained(&r, &selected, preview_artifact::Retention::Hours24)
                .unwrap()
                .is_none()
        );
        assert!(retain_ready(
            &r,
            &p,
            &ready(&r, &p),
            b"changed",
            preview_artifact::Retention::Hours24
        )
        .is_err());
        assert!(markdown(b" \n").is_err());
        assert!(markdown(&[0xff]).is_err());
        assert!(preview_artifact::materialize_markdown_retained(
            &r,
            &[0xff],
            json!({}),
            preview_artifact::Retention::Hours24
        )
        .is_err());
    }
}
