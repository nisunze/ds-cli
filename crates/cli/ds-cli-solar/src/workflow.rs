//! Headless Solar result-section, portfolio list and analysis reads.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

const MAX_PORTFOLIO_PATH_DEPTH: usize = 8;
const MAX_PORTFOLIO_PATH_KEY_CHARS: usize = 120;
const PORTFOLIO_PROJECTION_BYTES: usize = 15_000;
const PORTFOLIO_SERIES_EDGE_ITEMS: usize = 4;
const PORTFOLIO_STRING_CHARS: usize = 400;
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

pub static RESULTS_READ_COMMAND: Command = Command {
    id: "solar.results.read",
    path: &["solar", "results", "read"],
    contract: 2,
    summary: "Read one dashboard section from a native Solar result.",
    purpose: "Verifies an exact closed local Solar batch and reads a bounded semantic section of its sealed city report input through shared Rust. No desktop, browser, sign-in or network is required. Explicit local project attribution grants no cloud authority.",
    chapter: Chapter::Solar,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "source",
            "<dir>",
            "Closed local Solar city batch directory.",
        )
        .required(),
        Arg::value(
            "project",
            "<id>",
            "Exact project attribution in the sealed batch.",
        )
        .required(),
        Arg::value("run-id", "<id>", "Completed native Solar batch id.").required(),
        Arg::value("city", "<id>", "Canonical city context in that batch.").required(),
        Arg::value("section", "<name>", "Dashboard result section to read.")
            .required()
            .choices(RESULT_SECTIONS),
        Arg::repeated("path", "<field>", "Semantic child key. Repeat to descend."),
    ],
    output: "Verified project/run/city and source digests, bounded section projection and explicit completeness flag.",
    examples: &[Example {
        command: "ds solar results read --source ./batch --project project-1 --run-id run-123 --city kigali --section finance --path financial_summary --output json",
        note: "Reads the same canonical result section the Finance dashboard uses.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "solar_project_schema_unavailable",
            when: "the installed Solar owner lacks the sealed workspace contract",
            remedy: "install matching ds and ds-solar releases",
        },
        Refusal {
            code: "engine_refused",
            when: "the sealed source or project/run/city identity cannot be verified",
            remedy: "use an intact closed Solar batch and its exact identity",
        },
        Refusal {
            code: "solar_project_io",
            when: "the bounded owner request or receipt cannot be handled",
            remedy: "verify source access and matching ds and ds-solar releases",
        },
        Refusal {
            code: "solar_dashboard_path_invalid",
            when: "the section or bounded semantic path is unavailable",
            remedy: "read the section without --path, then select an existing child key",
        },
        Refusal {
            code: "solar_dashboard_invalid",
            when: "the Solar owner returned an invalid verified source receipt",
            remedy: "install matching ds and ds-solar releases",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    search: &[],
    requires: Requires::Server,
    availability: crate::project::availability,
};

pub static PORTFOLIO_LIST_COMMAND: Command = Command {
    id: "solar.portfolio.list",
    path: &["solar", "portfolio", "list"],
    contract: 1,
    summary: "List Solar portfolios and their city membership.",
    purpose: "Reads the explicit project's governed portfolio catalog through native authentication without a desktop or a selected-project change.",
    chapter: Chapter::Solar,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::portfolio_headless::PROJECT,
        crate::portfolio_headless::LANE,
    ],
    output: "Portfolio ids, names, display names, membership revisions, city counts and exact ordered member city ids.",
    examples: &[Example {
        command: "ds solar portfolio list --output json",
        note: "Use the selected id and membership revision together with `solar run start --portfolio`.",
        runnable: false,
    }],
    refusals: ds_cli_auth::PROJECT_STATUS_COMMAND.refusals,
    reference: Some("docs/reference/solar.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// The saved-analysis read, addressed by portfolio id alone.
///
/// It is deliberately NOT a second way to read a sealed result: it returns the
/// projection the application's own Pipeline panel renders — identity,
/// membership revision, status, the analysis identity when one exists, and the
/// governed refusal verbatim when the read failed. `solar portfolio read` keeps
/// its exact-run-id sealed-result semantics untouched.
pub static PORTFOLIO_ANALYSIS_COMMAND: Command = Command {
    id: "solar.portfolio.analysis",
    path: &["solar", "portfolio", "analysis"],
    contract: 2,
    summary: "Report one governed portfolio's saved analysis state.",
    purpose: "Reads saved portfolio analysis with explicit native project authority. Captures governed membership before the read and rechecks it afterward. Rust supplies the same ready, failed or none projection used by WASM consumers. Never calculates or selects a run.",
    chapter: Chapter::Solar,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value("portfolio", "<id>", "Exact governed portfolio id.").required(),
        crate::portfolio_headless::PROJECT,
        crate::portfolio_headless::LANE,
    ],
    output: "Portfolio id and name, membership revision, exact ordered members, saved-analysis state (ready, failed or none), the analysis identity when present, and the verbatim saved error when the read failed.",
    examples: &[Example {
        command: "ds solar portfolio analysis --project project_id --lane canary --portfolio aderm_loc7 --output json",
        note: "The same answer the Pipeline panel shows for that portfolio; `none` means nothing has been saved yet, not an error.",
        runnable: false,
    }],
    refusals: ds_cli_auth::PROJECT_STATUS_COMMAND.refusals,
    reference: Some("docs/reference/solar.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn results_read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut source = crate::dashboard::source(inputs)?;
    let projection = ds_command_kernel::solar_dashboard::read_section(
        &source["report"],
        inputs.require("section")?,
        &inputs
            .repeated("path")
            .iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>(),
    )
    .map_err(|e| Failure::invalid("solar_dashboard_path_invalid", e))?;
    source
        .as_object_mut()
        .ok_or_else(|| Failure::invalid("solar_dashboard_invalid", "invalid owner source receipt"))?
        .remove("report");
    Ok(json!({"source":source,"projection":projection}))
}

pub fn portfolio_list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    crate::portfolio_headless::execute(inputs, ds_cli_auth::SolarPortfolioCommand::List)
}

/// Ask the application for one portfolio's saved-analysis projection.
///
/// The reply is checked against the id that was asked for. A projection is the
/// answer to a question about ONE portfolio, so a reply about a different one is
/// a contract mismatch, not a result to print — the same rule
/// `require_exact_identity` applies to run-addressed operations.
pub fn portfolio_analysis(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    crate::portfolio_headless::execute(
        inputs,
        ds_cli_auth::SolarPortfolioCommand::Analysis {
            portfolio: inputs.require("portfolio")?.into(),
        },
    )
}

pub(crate) fn portfolio_path(inputs: &Inputs) -> Result<Vec<&str>, Failure> {
    let path = inputs.repeated("path");
    if path.len() > MAX_PORTFOLIO_PATH_DEPTH
        || path.iter().any(|key| {
            key.is_empty()
                || key.trim() != *key
                || key.chars().count() > MAX_PORTFOLIO_PATH_KEY_CHARS
        })
    {
        return Err(Failure::invalid(
            "invalid_portfolio_path",
            "--path must contain at most eight non-empty semantic keys of at most 120 characters",
        )
        .remedy("pass fewer, shorter object keys, or omit --path for the bounded root outline"));
    }
    Ok(path.iter().map(String::as_str).collect())
}

pub(crate) fn select_portfolio_path<'a>(
    document: &'a Value,
    path: &[&str],
) -> Result<&'a Value, Failure> {
    let mut selected = document;
    for key in path {
        selected = selected.get(*key).ok_or_else(|| {
            Failure::invalid(
                "portfolio_path_not_found",
                format!(
                    "the sealed portfolio result has no value at `{}`",
                    path.join(".")
                ),
            )
            .remedy("omit --path to inspect the bounded root outline, then request a declared key")
        })?;
    }
    Ok(selected)
}

