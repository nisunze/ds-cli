//! `ds work record list` — what has been said, sent, asked and decided.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, ArgKind, Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, LIMIT_ARG, PAGE_ARG};

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
    id: "work.record.list",
    path: &["work", "record", "list"],
    contract: 1,
    summary: "List the project's records, newest first.",
    purpose: "\
The correspondence layer of Project Work: instructions, requests for \
information, submissions, reviews, decisions and field records, newest first, \
with what each one is waiting on. Bodies are not returned here — one row is a \
subject line, and `ds work record read` opens the one you chose.",
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[QUERY_ARG, CATEGORY_ARG, LIMIT_ARG, PAGE_ARG, DESCRIPTOR_ARG],
    output: "\
The project, the matched total, the page bounds, and rows of `id`, `category`, \
`state`, `direction`, `subject`, `happenedAt`, `responseRequired`, \
`responseDueDate` and the count of tasks each record touches.",
    examples: &[Example {
        command: "ds work record list --category request_for_information --output json",
        note: "Read .data.records[].id to open one with `ds work record read`.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::WORK_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/work.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    for flag in ["query", "category"] {
        if let Some(value) = inputs.value(flag) {
            arguments.insert(flag.into(), json!(value));
        }
    }
    if let Some(limit) = inputs.value("limit") {
        arguments.insert(
            "limit".into(),
            json!(crate::integer(limit, "limit", 1, crate::MAX_PAGE_SIZE)?),
        );
    }
    if let Some(page) = inputs.value("page") {
        arguments.insert(
            "page".into(),
            json!(crate::integer(page, "page", 0, 10_000)?),
        );
    }

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::RECORDS_LIST,
        Value::Object(arguments),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_work_failure)
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
