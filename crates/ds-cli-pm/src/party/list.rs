//! `ds pm party list` — who the project corresponds with.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::{Action, PartyFilters};
use serde_json::Value;

use crate::LANE_ARG;

const QUERY_ARG: Arg = Arg::value(
    "query",
    "<text>",
    "Match a party's name, emails or notes, case-insensitively.",
);
const ROLE_ARG: Arg = Arg::value(
    "role",
    "<role>",
    "Only this role — client, consultant, contractor, supplier, authority, other (`ds pm plan` publishes the list).",
);
const KIND_ARG: Arg = Arg::value("kind", "<organisation|person>", "Only this kind of party.");
const ARCHIVED_ARG: Arg = Arg::switch(
    "include-archived",
    "Also list archived parties; they are hidden by default.",
);
const LIMIT_ARG: Arg = Arg::value(
    "limit",
    "<count>",
    "Rows in one page (1-100). The total is always reported.",
)
.default("50");
const PAGE_ARG: Arg =
    Arg::value("page", "<index>", "Zero-based page of the bounded result.").default("0");

pub static COMMAND: Command = Command {
    id: "pm.party.list",
    path: &["pm", "party", "list"],
    contract: 1,
    summary: "List the outside organisations and people the project writes to.",
    purpose: "\
The counterparties a record can name — the client, the consultant reviewing \
the design, the contractor, the supplier, the authority — with the ids \
`ds pm record create --party` and `--response-owed-by` take. Parties are \
per project. One bounded page in name order; archived parties are hidden \
unless asked for. Headless: the selected project of the signed-in native \
credential, no window.",
    chapter: Chapter::Project,
    effect: Effect::ReadOnly,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        QUERY_ARG,
        ROLE_ARG,
        KIND_ARG,
        ARCHIVED_ARG,
        LIMIT_ARG,
        PAGE_ARG,
        LANE_ARG,
    ],
    output: "\
The project, the matched `total`, the page bounds, `truncated` when the \
server stopped at its scan cap, and `parties` rows of `id`, `kind`, `name`, \
`role`, `organisationPartyId`, `emails`, `phones`, `notes`, `archived` and \
`version`.",
    examples: &[Example {
        command: "ds pm party list --role consultant --output json",
        note: "Read .data.parties[].id to name a party on a record.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<22>(&[crate::INVALID_NUMBER]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "correspondence",
        "counterparty",
        "client",
        "consultant",
        "contractor",
        "supplier",
        "authority",
        "contact",
        "organisation",
        "company",
        "who owes",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let limit = match inputs.value("limit") {
        Some(limit) => crate::integer(limit, "limit", 1, 100)?,
        None => 50,
    };
    let page = match inputs.value("page") {
        Some(page) => crate::integer(page, "page", 0, 10_000)?,
        None => 0,
    };
    let report = crate::correspondence(
        inputs.value("lane").unwrap_or("stable"),
        &Action::PartyList(PartyFilters {
            query: inputs.value("query").map(str::to_owned),
            role: inputs.value("role").map(str::to_owned),
            kind: inputs.value("kind").map(str::to_owned),
            include_archived: inputs.switch("include-archived"),
            limit: Some(limit),
            page: Some(page + 1),
        }),
    )?;
    let project = report.project_id().to_owned();
    crate::data(
        &ds_command_kernel::project_management::correspondence::party_page(
            &project,
            &report.into_result(),
        ),
    )
}

pub fn render(data: &Value) -> String {
    let total = data["total"].as_u64().unwrap_or(0);
    let mut out = format!(
        "{} in {}\n",
        crate::plural(total, "party"),
        data["project"].as_str().unwrap_or("?"),
    );
    if let Some(rows) = data["parties"].as_array() {
        for row in rows {
            out.push_str(&super::party_line(row));
        }
        let through = data["to"].as_u64().unwrap_or(rows.len() as u64);
        if through < total {
            out.push_str(&format!(
                "  … {} more; raise --limit or ask for --page {}\n",
                total - through,
                data["page"].as_u64().unwrap_or(0) + 1,
            ));
        }
    }
    out
}
