//! Exact API resolution, create-only standard seeding and backup-first audits.
use crate::{LANE_ARG, PROJECT_ARG};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::style_governance::Command as Operation;
use serde_json::{Value, json};

const ENTITY: Arg = Arg::value(
    "entity-class",
    "<declared-class>",
    "Exact governed entity class; read style resolution table for the declared combinations.",
)
.required();
const SOURCE: Arg=Arg::value("source-kind","<declared-kind>","Exact source kind; live GeoJSON, vector tiles, cold existing and saved print designs are independent.").required();
const TARGET: Arg = Arg::value(
    "target",
    "<screen|print>",
    "Independent screen or paper document authority.",
)
.choices(&["screen", "print"])
.required();
const ROLE: Arg = Arg::value(
    "role",
    "<declared-role>",
    "Exact role, such as map, focused, context, mv_context or project; no fallback is inferred.",
)
.required();
const INK: Arg = Arg::value(
    "ink",
    "<colour|monochrome>",
    "Governed paper ink document; colour preserves the default tuple, monochrome requires target print.",
)
.choices(&["colour", "monochrome"])
.default("colour");
const MANIFEST_REVISION: Arg = Arg::value(
    "expected-manifest",
    "<64hex>",
    "Exact manifest_revision from the reviewed seed plan.",
)
.required();
const PLAN: Arg = Arg::value(
    "expected-plan",
    "<64hex>",
    "Exact plan_sha256 from the reviewed seed plan; changed heads refuse atomically.",
)
.required();
const INVENTORY: Arg = Arg::value(
    "expected-inventory",
    "<64hex>",
    "Complete reviewed inventory_sha256; changed inventory refuses.",
)
.required();
const BACKUP: Arg = Arg::value(
    "backup",
    "<exact-id>",
    "Immutable API backup id; retirement planning never deletes documents.",
)
.required();
const CURSOR: Arg = Arg::value(
    "cursor",
    "<opaque-cursor>",
    "Exact continuation returned by the preceding inventory page.",
);
const ALL_PROJECTS: Arg = Arg::switch(
    "all-projects",
    "Inventory all project scopes and unselected styling templates; the API additionally requires platform.admin.",
);
const LIMIT: Arg = Arg::value("limit", "<1..200>", "Bounded inventory page size.").default("100");
const PURPOSE_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "style_purpose_not_composition",
        when: "the selected purpose declares style editing rather than printing pages",
        remedy: "select a print_composition purpose returned by style purpose index",
    },
    Refusal {
        code: "style_purpose_layout_revision_mismatch",
        when: "a captured Templates document differs from the purpose's revision or content pins",
        remedy: "review the exact authored purpose and printing revision together; never substitute a newer head",
    },
    Refusal {
        code: "print_template_invalid",
        when: "the separate Templates capture has an invalid scope, paper or page body",
        remedy: "repair the governed printing capture; a style binding does not authorize a layout body",
    },
    Refusal {
        code: "print_template_missing",
        when: "the Templates capture lacks a required purpose page role",
        remedy: "capture the exact governed document declared by the purpose",
    },
    Refusal {
        code: "print_template_ambiguous",
        when: "the Templates capture repeats the same template and page role",
        remedy: "repair the duplicate printing capture rather than choosing its first body",
    },
    Refusal {
        code: "print_template_revision_mismatch",
        when: "a captured printing body differs from its own content pin",
        remedy: "read a fresh authorized printing capture and verify its immutable bytes",
    },
    Refusal {
        code: "style_purpose_missing",
        when: "held governed documents declare no purpose for the exact target and ink",
        remedy: "have the style owner author versioned purpose declarations; reads never invent or seed them",
    },
    Refusal {
        code: "style_purpose_invalid",
        when: "a purpose declaration, role, target, layout reference or bound is invalid",
        remedy: "repair the named declaration in its governed source document",
    },
    Refusal {
        code: "style_purpose_ambiguous",
        when: "more than one held document declares the same purpose for the target and ink",
        remedy: "resolve the duplicate through the governed document owner",
    },
    Refusal {
        code: "style_purpose_role_missing",
        when: "a required purpose role has no explicit declaration",
        remedy: "author every required role and exact tuple; do not substitute a neighbouring source or role",
    },
    Refusal {
        code: "style_purpose_line_type_missing",
        when: "a line role lacks an explicit linetype constraint or its held document lacks line-dasharray",
        remedy: "have the style owner declare the role's exact linetype; no default pattern is inferred",
    },
    Refusal {
        code: "style_purpose_constraint_mismatch",
        when: "the held role document differs from its authored linetype constraint",
        remedy: "reconcile the governed purpose and role documents before composing",
    },
    Refusal {
        code: "style_purpose_layout_capture_required",
        when: "the purpose requires printing roles whose separate Templates capture was not supplied",
        remedy: "capture and admit the exact printing Templates through the print composition owner before preview or delivery",
    },
];
pub const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "project_context_changed",
        when: "the returned table or style capture belongs to another project",
        remedy: "read the resolution again under the explicitly named project",
    },
    Refusal {
        code: "style_governance_not_found",
        when: "the exact manifest, document or immutable backup is absent",
        remedy: "copy its exact id from the preceding API read",
    },
    Refusal {
        code: "style_governance_forbidden",
        when: "the account lacks the required style or all-project inventory capability",
        remedy: "use the authorized project scope or request platform.admin for an all-project census",
    },
    Refusal {
        code: "style_governance_required",
        when: "an incompatible seed or unmatched backup prevents the operation",
        remedy: "read the exact refusal, repair the named prerequisite through its owner and review again",
    },
    Refusal {
        code: "style_seed_plan_conflict",
        when: "heads changed after the reviewed seed plan",
        remedy: "read and review a fresh seed plan",
    },
    Refusal {
        code: "style_governance_conflict",
        when: "the expected manifest revision changed",
        remedy: "read the current manifest and review a fresh plan",
    },
    Refusal {
        code: "style_governance_bound",
        when: "the census or backup exceeds its declared bound",
        remedy: "follow the named continuation or narrow scope; do not treat an incomplete inventory as complete",
    },
    Refusal {
        code: "style_governance_failed",
        when: "the governed API failed unexpectedly",
        remedy: "retry the same bounded read; report a repeated server failure",
    },
    Refusal {
        code: "style_resolution_unknown",
        when: "no governed document declares the exact class/source/target/role tuple",
        remedy: "read style resolution table; request the missing combination from the style owner",
    },
    Refusal {
        code: "style_resolution_ambiguous",
        when: "more than one document declares the tuple",
        remedy: "repair the governed resolution table through the API",
    },
    Refusal {
        code: "style_resolution_migration_required",
        when: "an exact installed legacy scaling head still requires its declared versioned migration",
        remedy: "review style catalogue seed plan and apply its exact manifest/plan fences before releasing print consumers",
    },
    Refusal {
        code: "style_resolution_unseeded",
        when: "the standard manifest or its required persisted documents are absent",
        remedy: "review style catalogue seed plan, then apply that exact plan with its fences",
    },
    Refusal {
        code: "style_resolution_revision_mismatch",
        when: "a held document differs from its exact content digest",
        remedy: "read a fresh style resolution table under the same project",
    },
    Refusal {
        code: "style_resolution_invalid",
        when: "the table, document authority or semantic key is invalid",
        remedy: "read the named API refusal and repair its governed document",
    },
    Refusal {
        code: "style_governance_invalid",
        when: "a digest, cursor, backup or governed receipt is invalid",
        remedy: "use exact values from the preceding read or plan",
    },
    Refusal {
        code: "style_seed_conflict",
        when: "the manifest or inventory changed after review",
        remedy: "read and review a new seed plan",
    },
    Refusal {
        code: "style_inventory_conflict",
        when: "the captured inventory changed after review",
        remedy: "read a fresh complete inventory and create a matching backup",
    },
];
fn named(failure: Failure) -> Failure {
    if let Some(detail) = failure.detail_value()
        && let Some(code) = detail["service_code"].as_str()
        && let Some(refusal) = REFUSALS.iter().find(|r| r.code == code)
    {
        let message = detail["service_message"]
            .as_str()
            .unwrap_or(failure.message());
        let result = match code {
            "style_governance_forbidden" => Failure::unauthorized(refusal.code, message),
            "style_governance_failed" => Failure::failed(refusal.code, message),
            _ => Failure::invalid(refusal.code, message),
        };
        return result.remedy(refusal.remedy).detail(detail.clone());
    }
    crate::native::named(failure)
}
pub(crate) fn invoke(inputs: &Inputs, operation: Operation) -> Result<Value, Failure> {
    let request = ds_command_kernel::style_governance::Request {
        project: inputs.require("project")?.into(),
        command: operation,
    };
    ds_command_kernel::style_governance::request_body(&request)
        .map_err(|e| Failure::invalid("style_governance_invalid", e))?;
    ds_cli_auth::style_governance(inputs.require("lane")?, &request.project, &request.command)
        .map_err(named)
}
macro_rules! command {
    ($module:ident,$id:literal,$path:expr,$summary:literal,$effect:ident,$args:expr,$output:literal)=>{pub mod $module{use super::*;
        pub static COMMAND:Command=Command{id:$id,path:$path,contract:1,summary:$summary,purpose:"Use the captured project's fixed Styles API. Standard documents and bindings are declarative and independently versioned. Seeds create missing style heads and closed governed printing records (global MV layouts, standard page bodies and defaults); publishing printing records also needs map.defaults.edit. Existing authored printing documents are preserved or refuse drift. Explicit baseline migrations append versions only behind exact original-head fences. Authored changes survive. Inventory names its exact global and selected-project scope and immutable backup. Retirement is a deletion-disabled dry run; this command never deletes a style. Reads never seed implicitly.",chapter:Chapter::MapPresentation,effect:Effect::$effect,authority:Authority::HeadlessProject,execution:Execution::Sync,args:$args,output:$output,examples:&[],refusals:crate::governance::command_refusals(),reference:Some("docs/reference/style.md"),search:&[],requires:Requires::Server,availability:ds_cli_auth::native_availability};
        pub fn run(inputs:&Inputs,_:&Context)->Result<Value,Failure>{super::run(inputs,$id)}
        pub fn render(data:&Value)->String{format!("{}\n",serde_json::to_string_pretty(data).unwrap_or_default())}
    }};
}
const fn all_refusals() -> [Refusal; REFUSALS.len() + crate::native::PUBLISH_REFUSALS.len()] {
    let mut result = [REFUSALS[0]; REFUSALS.len() + crate::native::PUBLISH_REFUSALS.len()];
    let mut i = 0;
    while i < REFUSALS.len() {
        result[i] = REFUSALS[i];
        i += 1;
    }
    let mut j = 0;
    while j < crate::native::PUBLISH_REFUSALS.len() {
        result[i + j] = crate::native::PUBLISH_REFUSALS[j];
        j += 1;
    }
    result
}
const ALL: [Refusal; REFUSALS.len() + crate::native::PUBLISH_REFUSALS.len()] = all_refusals();
pub(crate) const fn command_refusals() -> &'static [Refusal] {
    &ALL
}
command!(
    resolve,
    "style.resolve",
    &["style", "resolve"],
    "Resolve an exact style key to a governed document revision.",
    LocalAuthState,
    &[PROJECT_ARG, ENTITY, SOURCE, TARGET, ROLE, INK, LANE_ARG],
    "Exact style_ref, revision_id, content_sha256, scope and held document; unknown tuple is refused by name."
);
command!(
    table,
    "style.resolution.table",
    &["style", "resolution", "table"],
    "Read the persisted governed style-resolution table.",
    LocalAuthState,
    &[PROJECT_ARG, LANE_ARG],
    "ds.style-resolution/v1 snapshot with captured project, revision and all exact bindings."
);
const fn purpose_refusals() -> [Refusal; ALL.len() + PURPOSE_REFUSALS.len()] {
    let mut result = [ALL[0]; ALL.len() + PURPOSE_REFUSALS.len()];
    let mut i = 0;
    while i < ALL.len() {
        result[i] = ALL[i];
        i += 1;
    }
    let mut j = 0;
    while j < PURPOSE_REFUSALS.len() {
        result[i + j] = PURPOSE_REFUSALS[j];
        j += 1;
    }
    result
}
const PURPOSE_ALL: [Refusal; ALL.len() + PURPOSE_REFUSALS.len()] = purpose_refusals();
pub mod purpose_index {
    use super::*;
    pub static COMMAND: Command = Command {
        id: "style.purpose.index",
        path: &["style", "purpose", "index"],
        contract: 1,
        summary: "Read authored purpose groups and their exact held style roles.",
        purpose: "Read metadata.style_purposes from the named project's immutable governed style documents. Rust resolves every required exact tuple and authored linetype constraint, preserving target, ink, source kind, role and revision. Missing declarations, roles or constraints refuse; no adjacent style or default is inferred. Required printing layout references remain separate Templates authority and return explicit capture-required blockers; this style table alone never admits layout bodies. No seed, adoption, preview or production write occurs.",
        chapter: Chapter::MapPresentation,
        effect: Effect::LocalAuthState,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[PROJECT_ARG, TARGET, INK, LANE_ARG],
        output: "ds.style-purpose-index/v1: captured project/table revision, authored groups and roles with exact held bindings, separate required layout references, composition_ready and explicit layout capture blockers; at most 64 groups and 32 MiB.",
        examples: &[],
        refusals: &PURPOSE_ALL,
        reference: Some("docs/reference/style.md"),
        search: &["transformer sheet", "adjacent circuit"],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    };
    pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
        let snapshot = serde_json::from_value(invoke(inputs, Operation::Table)?)
            .map_err(|error| Failure::invalid("style_resolution_invalid", error.to_string()))?;
        ds_command_kernel::style_purpose::index(
            inputs.require("project")?,
            &snapshot,
            if inputs.require("target")? == "print" {
                ds_command_kernel::style_resolution::Target::Print
            } else {
                ds_command_kernel::style_resolution::Target::Screen
            },
            if inputs.require("ink")? == "monochrome" {
                ds_command_kernel::style_resolution::Ink::Monochrome
            } else {
                ds_command_kernel::style_resolution::Ink::Colour
            },
        )
        .map_err(|error| {
            let remedy = PURPOSE_ALL
                .iter()
                .find(|refusal| refusal.code == error.code)
                .map(|refusal| refusal.remedy);
            let failure = Failure::invalid(error.code, error.message);
            if let Some(remedy) = remedy {
                failure.remedy(remedy)
            } else {
                failure
            }
        })
    }
    pub fn render(data: &Value) -> String {
        format!(
            "{}\n",
            serde_json::to_string_pretty(data).unwrap_or_default()
        )
    }
}

