//! `ds design config copy` — a project's own network documents, copied from
//! another project.
//!
//! ```text
//!   plan → apply        census (platform administrators)
//! ```
//!
//! A project owns `docs/network_template` (the network template it reads) and
//! `docs/network_config` (its Settings sheets). A new or empty project takes
//! them from a template project, whole or one at a time; ds-brain copies them
//! under the reviewed plan's digest and never changes the source. The global
//! template store is a migration source only (`--source-template`), for
//! platform administrators while it is retired
//! (ds-brain `docs/contracts/project-network-documents.md`).
//!
//! Like `ds design migrate`, both projects are named on every call and
//! neither is the saved selection.

use ds_cli_auth::{
    NETWORK_DOCUMENTS_INVALID_REFUSAL, NETWORK_DOCUMENTS_PLAN_CHANGED_REFUSAL,
    NETWORK_DOCUMENTS_PROJECT_NOT_FOUND_REFUSAL, NETWORK_DOCUMENTS_ROUTE_UNAVAILABLE_REFUSAL,
    NETWORK_DOCUMENTS_SOURCE_MISSING_REFUSAL, NetworkDocumentsRequest, NetworkDocumentsSource,
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
);
const SOURCE_TEMPLATE: Arg = Arg::value(
    "source-template",
    "<id|selected>",
    "Migration only: the global network template, or `selected` for the one the project names.",
);
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
        remedy: "a project copy needs membership of the source and project.edit on the destination; a global source or census needs platform.admin",
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

const PURPOSE: &str = "Give a project the network template and Settings it computes with by copying them from a template project, whole or one document at a time. Plan states each document's outcome (create, replace or identical) and a plan_sha256; apply writes exactly that plan and never changes the source. --source-template reads the retiring global store and is for platform administrators only.";

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
    args: &[PROJECT, SOURCE_PROJECT, SOURCE_TEMPLATE, PART, LANE],
    output: "Per document: source and destination sha256 and the outcome create|replace|identical; the resolved source (template, master fallback) and the plan_sha256 apply checks.",
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
    args: &[
        PROJECT,
        SOURCE_PROJECT,
        SOURCE_TEMPLATE,
        PART,
        EXPECTED_PLAN,
        LANE,
    ],
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

