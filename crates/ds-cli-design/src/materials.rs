//! Explicit, preview-pinned catalog replication through the governed owner.
use crate::LANE_ARG;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

const TEMPLATE: Arg = Arg {
    name: "template",
    kind: ArgKind::Value,
    value: "<name>",
    required: true,
    default: None,
    choices: &[],
    summary: "Exact global template to update alongside matching projects.",
};
const RULE: Arg = Arg {
    name: "rule-set",
    kind: ArgKind::Value,
    value: "<name>",
    required: true,
    default: None,
    choices: &[],
    summary: "Only projects explicitly selecting this LV pole rule family are included.",
};
const ROW: Arg = Arg {
    name: "row",
    kind: ArgKind::Repeated,
    value: "<clean-name>",
    required: true,
    default: None,
    choices: &[],
    summary: "Exact source pole-catalog row identity. Repeat for the authorized rows (1-64).",
};
const DIGEST: Arg = Arg {
    name: "digest",
    kind: ArgKind::Value,
    value: "<sha256>",
    required: true,
    default: None,
    choices: &[],
    summary: "The digest returned by preview for this same source and scope.",
};
pub static PREVIEW: Command = Command {
    id: "design.materials.preview",
    path: &["design", "materials", "preview"],
    contract: 1,
    summary: "Preview a scoped pole-material catalog repair.",
    purpose: "Uses the active project's stored catalog as source. Reads the named global template and project inventory without seed-on-read. Includes only projects selecting the exact rule family. Shows exact before/after rows and revision fences; never replaces entire configurations or edits transformer results. Requires network.template.propagate. Bounds: 64 source rows, 200 selected documents, 2000 inventory projects and a 512 KiB plan.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TEMPLATE, RULE, ROW, LANE_ARG],
    output: "Source rows, selected targets with revisions and changes, skipped project IDs, digest and applied=false.",
    examples: &[],
    refusals: &crate::headless_refusals!(crate::NOT_PERMITTED, crate::CONFLICT,),
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static APPLY: Command = Command {
    id: "design.materials.apply",
    path: &["design", "materials", "apply"],
    contract: 1,
    summary: "Commit exactly the previewed material catalog repair.",
    purpose: "Atomically patches the previewed catalog rows and records the authenticated actor receipt. A changed source, selection or target revision refuses the whole write. Other rules, settings and transformer data are preserved. A successful retry returns the retained receipt. The source is the paired application's active project; no project or credential override is accepted. Requires network.template.propagate.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[TEMPLATE, RULE, ROW, DIGEST, LANE_ARG],
    output: "The committed plan with applied=true; no partial fanout and no transformer result save.",
    examples: &[],
    refusals: &crate::headless_refusals!(
        crate::NOT_PERMITTED,
        crate::CONFLICT,
        crate::CONFIRMATION_REQUIRED,
    ),
    reference: Some("docs/reference/design.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
fn run(inputs: &Inputs, apply: bool) -> Result<Value, Failure> {
    let mut args = json!({"template":inputs.require("template")?,"rule-set":inputs.require("rule-set")?,"rows":inputs.repeated("row")});
    if apply {
        args["digest"] = json!(inputs.require("digest")?);
    }
    crate::headless::perform(
        if apply {
            "design.materials.apply"
        } else {
            "design.materials.preview"
        },
        args,
        inputs.value("lane").unwrap_or("stable"),
    )
}
pub fn preview(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    run(inputs, false)
}
pub fn apply(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    run(inputs, true)
}
pub fn render(data: &Value) -> String {
    format!(
        "Material repair: {} · {} targets · digest {}\n",
        if data["applied"] == true {
            "applied"
        } else {
            "preview"
        },
        data["targets"].as_array().map_or(0, Vec::len),
        data["digest"].as_str().unwrap_or("missing")
    )
}
