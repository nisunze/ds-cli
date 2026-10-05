//! Create-only print tuple binding to one existing governed style head.
use crate::{LANE_ARG, PROJECT_ARG};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::style_governance::Command as Operation;
use ds_command_kernel::style_resolution::{Ink, Key, Target};
use serde_json::Value;

const ENTITY: Arg = Arg::value(
    "entity-class",
    "<exact-class>",
    "Exact entity class from the blocked print layer.",
)
.required();
const SOURCE: Arg = Arg::value(
    "source-kind",
    "<exact-kind>",
    "Exact source kind from the blocked print layer.",
)
.required();
const ROLE: Arg = Arg::value(
    "role",
    "<exact-role>",
    "Exact print role from the blocked layer; no role is inferred.",
)
.required();
const INK: Arg = Arg::value("ink", "<colour|monochrome>", "Independent print ink tuple.")
    .choices(&["colour", "monochrome"])
    .default("colour");
const STYLE: Arg = Arg::value(
    "ref",
    "<existing-print-ref>",
    "Exact existing _print style ref; no style document is created or changed.",
)
.required();
const MANIFEST: Arg = Arg::value(
    "expected-manifest",
    "<64hex>",
    "Exact manifest_revision from the reviewed binding plan.",
)
.required();
const PLAN: Arg = Arg::value(
    "expected-plan",
    "<64hex>",
    "Exact plan_sha256; a changed manifest or style head refuses atomically.",
)
.required();
const PURPOSE: &str = "Bind one exact print entity/source/role/ink tuple to one already persisted print style. The Styles API captures the named project, authenticated principal, current manifest revision, exact style head/content digests and update time. Creation requires styles.edit and the reviewed manifest/plan fences. Existing exact bindings are preserved; a conflicting tuple/ref refuses. No style body, layout, printing defaults or global page is written. The prior manifest is retained. Template projects use ordinary project context.";

fn operation(inputs: &Inputs, create: bool) -> Result<Operation, Failure> {
    let key = Key {
        entity_class: inputs.require("entity-class")?.into(),
        source_kind: inputs.require("source-kind")?.into(),
        target: Target::Print,
        role: inputs.require("role")?.into(),
        ink: match inputs.require("ink")? {
            "colour" => Ink::Colour,
            "monochrome" => Ink::Monochrome,
            _ => {
                return Err(Failure::invalid(
                    "style_governance_invalid",
                    "Print ink must be colour or monochrome",
                ));
            }
        },
    };
    let style_ref = inputs.require("ref")?.into();
    Ok(if create {
        Operation::CreateBinding {
            key,
            style_ref,
            expected_manifest_revision: inputs.require("expected-manifest")?.into(),
            expected_plan_sha256: inputs.require("expected-plan")?.into(),
        }
    } else {
        Operation::PlanBinding { key, style_ref }
    })
}

macro_rules! command {
    ($module:ident,$id:literal,$leaf:literal,$summary:literal,$effect:ident,$args:expr,$create:expr) => {
        pub mod $module {
            use super::*;
            pub static COMMAND: Command = Command {
                id: $id, path: &["style", "catalogue", "binding", $leaf], contract: 1,
                summary: $summary, purpose: PURPOSE, chapter: Chapter::MapPresentation,
                effect: Effect::$effect, authority: Authority::HeadlessProject, execution: Execution::Sync,
                args: $args,
                output: "ds.style-binding-plan/v1 with exact tuple/ref, project/principal, manifest and style head/content fences, create/preserve counts and plan SHA; creation returns the unchanged reviewed plan and resulting manifest revision.",
                examples: &[], refusals: crate::governance::command_refusals(),
                reference: Some("docs/reference/style.md"), search: &["print", "tuple", "governance"],
                requires: Requires::Server, availability: ds_cli_auth::native_availability,
            };
            pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
                crate::governance::invoke(inputs, operation(inputs, $create)?)
            }
            pub fn render(data: &Value) -> String {
                format!("{}\n", serde_json::to_string_pretty(data).unwrap_or_default())
            }
        }
    }
}
command!(
    plan,
    "style.catalogue.binding.plan",
    "plan",
    "Review one exact print binding to an existing style.",
    LocalAuthState,
    &[PROJECT_ARG, ENTITY, SOURCE, ROLE, INK, STYLE, LANE_ARG],
    false
);
command!(
    create,
    "style.catalogue.binding.create",
    "create",
    "Create only the reviewed print tuple binding.",
    GlobalWrite,
    &[
        PROJECT_ARG,
        ENTITY,
        SOURCE,
        ROLE,
        INK,
        STYLE,
        MANIFEST,
        PLAN,
        LANE_ARG
    ],
    true
);

#[cfg(test)]
mod tests {
    use super::*;
    use ds_command_kernel::style_governance::{Request, request_body};
    use serde_json::json;

    #[test]
    fn exact_print_flags_preserve_the_key_and_both_reviewed_fences() {
        let mut flags: Vec<String> = [
            "--project",
            "project_a",
            "--entity-class",
            "survey_existing_poles",
            "--source-kind",
            "live_geojson",
            "--role",
            "project",
            "--ref",
            "lv_poles_as_built_print",
            "--ink",
            "monochrome",
        ]
        .into_iter()
        .map(Into::into)
        .collect();
        let inputs = ds_cli_contract::parse(&plan::COMMAND, &flags).unwrap();
        let body = request_body(&Request {
            project: "project_a".into(),
            command: operation(&inputs, false).unwrap(),
        })
        .unwrap();
        assert_eq!(
            body["data"]["key"],
            json!({
                "entity_class":"survey_existing_poles", "source_kind":"live_geojson",
                "role":"project", "target":"print", "ink":"monochrome"
            })
        );
        assert_eq!(body["data"]["style_ref"], "lv_poles_as_built_print");
        assert!(ds_cli_contract::parse(&create::COMMAND, &flags).is_err());
        flags.extend([
            "--expected-manifest".into(),
            "a".repeat(64),
            "--expected-plan".into(),
            "b".repeat(64),
        ]);
        let inputs = ds_cli_contract::parse(&create::COMMAND, &flags).unwrap();
        let body = request_body(&Request {
            project: "project_a".into(),
            command: operation(&inputs, true).unwrap(),
        })
        .unwrap();
        assert_eq!(body["data"]["expected_manifest_revision"], "a".repeat(64));
        assert_eq!(body["data"]["expected_plan_sha256"], "b".repeat(64));
        flags.extend(["--body".into(), "{}".into()]);
        assert!(ds_cli_contract::parse(&create::COMMAND, &flags).is_err());
    }
}
