//! `ds design known-columns set` — publish or hide one ordinary property.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, KNOWN_COLUMNS_SET};

const LAYER_ARG: Arg = Arg {
    name: "layer",
    kind: ArgKind::Value,
    value: "<layer-id>",
    required: true,
    default: None,
    choices: &[],
    summary: "Canonical design layer, for example lv_lines or mv_lines.",
};

const FIELD_ARG: Arg = Arg {
    name: "field",
    kind: ArgKind::Value,
    value: "<property>",
    required: true,
    default: None,
    choices: &[],
    summary: "Ordinary property name, including tag_<definition_id> fields.",
};

const VISIBILITY_ARG: Arg = Arg {
    name: "visibility",
    kind: ArgKind::Value,
    value: "<state>",
    required: true,
    default: None,
    choices: &["published", "hidden"],
    summary: "Whether the property may appear on external design surfaces.",
};

pub static COMMAND: Command = Command {
    id: "design.known-columns.set",
    path: &["design", "known-columns", "set"],
    contract: 1,
    summary: "Publish or hide one design property on one layer.",
    purpose: "Atomically edits one field in the project's authoritative know_columns sheet. This is the same property-level operation used by Properties and Attribute Table configuration: tag fields are not a separate hardcoded domain. Hiding a field never deletes it from the internal model. The paired application reads the current revision first, so a concurrent edit is refused instead of overwritten.",
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[LAYER_ARG, FIELD_ARG, VISIBILITY_ARG, DESCRIPTOR_ARG],
    output: "The project, authority=know_columns, layer, field, stored visibility, changed flag and committed revision.",
    examples: &[
        Example {
            command: "ds design known-columns set --layer mv_lines --field tag_city --visibility published --yes",
            note: "Allows only this city tag property on this external layer.",
            runnable: false,
        },
        Example {
            command: "ds design known-columns set --layer mv_lines --field tag_internal_review --visibility hidden --yes",
            note: "Keeps the internal tag in the model but removes it from later exports and tiles.",
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
        crate::CONFIRMATION_REQUIRED,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let visibility = inputs.require("visibility")?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &KNOWN_COLUMNS_SET,
        json!({
            "layer": inputs.require("layer")?,
            "field": inputs.require("field")?,
            "visible": visibility == "published",
        }),
        crate::WRITE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    format!(
        "{}.{} = {} · {} revision {}{}\n",
        data["layer"].as_str().unwrap_or("?"),
        data["field"].as_str().unwrap_or("?"),
        if data["visible"].as_bool().unwrap_or(false) {
            "published"
        } else {
            "hidden"
        },
        data["authority"].as_str().unwrap_or("know_columns"),
        data["revision"].as_i64().unwrap_or(0),
        if data["changed"].as_bool() == Some(false) {
            " · unchanged"
        } else {
            ""
        },
    )
}
