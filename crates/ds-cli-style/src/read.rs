//! `ds style read` — one style document, its fields, and what the map shows.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::native::TRANSFORMER_ARG;
use crate::{LANE_ARG, PROJECT_ARG, REF_ARG};

pub static COMMAND: Command = Command {
    id: "style.read",
    path: &["style", "read"],
    contract: 2,
    summary: "One style document, its fields, channels, and what the data holds.",
    purpose: "Reads one backend-published style editor and its full document, field vocabulary, property bounds, icon names and supported channels. With --transformer it also reports what that canonical project data holds: each field's values, per-value counts and scalar type. Nothing is read from a renderer, so an off-screen value is reported exactly as an on-screen one is.",
    chapter: Chapter::MapPresentation,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, REF_ARG, TRANSFORMER_ARG, LANE_ARG],
    output: "Project, ref, type, target, document, fields, fieldValues, fieldDomains, propertySchema, icons, channels, second, colorField, more, observed (derived, or a named refusal saying why nothing was observed), and — when derived — fieldTypes, fieldTypeSource, fieldTypeConflicts, fieldCounts, fieldMatchLabels and fieldProvenance.",
    examples: &[Example {
        command: "ds style read --project <id> --ref master/lv_poles --transformer T-1042 --output json",
        note: "Read .data.channels and .data.observed.types before `ds style dimension plan`.",
        runnable: false,
    }],
    refusals: crate::native::REFUSALS,
    reference: Some("docs/reference/style.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    crate::native::read(
        inputs,
        crate::native::Read::Describe,
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
    // What the project's own data holds — never what a renderer painted.
    let observed = &data["observed"];
    match observed["status"].as_str() {
        Some("derived") => {
            out.push_str(&format!(
                "  in {}: {}{}\n",
                observed["source"].as_str().unwrap_or("project data"),
                crate::plural(
                    observed["features"].as_u64().unwrap_or(0),
                    "canonical feature"
                ),
                if observed["truncated"].as_bool().unwrap_or(false) {
                    " (bounded)"
                } else {
                    ""
                },
            ));
            let typed: Vec<String> = data["fieldTypes"]
                .as_object()
                .into_iter()
                .flatten()
                .map(|(field, kind)| format!("{field}:{}", kind.as_str().unwrap_or("?")))
                .collect();
            if !typed.is_empty() {
                out.push_str(&format!(
                    "  field types: {}\n",
                    crate::truncate(&typed.join(", "), 110)
                ));
            }
            for conflict in data["fieldTypeConflicts"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "  warning: {} is declared {} but its data carries {}\n",
                    conflict["field"].as_str().unwrap_or("?"),
                    conflict["declared"].as_str().unwrap_or("?"),
                    conflict["observed"].as_str().unwrap_or("?"),
                ));
            }
        }
        Some("refused") => out.push_str(&format!(
            "  not observed ({}): {}\n",
            observed["code"].as_str().unwrap_or("?"),
            crate::truncate(observed["detail"].as_str().unwrap_or(""), 110),
        )),
        _ => {}
    }
    for warning in data["warnings"].as_array().into_iter().flatten() {
        if let Some(text) = warning.as_str() {
            out.push_str(&format!("  warning: {}\n", crate::truncate(text, 110)));
        }
    }
    out
}
