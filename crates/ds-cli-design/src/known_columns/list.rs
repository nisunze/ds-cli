//! `ds design known-columns list` — inspect the external property authority.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{LANE_ARG, PROJECT_ARG};

pub static COMMAND: Command = Command {
    id: "design.known-columns.list",
    path: &["design", "known-columns", "list"],
    contract: 1,
    summary: "List the known columns that may leave DS.",
    purpose: "Reads the project's authoritative know_columns sheet and its optimistic revision. Internal properties and tag assignments remain in the model whether or not they appear here.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, LANE_ARG],
    output: "The active project, authority=know_columns, revision, and allowed property names by layer.",
    examples: &[Example {
        command: "ds design known-columns list --project <id>",
        note: "An omitted tag field remains internal and is not emitted to external design surfaces.",
        runnable: false,
    }],
    refusals: &crate::headless_refusals!(
        ds_cli_auth::DESIGN_ROUTE_UNAVAILABLE_REFUSAL,
        crate::NOT_PERMITTED,
    ),
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    crate::headless::perform(
        "design.known-columns.list",
        json!({}),
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
    )
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
