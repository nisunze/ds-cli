use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::detect::{self, Platform};

pub static COMMAND: Command = Command {
    id: "workstation.components",
    path: &["workstation", "components"],
    contract: 1,
    chapter: Chapter::Workstation,
    summary: "Governed prerequisite and reference-component catalogue.",
    purpose: "Lists why each component exists, its accepted provenance, platform applicability, local receipt state, and whether acquisition is currently implemented. Discovery never authorizes acquisition.",
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[crate::OPTIONAL_COMPONENT_ARG],
    output: "A bounded component catalogue with provenance, local state, acquisition policy, and lifecycle-proof status.",
    examples: &[Example {
        command: "ds workstation components --output json",
        note: "No network access or download occurs.",
        runnable: true,
    }],
    refusals: &[crate::COMPONENT_UNKNOWN],
    reference: Some("docs/reference/workstation.md"),
    availability: crate::always,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let platform = Platform::current();
    let selected = inputs.value("component");
    let all = detect::catalog();
    if let Some(id) = selected
        && !all.iter().any(|component| component.id == id)
    {
        return Err(Failure::invalid(
            "workstation_component_unknown",
            format!("`{id}` is not a governed workstation component"),
        )
        .remedy(crate::COMPONENT_UNKNOWN.remedy));
    }
    let components = all
        .iter()
        .filter(|component| selected.is_none_or(|id| component.id == id))
        .map(|component| {
            let local = detect::snapshot(component, platform, false);
            json!({
                "id": component.id,
                "required": component.required,
                "purpose": component.purpose,
                "provenance": component.provenance,
                "state": local["state"],
                "path": local["path"],
                "receipt": local.get("receipt").cloned().unwrap_or(Value::Null),
                "acquisition": {
                    "implemented": false,
                    "explicit_intent_required": true,
                    "reason": "installation and dataset acquisition are outside Skill Zero's proven lifecycle scope",
                }
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "platform": platform.token(),
        "mutated": false,
        "components": components,
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = String::from("workstation components · discovery only\n");
    for component in data["components"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<17} {} · {}\n",
            component["id"].as_str().unwrap_or("?"),
            component["state"].as_str().unwrap_or("unknown"),
            component["purpose"].as_str().unwrap_or("")
        ));
    }
    out
}
