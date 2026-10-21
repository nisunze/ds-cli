//! `ds design config copy` — a project's own network documents, copied from
//! another project.
//!
//! ```text
//!   plan → apply
//! ```
//!
//! A project owns `docs/network_template` (the network template it reads) and
//! `docs/network_config` (its Settings sheets). A new or empty project takes
//! them from a template project, whole or one at a time; ds-brain copies them
//! under the reviewed plan's digest and never changes the source. There is no
//! global template store (ds-brain `docs/contracts/project-network-documents.md`).
//!
//! `--sheet` is the narrower copy of ONE Settings sheet, created when the
//! destination lacks it: the kernel (`design_config::plan_sheet_copy`) admits
//! the sheet from the source's own configuration groups, validates it exactly
//! as an edit, and the native client writes and reads back only that sheet.
//!
//! Like `ds design migrate`, both projects are named on every call and
//! neither is the saved selection.

use ds_cli_auth::{
    NETWORK_DOCUMENTS_INVALID_REFUSAL, NETWORK_DOCUMENTS_PLAN_CHANGED_REFUSAL,
    NETWORK_DOCUMENTS_PROJECT_NOT_FOUND_REFUSAL, NETWORK_DOCUMENTS_ROUTE_UNAVAILABLE_REFUSAL,
    NETWORK_DOCUMENTS_SOURCE_MISSING_REFUSAL, NetworkDocumentsRequest,
};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::ProjectConfigurationChange as Change;
use ds_command_kernel::design_config;
use serde_json::{Value, json};

const LANE: Arg = ds_cli_contract::spec::LANE.summary("Deployment lane.");
const PROJECT: Arg = Arg::value(
    "project",
    "<id>",
    "Explicit DESTINATION project whose network documents are written.",
)
.required();
const SOURCE_PROJECT: Arg = Arg::value(
    "source-project",
    "<id>",
    "Copy FROM this project, usually a template project.",
)
.required();
const PART: Arg = Arg::repeated(
    "part",
    "<network_template|network_config>",
    "One document to copy; repeat for both. Default: every document the source holds.",
);
const SHEET: Arg = Arg::value(
    "sheet",
    "<key>",
    "Copy only this Settings sheet (exact key from the source's design config sheets), created when the destination lacks it; every other destination sheet is kept. Not with --part.",
);
const EXPECTED_PLAN: Arg = Arg::value(
    "expected-plan",
    "<64hex>",
    "Exact plan_sha256 of the reviewed plan with the same projects and parts.",
)
.required();

const REFUSALS: &[Refusal] = &[
    NETWORK_DOCUMENTS_INVALID_REFUSAL,
    NETWORK_DOCUMENTS_PLAN_CHANGED_REFUSAL,
    NETWORK_DOCUMENTS_SOURCE_MISSING_REFUSAL,
    NETWORK_DOCUMENTS_PROJECT_NOT_FOUND_REFUSAL,
    NETWORK_DOCUMENTS_ROUTE_UNAVAILABLE_REFUSAL,
    ds_cli_report::project::NATIVE_PROFILE,
    ds_cli_report::project::NATIVE_PROFILE_DIGEST,
    ds_cli_report::project::NATIVE_PROFILE_UNSAFE,
    ds_cli_report::project::HEADLESS_SIGNED_OUT,
    ds_cli_report::project::HEADLESS_NO_PROJECT,
    ds_cli_report::project::PROJECT_CONTEXT_STALE,
    ds_cli_report::project::NATIVE_STATE_UNSAFE,
    ds_cli_report::project::NATIVE_STATE_UNAVAILABLE,
    ds_cli_report::project::NATIVE_STATE_PROTECTION,
    ds_cli_report::project::NATIVE_STATE_ROOT,
    ds_cli_report::project::NATIVE_STATE_CONFLICT,
    ds_cli_report::project::NATIVE_CLEANUP,
    ds_cli_report::project::AUTH_CONTEXT_MISMATCH,
    ds_cli_report::project::AUTH_REVOKED,
    ds_cli_report::project::AUTH_IDENTITY_MISMATCH,
    Refusal {
        code: "auth_rejected",
        when: "the caller may not read the source or edit the destination configuration",
        remedy: "a copy needs membership of the source and project.edit on the destination",
    },
    Refusal {
        code: "auth_transient",
        when: "ds-brain is unavailable",
        remedy: "plan again before retrying an uncertain apply",
    },
    Refusal {
        code: "auth_response_unreadable",
        when: "the plan or receipt does not answer the request",
        remedy: "plan again and report the mismatched receipt",
    },
    Refusal {
        code: "confirmation_required",
        when: "an apply lacks --yes",
        remedy: "review the plan and pass --yes",
    },
    Refusal {
        code: "config_sheet_unknown",
        when: "--sheet names a sheet the source project's Settings do not hold",
        remedy: "list the source's sheets with design config sheets --project <source-project> and name an exact key",
    },
    Refusal {
        code: "config_sheet_not_admitted",
        when: "the source holds --sheet but its configuration groups do not declare it a Settings sheet",
        remedy: "copy a declared sheet, or copy the whole network_config document with --part",
    },
    Refusal {
        code: "config_copy_invalid",
        when: "--sheet is combined with --part, names the destination as source, or its value fails the Settings validation an edit of that sheet passes",
        remedy: "name one sheet without --part from another project, and correct the source sheet with design config save",
    },
    Refusal {
        code: "config_copy_plan_changed",
        when: "either project's sheet moved after the reviewed --sheet plan",
        remedy: "plan again, review it, and apply the new plan_sha256",
    },
];

