//! `ds design tag define` — create or edit one project tag definition.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{DESCRIPTOR_ARG, MAX_TAG_VALUES};

pub const DEFINITION_ARG: Arg = Arg {
    name: "definition",
    kind: ArgKind::Value,
    value: "<definition-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "The definition's stable id, e.g. transformer_scope.",
};

const NAME_ARG: Arg = Arg {
    name: "name",
    kind: ArgKind::Value,
    value: "<text>",
    required: true,
    default: None,
    choices: &[],
    summary: "The human name shown wherever the tag is offered.",
};

const CARDINALITY_ARG: Arg = Arg {
    name: "cardinality",
    kind: ArgKind::Value,
    value: "<how-many>",
    required: false,
    default: Some("single"),
    choices: &["single", "multiple"],
    summary: "One value (a radio) or any number of them (checkboxes).",
};

const VALUES_ARG: Arg = Arg {
    name: "values",
    kind: ArgKind::Value,
    value: "<values>",
    required: true,
    default: None,
    choices: &[],
    summary: "Comma-separated allowed values, in the order they should appear.",
};

const DESCRIPTION_ARG: Arg = Arg {
    name: "description",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "What the tag means. Empty is allowed.",
};

pub static COMMAND: Command = Command {
    id: "design.tag.define",
    path: &["design", "tag", "define"],
    contract: 1,
    summary: "Create or edit one of the project's typed tag definitions.",
    purpose: "\
Declares the vocabulary a tag may take — its allowed values, whether one or \
many apply, and the order they are offered in. Re-running on an existing \
definition edits it: the application reads its current version and writes under \
it, so a concurrent edit is refused rather than overwritten. Editing a \
definition the project ADOPTED from a global template detaches it from that \
template, and that detachment is recorded rather than hidden — the project's \
copy no longer says what that exact template version says.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        DEFINITION_ARG,
        NAME_ARG,
        CARDINALITY_ARG,
        VALUES_ARG,
        DESCRIPTION_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The project, the `definition` id, its `name`, `cardinality`, committed `version` and the accepted `values`.",
    examples: &[Example {
        command: "ds design tag define --definition transformer_scope --name \"Transformer scope\" --values initial_scope,additional_scope --yes",
        note: "Cardinality defaults to single; pass --cardinality multiple for checkboxes.",
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
        crate::INVALID_VALUE_LIST,
        crate::TOO_MANY,
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let values = crate::list_values(inputs.require("values")?, "values", MAX_TAG_VALUES)?;
    let mut arguments = Map::new();
    arguments.insert("definition".into(), json!(inputs.require("definition")?));
    arguments.insert("name".into(), json!(inputs.require("name")?));
    arguments.insert("values".into(), json!(values));
    for flag in ["cardinality", "description"] {
        if let Some(value) = inputs.value(flag) {
            arguments.insert(flag.into(), json!(value));
        }
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TAG_DEFINE,
        Value::Object(arguments),
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let values: Vec<&str> = data["values"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    format!(
        "{} \"{}\" ({}) = {} · v{}\n",
        data["definition"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or("?"),
        data["cardinality"].as_str().unwrap_or("?"),
        values.join(", "),
        data["version"].as_u64().unwrap_or(0),
    )
}
