//! `ds design tag list` — the project's vocabulary and this object's values.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

use crate::{DESCRIPTOR_ARG, KIND_ARG, OBJECT_ARG, VERSION_ARG};

pub static COMMAND: Command = Command {
    id: "design.tag.list",
    path: &["design", "tag", "list"],
    contract: 1,
    summary: "List the project's tag definitions and this object's values.",
    purpose: "\
Names every tag definition the project owns — its value type, input control, \
constraints, choice vocabulary, cardinality and governed-template origin — \
plus the lossless typed values currently applied to the named object. The \
legacy canonical string projection remains present for compatibility. Pass \
--version to read the values \
anchored to one exact object version rather than the object-level ones; the two \
are separate records, which is how a value keeps its historical context after a \
later edit. This is where a reporting run learns the vocabulary instead of \
scraping it out of a UI.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[KIND_ARG, OBJECT_ARG, VERSION_ARG, DESCRIPTOR_ARG],
    output: "\
The project, anchored object, and rows of definition, value_type, input_control, \
constraints, cardinality, state, allowed choice vocabulary, values, typed_values \
and template origin.",
    examples: &[Example {
        command: "ds design tag list --kind lv_transformer --object kigali_a --output json",
        note: "Read value_type and allowed before choosing --values, --text, --integer or --number.",
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
    let arguments = crate::anchor(inputs)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::TAG_LIST,
        Value::Object(arguments),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "tags on {} in {}\n",
        data["object"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
    );
    for row in data["tags"].as_array().into_iter().flatten() {
        let values: Vec<&str> = row["values"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        out.push_str(&format!(
            "  {} ({}/{}, {}) = {}{}\n",
            row["definition"].as_str().unwrap_or("?"),
            row["value_type"].as_str().unwrap_or("choice"),
            row["input_control"].as_str().unwrap_or("?"),
            row["cardinality"].as_str().unwrap_or("?"),
            if values.is_empty() {
                "—".to_string()
            } else {
                values.join(", ")
            },
            if row["state"].as_str() == Some("active") {
                ""
            } else {
                " · retired definition"
            },
        ));
    }
    out
}