const PURPOSE: &str = "Give a project the network template and Settings it computes with by copying them from a template project, whole, one document, or one Settings sheet at a time (--sheet, e.g. a layer_mapping the destination lacks). Plan states each outcome (create, replace or identical) and a plan_sha256; apply writes exactly that plan, reads it back, and never changes the source.";

pub static PLAN: Command = Command {
    id: "design.config.copy.plan",
    path: &["design", "config", "copy", "plan"],
    contract: 1,
    summary: "Plan copying a template project's network documents into a project.",
    purpose: PURPOSE,
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT, SOURCE_PROJECT, PART, SHEET, LANE],
    output: "Per document: source and destination sha256 and the outcome create|replace|identical, and the plan_sha256 apply checks.",
    examples: &[Example {
        command: "ds design config copy plan --project <new-project> --source-project <template-project> --output json",
        note: "Both documents; read .data.parts[].outcome and .data.plan_sha256.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &[
        "template project",
        "from template",
        "apply template",
        "network template",
        "copy settings",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static APPLY: Command = Command {
    id: "design.config.copy.apply",
    path: &["design", "config", "copy", "apply"],
    contract: 1,
    summary: "Apply a reviewed network template and Settings copy.",
    purpose: PURPOSE,
    chapter: Chapter::Design,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT, SOURCE_PROJECT, PART, SHEET, EXPECTED_PLAN, LANE],
    output: "Receipt: the documents written (identical ones are not), the plan_sha256, and whether the copy moved the project's design data locality.",
    examples: &[Example {
        command: "ds design config copy apply --project <new-project> --source-project <template-project> --expected-plan <plan_sha256> --yes --output json",
        note: "Writes only what the reviewed plan names; a changed source refuses.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &["apply template", "template project"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn parts(i: &Inputs) -> Vec<String> {
    i.repeated("part").to_vec()
}

const SHEET_RECEIPT_SCHEMA: &str = "ds.design-config-sheet-copy-receipt/v1";

fn sheet_refusal(code: &str) -> Failure {
    match code {
        "config_sheet_unknown" => Failure::invalid(
            "config_sheet_unknown",
            "the source project's Settings hold no sheet with that key",
        )
        .next("ds design config sheets --project <source-project>"),
        "config_sheet_not_admitted" => Failure::invalid(
            "config_sheet_not_admitted",
            "the source holds that sheet but its configuration groups do not declare it",
        ),
        "config_copy_plan_changed" => Failure::conflict(
            "config_copy_plan_changed",
            "a project's sheet moved after the reviewed plan; nothing was written",
        ),
        other => Failure::invalid(
            "config_copy_invalid",
            format!("the sheet cannot be copied: {other}"),
        ),
    }
}

/// `--sheet`: both projects' Settings are read under the caller's own
/// authority (the source READ), and the kernel decides the plan. ds-brain's
/// `save_config` remains the destination WRITE authority.
fn sheet_plan(i: &Inputs, sheet: &str) -> Result<(Value, Value), Failure> {
    if !parts(i).is_empty() {
        return Err(sheet_refusal(
            "--sheet copies one Settings sheet; do not also name --part",
        ));
    }
    let lane = i.require("lane")?;
    let project = i.require("project")?;
    let source_project = i.require("source-project")?;
    if project == source_project {
        return Err(sheet_refusal("the source must be another project"));
    }
    let source =
        ds_cli_auth::settings_configuration(lane, source_project, Change::ReadSettings)?.document;
    let destination =
        ds_cli_auth::settings_configuration(lane, project, Change::ReadSettings)?.document;
    let copy =
        design_config::plan_sheet_copy(project, source_project, &source, &destination, sheet)
            .map_err(sheet_refusal)?;
    let mut plan = copy.plan;
    plan["project_id"] = json!(project);
    Ok((plan, source))
}

pub fn plan(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    if let Some(sheet) = i.value("sheet") {
        return sheet_plan(i, sheet).map(|(plan, _)| plan);
    }
    let request = NetworkDocumentsRequest::Plan {
        source_project: i.require("source-project")?.to_owned(),
        parts: parts(i),
    };
    ds_cli_auth::network_documents(i.require("lane")?, i.require("project")?, &request)
}

pub fn apply(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    if let Some(sheet) = i.value("sheet") {
        return apply_sheet(i, sheet);
    }
    let request = NetworkDocumentsRequest::Apply {
        source_project: i.require("source-project")?.to_owned(),
        parts: parts(i),
        expected_plan_sha256: i.require("expected-plan")?.to_owned(),
    };
    ds_cli_auth::network_documents(i.require("lane")?, i.require("project")?, &request)
}

/// The kernel re-decides the plan against the fresh destination read inside
/// the native client's write, which then reads the saved sheet back.
fn apply_sheet(i: &Inputs, sheet: &str) -> Result<Value, Failure> {
    let expected = i.require("expected-plan")?;
    let (plan, source) = sheet_plan(i, sheet)?;
    if plan["plan_sha256"] != expected {
        return Err(sheet_refusal("config_copy_plan_changed"));
    }
    let project = i.require("project")?;
    let source_project = i.require("source-project")?;
    let change = Change::CopySheet {
        sheet: sheet.to_owned(),
        source_project: source_project.to_owned(),
        source,
        expected_plan_sha256: expected.to_owned(),
    };
    let saved = ds_cli_auth::settings_configuration(i.require("lane")?, project, change).map_err(
        |failure| {
            if failure.message().contains("config_copy_plan_changed") {
                sheet_refusal("config_copy_plan_changed")
            } else {
                failure
            }
        },
    )?;
    let written = saved.summary["saved"] == true;
    Ok(json!({
        "schema": SHEET_RECEIPT_SCHEMA,
        "project_id": project,
        "source_project": source_project,
        "sheet": sheet,
        "outcome": plan["outcome"],
        "plan_sha256": expected,
        "written": if written { vec![sheet] } else { Vec::new() },
        "verified_readback": written,
    }))
}

/// A plan reads one line per document; a receipt the documents written.
pub fn render(data: &Value) -> String {
    let text = |value: &Value| value.as_str().unwrap_or("-").to_owned();
    let mut out = String::new();
    if let Some(written) = data["written"].as_array() {
        out.push_str(&format!(
            "{} written into {}\n",
            written
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", "),
            text(&data["project_id"])
        ));
        if written.is_empty() {
            out.push_str("nothing written: every document was already identical\n");
        }
        return out;
    }
    if data["sheet"].is_string() {
        out.push_str(&format!(
            "into {} from {}\n  sheet {} {} ({} other sheets kept)\nplan_sha256 {}\n",
            text(&data["project_id"]),
            text(&data["source_project"]),
            text(&data["sheet"]),
            text(&data["outcome"]),
            data["preserved_sheets"],
            text(&data["plan_sha256"])
        ));
        return out;
    }
    if let Some(parts) = data["parts"].as_array() {
        out.push_str(&format!(
            "into {} from {}\n",
            text(&data["project_id"]),
            text(&data["source_project"])
        ));
        for part in parts {
            out.push_str(&format!(
                "  {} {}\n",
                text(&part["part"]),
                text(&part["outcome"])
            ));
        }
        out.push_str(&format!("plan_sha256 {}\n", text(&data["plan_sha256"])));
        return out;
    }
    format!("{data}\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn parse(command: &Command, args: &[&str]) -> Inputs {
        let owned = args.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        ds_cli_contract::parse(command, &owned).unwrap()
    }

    #[test]
    fn a_copy_names_both_projects_and_its_parts() {
        let inputs = parse(
            &PLAN,
            &[
                "--project",
                "p",
                "--source-project",
                "t",
                "--part",
                "network_config",
            ],
        );
        assert_eq!(inputs.value("source-project"), Some("t"));
        assert_eq!(parts(&inputs), ["network_config"]);
        assert!(
            PLAN.args
                .iter()
                .any(|a| a.name == "source-project" && a.required)
        );
        assert!(!PLAN.args.iter().any(|a| a.name == "source-template"));
    }

    #[test]
    fn apply_is_a_write_fenced_on_the_reviewed_plan() {
        assert_eq!(APPLY.effect, Effect::GlobalWrite);
        assert_eq!(PLAN.effect, Effect::LocalAuthState);
        assert!(
            APPLY
                .args
                .iter()
                .any(|a| a.name == "expected-plan" && a.required)
        );
        assert!(!PLAN.args.iter().any(|a| a.name == "expected-plan"));
    }

    #[test]
    fn renderings_name_what_happened() {
        let plan = render(&json!({"project_id":"p","source_project":"t",
            "parts":[{"part":"network_template","outcome":"create"}],"plan_sha256":"a"}));
        assert!(
            plan.contains("into p from t")
                && plan.contains("network_template create")
                && plan.contains("plan_sha256 a")
        );
        let receipt = render(&json!({"project_id":"p","written":[]}));
        assert!(receipt.contains("already identical"));
    }

    #[test]
    fn a_sheet_copy_is_one_named_sheet_and_never_a_document_part() {
        let inputs = parse(
            &PLAN,
            &[
                "--project",
                "p",
                "--source-project",
                "t",
                "--sheet",
                "layer_mapping",
                "--part",
                "network_config",
            ],
        );
        let refused = sheet_plan(&inputs, "layer_mapping").unwrap_err();
        assert_eq!(refused.code(), "config_copy_invalid");
        let same = parse(
            &PLAN,
            &["--project", "p", "--source-project", "p", "--sheet", "x"],
        );
        assert_eq!(
            sheet_plan(&same, "x").unwrap_err().code(),
            "config_copy_invalid"
        );
        assert!(APPLY.args.iter().any(|a| a.name == "sheet" && !a.required));
        for code in [
            "config_sheet_unknown",
            "config_sheet_not_admitted",
            "config_copy_plan_changed",
        ] {
            assert_eq!(sheet_refusal(code).code(), code);
            assert!(REFUSALS.iter().any(|r| r.code == code));
        }
        assert_eq!(
            sheet_refusal("config_sheet_shape_invalid").code(),
            "config_copy_invalid"
        );
    }

    #[test]
    fn a_sheet_plan_renders_its_one_sheet_and_kept_sheets() {
        let plan = render(
            &json!({"project_id":"p","source_project":"t","sheet":"layer_mapping",
            "outcome":"create","preserved_sheets":21,"plan_sha256":"a"}),
        );
        assert!(
            plan.contains("sheet layer_mapping create (21 other sheets kept)")
                && plan.contains("plan_sha256 a"),
            "{plan}"
        );
        let receipt =
            render(&json!({"project_id":"p","sheet":"layer_mapping","written":["layer_mapping"]}));
        assert!(receipt.contains("layer_mapping written into p"));
    }
}
