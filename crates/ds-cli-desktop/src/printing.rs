//! Prepare the paired application's project printing workflow; Brain owns saves.
use crate::ops::{self, BridgeOp, DESCRIPTOR_ARG, TARGET_ARG};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, ArgKind, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
};
use serde_json::{Value, json};
use std::{io::Read, time::Duration};

pub const PREPARE_OP: BridgeOp = BridgeOp {
    operation: "printing.prepare",
    arguments: &["request"],
};
pub const SEED_CONTEXT_OP: BridgeOp = BridgeOp {
    operation: "printing.seed_context",
    arguments: &["transformer"],
};
pub static SEED_CONTEXT_COMMAND: Command = Command {
    id: "desktop.printing.seed-context",
    path: &["desktop", "printing", "seed-context"],
    contract: 1,
    summary: "Seed selected geographic context before a transformer is printed.",
    purpose: "Runs the explicit acquisition stage for one transformer's selected printing setups: downloads missing indexed geographic datasets, caches current project MV models and retains bounded derived building/contour context. Report export performs this same preparation automatically when selected context is missing; offline export reads held data and names missing coverage. No template, design geometry or project version is changed.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "transformer",
            "<name>",
            "One exact project transformer name.",
        )
        .required(),
        TARGET_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "Project, transformer, complete, named warnings and actual cached feature counts per layer.",
    examples: &[],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::SIGNED_OUT,
    ],
    reference: Some("docs/reference/desktop.printing.md"),
    search: &[],
    requires: Requires::Window,
    availability: ops::paired_availability,
};
pub fn seed_context(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    ops::invoke(
        &ops::paired(inputs.value("desktop-descriptor"))?,
        &SEED_CONTEXT_OP,
        json!({"transformer": inputs.require("transformer")?}),
        Duration::from_secs(30 * 60),
    )
    .map_err(ops::classify_signed_out)
}
pub const SETTINGS_OP: BridgeOp = BridgeOp {
    operation: "printing.settings",
    arguments: &["project"],
};
pub static SETTINGS_COMMAND: Command = Command {
 id: "desktop.printing.settings", path: &["desktop", "printing", "settings"], contract: 1,
 summary: "Read project print settings, selected outputs and template papers.",
 purpose: "Read the saved design output selection for one exact project through Brain and the shared Rust report planner. Returns the authored setting, effective outputs and selected template paper metadata; never guesses from filenames, changes settings, runs an export, or switches the GUI map. Use report layout list/get to inspect templates natively. This read does not choose outputs for printing export: supply its explicit --selection matrix to print selected papers; otherwise the documented data defaults apply. Same printing MCP profile.",
 chapter: Chapter::Reports, effect: Effect::ReadOnly, authority: Authority::DesktopUser, execution: Execution::Sync,
 args: &[Arg::value("project", "<exact-id>", "Exact project whose saved printing output settings should be read; no GUI project switch.").required(), TARGET_ARG, DESCRIPTOR_ARG],
 output: "Exact project, selection source, authored setting, planned outputs, selected template papers and receipt SHA-256. No design features or credentials.", examples: &[],
 refusals: &[ops::NOT_PAIRED,ops::AMBIGUOUS,ops::UNREACHABLE,ops::PAIRING_REJECTED,ops::REFUSED,ops::UNSUPPORTED,ops::UNREADABLE,ops::SIGNED_OUT,PRINTING_READ_INVALID],
 reference: Some("docs/reference/desktop.printing.md"), search: &[], requires: Requires::Window, availability: ops::paired_availability,
};
pub fn settings(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = bounded_project(inputs.require("project")?)?;
    ops::invoke(
        &ops::paired(inputs.value("desktop-descriptor"))?,
        &SETTINGS_OP,
        json!({"project":project}),
        Duration::from_secs(240),
    )
    .map_err(ops::classify_signed_out)
}

