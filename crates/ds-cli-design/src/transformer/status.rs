//! `ds design status` — the project's transformer status rows, headless.
//!
//! This is the read every other Design answer is built from. It lives beside
//! the transformer lifecycle commands because it uses their credential path,
//! their scope flag and their refusals, but it is not one of them: it answers
//! `ds design status`, not `ds design transformer status`, because a status
//! row is the project's own state, not a step in a transformer's lifecycle.

use ds_cli_auth::{TransformerStatusList, TransformerStatusRow};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::design_health::{TransformerHealth, summarize, transformer_health};
use serde_json::{Value, json};

use super::{LANE_ARG, TRANSFORMER_ARG};

pub const FINDINGS_ARG: Arg = Arg::switch(
    "findings",
    "List the project's findings as rows: transformer, phase, code, affected count.",
);

pub const SEARCH_ARG: Arg = Arg::value(
    "search",
    "<text>",
    "Name substring; quote it for an exact name.",
);
pub const SORT_ARG: Arg = Arg::value("sort", "<key>", "Order the rows by this.").choices(&[
    "name",
    "district",
    "tags",
    "legacy",
    "process",
    "report",
    "combined",
    "governance",
    "user",
    "updated",
    "version",
]);
pub const DESC_ARG: Arg = Arg::switch("desc", "Sort descending.");
pub const FILTER_ARG: Arg = Arg::repeated(
    "filter",
    "<dimension=value>",
    "Keep matching rows; repeat to combine. Dimensions are in the reference.",
);

const QUERY_INVALID: Refusal = Refusal {
    code: "design_status_query_invalid",
    when: "A --filter is not <dimension=value> over a known dimension, or names two admin levels",
    remedy: "See the reference for the dimensions; name one admin level",
};

const REFUSALS: &[Refusal] = &[
    super::NATIVE_PROFILE,
    super::NATIVE_PROFILE_DIGEST,
    super::NATIVE_PROFILE_UNSAFE,
    super::HEADLESS_SIGNED_OUT,
    super::HEADLESS_NO_PROJECT,
    super::PROJECT_CONTEXT_STALE,
    QUERY_INVALID,
];

pub static COMMAND: Command = Command {
    id: "design.status",
    path: &["design", "status"],
    contract: 1,
    summary: "Read the project's transformer status rows and verdicts, headlessly.",
    purpose: "\
The read every headless Design answer starts from. Restores the native user \
and reads only that user's audience-fenced selected project through the fixed \
status call. Without --transformer it answers every transformer document; \
with names, exactly those that exist. Rows come back as the service sent \
them, each with the shared kernel's verdict (health) and truth (view, phase \
ownership, latest action, governance, retry, kind, allows). With --search, \
--sort or --filter the rows are the register's own selection, in its order, \
decided by the same kernel the application reads. Nothing falls back to a \
browser; the reference describes each member.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TRANSFORMER_ARG,
        LANE_ARG,
        FINDINGS_ARG,
        SEARCH_ARG,
        SORT_ARG,
        DESC_ARG,
        FILTER_ARG,
    ],
    output: "\
Lane and selected-project identity/status, the row count, the project's \
severity summary, and one row per transformer as the service sent it — \
process/report/draft/sketch metadata, layer counts, uploads, artifacts, \
retry capabilities — plus `health`, `view`, `phase_ownership`, `latest_action`, \
`governance`, `retry`, `kind` and `allows`. With --findings, one `findings` row \
per finding; with a selector, `query` carries the options each filter may offer.",
    examples: &[
        Example {
            command: "ds design status --transformer TX-1 --output json",
            note: "`.data.transformers[0].health.severity` is the row's verdict.",
            runnable: false,
        },
        Example {
            command: "ds design status --findings --output json",
            note: "`.data.findings` is the project's issue list.",
            runnable: false,
        },
    ],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // The selector is validated before any credential is touched.
    let selector = selector_from_inputs(inputs)?;
    let requested = super::transformer_set(inputs, false)?;
    let headless = ds_cli_auth::transformer_status(inputs.require("lane")?, &requested)?;
    let mut output = super::project_receipt(&headless);
    let mut rows = status_json(headless.result(), inputs.switch("findings"));
    if let Some(selector) = selector {
        apply_selector(headless.result(), &mut rows, selector)?;
    }
    output
        .as_object_mut()
        .expect("receipt is an object")
        .extend(rows.as_object().expect("rows are an object").clone());
    Ok(output)
}

