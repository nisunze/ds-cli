//! `ds feedback list` — the shared backlog, as the `fb` tab reads it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
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
`changed: false` with no rows and no local file to keep. Enumeration is \
complete: a `--limit` that holds rows back holds them for the NEXT call, so \
listing again returns the next chunk until nothing is left — the backlog is \
read once in pieces, never re-read in full. Narrowing with `--component` or \
`--query` asks a different question, so it is answered in full and leaves the \
sweep where it was; `--all` reads the top the same way. \
Each row decides on its own: status, whether it is blocked and on what, the \
latest note, and the id and version a close must carry.",
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
            "Read the newest reports from the top, ignoring the watermark.",
        ),
        crate::LANE_ARG,
    ],
    output: "\
`changed` (false means nothing moved since this account last read), `total`, \
`complete`, `truncated`, `drains` (true when listing again returns the next \
chunk), `cursor`, `cursor_source`, `backlog` counts on a full read, \
`unreadable` when a stored report could not be decoded, and `reports` rows \
with `id`, `status`, `kind`, `severity`, `component`, \
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
        if data["drains"].as_bool().unwrap_or(false) {
            out.push_str(&format!(
                "  … {rows} of {total} shown; list again for the next chunk\n"
            ));
        } else {
            out.push_str(&format!(
                "  … {rows} of {total} shown; narrow with --component or --query\n"
            ));
        }
    }
    out.push_str(&unreadable);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A truncated answer means one of two opposite things, and the line under
    /// the rows is the only place a person learns which.
    ///
    /// A watermark read DRAINS: the rows it held back are the rows the next
    /// call delivers, so listing again finishes the backlog. Telling that
    /// caller to narrow instead would be the expensive mistake — a narrower
    /// question is a different question with its own watermark, so it reads
    /// from the top again and abandons the drain half-done. `--all` is the
    /// read where narrowing IS the remedy, because it always returns the top.
    #[test]
    fn a_truncated_drain_says_list_again_and_a_truncated_top_read_says_narrow() {
        let page = |drains: bool| {
            json!({
                "view": "not_addressed", "total": 45, "truncated": true, "drains": drains,
                "reports": [{"id":"fb_1","status":"open","severity":"major",
                             "component":"ds-brain","title":"list rescans the backlog"}]
            })
        };
        let drained = render(&page(true));
        assert!(
            drained.contains("list again for the next chunk"),
            "a drain must name the next call, not a narrower question: {drained}"
        );
        assert!(!drained.contains("narrow with"));

        let top = render(&page(false));
        assert!(
            top.contains("narrow with --component"),
            "a top read returns these same rows forever; narrowing is the remedy: {top}"
        );
        assert!(!top.contains("list again"));
    }

    /// The cheapest answer has no rows to read, and the row above it carries
    /// the blocker a reader would otherwise open the full report to find.
    #[test]
    fn nothing_changed_prints_one_line_and_a_blocked_row_names_its_blocker() {
        let quiet = render(&json!({"view":"not_addressed","changed":false,"reports":[]}));
        assert_eq!(quiet.lines().count(), 1, "{quiet}");
        assert!(quiet.contains("unchanged since this account last read it"));

        let blocked = render(&json!({
            "view":"not_addressed","total":1,"truncated":false,
            "reports":[{"id":"fb_1","status":"open","blocked":true,
                        "blocked_on":"deploy:ds-brain-canary","severity":"major",
                        "component":"ds-brain","title":"waits on a deploy"}]
        }));
        assert!(blocked.contains("blocked") && blocked.contains("waits on deploy:ds-brain-canary"));
    }

    /// The cheap answer is the one place a partial read could hide forever.
    ///
    /// A stored report the backlog cannot decode is left out AND passed over by
    /// the watermark, so it never appears in a later difference either. An
    /// unchanged read still enumerated the store, so it can carry that count —
    /// and printing one line and stopping would be exactly the silent partial
    /// answer `scan_incomplete` was removed for being, minus the admission.
    #[test]
    fn a_report_the_backlog_could_not_read_is_named_even_on_the_cheap_answer() {
        let quiet = render(&json!({
            "view":"not_addressed","changed":false,"reports":[],"unreadable":2
        }));
        assert!(
            quiet.contains("unchanged since this account last read it"),
            "{quiet}"
        );
        assert!(
            quiet.contains("2 stored reports could not be read"),
            "an unchanged answer still hides a document nobody can see: {quiet}"
        );

        let clean = render(&json!({"view":"not_addressed","changed":false,"reports":[]}));
        assert_eq!(
            clean.lines().count(),
            1,
            "a read with nothing wrong says nothing about a failure that did not happen: {clean}"
        );
    }
}
