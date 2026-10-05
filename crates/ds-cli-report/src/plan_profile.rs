//! `ds report plan-profile` — headless DS Grid sheet rendering through the
//! reporter's typed task. The engine owns every projection and drawing byte.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{DS_REPORT, EXPORT_TIMEOUT};

pub static COMMAND: Command = Command {
    id: "report.plan-profile",
    path: &["report", "plan-profile"],
    contract: 4,
    summary: "Render DS Grid plan/profile sheets from a pinned scene and plan.",
    purpose: "Resolve the named project canonical MV setup from its exact adopted printing-library revision, then render same-revision engine projections and approved front matter into one local PDF. Repeat --alignment for exact scene band IDs, or omit it for every alignment. Title, party logos, page order, scales, fonts and fixed publication version/date come only from that setup. Allowed model identity/title differences are explicit. Missing configuration or held approved assets refuses before output. Use report layout copy and report project mv-setup set to adopt and select; no print data is mutated by this command. --request alone renders a complete engine print request, such as a fixture booklet, with no project read.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "request",
            "<json-file>",
            "Complete engine print request; replaces every project, geometry and setup flag.",
        ),
        Arg::switch(
            "preview-only",
            "Produce the complete ordered PNG preview set and layout plan before assembling a PDF.",
        ),
        Arg::value(
            "project",
            "<ds-project>",
            "Project named for this request; required without --request.",
        ),
        crate::project::LANE_ARG,
        Arg::value(
            "scene",
            "<path>",
            "Absolute same-revision project_profile_atlas scene JSON.",
        ),
        Arg::repeated(
            "alignment",
            "<id>",
            "Print only these exact scene alignment band IDs (repeatable, 1..256). Omit for all bands; native selection preserves stationing and cross-alignment spans.",
        ),
        Arg::value(
            "plan",
            "<path>",
            "Absolute same-revision project_plan rows JSON.",
        ),
        Arg::value(
            "out-dir",
            "<path>",
            "Fresh absolute directory for the complete MV publication.",
        ),
        Arg::value(
            "model-identity",
            "<text>",
            "Only when the adopted template explicitly permits this model identity difference.",
        ),
        Arg::value(
            "model-title",
            "<text>",
            "Only when the adopted template explicitly permits this model drawing title difference.",
        ),
        Arg::value(
            "publication-assets",
            "<json-file>",
            "Map approved preserved_pdf asset ids to absolute held PDF paths; digests come from the adopted template.",
        ),
        Arg::value(
            "side-profiles",
            "<path>",
            "Optional same-model measured terrain traces.",
        ),
        Arg::value(
            "notes",
            "<path>",
            "Optional explicit scoped drawing annotations.",
        ),
        Arg::value(
            "structure-descriptions",
            "<json-file>",
            "Exact staking XLSX Description binding; discover its schema with report plan-profile-config schema. Project, model revision, workbook SHA, number, native name and XY must match.",
        ),
        Arg::value(
            "context-pages",
            "<json-file>",
            "Alignment-keyed geographic context pinned to its source/recipe digest; legacy ordered model-pinned captures remain supported.",
        ),
        Arg::value(
            "model-crs",
            "<declared-crs>",
            "Declared model CRS for registered map context.",
        ),
        Arg::value(
            "sample-pages",
            "<count>",
            "1..20 representative drawing sheets; front matter still follows the approved order.",
        ),
        Arg::value("result", "<path>", "Fresh reporter receipt path."),
    ],
    output: "Exact model revision, layout plan digest, ordered PNG paths and raster hashes including front matter; PDF path/digest unless preview-only; source and approved component digests.",
    examples: &[Example {
        command: "ds report plan-profile --project gisagara --scene /project/profile.json --plan /project/plan.json --alignment al-main --out-dir /project/gisagara-sheets --output json",
        note: "Render the adopted publication for one alignment from held engine projections.",
        runnable: false,
    }],
    refusals: &crate::project::joined::<{ crate::project::NATIVE_READ_REFUSALS.len() + 14 }>(&[
        crate::project::NATIVE_READ_REFUSALS,
        &[
            REQUEST_MODE_REFUSAL,
            PRINT_REQUEST_REFUSAL,
            crate::project::mv_setup::REFUSAL,
            crate::project::mv_setup::STYLE_REFUSAL,
            crate::project::mv_setup::PROJECT_CRS_CONTEXT_REFUSAL,
            Refusal {
                code: "alignment_selection_invalid",
                when: "selection names an unknown band, is unbounded or malformed, or its scene cannot be decoded",
                remedy: "Read alignment_id values in the held scene and pass exact IDs; acquire a valid project_profile_atlas projection",
            },
            Refusal {
                code: "projection_missing",
                when: "scene or plan is absent",
                remedy: "Acquire paired projections from the same model revision",
            },
            Refusal {
                code: "output_exists",
                when: "destination already exists",
                remedy: "Choose fresh output and receipt paths",
            },
            Refusal {
                code: "request_encode_failed",
                when: "typed request cannot be encoded",
                remedy: "Report the input and build identity",
            },
            Refusal {
                code: "request_write_failed",
                when: "typed request cannot be written",
                remedy: "Check temporary storage permissions and free space",
            },
            Refusal {
                code: "reporter_engine_missing",
                when: "reporter is absent",
                remedy: "Install the matching ds-report",
            },
            Refusal {
                code: "engine_refused",
                when: "reporter refuses setup, held assets or geometry pairing",
                remedy: "Read the reporter keyed detail and correct the named input",
            },
            Refusal {
                code: "context_manifest_invalid",
                when: "an input manifest is missing, oversized or malformed",
                remedy: "Supply a bounded JSON context array or publication asset map",
            },
            Refusal {
                code: "invalid_scale",
                when: "sample count is not 1..20",
                remedy: "Supply 1..20 or omit sample-pages for a full publication",
            },
        ],
    ]),
    reference: Some("docs/reference/report.md"),
    search: &[
        "dsgrid",
        "print",
        "alignment",
        "staking description",
        "workbook column",
    ],
    requires: Requires::Server,
    availability,
};

