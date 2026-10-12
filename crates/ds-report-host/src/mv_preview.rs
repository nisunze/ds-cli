//! MV plan/profile template preview, shared by every door.
//!
//! The host stages the held engine projections in its own private directory,
//! the kernel composes the request (`printing::mv_preview`), the engine renders
//! one `preview_only` sample, and the host answers that sheet's SVG with its
//! station window. Runs stay private scratch. An authenticated host may attach
//! the shared local preview room so navigation can reuse the exact result.

use std::path::{Path, PathBuf};

use ds_command_kernel::printing::{self, mv, mv_preview, project_crs};
use ds_command_kernel::style_resolution::Snapshot;
use serde_json::{json, Value};

use crate::{HostFailure, ReportEngine, CODE_ENGINE_REFUSED, CODE_RESULT_INVALID};
use crate::{CODE_INPUTS_INVALID, CODE_STAGING_FAILED, MAX_PREVIEW_BYTES};

/// The largest held projection a door hands over for one preview.
pub const MAX_PROJECTION_BYTES: usize = 256 * 1024 * 1024;

/// What a door holds for one MV template preview, read without a decision.
pub struct MvPreviewInputs<'a> {
    pub project_id: &'a str,
    /// The server-issued configuration sheets (the input receipt's own bytes).
    pub sheets: &'a Value,
    /// The previewed MV layout, saved or draft.
    pub layout: &'a printing::Layout,
    /// The project's governed style table.
    pub table: &'a Snapshot,
    pub project_crs: Option<&'a project_crs::Capture>,
    pub model_crs: Option<&'a str>,
    /// The held model's `project_profile_atlas` scene JSON.
    pub scene: &'a [u8],
    /// The same revision's plan projection JSON.
    pub plan: &'a [u8],
    /// A host-private root the run directory is created under.
    pub staging: &'a Path,
}

/// The rendered sample sheet.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MvPreviewSheet {
    pub source_sheet_number: u64,
    pub alignment_id: String,
    pub from_station_m: f64,
    pub to_station_m: f64,
    pub svg: String,
}

/// One preview's answer: the pinned model and setup and the sample sheet.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MvPreviewAnswer {
    pub project_id: String,
    pub layout_id: String,
    pub model_revision: String,
    pub setup_sha256: String,
    pub page_count: u64,
    pub full_page_count: u64,
    pub sheet: MvPreviewSheet,
}

impl MvPreviewAnswer {
    /// The envelope `data` every door answers with.
    pub fn to_value(&self) -> Value {
        json!({
            "project": self.project_id,
            "layout_id": self.layout_id,
            "model_revision": self.model_revision,
            "setup_sha256": self.setup_sha256,
            "page_count": self.page_count,
            "full_page_count": self.full_page_count,
            "sheet": {
                "source_sheet_number": self.sheet.source_sheet_number,
                "alignment_id": self.sheet.alignment_id,
                "from_station_m": self.sheet.from_station_m,
                "to_station_m": self.sheet.to_station_m,
                "svg": self.sheet.svg,
            },
        })
    }
}

/// Why no sheet came back: the kernel's keyed refusal or a host failure.
#[derive(Debug, Clone, PartialEq)]
pub enum MvPreviewRefusal {
    Kernel(mv::Refusal),
    Host(HostFailure),
}

impl From<HostFailure> for MvPreviewRefusal {
    fn from(failure: HostFailure) -> Self {
        Self::Host(failure)
    }
}

/// One preview's private directory, removed however the run ends.
struct RunDir(PathBuf);

