//! `ds style read` — one style document, its fields, and what the map shows.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{LANE_ARG, REF_ARG};

pub static COMMAND: Command = Command {
    id: "style.read",
    path: &["style", "read"],
    contract: 2,
    summary: "One style document with its fields, on-map values and channels.",
    purpose: "Reads one backend-published style editor and its full document, field vocabulary, property bounds, icon names and supported channels. Runtime presence is not inferred.",
    chapter: Chapter::MapPresentation,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[REF_ARG, LANE_ARG],
    output: "Project, ref, type, target, document, fields, fieldValues, fieldDomains, propertySchema, icons, channels, second, colorField, onMap null, and more.",
    examples: &[Example {
        command: "ds style read --ref master/lv_poles --output json",
        note: "Read .data.channels and .data.onMap.types before `ds style dimension plan`.",
        runnable: false,
    }],
    refusals: crate::native::REFUSALS,
    reference: Some("docs/reference/style.md"),
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    crate::native::execute(
        inputs,
        context,
        &crate::STYLE_READ,
        json!({ "ref": inputs.require("ref")? }),
    )
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