fn availability() -> Availability {
    DS_REPORT.availability()
}

/// One mode per call. A request file is the engine's whole request, so a
/// project, geometry or setup flag beside it would be silently ignored.
const REQUEST_MODE_REFUSAL: Refusal = Refusal {
    code: "request_mode_invalid",
    when: "--request is mixed with project, geometry or setup flags, or a project render lacks --project, --scene, --plan or --out-dir",
    remedy: "Pass --request alone, or --project, --scene, --plan and --out-dir together",
};
const PRINT_REQUEST_REFUSAL: Refusal = Refusal {
    code: "print_request_invalid",
    when: "the --request file is unreadable, over 32 MiB, not a JSON object or has no absolute out_dir",
    remedy: "Pass a complete engine print request; discover it with report tasks",
};
const REQUEST_LIMIT: u64 = 32 * 1024 * 1024;
/// Inputs that only exist so ds can build the request itself.
const PROJECT_MODE_INPUTS: &[&str] = &[
    "project",
    "scene",
    "plan",
    "out-dir",
    "model-identity",
    "model-title",
    "publication-assets",
    "side-profiles",
    "notes",
    "structure-descriptions",
    "context-pages",
    "model-crs",
    "sample-pages",
];

fn mode_invalid(message: String) -> Failure {
    Failure::invalid(REQUEST_MODE_REFUSAL.code, message).remedy(REQUEST_MODE_REFUSAL.remedy)
}

fn print_request_invalid(message: String) -> Failure {
    Failure::invalid(PRINT_REQUEST_REFUSAL.code, message).remedy(PRINT_REQUEST_REFUSAL.remedy)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    match inputs.value("request") {
        Some(request) => run_request(inputs, PathBuf::from(request)),
        None => run_project(inputs),
    }
}

/// Render a held engine request exactly as given, through the same engine
/// call a project render makes. Nothing is read from any project.
fn run_request(inputs: &Inputs, request: PathBuf) -> Result<Value, Failure> {
    let mut mixed: Vec<&str> = PROJECT_MODE_INPUTS
        .iter()
        .copied()
        .filter(|name| inputs.value(name).is_some())
        .collect();
    if !inputs.repeated("alignment").is_empty() {
        mixed.push("alignment");
    }
    if inputs.switch("preview-only") {
        mixed.push("preview-only");
    }
    if !mixed.is_empty() {
        return Err(mode_invalid(format!(
            "--request is the whole print request; remove --{}",
            mixed.join(", --")
        )));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&request)
        .and_then(|file| {
            use std::io::Read;
            file.take(REQUEST_LIMIT + 1).read_to_end(&mut bytes)
        })
        .map_err(|e| print_request_invalid(format!("{}: {e}", request.display())))?;
    if bytes.len() as u64 > REQUEST_LIMIT {
        return Err(print_request_invalid("request exceeds 32 MiB".into()));
    }
    let document: Value = serde_json::from_slice(&bytes)
        .map_err(|e| print_request_invalid(format!("request is not JSON: {e}")))?;
    let out_dir = document["out_dir"]
        .as_str()
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .ok_or_else(|| print_request_invalid("request out_dir must be an absolute path".into()))?;
    if out_dir.symlink_metadata().is_ok() {
        return Err(Failure::invalid(
            "output_exists",
            format!("output directory exists: {}", out_dir.display()),
        ));
    }
    let staged = Staged::new(inputs.value("result"))?;
    let mut document = staged.render(bytes)?;
    if staged.keep {
        document["result_path"] = json!(staged.result_path.display().to_string());
    }
    Ok(document)
}

