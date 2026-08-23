//! Closed read/import/status operations over the paired desktop Solar workflow.

use std::path::PathBuf;
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::paired;

const READ_TIMEOUT: Duration = Duration::from_secs(30);
const IMPORT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const RESULTS_READ_OPERATION: &str = "solar.results.read";
const SYNC_STATUS_OPERATION: &str = "solar.sync.status";
const PORTFOLIO_LIST_OPERATION: &str = "solar.portfolio.list";
const FINAL_IMPORT_OPERATION: &str = "solar.final.import";
const RESULT_SECTIONS: &[&str] = &[
    "config",
    "costs",
    "finance",
    "identity",
    "inputs",
    "plant",
    "site",
    "summary",
    "templates",
];

const DESCRIPTOR_ARG: Arg = Arg::value(
    "desktop-descriptor",
    "<path>",
    "Use this bridge descriptor instead of discovering one.",
);

pub static RESULTS_READ_COMMAND: Command = Command {
    id: "solar.results.read",
    path: &["solar", "results", "read"],
    contract: 1,
    summary: "Read one dashboard section from a native Solar result.",
    purpose: "Reads a bounded semantic projection from the canonical report_input receipt used by the Solar dashboards. It does not read result.json, a workspace path, or a cloud-specific cache.",
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("run-id", "<id>", "Completed native Solar batch id.").required(),
        Arg::value("city", "<id>", "Canonical city context in that batch.").required(),
        Arg::value("section", "<name>", "Dashboard result section to read.")
            .required()
            .choices(RESULT_SECTIONS),
        Arg::repeated("path", "<field>", "Semantic child key. Repeat to descend."),
        DESCRIPTOR_ARG,
    ],
    output: "A bounded projection, completeness flag, canonical per-city result id, batch id and city context.",
    examples: &[Example {
        command: "ds solar results read --run-id run-123 --city kigali --section finance --path financial_summary --output json",
        note: "Reads the same canonical result section the Finance dashboard uses.",
        runnable: false,
    }],
    refusals: paired::REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub static SYNC_STATUS_COMMAND: Command = Command {
    id: "solar.sync.status",
    path: &["solar", "sync", "status"],
    contract: 1,
    summary: "Read Solar publication state from the desktop Sync Center.",
    purpose: "Projects the active project's durable Solar outbox rows and their pending, syncing, conflict, failed, or synced states. An optional batch run id narrows the view without starting or retrying publication.",
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("run-id", "<id>", "Optional native batch id to filter."),
        DESCRIPTOR_ARG,
    ],
    output: "Matching calculation/report publication rows and counts by state.",
    examples: &[Example {
        command: "ds solar sync status --run-id run-123 --output json",
        note: "Answers whether every city result and report has converged.",
        runnable: false,
    }],
    refusals: paired::REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub static PORTFOLIO_LIST_COMMAND: Command = Command {
    id: "solar.portfolio.list",
    path: &["solar", "portfolio", "list"],
    contract: 1,
    summary: "List Solar portfolios and their city membership.",
    purpose: "Reads the active project's governed portfolio catalog through the desktop, refreshing it once when online and retaining the same offline cache used by the Pipeline page.",
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[DESCRIPTOR_ARG],
    output: "Portfolio ids, names, display names, city counts and exact member city ids.",
    examples: &[Example {
        command: "ds solar portfolio list --output json",
        note: "Use an id from this result with `solar run start --portfolio`.",
        runnable: false,
    }],
    refusals: paired::REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub static FINAL_IMPORT_COMMAND: Command = Command {
    id: "solar.final.import",
    path: &["solar", "final", "import"],
    contract: 1,
    summary: "Import an externally interpreted Markdown final.",
    purpose: "Hands one explicit Markdown source path to the paired native shell, which validates and stores it in the selected run/city final slot, optionally renders DOCX with the installed Pandoc, and queues the final report variant for governed publication. The app calls no model.",
    effect: Effect::ArtifactWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("run-id", "<id>", "Completed native Solar batch id.").required(),
        Arg::value("city", "<id>", "Canonical city context in that batch.").required(),
        Arg::value(
            "file",
            "<markdown>",
            "UTF-8 Markdown final to import (maximum 2 MiB).",
        )
        .required(),
        DESCRIPTOR_ARG,
    ],
    output: "An imported receipt naming the run and city. The final remains visible locally while its publication drains lazily.",
    examples: &[Example {
        command: "ds solar final import --run-id run-123 --city kigali --file ./kigali-final.md --yes --output json",
        note: "The source is interpreted externally; DS GridDesign only validates, stores, renders and queues it.",
        runnable: false,
    }],
    refusals: paired::REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub fn results_read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = run_city(inputs)?;
    arguments.insert("section".into(), json!(inputs.require("section")?));
    if !inputs.repeated("path").is_empty() {
        arguments.insert("path".into(), json!(inputs.repeated("path")));
    }
    paired::require_run_id(
        paired::invoke(
            inputs,
            RESULTS_READ_OPERATION,
            Value::Object(arguments),
            READ_TIMEOUT,
        )?,
        RESULTS_READ_OPERATION,
    )
}

pub fn sync_status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = inputs
        .value("run-id")
        .map_or_else(|| json!({}), |run_id| json!({ "run_id": run_id }));
    paired::invoke(inputs, SYNC_STATUS_OPERATION, arguments, READ_TIMEOUT)
}

pub fn portfolio_list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    paired::invoke(inputs, PORTFOLIO_LIST_OPERATION, json!({}), READ_TIMEOUT)
}

pub fn final_import(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = run_city(inputs)?;
    let source = absolute_path(inputs.require("file")?);
    arguments.insert("source_path".into(), json!(source));
    paired::require_run_id(
        paired::invoke(
            inputs,
            FINAL_IMPORT_OPERATION,
            Value::Object(arguments),
            IMPORT_TIMEOUT,
        )?,
        FINAL_IMPORT_OPERATION,
    )
}

fn run_city(inputs: &Inputs) -> Result<Map<String, Value>, Failure> {
    let mut arguments = Map::new();
    arguments.insert("run_id".into(), json!(inputs.require("run-id")?));
    arguments.insert("context".into(), json!(inputs.require("city")?));
    Ok(arguments)
}

fn absolute_path(raw: &str) -> String {
    let path = PathBuf::from(raw);
    let resolved = if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    resolved.to_string_lossy().into_owned()
}

pub fn render(data: &Value) -> String {
    let mut out = String::new();
    if let Some(status) = data["status"].as_str() {
        out.push_str(&format!("status  {status}\n"));
    }
    if let Some(run_id) = data["run_id"].as_str() {
        out.push_str(&format!("run     {run_id}\n"));
    }
    if let Some(portfolios) = data["portfolios"].as_array() {
        for portfolio in portfolios {
            out.push_str(&format!(
                "{}  {} cities\n",
                portfolio["id"].as_str().unwrap_or("?"),
                portfolio["city_count"].as_u64().unwrap_or(0),
            ));
        }
    }
    out
}
