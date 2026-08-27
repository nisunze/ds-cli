//! Paired Solar run lifecycle commands.
//!
//! The product's local computation lifecycle is named, closed, and owned by
//! the paired desktop: start returns a durable run id; subsequent commands
//! use that id to observe, read, or cancel the same local run. The commands
//! below do not reimplement calculation or reach into IndexedDB.

use std::{collections::BTreeSet, time::Duration};

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

pub static START_COMMAND: Command = Command {
    id: "solar.run.start",
    path: &["solar", "run", "start"],
    contract: 3,
    summary: "Start an explicit paired city or Solar portfolio run.",
    purpose: "Starts the paired application's local native Solar lifecycle after city inputs have been prepared. A city run names one or more prepared contexts. A portfolio run names one governed portfolio, pins the membership revision returned by portfolio list, and explicitly supplies its currency, project years, discount rate, representative member, language and report intents; none are inferred by the CLI. It returns a run id immediately, while the desktop retains ownership of the selected project, cached inputs and output workspace.",
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
            "currency",
            "<ISO>",
            "Portfolio-only three-letter uppercase ASCII currency; required with --portfolio.",
        ),
        Arg::value(
            "project-years",
            "<n>",
            "Portfolio-only project horizon from 1 through 100; required with --portfolio.",
        ),
        Arg::value(
            "discount-rate",
            "<rate>",
            "Portfolio-only finite discount rate from 0 inclusive to 1 exclusive; required with --portfolio.",
        ),
        Arg::value(
            "representative-city",
            "<id>",
            "Portfolio-only representative member id; required with --portfolio.",
        ),
        Arg::value(
            "language",
            "<fr|en>",
            "Portfolio-only report language; required with --portfolio.",
        )
        .choices(&["fr", "en"]),
        Arg::repeated(
            "report",
            "<intent>",
            "Portfolio-only report intent. Repeat at least once with --portfolio.",
        )
        .choices(&["apd", "network", "plant", "financial"]),
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
            command: "ds solar run start --portfolio aderm --membership-revision sha256:<digest> --currency XAF --project-years 25 --discount-rate 0.08 --representative-city kigali --language fr --report apd --report financial --output json",
            note: "Starts the exact listed membership with no inferred financial or report assumptions.",
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
    contract: 1,
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
        let currency = portfolio_value(inputs, "currency")?;
        let currency = parse_currency(currency)?;
        let project_years = parse_project_years(portfolio_value(inputs, "project-years")?)?;
        let discount_rate = parse_discount_rate(portfolio_value(inputs, "discount-rate")?)?;
        let representative_city = require_exact_value(
            portfolio_value(inputs, "representative-city")?,
            "representative-city",
            "invalid_representative_city",
        )?;
        let language = portfolio_value(inputs, "language")?;
        let report_intents = inputs.repeated("report");
        if report_intents.is_empty() {
            return Err(missing_portfolio_input("report"));
        }
        if let Some(duplicate) = first_duplicate(report_intents) {
            return Err(Failure::invalid(
                "duplicate_report_intent",
                format!("portfolio report intent `{duplicate}` was requested more than once"),
            )
            .remedy("pass each --report intent at most once"));
        }

        arguments.insert("portfolio".into(), json!(portfolio));
        arguments.insert("membership_revision".into(), json!(membership_revision));
        arguments.insert("currency".into(), json!(currency));
        arguments.insert("project_years".into(), json!(project_years));
        arguments.insert("discount_rate".into(), json!(discount_rate));
        arguments.insert("representative_city".into(), json!(representative_city));
        arguments.insert("language".into(), json!(language));
        arguments.insert("report_intents".into(), json!(report_intents));
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
    run_receipt(inputs, RESULT_OPERATION)
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

const PORTFOLIO_ONLY_VALUE_INPUTS: &[&str] = &[
    "membership-revision",
    "currency",
    "project-years",
    "discount-rate",
    "representative-city",
    "language",
];

fn reject_portfolio_inputs_for_city(inputs: &Inputs) -> Result<(), Failure> {
    let mut supplied = PORTFOLIO_ONLY_VALUE_INPUTS
        .iter()
        .copied()
        .filter(|name| inputs.value(name).is_some())
        .collect::<Vec<_>>();
    if !inputs.repeated("report").is_empty() {
        supplied.push("report");
    }
    if supplied.is_empty() {
        return Ok(());
    }
    Err(Failure::invalid(
        "portfolio_only_input",
        "portfolio assumptions and report intents cannot be used with --city",
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

fn parse_currency(value: &str) -> Result<&str, Failure> {
    if value.len() == 3 && value.bytes().all(|byte| byte.is_ascii_uppercase()) {
        return Ok(value);
    }
    Err(Failure::invalid(
        "invalid_currency",
        "--currency must be exactly three uppercase ASCII letters",
    )
    .remedy("pass an explicit three-letter currency such as XAF or USD"))
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

fn parse_project_years(value: &str) -> Result<u8, Failure> {
    value
        .parse::<u8>()
        .ok()
        .filter(|years| (1..=100).contains(years))
        .ok_or_else(|| {
            Failure::invalid(
                "invalid_project_years",
                "--project-years must be a whole number from 1 through 100",
            )
            .remedy("pass an explicit integer from 1 through 100")
        })
}

fn parse_discount_rate(value: &str) -> Result<f64, Failure> {
    value
        .parse::<f64>()
        .ok()
        .filter(|rate| rate.is_finite() && *rate >= 0.0 && *rate < 1.0)
        .ok_or_else(|| {
            Failure::invalid(
                "invalid_discount_rate",
                "--discount-rate must be finite, at least 0 and less than 1",
            )
            .remedy("pass an explicit decimal rate such as 0.08")
        })
}

fn first_duplicate(values: &[String]) -> Option<&str> {
    let mut seen = BTreeSet::new();
    values
        .iter()
        .find_map(|value| (!seen.insert(value.as_str())).then_some(value.as_str()))
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
