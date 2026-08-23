//! Paired Solar run lifecycle commands.
//!
//! The product's local computation lifecycle is named, closed, and owned by
//! the paired desktop: start returns a durable run id; subsequent commands
//! use that id to observe, read, or cancel the same local run. The commands
//! below do not reimplement calculation or reach into IndexedDB.

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::paired;

const START_OPERATION: &str = "solar.run.start";
const PROGRESS_OPERATION: &str = "solar.run.progress";
const RESULT_OPERATION: &str = "solar.run.result";
const CANCEL_OPERATION: &str = "solar.run.cancel";
const READ_OPERATION: &str = "solar.result.read";

/// Starting a run creates only a receipt synchronously. The compute continues
/// in the paired local application and is observed through its run id.
const START_TIMEOUT: Duration = Duration::from_secs(30);
const READ_TIMEOUT: Duration = Duration::from_secs(30);

pub static START_COMMAND: Command = Command {
    id: "solar.run.start",
    path: &["solar", "run", "start"],
    contract: 1,
    summary: "Start a paired local Solar run for selected city contexts.",
    purpose: "Starts the paired application's local native Solar lifecycle after city inputs have been prepared. It returns a run id immediately; use `solar run progress` and `solar run result` to observe that run. The desktop owns the selected project, cached inputs and output workspace, so this command never scans IndexedDB or passes a cache path, URL or credential.",
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Job,
    args: &[
        Arg::repeated(
            "city",
            "<id>",
            "Canonical prepared city context id. Repeat to run several cities.",
        ),
        Arg::value(
            "portfolio",
            "<id>",
            "Run the exact cached/governed membership of one Solar portfolio instead of --city.",
        ),
        Arg::switch("no-charts", "Do not render chart artifacts for this run."),
        Arg::value(
            "concurrency",
            "<n>",
            "Cities to calculate concurrently, from 1 through 32.",
        ),
        Arg::switch("serial", "Force strictly serial calculation."),
        Arg::value(
            "desktop-descriptor",
            "<path>",
            "Use this bridge descriptor instead of discovering one.",
        ),
    ],
    output: "A paired local run receipt with `run_id`, selected contexts and placement. The calculation continues after this command returns; read it with the lifecycle commands rather than treating a launch receipt as a completed result.",
    examples: &[
        Example {
            command: "ds solar run start --city kigali --output json",
            note: "Starts one prepared city and returns its run id.",
            runnable: false,
        },
        Example {
            command: "ds solar run start --city kigali --city butare --concurrency 2 --no-charts --output json",
            note: "Starts two prepared contexts with bounded local concurrency.",
            runnable: false,
        },
    ],
    refusals: paired::START_REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub static PROGRESS_COMMAND: Command = Command {
    id: "solar.run.progress",
    path: &["solar", "run", "progress"],
    contract: 1,
    summary: "Read progress for one paired local Solar run.",
    purpose: "Reads the paired application's bounded progress receipt for a run returned by `solar run start`. It performs no calculation, writes no result, and does not inspect browser storage; the desktop remains the owner of local run state.",
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("run-id", "<id>", "Run id returned by solar run start.").required(),
        Arg::value(
            "desktop-descriptor",
            "<path>",
            "Use this bridge descriptor instead of discovering one.",
        ),
    ],
    output: "The paired application's bounded progress receipt for the requested run.",
    examples: &[Example {
        command: "ds solar run progress --run-id solar-run-123 --output json",
        note: "Reads a launch receipt's current state.",
        runnable: false,
    }],
    refusals: paired::REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub static RESULT_COMMAND: Command = Command {
    id: "solar.run.result",
    path: &["solar", "run", "result"],
    contract: 1,
    summary: "Read the result receipt for one paired local Solar run.",
    purpose: "Reads the paired application's public result receipt for a run returned by `solar run start`. The receipt says whether the local run finished and where to continue; use `solar result read` for one bounded city/result projection.",
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("run-id", "<id>", "Run id returned by solar run start.").required(),
        Arg::value(
            "desktop-descriptor",
            "<path>",
            "Use this bridge descriptor instead of discovering one.",
        ),
    ],
    output: "The paired application's bounded public result receipt for the requested run.",
    examples: &[Example {
        command: "ds solar run result --run-id solar-run-123 --output json",
        note: "Reads a completed run's public receipt.",
        runnable: false,
    }],
    refusals: paired::REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub static CANCEL_COMMAND: Command = Command {
    id: "solar.run.cancel",
    path: &["solar", "run", "cancel"],
    contract: 1,
    summary: "Request cancellation of one paired local Solar run.",
    purpose: "Requests that the paired application cancel a local Solar run. Cancellation is an application-visible lifecycle transition, not a filesystem shortcut; the desktop reports the resulting receipt and owns safe cleanup of its workspace.",
    effect: Effect::LocalUi,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("run-id", "<id>", "Run id returned by solar run start.").required(),
        Arg::value(
            "desktop-descriptor",
            "<path>",
            "Use this bridge descriptor instead of discovering one.",
        ),
    ],
    output: "The paired application's cancellation receipt for the requested run.",
    examples: &[Example {
        command: "ds solar run cancel --run-id solar-run-123 --output json",
        note: "Requests cancellation and returns the desktop's receipt.",
        runnable: false,
    }],
    refusals: paired::REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub static READ_COMMAND: Command = Command {
    id: "solar.result.read",
    path: &["solar", "result", "read"],
    contract: 1,
    summary: "Read one bounded city result from a paired local Solar run.",
    purpose: "Reads one selected city's result projection from the paired application's local Solar run store. Optional paths narrow the projection; they are semantic paths inside the result document, never filesystem paths. The application bounds the reply before it crosses the bridge.",
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("run-id", "<id>", "Run id returned by solar run start.").required(),
        Arg::value("city", "<id>", "Canonical city context id to read.").required(),
        Arg::repeated(
            "path",
            "<field>",
            "Semantic result field to include. Repeat for several fields.",
        ),
        Arg::value(
            "desktop-descriptor",
            "<path>",
            "Use this bridge descriptor instead of discovering one.",
        ),
    ],
    output: "A bounded result projection with its run id, city context and digest.",
    examples: &[Example {
        command: "ds solar result read --run-id solar-run-123 --city kigali --path annual --output json",
        note: "Reads one selected projection from one city's result.",
        runnable: false,
    }],
    refusals: paired::REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub fn start(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    if !inputs.repeated("city").is_empty() {
        arguments.insert("contexts".into(), json!(inputs.repeated("city")));
    }
    if let Some(portfolio) = inputs.value("portfolio") {
        arguments.insert("portfolio".into(), json!(portfolio));
    }
    if inputs.switch("no-charts") {
        arguments.insert("render_charts".into(), Value::Bool(false));
    }
    if let Some(concurrency) = inputs.value("concurrency") {
        let concurrency = parse_concurrency(concurrency)?;
        arguments.insert("concurrency".into(), json!(concurrency));
    }
    if inputs.switch("serial") {
        arguments.insert("serial".into(), Value::Bool(true));
    }
    paired::require_run_id(
        paired::invoke(
            inputs,
            START_OPERATION,
            Value::Object(arguments),
            START_TIMEOUT,
        )?,
        START_OPERATION,
    )
}

pub fn progress(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    run_receipt(inputs, PROGRESS_OPERATION)
}

pub fn result(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    run_receipt(inputs, RESULT_OPERATION)
}

pub fn cancel(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    run_receipt(inputs, CANCEL_OPERATION)
}

pub fn read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert(
        "run_id".into(),
        Value::String(inputs.require("run-id")?.to_string()),
    );
    arguments.insert(
        "context".into(),
        Value::String(inputs.require("city")?.to_string()),
    );
    if !inputs.repeated("path").is_empty() {
        arguments.insert("path".into(), json!(inputs.repeated("path")));
    }
    paired::require_run_id(
        paired::invoke(
            inputs,
            READ_OPERATION,
            Value::Object(arguments),
            READ_TIMEOUT,
        )?,
        READ_OPERATION,
    )
}

