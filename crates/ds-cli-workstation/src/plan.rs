use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::detect::{self, Platform};

const FEEDBACK_ROUTE: &str = "ds feedback submit";

const PLATFORM_ARG: Arg = Arg::value(
    "platform",
    "<current|windows|macos|linux|browser>",
    "Target host; browser returns the typed unsupported-host refusal.",
)
.choices(&["current", "windows", "macos", "linux", "browser"])
.default("current");

const INTENT_ARG: Arg = Arg::value(
    "intent",
    "<install|configure>",
    "Plan acquisition or one integration setting; apply neither.",
)
.choices(&["install", "configure"])
.default("install");

const TARGET_ARG: Arg = Arg::value(
    "target",
    "<vscode|windows-terminal|ds-subprocess>",
    "The one Git Bash integration to configure later.",
)
.choices(&["vscode", "windows-terminal", "ds-subprocess"]);

pub static COMMAND: Command = Command {
    id: "workstation.plan",
    path: &["workstation", "plan"],
    contract: 1,
    chapter: Chapter::Workstation,
    summary: "Review a no-side-effect prerequisite or integration plan.",
    purpose: "Returns required setup steps and proof boundaries. It never runs a package manager, downloads, or writes settings.",
    effect: Effect::Proposal,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[crate::COMPONENT_ARG, PLATFORM_ARG, INTENT_ARG, TARGET_ARG],
    output: "Current state, ordered policy steps, authorization/proof boundaries, and the typed feedback route. `mutated` is always false.",
    examples: &[
        Example {
            command: "ds workstation plan --component libreoffice --platform windows --output json",
            note: "Proven package-manager path and official-Metalink/hash fallback policy; no download.",
            runnable: true,
        },
        Example {
            command: "ds workstation plan --component git-bash --platform windows --intent configure --target vscode --output json",
            note: "One settings target; no write.",
            runnable: true,
        },
    ],
    refusals: &[
        crate::UNSUPPORTED_PLATFORM,
        crate::COMPONENT_UNKNOWN,
        crate::PLAN_INVALID,
    ],
    reference: Some("docs/reference/workstation.md"),
    availability: crate::always,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let component_id = inputs.require("component")?;
    let component = detect::component(component_id).ok_or_else(|| {
        Failure::invalid(
            "workstation_component_unknown",
            format!("`{component_id}` is not a governed workstation component"),
        )
        .remedy(crate::COMPONENT_UNKNOWN.remedy)
    })?;
    let platform_token = inputs.require("platform")?;
    if platform_token == "browser" {
        return Err(Failure::unavailable(
            "workstation_platform_unsupported",
            "a browser host cannot inspect or configure local workstation prerequisites",
        )
        .remedy(crate::UNSUPPORTED_PLATFORM.remedy));
    }
    let platform = match platform_token {
        "current" => Platform::current(),
        "windows" => Platform::Windows,
        "macos" => Platform::Macos,
        "linux" => Platform::Linux,
        _ => unreachable!("the declared argument choices reject other platforms"),
    };
    let intent = inputs.require("intent")?;
    let target = inputs.value("target");
    if intent == "configure"
        && (component_id != "git-bash" || platform != Platform::Windows || target.is_none())
    {
        return Err(Failure::invalid(
            "workstation_plan_invalid",
            "configuration planning requires Git Bash on Windows and one explicit target",
        )
        .remedy(crate::PLAN_INVALID.remedy));
    }
    if intent == "install" && target.is_some() {
        return Err(Failure::invalid(
            "workstation_plan_invalid",
            "`--target` belongs only to a configuration plan",
        )
        .remedy(crate::PLAN_INVALID.remedy));
    }
    if intent == "configure" && target == Some("ds-subprocess") {
        return Err(Failure::invalid(
            "workstation_plan_invalid",
            "ds uses direct typed process execution and owns no default subprocess shell to replace",
        )
        .remedy("choose --target vscode or --target windows-terminal; leave Remote-SSH on its native shell"));
    }

    let current_state = if platform == Platform::current() {
        detect::snapshot(&component, platform, false)
    } else {
        json!({"state": "not_inspected", "reason": "a plan for another platform does not claim local discovery"})
    };
    let already_satisfied = current_state["state"] == "installed";
    let steps = if intent == "configure" {
        let target = target.expect("configuration target was validated");
        vec![
            format!("Read the current {target} setting and retain a before value."),
            "Merge only the selected default-profile key; preserve unrelated settings and keep Remote-SSH on the remote native shell.".to_string(),
            "Return a before/after receipt and make an identical second call a no-op.".to_string(),
        ]
    } else {
        component.plan(platform).to_vec()
    };
    let implemented = (intent == "install"
        && ((component_id == "libreoffice" && platform == Platform::Windows)
            || component_id == "rwanda-reference"))
        || (intent == "configure" && target == Some("vscode"));
    Ok(json!({
        "component": component.id,
        "platform": platform.token(),
        "intent": intent,
        "target": target,
        "current": current_state,
        "already_satisfied": already_satisfied,
        "mutated": false,
        "authorized": false,
        "implementation": if implemented { "available" } else { "planning_only" },
        "steps": steps,
        "constraints": {
            "explicit_future_authorization_required": true,
            "never_reinstall_detected_suitable_component": true,
            "never_remove_preexisting_component": true,
            "task_owned_cleanup_only": true,
            "qgis_install_requires_explicit_request": component_id == "qgis",
            "rwanda_download_requires_explicit_request_and_receipt": component_id == "rwanda-reference",
            "windows_libreoffice_lifecycle_proven": component_id == "libreoffice" && platform == Platform::Windows,
            "libreoffice_mcp_required": false,
            "third_party_qgis_mcp_allowed": false,
        },
        "evidence": if component_id == "libreoffice" && platform == Platform::Windows { json!({
            "state": "proven",
            "feedback_report": "368cdd5a-eb52-4f30-982a-97c5d1dd2e65"
        }) } else { Value::Null },
        "remaining_gap": if implemented { Value::Null } else { json!({
            "route": FEEDBACK_ROUTE,
            "title": "Prove and implement this exact workstation lifecycle",
            "acceptance": "Lifecycle-tested idempotence, verification, task-owned cleanup, and exact settings or acquisition evidence on the owning local host."
        }) }
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "{} {} plan for {} · no changes\n",
        data["component"].as_str().unwrap_or("?"),
        data["intent"].as_str().unwrap_or("?"),
        data["platform"].as_str().unwrap_or("?")
    );
    for step in data["steps"].as_array().into_iter().flatten() {
        out.push_str(&format!("  - {}\n", step.as_str().unwrap_or("")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_component_plan_is_data_driven_and_mutation_free() {
        for component in detect::catalog() {
            for platform in [Platform::Windows, Platform::Macos, Platform::Linux] {
                assert!(!component.plan(platform).is_empty());
            }
        }
    }

    #[test]
    fn feedback_gap_uses_the_existing_typed_route() {
        for component in detect::catalog() {
            let text = component.plan(Platform::Windows).join(" ");
            assert!(!text.contains("curl "));
            assert!(!text.contains("Invoke-WebRequest"));
        }
        assert_eq!(FEEDBACK_ROUTE, "ds feedback submit");
    }
}
