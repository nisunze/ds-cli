//! `ds pm record list` — what has been said, sent, asked and decided.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_management::reads::{self, RecordListFilter};
use serde_json::Value;

use crate::{LANE_ARG, LIMIT_ARG, PAGE_ARG};

const QUERY_ARG: Arg = Arg {
    name: "query",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Match the subject, body, category or the message it came from.",
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

pub static COMMAND: Command = Command {
    id: "pm.record.list",
    path: &["pm", "record", "list"],
    contract: 1,
    summary: "List the project's records, newest first.",
    purpose: "\
The correspondence layer of Project Work: instructions, requests for \
information, submissions, reviews, decisions and field records, newest first, \
with what each one is waiting on. Bodies are not returned here — one row is a \
subject line, and `ds pm record read` opens the one you chose. Headless: the \
selected project of the signed-in native credential, no window. The server \
answers at most 100 records per read; a project past that lists with \
`truncated: true`.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[QUERY_ARG, CATEGORY_ARG, LIMIT_ARG, PAGE_ARG, LANE_ARG],
    output: "\
The project, the matched total, the page bounds, `truncated` when the server \
cut the context this list was folded from, and rows of `id`, `category`, \
`state`, `direction`, `channel`, `subject`, `happenedAt`, `responseRequired`, \
`responseDueDate`, `responseStatus`, `threadId` and the count of tasks each \
record touches.",
    examples: &[Example {
        command: "ds pm record list --category request_for_information --output json",
        note: "Read .data.records[].id to open one with `ds pm record read`.",
        runnable: false,
    }],
    refusals: &crate::read_refusals::<17>(&[crate::INVALID_NUMBER]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "correspondence",
        "rfi",
        "instruction",
        "submission",
        "decision",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let filter = RecordListFilter {
        query: inputs.value("query").unwrap_or_default().to_owned(),
        category: inputs.value("category").map(str::to_owned),
        page_size: match inputs.value("limit") {
            Some(limit) => crate::integer(limit, "limit", 1, crate::MAX_PAGE_SIZE)?,
            None => 50,
        },
        page: match inputs.value("page") {
            Some(page) => crate::integer(page, "page", 0, 10_000)?,
            None => 0,
        },
    };
    let (project, records, truncated) = crate::records(inputs.value("lane").unwrap_or("stable"))?;
    crate::data(&reads::record_list(&project, &records, truncated, &filter))
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
                if row["responseRequired"].as_bool().unwrap_or(false) {
                    "  · reply due"
                } else {
                    ""
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
