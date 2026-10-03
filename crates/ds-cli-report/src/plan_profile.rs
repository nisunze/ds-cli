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
    contract: 3,
    summary: "Render DS Grid plan/profile sheets from a pinned scene and plan.",
    purpose: "Resolve the named project canonical MV setup from its exact adopted printing-library revision, then render same-revision engine projections and approved front matter into one local PDF. Repeat --alignment for exact scene band IDs, or omit it for every alignment. Title, party logos, page order, scales, fonts and fixed publication version/date come only from that setup. Allowed model identity/title differences are explicit. Missing configuration or held approved assets refuses before output. Use report layout copy and report project mv-setup set to adopt and select; no print data is mutated by this command.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::switch(
            "preview-only",
            "Produce the complete ordered PNG preview set and layout plan before assembling a PDF.",
        ),
        crate::project::PROJECT_ARG,
        crate::project::LANE_ARG,
        Arg::value(
            "scene",
            "<path>",
            "Absolute same-revision project_profile_atlas scene JSON.",
        )
        .required(),
        Arg::repeated(
            "alignment",
            "<id>",
            "Print only these exact scene alignment band IDs (repeatable, 1..256). Omit for all bands; native selection preserves stationing and cross-alignment spans.",
        ),
        Arg::value(
            "plan",
            "<path>",
            "Absolute same-revision project_plan rows JSON.",
        )
        .required(),
        Arg::value(
            "out-dir",
            "<path>",
            "Fresh absolute directory for the complete MV publication.",
        )
        .required(),
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
    refusals: &crate::project::joined::<{ crate::project::NATIVE_READ_REFUSALS.len() + 11 }>(&[
        crate::project::NATIVE_READ_REFUSALS,
        &[
            crate::project::mv_setup::REFUSAL,
            crate::project::mv_setup::STYLE_REFUSAL,
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

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = inputs.require("project")?;
    let scene = PathBuf::from(inputs.require("scene")?);
    let plan = PathBuf::from(inputs.require("plan")?);
    let out_dir = PathBuf::from(inputs.require("out-dir")?);
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
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_nanos())
        .unwrap_or_default();
    let request_path = std::env::temp_dir().join(format!(
        "ds-grid-print-request-{}-{nonce}.json",
        std::process::id()
    ));
    let (result_path, keep) = match inputs.value("result") {
        Some(path) => (PathBuf::from(path), true),
        None => (
            std::env::temp_dir().join(format!(
                "ds-grid-print-result-{}-{nonce}.json",
                std::process::id()
            )),
            false,
        ),
    };
    if result_path.symlink_metadata().is_ok() {
        return Err(Failure::invalid(
            "output_exists",
            format!("result file exists: {}", result_path.display()),
        ));
    }
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
    let (resolved, style_resolution, renderer_defaults) =
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
    let request = json!({"renderer_defaults":renderer_defaults,"style_resolution":style_resolution,"preview_only":inputs.switch("preview-only"),"project_id":project,"scene_path":scene_path,"plan_path":plan,"side_profiles_path":inputs.value("side-profiles"),"notes_path":inputs.value("notes"),"structure_descriptions_path":inputs.value("structure-descriptions"),"out_dir":out_dir,"sample_pages":sample_pages,"context_page_files":context_page_files,"model_crs":inputs.value("model-crs"),"settings":resolved.settings,"mv_setup":resolved,"publication_assets":publication_assets});
    let bytes = serde_json::to_vec(&request)
        .map_err(|e| Failure::internal("request_encode_failed", e.to_string()))?;
    ds_layer_store::private::write(&request_path, bytes)
        .map_err(|e| Failure::failed("request_write_failed", e.to_string()))?;
    let args = vec![
        OsString::from("--request"),
        request_path.clone().into(),
        OsString::from("--result"),
        result_path.clone().into(),
    ];
    let completed = DS_REPORT.call("render-grid-plan-profile", &args, EXPORT_TIMEOUT);
    let _ = std::fs::remove_file(&request_path);
    let completed = completed?;
    let document = std::fs::read(&result_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
    if !keep {
        let _ = std::fs::remove_file(&result_path);
    }
    if !completed.succeeded() {
        return Err(DS_REPORT.failure_from(&completed, "render-grid-plan-profile"));
    }
    let Some(mut document) = document else {
        return Err(Failure::failed(
            "engine_refused",
            "reporter returned no print receipt",
        ));
    };
    if let Some(selected) = &selected {
        document["alignment_selection"] = selected.receipt.clone();
        if keep {
            let bytes = serde_json::to_vec(&document)
                .map_err(|e| Failure::internal("request_encode_failed", e.to_string()))?;
            ds_layer_store::private::write(&result_path, bytes)
                .map_err(|e| Failure::failed("request_write_failed", e.to_string()))?;
        }
    }
    if keep {
        document["result_path"] = json!(result_path.display().to_string());
    }
    Ok(document)
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