impl RunDir {
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for RunDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn execute(
    engine: &dyn ReportEngine,
    inputs: &MvPreviewInputs<'_>,
) -> Result<MvPreviewAnswer, MvPreviewRefusal> {
    execute_retained(engine, inputs, None)
}

/// Resolve the same kernel request before reading an exact retained result.
/// Transformer and MV previews use one account/project report-hold owner.
pub fn execute_retained(
    engine: &dyn ReportEngine,
    inputs: &MvPreviewInputs<'_>,
    result_room: Option<&ds_project_data::preview_room::Room>,
) -> Result<MvPreviewAnswer, MvPreviewRefusal> {
    for (name, bytes) in [("scene", inputs.scene), ("plan", inputs.plan)] {
        if bytes.is_empty() || bytes.len() > MAX_PROJECTION_BYTES {
            return Err(HostFailure::new(
                CODE_INPUTS_INVALID,
                format!(
                    "the held {name} projection is empty or exceeds {MAX_PROJECTION_BYTES} bytes"
                ),
            )
            .into());
        }
    }
    let run = RunDir(
        inputs
            .staging
            .join(format!("mv-preview-{}", crate::random_run_id())),
    );
    std::fs::create_dir(run.path())
        .map_err(|error| HostFailure::new(CODE_STAGING_FAILED, error.to_string()))?;
    let scene = run.path().join("profile-scene.json");
    let plan = run.path().join("project-plan.json");
    let out_dir = run.path().join("out");
    for (path, bytes) in [(&scene, inputs.scene), (&plan, inputs.plan)] {
        std::fs::write(path, bytes)
            .map_err(|error| HostFailure::new(CODE_STAGING_FAILED, error.to_string()))?;
    }
    let preview = mv_preview::request(
        inputs.project_id,
        inputs.sheets,
        inputs.layout,
        inputs.table,
        inputs.project_crs,
        inputs.model_crs,
        mv_preview::Paths {
            scene: &scene,
            plan: &plan,
            out_dir: &out_dir,
        },
    )
    .map_err(MvPreviewRefusal::Kernel)?;
    let retained = result_room.map(|room| {
        if room.scope.project != inputs.project_id {
            return Err(HostFailure::new(CODE_INPUTS_INVALID, "the MV preview result room belongs to another project"));
        }
        let identity = crate::engine_identity_now(engine)?;
        // The effective native policy is part of the result identity. Private
        // per-run paths transport the already-digested scene/plan and output.
        let mut effective = preview.request.clone();
        for path in ["scene_path", "plan_path", "out_dir"] {
            effective.as_object_mut().expect("native MV request object").remove(path);
        }
        let key = json!({"project":inputs.project_id,"sheets":inputs.sheets,"layout":inputs.layout,
            "styles":inputs.table,"project_crs":inputs.project_crs,"model_crs":inputs.model_crs,
            "scene_sha256": ds_command_kernel::report_export::sha256_hex(inputs.scene),
            "plan_sha256": ds_command_kernel::report_export::sha256_hex(inputs.plan),"engine":identity,"request":effective});
        Ok((room, key, identity))
    }).transpose()?;
    let render = || -> Result<MvPreviewAnswer, MvPreviewRefusal> {
        let request_path = run.path().join("request.json");
        let result_path = run.path().join("result.json");
        let request_bytes = serde_json::to_vec(&preview.request)
            .map_err(|error| HostFailure::new(CODE_STAGING_FAILED, error.to_string()))?;
        std::fs::write(&request_path, request_bytes)
            .map_err(|error| HostFailure::new(CODE_STAGING_FAILED, error.to_string()))?;
        let exit = engine.render_grid_plan_profile(&request_path, &result_path)?;
        let document: Option<Value> = std::fs::read(&result_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok());
        let Some(document) = document.filter(|_| exit.succeeded) else {
            return Err(HostFailure::new(
                CODE_ENGINE_REFUSED,
                format!(
                    "the MV plan and profile preview did not render: {}",
                    exit.summary
                ),
            )
            .into());
        };
        answer(inputs, &preview, &document, &out_dir).map_err(MvPreviewRefusal::Host)
    };
    let Some((room, key, identity)) = retained else {
        return render();
    };
    let answer = ds_project_data::preview_room::consume(room, "mv/models", &key, render).map_err(
        |error| match error {
            ds_project_data::preview_room::Error::Render(error) => error,
            ds_project_data::preview_room::Error::Store(message) => {
                MvPreviewRefusal::Host(HostFailure::new(CODE_STAGING_FAILED, message))
            }
        },
    )?;
    if crate::engine_identity_now(engine)? != identity {
        return Err(HostFailure::new(
            crate::CODE_ENGINE_IDENTITY_CHANGED,
            "the reporter build changed while reading a retained MV preview",
        )
        .into());
    }
    Ok(answer)
}

fn answer(
    inputs: &MvPreviewInputs<'_>,
    preview: &mv_preview::Preview,
    document: &Value,
    out_dir: &Path,
) -> Result<MvPreviewAnswer, HostFailure> {
    let invalid = |message: &str| HostFailure::new(CODE_RESULT_INVALID, message.to_string());
    if document["project_id"] != inputs.project_id
        || document["preview_only"] != true
        || document["mv_setup_sha256"] != preview.setup.sha256.as_str()
    {
        return Err(invalid(
            "the engine answered another project, setup or a non-preview render",
        ));
    }
    let model_revision = document["model_revision"]
        .as_str()
        .ok_or_else(|| invalid("the preview names no model revision"))?;
    let window = &document["page_station_windows"][0];
    let path = document["previews"][0]
        .as_str()
        .map(PathBuf::from)
        .ok_or_else(|| invalid("the preview wrote no sample sheet"))?;
    // Only a sheet this run's engine wrote under its own output is read.
    if path.parent() != Some(out_dir.join("preview").as_path())
        || path.extension().and_then(|e| e.to_str()) != Some("svg")
    {
        return Err(invalid("the sample sheet lies outside this run's output"));
    }
    let size = std::fs::metadata(&path)
        .map_err(|error| invalid(&format!("the sample sheet is unreadable: {error}")))?
        .len();
    if size == 0 || size > MAX_PREVIEW_BYTES {
        return Err(invalid(
            "the sample sheet is empty or exceeds the preview bound",
        ));
    }
    let svg = std::fs::read_to_string(&path)
        .map_err(|error| invalid(&format!("the sample sheet is not UTF-8 SVG: {error}")))?;
    if !svg.trim_start().starts_with("<svg") && !svg.trim_start().starts_with("<?xml") {
        return Err(invalid("the sample sheet is not an SVG document"));
    }
    let count = |key: &str| document[key].as_u64().unwrap_or(0);
    Ok(MvPreviewAnswer {
        project_id: inputs.project_id.into(),
        layout_id: inputs.layout.id.clone(),
        model_revision: model_revision.into(),
        setup_sha256: preview.setup.sha256.clone(),
        page_count: count("page_count"),
        full_page_count: count("full_page_count"),
        sheet: MvPreviewSheet {
            source_sheet_number: window["source_sheet_number"].as_u64().unwrap_or(1),
            alignment_id: window["alignment_id"].as_str().unwrap_or_default().into(),
            from_station_m: window["from_station_m"].as_f64().unwrap_or(0.0),
            to_station_m: window["to_station_m"].as_f64().unwrap_or(0.0),
            svg,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::EngineExit;
    use ds_command_kernel::style_resolution::{self, Binding, Key};
    use serde_json::json;
    use std::sync::Mutex;

    fn layout() -> printing::Layout {
        serde_json::from_str(include_str!(
            "../../../tests/fixtures/mv-global-canonical.json"
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
            "../../../tests/fixtures/governed-renderer-defaults.json"
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

    /// Answers like the reporter, or as told to misbehave.
    struct FakeEngine {
        mode: &'static str,
        calls: Mutex<u32>,
    }

    impl ReportEngine for FakeEngine {
        fn build_info(&self) -> Result<Value, HostFailure> {
            crate::tests::FakeEngine::release().build_info()
        }
        fn export_transformer_report(&self, _: &Path, _: &Path) -> Result<EngineExit, HostFailure> {
            unreachable!("an MV preview never exports a transformer")
        }
        fn render_grid_plan_profile(
            &self,
            request: &Path,
            result: &Path,
        ) -> Result<EngineExit, HostFailure> {
            *self.calls.lock().unwrap() += 1;
            let request: Value = serde_json::from_slice(&std::fs::read(request).unwrap()).unwrap();
            assert_eq!(request["preview_only"], true);
            assert_eq!(request["sample_pages"], 1);
            let out = PathBuf::from(request["out_dir"].as_str().unwrap());
            std::fs::create_dir_all(out.join("preview")).unwrap();
            let sheet = out.join("preview").join("sheet-001.svg");
            std::fs::write(&sheet, "<svg xmlns=\"http://www.w3.org/2000/svg\"/>").unwrap();
            let previews = match self.mode {
                "escape" => json!([request["scene_path"]]),
                _ => json!([sheet]),
            };
            let project = if self.mode == "foreign" {
                "another"
            } else {
                "template_project"
            };
            std::fs::write(
                result,
                serde_json::to_vec(&json!({
                    "project_id": project, "preview_only": true,
                    "mv_setup_sha256": request["mv_setup"]["sha256"],
                    "model_revision": "rev-model", "page_count": 1, "full_page_count": 42,
                    "page_station_windows": [{"source_sheet_number": 7, "alignment_id": "aln-1",
                        "from_station_m": 1200.0, "to_station_m": 1650.5}],
                    "previews": previews,
                }))
                .unwrap(),
            )
            .unwrap();
            Ok(EngineExit {
                succeeded: self.mode != "refuse",
                summary: "fake".into(),
            })
        }
    }

    fn run(
        mode: &'static str,
        layout: &printing::Layout,
        scene: &[u8],
    ) -> (Result<MvPreviewAnswer, MvPreviewRefusal>, u32) {
        let staging = tempfile::tempdir().unwrap();
        let engine = FakeEngine {
            mode,
            calls: Mutex::new(0),
        };
        let table = table(&self::layout());
        let sheets = sheets();
        let answer = execute(
            &engine,
            &MvPreviewInputs {
                project_id: "template_project",
                sheets: &sheets,
                layout,
                table: &table,
                project_crs: None,
                model_crs: None,
                scene,
                plan: b"{}",
                staging: staging.path(),
            },
        );
        let calls = *engine.calls.lock().unwrap();
        assert_eq!(
            std::fs::read_dir(staging.path()).unwrap().count(),
            0,
            "the run directory is removed"
        );
        (answer, calls)
    }

    #[test]
    fn a_returning_mv_consumer_reuses_the_native_result_and_changed_inputs_render_again() {
        let staging = tempfile::tempdir().unwrap();
        let hold = tempfile::tempdir().unwrap();
        let engine = FakeEngine {
            mode: "ok",
            calls: Mutex::new(0),
        };
        let layout = layout();
        let table = table(&layout);
        let sheets = sheets();
        let room = ds_project_data::preview_room::Room {
            root: hold.path().into(),
            scope: ds_command_kernel::project_dataset_cache::Scope {
                principal: "owner".into(),
                project: "template_project".into(),
            },
            lane: "canary".into(),
            credential_audience_sha256: "a".repeat(64),
        };
        let mut inputs = MvPreviewInputs {
            project_id: "template_project",
            sheets: &sheets,
            layout: &layout,
            table: &table,
            project_crs: None,
            model_crs: None,
            scene: b"{}",
            plan: b"{}",
            staging: staging.path(),
        };
        let first = execute_retained(&engine, &inputs, Some(&room)).unwrap();
        let second = execute_retained(&engine, &inputs, Some(&room)).unwrap();
        assert_eq!(first, second);
        assert_eq!(*engine.calls.lock().unwrap(), 1);
        inputs.scene = b"{\"model_revision\":\"changed\"}";
        execute_retained(&engine, &inputs, Some(&room)).unwrap();
        assert_eq!(*engine.calls.lock().unwrap(), 2);
        assert_eq!(std::fs::read_dir(staging.path()).unwrap().count(), 0);
    }

    #[test]
    fn preview_answers_the_sample_sheet_and_its_station_window() {
        let (answer, calls) = run("ok", &layout(), b"{}");
        let answer = answer.unwrap();
        assert_eq!(calls, 1);
        assert_eq!(answer.layout_id, layout().id);
        assert_eq!(answer.model_revision, "rev-model");
        assert_eq!(answer.full_page_count, 42);
        assert_eq!(answer.sheet.source_sheet_number, 7);
        assert_eq!(answer.sheet.alignment_id, "aln-1");
        assert_eq!(answer.sheet.to_station_m, 1650.5);
        assert!(answer.sheet.svg.starts_with("<svg"));
    }

    #[test]
    fn engine_and_result_failures_are_host_refusals() {
        let code = |mode| match run(mode, &layout(), b"{}").0 {
            Err(MvPreviewRefusal::Host(failure)) => failure.code,
            other => panic!("{mode}: {other:?}"),
        };
        assert_eq!(code("refuse"), CODE_ENGINE_REFUSED);
        assert_eq!(code("foreign"), CODE_RESULT_INVALID);
        assert_eq!(code("escape"), CODE_RESULT_INVALID);
    }

    #[test]
    fn kernel_and_input_refusals_never_reach_the_engine() {
        let mut plain = layout();
        plain.mv = None;
        let (answer, calls) = run("ok", &plain, b"{}");
        assert!(
            matches!(answer, Err(MvPreviewRefusal::Kernel(r)) if r.code == "mv_print_template_invalid")
        );
        assert_eq!(calls, 0);
        let (answer, calls) = run("ok", &layout(), b"");
        assert!(matches!(answer, Err(MvPreviewRefusal::Host(f)) if f.code == CODE_INPUTS_INVALID));
        assert_eq!(calls, 0);
    }
}