pub mod purpose_plan {
    use super::*;
    pub static COMMAND: Command = Command {
        id: "style.purpose.plan",
        path: &["style", "purpose", "plan"],
        contract: 1,
        summary: "Plan a print purpose from exact styles and held printing pages.",
        purpose: "Capture the explicit project's governed Styles table and separate printing_standard Templates under the same native account, credential audience and lane. The kernel admits every required role and page, verifies the authored revision/content/paper pins, and returns actual printing bodies with existing resolver captures. Missing declarations or pages refuse. Geometry acquisition and rendering remain the report owner's next step; this plan seeds nothing, changes no defaults and queues no publication.",
        chapter: Chapter::MapPresentation,
        effect: Effect::LocalAuthState,
        authority: Authority::HeadlessProject,
        execution: Execution::Sync,
        args: &[
            PROJECT_ARG,
            Arg::value(
                "purpose",
                "<authored-id>",
                "Exact print purpose id returned by style purpose index.",
            )
            .required(),
            INK,
            LANE_ARG,
        ],
        output: "ds.style-purpose-plan/v1: exact purpose, Templates/table revisions, admitted page bodies and style captures; no geometry, artifacts or publication; bounded to 32 MiB.",
        examples: &[],
        refusals: &PURPOSE_ALL,
        reference: Some("docs/reference/style.md"),
        search: &["transformer sheet", "composition"],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    };

    pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
        let lane = inputs.require("lane")?;
        let project = inputs.require("project")?;
        let styles = ds_cli_auth::style_governance_receipt(lane, project, &Operation::Table)
            .map_err(named)?;
        let config = ds_cli_auth::feeder_configuration_for_project(lane, project)?;
        if styles.identity() != config.identity()
            || styles.project_id() != project
            || config.project_id() != project
            || styles.lane() != config.lane()
            || ds_cli_auth::headless_identity_for_named_project(lane)? != *styles.identity()
        {
            return Err(Failure::conflict(
                "project_context_changed",
                "native identity or explicit project changed during composition capture",
            ));
        }
        let receipt =
            ds_command_kernel::report_export::InputReceipt::from_config(&config.result().document)
                .map_err(|error| Failure::invalid("style_purpose_invalid", error))?;
        let sheets = receipt
            .sheets()
            .map_err(|error| Failure::invalid("style_purpose_invalid", error))?;
        let templates =
            serde_json::from_value(sheets["printing_standard"].clone()).map_err(|error| {
                Failure::invalid(
                    "print_template_invalid",
                    format!("separate Templates capture: {error}"),
                )
            })?;
        let snapshot = serde_json::from_value(styles.result().clone())
            .map_err(|error| Failure::invalid("style_resolution_invalid", error.to_string()))?;
        let mut result = ds_command_kernel::style_purpose::plan(
            project,
            &snapshot,
            inputs.require("purpose")?,
            if inputs.require("ink")? == "monochrome" {
                ds_command_kernel::style_resolution::Ink::Monochrome
            } else {
                ds_command_kernel::style_resolution::Ink::Colour
            },
            &templates,
        )
        .map_err(|error| Failure::invalid(error.code, error.message))?;
        result["lane"] = json!(styles.lane());
        result["report_input_source"] =
            json!({"schema":receipt.schema,"sheets_sha256":receipt.sheets_sha256});
        Ok(result)
    }
    pub fn render(data: &Value) -> String {
        format!(
            "{}\n",
            serde_json::to_string_pretty(data).unwrap_or_default()
        )
    }
}
command!(
    manifest,
    "style.catalogue.manifest",
    &["style", "catalogue", "manifest"],
    "Read the versioned declarative standard style manifest.",
    LocalAuthState,
    &[PROJECT_ARG, LANE_ARG],
    "Manifest revision, complete documents, bindings, presentation rules, presets and explicit obsolete declarations."
);
command!(
    seed_plan,
    "style.catalogue.seed.plan",
    &["style", "catalogue", "seed", "plan"],
    "Review standard style seeds and exact baseline migrations.",
    LocalAuthState,
    &[PROJECT_ARG, LANE_ARG],
    "manifest_revision, plan_sha256, creates, preserves, exact-head migrations, incompatibilities and apply_allowed."
);
command!(
    seed_apply,
    "style.catalogue.seed.apply",
    &["style", "catalogue", "seed", "apply"],
    "Apply reviewed seeds and versioned baseline migrations.",
    GlobalWrite,
    &[PROJECT_ARG, MANIFEST_REVISION, PLAN, LANE_ARG],
    "Fenced applied plan, created, preserved and migrated documents; immutable original and destination revisions."
);
command!(
    inventory,
    "style.catalogue.inventory",
    &["style", "catalogue", "inventory"],
    "Inventory a bounded page of catalogue and project style documents.",
    LocalAuthState,
    &[PROJECT_ARG, ALL_PROJECTS, CURSOR, LIMIT, LANE_ARG],
    "Exact paths, raw documents, revisions, resolver mappings, obsolete declarations, inventory digest, scope and continuation; selected project is not an all-project census."
);
command!(
    backup_create,
    "style.catalogue.backup.create",
    &["style", "catalogue", "backup", "create"],
    "Back up the reviewed complete style inventory through the API.",
    GlobalWrite,
    &[PROJECT_ARG, ALL_PROJECTS, INVENTORY, LANE_ARG],
    "Immutable backup_id, complete exact entries and inventory_sha256; no style is deleted."
);
command!(
    backup_read,
    "style.catalogue.backup.read",
    &["style", "catalogue", "backup", "read"],
    "Read one exact immutable style-inventory backup.",
    LocalAuthState,
    &[PROJECT_ARG, ALL_PROJECTS, BACKUP, LANE_ARG],
    "Exact backup entries, manifest revision and captured inventory digest."
);
command!(
    retirement_plan,
    "style.catalogue.retirement.plan",
    &["style", "catalogue", "retirement", "plan"],
    "Plan declared obsolete-style retirement against an exact backup.",
    LocalAuthState,
    &[PROJECT_ARG, ALL_PROJECTS, INVENTORY, BACKUP, LANE_ARG],
    "Deletion-disabled dry run, ids, reasons, replacements, current heads, dependencies and blocked candidates; main session needs owner approval for any later API deletion."
);
fn run(inputs: &Inputs, id: &str) -> Result<Value, Failure> {
    match id {
        "style.resolve" => {
            let value = invoke(inputs, Operation::Table)?;
            let snapshot = serde_json::from_value(value)
                .map_err(|e| Failure::invalid("style_resolution_invalid", format!("{e}")))?;
            let key = ds_command_kernel::style_resolution::Key {
                entity_class: inputs.require("entity-class")?.into(),
                source_kind: inputs.require("source-kind")?.into(),
                target: if inputs.require("target")? == "print" {
                    ds_command_kernel::style_resolution::Target::Print
                } else {
                    ds_command_kernel::style_resolution::Target::Screen
                },
                role: inputs.require("role")?.into(),
                ink: if inputs.require("ink")? == "monochrome" {
                    ds_command_kernel::style_resolution::Ink::Monochrome
                } else {
                    ds_command_kernel::style_resolution::Ink::Colour
                },
            };
            ds_command_kernel::style_resolution::resolve(&snapshot, &key)
                .map(|binding| json!(binding))
                .map_err(|e| Failure::invalid(e.code, e.message))
        }
        "style.resolution.table" => invoke(inputs, Operation::Table),
        "style.catalogue.manifest" => invoke(inputs, Operation::Manifest),
        "style.catalogue.seed.plan" => invoke(inputs, Operation::PlanSeed),
        "style.catalogue.seed.apply" => invoke(
            inputs,
            Operation::Seed {
                expected_manifest_revision: inputs.require("expected-manifest")?.into(),
                expected_plan_sha256: inputs.require("expected-plan")?.into(),
            },
        ),
        "style.catalogue.inventory" => invoke(
            inputs,
            Operation::Inventory {
                all_projects: inputs.switch("all-projects"),
                cursor: inputs.value("cursor").map(Into::into),
                limit: crate::integer(inputs.require("limit")?, "limit", 1, 200)? as u16,
            },
        ),
        "style.catalogue.backup.create" => invoke(
            inputs,
            Operation::Backup {
                expected_inventory_sha256: inputs.require("expected-inventory")?.into(),
                all_projects: inputs.switch("all-projects"),
            },
        ),
        "style.catalogue.backup.read" => invoke(
            inputs,
            Operation::ReadBackup {
                backup_id: inputs.require("backup")?.into(),
                all_projects: inputs.switch("all-projects"),
            },
        ),
        _ => invoke(
            inputs,
            Operation::PlanRetirement {
                expected_inventory_sha256: inputs.require("expected-inventory")?.into(),
                backup_id: inputs.require("backup")?.into(),
                all_projects: inputs.switch("all-projects"),
            },
        ),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn seed_effect_is_fenced_and_retirement_is_read_only() {
        assert_eq!(seed_apply::COMMAND.effect, Effect::GlobalWrite);
        assert!(
            seed_apply::COMMAND
                .args
                .iter()
                .any(|a| a.name == "expected-plan" && a.required)
        );
        assert_eq!(retirement_plan::COMMAND.effect, Effect::LocalAuthState);
        assert!(
            retirement_plan::COMMAND
                .args
                .iter()
                .any(|a| a.name == "backup" && a.required)
        );
        assert_eq!(
            resolve::COMMAND.args.iter().filter(|a| a.required).count(),
            5
        );
    }
}
