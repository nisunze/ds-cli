//! `ds design attachment list` — every file on one design object.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::{KIND_ARG, OBJECT_ARG, VERSION_ARG};

const ARCHIVED_ARG: Arg = Arg {
    name: "archived",
    kind: ArgKind::Switch,
    value: "",
    required: false,
    default: None,
    choices: &[],
    summary: "Include archived files and retired revisions.",
};

pub static COMMAND: Command = Command {
    id: "design.attachment.list",
    path: &["design", "attachment", "list"],
    contract: 1,
    summary: "List the files attached to a transformer or DS Grid model.",
    purpose: "List server-owned file heads and immutable revisions for the explicit project/object. --version filters exact LV vN or MV content-revision bindings; use returned attachment_id and revision_id for subsequent operations.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        KIND_ARG,
        OBJECT_ARG,
        VERSION_ARG,
        ARCHIVED_ARG,
        crate::versions::PROJECT,
        crate::transformer::LANE_ARG,
    ],
    output: "\
The project, anchored object, total, truncated flag, and attachments rows. \
Each row contains the server attachment head and immutable revisions with \
revision_id, file_name, size_bytes, sha256, object_version and state.",
    examples: &[Example {
        command: "ds design attachment list --project <id> --kind lv_transformer --object kigali_a --output json",
        note: "Read .data.attachments[].revisions[].revision_id to download an exact revision.",
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
        ds_client_core::design_attachments::Command::List {
            object: super::object(inputs)?,
            archived: inputs.switch("archived"),
        },
    )
}

pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default()
}
