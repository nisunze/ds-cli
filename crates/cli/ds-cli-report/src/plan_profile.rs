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

use crate::{DS_REPORT, EXPORT_TIMEOUT, capture_engine};
mod assets;
mod context;
mod model_mode;
use model_mode::RenderTask;

pub static COMMAND: Command = Command {
    id: "report.plan-profile",
    path: &["report", "plan-profile"],
    contract: 9,
    summary: "Render DS Grid plan/profile sheets from a pinned model or scene.",
    purpose: "Resolve the project's adopted MV setup and render a digest/revision-pinned model or same-revision projections into its ordered local publication. Exact alignment IDs narrow projection scope; model mode prints the complete model and refuses alignment/template-job preview flags. Text, logos, scales, fonts and issue policy inherit that setup. Missing inputs refuse before output. --request alone runs a native request without project reads. --preview-layout renders a saved or draft template sample into the scoped native disk cache with exact fences and 24h or 7d retention; nothing is saved. Responses carry file receipts and station metadata.",
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
        Arg::value(
            "preview-layout",
            "<json-file>",
            "Saved or draft MV layout sample from --scene/--plan; requires --project, returns a disk receipt.",
        ),
        Arg::value(
            "preview-retention",
            "<24h|7d>",
            "SVG cache lifetime; requires --preview-layout, default 24h.",
        )
        .choices(&["24h", "7d"]),
        Arg::switch(
            "preview-all",
            "Start all drawing sheets as one native background disk job; requires --preview-layout.",
        ),
        Arg::value(
            "preview-job",
            "<input-sha256>",
            "Read one exact native disk job; requires --project and excludes geometry/layout flags.",
        ),
        Arg::value(
            "preview-page",
            "<number>",
            "Consume one ready page of --preview-job; omitted means metadata only.",
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
            "model",
            "<absolute.dsgrid>",
            "Ordinary absolute model package, at most 512 MiB; replaces --scene and --plan and requires --expected-model-sha256.",
        ),
        Arg::value(
            "expected-model-sha256",
            "<sha256:hex>",
            "Exact sha256: plus 64 lowercase hexadecimal digits of --model, checked before project reads and rechecked by Reporter.",
        ),
        Arg::value(
            "expected-authored-revision",
            "<revision>",
            "Optional exact authored revision fence; omitted, Network derives it from the verified --model bytes.",
        ),
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
            "Explicit alignment-keyed context captures; omitted with --model, held project context is read by default.",
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
    output: "Model mode returns source model SHA-256/authored revision, Network projection identity/evidence, mv_context (held layers and omissions per alignment and key plan) and nested preview/print receipts. Projection mode returns exact revision, ordered file paths/digests and station windows. Samples return ds.mv-preview-artifacts/v1. --preview-all/--preview-job return ds.mv-preview-job/v1 progress and ordered ready pages; --preview-page includes its native SVG receipt. Publication includes its layout plan and optional PDF.",
    examples: &[Example {
        command: "ds report plan-profile --project gisagara --scene /project/profile.json --plan /project/plan.json --alignment al-main --out-dir /project/gisagara-sheets --output json",
        note: "Render the adopted publication for one alignment from held engine projections.",
        runnable: false,
    }],
    refusals: &crate::project::joined::<{ crate::project::NATIVE_READ_REFUSALS.len() + 31 }>(&[
        crate::project::NATIVE_READ_REFUSALS,
        &[
            Refusal {
                code: "mv_model_source_stale",
                when: "the head or copy moved",
                remedy: "select an exact copy from project choices",
            },
            Refusal {
                code: "mv_model_catalogue_invalid",
                when: "project heads are invalid",
                remedy: "refresh project choices",
            },
            Refusal {
                code: "project_model_catalogue_acquisition_required",
                when: "project heads are missing",
                remedy: "read project choices online",
            },
            REQUEST_MODE_REFUSAL,
            model_mode::SOURCE_INVALID,
            model_mode::SOURCE_CHANGED,
            crate::project::mv_context::NETWORK_ENGINE_MISSING,
            crate::project::mv_context::NETWORK_ENGINE_INCOMPATIBLE,
            context::CONTEXT_UNAVAILABLE,
            PREVIEW_LAYOUT_REFUSAL,
            crate::project::export::INPUTS_INVALID,
            crate::project::export::STAGING_FAILED,
            crate::project::export::RESULT_INVALID,
            Refusal {
                code: "print_preview_artifact_unavailable",
                when: "the exact native preview file or manifest is unavailable",
                remedy: "request the same preview again; repair its scoped native cache if it persists",
            },
            Refusal {
                code: "print_preview_artifact_expired",
                when: "the admitted preview receipt has expired",
                remedy: "request its exact preview again",
            },
            crate::project::export::PREVIEW_JOB_CAPACITY,
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
            crate::ENGINE_MISSING,
            crate::ENGINE_RECEIPT_INVALID,
            Refusal {
                code: "engine_refused",
                when: "reporter refuses setup, held assets or geometry pairing",
                remedy: "Read the reporter keyed detail and correct the named input",
            },
            Refusal {
                code: "mv_print_asset_missing",
                when: "--publication-assets omits an approved preserved_pdf asset the adopted setup names, or supplies one it does not",
                remedy: "Map exactly the setup's asset ids to absolute held PDF paths acquired through DS assets",
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
    // Retained status/page reads need no renderer. Rendering branches name a
    // missing matching Reporter at their existing fixed process boundary.
    Availability::Available
}

/// One mode per call. A request file is the engine's whole request, so a
/// project, geometry or setup flag beside it would be silently ignored.
const REQUEST_MODE_REFUSAL: Refusal = Refusal {
    code: "request_mode_invalid",
    when: "--request is mixed with project, geometry or setup flags, or a project render mixes/incompletely pins model and projection input modes",
    remedy: "Pass --request alone; otherwise use --project and --out-dir with either --scene plus --plan, or --model plus its expected SHA-256",
};
const PREVIEW_LAYOUT_REFUSAL: Refusal = Refusal {
    code: "preview_layout_invalid",
    when: "the --preview-layout file is unreadable, oversized or not a printing layout document",
    remedy: "Pass a ds.print-layout/v2 MV document from report layout get, or the draft being edited",
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
    "model",
    "expected-model-sha256",
    "expected-authored-revision",
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
    "preview-retention",
    "preview-job",
    "preview-page",
];

fn mode_invalid(message: String) -> Failure {
    Failure::invalid(REQUEST_MODE_REFUSAL.code, message).remedy(REQUEST_MODE_REFUSAL.remedy)
}

fn print_request_invalid(message: String) -> Failure {
    Failure::invalid(PRINT_REQUEST_REFUSAL.code, message).remedy(PRINT_REQUEST_REFUSAL.remedy)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let model = model_mode::admit(inputs, &model_mode::derive_authored_revision)?;
    let page = inputs
        .value("preview-page")
        .map(|page| {
            page.parse::<usize>()
                .map_err(|_| mode_invalid("preview page must be an integer".into()))
        })
        .transpose()?;
    let action = ds_command_kernel::printing::mv_preview::action(
        inputs.value("preview-layout").is_some(),
        inputs.switch("preview-all"),
        inputs.value("preview-job"),
        page,
        inputs.value("preview-retention").is_some(),
    )
    .map_err(|error| mode_invalid(error.to_string()).remedy(error.remedy))?;
    if let ds_command_kernel::printing::mv_preview::Action::Read { input_sha256, page } = action {
        return run_job_read(inputs, &input_sha256, page);
    }
    if let Some(layout) = inputs.value("preview-layout") {
        return run_template_preview(inputs, layout);
    }
    if inputs.value("preview-retention").is_some() {
        return Err(mode_invalid(
            "--preview-retention requires --preview-layout".into(),
        ));
    }
    match inputs.value("request") {
        Some(request) => run_request(inputs, PathBuf::from(request)),
        None => run_project(inputs, model),
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
    if inputs.switch("preview-all") {
        mixed.push("preview-all");
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
    let engine = capture_engine(inputs)?;
    let mut document = staged.render(&engine, bytes, RenderTask::Projection)?;
    if staged.keep {
        document["result_path"] = json!(staged.result_path.display().to_string());
    }
    Ok(document)
}

/// Preview one saved or draft MV layout on a single sample sheet of held
/// projections, under the project's issue identity and governed styles. The
/// shared report host stages and renders; only its scoped disk artifact is kept.
fn run_template_preview(inputs: &Inputs, layout_path: &str) -> Result<Value, Failure> {
    use ds_report_host::mv_preview::{MvPreviewInputs, MvPreviewRefusal};
    use ds_report_host::{mv_artifact, preview_artifact::Retention};
    let mut mixed: Vec<&str> = [
        "request",
        "out-dir",
        "model-identity",
        "model-title",
        "publication-assets",
        "side-profiles",
        "notes",
        "structure-descriptions",
        "context-pages",
        "sample-pages",
        "result",
    ]
    .into_iter()
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
            "--preview-layout previews its native template; remove --{}",
            mixed.join(", --")
        )));
    }
    let required = |name: &str| {
        inputs.value(name).ok_or_else(|| {
            mode_invalid(format!(
                "--preview-layout requires --project, --scene and --plan; --{name} is missing"
            ))
        })
    };
    let project = required("project")?;
    let read = |path: &str, limit: usize| -> Option<Vec<u8>> {
        use std::io::Read;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .and_then(|file| file.take(limit as u64 + 1).read_to_end(&mut bytes))
            .ok()
            .filter(|_| !bytes.is_empty() && bytes.len() <= limit)
            .map(|_| bytes)
    };
    let mut projections = Vec::new();
    for name in ["scene", "plan"] {
        let path = required(name)?;
        projections.push(
            read(path, ds_report_host::mv_preview::MAX_PROJECTION_BYTES).ok_or_else(|| {
                Failure::invalid(
                    "projection_missing",
                    format!("{name} file is missing, empty or oversized: {path}"),
                )
            })?,
        );
    }
    let layout: ds_command_kernel::printing::Layout =
        read(layout_path, ds_command_kernel::printing::MAX_LAYOUT_BYTES)
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .ok_or_else(|| {
                Failure::invalid(
                    PREVIEW_LAYOUT_REFUSAL.code,
                    format!("{layout_path} is not a printing layout"),
                )
                .remedy(PREVIEW_LAYOUT_REFUSAL.remedy)
            })?;
    let lane = inputs.require("lane")?;
    let (sheets, project_crs, identity) =
        crate::project::mv_setup::project_print_context_with_identity(lane, project)?;
    let table = crate::project::mv_setup::style_table(lane, project)?;
    let current = || {
        if ds_cli_auth::headless_identity_for_named_project(lane)? != identity {
            return Err(Failure::invalid(
                "print_project_crs_context_mismatch",
                "the native preview account or deployment audience changed",
            ));
        }
        Ok(())
    };
    current()?;
    let result_room = ds_project_data::preview_room::Room {
        root: ds_layer_store::default_root().map_err(|message| {
            crate::project::export::host_failure(ds_report_host::HostFailure::new(
                crate::project::export::STAGING_FAILED.code,
                message,
            ))
        })?,
        scope: ds_command_kernel::project_dataset_cache::Scope {
            principal: identity.uid().into(),
            project: project.into(),
        },
        lane: identity.lane().into(),
        credential_audience_sha256: identity.credential_audience_sha256().into(),
    };
    let staging = tempfile::tempdir().map_err(|error| {
        Failure::failed(
            crate::project::export::STAGING_FAILED.code,
            error.to_string(),
        )
        .remedy(crate::project::export::STAGING_FAILED.remedy)
    })?;
    let retention = match inputs.value("preview-retention") {
        None | Some("24h") => Retention::Hours24,
        Some("7d") => Retention::Days7,
        Some(_) => return Err(mode_invalid("preview retention must be 24h or 7d".into())),
    };
    let held = MvPreviewInputs {
        project_id: project,
        sheets: &sheets,
        layout: &layout,
        table: &table,
        project_crs: project_crs.as_ref(),
        model_crs: inputs.value("model-crs"),
        scene: &projections[0],
        plan: &projections[1],
        staging: staging.path(),
    };
    let answer = if inputs.switch("preview-all") {
        ds_report_host::mv_job::start(
            &crate::project::export::CliEngine::capture(identity.lane())
                .map_err(crate::project::export::host_failure)?,
            &held,
            &result_room,
            retention,
        )
    } else {
        mv_artifact::execute_retained(
            &crate::project::export::CliEngine::capture(identity.lane())
                .map_err(crate::project::export::host_failure)?,
            &held,
            &result_room,
            retention,
        )
        .map(|answer| answer.to_value())
    }
    .map_err(|refusal| match refusal {
        MvPreviewRefusal::Kernel(error) => crate::project::mv_setup::failure(error),
        MvPreviewRefusal::Host(failure) => crate::project::export::host_failure(failure),
    })?;
    current()?;
    Ok(answer)
}

fn run_job_read(inputs: &Inputs, key: &str, page: Option<usize>) -> Result<Value, Failure> {
    let mixed = COMMAND
        .args
        .iter()
        .filter(|arg| {
            !matches!(
                arg.name,
                "project" | "lane" | "preview-job" | "preview-page"
            )
        })
        .filter(|arg| {
            inputs.value(arg.name).is_some()
                || inputs.switch(arg.name)
                || !inputs.repeated(arg.name).is_empty()
        })
        .map(|arg| arg.name)
        .collect::<Vec<_>>();
    if !mixed.is_empty() {
        return Err(mode_invalid(format!(
            "--preview-job excludes --{}",
            mixed.join(", --")
        )));
    }
    let project = project_input(inputs, "project")?;
    let lane = inputs.require("lane")?;
    let identity = ds_cli_auth::headless_identity_for_named_project(lane)?;
    let room = ds_project_data::preview_room::Room {
        root: ds_layer_store::default_root()
            .map_err(|error| Failure::failed(crate::project::export::STAGING_FAILED.code, error))?,
        scope: ds_command_kernel::project_dataset_cache::Scope {
            principal: identity.uid().into(),
            project: project.into(),
        },
        lane: identity.lane().into(),
        credential_audience_sha256: identity.credential_audience_sha256().into(),
    };
    let answer = ds_report_host::mv_job::read(&room, key, page)
        .map_err(crate::project::export::host_failure)?;
    if ds_cli_auth::headless_identity_for_named_project(lane)? != identity {
        return Err(Failure::invalid(
            "print_project_crs_context_mismatch",
            "native preview account changed while reading",
        ));
    }
    Ok(answer)
}

fn project_input<'a>(inputs: &'a Inputs, name: &str) -> Result<&'a str, Failure> {
    inputs
        .value(name)
        .ok_or_else(|| mode_invalid(format!("--{name} is required without --request")))
}

