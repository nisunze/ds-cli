//! `ds design attachment list-project` — every file in the project, paged.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

const LIMIT_ARG: Arg = Arg::value(
    "limit",
    "<count>",
    "Files in one page (1-500). `more` and `next_cursor` report the rest.",
)
.default("50");
const CURSOR_ARG: Arg = Arg::value(
    "cursor",
    "<cursor>",
    "Exact next_cursor from the previous page.",
);
const ARCHIVED_ARG: Arg = Arg::switch("archived", "Include retired files.");

pub static COMMAND: Command = Command {
    id: "design.attachment.list-project",
    path: &["design", "attachment", "list-project"],
    contract: 1,
    summary: "List every attached file in a project, paged.",
    purpose: "Page through every file head in the explicit project, newest change first, whatever object it is anchored to. Heads only — the object, latest pointer, file name and version fence; read one file's revisions with `show`. Retired files are filtered after the page is cut, so a page may be short while `more` is still true.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        LIMIT_ARG,
        CURSOR_ARG,
        ARCHIVED_ARG,
        crate::versions::PROJECT,
        crate::transformer::LANE_ARG,
    ],
    output: "The project, `total` heads on this page, the `attachments` heads, `more`, and `next_cursor` when there is another page.",
    examples: &[Example {
        command: "ds design attachment list-project --project <id> --limit 100 --output json",
        note: "Pass .data.next_cursor as --cursor while .data.more is true.",
        runnable: false,
    }],
    refusals: &[super::NATIVE_REFUSED, crate::INVALID_NUMBER],
    reference: Some("docs/reference/design.md"),
    search: &["project attachments", "all attachments"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = crate::integer(
        inputs.require("limit")?,
        "limit",
        1,
        i64::from(ds_client_core::design_attachments::MAX_PROJECT_PAGE),
    )?;
    super::ask(
        inputs,
        ds_client_core::design_attachments::Command::ListProject {
            archived: inputs.switch("archived"),
            limit: limit as u16,
            cursor: inputs.value("cursor").map(str::to_owned),
        },
    )
}

pub fn render(data: &Value) -> String {
    let mut out = String::new();
    for head in data["attachments"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{} · {} {} · {} · latest {}\n",
            head["attachment_id"].as_str().unwrap_or("?"),
            head["object"]["kind"].as_str().unwrap_or("?"),
            head["object"]["id"].as_str().unwrap_or("?"),
            head["label"].as_str().unwrap_or("?"),
            head["latest_revision_id"].as_str().unwrap_or("none"),
        ));
    }
    if data["more"] == true {
        out.push_str(&format!(
            "more: --cursor {}\n",
            data["next_cursor"].as_str().unwrap_or("?")
        ));
    }
    if out.is_empty() {
        out.push_str("no attached files\n");
    }
    out
}
