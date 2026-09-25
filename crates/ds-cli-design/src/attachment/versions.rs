//! `ds design attachment versions` — which object versions carry one file.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::attachment::download::ATTACHMENT_ARG;

pub static COMMAND: Command = Command {
    id: "design.attachment.versions",
    path: &["design", "attachment", "versions"],
    contract: 1,
    summary: "List the object versions one attached file is bound to.",
    purpose: "The reverse map for one file: each object version (MV content revision or LV vN) its revisions bind to, retired included — which submitted model versions carry a .bak. Over 200 revisions is refused.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        ATTACHMENT_ARG,
        crate::versions::PROJECT,
        crate::transformer::LANE_ARG,
    ],
    output: "`total` and `object_versions` rows (`object` with version_id, `revision_ids`, `revision_count`, `latest_upload`), newest first.",
    examples: &[Example {
        command: "ds design attachment versions --project <id> --attachment att-line-a-bak --output json",
        note: "A row without version_id binds the object as a whole.",
        runnable: false,
    }],
    refusals: &[super::NATIVE_REFUSED],
    reference: Some("docs/reference/design.md"),
    search: &["attachment model versions", "which versions carry"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    super::ask(
        inputs,
        ds_client_core::design_attachments::Command::Versions {
            attachment: inputs.require("attachment")?.into(),
        },
    )
}

pub fn render(data: &Value) -> String {
    let mut out = String::new();
    for row in data["object_versions"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{} {} @ {} · {} revision(s) · latest upload {}\n",
            row["object"]["kind"].as_str().unwrap_or("?"),
            row["object"]["id"].as_str().unwrap_or("?"),
            row["object"]["version_id"].as_str().unwrap_or("unpinned"),
            row["revision_count"].as_u64().unwrap_or(0),
            row["latest_upload"].as_str().unwrap_or("?"),
        ));
    }
    if out.is_empty() {
        out.push_str("bound to no object version\n");
    }
    out
}