pub static CENSUS: Command = Command {
    id: "design.config.copy.census",
    path: &["design", "config", "copy", "census"],
    contract: 1,
    summary: "List which projects already own the network template they read.",
    purpose: "Platform administrators page every project: the template it names and resolves, whether its own docs/network_template holds exactly that template, and the plan_sha256 of the migration that would make it so. Writes nothing.",
    chapter: Chapter::Design,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "cursor",
            "<project-id>",
            "next_cursor of the previous page.",
        ),
        Arg::value("limit", "<n>", "Projects per page, 1-25 (default 25)."),
        LANE,
    ],
    output: "One row per project: selected and resolved template, master_fallback, global, template and config digests, in_sync and the migration plan_sha256; next_cursor while more remain.",
    examples: &[Example {
        command: "ds design config copy census --output json",
        note: "Continue with --cursor <next_cursor>; apply a row with copy apply --source-template selected.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/design.md"),
    search: &["template migration", "migration status"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn invalid(message: &str) -> Failure {
    Failure::invalid("network_documents_invalid", message.to_owned())
        .remedy(NETWORK_DOCUMENTS_INVALID_REFUSAL.remedy)
}

fn source(i: &Inputs) -> Result<NetworkDocumentsSource, Failure> {
    match (i.value("source-project"), i.value("source-template")) {
        (Some(project), None) => Ok(NetworkDocumentsSource::Project {
            project: project.to_owned(),
        }),
        (None, Some("selected")) => Ok(NetworkDocumentsSource::Global { template: None }),
        (None, Some(template)) => Ok(NetworkDocumentsSource::Global {
            template: Some(template.to_owned()),
        }),
        _ => Err(invalid(
            "name exactly one source: --source-project or --source-template",
        )),
    }
}

fn parts(i: &Inputs) -> Vec<String> {
    i.repeated("part").to_vec()
}

pub fn plan(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let request = NetworkDocumentsRequest::Plan {
        source: source(i)?,
        parts: parts(i),
    };
    ds_cli_auth::network_documents(i.require("lane")?, Some(i.require("project")?), &request)
}

pub fn apply(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let request = NetworkDocumentsRequest::Apply {
        source: source(i)?,
        parts: parts(i),
        expected_plan_sha256: i.require("expected-plan")?.to_owned(),
    };
    ds_cli_auth::network_documents(i.require("lane")?, Some(i.require("project")?), &request)
}

pub fn census(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = match i.value("limit") {
        Some(text) => Some(
            text.parse::<u32>()
                .ok()
                .filter(|n| (1..=25).contains(n))
                .ok_or_else(|| invalid("--limit is 1-25"))?,
        ),
        None => None,
    };
    let request = NetworkDocumentsRequest::Census {
        cursor: i.value("cursor").map(str::to_owned),
        limit,
    };
    ds_cli_auth::network_documents(i.require("lane")?, None, &request)
}

/// A plan reads one line per document; a receipt the documents written; a
/// census one line per project.
pub fn render(data: &Value) -> String {
    let text = |value: &Value| value.as_str().unwrap_or("-").to_owned();
    let mut out = String::new();
    if let Some(projects) = data["projects"].as_array() {
        for row in projects {
            out.push_str(&format!(
                "{} {} {}{}\n",
                text(&row["project_id"]),
                text(&row["resolved_template"]),
                if row["in_sync"] == true {
                    "in_sync"
                } else {
                    "pending"
                },
                row["refusal"]
                    .as_str()
                    .map(|code| format!(" refused {code}"))
                    .unwrap_or_default()
            ));
        }
        if let Some(next) = data["next_cursor"].as_str() {
            out.push_str(&format!("next_cursor {next}\n"));
        }
        return out;
    }
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
        let source = &data["source"];
        out.push_str(&format!(
            "into {} from {} {}{}\n",
            text(&data["project_id"]),
            text(&source["kind"]),
            source["project_id"]
                .as_str()
                .or_else(|| source["template"].as_str())
                .unwrap_or("-"),
            if source["master_fallback"] == true {
                " (master fallback)"
            } else {
                ""
            }
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
    fn exactly_one_source_is_named_before_anything_is_sent() {
        assert_eq!(
            source(&parse(&PLAN, &["--project", "p", "--source-project", "t"])).unwrap(),
            NetworkDocumentsSource::Project {
                project: "t".into()
            }
        );
        assert_eq!(
            source(&parse(
                &PLAN,
                &["--project", "p", "--source-template", "selected"]
            ))
            .unwrap(),
            NetworkDocumentsSource::Global { template: None }
        );
        assert_eq!(
            source(&parse(
                &PLAN,
                &["--project", "p", "--source-template", "tchad"]
            ))
            .unwrap(),
            NetworkDocumentsSource::Global {
                template: Some("tchad".into())
            }
        );
        for args in [
            &["--project", "p"][..],
            &[
                "--project",
                "p",
                "--source-project",
                "t",
                "--source-template",
                "master",
            ][..],
        ] {
            assert_eq!(
                source(&parse(&PLAN, args)).unwrap_err().code(),
                "network_documents_invalid"
            );
        }
        assert_eq!(
            parts(&parse(
                &PLAN,
                &[
                    "--project",
                    "p",
                    "--source-project",
                    "t",
                    "--part",
                    "network_config"
                ]
            )),
            ["network_config"]
        );
    }

    #[test]
    fn apply_is_a_write_fenced_on_the_reviewed_plan() {
        assert_eq!(APPLY.effect, Effect::GlobalWrite);
        assert_eq!(PLAN.effect, Effect::LocalAuthState);
        assert_eq!(CENSUS.authority, Authority::HeadlessUser);
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
        let plan = render(
            &json!({"project_id":"p","source":{"kind":"project","project_id":"t"},
            "parts":[{"part":"network_template","outcome":"create"}],"plan_sha256":"a"}),
        );
        assert!(plan.contains("network_template create") && plan.contains("plan_sha256 a"));
        let receipt = render(&json!({"project_id":"p","written":[]}));
        assert!(receipt.contains("already identical"));
        let census = render(
            &json!({"projects":[{"project_id":"p","resolved_template":"master","in_sync":false}],"next_cursor":"p"}),
        );
        assert!(census.contains("p master pending") && census.contains("next_cursor p"));
    }
}
