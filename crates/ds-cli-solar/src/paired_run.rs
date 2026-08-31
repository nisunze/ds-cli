//! Paired Solar run lifecycle commands.
//!
//! The product's local computation lifecycle is named, closed, and owned by
//! the paired desktop: start returns a durable run id; subsequent commands
//! use that id to observe, read, or cancel the same local run. The commands
//! below do not reimplement calculation or reach into IndexedDB.

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
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

/// The wire key the paired session carries a portfolio's failed governed
/// publication under, inside the result receipt's `portfolio` projection.
///
/// It is the snake_case spelling of the application receipt's own
/// `publicationError`, hand-copied like every other field of that projection
/// (`sourceRunId` → `source_run_id`, `inputDigest` → `input_digest`) and pinned
/// against the owner by `bridge_parity.rs`.
pub const PUBLICATION_ERROR_KEY: &str = "publication_error";

/// The application's own bound on the reason it records.
///
/// `runExplicitNativePortfolio` slices a queue failure to 256 characters before
/// it reaches the receipt, so a longer value does not mean a longer warning —
/// it means this `ds` and that application no longer agree on the field.
pub const PUBLICATION_ERROR_CHARS: usize = 256;

/// The application's own token for an intent that was never queued, as its
/// `solar.final.import` receipt already spells it.
pub const PUBLICATION_NOT_QUEUED: &str = "not_queued";

/// Nothing else re-queues it. The intent never reached the outbox, so there is
/// no Sync Center row to retry and no state a later poll will change.
const PUBLICATION_REMEDY: &str = "the sealed local result is unaffected; clear the reported cause in DS GridDesign and run the same portfolio again to publish the governed copy";

