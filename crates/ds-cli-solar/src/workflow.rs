//! Closed read/import/status operations over the paired desktop Solar workflow.

use std::path::PathBuf;
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::paired;

const READ_TIMEOUT: Duration = Duration::from_secs(30);
const IMPORT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const RESULTS_READ_OPERATION: &str = "solar.results.read";
const SYNC_STATUS_OPERATION: &str = "solar.sync.status";
const PORTFOLIO_LIST_OPERATION: &str = "solar.portfolio.list";
const PORTFOLIO_ANALYSIS_OPERATION: &str = "solar.portfolio.analysis";
const MAX_PORTFOLIO_ID_CHARS: usize = 128;
const FINAL_IMPORT_OPERATION: &str = "solar.final.import";
const FINAL_SUBMIT_OPERATION: &str = "solar.final.submit";
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

const DESCRIPTOR_ARG: Arg = Arg::value(
    "desktop-descriptor",
    "<path>",
    "Use this bridge descriptor instead of discovering one.",
);

static PORTFOLIO_READ_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "invalid_portfolio_path",
        when: "--path is empty, too deep, or contains an overlong semantic key",
        remedy: "pass at most eight non-empty semantic object keys, each no longer than 120 characters",
    },
    Refusal {
        code: "portfolio_path_not_found",
        when: "the sealed aggregate result has no value at the requested semantic path",
        remedy: "omit --path to inspect the bounded root outline, then request keys that result declares",
    },
    Refusal {
        code: "desktop_not_paired",
        when: "no DS GridDesign session is running on this machine",
        remedy: "start DS GridDesign, sign in, and retry",
    },
    Refusal {
        code: "desktop_ambiguous",
        when: "more than one DS GridDesign session is running",
        remedy: "name one with --desktop-descriptor <path>",
    },
    Refusal {
        code: "desktop_unreachable",
        when: "the bridge descriptor names a session that does not answer",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_unreadable",
        when: "the paired session's reply could not be read",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_operation_unsupported",
        when: "this DS GridDesign build does not offer the named Solar operation",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "desktop_refused",
        when: "the paired application declined the requested Solar operation",
        remedy: "read the refusal detail, correct the application state, and retry",
    },
    Refusal {
        code: "desktop_contract_mismatch",
        when: "the paired session returned an invalid artifact slice or aggregate result JSON",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "pairing_rejected",
        when: "the descriptor's pairing secret is stale",
        remedy: "restart DS GridDesign to publish a fresh descriptor",
    },
];

