//! `ds design selection save` — name a set of transformers, or replace one.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, MAX_SELECTION_MEMBERS};

const NAME_ARG: Arg = Arg {
    name: "name",
    kind: ArgKind::Value,
    value: "<text>",
    required: true,
    default: None,
    choices: &[],
    summary: "What the selection is for. The one field it cannot be without.",
};

const TRANSFORMERS_ARG: Arg = Arg {
    name: "transformers",
    kind: ArgKind::Value,
    value: "<names>",
    required: true,
    default: None,
    choices: &[],
    summary: "Comma-separated transformer names. These become stable identities.",
};

const SELECTION_ARG: Arg = Arg {
    name: "selection",
    kind: ArgKind::Value,
    value: "<selection-id>",
    required: false,
    default: None,
    choices: &[],
    summary: "Replace this existing selection's membership instead of creating one.",
};

const DESCRIPTION_ARG: Arg = Arg {
    name: "description",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "Why this set exists. Empty is allowed and common.",
};

pub static COMMAND: Command = Command {
    id: "design.selection.save",
    path: &["design", "selection", "save"],
    contract: 1,
    summary: "Save a named selection of transformers, or replace one's members.",
    purpose: "\
Stores stable transformer identities under a name so the same set can be \
rediscovered, and assigned, later. Without --selection this creates a new \
saved selection; with it, the application reads the current version and \
replaces that selection's membership under it, so a concurrent edit is refused \
rather than overwritten. A name the project has no transformer for is refused \
at save time — a member that was already missing when it was saved would be \
noise on every later read.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        NAME_ARG,
        TRANSFORMERS_ARG,
        SELECTION_ARG,
        DESCRIPTION_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The project, the `selection` id, its `name`, the committed `version`, and the member count.",
    examples: &[Example {
        command: "ds design selection save --name \"Week 32 review\" --transformers kigali_a,kigali_b --yes",
        note: "Without --yes dispatch refuses before the bridge is opened.",
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
        crate::READ_ONLY,
        crate::CONFLICT,
        crate::TOO_MANY,
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformers = crate::list_values(
        inputs.require("transformers")?,
        "transformers",
        MAX_SELECTION_MEMBERS,
    )?;
    let mut arguments = Map::new();
    arguments.insert("name".into(), json!(inputs.require("name")?));
    arguments.insert("transformers".into(), json!(transformers));
    for flag in ["selection", "description"] {
        if let Some(value) = inputs.value(flag) {
            arguments.insert(flag.into(), json!(value));
        }
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::SELECTION_SAVE,
        Value::Object(arguments),
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    format!(
        "saved {} \"{}\" in {} · v{} · {}\n",
        data["selection"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        data["version"].as_u64().unwrap_or(0),
        crate::plural(data["members"].as_u64().unwrap_or(0), "transformer"),
    )
}
