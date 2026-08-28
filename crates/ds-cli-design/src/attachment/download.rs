//! `ds design attachment download` — authorize one revision's bytes.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;

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
    purpose: "\
Returns a time-limited URL that ds-brain minted and signed against the exact \
storage generation it verified when the revision was published. `ds` never \
composes a storage URL and never holds a credential; what comes back is an \
authorization to fetch those exact bytes, and a later object at the same name \
cannot be served in its place. The digest is returned alongside so the fetched \
bytes can be checked.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[ATTACHMENT_ARG, REVISION_ARG, DESCRIPTOR_ARG],
    output: "The project, the `attachment` and `revision`, the `file` name, its `bytes` and `digest`, the signed `url`, and when it `expiresAt`.",
    examples: &[Example {
        command: "ds design attachment download --attachment att-site-a-bak --output json",
        note: "Read .data.url; it expires, so fetch promptly and verify .data.digest.",
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
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert("attachment".into(), json!(inputs.require("attachment")?));
    if let Some(revision) = inputs.value("revision") {
        arguments.insert("revision".into(), json!(revision));
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::ATTACHMENT_DOWNLOAD,
        Value::Object(arguments),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
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