pub const TRANSFORMERS_OP: BridgeOp = BridgeOp {
    operation: "printing.transformers",
    arguments: &["project", "limit"],
};
pub const EXPORT_OP: BridgeOp = BridgeOp {
    operation: "printing.export",
    arguments: &[
        "project",
        "transformer",
        "transformers",
        "force",
        "selection",
        "intent",
        "preview_retention",
    ],
};
pub const PREVIEW_OP: BridgeOp = BridgeOp {
    operation: "printing.preview",
    arguments: &[
        "project",
        "transformer",
        "layout",
        "template",
        "out",
        "retention",
    ],
};
pub static PREVIEW_COMMAND: Command = Command {
    id: "desktop.printing.preview",
    path: &["desktop", "printing", "preview"],
    contract: 2,
    summary: "Render a transformer SVG into the native disk preview cache.",
    purpose: "Use exactly one --layout draft or --template catalogue location in the current signed-in project. Native authenticated template reads, exact styles and the held transformer room feed the shared headless renderer. Returns a verified local file manifest with diagnostics and omissions; no inline page bytes. Exact repeats reuse native results and disk bytes. Default retention24h; optional7d. --out creates a verified new copy. No save, publication, project switch, geographic-provider acquisition or cloud compute.",
    chapter: Chapter::Reports,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<exact-id>",
            "Exact current Desktop project; a mismatch refuses without switching it.",
        )
        .required(),
        Arg::value(
            "transformer",
            "<name>",
            "One canonical individual transformer name; combined_transformer is refused.",
        )
        .required(),
        Arg::value(
            "layout",
            "<json-file>",
            "Full draft layout, at most800KB; mutually exclusive with --template.",
        ),
        Arg::value(
            "template",
            "<json-file>",
            "Exact layout Location from desktop printing view --catalogue; mutually exclusive with --layout.",
        ),
        Arg::value(
            "out",
            "<absolute.svg>",
            "Optional new absolute SVG copy; existing paths refuse.",
        ),
        Arg::value(
            "retention",
            "<24h|7d>",
            "Managed cache lifetime, default24h; reuse keeps original expiry.",
        )
        .choices(&["24h", "7d"]),
        TARGET_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "Project, pinned preview identity, ordered native file manifest (path,SHA-256,size,contenttype,scope/result pins,expiry), diagnostics,warnings,omissions and optionalout. Metadata bridge bound8MiB; no inlineSVG.",
    examples: &[],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::SIGNED_OUT,
        PRINTING_READ_INVALID,
    ],
    reference: Some("docs/reference/desktop.printing.md"),
    search: &[],
    requires: Requires::Window,
    availability: ops::paired_availability,
};
const FORCE_ARG: Arg = Arg {
    name: "force",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Regenerate from the held local room even when the current report is fresh.",
};

const PRINTING_READ_INVALID: Refusal = Refusal {
    code: "printing_request_invalid",
    when: "the setup id or explicit project is invalid, or --project is combined with global scope",
    remedy: "use one exact bounded project id; named setups are read and published natively with `ds report layout list|get|save`",
};
const PRINTING_EXPORT_INVALID: Refusal = Refusal {
    code: "printing_request_invalid",
    when: "the explicit project or transformer is invalid, or preview intent selects combined_transformer",
    remedy: "use canonical project/transformer names; preview individual transformers or omit --intent for the ordinary combined export",
};

pub static TRANSFORMERS_COMMAND: Command = Command {
    id: "desktop.printing.transformers",
    path: &["desktop", "printing", "transformers"],
    contract: 1,
    summary: "List printable transformers in one explicit project.",
    purpose: "Reads the fresh bounded transformer inventory for one explicit project under the paired user's Brain permissions. It reports only transformer identity and local-room readiness needed to choose a print target; it does not open or switch the Desktop map project.",
    chapter: Chapter::Reports,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<exact-id>",
            "Exact project to inspect without changing the Desktop map project.",
        )
        .required(),
        Arg::value(
            "limit",
            "<n>",
            "Return at most this many transformers; 1..500.",
        )
        .default("100"),
        TARGET_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "Explicit project, total printable transformer count, bounded name/kind/cache/version rows, and omitted count.",
    examples: &[],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::SIGNED_OUT,
        ds_cli_contract::args::INVALID_NUMBER,
        PRINTING_READ_INVALID,
    ],
    reference: Some("docs/reference/desktop.printing.md"),
    search: &[],
    requires: Requires::Window,
    availability: ops::paired_availability,
};

