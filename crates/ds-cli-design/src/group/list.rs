//! `ds design group list` — the governed vocabularies and what a set carries.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::DESCRIPTOR_ARG;
use crate::group::TRANSFORMERS_ARG;

pub static COMMAND: Command = Command {
    id: "design.group.list",
    path: &["design", "group", "list"],
    contract: 1,
    summary: "List batchable tag definitions and current transformer values.",
    purpose: "\
Discovers active single-valued choice definitions that apply to LV \
transformers, the values each allows, and what every named transformer carries today. Read \
`allowed` here before assigning: values are matched against the project's own \
vocabulary by exact bytes, so a spelling that only differs in case is refused \
rather than corrected. `allowed` carries the exact stored spelling, not a \
display normalization. A group the project has not defined yet is a real \
state, not an error — it is defined in the application's Tags surface.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMERS_ARG, DESCRIPTOR_ARG],
    output: "\
The project and one row per group: `group`, `defined`, `cardinality`, the \
`allowed` vocabulary, optional model-evidence state, and each named \
transformer's current `value` and `modelState`.",
    examples: &[Example {
        command: "ds design group list --transformers kigali_a,kigali_b --output json",
        note: "Read .data.groups[].allowed before `ds design group preview`.",
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
        crate::INVALID_VALUE_LIST,
        crate::TOO_MANY,
    ],
    reference: Some("docs/reference/design.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert(
        "transformers".into(),
        json!(crate::group::transformers(inputs)?),
    );
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::GROUP_LIST,
        Value::Object(arguments),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)
}

pub fn render(data: &Value) -> String {
    let mut out = String::new();
    for group in data["groups"].as_array().into_iter().flatten() {
        let name = group["group"].as_str().unwrap_or("?");
        if !group["defined"].as_bool().unwrap_or(false) {
            out.push_str(&format!("{name} · not defined in this project\n"));
            continue;
        }
        let allowed: Vec<&str> = group["allowed"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .collect();
        out.push_str(&format!(
            "{name} · {}{}\n",
            allowed.join(", "),
            if group["needsModel"].as_bool().unwrap_or(false) {
                " · also needs the DS Grid model"
            } else {
                ""
            },
        ));
        for row in group["values"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "  {} = {}{}\n",
                row["transformer"].as_str().unwrap_or("?"),
                row["value"].as_str().unwrap_or("—"),
                match row["modelState"].as_str() {
                    Some(state) => format!(" · model {state}"),
                    None => String::new(),
                },
            ));
        }
    }
    out
}
