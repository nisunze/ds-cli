//! `ds design attachment list` — every file on one design object.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, KIND_ARG, OBJECT_ARG, VERSION_ARG};

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
    purpose: "\
Names every logical file on the object with all of its revisions: the file \
name, byte size, server-verified digest, which object version each revision is \
bound to, and which one the logical latest pointer names. Passing --version \
narrows to the revisions bound to that exact object version, which is how you \
answer \"what was attached when this model was at rev_2\". Every other command \
in this family needs an id from here.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        KIND_ARG,
        OBJECT_ARG,
        VERSION_ARG,
        ARCHIVED_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "\
The project, the anchored object, the total, and rows of `attachment`, `label`, \
`state`, `version`, `latest`, and per-revision `revision`, `ordinal`, `file`, \
`bytes`, `digest`, `boundVersion` and `retired`.",
    examples: &[Example {
        command: "ds design attachment list --kind lv_transformer --object kigali_a --output json",
        note: "Read .data.attachments[].revisions[].revision to download an exact revision.",
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
    if inputs.switch("archived") {
        arguments.insert("archived".into(), json!(true));
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::ATTACHMENT_LIST,
        Value::Object(arguments),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let total = data["total"].as_u64().unwrap_or(0);
    let mut out = format!(
        "{} on {} in {}\n",
        crate::plural(total, "attachment"),
        data["object"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
    );
    for row in data["attachments"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {} · {} · v{}\n",
            row["attachment"].as_str().unwrap_or("?"),
            row["label"].as_str().unwrap_or("?"),
            row["version"].as_u64().unwrap_or(0),
        ));
        let latest = row["latest"].as_str().unwrap_or("");
        for revision in row["revisions"].as_array().into_iter().flatten() {
            let id = revision["revision"].as_str().unwrap_or("?");
            out.push_str(&format!(
                "    r{} {} · {} bytes · {}{}{}\n",
                revision["ordinal"].as_u64().unwrap_or(0),
                revision["file"].as_str().unwrap_or("?"),
                revision["bytes"].as_u64().unwrap_or(0),
                revision["boundVersion"].as_str().unwrap_or("object"),
                if id == latest { " · latest" } else { "" },
                if revision["retired"].as_bool() == Some(true) {
                    " · retired"
                } else {
                    ""
                },
            ));
        }
        if row["more"].as_bool() == Some(true) {
            out.push_str("    … more revisions exist than are shown\n");
        }
    }
    out
}
