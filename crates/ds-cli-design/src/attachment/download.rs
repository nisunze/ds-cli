//! `ds design attachment download` — authorize one revision's bytes.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

pub const ATTACHMENT_ARG: Arg = Arg {
    name: "attachment",
    kind: ArgKind::Value,
    value: "<attachment-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The logical file, from `ds design attachment list`.",
};

pub const REVISION_ARG: Arg = Arg {
    name: "revision",
    kind: ArgKind::Value,
    value: "<revision-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "One exact revision. Omit for the file's current latest.",
};

pub static COMMAND: Command = Command {
    id: "design.attachment.download",
    path: &["design", "attachment", "download"],
    contract: 1,
    summary: "Authorize a download of one attachment revision.",
    purpose: "Authorize an exact immutable revision for the explicit project. Returns a server-signed, generation-pinned URL and digest for the caller to fetch and verify; native identity credentials are never sent to Storage.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        crate::versions::PROJECT,
        crate::transformer::LANE_ARG,
        ATTACHMENT_ARG,
        REVISION_ARG,
    ],
    output: "The project, the `attachment` and `revision`, the `file` name, its `bytes` and `digest`, the signed `url`, and when it `expiresAt`.",
    examples: &[Example {
        command: "ds design attachment download --project <id> --attachment att-site-a-bak --output json",
        note: "Read .data.url; it expires, so fetch promptly and verify .data.digest.",
        runnable: false,
    }],
    refusals: &[super::NATIVE_REFUSED],
    reference: Some("docs/reference/design.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    super::ask(
        inputs,
        ds_client_core::design_attachments::Command::Download {
            attachment: inputs.require("attachment")?.into(),
            revision: inputs.value("revision").map(str::to_owned),
        },
    )
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} · {} · {} bytes\n",
        data["file"].as_str().unwrap_or("?"),
        data["revision"].as_str().unwrap_or("?"),
        data["bytes"].as_u64().unwrap_or(0),
    );
    if let Some(url) = data["url"].as_str() {
        out.push_str(&format!("  {url}\n"));
    }
    if let Some(expires) = data["expiresAt"].as_str() {
        out.push_str(&format!("  expires {expires}\n"));
    }
    out
}
