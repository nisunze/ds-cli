//! `ds pm record list` — what has been said, sent, asked and decided.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::{Action, RecordFilters};
use serde_json::Value;

use crate::LANE_ARG;

const QUERY_ARG: Arg = Arg {
    name: "query",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Match the subject, reference or body.",
};

const CATEGORY_ARG: Arg = Arg {
    name: "category",
    kind: ArgKind::Value,
    value: "<category>",
    required: false,
    default: None,
    choices: &[],
    summary: "Only this category, e.g. instruction or request_for_information.",
};
const THREAD_ARG: Arg = Arg::value("thread", "<record-id>", "Only the records of this thread.");
const DIRECTION_ARG: Arg = Arg::value(
    "direction",
    "<inbound|outbound|internal>",
    "Only this direction.",
);
const CHANNEL_ARG: Arg = Arg::value(
    "channel",
    "<channel>",
    "Only this channel, e.g. email or letter.",
);
const PARTY_ARG: Arg = Arg::value(
    "party",
    "<party-id>",
    "Only records this party is on, or owes the answer to.",
);
const AWAITING_ARG: Arg = Arg::switch(
    "awaiting",
    "Only records whose answer is still owed (outstanding or overdue).",
);
const OVERDUE_ARG: Arg = Arg::switch("overdue", "Only records whose answer is past its due day.");
const SINCE_ARG: Arg = Arg::value(
    "since",
    "<yyyy-mm-dd | instant>",
    "Only records that happened at or after this day or instant.",
);
const LIMIT_ARG: Arg = Arg::value(
    "limit",
    "<count>",
    "Rows in one page (1-100). The total is always reported.",
)
.default("50");
const PAGE_ARG: Arg =
    Arg::value("page", "<index>", "Zero-based page of the bounded result.").default("0");

pub static COMMAND: Command = Command {
    id: "pm.record.list",
    path: &["pm", "record", "list"],
    contract: 1,
    summary: "List records newest first; by thread, party, or what is still owed.",
    purpose: "\
The correspondence layer of Project Management: instructions, requests for \
information, submissions, reviews, decisions, letters and field records, \
newest first, with what each one is waiting on and who owes it. `--awaiting` \
answers \"who owes us an answer\" and `--overdue` \"what is late\"; `--party` \
narrows to one counterparty and `--thread` to one exchange. Bodies are not \
returned here — one row is a subject line, and `ds pm record read` opens \
the one you chose. Headless: the named project of the signed-in native \
credential, no window. The server scans at most 1000 records; past that \
the list says `truncated: true`.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        QUERY_ARG,
        CATEGORY_ARG,
        THREAD_ARG,
        DIRECTION_ARG,
        CHANNEL_ARG,
        PARTY_ARG,
        AWAITING_ARG,
        OVERDUE_ARG,
        SINCE_ARG,
        LIMIT_ARG,
        PAGE_ARG,
        LANE_ARG,
        crate::PROJECT_ARG,
    ],
    output: "\
The project, the matched `total`, the page bounds, `scanned`, `truncated` \
when the server stopped at its scan cap, `asOf`, and rows of `id`, \
`category`, `state`, `direction`, `channel`, `subject`, `happenedAt`, \
`responseRequired`, `responseDueDate`, `responseStatus`, \
`responseOwnerPartyId`/`responseOwnerId`, `partyIds`, `sensitivity`, \
`threadId` and the count of tasks each record touches.",
    examples: &[Example {
        command: "ds pm record list --awaiting --output json --project <exact-id>",
        note: "Every record whose answer is still owed; .data.records[].responseOwnerPartyId says by whom.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<23>(&[crate::INVALID_NUMBER, crate::INVALID_STAMP]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "project management",
        "correspondence",
        "letter",
        "email",
        "rfi",
        "instruction",
        "submission",
        "decision",
        "who owes",
        "ball court",
        "overdue",
        "outstanding",
        "awaiting",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = match inputs.value("limit") {
        Some(limit) => crate::integer(limit, "limit", 1, 100)?,
        None => 50,
    };
    let page = match inputs.value("page") {
        Some(page) => crate::integer(page, "page", 0, 10_000)?,
        None => 0,
    };
    let since = inputs
        .value("since")
        .map(|raw| crate::stamp(raw, "since"))
        .transpose()?;
    let report = crate::correspondence(
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
        &Action::RecordList(RecordFilters {
            query: inputs.value("query").map(str::to_owned),
            category: inputs.value("category").map(str::to_owned),
            thread_id: inputs.value("thread").map(str::to_owned),
            direction: inputs.value("direction").map(str::to_owned),
            channel: inputs.value("channel").map(str::to_owned),
            party_id: inputs.value("party").map(str::to_owned),
            awaiting: inputs.switch("awaiting"),
            overdue: inputs.switch("overdue"),
            since,
            limit: Some(limit),
            page: Some(page + 1),
        }),
    )?;
    let project = report.project_id().to_owned();
    crate::data(
        &ds_command_kernel::project_management::correspondence::record_page(
            &project,
            &report.into_result(),
        ),
    )
}

pub fn render(data: &Value) -> String {
    let total = data["total"].as_u64().unwrap_or(0);
    let mut out = format!(
        "{} in {}\n",
        crate::plural(total, "record"),
        data["project"].as_str().unwrap_or("?"),
    );
    if let Some(rows) = data["records"].as_array() {
        for row in rows {
            out.push_str(&format!(
                "  {:<20} {:<10} {:<24} {:<40}{}\n",
                crate::truncate(row["id"].as_str().unwrap_or("?"), 20),
                row["happenedAt"]
                    .as_str()
                    .unwrap_or("—")
                    .get(..10)
                    .unwrap_or("—"),
                crate::truncate(row["category"].as_str().unwrap_or("—"), 24),
                crate::truncate(row["subject"].as_str().unwrap_or("(no subject)"), 40),
                match row["responseStatus"].as_str() {
                    Some("outstanding") => "  · answer owed",
                    Some("overdue") => "  · answer OVERDUE",
                    Some("responded") => "  · answered",
                    Some("waived") => "  · waived",
                    _ => "",
                },
            ));
        }
        let through = data["to"].as_u64().unwrap_or(rows.len() as u64);
        if through < total {
            out.push_str(&format!(
                "  … {} more; raise --limit or ask for --page {}\n",
                total - through,
                data["page"].as_u64().unwrap_or(0) + 1,
            ));
        }
    }
    out
}
