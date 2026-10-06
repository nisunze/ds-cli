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
use serde_json::Value;

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
];

const PURPOSE: &str = "Give a project the network template and Settings it computes with by copying them from a template project, whole or one document at a time. Plan states each document's outcome (create, replace or identical) and a plan_sha256; apply writes exactly that plan and never changes the source.";

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
    args: &[PROJECT, SOURCE_PROJECT, PART, LANE],
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
    args: &[PROJECT, SOURCE_PROJECT, PART, EXPECTED_PLAN, LANE],
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

pub fn plan(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let request = NetworkDocumentsRequest::Plan {
        source_project: i.require("source-project")?.to_owned(),
        parts: parts(i),
    };
    ds_cli_auth::network_documents(i.require("lane")?, i.require("project")?, &request)
}

pub fn apply(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let request = NetworkDocumentsRequest::Apply {
        source_project: i.require("source-project")?.to_owned(),
        parts: parts(i),
        expected_plan_sha256: i.require("expected-plan")?.to_owned(),
    };
    ds_cli_auth::network_documents(i.require("lane")?, i.require("project")?, &request)
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
}
