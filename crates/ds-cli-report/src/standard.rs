//! `ds report standard` — the governed global LV standard pages and A4 report
//! documents a project binds by reference. ds-brain owns them and seeding owns
//! every write; the shared printing library (`report layout list --scope
//! global`) does not list them. This domain only reads them.
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
const PURPOSE: &str = "Read the governed A0/A3 LV standard pages and the A4 voltage-drop document projects bind by reference; report layout list does not show them. Seeding owns every write.";

pub static LIST: Command = Command {
    id: "report.standard.list",
    path: &["report", "standard", "list"],
    contract: 1,
    summary: "List the governed standard and A4 print documents.",
    purpose: PURPOSE,
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[LANE],
    output: "Catalogue schema, one row per document (kind, id, paper, role, revision, bound) and the bound defaults.",
    examples: &[Example {
        command: "ds report standard list --output json",
        note: "Needs a restored session.",
        runnable: false,
    }],
    refusals: &[
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
    contract: 1,
    summary: "Read one governed standard or A4 print document.",
    purpose: PURPOSE,
    chapter: Chapter::Reports,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessUser,
    execution: Execution::Sync,
    args: &[
        LANE,
        Arg::value("kind", "<kind>", "standard page or a4 document.")
            .choices(&["standard", "a4"])
            .required(),
        Arg::value("id", "<id>", "Exact id from report.standard.list.").required(),
    ],
    output: "standard: template, role, revision and print document. a4: definition and its HTML when the deployment holds it.",
    examples: &[Example {
        command: "ds report standard get --kind a4 --id voltage-drop-a4-v1 --output json",
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
    ds_cli_auth::printing_standard(i.require("lane")?, &PrintingStandardRequest::List {})
}

pub fn get(i: &Inputs, _c: &Context) -> Result<Value, Failure> {
    let kind = match i.require("kind")? {
        "a4" => PrintingStandardKind::A4,
        _ => PrintingStandardKind::Standard,
    };
    ds_cli_auth::printing_standard(
        i.require("lane")?,
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