pub static EXPORT_COMMAND: Command = Command {
    id: "desktop.printing.export",
    path: &["desktop", "printing", "export"],
    contract: 4,
    summary: "Export selected formats for one or more held transformers.",
    purpose: "Run the desktop-native Network Reporter for one explicit project; repeat --transformer for a batch. Defaults: SHP/KMZ/XLSX; they never require voltage drop. --selection supplies a run matrix without saving settings. Main exports replace selected outputs and queue publication; unselected provenance is retained. --intent preview is local-only, without main output replacement, publication or settings writes; combined_transformer cannot be previewed. Acquire missing selected context online; use held data offline.",
    chapter: Chapter::Reports,
    effect: Effect::ArtifactWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<exact-id>",
            "Exact project whose held local transformer room will be printed; never changes the Desktop map project.",
        )
        .required(),
        Arg::repeated(
            "transformer",
            "<name>",
            "Canonical transformer name; repeat for a batch. combined_transformer is accepted only alone without a local selection override.",
        )
        .required(),
        Arg::value("selection", "<json-file>", "Local ds.design-output-selection/v1 matrix. Only selected outputs are regenerated; no project settings are saved."),
        Arg::value("intent", "<preview>", "Produce local-only review artifacts; omit for the ordinary main export.").choices(&["preview"]),
        Arg::value("preview-retention", "<24h|7d>", "A4-only preview disk lifetime, default24h; reuse keeps original expiry.").choices(&["24h","7d"]),
        FORCE_ARG,
        TARGET_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "For one transformer: explicit project and transformer, artifact count, exact filenames/formats/sizes/SHA-256/locators and recorded layout/paper/orientation/dimensions, context warnings and cached layer feature counts, and publication state. A batch returns per-transformer receipts and a failed count.",
    examples: &[],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::SIGNED_OUT,
        PRINTING_EXPORT_INVALID,
        Refusal {
            code: "confirmation_required",
            when: "--yes was not given for a command that writes report artifacts",
            remedy: "re-run with --yes once you intend the local export",
        },
    ],
    reference: Some("docs/reference/desktop.printing.md"),
    search: &[], requires: Requires::Window,
    availability: ops::paired_availability,
};

pub static PREPARE_COMMAND: Command = Command {
    id: "desktop.printing.prepare",
    path: &["desktop", "printing", "prepare"],
    contract: 1,
    summary: "Save a project print layout, select exports and prepare inputs.",
    purpose: "Prepares the request's exact project as the paired signed-in user; never reads/changes the Desktop map project. Request: project, layout, expectedRevision (empty for create), ds.design-output-selection/v1 paper-by-format selection. Optional overrides map canonical transformer names to kernel-validated element/table/legend/style instructions for this layout; null removes its exception. Saves settings only at the current read base; saves survive later preparation failures (sequence in reference). Edit using the returned revision. No report export; follow with desktop printing export.",
    chapter: Chapter::Reports,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "request",
            "<json-file>",
            "Exact project, authored layout, expectedRevision, versioned output selection, optional transformer views/transformer-keyed overrides; null removes this layout's exception. <=800 KB.",
        )
        .required(),
        TARGET_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "Resolved project, saved setup id/name/revision, selected output matrix and ready=true; no raw design features or credentials.",
    examples: &[],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::SIGNED_OUT,
        Refusal {
            code: "printing_request_invalid",
            when: "the request file cannot be read or is not a bounded JSON object",
            remedy: "provide a JSON object containing layout, expectedRevision and selection, at most 800 KB",
        },
    ],
    reference: Some("docs/reference/desktop.printing.md"),
    search: &[], requires: Requires::Window,
    availability: ops::paired_availability,
};

fn read_request(path: &str, remedy: &'static str) -> Result<Value, Failure> {
    let invalid =
        |message: String| Failure::invalid("printing_request_invalid", message).remedy(remedy);
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|file| file.take(800_001).read_to_end(&mut bytes))
        .map_err(|e| invalid(e.to_string()))?;
    if bytes.len() > 800_000 {
        return Err(invalid("printing request exceeds 800 KB".into()));
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
    if !value.is_object() {
        return Err(invalid("printing request must be an object".into()));
    }
    Ok(value)
}
pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let request = read_request(
        inputs.require("request")?,
        "provide a JSON object containing layout, expectedRevision and selection, at most 800 KB",
    )?;
    require_request_project(&request, "printing.prepare")?;
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(
        &descriptor,
        &PREPARE_OP,
        json!({"request":request}),
        Duration::from_secs(600),
    )
}

pub fn transformers(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = bounded_project(inputs.require("project")?)?;
    let limit = ds_cli_contract::args::integer(inputs.require("limit")?, "limit", 1, 500)?;
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(
        &descriptor,
        &TRANSFORMERS_OP,
        json!({"project": project, "limit": limit}),
        Duration::from_secs(120),
    )
    .map_err(ops::classify_signed_out)
}

