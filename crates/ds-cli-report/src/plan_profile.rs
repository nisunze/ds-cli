//! `ds report plan-profile` — headless DS Grid sheet rendering through the
//! reporter's typed task. The engine owns every projection and drawing byte.

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
    contract: 1,
    summary: "Render DS Grid plan/profile sheets from a pinned scene and plan.",
    purpose: "Produces A3 SVG sheet previews and one combined vector PDF. Both inputs must be exact DS Grid engine projections for the same model revision. Choose horizontal and vertical scale denominators independently; profile elevation breaks keep the preferred vertical scale where a steep section requires a new datum on the same sheet. The task is local and headless; the result names every preview and its digest-pinned PDF.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<id>",
            "Exact project context for this print run.",
        )
        .required(),
        Arg::value(
            "scene",
            "<path>",
            "Absolute project_profile_atlas scene JSON path.",
        )
        .required(),
        Arg::value("plan", "<path>", "Absolute project_plan row JSON path.").required(),
        Arg::value(
            "out-dir",
            "<path>",
            "Fresh absolute directory for SVG previews and PDF.",
        )
        .required(),
        Arg::value(
            "format",
            "<simple|advanced>",
            "Sheet density and base scales.",
        )
        .required()
        .choices(&["simple", "advanced"]),
        Arg::value("title", "<text>", "Project title printed on every page.").required(),
        Arg::value("sheet-title", "<text>", "Drawing title.").default("MV plan & profile"),
        Arg::value(
            "ink",
            "<monochrome|reference_accents>",
            "Mostly black pens or restrained conductor and structure accents from the approved 120 ACSR reference.",
        )
        .default("monochrome")
        .choices(&["monochrome", "reference_accents"]),
        Arg::value(
            "horizontal-scale",
            "<denominator>",
            "Advanced format horizontal denominator, 500..10000.",
        ),
        Arg::value(
            "vertical-scale",
            "<denominator>",
            "Advanced format preferred vertical denominator, 100..5000.",
        ),
        Arg::value(
            "plan-scale",
            "<denominator>",
            "Advanced format plan denominator, 500..10000; must equal horizontal scale for the geographic plan scale.",
        ),
        Arg::value(
            "panel-order",
            "<profile_top|plan_top>",
            "Place the profile or plan panel above the other.",
        )
        .default("profile_top")
        .choices(&["profile_top", "plan_top"]),
        Arg::value(
            "label-orientation",
            "<vertical|horizontal>",
            "Station-aligned upward labels inside the profile, or the horizontal ledger.",
        )
        .default("vertical")
        .choices(&["vertical", "horizontal"]),
        Arg::value(
            "long-axis",
            "<on|off>",
            "Permit rotated plan sections where the physical bend cannot fit the panel.",
        )
        .default("on")
        .choices(&["on", "off"]),
        Arg::value(
            "angle-policy",
            "<preserve_if_fit|split_at_authored>",
            "Keep a physical angled span when its footprint fits the plan, or force an authored angle cut.",
        )
        .default("preserve_if_fit")
        .choices(&["preserve_if_fit", "split_at_authored"]),
        Arg::value(
            "angle-gap-mm",
            "<millimetres>",
            "Local break-mark size at a plan angle section, 2..30 mm.",
        ),
        Arg::value(
            "min-angle-deg",
            "<degrees>",
            "Minimum route deflection that opens a plan gap, 0..90 degrees.",
        ),
        Arg::value(
            "plan-buffer-m",
            "<metres>",
            "Dashed plan corridor on either side of the route; default 6 m, zero hides it.",
        ),
        Arg::value(
            "profile-grid",
            "<on|off>",
            "Draw major and minor station/elevation grids in the profile.",
        )
        .default("on")
        .choices(&["on", "off"]),
        Arg::value(
            "profile-elevation-breaks",
            "<on|off>",
            "Reset the elevation datum at a structure within a sheet where the preferred vertical scale cannot fit.",
        )
        .default("on")
        .choices(&["on", "off"]),
        Arg::value(
            "profile-continuations",
            "<on|off>",
            "Repeat the cut structure with incoming and outgoing wires and matched sheet references.",
        )
        .default("on")
        .choices(&["on", "off"]),
        Arg::value(
            "attachments",
            "<auto|show|hide>",
            "Show exact engine profile attachment positions.",
        )
        .default("auto")
        .choices(&["auto", "show", "hide"]),
        Arg::value(
            "span-labels",
            "<auto|show|hide>",
            "Show the engine's physical span labels once per attachment set.",
        )
        .default("auto")
        .choices(&["auto", "show", "hide"]),
        Arg::value(
            "feature-codes",
            "<auto|show|hide>",
            "Show meaningful engine feature names at surveyed profile points; generic ground-point codes are suppressed.",
        )
        .default("auto")
        .choices(&["auto", "show", "hide"]),
        Arg::value(
            "clearance",
            "<auto|show|hide>",
            "Show required clearance thresholds at coded features.",
        )
        .default("auto")
        .choices(&["auto", "show", "hide"]),
        Arg::value(
            "context-pages",
            "<manifest.json>",
            "Ordered project-pinned LV map capture paths, one per output sheet.",
        ),
        Arg::value(
            "model-crs",
            "<declared-crs>",
            "Selected model CRS, required for registered map context.",
        ),
        Arg::value(
            "logos",
            "<manifest.json>",
            "JSON array of one or two absolute PNG/JPEG logo paths for the bottom title block.",
        ),
        Arg::value(
            "label-rows",
            "<json-file>",
            "One to three ordered structure label lines built from canonical staking fields.",
        ),
        Arg::value(
            "sample-pages",
            "<count>",
            "Render 1..20 representative sheets, retaining their original sheet numbers and full set count.",
        ),
        Arg::value(
            "result",
            "<path>",
            "Keep the reporter receipt here; must not exist.",
        ),
    ],
    output: "Model revision, projection SHA-256 digests, page count, SVG preview paths, PDF path and PDF SHA-256.",
    examples: &[Example {
        command: "ds report plan-profile --project gisagara --scene /tmp/profile.json --plan /tmp/plan.json --format simple --title Gisagara --out-dir /tmp/gisagara-sheets --output json",
        note: "Render a new simple A3 set from held engine projections.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "logo_manifest_invalid",
            when: "the logo manifest cannot be read as a JSON array",
            remedy: "provide a valid JSON array of one or two absolute PNG/JPEG paths",
        },
        Refusal {
            code: "label_rows_invalid",
            when: "the structure label rows file cannot be read as JSON",
            remedy: "provide valid JSON with one to three ordered label rows",
        },
        Refusal {
            code: "request_encode_failed",
            when: "the validated renderer request cannot be encoded as JSON",
            remedy: "report the input and this build; the reporter was not started",
        },
        Refusal {
            code: "request_write_failed",
            when: "the renderer request file cannot be written locally",
            remedy: "check output-path permissions and available disk space",
        },
        Refusal {
            code: "reporter_engine_missing",
            when: "ds-report is unavailable",
            remedy: "install the matching reporter",
        },
        Refusal {
            code: "projection_missing",
            when: "the named scene or plan file is missing",
            remedy: "obtain both projections from the same model revision",
        },
        Refusal {
            code: "output_exists",
            when: "the output directory or result file already exists",
            remedy: "choose fresh output paths",
        },
        Refusal {
            code: "engine_refused",
            when: "the reporter cannot decode, pair, paginate or encode the sheets",
            remedy: "read detail.engine and correct the inputs",
        },
        Refusal {
            code: "invalid_scale",
            when: "a scale denominator is not a whole number",
            remedy: "use a positive integer in the printed scale range",
        },
        Refusal {
            code: "context_manifest_invalid",
            when: "the context page manifest is unreadable or not a JSON array of paths",
            remedy: "provide ordered complete map capture paths for this model revision",
        },
    ],
    reference: Some("docs/reference/report.md"),
    search: &["dsgrid", "print"],
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
    let scale = |name: &str| -> Result<Option<u32>, Failure> {
        inputs
            .value(name)
            .map(|value| {
                value.parse::<u32>().map_err(|_| {
                    Failure::invalid("invalid_scale", format!("{name} must be a whole number"))
                })
            })
            .transpose()
    };
    let decimal = |name: &str| -> Result<Option<f64>, Failure> {
        inputs
            .value(name)
            .map(|value| {
                value.parse::<f64>().map_err(|_| {
                    Failure::invalid("invalid_scale", format!("{name} must be a decimal number"))
                })
            })
            .transpose()
    };
    let selection = |name: &str| match inputs.value(name).unwrap_or("auto") {
        "show" => Some(true),
        "hide" => Some(false),
        _ => None,
    };
    let context_page_files: Vec<PathBuf> = match inputs.value("context-pages") {
        Some(path) => {
            let bytes = std::fs::read(path)
                .map_err(|e| Failure::invalid("context_manifest_invalid", e.to_string()))?;
            serde_json::from_slice(&bytes)
                .map_err(|e| Failure::invalid("context_manifest_invalid", e.to_string()))?
        }
        None => Vec::new(),
    };
    let logo_files: Vec<PathBuf> = match inputs.value("logos") {
        Some(path) => {
            let bytes = std::fs::read(path)
                .map_err(|e| Failure::invalid("logo_manifest_invalid", e.to_string()))?;
            serde_json::from_slice(&bytes)
                .map_err(|e| Failure::invalid("logo_manifest_invalid", e.to_string()))?
        }
        None => Vec::new(),
    };
    let label_rows: Value = match inputs.value("label-rows") {
        Some(path) => {
            let bytes = std::fs::read(path)
                .map_err(|e| Failure::invalid("label_rows_invalid", e.to_string()))?;
            serde_json::from_slice(&bytes)
                .map_err(|e| Failure::invalid("label_rows_invalid", e.to_string()))?
        }
        None => json!([]),
    };
    let request = json!({"project_id":project,"scene_path":scene,"plan_path":plan,"out_dir":out_dir,"sample_pages":scale("sample-pages")?,"context_page_files":context_page_files,"logo_files":logo_files,"model_crs":inputs.value("model-crs"),"settings":{"format":inputs.require("format")?,"ink_mode":inputs.value("ink").unwrap_or("monochrome"),"project_title":inputs.require("title")?,"sheet_title":inputs.value("sheet-title").unwrap_or("MV plan & profile"),"horizontal_scale":scale("horizontal-scale")?,"vertical_scale":scale("vertical-scale")?,"plan_scale":scale("plan-scale")?,"panel_order":inputs.value("panel-order").unwrap_or("profile_top"),"structure_label_orientation":inputs.value("label-orientation").unwrap_or("vertical"),"long_axis_plot":inputs.value("long-axis").unwrap_or("on")=="on","plan_angle_policy":inputs.value("angle-policy").unwrap_or("preserve_if_fit"),"angle_gap_mm":decimal("angle-gap-mm")?.unwrap_or(7.0),"minimum_angle_deg":decimal("min-angle-deg")?.unwrap_or(0.0),"plan_buffer_m":decimal("plan-buffer-m")?.unwrap_or(6.0),"show_profile_grid":inputs.value("profile-grid").unwrap_or("on")=="on","profile_elevation_breaks":inputs.value("profile-elevation-breaks").unwrap_or("on")=="on","show_profile_continuations":inputs.value("profile-continuations").unwrap_or("on")=="on","show_attachment_points":selection("attachments"),"show_span_labels":selection("span-labels"),"show_feature_codes":selection("feature-codes"),"show_clearance_thresholds":selection("clearance"),"structure_label_rows":label_rows}});
    let bytes = serde_json::to_vec(&request)
        .map_err(|e| Failure::internal("request_encode_failed", e.to_string()))?;
    std::fs::write(&request_path, bytes)
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
    format!(
        "{} A3 sheets · {} alignments\nPDF {}\nrevision {}",
        data["page_count"],
        data["alignments"],
        data["pdf"].as_str().unwrap_or("?"),
        data["model_revision"].as_str().unwrap_or("?")
    )
}
