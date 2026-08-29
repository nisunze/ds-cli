//! `ds design tag set` — apply a definition's values to one object.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::tag::define::DEFINITION_ARG;
use crate::{DESCRIPTOR_ARG, KIND_ARG, MAX_TAG_VALUES, OBJECT_ARG, VERSION_ARG};

const VALUES_ARG: Arg = Arg {
    name: "values",
    kind: ArgKind::Value,
    value: "<values>",
    required: false,
    default: None,
    choices: &[],
    summary: "Choice value(s) from the vocabulary. Omit every value flag to clear.",
};

const TEXT_ARG: Arg = Arg {
    name: "text",
    kind: ArgKind::Value,
    value: "<text>",
    required: false,
    default: None,
    choices: &[],
    summary: "One text value for a text definition.",
};

const INTEGER_ARG: Arg = Arg {
    name: "integer",
    kind: ArgKind::Value,
    value: "<integer>",
    required: false,
    default: None,
    choices: &[],
    summary: "One signed 64-bit value for an integer definition.",
};

const NUMBER_ARG: Arg = Arg {
    name: "number",
    kind: ArgKind::Value,
    value: "<number>",
    required: false,
    default: None,
    choices: &[],
    summary: "One finite value for a number definition.",
};

pub static COMMAND: Command = Command {
    id: "design.tag.set",
    path: &["design", "tag", "set"],
    contract: 1,
    summary: "Apply one tag definition's values to a transformer or model.",
    purpose: "\
Writes the value(s) an object carries under one definition, replacing whatever \
it carried before. Choice definitions use --values; free-form definitions use \
exactly one of --text, --integer or --number. Omitting every value flag clears \
the assignment. The server checks the value against the definition's type, \
vocabulary and constraints and never coerces it. Pass --version to tag one \
exact object version instead of the object \
as a whole — a separate record, so a version-anchored value survives a later \
edit of the object-level one.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        KIND_ARG,
        OBJECT_ARG,
        DEFINITION_ARG,
        VALUES_ARG,
        TEXT_ARG,
        INTEGER_ARG,
        NUMBER_ARG,
        VERSION_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The project, object, definition, canonical values, closed typed_values and committed version.",
    examples: &[
        Example {
            command: "ds design tag set --kind lv_transformer --object kigali_a --definition transformer_scope --values additional_scope --yes",
            note: "Legacy choice assignment remains valid.",
            runnable: false,
        },
        Example {
            command: "ds design tag set --kind lv_transformer --object kigali_a --definition completion --number 82.5 --yes",
            note: "Numeric values stay numeric in typed_values; no string inference is involved.",
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
        crate::INVALID_ANCHOR,
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = crate::anchor(inputs)?;
    arguments.insert("definition".into(), json!(inputs.require("definition")?));
    let supplied = ["values", "text", "integer", "number"]
        .into_iter()
        .filter(|flag| inputs.value(flag).is_some())
        .collect::<Vec<_>>();
    if supplied.len() > 1 {
        return Err(Failure::invalid(
            "invalid_tag_input",
            "tag assignment accepts only one value shape",
        )
        .remedy("pass one of --values, --text, --integer or --number"));
    }
    match supplied.first().copied() {
        Some("values") => {
            let values = crate::list_values(inputs.require("values")?, "values", MAX_TAG_VALUES)?;
            arguments.insert("values".into(), json!(values));
        }
        Some("text") => {
            arguments.insert(
                "typed_values".into(),
                json!([{"type": "text", "text": inputs.require("text")?}]),
            );
        }
        Some("integer") => {
            let value = inputs.require("integer")?.parse::<i64>().map_err(|_| {
                Failure::invalid(
                    "invalid_number",
                    "`--integer` must be a signed 64-bit integer",
                )
                .remedy("pass a whole number")
            })?;
            arguments.insert(
                "typed_values".into(),
                json!([{"type": "integer", "integer": value}]),
            );
        }
        Some("number") => {
            let value = inputs.require("number")?.parse::<f64>().map_err(|_| {
                Failure::invalid("invalid_number", "`--number` must be numeric")
                    .remedy("pass one finite number")
            })?;
            if !value.is_finite() {
                return Err(
                    Failure::invalid("invalid_number", "`--number` must be finite")
                        .remedy("pass one finite number"),
                );
            }
            arguments.insert(
                "typed_values".into(),
                json!([{"type": "number", "number": value}]),
            );
        }
        None => {
            // Omitting every shape is the explicit clear instruction. The
            // adapter accepts both projections but receives only this empty
            // legacy array, never two competing representations.
            arguments.insert("values".into(), json!([]));
        }
        _ => unreachable!("supplied flags are drawn from the closed set"),
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TAG_SET,
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
        "{} on {} = {} · v{}\n",
        data["definition"].as_str().unwrap_or("?"),
        data["object"].as_str().unwrap_or("?"),
        if values.is_empty() {
            "cleared".to_string()
        } else {
            values.join(", ")
        },
        data["version"].as_u64().unwrap_or(0),
    )
}