fn export_arguments(inputs: &Inputs) -> Result<Value, Failure> {
    let project = bounded_project(inputs.require("project")?)?;
    let transformers = inputs.repeated("transformer");
    if transformers.is_empty() || transformers.len() > 2000 {
        return Err(invalid_read("select 1..2000 transformers"));
    }
    for transformer in transformers {
        bounded_transformer(transformer)?;
    }
    if inputs.value("intent") == Some("preview")
        && transformers
            .iter()
            .any(|name| name == "combined_transformer")
    {
        return Err(Failure::invalid(
            PRINTING_EXPORT_INVALID.code,
            "combined_transformer does not support preview intent",
        )
        .remedy(PRINTING_EXPORT_INVALID.remedy));
    }
    let mut arguments = json!({"project": project, "force": inputs.switch("force")});
    if let Some(retention) = inputs.value("preview-retention") {
        if inputs.value("intent") != Some("preview") {
            return Err(invalid_read(
                "--preview-retention requires --intent preview selecting only voltage_drop_pdf",
            ));
        }
        arguments["preview_retention"] = json!(retention);
    }
    if let Some(intent) = inputs.value("intent") {
        arguments["intent"] = json!(intent);
    }
    if transformers.len() == 1 {
        arguments["transformer"] = json!(transformers[0]);
    } else {
        arguments["transformers"] = json!(transformers);
    }
    // Without --selection the Desktop resolves the run through the kernel:
    // the ordinary held export runs its SHP/KMZ/XLSX defaults, which never
    // require the saved voltage-drop analysis.
    if let Some(path) = inputs.value("selection") {
        arguments["selection"] =
            read_request(path, "provide a ds.design-output-selection/v1 JSON matrix")?;
    }
    Ok(arguments)
}

fn bounded_transformer(transformer: &str) -> Result<&str, Failure> {
    if transformer.is_empty()
        || transformer.len() > 121
        || !transformer
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
        || !(transformer.as_bytes()[0].is_ascii_lowercase()
            || (transformer.starts_with('_')
                && transformer
                    .as_bytes()
                    .get(1)
                    .is_some_and(u8::is_ascii_digit)))
    {
        return Err(invalid_read("invalid canonical transformer name"));
    }
    Ok(transformer)
}

fn preview_arguments(inputs: &Inputs, layout: Value) -> Result<Value, Failure> {
    let project = bounded_project(inputs.require("project")?)?;
    let transformer = bounded_transformer(inputs.require("transformer")?)?;
    if transformer == "combined_transformer" {
        return Err(invalid_read(
            "live sheet preview requires one individual transformer",
        ));
    }
    let parsed: ds_command_kernel::printing::Layout =
        serde_json::from_value(layout.clone()).map_err(|error| invalid_read(error.to_string()))?;
    ds_command_kernel::printing::validate(&parsed).map_err(invalid_read)?;
    let mut arguments = json!({"project":project,"transformer":transformer,"layout":layout});
    preview_delivery_arguments(inputs, &mut arguments)?;
    Ok(arguments)
}

fn preview_delivery_arguments(inputs: &Inputs, arguments: &mut Value) -> Result<(), Failure> {
    if let Some(raw) = inputs.value("out") {
        let path = std::path::Path::new(raw);
        if raw.is_empty()
            || raw.len() > 4096
            || raw.chars().any(char::is_control)
            || !path.is_absolute()
            || path.extension().and_then(|v| v.to_str()) != Some("svg")
            || std::fs::symlink_metadata(path).is_ok()
            || !path.parent().is_some_and(|parent| {
                std::fs::symlink_metadata(parent)
                    .is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
            })
        {
            return Err(invalid_read(
                "preview output must be a new absolute .svg in an existing real directory",
            ));
        }
        arguments["out"] = json!(raw);
    }
    if let Some(value) = inputs.value("retention") {
        arguments["retention"] = json!(value);
    }
    Ok(())
}
fn preview_template_arguments(inputs: &Inputs, template: Value) -> Result<Value, Failure> {
    let project = bounded_project(inputs.require("project")?)?;
    let transformer = bounded_transformer(inputs.require("transformer")?)?;
    if transformer == "combined_transformer" {
        return Err(invalid_read(
            "live sheet preview requires one individual transformer",
        ));
    }
    let location = serde_json::from_value(template.clone())
        .map_err(|e| invalid_read(format!("invalid template location: {e}")))?;
    ds_command_kernel::printing::template_read::admit_location(&location).map_err(invalid_read)?;
    let mut arguments = json!({"project":project,"transformer":transformer,"template":template});
    preview_delivery_arguments(inputs, &mut arguments)?;
    Ok(arguments)
}

