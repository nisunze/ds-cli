//! `ds design known-columns list` — inspect the external property authority.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, KNOWN_COLUMNS_LIST};

pub static COMMAND: Command = Command {
    id: "design.known-columns.list",
    path: &["design", "known-columns", "list"],
    contract: 1,
    summary: "List the known columns that may leave DS.",
    purpose: "Reads the project's authoritative know_columns sheet and its optimistic revision. Internal properties and tag assignments remain in the model whether or not they appear here.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[DESCRIPTOR_ARG],
    output: "The active project, authority=know_columns, revision, and allowed property names by layer.",
    examples: &[Example {
        command: "ds design known-columns list",
        note: "An omitted tag field remains internal and is not emitted to external design surfaces.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        crate::NOT_PERMITTED,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &KNOWN_COLUMNS_LIST,
        json!({}),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} authority for {} · revision {}\n",
        data["authority"].as_str().unwrap_or("know_columns"),
        data["project"].as_str().unwrap_or("?"),
        data["revision"].as_i64().unwrap_or(0),
    );
    if let Some(columns) = data["columns"].as_object() {
        for (layer, fields) in columns {
            let fields = fields
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ");
            out.push_str(&format!("  {layer}: {fields}\n"));
        }
    }
    out
}