pub static START_COMMAND: Command = Command {
    id: "solar.run.start",
    path: &["solar", "run", "start"],
    contract: 4,
    summary: "Start an explicit paired city or Solar portfolio run.",
    purpose: "Starts the paired application's local native Solar lifecycle after city inputs have been prepared. A city run names one or more prepared contexts. A portfolio run names one governed portfolio, pins the exact ordered membership returned by portfolio list, and chooses only how representative graphs use that membership: first member, round-robin, or one exact member. Currency, project horizon and discount rate remain governed prepared-input facts; language and report intent belong to later report generation rather than calculation launch. It returns a run id immediately, while the desktop retains ownership of the selected project, cached inputs and output workspace.",
    chapter: Chapter::Solar,
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
            "Exact governed Solar portfolio id. Cannot be combined with --city.",
        ),
        Arg::value(
            "membership-revision",
            "<sha256:digest>",
            "Portfolio-only exact membership revision returned by `solar portfolio list`; required with --portfolio.",
        ),
        Arg::value(
            "graph-strategy",
            "<first|round-robin|city:id>",
            "Portfolio-only representative graph strategy; required with --portfolio. Use `city:<exact-member-id>` to pin one member.",
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
        Example {
            command: "ds solar run start --portfolio aderm --membership-revision sha256:<digest> --graph-strategy round-robin --output json",
            note: "Starts the exact listed membership and rotates representative graphs across its ordered members.",
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
    chapter: Chapter::Solar,
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
    contract: 2,
    summary: "Read the result receipt for one paired local Solar run.",
    purpose: "Reads the paired application's public result receipt for a run returned by `solar run start`. The receipt says whether the local run finished and where to continue; use `solar result read` for one bounded city/result projection.",
    chapter: Chapter::Solar,
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
    output: "The paired application's bounded public result receipt for the requested run. A portfolio run that sealed its result but could not queue its governed publication stays `succeeded` and carries `publication` with the state, the application's reason and the remedy; that intent never reached the outbox, so `solar sync status` has no row for it. No `publication` means the application stated nothing about one.",
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
    chapter: Chapter::Solar,
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
    chapter: Chapter::Solar,
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
    let cities = inputs.repeated("city");
    let portfolio = inputs.value("portfolio");
    match (!cities.is_empty(), portfolio.is_some()) {
        (true, true) | (false, false) => {
            return Err(Failure::invalid(
                "invalid_run_selection",
                "solar run start requires exactly one of --city or --portfolio",
            )
            .remedy("pass one or more --city values, or pass one --portfolio with its explicit portfolio inputs"));
        }
        _ => {}
    }

    let mut arguments = Map::new();
    if let Some(portfolio) = portfolio {
        let portfolio = require_exact_value(portfolio, "portfolio", "invalid_run_selection")?;
        let membership_revision =
            parse_membership_revision(portfolio_value(inputs, "membership-revision")?)?;
        let graph_strategy = parse_graph_strategy(portfolio_value(inputs, "graph-strategy")?)?;

        arguments.insert("portfolio".into(), json!(portfolio));
        arguments.insert("membership_revision".into(), json!(membership_revision));
        arguments.insert("graph_strategy".into(), json!(graph_strategy));
    } else {
        reject_portfolio_inputs_for_city(inputs)?;
        arguments.insert("contexts".into(), json!(cities));
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
    publication_receipt(run_receipt(inputs, RESULT_OPERATION)?)
}

/// Keep a sealed portfolio calculation a success while naming a governed
/// publication that did not happen.
///
/// The application publishes the aggregate as a handoff *after* the local
/// commit: the result is on disk and its receipt is written before the intent
/// is queued, so a queue failure is a sync-lane fact that never undoes the run.
/// It records that fact on the portfolio receipt and nowhere else — an intent
/// that never reached the outbox has no Sync Center row, so `solar sync status`
/// cannot report it either. Reading it here is what lets the deployed `ds` see
/// what the application's own screen sees.
///
/// So the status is untouched and the fact is lifted out of the pass-through
/// projection into one named block, which is the only place `ds` describes it.
/// A receipt written before the application recorded the field carries none,
/// and gets no block: `ds` says nothing about a publication the application
/// made no statement about.
fn publication_receipt(mut receipt: Value) -> Result<Value, Failure> {
    if receipt["status"].as_str() != Some("succeeded") {
        return Ok(receipt);
    }
    let reported = receipt["portfolio"]
        .as_object_mut()
        .and_then(|portfolio| portfolio.remove(PUBLICATION_ERROR_KEY));
    let Some(reported) = reported else {
        return Ok(receipt);
    };
    let detail = reported
        .as_str()
        .map(str::trim)
        .filter(|detail| !detail.is_empty() && detail.chars().count() <= PUBLICATION_ERROR_CHARS)
        .ok_or_else(|| {
            Failure::unavailable(
                "desktop_contract_mismatch",
                format!(
                    "the paired session reported a portfolio publication failure outside its \
                     bounded {PUBLICATION_ERROR_CHARS}-character reason"
                ),
            )
            .remedy("update DS GridDesign and ds to matching releases")
        })?
        .to_string();
    receipt["publication"] = json!({
        "state": PUBLICATION_NOT_QUEUED,
        "detail": detail,
        "remedy": PUBLICATION_REMEDY,
    });
    Ok(receipt)
}

pub fn cancel(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    run_receipt(inputs, CANCEL_OPERATION)
}

pub fn read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let run_id = inputs.require("run-id")?;
    let city = inputs.require("city")?;
    let mut arguments = Map::new();
    arguments.insert("run_id".into(), Value::String(run_id.to_string()));
    arguments.insert("context".into(), Value::String(city.to_string()));
    if !inputs.repeated("path").is_empty() {
        arguments.insert("path".into(), json!(inputs.repeated("path")));
    }
    paired::require_exact_identity(
        paired::invoke(
            inputs,
            READ_OPERATION,
            Value::Object(arguments),
            READ_TIMEOUT,
        )?,
        READ_OPERATION,
        run_id,
        Some(city),
    )
}

fn run_receipt(inputs: &Inputs, operation: &'static str) -> Result<Value, Failure> {
    let run_id = inputs.require("run-id")?;
    paired::require_exact_identity(
        paired::invoke(inputs, operation, json!({ "run_id": run_id }), READ_TIMEOUT)?,
        operation,
        run_id,
        None,
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

const PORTFOLIO_ONLY_VALUE_INPUTS: &[&str] = &["membership-revision", "graph-strategy"];

fn reject_portfolio_inputs_for_city(inputs: &Inputs) -> Result<(), Failure> {
    let supplied = PORTFOLIO_ONLY_VALUE_INPUTS
        .iter()
        .copied()
        .filter(|name| inputs.value(name).is_some())
        .collect::<Vec<_>>();
    if supplied.is_empty() {
        return Ok(());
    }
    Err(Failure::invalid(
        "portfolio_only_input",
        "portfolio membership and graph strategy cannot be used with --city",
    )
    .remedy("remove the portfolio-only flags, or replace --city with one --portfolio")
    .detail(json!({ "inputs": supplied })))
}

fn portfolio_value<'a>(inputs: &'a Inputs, name: &str) -> Result<&'a str, Failure> {
    inputs
        .value(name)
        .ok_or_else(|| missing_portfolio_input(name))
}

fn missing_portfolio_input(name: &str) -> Failure {
    Failure::invalid(
        "missing_portfolio_input",
        format!("--{name} is required with --portfolio"),
    )
    .remedy(format!(
        "pass --{name} with an explicit value; use command help for its contract"
    ))
    .detail(json!({ "input": name }))
}

fn require_exact_value<'a>(
    value: &'a str,
    name: &str,
    code: &'static str,
) -> Result<&'a str, Failure> {
    if !value.is_empty() && value.trim() == value && value.len() <= 128 {
        return Ok(value);
    }
    Err(Failure::invalid(
        code,
        format!("--{name} must be a non-blank exact identifier of at most 128 bytes"),
    )
    .remedy(format!(
        "pass the exact {name} identifier without surrounding whitespace"
    )))
}