fn project_input<'a>(inputs: &'a Inputs, name: &str) -> Result<&'a str, Failure> {
    inputs
        .value(name)
        .ok_or_else(|| mode_invalid(format!("--{name} is required without --request")))
}

fn run_project(inputs: &Inputs) -> Result<Value, Failure> {
    let project = project_input(inputs, "project")?;
    let scene = PathBuf::from(project_input(inputs, "scene")?);
    let plan = PathBuf::from(project_input(inputs, "plan")?);
    let out_dir = PathBuf::from(project_input(inputs, "out-dir")?);
    for (name, path) in [("scene", &scene), ("plan", &plan)] {
        if !path.is_file() {
            return Err(Failure::invalid(
                "projection_missing",
                format!("{name} file does not exist: {}", path.display()),
            ));
        }
    }
    if out_dir.symlink_metadata().is_ok() {
        return Err(Failure::invalid(
            "output_exists",
            format!("output directory exists: {}", out_dir.display()),
        ));
    }
    let staged = Staged::new(inputs.value("result"))?;
    let mut fields = BTreeMap::new();
    for (arg, field) in [
        (
            "model-identity",
            ds_command_kernel::printing::mv::ModelField::Identity,
        ),
        (
            "model-title",
            ds_command_kernel::printing::mv::ModelField::Title,
        ),
    ] {
        if let Some(value) = inputs.value(arg) {
            fields.insert(field, value.to_owned());
        }
    }
    // Validate the held geometry scope before any authenticated setup read.
    let selected = stage_selected_scene(&scene, inputs.repeated("alignment"))?;
    let (resolved, style_resolution, renderer_defaults, report_config, project_crs) =
        crate::project::mv_setup::resolve_project_print(inputs.require("lane")?, project, fields)?;
    let read_manifest = |name: &str, empty: Value| -> Result<Value, Failure> {
        let Some(path) = inputs.value(name) else {
            return Ok(empty);
        };
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(1024 * 1024 + 1).read_to_end(&mut bytes))
            .map_err(|e| Failure::invalid("context_manifest_invalid", e.to_string()))?;
        if bytes.len() > 1024 * 1024 {
            return Err(Failure::invalid(
                "context_manifest_invalid",
                "manifest exceeds 1 MiB",
            ));
        }
        serde_json::from_slice(&bytes)
            .map_err(|e| Failure::invalid("context_manifest_invalid", e.to_string()))
    };
    let context_page_files = read_manifest("context-pages", json!([]))?;
    let publication_assets = read_manifest("publication-assets", json!({}))?;
    let sample_pages = inputs
        .value("sample-pages")
        .map(|value| {
            value
                .parse::<u32>()
                .ok()
                .filter(|n| (1..=20).contains(n))
                .ok_or_else(|| Failure::invalid("invalid_scale", "sample-pages must be 1..20"))
        })
        .transpose()?;
    // Keep the original file untouched and hold the private scoped projection
    // until the typed reporter call finishes. The native engine owns selection.
    let scene_path = selected
        .as_ref()
        .map(|selection| selection.file.path())
        .unwrap_or(&scene);
    let request = json!({"project_crs":project_crs,"report_config":report_config,"renderer_defaults":renderer_defaults,"style_resolution":style_resolution,"preview_only":inputs.switch("preview-only"),"project_id":project,"scene_path":scene_path,"plan_path":plan,"side_profiles_path":inputs.value("side-profiles"),"notes_path":inputs.value("notes"),"structure_descriptions_path":inputs.value("structure-descriptions"),"out_dir":out_dir,"sample_pages":sample_pages,"context_page_files":context_page_files,"model_crs":inputs.value("model-crs"),"settings":resolved.settings,"mv_setup":resolved,"publication_assets":publication_assets});
    let bytes = serde_json::to_vec(&request)
        .map_err(|e| Failure::internal("request_encode_failed", e.to_string()))?;
    let mut document = staged.render(bytes)?;
    if let Some(selected) = &selected {
        document["alignment_selection"] = selected.receipt.clone();
        if staged.keep {
            let bytes = serde_json::to_vec(&document)
                .map_err(|e| Failure::internal("request_encode_failed", e.to_string()))?;
            ds_layer_store::private::write(&staged.result_path, bytes)
                .map_err(|e| Failure::failed("request_write_failed", e.to_string()))?;
        }
    }
    if staged.keep {
        document["result_path"] = json!(staged.result_path.display().to_string());
    }
    Ok(document)
}

