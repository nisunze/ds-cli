//! `ds sre events` — bounded request-event investigation.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

const DAYS: Arg = Arg::value("days", "<n>", "Newest event window in days; 1..365.").default("3");
const LIMIT: Arg = Arg::value("limit", "<n>", "Matching events to return; 1..250.").default("50");
const SCAN_LIMIT: Arg = Arg::value(
    "scan-limit",
    "<n>",
    "Newest owner events to scan before filtering; 1..5000.",
)
.default("1000");
const SERVICE: Arg = Arg::value("service", "<name>", "Match one exact service name.");
const OUTCOME: Arg = Arg::value(
    "outcome",
    "<failure|success|all>",
    "Match failures, successes, or both.",
)
.choices(&["failure", "success", "all"])
.default("failure");
const CATEGORY: Arg = Arg::value("category", "<name>", "Match one exact error category.");
/// The lane an EVENT was recorded on, which is not the lane this machine signs
/// in to. `--lane` means the caller's credential lane everywhere else in `ds`,
/// and it means that here too, so the filter carries its own name: a caller on
/// stable credentials may well be reading canary's errors.
const EVENT_LANE: Arg = Arg::value(
    "event-lane",
    "<name>",
    "Match one exact deployment lane the event was recorded on.",
);
const ACTION: Arg = Arg::value("action", "<name>", "Match one exact action name.");
const PROJECT: Arg = Arg::value(
    "project",
    "<id>",
    "Filter event metadata by project; this does not select or require an active project.",
);
const SOURCE: Arg = Arg::value("source", "<name>", "Match one exact event source.");

pub static COMMAND: Command = Command {
    id: "sre.events",
    path: &["sre", "events"],
    contract: 1,
    summary: "Read and filter a bounded newest-first request-event window.",
    purpose: "\
Investigate recent errors in diagnostic request-event logs, or include \
successful requests when needed. The read scans a bounded newest-first \
Reliability event window under this machine's restored native user, applies \
exact case-insensitive filters, and returns a bounded projection. This is a \
global read: --project filters event metadata and never selects a project. A \
window that lost rows on the way is refused, not returned short.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        DAYS,
        LIMIT,
        SCAN_LIMIT,
        SERVICE,
        OUTCOME,
        CATEGORY,
        EVENT_LANE,
        ACTION,
        PROJECT,
        SOURCE,
        crate::LANE_ARG,
    ],
    output: "\
`generated_at`, `window_days`, `scan_limit`, applied `filters`, `scanned`, \
`matching`, `returned`, and bounded `events`; `more.matching` reports omitted \
matches and `more.scan` reports a saturated owner scan. Each event's \
`truncated_fields` names projected text clipped to its declared byte-safe bound.",
    examples: &[Example {
        command: "ds sre events --service ds-brain --category timeout --output json",
        note: "Defaults to failures from the last three days, returning at most 50.",
        runnable: false,
    }],
    refusals: &crate::native_refusals::<
        4,
        { 4 + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() },
    >([
        crate::INVALID_NUMBER,
        crate::INVALID_TEXT,
        crate::NOT_PERMITTED,
        crate::UNREADABLE,
    ]),
    reference: Some("docs/reference/sre.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // Every flag is validated before the read is opened, so an input error is
    // the same answer on CI, on a server and on a machine with no credential.
    let days = crate::integer(inputs.require("days")?, "days", 1, crate::MAX_DAYS)?;
    let limit = crate::integer(inputs.require("limit")?, "limit", 1, crate::MAX_EVENTS)?;
    let scan_limit = crate::integer(
        inputs.require("scan-limit")?,
        "scan-limit",
        1,
        crate::MAX_SCAN_EVENTS,
    )?;
    let outcome = ds_client_core::sre::Outcome::parse(inputs.require("outcome")?)
        .map_err(|_| unknown_outcome())?;

    // Each filter is one exact value, bounded and named by its own flag.
    let filter = |flag: &str| -> Result<Option<String>, Failure> {
        match inputs.value(flag) {
            None => Ok(None),
            Some(value) => Ok(Some(crate::bounded_filter(value, flag)?.to_string())),
        }
    };
    let filters = ds_client_core::sre::Filters {
        service: filter("service")?,
        category: filter("category")?,
        lane: filter("event-lane")?,
        action: filter("action")?,
        project: filter("project")?,
        source: filter("source")?,
    };

    crate::invoke_native(
        inputs,
        &ds_client_core::sre::Command::Events {
            days,
            limit,
            scan_limit,
            outcome,
            filters,
        },
    )
}

fn unknown_outcome() -> Failure {
    Failure::invalid(
        "invalid_text",
        "`--outcome` must be failure, success or all",
    )
    .remedy("pass one of failure, success or all")
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} of {} matching events returned ({} scanned over {} days)\n",
        data["returned"].as_u64().unwrap_or(0),
        data["matching"].as_u64().unwrap_or(0),
        data["scanned"].as_u64().unwrap_or(0),
        data["window_days"].as_u64().unwrap_or(0),
    );
    for event in data["events"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<24} {:<18} {:<9} {}\n",
            crate::truncate(event["ts"].as_str().unwrap_or("pending"), 24),
            crate::truncate(event["service"].as_str().unwrap_or("?"), 18),
            event["outcome"].as_str().unwrap_or("?"),
            crate::truncate(
                event["error_category"]
                    .as_str()
                    .or_else(|| event["action"].as_str())
                    .unwrap_or("?"),
                48,
            ),
        ));
    }
    if data["more"]["matching"].as_bool().unwrap_or(false) {
        out.push_str("  more matching events omitted; raise --limit within its bound\n");
    }
    if data["more"]["scan"].as_bool().unwrap_or(false) {
        out.push_str("  scan saturated; narrow filters or raise --scan-limit\n");
    }
    out
}
