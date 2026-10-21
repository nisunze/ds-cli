//! `ds design attachment show` — one file, its pointer and its revisions.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::attachment::download::ATTACHMENT_ARG;

pub const ARCHIVED_ARG: Arg = Arg {
    name: "archived",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Include retired revisions.",
};

pub static COMMAND: Command = Command {
    id: "design.attachment.show",
    path: &["design", "attachment", "show"],
    contract: 1,
    summary: "Read one attachment's head, latest pointer and revisions.",
    purpose: "One file by attachment_id in the explicit project: its head (state, latest pointer, version fence) and up to 200 revisions newest first, each with its object version binding, digest and size. Read the fence here before set-latest or retire.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        ATTACHMENT_ARG,
        ARCHIVED_ARG,
        crate::versions::PROJECT,
        crate::transformer::LANE_ARG,
    ],
    output: "The project, the `attachment` head (state, latest_revision_id, version), its `revisions` rows, and `truncated` when more than 200 exist.",
    examples: &[Example {
        command: "ds design attachment show --project <id> --attachment att-site-a-bak --archived --output json",
        note: "Read .data.attachment.version for the fence and .data.revisions[].object.version_id for each binding.",
        runnable: false,
    }],
    refusals: &[super::NATIVE_REFUSED],
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    super::ask(
        inputs,
        ds_client_core::design_attachments::Command::Show {
            attachment: inputs.require("attachment")?.into(),
            archived: inputs.switch("archived"),
        },
    )
}

pub fn render(data: &Value) -> String {
    let head = &data["attachment"];
    let mut out = format!(
        "{} · {} · {} · latest {} · v{}\n",
        head["attachment_id"].as_str().unwrap_or("?"),
        head["label"].as_str().unwrap_or("?"),
        head["state"].as_str().unwrap_or("?"),
        head["latest_revision_id"].as_str().unwrap_or("none"),
        head["version"].as_u64().unwrap_or(0),
    );
    for revision in data["revisions"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  r{} {} · {} · {} bytes{}\n",
            revision["revision"].as_u64().unwrap_or(0),
            revision["revision_id"].as_str().unwrap_or("?"),
            revision["object"]["version_id"]
                .as_str()
                .unwrap_or("unpinned"),
            revision["byte_size"].as_u64().unwrap_or(0),
            if revision["deleted_at"]
                .as_str()
                .is_some_and(|at| !at.is_empty())
            {
                " · retired"
            } else {
                ""
            },
        ));
    }
    if data["truncated"] == true {
        out.push_str("  more revisions exist than one read returns (200)\n");
    }
    out
}
