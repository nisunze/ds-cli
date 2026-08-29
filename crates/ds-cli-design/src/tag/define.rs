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
    summary: "One value or many choice values. Free-form tags must be single.",
};

const VALUES_ARG: Arg = Arg {
    name: "values",
    kind: ArgKind::Value,
    value: "<values>",
    required: false,
    default: None,
    choices: &[],
    summary: "Choice vocabulary in display order. Required for choice; forbidden otherwise.",
};

const VALUE_TYPE_ARG: Arg = Arg {
    name: "value-type",
    kind: ArgKind::Value,
    value: "<type>",
    required: false,
    default: Some("choice"),
    choices: &["choice", "text", "integer", "number"],
    summary: "Stored value type. Existing definitions default to choice.",
};

const INPUT_CONTROL_ARG: Arg = Arg {
    name: "input-control",
    kind: ArgKind::Value,
    value: "<control>",
    required: false,
    default: None,
    choices: &["radio", "dropdown", "multiselect", "text", "number"],
    summary: "UI control metadata. Omit for the type/cardinality default.",
};

const MIN_ARG: Arg = Arg {
    name: "min",
    kind: ArgKind::Value,
    value: "<number>",
    required: false,
    default: None,
    choices: &[],
    summary: "Inclusive finite lower bound for integer or number tags.",
};

const MAX_ARG: Arg = Arg {
    name: "max",
    kind: ArgKind::Value,
    value: "<number>",
    required: false,
    default: None,
    choices: &[],
    summary: "Inclusive finite upper bound for integer or number tags.",
};

const MAX_LENGTH_ARG: Arg = Arg {
    name: "max-length",
    kind: ArgKind::Value,
    value: "<characters>",
    required: false,
    default: None,
    choices: &[],
    summary: "Text limit (1-500). Omit for the server default of 500.",
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
Declares the type and presentation of one project tag. Choice tags own an \
ordered vocabulary and may be single or multiple. Text, integer and number \
tags are single-valued and use constraints instead of pretending observed \
values are a vocabulary. Re-running on an existing \
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
        VALUE_TYPE_ARG,
        INPUT_CONTROL_ARG,
        MIN_ARG,
        MAX_ARG,
        MAX_LENGTH_ARG,
        DESCRIPTION_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The project, definition id, name, value type, input control, constraints, cardinality, version and choice vocabulary.",
    examples: &[
        Example {
            command: "ds design tag define --definition transformer_scope --name \"Transformer scope\" --values initial_scope,additional_scope --yes",
            note: "Legacy choice spelling stays valid; cardinality and value type default to single/choice.",
            runnable: false,
        },
        Example {
            command: "ds design tag define --definition completion --name \"Completion percent\" --value-type number --min 0 --max 100 --yes",
            note: "A numeric definition has no --values vocabulary; the server stores its closed type and bounds.",
            runnable: false,
        },
    ],
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
        crate::INVALID_TAG_INPUT,
        crate::INVALID_NUMBER,
        crate::TOO_MANY,
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let value_type = inputs.require("value-type")?;
    let values = match inputs.value("values") {
        Some(raw) => crate::list_values(raw, "values", MAX_TAG_VALUES)?,
        None => Vec::new(),
    };
    if (value_type == "choice") != !values.is_empty() {
        return Err(Failure::invalid(
            "invalid_tag_input",
            if value_type == "choice" {
                "a choice definition requires --values"
            } else {
                "only choice definitions accept --values"
            },
        )
        .remedy("pass --values for choice, or omit it for text/integer/number"));
    }
    if value_type != "choice" && inputs.require("cardinality")? != "single" {
        return Err(Failure::invalid(
            "invalid_tag_input",
            "text, integer and number definitions are single-valued",
        )
        .remedy("omit --cardinality or pass --cardinality single"));
    }
    let mut arguments = Map::new();
    arguments.insert("definition".into(), json!(inputs.require("definition")?));
    arguments.insert("name".into(), json!(inputs.require("name")?));
    arguments.insert("values".into(), json!(values));
    arguments.insert("value_type".into(), json!(value_type));
    if let Some(value) = inputs.value("input-control") {
        arguments.insert("input_control".into(), json!(value));
    }
    let mut constraints = Map::new();
    for flag in ["min", "max"] {
        if let Some(raw) = inputs.value(flag) {
            constraints.insert(flag.into(), json!(finite_number(raw, flag)?));
        }
    }
    if let Some(raw) = inputs.value("max-length") {
        constraints.insert(
            "max_length".into(),
            json!(crate::integer(raw, "max-length", 1, 500)?),
        );
    }
    if !constraints.is_empty() {
        arguments.insert("constraints".into(), Value::Object(constraints));
    }
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

fn finite_number(raw: &str, flag: &str) -> Result<f64, Failure> {
    let value = raw.parse::<f64>().map_err(|_| {
        Failure::invalid("invalid_number", format!("`--{flag}` must be a number"))
            .remedy("pass one finite numeric value")
    })?;
    if !value.is_finite() {
        return Err(
            Failure::invalid("invalid_number", format!("`--{flag}` must be finite"))
                .remedy("pass a finite number"),
        );
    }
    Ok(value)
}

pub fn render(data: &Value) -> String {
    let values: Vec<&str> = data["values"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    format!(
        "{} \"{}\" ({}/{}, {}) = {} · v{}\n",
        data["definition"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or("?"),
        data["value_type"].as_str().unwrap_or("choice"),
        data["input_control"].as_str().unwrap_or("?"),
        data["cardinality"].as_str().unwrap_or("?"),
        values.join(", "),
        data["version"].as_u64().unwrap_or(0),
    )
}
