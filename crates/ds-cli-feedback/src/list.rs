//! `ds feedback list` — the shared backlog, as the `fb` tab reads it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{MAX_LIST_LIMIT, bounded_text, truncate};

const MAX_FILTER_CHARS: usize = 200;
const MAX_CURSOR_CHARS: usize = 512;

pub static COMMAND: Command = Command {
    id: "feedback.list",
    path: &["feedback", "list"],
    contract: 2,
    summary: "The shared feedback backlog, as a difference since you last read it.",
    purpose: "\
Reads the same deduplicated backlog the `fb` tab shows, through the native \
signed-in user, without a selected project or Desktop. The backlog remembers \
how far this account has read, so a visit that changed nothing answers \
`changed: false` with no rows and no local file to keep; `--all` reads the \
whole thing regardless. Enumeration is complete — every matching report is \
counted in `total`, and `truncated` says only that `--limit` held rows back. \
Each row decides on its own: status, whether it is blocked and on what, \
whether anyone has left a note and the latest one, and the id and version a \
close must carry.",
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
            "cursor",
            "<token>",
            "Read the difference since this token the backlog issued.",
        ),
        Arg::switch(
            "all",
            "Read the whole backlog, ignoring how far this account has read.",
        ),
        crate::LANE_ARG,
    ],
    output: "\
`changed` (false means nothing moved since this account last read), `total`, \
`complete`, `truncated`, `cursor`, `cursor_source`, `backlog` counts on a full \
read, and `reports` rows with `id`, `status`, `kind`, `severity`, `component`, \
`surface`, `title`, `detail`, `detail_truncated`, `occurrences`, `reporters`, \
`resolution`, `version`, `blocked`, `blocked_on`, `note_count`, `latest_note`, \
`supersedes`, `superseded_by`, `last_seen_at` and `updated_by`. The `id` and \
`version` are what `ds feedback close` and `ds feedback note` take.",
    examples: &[Example {
        command: "ds feedback list --output json",
        note: "`.data.changed == false` means nothing moved; stop there instead of reading rows.",
        runnable: false,
    }],
    refusals: &crate::native_refusals::<
        7,
        { 7 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() },
    >([
        ds_cli_contract::args::INVALID_NUMBER,
        crate::INVALID_TEXT,
        crate::NOT_FOUND,
        crate::CONFLICT,
        crate::NOT_PERMITTED,
        crate::CURSOR_REJECTED,
        crate::BACKLOG_TOO_LARGE,
    ]),
    reference: Some("docs/reference/feedback.md"),
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
    // `--since` took an RFC3339 timestamp the CALLER had to remember, which
    // meant a local file on one machine and a full rescan everywhere else. The
    // watermark is the backlog's now: an opaque token it issued, or nothing at
    // all and it reads from where this account left off.
    if let Some(cursor) = inputs.value("cursor") {
        arguments.insert(
            "cursor".into(),
            json!(bounded_text(cursor, "cursor", MAX_CURSOR_CHARS)?),
        );
    }
    if inputs.switch("all") {
        arguments.insert("all".into(), json!(true));
    }
    crate::invoke_native(inputs, "list", arguments)
}

pub fn render(data: &Value) -> String {
    // The cheapest possible answer, and the one this command exists to make
    // cheap: nothing moved, so there is nothing to read.
    if data["changed"].as_bool() == Some(false) {
        return format!(
            "{} · unchanged since this account last read it\n",
            data["view"].as_str().unwrap_or("backlog"),
        );
    }
    let total = data["total"].as_u64().unwrap_or(0);
    let rows = data["reports"].as_array().map(Vec::len).unwrap_or(0);
    let mut out = format!(
        "{} · {}\n",
        data["view"].as_str().unwrap_or("backlog"),
        ds_cli_contract::args::plural(total, "report")
    );
    for report in data["reports"].as_array().into_iter().flatten() {
        let mark = if report["blocked"].as_bool().unwrap_or(false) {
            "blocked"
        } else {
            report["status"].as_str().unwrap_or("?")
        };
        out.push_str(&format!(
            "  {:<26} {:<12} {:<8} {:<20} {}\n",
            truncate(report["id"].as_str().unwrap_or("?"), 26),
            mark,
            report["severity"].as_str().unwrap_or("?"),
            truncate(report["component"].as_str().unwrap_or("—"), 20),
            truncate(report["title"].as_str().unwrap_or(""), 52),
        ));
        // The blocker and the last word are on the row on purpose: they are
        // what a reader would otherwise open the full report to discover.
        if let Some(blocked_on) = report["blocked_on"].as_str().filter(|s| !s.is_empty()) {
            out.push_str(&format!("      waits on {}\n", truncate(blocked_on, 72)));
        } else if let Some(note) = report["latest_note"]["text"].as_str() {
            out.push_str(&format!("      note: {}\n", truncate(note, 72)));
        }
    }
    if data["truncated"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "  … {rows} of {total} shown; narrow with --component or --query\n"
        ));
    }
    out
}
