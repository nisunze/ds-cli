//! Fixed governed voltage-drop status document; API alone owns creation.
use crate::{LANE_ARG, PROJECT_ARG};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::style_governance::Command as Operation;
use serde_json::Value;

const DEFINITION: Arg = Arg::value(
    "expected-document",
    "<64hex>",
    "Exact definition_sha256 from the reviewed A4 plan, including its HTML and body pins.",
)
.required();
const PLAN: Arg = Arg::value(
    "expected-plan",
    "<64hex>",
    "Exact plan_sha256 from that same project/principal; destination changes refuse atomically.",
)
.required();
const PURPOSE: &str = "Create only the fixed governed voltage-drop A4 status document. Plan holds its exact packaged HTML/body pins and definition, the authenticated project/principal and current destination. Creation requires styles.edit and map.defaults.edit; reads require project membership. Existing exact content is preserved, changed content refuses. No style, binding, layout, manifest head or printing-defaults pointer is changed. Review the plan, then confirm only this one document.";
macro_rules! command {
    ($module:ident,$id:literal,$leaf:literal,$summary:literal,$effect:ident,$args:expr) => {
        pub mod $module {
            use super::*;
            pub static COMMAND: Command = Command {
                id: $id, path: &["style", "catalogue", "a4", $leaf], contract: 1,
                summary: $summary, purpose: PURPOSE, chapter: Chapter::MapPresentation,
                effect: Effect::$effect, authority: Authority::HeadlessProject, execution: Execution::Sync,
                args: $args, output: "Fixed A4 definition/body/source digests, exact project/principal, destination state, create/preserve counts and plan SHA; create returns applied with its unchanged reviewed plan.",
                examples: &[], refusals: crate::governance::command_refusals(),
                reference: Some("docs/reference/style.md"), search: &["status", "template"],
                requires: ds_cli_contract::spec::Requires::Server, availability: ds_cli_auth::native_availability,
            };
            pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
                let operation = if $leaf == "plan" { Operation::PlanA4 } else {
                    Operation::CreateA4 { expected_definition_sha256: inputs.require("expected-document")?.into(), expected_plan_sha256: inputs.require("expected-plan")?.into() }
                };
                crate::governance::invoke(inputs, operation)
            }
            pub fn render(data: &Value) -> String { format!("{}\n", serde_json::to_string_pretty(data).unwrap_or_default()) }
        }
    }
}
command!(
    plan,
    "style.catalogue.a4.plan",
    "plan",
    "Review the fixed voltage-drop A4 document create-only plan.",
    LocalAuthState,
    &[PROJECT_ARG, LANE_ARG]
);
command!(
    create,
    "style.catalogue.a4.create",
    "create",
    "Create only the reviewed governed voltage-drop A4 document.",
    GlobalWrite,
    &[PROJECT_ARG, DEFINITION, PLAN, LANE_ARG]
);

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a4_create_is_globally_confirmed_and_carries_both_reviewed_fences() {
        assert_eq!(create::COMMAND.effect, Effect::GlobalWrite);
        assert_eq!(plan::COMMAND.effect, Effect::LocalAuthState);
        assert!(
            create::COMMAND
                .args
                .iter()
                .any(|arg| arg.name == "expected-document" && arg.required)
        );
        assert!(
            create::COMMAND
                .args
                .iter()
                .any(|arg| arg.name == "expected-plan" && arg.required)
        );
        for command in [&plan::COMMAND, &create::COMMAND] {
            assert!(
                !command
                    .args
                    .iter()
                    .any(|arg| ["path", "body", "manifest", "yes"].contains(&arg.name))
            );
        }
    }
}