/// The private request copy and the receipt path of one engine call.
struct Staged {
    request_path: PathBuf,
    result_path: PathBuf,
    keep: bool,
}

impl Staged {
    /// Fixed before any setup read, so an existing receipt refuses first.
    fn new(result: Option<&str>) -> Result<Self, Failure> {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|v| v.as_nanos())
            .unwrap_or_default();
        let scratch = |kind: &str| {
            std::env::temp_dir().join(format!(
                "ds-grid-print-{kind}-{}-{nonce}.json",
                std::process::id()
            ))
        };
        let (result_path, keep) = match result {
            Some(path) => (PathBuf::from(path), true),
            None => (scratch("result"), false),
        };
        if result_path.symlink_metadata().is_ok() {
            return Err(Failure::invalid(
                "output_exists",
                format!("result file exists: {}", result_path.display()),
            ));
        }
        Ok(Self {
            request_path: scratch("request"),
            result_path,
            keep,
        })
    }

    /// One `render-grid-plan-profile` call over these exact request bytes.
    fn render(&self, bytes: Vec<u8>) -> Result<Value, Failure> {
        ds_layer_store::private::write(&self.request_path, bytes)
            .map_err(|e| Failure::failed("request_write_failed", e.to_string()))?;
        let args = vec![
            OsString::from("--request"),
            self.request_path.clone().into(),
            OsString::from("--result"),
            self.result_path.clone().into(),
        ];
        let completed = DS_REPORT.call("render-grid-plan-profile", &args, EXPORT_TIMEOUT);
        let _ = std::fs::remove_file(&self.request_path);
        let completed = completed?;
        let document = std::fs::read(&self.result_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        if !self.keep {
            let _ = std::fs::remove_file(&self.result_path);
        }
        if !completed.succeeded() {
            return Err(DS_REPORT.failure_from(&completed, "render-grid-plan-profile"));
        }
        document
            .ok_or_else(|| Failure::failed("engine_refused", "reporter returned no print receipt"))
    }
}

struct SelectedScene {
    file: tempfile::NamedTempFile,
    receipt: Value,
}

fn stage_selected_scene(
    scene_path: &std::path::Path,
    ids: &[String],
) -> Result<Option<SelectedScene>, Failure> {
    if ids.is_empty() {
        return Ok(None);
    }
    if !scene_path.is_absolute() {
        return Err(Failure::invalid(
            "alignment_selection_invalid",
            "scene path must be absolute",
        ));
    }
    let bytes = std::fs::read(scene_path)
        .map_err(|e| Failure::invalid("alignment_selection_invalid", e.to_string()))?;
    let scene: ds_grid_engine::ProfileAtlasScene = serde_json::from_slice(&bytes)
        .map_err(|e| Failure::invalid("alignment_selection_invalid", e.to_string()))?;
    let selected = ds_grid_engine::publication_scope::select_publication_alignments(&scene, ids)
        .map_err(|e| Failure::invalid("alignment_selection_invalid", e.to_string()))?;
    let receipt = json!({"source_scene_path":scene_path,"source_scene_sha256":format!("sha256:{:x}",Sha256::digest(&bytes)),"alignment_ids":selected.bands.iter().map(|band| band.alignment_id.as_str()).collect::<Vec<_>>()});
    let mut file = tempfile::Builder::new()
        .prefix("ds-grid-print-scene-")
        .suffix(".json")
        .tempfile()
        .map_err(|e| Failure::failed("request_write_failed", e.to_string()))?;
    serde_json::to_writer(file.as_file_mut(), &selected)
        .map_err(|e| Failure::failed("request_write_failed", e.to_string()))?;
    Ok(Some(SelectedScene { file, receipt }))
}

