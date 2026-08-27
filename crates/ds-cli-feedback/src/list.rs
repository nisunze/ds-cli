//! `ds feedback list` — the shared backlog, as the `fb` tab reads it.

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops;
use serde_json::{Map, Value, json};

use crate::{MAX_LIST_LIMIT, bounded_text, truncate};

const TIMEOUT: Duration = Duration::from_secs(30);
const MAX_FILTER_CHARS: usize = 200;

pub static COMMAND: Command = Command {
    id: "feedback.list",
    path: &["feedback", "list"],
    contract: 1,
    summary: "The shared feedback backlog: what is still open, and its ids.",
    purpose: "\
Reads the same deduplicated backlog the `fb` tab shows, through the paired \
application's signed-in session. This is where a close begins: it returns the \
report id and the version a close must carry, the acceptance condition the \
original sighting wrote down, and how many times the gap was seen.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value("view", "<view>", "Which half of the backlog to return.")
            .choices(&["not_addressed", "addressed", "all"])
            .default("not_addressed"),
        Arg::value(
            "component",
            "<repository[/area]>",
            "Keep reports whose component contains this text.",
        ),
        Arg::value(
            "query",
            "<text>",
            "Keep reports whose title, detail or component contains this text.",
        ),
        Arg::value(
            "limit",
            "<count>",
            "Rows to return (1-50). The matching total is always reported.",
        )
        .default("20"),
        Arg::switch(
            "detail",
            "Return each report's full detail instead of a bounded excerpt.",
        ),
        ops::DESCRIPTOR_ARG,
    ],
    output: "\
`view`, `total`, `truncated`, and `reports` rows with `id`, `status`, `kind`, \
`severity`, `component`, `surface`, `title`, `detail`, `detail_truncated`, \
`occurrences`, `reporters`, `resolution`, `version`, `last_seen_at` and \
`updated_by`. The `id` and `version` are what `ds feedback close` takes.",
    examples: &[Example {
        command: "ds feedback list --component ds-cli --detail --output json",
        note: "Read the acceptance condition in .data.reports[].detail before closing anything.",
        runnable: false,
    }],
    refusals: &[
        ops::NOT_PAIRED,
        ops::AMBIGUOUS,
        ops::UNREACHABLE,
        ops::PAIRING_REJECTED,
        ops::REFUSED,
        ops::UNSUPPORTED,
        ops::UNREADABLE,
        ops::INVALID_NUMBER,
        crate::NOT_SIGNED_IN,
        crate::INVALID_TEXT,
    ],
    reference: Some("docs/reference/feedback.md"),
    availability: ops::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::from_iter([(
        "view".to_string(),
        Value::String(inputs.require("view")?.to_string()),
    )]);
    if let Some(component) = inputs.value("component") {
        arguments.insert(
            "component".into(),
            json!(bounded_text(component, "component", MAX_FILTER_CHARS)?),
        );
    }
    if let Some(query) = inputs.value("query") {
        arguments.insert(
            "query".into(),
            json!(bounded_text(query, "query", MAX_FILTER_CHARS)?),
        );
    }
    if let Some(limit) = inputs.value("limit") {
        arguments.insert(
            "limit".into(),
            json!(ops::integer(limit, "limit", 1, MAX_LIST_LIMIT)?),
        );
    }
    if inputs.switch("detail") {
        arguments.insert("detail".into(), json!(true));
    }
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(&descriptor, &crate::LIST, Value::Object(arguments), TIMEOUT)
        .map_err(crate::classify_feedback_failure)
}

pub fn render(data: &Value) -> String {
    let total = data["total"].as_u64().unwrap_or(0);
    let rows = data["reports"].as_array().map(Vec::len).unwrap_or(0);
    let mut out = format!(
        "{} · {}\n",
        data["view"].as_str().unwrap_or("backlog"),
        ops::plural(total, "report")
    );
    for report in data["reports"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<26} {:<12} {:<8} {:<20} {}\n",
            truncate(report["id"].as_str().unwrap_or("?"), 26),
            report["status"].as_str().unwrap_or("?"),
            report["severity"].as_str().unwrap_or("?"),
            truncate(report["component"].as_str().unwrap_or("—"), 20),
            truncate(report["title"].as_str().unwrap_or(""), 52),
        ));
    }
    if data["truncated"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "  … {rows} of {total} shown; narrow with --component or --query\n"
        ));
    }
    out
}