/// The register's selector, spelled as `ds` arguments. `None` when no
/// selecting argument was given: the rows are then the service's, unreshaped.
fn selector_from_inputs(inputs: &Inputs) -> Result<Option<Value>, Failure> {
    let search = inputs.value("search").unwrap_or("");
    let sort = inputs.value("sort");
    let filters = inputs.repeated("filter");
    if search.is_empty() && sort.is_none() && filters.is_empty() && !inputs.switch("desc") {
        return Ok(None);
    }
    let invalid =
        |detail: String| Failure::invalid(QUERY_INVALID.code, detail).remedy(QUERY_INVALID.remedy);
    let mut selector = json!({ "search": search });
    for raw in filters {
        let Some((dimension, value)) = raw.split_once('=') else {
            return Err(invalid(format!("--filter {raw} is not <dimension=value>")));
        };
        let (dimension, value) = (dimension.trim(), value.trim());
        if value.is_empty() {
            return Err(invalid(format!("--filter {dimension} names no value")));
        }
        match dimension {
            "sync" | "lane" | "warning-type" => {
                let key = if dimension == "warning-type" {
                    "warning_types"
                } else {
                    dimension
                };
                let list = selector[key].as_array().cloned().unwrap_or_default();
                let mut list = list;
                list.push(Value::String(value.to_string()));
                selector[key] = Value::Array(list);
            }
            "legacy" | "process" | "report" | "combined" | "governance" | "user" => {
                selector[dimension] = Value::String(value.to_string());
            }
            _ => {
                let Some(level) = dimension.strip_prefix("admin-") else {
                    return Err(invalid(format!(
                        "--filter names an unknown dimension: {dimension}"
                    )));
                };
                if !["district", "sector", "cell", "village"].contains(&level) {
                    return Err(invalid(format!(
                        "--filter names an unknown admin level: {level}"
                    )));
                }
                if selector.get("admin_level").is_some() {
                    return Err(invalid(
                        "--filter names two admin levels; one applies".to_string(),
                    ));
                }
                selector["admin_level"] = Value::String(level.to_string());
                selector["admin_value"] = Value::String(value.to_string());
            }
        }
    }
    let mut request = json!({ "selector": selector });
    if let Some(key) = sort {
        request["sort"] = json!({ "key": key, "asc": !inputs.switch("desc") });
    }
    Ok(Some(request))
}

/// One kernel question over the rows as the service sent them: the register's
/// order and the options each filter may offer. A headless client holds no
/// browser session, pins, tags or saved selection, so those members are empty.
fn apply_selector(
    list: &TransformerStatusList,
    rows: &mut Value,
    request: Value,
) -> Result<(), Failure> {
    let raw: Vec<Value> = list.rows().iter().map(|row| row.row().clone()).collect();
    let mut query = json!({
        "op": "query",
        "schema": ds_command_kernel::design_status_query::SCHEMA,
        "rows": raw,
        "context": { "now_ms": now_ms() },
    });
    for (key, value) in request.as_object().expect("request is an object") {
        query[key] = value.clone();
    }
    let input = serde_json::to_vec(&query)
        .map_err(|error| Failure::invalid(QUERY_INVALID.code, error.to_string()))?;
    let reply = ds_command_kernel::design_status_query::evaluate(&input).map_err(|error| {
        Failure::invalid(QUERY_INVALID.code, error).remedy(QUERY_INVALID.remedy)
    })?;
    let reply: Value = serde_json::from_str(&reply).unwrap_or(Value::Null);
    let object = rows.as_object_mut().expect("rows are an object");
    let mut queues: std::collections::HashMap<String, std::collections::VecDeque<Value>> =
        std::collections::HashMap::new();
    for row in object
        .remove("transformers")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
    {
        let name = row["name"].as_str().unwrap_or_default().to_string();
        queues.entry(name).or_default().push_back(row);
    }
    let ordered: Vec<Value> = reply["ordered"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|name| name.as_str())
        .filter_map(|name| queues.get_mut(name).and_then(|queue| queue.pop_front()))
        .collect();
    object.insert("count".into(), json!(ordered.len()));
    object.insert("transformers".into(), Value::Array(ordered));
    object.insert(
        "query".into(),
        json!({
            "selector": request["selector"],
            "sort": request.get("sort").cloned().unwrap_or(Value::Null),
            "options": reply["options"],
        }),
    );
    Ok(())
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The rows as they arrived, each with the verdict the shared kernel reads
/// from it, plus the count and the per-bucket summary the caller would
/// otherwise derive — and derive differently from the application, which is
/// the split this closes.
fn status_json(list: &TransformerStatusList, findings: bool) -> Value {
    let health: Vec<TransformerHealth> = list
        .rows()
        .iter()
        .map(|row| transformer_health(row.row()))
        .collect();
    // The rows' truth — saved/unsaved, locality, lane, presence, version,
    // which run owns the record, the latest action, the governance label, the
    // retry verdict, the row's kind and what it may be a target of — is the
    // shared kernel's answer over the same rows. A headless client holds no
    // browser rooms, so every row is remote and clean here by construction.
    let truth = status_row_truth(list);
    let rows: Vec<Value> = list
        .rows()
        .iter()
        .zip(&health)
        .enumerate()
        .map(|(index, (row, health))| {
            let mut row = row.row().clone();
            if let Some(object) = row.as_object_mut() {
                object.insert(
                    "health".into(),
                    serde_json::to_value(health).unwrap_or(Value::Null),
                );
                if let Some(answer) = truth.get(index) {
                    for key in [
                        "view",
                        "phase_ownership",
                        "latest_action",
                        "governance",
                        "retry",
                        "kind",
                        "allows",
                    ] {
                        object.insert(key.into(), answer[key].clone());
                    }
                }
            }
            row
        })
        .collect();
    let mut out = json!({
        "count": list.len(),
        "summary": summarize(health.iter()),
        "transformers": rows,
    });
    if findings {
        out.as_object_mut().expect("object").insert(
            "findings".into(),
            Value::Array(fleet_findings(list.rows(), &health)),
        );
    }
    out
}

/// One kernel call over the list: the row truth in server order (the kernel
/// keeps the server's first position for every name), keyed back by index.
fn status_row_truth(list: &TransformerStatusList) -> Vec<Value> {
    let rows: Vec<Value> = list.rows().iter().map(|row| row.row().clone()).collect();
    let request = json!({
        "schema": ds_command_kernel::design_status_row::SCHEMA,
        "server_rows": rows,
        "local_room_headers": [],
        "current_user_email": "",
        "now_ms": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0),
    });
    let Ok(input) = serde_json::to_vec(&request) else {
        return Vec::new();
    };
    let Ok(reply) = ds_command_kernel::design_status_row::evaluate(&input) else {
        return Vec::new();
    };
    let reply: Value = serde_json::from_str(&reply).unwrap_or(Value::Null);
    let mut by_index: Vec<Value> = vec![Value::Null; list.len()];
    for row in reply["rows"].as_array().into_iter().flatten() {
        let slot = row["server_index"]
            .as_u64()
            .and_then(|index| by_index.get_mut(index as usize));
        if let Some(slot) = slot {
            *slot = row.clone();
        }
    }
    by_index
}

