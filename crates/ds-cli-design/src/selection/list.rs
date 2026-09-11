//! `ds design selection list` — the project's saved selections.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::{DesignSelectionAnswer, DesignSelectionRequest};
use serde_json::{Value, json};

use super::LANE;
use crate::LIMIT_ARG;

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
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[ARCHIVED_ARG, LIMIT_ARG, LANE],
    output: "\
The project, the matched total, whether more exist, and rows of `selection`, \
`name`, `mode`, `version`, `state`, `members` (null for a query selection, \
whose membership is evaluated on read) and `assignments`.",
    examples: &[Example {
        command: "ds design selection list --output json",
        note: "Read .data.selections[].selection to feed read, archive or assign.",
        runnable: false,
    }],
    refusals: super::REFUSALS,
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // The page's own bound, applied to the page's own answer: ds-brain returns
    // every selection and the listing is paged here, exactly as the register
    // pages it.
    let limit = match inputs.value("limit") {
        Some(value) => crate::integer(value, "limit", 1, crate::MAX_PAGE_SIZE)? as usize,
        None => 50,
    };
    let (project, answer) = super::ask(
        inputs.require("lane")?,
        "",
        &DesignSelectionRequest::List {
            include_archived: inputs.switch("archived"),
        },
    )?;
    let DesignSelectionAnswer::List(rows) = answer else {
        return Err(Failure::unavailable(
            "auth_response_unreadable",
            "the saved-selection listing did not match its closed contract",
        ));
    };
    let total = rows.len();
    let listed: Vec<Value> = rows
        .iter()
        .take(limit)
        .map(|row| {
            json!({
                "selection": row.selection_id,
                "name": row.name,
                "mode": row.mode,
                "version": row.version,
                "state": row.state,
                // `null` for a query selection: its membership is evaluated on
                // read, and 0 would read as "empty".
                "members": row.members,
                "assignments": row.assignments,
            })
        })
        .collect();
    Ok(json!({
        "project": project,
        "total": total,
        "more": total > listed.len(),
        "selections": listed,
    }))
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
