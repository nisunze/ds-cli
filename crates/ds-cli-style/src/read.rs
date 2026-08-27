//! `ds style read` — one style document, its fields, and what the map shows.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, REF_ARG};

pub static COMMAND: Command = Command {
    id: "style.read",
    path: &["style", "read"],
    contract: 1,
    summary: "One style document with its fields, on-map values and channels.",
    purpose: "\
Reads the authored MapLibre document for one ref, the fields and values the \
backend publishes for it, the fields and value TYPES the map currently \
renders (so a numeric tile property gets numeric match labels), the second \
dimension if one is authored, and the channels this layer type offers.",
    chapter: Chapter::MapPresentation,
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[REF_ARG, DESCRIPTOR_ARG],
    output: "\
The `ds style list` row plus `document`, `fields`, `fieldValues`, `onMap` \
(features, fields, values, types — null when no map is mounted), `channels` \
(channel, numberProperty, colorProperty, min, max, on, off), `second` and \
`warnings` from ds-brain's expression validator.",
    examples: &[Example {
        command: "ds style read --ref master/lv_poles --output json",
        note: "Read .data.channels and .data.onMap.types before `ds style dimension plan`.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::STYLE_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
    ],
    reference: Some("docs/reference/style.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::STYLE_READ,
        json!({ "ref": inputs.require("ref")? }),
        crate::READ_TIMEOUT,
    )
    .map_err(crate::classify_style_failure)
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} · {} · target {} · colour by {}\n",
        data["ref"].as_str().unwrap_or("?"),
        data["type"].as_str().unwrap_or("?"),
        data["target"].as_str().unwrap_or("—"),
        data["colorField"].as_str().unwrap_or("(flat)"),
    );
    if let Some(second) = data["second"].as_object() {
        out.push_str(&format!(
            "  second dimension: {} → {}\n",
            second["field"].as_str().unwrap_or("?"),
            second["channel"].as_str().unwrap_or("?"),
        ));
        for row in second["values"].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "    {:<24} {}{}\n",
                crate::truncate(row["value"].as_str().unwrap_or("?"), 24),
                row["amount"],
                row["color"]
                    .as_str()
                    .map(|c| format!("  {c}"))
                    .unwrap_or_default(),
            ));
        }
    }
    let channels: Vec<&str> = data["channels"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| c["channel"].as_str())
        .collect();
    out.push_str(&format!("  channels: {}\n", channels.join(", ")));
    if let Some(fields) = data["fields"].as_array() {
        let names: Vec<&str> = fields.iter().filter_map(Value::as_str).collect();
        out.push_str(&format!(
            "  fields: {}\n",
            crate::truncate(&names.join(", "), 110)
        ));
    }
    if let Some(on_map) = data["onMap"].as_object() {
        out.push_str(&format!(
            "  on map: {}\n",
            crate::plural(on_map["features"].as_u64().unwrap_or(0), "rendered feature")
        ));
    }
    for warning in data["warnings"].as_array().into_iter().flatten() {
        if let Some(text) = warning.as_str() {
            out.push_str(&format!("  warning: {}\n", crate::truncate(text, 110)));
        }
    }
    out
}
