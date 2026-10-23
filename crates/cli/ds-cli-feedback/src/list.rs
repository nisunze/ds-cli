//! `ds feedback list` — the shared backlog, as the `fb` tab reads it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{MAX_LIST_LIMIT, bounded_text, truncate};

const MAX_FILTER_CHARS: usize = 200;
const MAX_CURSOR_BYTES: usize = ds_cli_auth::FeedbackCommand::MAX_CURSOR_BYTES;

/// The bound is declared by the placeholder, so the descriptor carries it as
/// `minimum`/`maximum`, and the same declaration is what `run` enforces.
const LIMIT: Arg = Arg::value(
    "limit",
    "<1-50>",
    "Rows to return. The matching total is always reported.",
)
.default("20");

pub static COMMAND: Command = Command {
    id: "feedback.list",
    path: &["feedback", "list"],
    contract: 2,
    summary: "The shared feedback backlog, as a difference since you last read it.",
    purpose: "\
Read the deduplicated `fb` backlog through the native signed-in user, without \
a project or Desktop. The API remembers this account's position; no changes \
returns `changed: false` with no rows or local state to keep. Enumeration is \
complete: --limit holds remaining rows for the NEXT call; repeat until empty. \
--component and --query are complete lookups that preserve the sweep position; \
--all peeks from the top without moving it. \
Each row decides on its own: status, whether it is blocked and on what, the \
latest note, and the id and version a close must carry.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "view",
            "<view>",
            "open: waiting on nothing; waiting: blocked on a named dependency.",
        )
        .choices(&["open", "waiting", "not_addressed", "addressed", "all"])
        .default("open"),
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
        LIMIT,
        Arg::switch(
            "detail",
            "Return each report's full detail instead of a bounded excerpt.",
        ),
        Arg::value(
            "cursor",
            "<token>",
            "Read the difference since this token (up to 4096 bytes); an --all read's token pages every match once, watermark untouched.",
        ),
        Arg::switch(
            "all",
            "Read the newest reports from the top, ignoring the watermark.",
        ),
        crate::LANE_ARG,
    ],
    output: "\
`changed` (false means nothing moved since this account last read), `total`, \
`complete`, `truncated`, `drains` (difference/cursor mode, not watermark \
mutation), `cursor`, `cursor_source`, `backlog` counts on a full read, \
`unreadable` when a stored report could not be decoded, and `reports` rows \
with `id`, `status`, `kind`, `severity`, `component`, \
`surface`, `title`, `detail`, `detail_truncated`, `occurrences`, `reporters`, \
`resolution`, `version`, `blocked`, `blocked_on`, `note_count`, `latest_note`, \
`supersedes`, `superseded_by`, `last_seen_at` and `updated_by`; with \
--detail, `replay` when the report carries one: `version`, `source_sha`, \
`channel`, `os`, `arch`, `command`, `argv` (`<redacted>` sensitive, \
`<unbound>` undeclared, `<elided>` over-long values), `redacted`, `elided` and \
`panic_location`, exactly what is needed to run it again. The `id` and \
`version` are what `ds feedback close` and `ds feedback note` take.",
    examples: &[Example {
        command: "ds feedback list --output json",
        note: "`.data.changed == false` means nothing moved; stop there instead of reading rows.",
        runnable: false,
    }],
    refusals: &crate::native_refusals::<
        7,
        { 7 + 1 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() },
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
        let (min, max) = LIMIT.range().unwrap_or((1, MAX_LIST_LIMIT));
        arguments.insert(
            "limit".into(),
            json!(ds_cli_contract::args::integer(limit, "limit", min, max)?),
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
        arguments.insert("cursor".into(), json!(bounded_cursor(cursor)?));
    }
    if inputs.switch("all") {
        arguments.insert("all".into(), json!(true));
    }
    crate::invoke_native(inputs, "list", arguments)
}

fn bounded_cursor(value: &str) -> Result<&str, Failure> {
    let value = bounded_text(value, "cursor", MAX_CURSOR_BYTES)?;
    if value.len() > MAX_CURSOR_BYTES {
        return Err(Failure::invalid(
            crate::INVALID_TEXT.code,
            format!("`--cursor` must be at most {MAX_CURSOR_BYTES} bytes"),
        )
        .remedy(crate::INVALID_TEXT.remedy));
    }
    Ok(value)
}

pub fn render(data: &Value) -> String {
    // A document the backlog could not decode is left out of the answer and
    // passed over by the watermark, so it is missing from every later
    // difference too. It rides on EVERY answer, including the cheap one: an
    // unchanged read still enumerated, so it is exactly where a silent skip
    // would hide the longest.
    let unreadable = match data["unreadable"].as_u64().filter(|count| *count > 0) {
        Some(count) => format!(
            "  ! {} could not be read and {} missing from this answer; \
             the backlog logged the ids\n",
            ds_cli_contract::args::plural(count, "stored report"),
            if count == 1 { "is" } else { "are" },
        ),
        None => String::new(),
    };
    // The cheapest possible answer, and the one this command exists to make
    // cheap: nothing moved, so there is nothing to read.
    if data["changed"].as_bool() == Some(false) {
        return format!(
            "{} · unchanged since this account last read it\n{unreadable}",
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
        // The same truncation means opposite things, and telling the caller to
        // narrow when it should list again is the costly mistake: a narrower
        // question is a DIFFERENT question, with its own watermark, so it
        // rescans from the top and abandons the drain half-finished.
        if data["cursor_source"].as_str() == Some("request") {
            out.push_str(&format!(
                "  … {rows} of {total} shown; continue with --cursor from this answer for the next chunk\n"
            ));
        } else if data["drains"].as_bool().unwrap_or(false) {
            out.push_str(&format!(
                "  … {rows} of {total} shown; list again for the next chunk\n"
            ));
        } else {
            out.push_str(&format!(
                "  … {rows} of {total} shown; continue with --cursor, or narrow with --component or --query\n"
            ));
        }
    }
    out.push_str(&unreadable);
    out
}

#[cfg(test)]
#[path = "list_tests.rs"]
mod tests;