/// Every finding in the project, in row order then finding order — the same
/// list the register's error and warning tables sort and render.
fn fleet_findings(rows: &[TransformerStatusRow], health: &[TransformerHealth]) -> Vec<Value> {
    let mut out = Vec::new();
    for (row, health) in rows.iter().zip(health) {
        for finding in &health.findings {
            let mut finding = serde_json::to_value(finding).unwrap_or(Value::Null);
            if let Some(object) = finding.as_object_mut() {
                object.insert("transformer".into(), Value::String(row.name().to_string()));
            }
            out.push(finding);
        }
    }
    out
}

pub fn render(data: &Value) -> String {
    let summary = &data["summary"];
    let mut out = format!(
        "project {} ({}) · {} · {} transformers · {} processing · {} warning · {} error\n",
        data["project"]["project_name"].as_str().unwrap_or("?"),
        data["project"]["ds_project"].as_str().unwrap_or("?"),
        data["lane"].as_str().unwrap_or("?"),
        data["count"].as_u64().unwrap_or(0),
        summary["processing"].as_u64().unwrap_or(0),
        summary["warnings"].as_u64().unwrap_or(0),
        summary["errors"].as_u64().unwrap_or(0),
    );
    if let Some(rows) = data["findings"].as_array() {
        for finding in rows {
            let line = format!(
                "  {:<32} {:<8} {:<8} {:<28} {}",
                finding["transformer"].as_str().unwrap_or("?"),
                finding["severity"].as_str().unwrap_or("-"),
                finding["phase"].as_str().unwrap_or("-"),
                finding["code"]
                    .as_str()
                    .filter(|code| !code.is_empty())
                    .unwrap_or("-"),
                finding["message"].as_str().unwrap_or(""),
            );
            out.push_str(line.trim_end());
            out.push('\n');
        }
        return out;
    }
    if let Some(rows) = data["transformers"].as_array() {
        for row in rows {
            let version = row["metadata"]["version"]
                .as_u64()
                .map(|version| format!("v{version}"))
                .unwrap_or_default();
            let line = format!(
                "  {:<32} {:<9} {:<10} {:<8} {:<12} {:<12} {}",
                row["name"].as_str().unwrap_or("?"),
                row["health"]["severity"].as_str().unwrap_or("-"),
                row["kind"].as_str().unwrap_or("-"),
                row["governance"]["state"].as_str().unwrap_or("-"),
                row["process_metadata"]["status"].as_str().unwrap_or("-"),
                row["report_metadata"]["status"].as_str().unwrap_or("-"),
                version,
            );
            out.push_str(line.trim_end());
            out.push('\n');
        }
    }
    out
}
