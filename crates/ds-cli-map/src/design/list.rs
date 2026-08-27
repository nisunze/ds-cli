//! `ds map design list` — the project's transformers, with processing state.
//!
//! The family's entry point when nothing else is known: `read`, `select`, and
//! every stage command need a transformer NAME, and until now only the
//! application's picker could supply one. Reads the same cached status list
//! the design Status surface renders; it saves, publishes and deletes
//! nothing.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

const LIMIT_ARG: Arg = Arg {
    name: "limit",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: Some("200"),
    choices: &[],
    summary: "Most transformers to return (1–1000). The total is always reported.",
};

pub static COMMAND: Command = Command {
    id: "map.design.list",
    path: &["map", "design", "list"],
    contract: 1,
    summary: "List the project's transformers with processing state.",
    purpose: "\
Names every transformer in the active project with its last process and \
report status, from the same cached status list the design Status surface \
renders. This is where a drafting session starts: every other `ds map design` \
command needs a transformer name from here.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[LIMIT_ARG, DESCRIPTOR_ARG],
    output: "\
The project, the total transformer count, and up to --limit rows of `name`, \
`processStatus`, `reportStatus`, and `locallyDirty` — true when staged local \
edits exist that `ds map design save` would push.",
    examples: &[Example {
        command: "ds map design list --output json",
        note: "Read .data.transformers[].name to feed the other design commands.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        super::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        ds_cli_contract::spec::Refusal {
            code: "invalid_limit",
            when: "--limit is not a whole number from 1 through 1000",
            remedy: "pass e.g. --limit 500",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    if let Some(limit) = inputs.value("limit") {
        let limit: u64 = limit.parse().map_err(|_| {
            Failure::invalid(
                "invalid_limit",
                "--limit must be a whole number from 1 through 1000",
            )
            .remedy("pass e.g. --limit 500")
        })?;
        arguments.insert("limit".into(), json!(limit));
    }

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_LIST,
        Value::Object(arguments),
        crate::DESIGN_READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;

    Ok(json!({
        "project": result["project"],
        "total": result["total"].as_u64().unwrap_or(0),
        "transformers": result["transformers"],
    }))
}

pub fn render(data: &Value) -> String {
    let total = data["total"].as_u64().unwrap_or(0);
    let mut out = format!(
        "{total} transformer(s) in {}\n",
        data["project"].as_str().unwrap_or("?")
    );
    if let Some(rows) = data["transformers"].as_array() {
        for row in rows {
            out.push_str(&format!(
                "  {:<40} process {:<10} report {:<10}{}\n",
                row["name"].as_str().unwrap_or("?"),
                row["processStatus"].as_str().unwrap_or("—"),
                row["reportStatus"].as_str().unwrap_or("—"),
                if row["locallyDirty"].as_bool().unwrap_or(false) {
                    "  · local edits"
                } else {
                    ""
                },
            ));
        }
        if (rows.len() as u64) < total {
            out.push_str(&format!(
                "  … {} more; raise --limit\n",
                total - rows.len() as u64
            ));
        }
    }
    out
}
