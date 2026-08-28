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
    summary: "Comma-separated values from the definition. Omit to clear them.",
};

pub static COMMAND: Command = Command {
    id: "design.tag.set",
    path: &["design", "tag", "set"],
    contract: 1,
    summary: "Apply one tag definition's values to a transformer or model.",
    purpose: "\
Writes the values the object carries under one definition, replacing whatever \
it carried before. Omitting --values clears them, which is a real instruction \
and not a no-op. Every value is checked against the definition's own vocabulary \
and cardinality server-side; an out-of-vocabulary value is refused, never \
coerced. Pass --version to tag one exact object version instead of the object \
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
        VERSION_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The project, the object, the `definition`, the accepted `values` and the committed `version`.",
    examples: &[Example {
        command: "ds design tag set --kind lv_transformer --object kigali_a --definition transformer_scope --values additional_scope --yes",
        note: "Omit --values to clear this object's values under the definition.",
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
        crate::INVALID_ANCHOR,
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = crate::anchor(inputs)?;
    arguments.insert("definition".into(), json!(inputs.require("definition")?));
    // An omitted --values CLEARS. Sending the empty list explicitly is what
    // distinguishes "no values" from "the flag was forgotten": the application
    // refuses a missing key, so the instruction is always deliberate.
    let values = match inputs.value("values") {
        Some(raw) => crate::list_values(raw, "values", MAX_TAG_VALUES)?,
        None => Vec::new(),
    };
    arguments.insert("values".into(), json!(values));
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
