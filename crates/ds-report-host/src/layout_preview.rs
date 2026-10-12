//! One captured layout page through the common scoped file-preview owner.
//! The fixed Reporter task remains the full RenderRequest decoder/renderer.
//! This host has no provider, project read, publication or default-style port.
use crate::{preview_artifact, HostFailure, ReportEngine};
use ds_command_kernel::{printing, report_export};
use ds_project_data::preview_room::Room;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{fs, io::Read, path::Path};

pub const MAX_CAPTURE_BYTES: u64 = 128 * 1024 * 1024;
const MAX_RESULT_BYTES: u64 = 1024 * 1024;

fn invalid(message: impl Into<String>) -> HostFailure {
    HostFailure::new(crate::CODE_INPUTS_INVALID, message)
}
fn result_invalid(message: impl Into<String>) -> HostFailure {
    HostFailure::new(crate::CODE_RESULT_INVALID, message)
}
fn sha(value: &Value) -> Result<String, HostFailure> {
    struct Hash(sha2::Sha256, u64);
    impl std::io::Write for Hash {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            use sha2::Digest;
            self.1 = self.1.saturating_add(bytes.len() as u64);
            if self.1 > MAX_CAPTURE_BYTES {
                return Err(std::io::Error::other(
                    "layout preview capture exceeds its byte bound",
                ));
            }
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    use sha2::Digest;
    let mut hash = Hash(sha2::Sha256::new(), 0);
    serde_json::to_writer(&mut hash, value).map_err(|e| invalid(e.to_string()))?;
    Ok(format!("{:x}", hash.0.finalize()))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Answer {
    pub project: String,
    pub lane: String,
    pub layout_id: String,
    pub page_mm: [f64; 2],
    pub page: preview_artifact::Artifact,
    pub engine: report_export::EngineIdentity,
    pub print_diagnostics: Value,
    pub preview_cache: preview_artifact::RequestReceipt,
}

/// Validate only the preview wrapper and native scope/default pins here.
/// The fixed producer decodes the complete existing RenderRequest, preserving
/// every supplied layout, geometry, source-metadata and style field.
fn admit(room: &Room, capture: &Value) -> Result<printing::Layout, HostFailure> {
    let fields = capture
        .as_object()
        .ok_or_else(|| invalid("layout capture must be an object"))?;
    if capture["schema"] != "ds.print-layout-export/v1"
        || !capture["render"].is_object()
        || fields.keys().any(|key| {
            !matches!(
                key.as_str(),
                "schema"
                    | "render"
                    | "renderer_defaults"
                    | "out_dir"
                    | "formats"
                    | "dpi"
                    | "additional_pages"
                    | "page_files"
                    | "additional_page_files"
                    | "page_cache_dir"
            )
        })
        || ["additional_pages", "page_files", "additional_page_files"]
            .iter()
            .any(|key| {
                capture
                    .get(*key)
                    .is_some_and(|v| !v.as_array().is_some_and(Vec::is_empty))
            })
        || capture.get("page_cache_dir").is_some_and(|v| !v.is_null())
    {
        return Err(invalid("preview requires one inline ds.print-layout-export/v1 page; file indirection, multipage and private page caches are refused"));
    }
    let binding: ds_command_kernel::style_resolution::Binding =
        serde_json::from_value(capture["renderer_defaults"].clone())
            .map_err(|e| invalid(format!("held renderer defaults: {e}")))?;
    printing::renderer_defaults::admit(&binding).map_err(invalid)?;
    if capture["render"]
        .get("renderer_defaults")
        .is_some_and(|v| !v.is_null() && *v != capture["renderer_defaults"])
    {
        return Err(invalid(
            "captured page and wrapper renderer policies disagree",
        ));
    }
    if let Some(value) = capture["render"]
        .get("project_crs")
        .filter(|v| !v.is_null())
    {
        let crs: printing::project_crs::Capture =
            serde_json::from_value(value.clone()).map_err(|e| invalid(e.to_string()))?;
        crs.validate_scope(
            &room.scope.project,
            &room.lane,
            &room.scope.principal,
            &room.credential_audience_sha256,
            None,
        )
        .map_err(invalid)?;
    }
    let layout: printing::Layout = serde_json::from_value(capture["render"]["layout"].clone())
        .map_err(|e| invalid(e.to_string()))?;
    printing::validate(&layout).map_err(invalid)?;
    Ok(layout)
}

pub fn execute(
    engine: &dyn ReportEngine,
    room: &Room,
    capture: &Value,
    retention: preview_artifact::Retention,
    staging_root: &Path,
) -> Result<Answer, HostFailure> {
    let layout = admit(room, capture)?;
    let identity = crate::engine_identity_now(engine)?;
    // Digest the entire semantic capture, including optional source metadata.
    // Only output transport/format are replaced by this explicit SVG mode.
    let mut semantic = capture.clone();
    let semantic_fields = semantic.as_object_mut().expect("admitted object");
    semantic_fields.remove("out_dir");
    semantic_fields.insert("formats".into(), json!(["svg"]));
    let provenance = json!({"subject":"captured-layout-svg","capture_sha256":sha(&semantic)?,
        "layout_sha256":sha(&serde_json::to_value(&layout).map_err(|e| invalid(e.to_string()))?)?,"engine":identity});
    let mut lease = preview_artifact::begin_request(room, provenance.clone(), retention)?;
    if let Some(metadata) = lease.retained() {
        let mut answer: Answer =
            serde_json::from_value(metadata.clone()).map_err(|e| result_invalid(e.to_string()))?;
        preview_artifact::verify(room, &answer.page, &provenance)?;
        if answer.project != room.scope.project
            || answer.lane != room.lane
            || answer.layout_id != layout.id
            || answer.page_mm != layout.page_mm
            || answer.engine != identity
            || answer.page.content_type != "image/svg+xml"
        {
            return Err(result_invalid(
                "retained layout preview metadata differs from admitted capture",
            ));
        }
        answer.preview_cache = lease.receipt(true)?;
        return Ok(answer);
    }
    let stage_path = staging_root.join(crate::random_run_id());
    crate::create_staging_root(&stage_path)?;
    let _stage = crate::Staging {
        path: stage_path.clone(),
    };
    let out_dir = stage_path.join("rendered");
    semantic["out_dir"] = json!(out_dir);
    let request_path = stage_path.join("request.json");
    let result_path = stage_path.join("result.json");
    let mut request_file = ds_layer_store::private::open_options()
        .write(true)
        .create_new(true)
        .open(&request_path)
        .map_err(|e| invalid(e.to_string()))?;
    serde_json::to_writer(&mut request_file, &semantic).map_err(|e| invalid(e.to_string()))?;
    request_file
        .sync_all()
        .map_err(|e| invalid(e.to_string()))?;
    drop(request_file);
    let exit = engine.render_print_layout(&request_path, &result_path)?;
    if !exit.succeeded {
        return Err(HostFailure::new(crate::CODE_EXPORT_BLOCKED, exit.summary));
    }
    if crate::engine_identity_now(engine)? != identity {
        return Err(HostFailure::new(
            crate::CODE_ENGINE_IDENTITY_CHANGED,
            "reporter changed during captured layout preview",
        ));
    }
    let result: Value = serde_json::from_slice(&read_regular(&result_path, MAX_RESULT_BYTES)?)
        .map_err(|e| result_invalid(e.to_string()))?;
    let rows = result["artifacts"]
        .as_array()
        .ok_or_else(|| result_invalid("layout result has no artifact inventory"))?;
    if result["schema"] != "ds.print-layout-result/v1"
        || result["status"] != "ok"
        || result["layout_id"] != layout.id
        || result["page_count"] != 1
        || result["page_mm"] != json!(layout.page_mm)
        || result["layout_sha256"] != provenance["layout_sha256"]
        || rows.len() != 1
        || rows[0]["format"] != "svg"
        || rows[0]["path"] != json!(out_dir.join("map.svg"))
        || !result["print_diagnostics"].is_object()
    {
        return Err(result_invalid(
            "fixed layout producer manifest differs from its one requested SVG page",
        ));
    }
    let bytes = read_regular(&out_dir.join("map.svg"), crate::MAX_PREVIEW_BYTES)?;
    if rows[0]["bytes"] != bytes.len() || rows[0]["sha256"] != report_export::sha256_hex(&bytes) {
        return Err(result_invalid(
            "layout SVG bytes differ from their producer inventory",
        ));
    }
    let svg = std::str::from_utf8(&bytes).map_err(|e| result_invalid(e.to_string()))?;
    let artifact =
        preview_artifact::materialize_svg_retained(room, svg, provenance.clone(), retention)?;
    let mut answer = Answer {
        project: room.scope.project.clone(),
        lane: room.lane.clone(),
        layout_id: layout.id,
        page_mm: layout.page_mm,
        page: artifact.clone(),
        engine: identity,
        print_diagnostics: result["print_diagnostics"].clone(),
        preview_cache: preview_artifact::RequestReceipt {
            input_sha256: String::new(),
            reused: false,
            created_at_unix_ms: artifact.created_at_unix_ms,
            expires_at_unix_ms: artifact.expires_at_unix_ms,
            retention_ms: artifact.retention_ms,
        },
    };
    lease.commit(
        serde_json::to_value(&answer).map_err(|e| result_invalid(e.to_string()))?,
        &artifact,
    )?;
    answer.preview_cache = lease.receipt(false)?;
    Ok(answer)
}

fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>, HostFailure> {
    let before = fs::symlink_metadata(path).map_err(|e| result_invalid(e.to_string()))?;
    if !before.is_file() || before.file_type().is_symlink() {
        return Err(result_invalid(
            "preview producer file must be regular and cannot be a link",
        ));
    }
    let mut options = ds_layer_store::private::open_options();
    options.read(true);
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32);
    }
    let file = options
        .open(path)
        .map_err(|e| result_invalid(e.to_string()))?;
    let meta = file.metadata().map_err(|e| result_invalid(e.to_string()))?;
    if !meta.is_file() || meta.len() == 0 || meta.len() > limit {
        return Err(result_invalid(
            "preview producer file is empty, not regular or exceeds its bound",
        ));
    }
    let mut bytes = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| result_invalid(e.to_string()))?;
    if bytes.len() as u64 > limit || bytes.len() as u64 != meta.len() {
        return Err(result_invalid(
            "preview producer file changed or exceeds its bound",
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    };
    struct Engine {
        renders: AtomicUsize,
        bad_digest: bool,
    }
    impl Engine {
        fn new() -> Self {
            Self {
                renders: AtomicUsize::new(0),
                bad_digest: false,
            }
        }
    }
    impl ReportEngine for Engine {
        fn build_info(&self) -> Result<Value, HostFailure> {
            Ok(
                json!({"schema":"ds.engine-build/v1","service":"ds-network-reporter","binary":"ds-report",
                "package_version":"0.1.0","source_sha":"a".repeat(40),"engine_version":format!("ds-network-reporter@0.1.0+{}","a".repeat(40)),
                "cargo_lock_sha256":"d".repeat(64),"features":[],"rustc":"rustc 1.97.0","target":"x86_64-unknown-linux-gnu",
                "profile":"release","schemas":["ds-network-reporter.task-result/v1"],"build_manifest_sha256":"f".repeat(64)}),
            )
        }
        fn export_transformer_report(
            &self,
            _: &Path,
            _: &Path,
        ) -> Result<crate::EngineExit, HostFailure> {
            panic!("captured layout preview must not export or analyze a transformer")
        }
        fn render_print_layout(
            &self,
            request: &Path,
            result: &Path,
        ) -> Result<crate::EngineExit, HostFailure> {
            self.renders.fetch_add(1, Ordering::SeqCst);
            let capture: Value = serde_json::from_slice(&fs::read(request).unwrap()).unwrap();
            assert_eq!(capture["formats"], json!(["svg"]));
            assert!(capture.get("page_cache_dir").is_none_or(Value::is_null));
            let layout: printing::Layout =
                serde_json::from_value(capture["render"]["layout"].clone()).unwrap();
            let out = Path::new(capture["out_dir"].as_str().unwrap());
            fs::create_dir(out).unwrap();
            let path = out.join("map.svg");
            let body =
                b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"1\" height=\"1\"/></svg>";
            fs::write(&path, body).unwrap();
            fs::write(result,serde_json::to_vec(&json!({"schema":"ds.print-layout-result/v1","status":"ok",
                "layout_id":layout.id,"page_mm":layout.page_mm,"page_count":1,"layout_sha256":sha(&serde_json::to_value(&layout).unwrap()).unwrap(),
                "artifacts":[{"format":"svg","path":path,"bytes":body.len(),"sha256":if self.bad_digest {"0".repeat(64)} else {report_export::sha256_hex(body)}}],
                "print_diagnostics":{"checked_pages":1,"affected_pages":0,"truncated":false,"pages":[]}})).unwrap()).unwrap();
            Ok(crate::EngineExit {
                succeeded: true,
                summary: String::new(),
            })
        }
    }
    fn fixture(root: &Path) -> (Room, Value) {
        let room = Room {
            root: root.into(),
            scope: ds_command_kernel::project_dataset_cache::Scope {
                principal: "owner".into(),
                project: "project".into(),
            },
            lane: "canary".into(),
            credential_audience_sha256: "b".repeat(64),
        };
        let defaults: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/governed-renderer-defaults.json"
        ))
        .unwrap();
        let document = json!({"type":"line","paint":{},"layout":{},"metadata":{"print_renderer_defaults":defaults}});
        let binding = ds_command_kernel::style_resolution::Binding {
            key: printing::renderer_defaults::key(),
            style_ref: "test/renderer_defaults_print".into(),
            revision_id: "a".repeat(64),
            content_sha256: ds_command_kernel::style_plan::style_digest(&document),
            scope: "catalogue".into(),
            document,
            presentation_tier: None,
        };
        let crs = printing::project_crs::Capture::new(
            "project",
            "canary",
            "owner",
            &"b".repeat(64),
            json!({"crs":{"mode":"utm","utm_zone":36,"utm_hemisphere":"south"}}),
        )
        .unwrap();
        let capture = json!({"schema":"ds.print-layout-export/v1","out_dir":"never-created-delivery",
            "formats":["pdf"],"renderer_defaults":binding,"render":{"layout":printing::default_layout(),"layers":[],
                "project_crs":crs,"text":{"title":"Captured page"},"report_config":{"source_digest_metadata":{"sha256":"c".repeat(64)}}}});
        (room, capture)
    }
    #[test]
    fn simultaneous_and_warm_captured_pages_use_one_fixed_renderer_and_common_svg_file() {
        let dir = tempfile::tempdir().unwrap();
        let (room, capture) = fixture(dir.path());
        let original = capture.clone();
        let engine = Arc::new(Engine::new());
        let barrier = Arc::new(Barrier::new(8));
        std::thread::scope(|threads| {
            for _ in 0..8 {
                let (engine, barrier, room, capture) =
                    (engine.clone(), barrier.clone(), &room, &capture);
                threads.spawn(move || {
                    barrier.wait();
                    execute(
                        engine.as_ref(),
                        room,
                        capture,
                        preview_artifact::Retention::Hours24,
                        &room.root.join("scratch"),
                    )
                    .unwrap();
                });
            }
        });
        assert_eq!(engine.renders.load(Ordering::SeqCst), 1);
        let warm = execute(
            engine.as_ref(),
            &room,
            &capture,
            preview_artifact::Retention::Hours24,
            &dir.path().join("unconsumed-scratch"),
        )
        .unwrap();
        assert!(warm.preview_cache.reused);
        assert!(!dir.path().join("unconsumed-scratch").exists());
        assert_eq!(engine.renders.load(Ordering::SeqCst), 1);
        assert_eq!(warm.page.content_type, "image/svg+xml");
        assert!(preview_artifact::read(&room, &warm.page)
            .unwrap()
            .starts_with(b"<svg"));
        assert_eq!(fs::read_dir(dir.path().join("scratch")).unwrap().count(), 0);
        assert_eq!(
            capture, original,
            "preview never changes the authored capture"
        );
    }
    #[test]
    fn source_geometry_text_policy_and_project_changes_never_reuse_the_wrong_page() {
        let dir = tempfile::tempdir().unwrap();
        let (room, capture) = fixture(dir.path());
        let engine = Engine::new();
        let mut keys = std::collections::BTreeSet::new();
        let first = execute(
            &engine,
            &room,
            &capture,
            preview_artifact::Retention::Hours24,
            &dir.path().join("scratch"),
        )
        .unwrap();
        keys.insert(first.preview_cache.input_sha256);
        for field in 0..5 {
            let mut changed = capture.clone();
            let mut target = room.clone();
            match field {
                0 => changed["render"]["text"]["title"] = json!("Changed title"),
                1 => {
                    changed["render"]["layers"] = json!([{"id":"source","features":[{"type":"Feature","geometry":{"type":"Point","coordinates":[30,-2]},"properties":{}}]}])
                }
                2 => {
                    changed["render"]["report_config"]["source_digest_metadata"]["sha256"] =
                        json!("d".repeat(64))
                }
                3 => changed["renderer_defaults"]["revision_id"] = json!("e".repeat(64)),
                _ => {
                    target.scope.project = "other-project".into();
                    changed["render"]["project_crs"]["project"] = json!("other-project");
                }
            }
            let rendered = execute(
                &engine,
                &target,
                &changed,
                preview_artifact::Retention::Hours24,
                &dir.path().join("scratch"),
            )
            .unwrap();
            assert!(!rendered.preview_cache.reused);
            assert!(keys.insert(rendered.preview_cache.input_sha256) || field == 4);
            if field == 4 {
                assert_ne!(rendered.page.authority_sha256, first.page.authority_sha256);
            }
        }
        assert_eq!(engine.renders.load(Ordering::SeqCst), 6);
        // out_dir/formats are transport/delivery choices, not a different SVG.
        let mut transport = capture.clone();
        transport["out_dir"] = json!("other-unused-delivery");
        transport["formats"] = json!(["png"]);
        assert!(
            execute(
                &engine,
                &room,
                &transport,
                preview_artifact::Retention::Hours24,
                &dir.path().join("scratch")
            )
            .unwrap()
            .preview_cache
            .reused
        );
        assert_eq!(engine.renders.load(Ordering::SeqCst), 6);
    }
    #[test]
    fn foreign_authority_missing_policy_and_file_indirection_refuse_before_render_or_scratch() {
        let dir = tempfile::tempdir().unwrap();
        let (room, capture) = fixture(dir.path());
        let engine = Engine::new();
        for field in 0..4 {
            let mut foreign = room.clone();
            match field {
                0 => foreign.scope.project = "other".into(),
                1 => foreign.scope.principal = "other".into(),
                2 => foreign.lane = "stable".into(),
                _ => foreign.credential_audience_sha256 = "d".repeat(64),
            };
            assert_eq!(
                execute(
                    &engine,
                    &foreign,
                    &capture,
                    preview_artifact::Retention::Hours24,
                    &dir.path().join("scratch")
                )
                .unwrap_err()
                .code,
                crate::CODE_INPUTS_INVALID
            );
        }
        for field in ["renderer_defaults", "page_files"] {
            let mut invalid = capture.clone();
            invalid[field] = if field == "renderer_defaults" {
                Value::Null
            } else {
                json!(["unread-file.json"])
            };
            assert!(execute(
                &engine,
                &room,
                &invalid,
                preview_artifact::Retention::Hours24,
                &dir.path().join("scratch")
            )
            .is_err());
        }
        assert_eq!(engine.renders.load(Ordering::SeqCst), 0);
        assert!(!dir.path().join("scratch").exists());
    }
    #[test]
    fn a_mismatched_real_producer_file_is_never_a_committed_preview() {
        let dir = tempfile::tempdir().unwrap();
        let (room, capture) = fixture(dir.path());
        let engine = Engine {
            renders: AtomicUsize::new(0),
            bad_digest: true,
        };
        assert_eq!(
            execute(
                &engine,
                &room,
                &capture,
                preview_artifact::Retention::Hours24,
                &dir.path().join("scratch")
            )
            .unwrap_err()
            .code,
            crate::CODE_RESULT_INVALID
        );
        assert_eq!(fs::read_dir(dir.path().join("scratch")).unwrap().count(), 0);
        let repaired = Engine::new();
        let answer = execute(
            &repaired,
            &room,
            &capture,
            preview_artifact::Retention::Hours24,
            &dir.path().join("scratch"),
        )
        .unwrap();
        assert!(!answer.preview_cache.reused);
        assert_eq!(repaired.renders.load(Ordering::SeqCst), 1);
    }
}
