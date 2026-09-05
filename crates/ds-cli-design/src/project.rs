//! Thin adapters to the Rust Design workspace and existing report owner.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal},
};
use ds_design_workspace::{Error, Workspace};
use serde_json::{Value, json};
use std::path::Path;

const WORKSPACE: Arg =
    Arg::value("workspace", "<dir>", "Private offline Design workspace.").required();
const TRANSFORMER: Arg = Arg::value(
    "transformer",
    "<name>",
    "Exact ordinary LV transformer identity.",
)
.required();
const RUN_ID: Arg = Arg::value("run-id", "<id>", "Immutable run identity.").required();
const OUT: Arg = Arg::value("out", "<path>", "New output file; never overwritten.").required();
const OPERATION: Arg = Arg::value(
    "operation-id",
    "<id>",
    "Stable retry identity; reuse only for identical input.",
)
.required();
const EXPECTED: Arg =
    Arg::value("expected", "<sha256>", "Expected current local revision.").required();
const fn command(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    args: &'static [Arg],
    effect: Effect,
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        summary,
        purpose: "The Rust Design owner stores explicit WGS84 transformer/source snapshots, transactional edits, immutable processing inputs and a durable publication outbox. No Desktop, map, sign-in or network is required. Project attribution grants no remote authority. Pending publication is retained; the authenticated Design reconciliation transport is not yet exposed by this workspace. See the reference for exact snapshot and edit schemas.",
        chapter: Chapter::Design,
        effect,
        authority: Authority::None,
        execution: Execution::Sync,
        args,
        output: "Bounded local receipt, revision and pending-publication state. Full geometry and computation results are written only to explicit output files.",
        examples: &[],
        refusals: &[
            Refusal {
                code: "design_workspace_invalid",
                when: "an input, source digest, schema, identity, engine setting or bound is invalid",
                remedy: "use the documented snapshot/edit contract and inspect the named field",
            },
            Refusal {
                code: "design_workspace_conflict",
                when: "a local revision moved, output exists or an operation/run id names different inputs",
                remedy: "read current state, reconcile edits and choose a new identity only for new work",
            },
            Refusal {
                code: "design_workspace_io",
                when: "a local file, database or output cannot be read or durably written",
                remedy: "verify private readable inputs, writable storage and free disk space",
            },
            Refusal {
                code: "design_workspace_busy",
                when: "another compute worker owns the workspace",
                remedy: "inspect the existing run and let it finish or request cancellation",
            },
            Refusal {
                code: "design_workspace_worker",
                when: "the fixed background worker cannot start",
                remedy: "retry without --background or repair the installed executable",
            },
            Refusal {
                code: "design_workspace_report",
                when: "the report owner is missing or rejects the local report request",
                remedy: "inspect the nested reporter refusal, install the matching reporter and supply its required local references",
            },
        ],
        reference: Some("docs/reference/design-project.md"),
        availability: || Availability::Available,
    }
}
pub static INIT: Command = command(
    "design.project.init",
    &["design", "project", "init"],
    "Create a private offline Design workspace.",
    &[
        WORKSPACE,
        Arg::value("project", "<id>", "Local attribution, not authorization.").required(),
    ],
    Effect::LocalFileWrite,
);
pub static WRITE: Command = command(
    "design.project.write",
    &["design", "project", "write"],
    "Create or replace one complete transformer snapshot.",
    &[
        WORKSPACE,
        Arg::value(
            "input",
            "<file>",
            "ds.design.snapshot/v1 document, at most 64 MiB.",
        )
        .required(),
        OPERATION,
        Arg::value(
            "expected",
            "<sha256>",
            "Required when replacing an existing transformer.",
        ),
    ],
    Effect::LocalFileWrite,
);
pub static EDIT: Command = command(
    "design.project.edit",
    &["design", "project", "edit"],
    "Apply one atomic batch of version-fenced feature edits.",
    &[
        WORKSPACE,
        Arg::value(
            "input",
            "<file>",
            "ds.design.edit/v1 document, at most 5000 affected features.",
        )
        .required(),
    ],
    Effect::LocalFileWrite,
);
pub static READ: Command = command(
    "design.project.read",
    &["design", "project", "read"],
    "Export an exact current or historical transformer snapshot.",
    &[
        WORKSPACE,
        TRANSFORMER,
        Arg::value(
            "revision",
            "<sha256>",
            "Historical revision; omit for current.",
        ),
        OUT,
    ],
    Effect::LocalFileWrite,
);
pub static RESTORE: Command = command(
    "design.project.restore",
    &["design", "project", "restore"],
    "Restore a historical snapshot as a new local operation.",
    &[
        WORKSPACE,
        TRANSFORMER,
        Arg::value("revision", "<sha256>", "Historical revision to restore.").required(),
        EXPECTED,
        OPERATION,
    ],
    Effect::LocalFileWrite,
);
pub static STATUS: Command = command(
    "design.project.status",
    &["design", "project", "status"],
    "Inspect workspace counts or one run's ordered job states.",
    &[
        WORKSPACE,
        Arg::value("run-id", "<id>", "Inspect a captured run."),
    ],
    Effect::ReadOnly,
);
pub static PROCESS: Command = command(
    "design.project.process",
    &["design", "project", "process"],
    "Capture and process local transformers on native workers.",
    &[
        WORKSPACE,
        RUN_ID,
        Arg::repeated(
            "transformer",
            "<name>",
            "Select 1..32 exact transformers for capture.",
        ),
        Arg::value(
            "workers",
            "<count>",
            "CPU budget; defaults to available logical CPUs, capped by job count.",
        ),
        Arg::switch(
            "background",
            "Capture inputs, start a detached worker and return its PID.",
        ),
    ],
    Effect::LocalFileWrite,
);
pub static CANCEL: Command = command(
    "design.project.cancel",
    &["design", "project", "cancel"],
    "Cancel pending jobs; already running jobs may finish.",
    &[WORKSPACE, RUN_ID],
    Effect::LocalFileWrite,
);
pub static RESULT: Command = command(
    "design.project.result",
    &["design", "project", "result"],
    "Export a complete immutable transformer process result.",
    &[WORKSPACE, RUN_ID, TRANSFORMER, OUT],
    Effect::LocalFileWrite,
);
pub static OUTBOX: Command = command(
    "design.project.outbox",
    &["design", "project", "outbox"],
    "Inspect pending publication without contacting the network.",
    &[
        WORKSPACE,
        Arg::value("after", "<sequence>", "Exclusive cursor; default 0."),
        Arg::value("limit", "<count>", "Page size 1..100; default 20."),
    ],
    Effect::ReadOnly,
);
pub static REPORT: Command = command(
    "design.project.report",
    &["design", "project", "report"],
    "Produce offline reports and printable PDFs from a captured run.",
    &[
        WORKSPACE,
        RUN_ID,
        TRANSFORMER,
        Arg::value(
            "out-dir",
            "<dir>",
            "New directory for pinned inputs, artifacts and reporter receipt.",
        )
        .required(),
        Arg::value(
            "country",
            "<name>",
            "Explicit reporting country; Rwanda needs a verified local admin asset.",
        )
        .required(),
        Arg::repeated(
            "format",
            "<name>",
            "Explicit reporter formats, including pdf_a0 or pdf_a3.",
        ),
        Arg::value("admin-bounds", "<file>", "Local admin-bounds asset."),
        Arg::value(
            "admin-bounds-sha256",
            "<sha256>",
            "Expected admin asset digest.",
        ),
    ],
    Effect::LocalFileWrite,
);

