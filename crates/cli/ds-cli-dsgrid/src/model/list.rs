//! `ds dsgrid model list` — every working copy this machine holds, and which
//! one is open.
//!
//! The entry point for the whole family: `set-active` and `publish-version`
//! both need an opaque local model id, and until 2026-09-18 only the
//! application's own Grid Models panel could supply one — so the id an
//! engineer needed on a server could only be read on a desktop.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Availability, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::model::{DEFAULT_LIST_LIMIT, MAX_LIST_LIMIT};

const LIMIT_ARG: Arg = Arg {
    name: "limit",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: Some(DEFAULT_LIST_LIMIT),
    choices: &[],
    summary: "Rows in one page (1-500). The total is always reported.",
};

pub static COMMAND: Command = Command {
    id: "dsgrid.model.list",
    path: &["dsgrid", "model", "list"],
    contract: 1,
    summary: "List this machine's DS Grid working copies and which one is open.",
    purpose: "\
Names every DS Grid working copy this machine holds — created empty, imported \
from a file, or taken from a project's governed head — with the opaque id \
every other command in this family needs, and says which one an editing \
session opens. A working copy is a fact about a machine, so this needs no \
sign-in, no project and no application: a machine that has never held one \
answers with an empty catalogue rather than a refusal.",
    chapter: Chapter::GridModel,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        LIMIT_ARG,
        crate::model::workspace::LANE_ARG,
        crate::model::workspace::ACCOUNT_ARG,
    ],
    output: "\
`active_model`, the matched `total`, `more` when the page was cut, and rows of \
`model`, `name`, `active`, `origin`, `crs`, `revision`, `size_bytes`, \
`content_digest`, `created_at`, `head_revision`, — for a copy taken from a \
project — the `project_binding` it came from, and — for a copy linked to a \
PLS-CADD workspace — its `pls_source` {path, digest, pls_version, \
member_versions}. Never model content.",
    examples: &[Example {
        command: "ds dsgrid model list --output json",
        note: "Read .data.models[].model to feed set-active or publish-version.",
        runnable: false,
    }],
    refusals: crate::model::workspace::REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = crate::model::integer(
        inputs.value("limit").unwrap_or(DEFAULT_LIST_LIMIT),
        "limit",
        1,
        MAX_LIST_LIMIT,
    )? as usize;
    let catalogue = crate::model::workspace::read(inputs)?;
    let active = catalogue.active.as_deref();
    let total = catalogue.models.len();
    // Newest first, which is the order an operator reads a working set in.
    let mut rows: Vec<&ds_command_kernel::local_models::LocalModel> =
        catalogue.models.iter().collect();
    rows.reverse();
    let page: Vec<Value> = rows
        .iter()
        .take(limit)
        .map(|model| crate::model::workspace::row(model, active))
        .collect();
    Ok(json!({
        "lane": catalogue.scope.lane,
        "account": catalogue.scope.uid,
        "active_model": active,
        "total": total,
        "more": total > page.len(),
        "models": page,
    }))
}

pub fn render(data: &Value) -> String {
    let total = data["total"].as_u64().unwrap_or(0);
    let active = data["active_model"].as_str().unwrap_or("");
    let mut out = format!(
        "{} local · active {}\n",
        crate::model::plural(total, "DS Grid model"),
        if active.is_empty() { "none" } else { active },
    );
    let rows = data["models"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    for row in rows {
        out.push_str(&crate::model::model_line(row, active));
    }
    if data["more"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "  … {} more; raise --limit\n",
            total.saturating_sub(rows.len() as u64)
        ));
    }
    out
}