fn run_receipt(inputs: &Inputs, operation: &'static str) -> Result<Value, Failure> {
    paired::require_run_id(
        paired::invoke(
            inputs,
            operation,
            json!({ "run_id": inputs.require("run-id")? }),
            READ_TIMEOUT,
        )?,
        operation,
    )
}

fn parse_concurrency(value: &str) -> Result<u8, Failure> {
    value
        .parse::<u8>()
        .ok()
        .filter(|value| (1..=32).contains(value))
        .ok_or_else(|| {
            Failure::invalid(
                "invalid_concurrency",
                "--concurrency must be a whole number from 1 through 32",
            )
            .remedy("pass an integer from 1 through 32, or omit the flag")
        })
}

pub fn render_start(data: &Value) -> String {
    let mut out = render_receipt(data);
    if let Some(contexts) = data["contexts"].as_array() {
        let contexts = contexts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("contexts  {contexts}\n"));
    }
    if let Some(placement) = data["placement"].as_str() {
        out.push_str(&format!("placement {placement}\n"));
    }
    out
}

pub fn render_receipt(data: &Value) -> String {
    let mut out = String::new();
    if let Some(status) = data["status"].as_str() {
        out.push_str(&format!("status    {status}\n"));
    }
    if let Some(run_id) = data["run_id"].as_str() {
        out.push_str(&format!("run       {run_id}\n"));
    }
    if let Some(context) = data["context"].as_str() {
        out.push_str(&format!("context   {context}\n"));
    }
    if let Some(digest) = data["result_digest"].as_str() {
        out.push_str(&format!("digest    {digest}\n"));
    }
    if let Some(note) = data["note"].as_str() {
        out.push_str(&format!("{note}\n"));
    }
    out
}