fn parse_membership_revision(value: &str) -> Result<&str, Failure> {
    if value.len() == 71
        && value.starts_with("sha256:")
        && value[7..]
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Ok(value);
    }
    Err(Failure::invalid(
        "invalid_membership_revision",
        "--membership-revision must be a lowercase sha256 digest returned by solar portfolio list",
    )
    .remedy("list portfolios again and pass the selected row's exact membership_revision"))
}

fn parse_graph_strategy(value: &str) -> Result<&str, Failure> {
    if matches!(value, "first" | "round-robin") {
        return Ok(if value == "round-robin" {
            "round_robin"
        } else {
            value
        });
    }
    if let Some(city_id) = value.strip_prefix("city:")
        && !city_id.is_empty()
        && city_id.trim() == city_id
        && city_id.len() <= 128
    {
        return Ok(value);
    }
    Err(Failure::invalid(
        "invalid_graph_strategy",
        "--graph-strategy must be first, round-robin, or city:<exact-member-id>",
    )
    .remedy("choose first, round-robin, or prefix one exact portfolio member id with city:"))
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
    // A succeeded run whose governed publication did not happen. Printing it
    // beside the status is the point: text callers must not have to read JSON
    // to learn that only the local copy exists.
    if let Some(state) = data["publication"]["state"].as_str() {
        out.push_str(&format!("publish   {state}\n"));
        if let Some(detail) = data["publication"]["detail"].as_str() {
            out.push_str(&format!("          {detail}\n"));
        }
        if let Some(remedy) = data["publication"]["remedy"].as_str() {
            out.push_str(&format!("remedy    {remedy}\n"));
        }
    }
    if let Some(note) = data["note"].as_str() {
        out.push_str(&format!("{note}\n"));
    }
    out
}
