//! `ds design selection list` — the project's saved selections.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, LIMIT_ARG};

const ARCHIVED_ARG: Arg = Arg {
    name: "archived",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Include archived selections, which are hidden by default.",
};

pub static COMMAND: Command = Command {
    id: "design.selection.list",
    path: &["design", "selection", "list"],
    contract: 1,
    summary: "List the project's saved Transformer Status selections.",
    purpose: "\
Names every saved selection in the active project with its membership mode, \
version and how many times it has scoped work. This is where a selection \
session starts: read, save, archive and assign all need an id from here. \
Membership itself is not evaluated by a listing — `ds design selection read` \
does that, because evaluating every selection to list them would cost a \
project-wide read per row.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[ARCHIVED_ARG, LIMIT_ARG, DESCRIPTOR_ARG],
    output: "\
The project, the matched total, whether more exist, and rows of `selection`, \
`name`, `mode`, `version`, `state`, `members` (null for a query selection, \
whose membership is evaluated on read) and `assignments`.",
    examples: &[Example {
        command: "ds design selection list --output json",
        note: "Read .data.selections[].selection to feed read, archive or assign.",
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
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    if inputs.switch("archived") {
        arguments.insert("archived".into(), json!(true));
    }
    if let Some(limit) = inputs.value("limit") {
        arguments.insert(
            "limit".into(),
            json!(crate::integer(limit, "limit", 1, crate::MAX_PAGE_SIZE)?),
        );
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::SELECTION_LIST,
        Value::Object(arguments),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let total = data["total"].as_u64().unwrap_or(0);
    let mut out = format!(
        "{} in {}\n",
        crate::plural(total, "saved selection"),
        data["project"].as_str().unwrap_or("?"),
    );
    for row in data["selections"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {} · {} · v{} · {}{}\n",
            row["selection"].as_str().unwrap_or("?"),
            row["name"].as_str().unwrap_or("?"),
            row["version"].as_u64().unwrap_or(0),
            match row["mode"].as_str() {
                Some("query") => "query".to_string(),
                _ => crate::plural(row["members"].as_u64().unwrap_or(0), "transformer"),
            },
            if row["state"].as_str() == Some("archived") {
                " · archived"
            } else {
                ""
            },
        ));
    }
    if data["more"].as_bool() == Some(true) {
        out.push_str("  … more exist; raise --limit\n");
    }
    out
}