fn run_project(inputs: &Inputs, model: Option<model_mode::ModelSource>) -> Result<Value, Failure> {
    let project = project_input(inputs, "project")?;
    let (scene, plan) = if model.is_some() {
        (None, None)
    } else {
        (
            Some(PathBuf::from(project_input(inputs, "scene")?)),
            Some(PathBuf::from(project_input(inputs, "plan")?)),
        )
    };
    let out_dir = PathBuf::from(project_input(inputs, "out-dir")?);
    for (name, path) in [("scene", &scene), ("plan", &plan)] {
        if let Some(path) = path
            && !path.is_file()
        {
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
    let selected = scene
        .as_deref()
        .map(|scene| stage_selected_scene(scene, inputs.repeated("alignment")))
        .transpose()?
        .flatten();
    let (resolved, style_resolution, renderer_defaults, report_config, project_crs, styles) =
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
    let mut context_page_files = read_manifest("context-pages", json!([]))?;
    // The held PDFs the adopted setup names are known now. Refuse a missing or
    // surplus asset before the minutes-long context acquisition and render,
    // exactly as the reporter would after them.
    let publication_assets = read_manifest("publication-assets", json!({}))?;
    assets::check_publication_assets(&resolved.preserved_pages, &publication_assets)?;
    // A pinned model carries the governed geographic context by default, read
    // from what this machine holds. Explicit captures replace it.
    let context = match (&model, inputs.value("context-pages")) {
        (Some(model), None) => {
            let lane = inputs.require("lane")?;
            let identity = ds_cli_auth::headless_identity_for_named_project(lane)?;
            let acquired =
                context::acquire(lane, project, &identity, &model.projection()?, &styles)?;
            context_page_files = json!(acquired.files);
            Some(acquired)
        }
        _ => None,
    };
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
        .or(scene.as_deref());
    let mut request = json!({"project_crs":project_crs,"report_config":report_config,"renderer_defaults":renderer_defaults,"style_resolution":style_resolution,"preview_only":inputs.switch("preview-only"),"project_id":project,"side_profiles_path":inputs.value("side-profiles"),"notes_path":inputs.value("notes"),"structure_descriptions_path":inputs.value("structure-descriptions"),"out_dir":out_dir,"sample_pages":sample_pages,"context_page_files":context_page_files,"model_crs":inputs.value("model-crs"),"settings":resolved.settings,"mv_setup":resolved,"publication_assets":publication_assets});
    if let Some(path) = context
        .as_ref()
        .and_then(|context| context.key_plan.as_ref())
    {
        request["key_plan_context_file"] = json!(path);
    }
    let (task, request) =
        model_mode::bind_request(request, model.as_ref(), scene_path, plan.as_deref());
    let bytes = serde_json::to_vec(&request)
        .map_err(|e| Failure::internal("request_encode_failed", e.to_string()))?;
    let engine = capture_engine(inputs)?;
    let mut document = staged.render(&engine, bytes, task)?;
    if let Some(context) = &context {
        document["mv_context"] = context.receipt.clone();
    }
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
    fn render(
        &self,
        engine: &ds_cli_exec::CapturedExternal,
        bytes: Vec<u8>,
        task: RenderTask,
    ) -> Result<Value, Failure> {
        ds_layer_store::private::write(&self.request_path, bytes)
            .map_err(|e| Failure::failed("request_write_failed", e.to_string()))?;
        let args = vec![
            OsString::from("--request"),
            self.request_path.clone().into(),
            OsString::from("--result"),
            self.result_path.clone().into(),
        ];
        let completed = engine.call(task.subcommand(), &args, EXPORT_TIMEOUT);
        let _ = std::fs::remove_file(&self.request_path);
        let completed = completed?;
        let document = std::fs::read(&self.result_path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok());
        if !self.keep {
            let _ = std::fs::remove_file(&self.result_path);
        }
        if !completed.succeeded() {
            return Err(DS_REPORT.failure_from(&completed, task.subcommand()));
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
    let scene: ds_network_contracts::ProfileAtlasScene = serde_json::from_slice(&bytes)
        .map_err(|e| Failure::invalid("alignment_selection_invalid", e.to_string()))?;
    let selected =
        ds_network_contracts::publication_scope::select_publication_alignments(&scene, ids)
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
    if data["schema"] == ds_report_host::mv_job::SCHEMA {
        let path = data["selected_page"]["page"]["path"]
            .as_str()
            .unwrap_or("No selected page is ready yet.");
        return format!(
            "MV preview {} · {}/{} ready
job {}
{}",
            data["progress"]["stage"].as_str().unwrap_or("preparing"),
            data["progress"]["completed"],
            data["progress"]["total"],
            data["job"]["input_sha256"].as_str().unwrap_or("?"),
            path
        );
    }
    if data["schema"] == ds_report_host::mv_artifact::SCHEMA {
        return format!(
            "MV preview sheet {} · {}",
            data["sheet"]["source_sheet_number"],
            data["sheet"]["page"]["path"].as_str().unwrap_or("?")
        );
    }
    if data.get("source_model_sha256").is_some() {
        if let Some(print) = data.get("print").filter(|value| value.is_object()) {
            return render(print);
        }
        return format!(
            "{} model preview pages / {} alignments\nrevision {}",
            data["preview"]["page_count"],
            data["preview"]["alignments"],
            data["source_authored_revision"].as_str().unwrap_or("?")
        );
    }
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
mod tests;
