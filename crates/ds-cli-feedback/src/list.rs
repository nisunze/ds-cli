//! `ds feedback list` — the shared backlog, as the `fb` tab reads it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{MAX_LIST_LIMIT, bounded_text, truncate};

const MAX_FILTER_CHARS: usize = 200;

pub static COMMAND: Command = Command {
    id: "feedback.list",
    path: &["feedback", "list"],
    contract: 2,
    summary: "The shared feedback backlog: what is still open, and its ids.",
    purpose: "\
Reads the same deduplicated backlog the `fb` tab shows through the native signed-in user, without a selected project or Desktop. Scans the latest 200 records; scan_incomplete reports when older records may exist. This is where a close begins: it returns the \
report id and the version a close must carry, the acceptance condition the \
original sighting wrote down, and how many times the gap was seen.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
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
        Arg::value(
            "since",
            "<RFC3339>",
            "Keep feedback seen or updated at or after this time; native host only.",
        ),
        crate::LANE_ARG,
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
    refusals: &crate::native_refusals::<
        5,
        { 5 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() },
    >([
        ds_cli_contract::args::INVALID_NUMBER,
        crate::INVALID_TEXT,
        crate::NOT_FOUND,
        crate::CONFLICT,
        crate::NOT_PERMITTED,
    ]),
    reference: Some("docs/reference/feedback.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
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
            json!(ds_cli_contract::args::integer(
                limit,
                "limit",
                1,
                MAX_LIST_LIMIT
            )?),
        );
    }
    if inputs.switch("detail") {
        arguments.insert("detail".into(), json!(true));
    }
    // `--since` was refused on the paired route and accepted on the native
    // one. With one route there is one answer.
    if let Some(since) = inputs.value("since") {
        arguments.insert("since".into(), json!(since));
    }
    crate::invoke_native(inputs, "list", arguments)
}

pub fn render(data: &Value) -> String {
    let total = data["total"].as_u64().unwrap_or(0);
    let rows = data["reports"].as_array().map(Vec::len).unwrap_or(0);
    let mut out = format!(
        "{} · {}\n",
        data["view"].as_str().unwrap_or("backlog"),
        ds_cli_contract::args::plural(total, "report")
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
