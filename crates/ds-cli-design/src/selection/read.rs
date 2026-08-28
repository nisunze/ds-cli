//! `ds design selection read` — one selection with its membership evaluated.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

pub const SELECTION_ARG: Arg = Arg {
    name: "selection",
    kind: ArgKind::Value,
    value: "<selection-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The saved selection, from `ds design selection list`.",
};

pub static COMMAND: Command = Command {
    id: "design.selection.read",
    path: &["design", "selection", "read"],
    contract: 1,
    summary: "Resolve one saved selection and evaluate its membership now.",
    purpose: "\
Evaluates the selection server-side and reports every member as `present`, \
`changed` or `missing` — under the label it was saved with. Nothing is \
substituted: because a transformer rename mints a new document identity, a \
renamed member reads as missing under its old name, which is the honest answer. \
Also returns `memberDigest`, which `ds design selection assign` must echo back; \
that echo is what proves the operator saw the exact set being assigned.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[SELECTION_ARG, DESCRIPTOR_ARG],
    output: "\
The project, the selection's `name`, `mode`, `version`, `state`, its \
`memberDigest`, the `present`/`changed`/`missing` counts, and one row per \
member with `id`, `label` and `state`.",
    examples: &[Example {
        command: "ds design selection read --selection sel-week-32 --output json",
        note: "Read .data.memberDigest before assigning; it pins what gets assigned.",
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
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::SELECTION_READ,
        json!({ "selection": inputs.require("selection")? }),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} · {} · v{} · {}\n",
        data["selection"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or("?"),
        data["version"].as_u64().unwrap_or(0),
        data["mode"].as_str().unwrap_or("?"),
    );
    out.push_str(&format!(
        "  {} current · {} changed · {} missing\n",
        data["present"].as_u64().unwrap_or(0),
        data["changed"].as_u64().unwrap_or(0),
        data["missing"].as_u64().unwrap_or(0),
    ));
    for member in data["members"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<10} {}\n",
            member["state"].as_str().unwrap_or("?"),
            member["label"].as_str().unwrap_or("?"),
        ));
    }
    if let Some(digest) = data["memberDigest"].as_str() {
        out.push_str(&format!("  digest {}\n", &digest[..digest.len().min(16)]));
    }
    out
}