fn admit_preview_result(inputs: &Inputs, data: Value) -> Result<Value, Failure> {
    let rows = data["results"]
        .as_array()
        .ok_or_else(|| invalid_read("native preview result rows are missing"))?;
    let digest = |v: &Value| {
        v.as_str().is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    };
    let row = rows
        .first()
        .ok_or_else(|| invalid_read("native preview page is missing"))?;
    let page = &row["page"];
    let retained = page["retention_ms"].as_u64();
    if data["project"] != inputs.require("project")?
        || rows.len() != 1
        || row["transformer"] != inputs.require("transformer")?
        || data["preview"]["artifact_schema"] != "ds.print-preview-artifact/v1"
        || page["schema"] != "ds.print-preview-artifact/v1"
        || page.get("svg").is_some()
        || page.get("png_data_url").is_some()
        || !page["path"]
            .as_str()
            .is_some_and(|s| std::path::Path::new(s).is_absolute())
        || !digest(&page["sha256"])
        || !digest(&page["authority_sha256"])
        || !digest(&page["result_sha256"])
        || !digest(&data["preview"]["layout_sha256"])
        || page["sha256"] != row["sha256"]
        || page["size_bytes"] != row["size_bytes"]
        || page["content_type"] != "image/svg+xml"
        || !page["size_bytes"]
            .as_u64()
            .is_some_and(|n| n > 0 && n <= 96 * 1024 * 1024)
        || !retained.is_some_and(|n| [86_400_000, 604_800_000].contains(&n))
        || page["created_at_unix_ms"]
            .as_u64()
            .and_then(|v| v.checked_add(retained?))
            != page["expires_at_unix_ms"].as_u64()
        || inputs.value("out").is_some_and(|out| row["out"] != out)
    {
        return Err(Failure::failed("desktop_unreadable","The native preview file receipt differs from the explicit request or its digest/expiry pins.")
            .remedy("use matching Desktop/CLI and retain any written preview file; no inline SVG is admitted"));
    }
    Ok(data)
}

pub fn preview(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    // Project/name/document validation precedes pairing or any application call.
    bounded_project(inputs.require("project")?)?;
    bounded_transformer(inputs.require("transformer")?)?;
    let arguments = match (inputs.value("layout"), inputs.value("template")) {
        (Some(path), None) => preview_arguments(
            inputs,
            read_request(path, "provide a draft layout at most800KB")?,
        )?,
        (None, Some(path)) => preview_template_arguments(
            inputs,
            read_request(
                path,
                "provide the exact layout Location from desktop printing view --catalogue",
            )?,
        )?,
        _ => return Err(invalid_read("supply exactly one --layout or --template")),
    };
    let result = ops::invoke(
        &ops::paired(inputs.value("desktop-descriptor"))?,
        &PREVIEW_OP,
        arguments,
        Duration::from_secs(30 * 60),
    )
    .map_err(ops::classify_signed_out)?;
    admit_preview_result(inputs, result)
}

pub fn export(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = export_arguments(inputs)?;
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(
        &descriptor,
        &EXPORT_OP,
        arguments,
        Duration::from_secs(30 * 60),
    )
    .map_err(ops::classify_signed_out)
}

fn invalid_read(message: impl Into<String>) -> Failure {
    Failure::invalid("printing_request_invalid", message).remedy(PRINTING_READ_INVALID.remedy)
}

fn require_request_project<'a>(request: &'a Value, operation: &str) -> Result<&'a str, Failure> {
    let project = request
        .get("project")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid_read(format!("{operation} requires one explicit project id")))?;
    bounded_project(project)
}

fn bounded_project(value: &str) -> Result<&str, Failure> {
    ds_client_core::validate_project_id(value).map_err(|error| invalid_read(error.to_string()))?;
    Ok(value)
}
pub fn render(data: &Value) -> String {
    format!("{data}\n")
}

#[cfg(test)]
#[path = "printing_tests.rs"]
mod tests;
