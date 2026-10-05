//! Fenced upgrade of the known approved project A4 baseline; originals stay retained.
use crate::{LANE_ARG, PROJECT_ARG};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::style_governance::Command as Operation;
use serde_json::Value;

const DEFINITION: Arg = Arg::value(
    "expected-document",
    "<64hex>",
    "Exact destination definition_sha256 from the reviewed migration plan.",
)
.required();
const PLAN: Arg = Arg::value(
    "expected-plan",
    "<64hex>",
    "Exact plan_sha256 fencing this project, principal and existing A4 revision.",
)
.required();
const PURPOSE: &str = "Upgrade only the named project's recognized approved A4 baseline to the current approved template. The API retains the exact previous definition and HTML atomically before updating its head. Unknown authored content refuses. Plan writes nothing; apply requires printing.setup.edit and exact reviewed source/destination fences. Creation remains a separate create-only operation. No global document, style or printing default changes.";

macro_rules! command {
    ($module:ident,$id:literal,$summary:literal,$effect:ident,$args:expr) => {
        pub mod $module {
            use super::*;
            pub static COMMAND: Command = Command {
                id: $id, path: &["style", "catalogue", "a4", "migration", stringify!($module)],
                contract: 1, summary: $summary, purpose: PURPOSE, chapter: Chapter::MapPresentation,
                effect: Effect::$effect, authority: Authority::HeadlessProject, execution: Execution::Sync,
                args: $args, output: "Exact approved definition, original revision, retained project path, update/preserve counts and plan SHA. Apply returns the unchanged reviewed plan and verified retention flags.",
                examples: &[], refusals: crate::governance::command_refusals(),
                reference: Some("docs/reference/style.md"), search: &["voltage drop", "template"],
                requires: ds_cli_contract::spec::Requires::Server, availability: ds_cli_auth::native_availability,
            };
            pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
                let operation = if stringify!($module) == "plan" { Operation::PlanA4Migration } else {
                    Operation::ApplyA4Migration {
                        expected_definition_sha256: inputs.require("expected-document")?.into(),
                        expected_plan_sha256: inputs.require("expected-plan")?.into(),
                    }
                };
                crate::governance::invoke(inputs, operation)
            }
            pub fn render(data: &Value) -> String { format!("{}\n", serde_json::to_string_pretty(data).unwrap_or_default()) }
        }
    }
}
command!(
    plan,
    "style.catalogue.a4.migration.plan",
    "Review the project A4 baseline upgrade and immutable retention.",
    LocalAuthState,
    &[PROJECT_ARG, LANE_ARG]
);
command!(
    apply,
    "style.catalogue.a4.migration.apply",
    "Apply only the reviewed project A4 baseline upgrade.",
    GlobalWrite,
    &[PROJECT_ARG, DEFINITION, PLAN, LANE_ARG]
);
