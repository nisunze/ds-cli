//! `ds design attachment set-latest` — move the file's latest pointer.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::attachment::download::ATTACHMENT_ARG;

const REVISION_ARG: Arg = Arg::value(
    "revision",
    "<revision-id>",
    "The ready revision that becomes latest, from `show`.",
)
.required();
const EXPECTED_VERSION_ARG: Arg = Arg::value(
    "expected-version",
    "<n>",
    "The head version you read (1 or more). Omit to fence on the version read just before.",
);

pub static COMMAND: Command = Command {
    id: "design.attachment.set-latest",
    path: &["design", "attachment", "set-latest"],
    contract: 1,
    summary: "Make one ready revision the attachment's latest.",
    purpose: "Move the file's latest pointer to one ready, non-retired revision in the explicit project, e.g. back to the .bak a submission actually shipped. The move is fenced on the head version: pass --expected-version from `show` to refuse if anyone changed the file since you read it, or omit it to fence on the version read immediately before. Bytes and revisions never change.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        ATTACHMENT_ARG,
        REVISION_ARG,
        EXPECTED_VERSION_ARG,
        crate::versions::PROJECT,
        crate::transformer::LANE_ARG,
    ],
    output: "The project, the `attachment`, the new `latest` revision and its `ordinal`, the `previous_version` fence, and the committed `version`.",
    examples: &[Example {
        command: "ds design attachment set-latest --project <id> --attachment att-line-a-bak --revision rev_1 --expected-version 4 --yes",
        note: "A moved fence is refused as design_attachment_refused with the conflict nested; read `show` again.",
        runnable: false,
    }],
    refusals: &[
        super::NATIVE_REFUSED,
        crate::INVALID_NUMBER,
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/design.md"),
    search: &["make latest", "latest revision"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let expected_version = inputs
        .value("expected-version")
        .map(|raw| crate::integer(raw, "expected-version", 1, i64::MAX))
        .transpose()?
        .map(|version| version as u64);
    super::ask(
        inputs,
        ds_client_core::design_attachments::Command::SetLatest {
            attachment: inputs.require("attachment")?.into(),
            revision: inputs.require("revision")?.into(),
            expected_version,
        },
    )
}

pub fn render(data: &Value) -> String {
    format!(
        "{} latest is now {} (r{}) · v{} → v{}\n",
        data["attachment"].as_str().unwrap_or("?"),
        data["latest"].as_str().unwrap_or("?"),
        data["ordinal"].as_u64().unwrap_or(0),
        data["previous_version"].as_u64().unwrap_or(0),
        data["version"].as_u64().unwrap_or(0),
    )
}
