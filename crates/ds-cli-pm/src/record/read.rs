//! `ds pm record read` — one record, with its body.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::project_management::reads;
use serde_json::{Value, json};

use crate::LANE_ARG;

const RECORD_ARG: Arg = Arg {
    name: "record",
    kind: ArgKind::Value,
    value: "<record-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The record, by the id `ds pm record list` reports.",
};

pub static COMMAND: Command = Command {
    id: "pm.record.read",
    path: &["pm", "record", "read"],
    contract: 1,
    summary: "Read one record with its body and what it touches.",
    purpose: "\
The whole record: what it is, which direction it travelled, what state it is \
in, whether a response is owed and by when, what it affects — scope, schedule, \
quality, cost — and which tasks, residuals and other records it references. \
The body is bounded, and a body that was cut says so rather than ending \
quietly. Headless: the selected project of the signed-in native credential, \
no window.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[RECORD_ARG, LANE_ARG],
    output: "\
`record` with its canonical fields and bounded `body`/related-id collections; \
each sets a truncation flag and reports its full count when cut.",
    examples: &[Example {
        command: "ds pm record read --record R-0031 --output json",
        note: "`.data.record.responseDueDate` is the date a reply is owed by.",
        runnable: false,
    }],
    refusals: &crate::read_refusals::<17>(&[crate::RECORD_NOT_FOUND]),
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
    let record_id = inputs.require("record")?;
    let (project, records, truncated) = crate::records(inputs.value("lane").unwrap_or("stable"))?;
    match reads::record_read(&project, &records, record_id) {
        Some(reply) => crate::data(&reply),
        None => Err(Failure::invalid(
            crate::RECORD_NOT_FOUND.code,
            format!("No record {record_id} in this project."),
        )
        .detail(json!({ "record": record_id, "project": project, "contextTruncated": truncated }))
        .remedy(crate::RECORD_NOT_FOUND.remedy)
        .next("ds pm record list")),
    }
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
