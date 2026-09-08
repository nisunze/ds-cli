//! Persist one backend-declared print-context source style exactly once.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, HOST_ARG, LANE_ARG, PROJECT_ARG, REF_ARG};

fn run_with(inputs: &Inputs, context: &Context, apply: bool) -> Result<Value, Failure> {
    crate::native::execute(
        inputs,
        context,
        &crate::STYLE_SEED_CREATE,
        json!({"ref": inputs.require("ref")?, "apply": apply}),
    )
}

fn render_result(data: &Value) -> String {
    format!(
        "{} · {}\n",
        data["ref"].as_str().unwrap_or("?"),
        if data["published"].as_bool().unwrap_or(false) {
            "created"
        } else {
            "plan only — nothing published"
        }
    )
}

pub mod plan {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "style.seed.plan",
        path: &["style", "seed", "plan"],
        contract: 1,
        summary: "Plan a create-only seed of one declared print-context style.",
        purpose: "Uses the backend-declared geometry, field vocabulary, palette and save routing for a renderer-only buildings or contour source. Nothing is saved and arbitrary source refs are refused by the shared Rust planner.",
        chapter: Chapter::MapPresentation,
        effect: Effect::LocalAuthState,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[REF_ARG, HOST_ARG, PROJECT_ARG, LANE_ARG, DESCRIPTOR_ARG],
        output: "The declared source ref, exact backend-owned style document and create-only payload receipt.",
        examples: &[Example {
            command: "ds style seed plan --ref print_context/contours_index --output json",
            note: "Review the exact declared seed before creating it.",
            runnable: false,
        }],
        refusals: crate::native::REFUSALS,
        reference: Some("docs/reference/style.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
        run_with(inputs, context, false)
    }
    pub fn render(data: &Value) -> String {
        render_result(data)
    }
}

pub mod create {
    use super::*;

    pub static COMMAND: Command = Command {
        id: "style.seed.create",
        path: &["style", "seed", "create"],
        contract: 1,
        summary: "Create one declared print-context source style.",
        purpose: "Publishes the exact backend-declared buildings or contour source style once. Existing source styles are preserved; use ordinary guided style commands for later edits and then create its governed `_print` variant.",
        chapter: Chapter::MapPresentation,
        effect: Effect::GlobalWrite,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[REF_ARG, HOST_ARG, PROJECT_ARG, LANE_ARG, DESCRIPTOR_ARG],
        output: "The seed receipt with `published: true` and the exact persisted source document.",
        examples: &[Example {
            command: "ds style seed create --ref print_context/contours_index --yes --output json",
            note: "Creates the canonical index-contour source style once.",
            runnable: false,
        }],
        refusals: crate::native::REFUSALS,
        reference: Some("docs/reference/style.md"),
        availability: crate::paired_availability,
    };

    pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
        run_with(inputs, context, true)
    }
    pub fn render(data: &Value) -> String {
        render_result(data)
    }
}
