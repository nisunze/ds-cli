//! Begin one deliberate, visible transformer version.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

const TRANSFORMERS_ARG: Arg = Arg {
    name: "transformer",
    kind: ArgKind::Repeated,
    value: "<name>",
    required: true,
    default: None,
    choices: &[],
    summary: "Transformer to version. Repeat for one deliberate batch.",
};

const REASON_ARG: Arg = Arg {
    name: "reason",
    kind: ArgKind::Value,
    value: "<text>",
    required: true,
    default: None,
    choices: &[],
    summary: "Why this visible version is being created (1-280 characters).",
};

pub static COMMAND: Command = Command {
    id: "map.design.version.begin",
    path: &["map", "design", "version", "begin"],
    contract: 2,
    summary: "Create a deliberate visible transformer version.",
    purpose: "Captures immutable offline version rooms through the paired application's shared Rust command kernel. Current local edits are included. Only names and one reason cross the bridge. Completion means durable local storage; cloud synchronization remains pending.",
    chapter: Chapter::Design,
    effect: Effect::ArtifactWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMERS_ARG, REASON_ARG, DESCRIPTOR_ARG],
    output: "Project, reason, created and failed counts, and bounded metadata per transformer with exact local-<digest> version identity. persisted means durable local version rooms, not a cloud acknowledgement.",
    examples: &[Example {
        command: "ds map design version begin --transformer agasharu --reason \"Drafting baseline approved\" --yes --output json",
        note: "Advances the visible version once; later saves remain in that version until another explicit create.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "a transformer does not exist, or version creation was declined",
            remedy: "check the exact transformer names and read detail.detail",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
        Refusal {
            code: "missing_input",
            when: "no --transformer was given",
            remedy: "pass --transformer <name>; repeat it for a deliberate batch",
        },
        Refusal {
            code: "invalid_input",
            when: "--reason is empty or longer than 280 characters",
            remedy: "give the version a concise reason that will remain useful in history",
        },
        Refusal {
            code: "confirmation_required",
            when: "--yes was not given for a command that creates retained project history",
            remedy: "re-run with --yes once the exact transformers and reason are intentional",
        },
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformers = inputs.repeated("transformer");
    if transformers.is_empty() {
        return Err(
            Failure::invalid("missing_input", "--transformer is required")
                .remedy("pass --transformer <name>; repeat it for a deliberate batch"),
        );
    }
    let reason = inputs.require("reason")?;
    if reason.trim().is_empty() || reason.chars().count() > 280 {
        return Err(
            Failure::invalid("invalid_input", "--reason must contain 1-280 characters")
                .remedy("give the version a concise reason that will remain useful in history"),
        );
    }
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_VERSION_BEGIN,
        json!({ "transformers": transformers, "reason": reason }),
        crate::DESIGN_STAGE_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;
    Ok(json!({
        "project": result["project"],
        "reason": result["reason"],
        "total": result["total"],
        "created": result["created"],
        "failed": result["failed"],
        "results": result["results"],
    }))
}

pub fn render(data: &Value) -> String {
    let created = data["created"].as_u64().unwrap_or(0);
    let failed = data["failed"].as_u64().unwrap_or(0);
    let mut output = format!("created {created} design version(s)  -  {failed} failed\n");
    if let Some(rows) = data["results"].as_array() {
        for row in rows {
            let name = row["transformer"].as_str().unwrap_or("?");
            if row["ok"].as_bool().unwrap_or(false) {
                output.push_str(&format!(
                    "  {name}  {}\n",
                    row["versionId"].as_str().unwrap_or("?")
                ));
            } else {
                output.push_str(&format!(
                    "  {name}  failed: {}\n",
                    row["error"].as_str().unwrap_or("refused")
                ));
            }
        }
    }
    output
}