pub static RESULTS_READ_COMMAND: Command = Command {
    id: "solar.results.read",
    path: &["solar", "results", "read"],
    contract: 1,
    summary: "Read one dashboard section from a native Solar result.",
    purpose: "Reads a bounded semantic projection from the canonical report_input receipt used by the Solar dashboards. It does not read result.json, a workspace path, or a cloud-specific cache.",
    chapter: Chapter::Solar,
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
    chapter: Chapter::Solar,
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
    chapter: Chapter::Solar,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[DESCRIPTOR_ARG],
    output: "Portfolio ids, names, display names, membership revisions, city counts and exact ordered member city ids.",
    examples: &[Example {
        command: "ds solar portfolio list --output json",
        note: "Use the selected id and membership revision together with `solar run start --portfolio`.",
        runnable: false,
    }],
    refusals: paired::REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

static PORTFOLIO_ANALYSIS_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "invalid_portfolio_id",
        when: "--portfolio is empty or longer than 128 characters",
        remedy: "pass one exact portfolio id from `ds solar portfolio list`",
    },
    Refusal {
        code: "desktop_not_paired",
        when: "no DS GridDesign session is running on this machine",
        remedy: "start DS GridDesign, sign in, and retry",
    },
    Refusal {
        code: "desktop_ambiguous",
        when: "more than one DS GridDesign session is running",
        remedy: "name one with --desktop-descriptor <path>",
    },
    Refusal {
        code: "desktop_unreachable",
        when: "the bridge descriptor names a session that does not answer",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_unreadable",
        when: "the paired session's reply could not be read",
        remedy: "restart DS GridDesign and retry",
    },
    Refusal {
        code: "desktop_operation_unsupported",
        when: "this DS GridDesign build does not offer the named Solar operation",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "desktop_refused",
        when: "the active project holds no portfolio with that id, or the governed read was declined",
        remedy: "read the refusal detail, select the owning project, and retry",
    },
    Refusal {
        code: "desktop_contract_mismatch",
        when: "the paired session returned a saved-analysis projection outside this command contract",
        remedy: "update DS GridDesign and ds to matching releases",
    },
    Refusal {
        code: "pairing_rejected",
        when: "the descriptor's pairing secret is stale",
        remedy: "restart DS GridDesign to publish a fresh descriptor",
    },
];

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
    contract: 1,
    summary: "Report one governed portfolio's saved analysis state.",
    purpose: "Reads one portfolio in the active project through the same governed saved-analysis projection the desktop Pipeline panel renders: portfolio identity, membership revision, whether a saved analysis exists, its identity when it does, and the refusal verbatim when the read failed. It never calculates, never selects a run, and never reconstructs an aggregate from city rows; use `solar portfolio read --run-id` for a sealed result projection.",
    chapter: Chapter::Solar,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("portfolio", "<id>", "Portfolio id in the active project.").required(),
        DESCRIPTOR_ARG,
    ],
    output: "Portfolio id and name, membership revision, exact ordered members, saved-analysis state (ready, failed or none), the analysis identity when present, and the verbatim saved error when the read failed.",
    examples: &[Example {
        command: "ds solar portfolio analysis --portfolio aderm_loc7 --output json",
        note: "The same answer the Pipeline panel shows for that portfolio; `none` means nothing has been saved yet, not an error.",
        runnable: false,
    }],
    refusals: PORTFOLIO_ANALYSIS_REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub static PORTFOLIO_READ_COMMAND: Command = Command {
    id: "solar.portfolio.read",
    path: &["solar", "portfolio", "read"],
    contract: 3,
    summary: "Read a bounded projection of one sealed portfolio result.",
    purpose: "Pages one completed paired Solar portfolio result, verifies its native name, content digest and batch identity, then validates the v2/v3 trace before returning one bounded semantic projection. V3 round-robin results carry no false single representative: the receipt instead names each graph's source member. Repeated --path values descend through object keys, never filesystem paths. The CLI performs no calculation and never reconstructs a portfolio from city rows.",
    chapter: Chapter::Solar,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("run-id", "<id>", "Completed native Solar run id.").required(),
        Arg::repeated(
            "path",
            "<field>",
            "Semantic aggregate-result object key. Repeat to descend, up to eight keys.",
        ),
        DESCRIPTOR_ARG,
    ],
    output: "The v2/v3 schema, bounded engine identity, portfolio name/id/revision, run identity, ordered members, input/result/content/batch digests, assumptions, representative city or null round-robin marker, bounded v3 graph-member evidence, selected path/value, completeness and sealed byte count. Large arrays and strings are edge-sampled; complete=false identifies elision.",
    examples: &[
        Example {
            command: "ds solar portfolio read --run-id run-123 --output json",
            note: "Reads a bounded root projection of the sealed aggregate result.",
            runnable: false,
        },
        Example {
            command: "ds solar portfolio read --run-id run-123 --path sections --path investment --output json",
            note: "Reads one narrow semantic value without exporting the full result file.",
            runnable: false,
        },
    ],
    refusals: PORTFOLIO_READ_REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub static FINAL_IMPORT_COMMAND: Command = Command {
    id: "solar.final.import",
    path: &["solar", "final", "import"],
    contract: 1,
    summary: "Import an externally interpreted Markdown final.",
    purpose: "Hands one explicit Markdown source path to the paired native shell, which validates and stores it in the selected run/city final slot and optionally renders DOCX with the installed Pandoc. Import is local review state only; `solar final submit` is the separate publication authority. The app calls no model.",
    chapter: Chapter::Solar,
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
    output: "An imported or cancelled receipt naming the run and city. Import does not queue publication.",
    examples: &[Example {
        command: "ds solar final import --run-id run-123 --city kigali --file ./kigali-final.md --yes --output json",
        note: "The source is interpreted externally; DS GridDesign validates, stores and optionally renders it for local review.",
        runnable: false,
    }],
    refusals: paired::REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub static FINAL_SUBMIT_COMMAND: Command = Command {
    id: "solar.final.submit",
    path: &["solar", "final", "submit"],
    contract: 1,
    summary: "Submit one imported interpreted final for publication.",
    purpose: "Explicitly queues the already-imported final for one completed native run and city. It never imports a file, chooses another run, or treats a calculation draft as final; use `solar final import` first and inspect Sync Center separately.",
    chapter: Chapter::Solar,
    effect: Effect::ArtifactWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("run-id", "<id>", "Completed native Solar batch id.").required(),
        Arg::value("city", "<id>", "Canonical city context in that batch.").required(),
        DESCRIPTOR_ARG,
    ],
    output: "A submitted or unchanged receipt naming the exact run and city.",
    examples: &[Example {
        command: "ds solar final submit --run-id run-123 --city kigali --yes --output json",
        note: "Queues only the imported final bound to this exact native run and city.",
        runnable: false,
    }],
    refusals: paired::REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub fn results_read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let run_id = inputs.require("run-id")?;
    let city = inputs.require("city")?;
    let mut arguments = run_city(inputs)?;
    arguments.insert("section".into(), json!(inputs.require("section")?));
    if !inputs.repeated("path").is_empty() {
        arguments.insert("path".into(), json!(inputs.repeated("path")));
    }
    paired::require_exact_identity(
        paired::invoke(
            inputs,
            RESULTS_READ_OPERATION,
            Value::Object(arguments),
            READ_TIMEOUT,
        )?,
        RESULTS_READ_OPERATION,
        run_id,
        Some(city),
    )
}

