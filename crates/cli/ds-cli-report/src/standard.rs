//! Read governed documents held by one explicit project, outside its custom
//! layout library. Template projects use the same ordinary project contract.
use ds_cli_auth::{
    PRINT_STANDARD_INVALID_REFUSAL, PRINT_STANDARD_NOT_FOUND_REFUSAL,
    PRINT_STANDARD_REQUEST_INVALID_REFUSAL, PRINT_STANDARD_ROUTE_UNAVAILABLE_REFUSAL,
    PrintingStandardKind, PrintingStandardRequest,
};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

const LANE: Arg = ds_cli_contract::spec::LANE
    .placeholder("<lane>")
    .choices(&["canary", "stable"])
    .default("canary");
const PROJECT: Arg = Arg::value(
    "project",
    "<exact-id>",
    "Exact project holding the governed documents; the saved selection is never read.",
)
.required();
const PURPOSE: &str = "Read only the named project's owned standard pages and A4 voltage-drop document; report layout list shows its custom layouts separately. A template is an ordinary project. Missing owned documents stay absent; no global fallback or seeding occurs.";

pub static LIST: Command = Command {
    id: "report.standard.list",
    path: &["report", "standard", "list"],
    contract: 2,
    summary: "List the governed standard and A4 print documents.",
    purpose: PURPOSE,
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[PROJECT, LANE],
    output: "Captured project identity and bounded document rows (kind, id, paper, role, revision/content pins, bound); no implicit defaults.",
    examples: &[Example {
        command: "ds report standard list --project <exact-id> --output json",
        note: "Needs a restored session.",
        runnable: false,
    }],
    refusals: &[
        PRINT_STANDARD_REQUEST_INVALID_REFUSAL,
        PRINT_STANDARD_INVALID_REFUSAL,
        PRINT_STANDARD_ROUTE_UNAVAILABLE_REFUSAL,
    ],
    reference: Some("docs/reference/report.md"),
    search: &["front matter", "voltage drop", "a0"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

const GET_REFUSALS: &[Refusal] = &[
    PRINT_STANDARD_REQUEST_INVALID_REFUSAL,
    PRINT_STANDARD_NOT_FOUND_REFUSAL,
    PRINT_STANDARD_INVALID_REFUSAL,
    PRINT_STANDARD_ROUTE_UNAVAILABLE_REFUSAL,
];

pub static GET: Command = Command {
    id: "report.standard.get",
    path: &["report", "standard", "get"],
    contract: 2,
    summary: "Read one governed standard or A4 print document.",
    purpose: PURPOSE,
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("kind", "<kind>", "standard page or a4 document.")
            .choices(&["standard", "a4"])
            .required(),
        Arg::value("id", "<id>", "Exact id from report.standard.list.").required(),
    ],
    output: "Captured project identity. standard: template, role, revision and document. a4: exact definition, governed body and revision/content/HTML pins.",
    examples: &[Example {
        command: "ds report standard get --project <exact-id> --kind a4 --id voltage-drop-a4-v1 --output json",
        note: "Needs a restored session.",
        runnable: false,
    }],
    refusals: GET_REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &["front matter", "voltage drop"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn list(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    ds_cli_auth::printing_standard(
        i.require("lane")?,
        i.require("project")?,
        &PrintingStandardRequest::List {},
    )
}

pub fn get(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let kind = match i.require("kind")? {
        "a4" => PrintingStandardKind::A4,
        _ => PrintingStandardKind::Standard,
    };
    ds_cli_auth::printing_standard(
        i.require("lane")?,
        i.require("project")?,
        &PrintingStandardRequest::Get {
            kind,
            id: i.require("id")?.into(),
        },
    )
}

/// A catalogue reads as one line per document; one document stays JSON.
pub fn render(data: &Value) -> String {
    let Some(rows) = data["documents"].as_array() else {
        return format!("{data}\n");
    };
    let mut text = String::new();
    for row in rows {
        let field = |name: &str| row[name].as_str().unwrap_or("-").to_owned();
        text.push_str(&format!(
            "{} {} {} {} {}{}\n",
            field("kind"),
            field("id"),
            field("paper"),
            field("orientation"),
            row["revision_id"]
                .as_str()
                .or(row["content_sha256"].as_str())
                .unwrap_or("-"),
            if row["bound"] == true { " bound" } else { "" },
        ));
    }
    text.push_str(&format!("{} documents\n", rows.len()));
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn owned_documents_capture_required_project_authority() {
        for command in [&LIST, &GET] {
            assert_eq!(command.authority, Authority::HeadlessProject);
            assert!(
                command
                    .args
                    .iter()
                    .any(|arg| arg.name == "project" && arg.required)
            );
            assert!(!command.args.iter().any(|arg| arg.name == "scope"));
        }
    }
}
