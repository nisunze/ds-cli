//! `ds pm party update` — correct or archive a counterparty.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::Action;
use serde_json::{Value, json};

use super::create::{
    EMAIL_ARG, KIND_ARG, NAME_ARG, NOTES_ARG, ORGANISATION_ARG, PHONE_ARG, ROLE_ARG,
};
use crate::LANE_ARG;

const ARCHIVE_ARG: Arg = Arg::switch(
    "archive",
    "Hide the party from lists; records that name it keep naming it.",
);
const UNARCHIVE_ARG: Arg = Arg::switch("unarchive", "List the party again.");

const NOTHING_TO_UPDATE: Refusal = Refusal {
    code: "nothing_to_update",
    when: "no field, --archive or --unarchive was given",
    remedy: "name at least one change, e.g. --role contractor",
};

pub static COMMAND: Command = Command {
    id: "pm.party.update",
    path: &["pm", "party", "update"],
    contract: 1,
    summary: "Change a party's name, role, organisation or contacts; archive it.",
    purpose: "\
Corrects one party in place — the same fields `ds pm party create` takes — or \
archives it so it drops out of lists while every record that names it keeps \
naming it. The current version is read first and the change is refused \
if the party moved in between. Headless: writes to the selected project of \
the signed-in native credential, no window.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        super::PARTY_ARG,
        NAME_ARG,
        KIND_ARG,
        ROLE_ARG,
        ORGANISATION_ARG,
        EMAIL_ARG,
        PHONE_ARG,
        NOTES_ARG,
        ARCHIVE_ARG,
        UNARCHIVE_ARG,
        LANE_ARG,
    ],
    output: "The project and `party` — the row as the server now holds it.",
    examples: &[Example {
        command: "ds pm party update --party p_acme --role contractor --yes",
        note: "Repeat --email to replace the whole list of addresses.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<26>(&[
        crate::PARTY_EXISTS,
        crate::PARTY_NOT_FOUND,
        crate::INVALID_EMAIL,
        crate::BOUND_EXCEEDED,
        NOTHING_TO_UPDATE,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "correspondence",
        "counterparty",
        "archive",
        "contact",
        "rename",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let party_id = inputs.require("party")?;
    let mut data = super::create::fields(inputs)?;
    if inputs.switch("archive") {
        data.insert("archived".into(), json!(true));
    } else if inputs.switch("unarchive") {
        data.insert("archived".into(), json!(false));
    }
    if data.is_empty() {
        return Err(
            Failure::invalid(NOTHING_TO_UPDATE.code, "no change was named")
                .remedy(NOTHING_TO_UPDATE.remedy)
                .next("ds pm party update --help"),
        );
    }
    let lane = inputs.value("lane").unwrap_or("stable");
    let Some((_, current)) = super::find(lane, party_id)? else {
        return Err(Failure::invalid(
            crate::PARTY_NOT_FOUND.code,
            format!("No party {party_id} in this project."),
        )
        .detail(json!({ "party": party_id }))
        .remedy(crate::PARTY_NOT_FOUND.remedy)
        .next("ds pm party list --include-archived"));
    };
    let report = crate::correspondence(
        lane,
        &Action::PartyUpdate {
            id: party_id.to_owned(),
            expected_version: current["version"].as_i64().unwrap_or(1).max(1),
            data,
        },
    )?;
    let project = report.project_id().to_owned();
    let party = ds_command_kernel::project_management::correspondence::party_row(
        &report.into_result()["item"],
    )
    .ok_or_else(|| {
        Failure::internal(
            crate::PLAN_UNREADABLE.code,
            "the party answered is not a row",
        )
        .remedy(crate::PLAN_UNREADABLE.remedy)
    })?;
    Ok(json!({ "project": project, "party": party }))
}

pub fn render(data: &Value) -> String {
    format!(
        "updated party {} in {}\n{}",
        data["party"]["id"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        super::party_line(&data["party"]),
    )
}