pub fn sync_status(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let run_id = inputs.value("run-id");
    let arguments = run_id.map_or_else(|| json!({}), |run_id| json!({ "run_id": run_id }));
    let result = paired::invoke(inputs, SYNC_STATUS_OPERATION, arguments, READ_TIMEOUT)?;
    match run_id {
        Some(run_id) => paired::require_exact_identity(result, SYNC_STATUS_OPERATION, run_id, None),
        None => Ok(result),
    }
}

pub fn portfolio_list(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    paired::invoke(inputs, PORTFOLIO_LIST_OPERATION, json!({}), READ_TIMEOUT)
}

/// Ask the application for one portfolio's saved-analysis projection.
///
/// The reply is checked against the id that was asked for. A projection is the
/// answer to a question about ONE portfolio, so a reply about a different one is
/// a contract mismatch, not a result to print — the same rule
/// `require_exact_identity` applies to run-addressed operations.
pub fn portfolio_analysis(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let portfolio_id = inputs.require("portfolio")?;
    if portfolio_id.is_empty() || portfolio_id.chars().count() > MAX_PORTFOLIO_ID_CHARS {
        return Err(Failure::invalid(
            "invalid_portfolio_id",
            "a portfolio id must be between one and 128 characters",
        )
        .remedy("pass one exact portfolio id from `ds solar portfolio list`"));
    }
    let result = paired::invoke(
        inputs,
        PORTFOLIO_ANALYSIS_OPERATION,
        json!({ "portfolio_id": portfolio_id }),
        READ_TIMEOUT,
    )?;
    if result.get("portfolio_id").and_then(Value::as_str) != Some(portfolio_id) {
        return Err(Failure::unavailable(
            "desktop_contract_mismatch",
            "the paired session answered about a different portfolio",
        )
        .remedy("update DS GridDesign and ds to matching releases"));
    }
    Ok(result)
}

