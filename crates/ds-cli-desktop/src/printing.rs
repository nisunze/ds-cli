//! Prepare the paired application's project printing workflow; Brain owns saves.
use crate::ops::{self, BridgeOp, DESCRIPTOR_ARG};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, ArgKind, Authority, Chapter, Command, Effect, Execution, Refusal},
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
 purpose: "Read the saved design output selection for one exact project through Brain and the shared Rust report planner. Returns the authored setting, effective outputs and selected template paper metadata; never guesses from filenames, changes settings, runs an export, or switches the GUI map. Start here before printing. Use report layout list/get to inspect templates natively, printing prepare to save an authorized selection, then read settings again and use printing export for a transformer. Available identically through the printing MCP profile.",
 chapter: Chapter::Reports, effect: Effect::ReadOnly, authority: Authority::DesktopUser, execution: Execution::Sync,
 args: &[Arg::value("project", "<exact-id>", "Exact project whose saved printing output settings should be read; no GUI project switch.").required(), DESCRIPTOR_ARG],
 output: "Exact project, selection source, authored setting, planned outputs, selected template papers and receipt SHA-256. No design features or credentials.", examples: &[],
 refusals: &[ops::NOT_PAIRED,ops::AMBIGUOUS,ops::UNREACHABLE,ops::PAIRING_REJECTED,ops::REFUSED,ops::UNSUPPORTED,ops::UNREADABLE,ops::SIGNED_OUT,PRINTING_READ_INVALID],
 reference: Some("docs/reference/desktop.printing.md"), availability: ops::paired_availability,
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
    ],
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
        ops::INVALID_NUMBER,
        PRINTING_READ_INVALID,
    ],
    reference: Some("docs/reference/desktop.printing.md"),
    availability: ops::paired_availability,
};

pub static EXPORT_COMMAND: Command = Command {
    id: "desktop.printing.export",
    path: &["desktop", "printing", "export"],
    contract: 2,
    summary: "Export selected formats for one or more held transformers.",
    purpose: "Runs the desktop-native Network Reporter for one explicit project and transformer. Repeat --transformer for a batch. Optional --selection reads a ds.design-output-selection/v1 matrix for this local run, leaving project settings unchanged. Selected canonical outputs are overwritten; unselected artifacts keep their producing provenance. Missing selected map context is acquired automatically when online; offline execution uses held data. Local artifacts are queued through the ordinary report publication outbox.",
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
        FORCE_ARG,
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
        PRINTING_READ_INVALID,
        Refusal {
            code: "confirmation_required",
            when: "--yes was not given for a command that writes report artifacts",
            remedy: "re-run with --yes once you intend the local export",
        },
    ],
    reference: Some("docs/reference/desktop.printing.md"),
    availability: ops::paired_availability,
};

pub static PREPARE_COMMAND: Command = Command {
    id: "desktop.printing.prepare",
    path: &["desktop", "printing", "prepare"],
    contract: 1,
    summary: "Save a project print layout, select exports and prepare inputs.",
    purpose: "Runs printing preparation for the request's required exact project under the paired signed-in user without reading or changing the Desktop map project. The request names project, layout, expectedRevision (empty for create) and a ds.design-output-selection/v1 paper-by-format selection. Optional overrides map canonical transformer names to kernel-validated element/table/legend/style instructions for this layout; null removes the exception. Project settings are saved only if their read base is still current. Brain validates and saves the layout and project settings; the app then refreshes the sealed receipt and installs required reference data. These are sequential durable actions: a later preparation failure does not roll back a saved layout. Use the returned revision for further edits. This does not export a report; follow with desktop printing export.",
    chapter: Chapter::Reports,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "request",
            "<json-file>",
            "Required exact project, authored layout, expectedRevision, versioned output selection, optional transformer views, and overrides keyed by transformer; null removes that layout exception. At most 800 KB.",
        )
        .required(),
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
    let limit = ops::integer(inputs.require("limit")?, "limit", 1, 500)?;
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(
        &descriptor,
        &TRANSFORMERS_OP,
        json!({"project": project, "limit": limit}),
        Duration::from_secs(120),
    )
    .map_err(ops::classify_signed_out)
}

pub fn export(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let project = bounded_project(inputs.require("project")?)?;
    let transformers = inputs.repeated("transformer");
    if transformers.is_empty() || transformers.len() > 2000 {
        return Err(invalid_read("select 1..2000 transformers"));
    }
    for transformer in transformers {
        if transformer.is_empty()
            || transformer.len() > 121
            || !transformer.bytes().enumerate().all(|(index, byte)| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || byte == b'_'
                    || (index == 0 && byte == b'_')
            })
            || (!transformer.as_bytes()[0].is_ascii_lowercase()
                && !(transformer.starts_with('_')
                    && transformer
                        .as_bytes()
                        .get(1)
                        .is_some_and(u8::is_ascii_digit)))
        {
            return Err(invalid_read("invalid canonical transformer name"));
        }
    }
    let mut arguments = json!({"project": project, "force": inputs.switch("force")});
    if transformers.len() == 1 {
        arguments["transformer"] = json!(transformers[0]);
    } else {
        arguments["transformers"] = json!(transformers);
    }
    if let Some(path) = inputs.value("selection") {
        arguments["selection"] =
            read_request(path, "provide a ds.design-output-selection/v1 JSON matrix")?;
    }
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
    if value.is_empty() || value.trim() != value || value.chars().count() > 160 {
        return Err(invalid_read(
            "`--project` must be non-empty, trimmed, and at most 160 characters",
        ));
    }
    Ok(value)
}
pub fn render(data: &Value) -> String {
    format!("{data}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_input_is_a_named_refusal_before_pairing() {
        assert_eq!(
            read_request("/nonexistent/printing-request.json", "remedy")
                .unwrap_err()
                .code(),
            "printing_request_invalid"
        );
    }

    #[test]
    fn read_operations_declare_explicit_project_without_a_switch_operation() {
        assert_eq!(TRANSFORMERS_OP.arguments, ["project", "limit"]);
        assert_eq!(
            EXPORT_OP.arguments,
            [
                "project",
                "transformer",
                "transformers",
                "force",
                "selection"
            ]
        );
    }

    #[test]
    fn request_writes_require_the_exact_project_locally() {
        let prepare = json!({"layout": {}, "expectedRevision": "", "selection": {}});
        assert!(
            require_request_project(&prepare, "printing.prepare")
                .expect_err("prepare project is required")
                .message()
                .contains("explicit project")
        );
        assert_eq!(
            require_request_project(&json!({"project":"huye"}), "printing.prepare").unwrap(),
            "huye"
        );
    }
}