pub(crate) fn bounded_portfolio_projection(value: &Value) -> (Value, bool) {
    let (projected, mut complete) = elide_portfolio_value(value);
    if serde_json::to_vec(&projected).is_ok_and(|bytes| bytes.len() <= PORTFOLIO_PROJECTION_BYTES) {
        return (projected, complete);
    }
    complete = false;
    let outline = match value {
        Value::Array(items) => json!({ "_outline": { "kind": "array", "length": items.len() } }),
        Value::Object(fields) => {
            let mut keys = fields.keys().take(200).cloned().collect::<Vec<_>>();
            keys.sort();
            json!({ "_outline": {
                "kind": "object",
                "keys": keys,
                "keys_truncated": fields.len() > 200,
            } })
        }
        _ => json!({ "_outline": { "kind": "scalar" } }),
    };
    (outline, complete)
}

fn elide_portfolio_value(value: &Value) -> (Value, bool) {
    match value {
        Value::Array(items) if items.len() > PORTFOLIO_SERIES_EDGE_ITEMS * 2 => {
            let head = items
                .iter()
                .take(PORTFOLIO_SERIES_EDGE_ITEMS)
                .map(|item| {
                    let (value, _) = elide_portfolio_value(item);
                    value
                })
                .collect::<Vec<_>>();
            let tail = items
                .iter()
                .skip(items.len() - PORTFOLIO_SERIES_EDGE_ITEMS)
                .map(|item| {
                    let (value, _) = elide_portfolio_value(item);
                    value
                })
                .collect::<Vec<_>>();
            (
                json!({ "_series": {
                "length": items.len(),
                "head": head,
                "tail": tail,
                "note": "series elided; request a narrower semantic path",
            } }),
                false,
            )
        }
        Value::Array(items) => {
            let mut complete = true;
            let projected = items
                .iter()
                .map(|item| {
                    let (value, item_complete) = elide_portfolio_value(item);
                    complete &= item_complete;
                    value
                })
                .collect();
            (Value::Array(projected), complete)
        }
        Value::Object(fields) => {
            let mut complete = true;
            let projected = fields
                .iter()
                .map(|(key, item)| {
                    let (value, item_complete) = elide_portfolio_value(item);
                    complete &= item_complete;
                    (key.clone(), value)
                })
                .collect();
            (Value::Object(projected), complete)
        }
        Value::String(text) if text.chars().count() > PORTFOLIO_STRING_CHARS => (
            json!({ "_text": {
                "characters": text.chars().count(),
                "head": text.chars().take(PORTFOLIO_STRING_CHARS).collect::<String>(),
            } }),
            false,
        ),
        _ => (value.clone(), true),
    }
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
    if let Some(value) = data.get("value") {
        out.push_str(&serde_json::to_string_pretty(value).unwrap_or_else(|_| "null".into()));
        out.push('\n');
        if data["complete"] == Value::Bool(false) {
            out.push_str("projection incomplete; request a narrower --path\n");
        }
    }
    out
}
