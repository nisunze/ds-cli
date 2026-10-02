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

use crate::{DS_REPORT, EXPORT_TIMEOUT};

pub static COMMAND: Command = Command {
    id: "report.plan-profile",
    path: &["report", "plan-profile"],
    contract: 3,
    summary: "Render DS Grid plan/profile sheets from a pinned scene and plan.",
    purpose: "Resolve the named project canonical MV setup from its exact adopted printing-library revision, then render same-revision engine projections and approved front matter into one local PDF. Title, party logos, page order, scales, fonts and fixed publication version/date come only from that setup. Allowed model identity/title differences are explicit. Missing configuration or held approved assets refuses before output. Use report layout copy and report project mv-setup set to adopt and select; no print data is mutated by this command.",
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
        command: "ds report plan-profile --project gisagara --scene /tmp/profile.json --plan /tmp/plan.json --out-dir /tmp/gisagara-sheets --output json",
        note: "Render a new simple A3 set from held engine projections.",
        runnable: false,
    }],
    refusals: &crate::project::joined::<{ crate::project::NATIVE_READ_REFUSALS.len() + 10 }>(&[
        crate::project::NATIVE_READ_REFUSALS,
        &[
            crate::project::mv_setup::REFUSAL,
            crate::project::mv_setup::STYLE_REFUSAL,
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
    search: &["dsgrid", "print", "staking description", "workbook column"],
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
    let (resolved, style_resolution) =
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
    let request = json!({"style_resolution":style_resolution,"preview_only":inputs.switch("preview-only"),"project_id":project,"scene_path":scene,"plan_path":plan,"side_profiles_path":inputs.value("side-profiles"),"notes_path":inputs.value("notes"),"structure_descriptions_path":inputs.value("structure-descriptions"),"out_dir":out_dir,"sample_pages":sample_pages,"context_page_files":context_page_files,"model_crs":inputs.value("model-crs"),"settings":resolved.settings,"mv_setup":resolved,"publication_assets":publication_assets});
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
    if keep {
        document["result_path"] = json!(result_path.display().to_string());
    }
    Ok(document)
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