pub fn map_error(e: Error) -> Failure {
    match e {
        Error::Invalid(m) => Failure::invalid("design_workspace_invalid", m),
        Error::Conflict(m) => Failure::conflict("design_workspace_conflict", m),
        Error::Busy(m) => Failure::conflict("design_workspace_busy", m),
        Error::Io(m) => Failure::failed("design_workspace_io", m),
    }
}
fn open(i: &Inputs) -> Result<Workspace, Failure> {
    Workspace::open(Path::new(i.require("workspace")?)).map_err(map_error)
}
fn input(i: &Inputs) -> Result<Vec<u8>, Failure> {
    ds_design_workspace::read_file(Path::new(i.require("input")?), 64 * 1024 * 1024)
        .map_err(map_error)
}
fn number(i: &Inputs, key: &str, default: usize) -> Result<usize, Failure> {
    i.value(key)
        .map(|v| {
            v.parse().map_err(|_| {
                Failure::invalid(
                    "design_workspace_invalid",
                    format!("--{key} must be a nonnegative integer"),
                )
            })
        })
        .unwrap_or(Ok(default))
}
pub fn init(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    Workspace::init(Path::new(i.require("workspace")?), i.require("project")?).map_err(map_error)
}
pub fn write(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    open(i)?
        .write(&input(i)?, i.value("expected"), i.require("operation-id")?)
        .map_err(map_error)
}
pub fn edit(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    open(i)?.edit(&input(i)?).map_err(map_error)
}
pub fn read(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    open(i)?
        .export_snapshot(
            i.require("transformer")?,
            i.value("revision"),
            Path::new(i.require("out")?),
        )
        .map_err(map_error)
}
pub fn restore(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    open(i)?
        .restore(
            i.require("transformer")?,
            i.require("revision")?,
            i.require("expected")?,
            i.require("operation-id")?,
        )
        .map_err(map_error)
}
pub fn status(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let w = open(i)?;
    match i.value("run-id") {
        Some(id) => w.run_status(id),
        None => w.status(),
    }
    .map_err(map_error)
}
pub fn cancel(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    open(i)?.cancel(i.require("run-id")?).map_err(map_error)
}
pub fn result(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    open(i)?
        .export_result(
            i.require("run-id")?,
            i.require("transformer")?,
            Path::new(i.require("out")?),
        )
        .map_err(map_error)
}
pub fn outbox(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let after = i64::try_from(number(i, "after", 0)?).map_err(|_| {
        Failure::invalid(
            "design_workspace_invalid",
            "cursor exceeds signed integer bound",
        )
    })?;
    open(i)?
        .outbox(after, number(i, "limit", 20)?)
        .map_err(map_error)
}
pub fn process(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let mut w = open(i)?;
    let id = i.require("run-id")?;
    let available = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1);
    let workers = number(i, "workers", available)?;
    if workers == 0 || workers > available {
        return Err(Failure::invalid(
            "design_workspace_invalid",
            format!("workers must be 1..{available}"),
        ));
    }
    // An empty selection resumes an already captured run; it never means all.
    if i.repeated("transformer").is_empty() {
        w.run_status(id).map_err(map_error)?;
    } else {
        w.prepare_run(id, i.repeated("transformer"))
            .map_err(map_error)?;
    }
    if i.switch("background") {
        let root = std::fs::canonicalize(i.require("workspace")?)
            .map_err(|e| map_error(Error::Io(e.to_string())))?;
        return Ok(
            json!({"run_id":id,"worker_pid":ds_cli_exec::start_design_project_process(&root,id,workers)?,"state":"launched","progress":"ds design project status --workspace <dir> --run-id <id>"}),
        );
    }
    serde_json::to_value(w.process(id, workers).map_err(map_error)?)
        .map_err(|e| map_error(Error::Invalid(e.to_string())))
}
pub fn report(i: &Inputs, c: &Context) -> Result<Value, Failure> {
    let mut w = open(i)?;
    let root = Path::new(i.require("out-dir")?);
    if i.repeated("format").is_empty() {
        return Err(Failure::invalid(
            "design_workspace_invalid",
            "name at least one --format, including pdf_a0 or pdf_a3 for printing",
        ));
    }
    w.report_inputs(i.require("run-id")?, i.require("transformer")?, root)
        .map_err(map_error)?;
    let mut args = vec![
        "--task".into(),
        "transformer".into(),
        "--input-shape".into(),
        "plain_local".into(),
        "--transformer".into(),
        i.require("transformer")?.into(),
        "--transformer-document".into(),
        root.join("transformer.json").to_string_lossy().into_owned(),
        "--network-config".into(),
        root.join("network-config.json")
            .to_string_lossy()
            .into_owned(),
        "--out-dir".into(),
        root.join("artifacts").to_string_lossy().into_owned(),
        "--result".into(),
        root.join("report-result.json")
            .to_string_lossy()
            .into_owned(),
        "--country".into(),
        i.require("country")?.into(),
    ];
    for format in i.repeated("format") {
        args.extend(["--format".into(), format.clone()]);
    }
    for key in ["admin-bounds", "admin-bounds-sha256"] {
        if let Some(value) = i.value(key) {
            args.extend([format!("--{key}"), value.into()]);
        }
    }
    let inputs = ds_cli_contract::args::parse(&ds_cli_report::export::COMMAND, &args)
        .map_err(report_error)?;
    let report = ds_cli_report::export::run(&inputs, c).map_err(report_error)?;
    if report["status"] != "completed" {
        return Ok(json!({"report":report,"publication":"not_queued_partial"}));
    }
    let retained = w
        .retain_report(
            i.require("run-id")?,
            i.require("transformer")?,
            root,
            &report,
        )
        .map_err(map_error)?;
    Ok(json!({"report":report,"delivery":retained}))
}
fn report_error(e: Failure) -> Failure {
    Failure::failed("design_workspace_report", e.to_string())
        .detail(json!({"code":e.code(),"detail":e.detail_value(),"remedy":e.remedy_text()}))
}
pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}

pub static SOURCES: Command = command(
    "design.project.resolve-sources",
    &["design", "project", "resolve-sources"],
    "Resolve project and user source selections through the shared kernel.",
    &[
        Arg::value(
            "input",
            "<file>",
            "ds.design.source-resolution/v1 request, at most 64 KiB.",
        )
        .required(),
        OUT,
    ],
    Effect::LocalFileWrite,
);
pub fn sources(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let bytes = ds_design_workspace::read_file(Path::new(i.require("input")?), 64 * 1024)
        .map_err(map_error)?;
    let result = ds_command_kernel::design::resolve_sources(&bytes)
        .map_err(|e| map_error(Error::Invalid(e)))?;
    let bytes =
        serde_json::to_vec(&result).map_err(|e| map_error(Error::Invalid(e.to_string())))?;
    ds_design_workspace::write_new(Path::new(i.require("out")?), &bytes).map_err(map_error)?;
    Ok(
        json!({"out":i.require("out")?,"scope":result["scope"],"sources":result["addresses"].as_array().map(Vec::len)}),
    )
}