pub fn render(data: &Value) -> String {
    let artifact = data["pdf"]
        .as_str()
        .map(|path| format!("PDF {path}"))
        .unwrap_or_else(|| {
            format!(
                "PNG previews: {} pages",
                data["booklet_previews"].as_array().map_or(0, Vec::len)
            )
        });
    format!(
        "{} A3 booklet pages · {} alignments\n{}\nrevision {}",
        data.get("publication_page_count")
            .unwrap_or(&data["page_count"]),
        data["alignments"],
        artifact,
        data["model_revision"].as_str().unwrap_or("?")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refusal(args: &[&str]) -> String {
        let tokens: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        let inputs = ds_cli_contract::args::parse(&COMMAND, &tokens).unwrap();
        let outcome = match inputs.value("request") {
            Some(request) => run_request(&inputs, PathBuf::from(request)),
            None => run_project(&inputs),
        };
        outcome
            .expect_err("refused before the engine")
            .code()
            .to_owned()
    }

    #[test]
    fn request_mode_is_exclusive_and_refuses_before_the_engine() {
        let dir = tempfile::tempdir().unwrap();
        let request = dir.path().join("request.json");
        let path = request.to_str().unwrap();
        let held = dir.path().join("held");
        std::fs::create_dir(&held).unwrap();
        for mixed in [
            &["--request", path, "--project", "fixture"][..],
            &["--request", path, "--alignment", "al-1"],
            &["--request", path, "--preview-only"],
            &["--scene", path, "--plan", path, "--out-dir", "/x"],
        ] {
            assert_eq!(refusal(mixed), "request_mode_invalid", "{mixed:?}");
        }
        assert_eq!(refusal(&["--request", path]), "print_request_invalid");
        for (bytes, code) in [
            (json!("not an object").to_string(), "print_request_invalid"),
            (
                json!({"out_dir": "relative/out"}).to_string(),
                "print_request_invalid",
            ),
            (json!({"out_dir": held}).to_string(), "output_exists"),
        ] {
            std::fs::write(&request, bytes).unwrap();
            assert_eq!(refusal(&["--request", path]), code);
        }
        std::fs::write(
            &request,
            json!({"out_dir": dir.path().join("fresh")}).to_string(),
        )
        .unwrap();
        assert_eq!(
            refusal(&["--request", path, "--result", held.to_str().unwrap()]),
            "output_exists"
        );
        assert!(!dir.path().join("fresh").exists());
    }

    #[test]
    fn alignment_flag_stages_a_native_fixture_selection_and_cleans_up() {
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../../ds-network/fixtures/pls-public/humble-pole/humble-pole.dsgrid");
        let (package, _) =
            ds_grid_exchange::dsgrid::ingest(&std::fs::read(fixture).unwrap()).unwrap();
        let scene = ds_grid_engine::GridSession::open(package.snapshot)
            .profile_atlas_scene(ds_grid_engine::ProfileAtlasOptions::default())
            .unwrap();
        let id = scene.bands[0].alignment_id.as_str().to_owned();
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("scene.json");
        let bytes = serde_json::to_vec(&scene).unwrap();
        std::fs::write(&source, &bytes).unwrap();
        let inputs = ds_cli_contract::args::parse(
            &COMMAND,
            &[
                "--project",
                "fixture",
                "--scene",
                source.to_str().unwrap(),
                "--plan",
                source.to_str().unwrap(),
                "--out-dir",
                dir.path().join("output").to_str().unwrap(),
                "--alignment",
                &id,
                "--alignment",
                &id,
            ]
            .map(str::to_owned),
        )
        .unwrap();
        let selected = stage_selected_scene(&source, inputs.repeated("alignment"))
            .unwrap()
            .unwrap();
        let staged = selected.file.path().to_owned();
        let decoded: ds_grid_engine::ProfileAtlasScene =
            serde_json::from_slice(&std::fs::read(&staged).unwrap()).unwrap();
        assert_eq!(decoded.bands, vec![scene.bands[0].clone()]);
        assert_eq!(decoded.model_revision, scene.model_revision);
        assert_eq!(selected.receipt["alignment_ids"], json!([id]));
        assert_eq!(
            selected.receipt["source_scene_sha256"],
            format!("sha256:{:x}", Sha256::digest(&bytes))
        );
        assert_eq!(std::fs::read(&source).unwrap(), bytes);
        drop(selected);
        assert!(!staged.exists());
        assert!(stage_selected_scene(&source, &[]).unwrap().is_none());
        assert_eq!(
            stage_selected_scene(&source, &["unknown".into()])
                .err()
                .unwrap()
                .code(),
            "alignment_selection_invalid"
        );
    }
}
