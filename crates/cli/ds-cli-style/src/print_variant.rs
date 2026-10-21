//! Create one predictable Style Center print variant from a governed screen style.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{LANE_ARG, PROJECT_ARG, REF_ARG};

fn run_with(inputs: &Inputs, apply: bool) -> Result<Value, Failure> {
    crate::native::edit(
        inputs,
        crate::native::Edit::PrintVariant,
        json!({"ref": inputs.require("ref")?, "apply": apply}),
    )
}

fn render_result(data: &Value) -> String {
    format!(
        "{} → {} · {}\n",
        data["sourceRef"].as_str().unwrap_or("?"),
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
        id: "style.print.plan",
        path: &["style", "print", "plan"],
        contract: 1,
        summary: "Plan a create-only `_print` clone of one governed screen style.",
        purpose: "Copies the complete authored Style Center document, including catalog sprite names, into the predictable `<source-ref>_print` identity. It declares `print_paper_size` with print_a0..print_a5 and print_custom for later expression authoring. Nothing is saved.",
        chapter: Chapter::MapPresentation,
        effect: Effect::LocalAuthState,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[PROJECT_ARG, REF_ARG, LANE_ARG],
        output: "Source ref, `_print` ref, exact cloned document, paper expression vocabulary and create-only payload receipt.",
        examples: &[Example {
            command: "ds style print plan --project <id> --ref master/lv_lines --output json",
            note: "Review the exact clone before creating it.",
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
        id: "style.print.create",
        path: &["style", "print", "create"],
        contract: 1,
        summary: "Create the governed `_print` clone of one screen style.",
        purpose: "Publishes a create-only print style through the same Style Center write boundary. Existing `_print` styles are never overwritten; customize the created ref with the ordinary appearance, dimension and cartography commands.",
        chapter: Chapter::MapPresentation,
        effect: Effect::GlobalWrite,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[PROJECT_ARG, REF_ARG, LANE_ARG],
        output: "The print clone receipt with `published: true` and the exact persisted document.",
        examples: &[Example {
            command: "ds style print create --project <id> --ref master/lv_lines --yes --output json",
            note: "Creates master/lv_lines_print once.",
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
