//! Persist one backend-declared print-context source style exactly once.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{LANE_ARG, PROJECT_ARG, REF_ARG};

fn run_with(inputs: &Inputs, apply: bool) -> Result<Value, Failure> {
    crate::native::edit(
        inputs,
        crate::native::Edit::Seed,
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
        args: &[PROJECT_ARG, REF_ARG, LANE_ARG],
        output: "The declared source ref, exact backend-owned style document and create-only payload receipt.",
        examples: &[Example {
            command: "ds style seed plan --project <id> --ref print_context/contours_index --output json",
            note: "Review the exact declared seed before creating it.",
            runnable: false,
        }],
        refusals: crate::native::REFUSALS,
        reference: Some("docs/reference/style.md"),
        search: &[],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    };

    pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
        run_with(inputs, false)
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
        args: &[PROJECT_ARG, REF_ARG, LANE_ARG],
        output: "The seed receipt with `published: true` and the exact persisted source document.",
        examples: &[Example {
            command: "ds style seed --project <id> create --ref print_context/contours_index --yes --output json",
            note: "Creates the canonical index-contour source style once.",
            runnable: false,
        }],
        refusals: crate::native::PUBLISH_REFUSALS,
        reference: Some("docs/reference/style.md"),
        search: &[],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    };

    pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
        run_with(inputs, true)
    }
    pub fn render(data: &Value) -> String {
        render_result(data)
    }
}
