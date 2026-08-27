use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::detect::{self, Platform};

pub static COMMAND: Command = Command {
    id: "workstation.verify",
    path: &["workstation", "verify"],
    contract: 1,
    chapter: Chapter::Workstation,
    summary: "Verify discovered executables and governed component receipts.",
    purpose: "Runs only fixed harmless version probes and validates local DS component receipts and hashes. LibreOffice document-conversion lifecycle proof remains explicitly deferred; this command does not claim it.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[crate::OPTIONAL_COMPONENT_ARG],
    output: "Per-component discovery and bounded verification result, with the exact proof level and any intentionally deferred lifecycle smoke.",
    examples: &[Example {
        command: "ds workstation verify --component libreoffice --output json",
        note: "Proves executable/version only in this interim scope; no document is converted.",
        runnable: true,
    }],
    refusals: &[crate::COMPONENT_UNKNOWN],
    reference: Some("docs/reference/workstation.md"),
    availability: crate::always,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let selected = inputs.value("component");
    let catalog = detect::catalog();
    if let Some(id) = selected
        && !catalog.iter().any(|component| component.id == id)
    {
        return Err(Failure::invalid(
            "workstation_component_unknown",
            format!("`{id}` is not a governed workstation component"),
        )
        .remedy(crate::COMPONENT_UNKNOWN.remedy));
    }
    let platform = Platform::current();
    let results = catalog
        .iter()
        .filter(|component| selected.is_none_or(|id| component.id == id))
        .map(|component| {
            let snapshot = detect::snapshot(component, platform, true);
            let verified = match component.id.as_str() {
                "rwanda-reference" => snapshot["receipt"]["verified"] == true,
                "git-bash" if platform != Platform::Windows => true,
                _ => snapshot["state"] == "installed" && snapshot["version"].is_string(),
            };
            json!({
                "id": component.id,
                "verified": verified,
                "proof": if component.id == "rwanda-reference" { "receipt_and_file_hashes" } else if verified { "executable_and_version" } else { "not_proven" },
                "discovery": snapshot,
                "functional_smoke": if component.id == "libreoffice" {
                    json!({
                        "state": "deferred",
                        "reason": "headless document-conversion and Windows install/uninstall/reinstall lifecycle proof belong to a future dedicated session"
                    })
                } else {
                    Value::Null
                },
                "mutated": false,
            })
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "platform": platform.token(),
        "mutated": false,
        "results": results,
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = String::from("workstation verification · no changes\n");
    for result in data["results"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<17} {} ({})\n",
            result["id"].as_str().unwrap_or("?"),
            if result["verified"].as_bool().unwrap_or(false) {
                "verified"
            } else {
                "not proven"
            },
            result["proof"].as_str().unwrap_or("unknown")
        ));
    }
    out
}