pub fn portfolio_read(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let path = portfolio_path(inputs)?;
    let sealed = crate::exports::read_portfolio_result(inputs)?;
    let result_bytes = sealed.bytes.len();
    let document: Value = serde_json::from_slice(&sealed.bytes).map_err(|_| {
        Failure::unavailable(
            "desktop_contract_mismatch",
            "the paired session returned a sealed portfolio result that is not valid JSON",
        )
        .remedy("update DS GridDesign and ds to matching releases")
    })?;
    let trace = portfolio_trace(&document, inputs.require("run-id")?)?;
    let selected = select_portfolio_path(&document, &path)?;
    let (value, complete) = bounded_portfolio_projection(selected);
    let mut result = json!({
        "status": "ok",
        "run_id": inputs.require("run-id")?,
        "artifact": "result",
        "name": sealed.name,
        "content_digest": sealed.content_digest,
        "batch_id": sealed.batch_id,
        "batch_digest": sealed.batch_digest,
        "path": path,
        "value": value,
        "complete": complete,
        "result_bytes": result_bytes,
    });
    if !complete {
        result["more"] = json!({
            "reason": "projection_elided",
            "next": "repeat with a narrower --path",
        });
    }
    result
        .as_object_mut()
        .expect("portfolio read result is an object")
        .extend(trace);
    Ok(result)
}

pub fn final_import(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let run_id = inputs.require("run-id")?;
    let city = inputs.require("city")?;
    let mut arguments = run_city(inputs)?;
    let source = absolute_path(inputs.require("file")?);
    arguments.insert("source_path".into(), json!(source));
    paired::require_exact_identity(
        paired::invoke(
            inputs,
            FINAL_IMPORT_OPERATION,
            Value::Object(arguments),
            IMPORT_TIMEOUT,
        )?,
        FINAL_IMPORT_OPERATION,
        run_id,
        Some(city),
    )
}

pub fn final_submit(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let run_id = inputs.require("run-id")?;
    let city = inputs.require("city")?;
    paired::require_exact_identity(
        paired::invoke(
            inputs,
            FINAL_SUBMIT_OPERATION,
            Value::Object(run_city(inputs)?),
            READ_TIMEOUT,
        )?,
        FINAL_SUBMIT_OPERATION,
        run_id,
        Some(city),
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

fn portfolio_path(inputs: &Inputs) -> Result<Vec<&str>, Failure> {
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

fn select_portfolio_path<'a>(document: &'a Value, path: &[&str]) -> Result<&'a Value, Failure> {
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

fn portfolio_trace(document: &Value, expected_run_id: &str) -> Result<Map<String, Value>, Failure> {
    let encoded = serde_json::to_vec(document).map_err(|_| {
        Failure::unavailable(
            "desktop_contract_mismatch",
            "the paired session returned a Solar portfolio result that cannot be encoded",
        )
        .remedy("update DS GridDesign and ds to matching releases")
    })?;
    let validated = ds_command_kernel::validate_solar_portfolio_result(&encoded, expected_run_id)
        .map_err(|error| {
        Failure::unavailable(
            "desktop_contract_mismatch",
            format!("the paired session returned an invalid sealed portfolio identity: {error}"),
        )
        .remedy("update DS GridDesign and ds to matching releases")
    })?;
    let validated: Value = serde_json::from_str(&validated).map_err(|_| {
        Failure::unavailable(
            "desktop_contract_mismatch",
            "the shared Solar portfolio projection could not be decoded",
        )
        .remedy("update DS GridDesign and ds to matching releases")
    })?;
    validated
        .get("trace")
        .and_then(Value::as_object)
        .cloned()
        .ok_or_else(|| {
            Failure::unavailable(
                "desktop_contract_mismatch",
                "the shared Solar portfolio projection omitted its trace",
            )
            .remedy("update DS GridDesign and ds to matching releases")
        })
}
fn bounded_portfolio_projection(value: &Value) -> (Value, bool) {
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
