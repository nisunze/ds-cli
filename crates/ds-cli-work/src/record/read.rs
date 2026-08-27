//! `ds work record read` — one record, with its body.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

const RECORD_ARG: Arg = Arg {
    name: "record",
    kind: ArgKind::Value,
    value: "<record-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The record, by the id `ds work record list` reports.",
};

pub static COMMAND: Command = Command {
    id: "work.record.read",
    path: &["work", "record", "read"],
    contract: 1,
    summary: "Read one record with its body and what it touches.",
    purpose: "\
The whole record: what it is, which direction it travelled, what state it is \
in, whether a response is owed and by when, what it affects — scope, schedule, \
quality, cost — and which tasks, residuals and other records it references. \
The body is bounded, and a body that was cut says so rather than ending \
quietly.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[RECORD_ARG, DESCRIPTOR_ARG],
    output: "\
`record` with its canonical fields and bounded `body`/related-id collections; \
each sets a truncation flag and reports its full count when cut.",
    examples: &[Example {
        command: "ds work record read --record R-0031 --output json",
        note: "`.data.record.responseDueDate` is the date a reply is owed by.",
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
    ],
    reference: Some("docs/reference/work.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::RECORDS_READ,
        json!({ "record": inputs.require("record")? }),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_work_failure)
}

pub fn render(data: &Value) -> String {
    let record = &data["record"];
    let mut out = format!(
        "{}\n  {} · {} · {} · {}\n",
        record["subject"].as_str().unwrap_or("(no subject)"),
        record["category"].as_str().unwrap_or("—"),
        record["direction"].as_str().unwrap_or("—"),
        record["state"].as_str().unwrap_or("—"),
        record["happenedAt"].as_str().unwrap_or("—"),
    );
    if record["responseRequired"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "  reply owed{}\n",
            record["responseDueDate"]
                .as_str()
                .map(|due| format!(" by {due}"))
                .unwrap_or_default(),
        ));
    }
    if let Some(body) = record["body"].as_str().filter(|text| !text.is_empty()) {
        out.push('\n');
        out.push_str(body);
        out.push('\n');
        if record["bodyTruncated"].as_bool().unwrap_or(false) {
            out.push_str("… body cut to its bound; open the record in the app for the rest\n");
        }
    }
    let related = record["relatedTaskIds"].as_array().map_or(0, Vec::len);
    if related > 0 {
        out.push_str(&format!(
            "\n{}\n",
            crate::plural(related as u64, "linked task")
        ));
    }
    out
}
