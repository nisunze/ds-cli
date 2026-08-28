//! `ds design comment list` — the threads on one design object.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, KIND_ARG, OBJECT_ARG, VERSION_ARG};

const RESOLVED_ARG: Arg = Arg {
    name: "resolved",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Include resolved threads, which are hidden by default.",
};

pub static COMMAND: Command = Command {
    id: "design.comment.list",
    path: &["design", "comment", "list"],
    contract: 1,
    summary: "List the comment threads on a transformer or DS Grid model.",
    purpose: "\
Names every open thread on the object with its title, comment count, version, \
its optional feature anchor and the project work task it was promoted to, if \
any. Pass --version to see only the threads anchored to one exact object \
version. `ds design comment read` opens one; every write in this family needs \
an id from here.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        KIND_ARG,
        OBJECT_ARG,
        VERSION_ARG,
        RESOLVED_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The project, the anchored object, the total, and rows of `thread`, `title`, `state`, `comments`, `version`, `anchor` and `task`.",
    examples: &[Example {
        command: "ds design comment list --kind mv_model --object mv_line_a --resolved --output json",
        note: "Read .data.threads[].thread to open one with `ds design comment read`.",
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
        crate::INVALID_ANCHOR,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = crate::anchor(inputs)?;
    if inputs.switch("resolved") {
        arguments.insert("resolved".into(), json!(true));
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::COMMENT_LIST,
        Value::Object(arguments),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let total = data["total"].as_u64().unwrap_or(0);
    let mut out = format!(
        "{} on {} in {}\n",
        crate::plural(total, "thread"),
        data["object"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
    );
    for row in data["threads"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {} · {} · {} · {}{}\n",
            row["thread"].as_str().unwrap_or("?"),
            row["title"].as_str().unwrap_or("?"),
            row["state"].as_str().unwrap_or("?"),
            crate::plural(row["comments"].as_u64().unwrap_or(0), "comment"),
            row["task"]
                .as_str()
                .map(|task| format!(" · task {task}"))
                .unwrap_or_default(),
        ));
    }
    out
}
