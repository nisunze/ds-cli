//! `ds pm party create` — name a counterparty once, so records can name it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_correspondence::Action;
use serde_json::{Map, Value, json};

use crate::LANE_ARG;

pub const NAME_ARG: Arg = Arg::value(
    "name",
    "<text>",
    "The organisation's or person's name. Unique per kind within the project.",
);
pub const KIND_ARG: Arg = Arg::value(
    "kind",
    "<organisation|person>",
    "What the party is (`ds pm plan` publishes the kinds).",
);
pub const ROLE_ARG: Arg = Arg::value(
    "role",
    "<role>",
    "client, consultant, contractor, supplier, authority or other (`ds pm plan` publishes the roles).",
);
pub const ORGANISATION_ARG: Arg = Arg::value(
    "organisation",
    "<party-id>",
    "For a person: the organisation party they belong to.",
);
pub const EMAIL_ARG: Arg = Arg::repeated(
    "email",
    "<address>",
    "An address the party writes from. Repeat for several (at most 20).",
);
pub const PHONE_ARG: Arg = Arg::repeated(
    "phone",
    "<number>",
    "A phone number. Repeat for several (at most 20).",
);
pub const NOTES_ARG: Arg = Arg::value(
    "notes",
    "<text>",
    "Anything worth knowing about the party (at most 2000 characters).",
);
const ID_ARG: Arg = Arg::value(
    "id",
    "<party-id>",
    "Mint this id. Reuse it on a retry; a second create is refused as existing.",
);

pub static COMMAND: Command = Command {
    id: "pm.party.create",
    path: &["pm", "party", "create"],
    contract: 1,
    summary: "Add an outside organisation or person the project corresponds with.",
    purpose: "\
A record names its counterparties by party id and the party that owes the \
next answer is one of these, so a party is created once and referenced \
everywhere. A duplicate name of the same kind is refused rather than \
doubled; a retry with the same --id is refused rather than duplicated. \
Headless: writes to the named project of the signed-in native \
credential, no window.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        NAME_ARG.required(),
        KIND_ARG.required(),
        ROLE_ARG,
        ORGANISATION_ARG,
        EMAIL_ARG,
        PHONE_ARG,
        NOTES_ARG,
        ID_ARG,
        LANE_ARG,
        crate::PROJECT_ARG,
    ],
    output: "The project and `party` — the row as the server holds it, with its `id` and `version`.",
    examples: &[Example {
        command: "ds pm party create --name \"Acme Consultancy\" --kind organisation --role consultant --email review@acme.example --yes --project <exact-id>",
        note: "Read .data.party.id to name the party on a record.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<25>(&[
        crate::PARTY_EXISTS,
        crate::PARTY_NOT_FOUND,
        crate::INVALID_EMAIL,
        crate::BOUND_EXCEEDED,
    ]),
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
        "company",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

/// The party fields a create or an update carries, from the flags given.
pub fn fields(inputs: &Inputs) -> Result<Map<String, Value>, Failure> {
    let mut data = Map::new();
    for (flag, key) in [
        ("name", "name"),
        ("kind", "kind"),
        ("role", "role"),
        ("organisation", "organisation_party_id"),
        ("notes", "notes"),
    ] {
        if let Some(value) = inputs.value(flag) {
            data.insert(key.into(), json!(value.trim()));
        }
    }
    let emails = inputs.repeated("email");
    if !emails.is_empty() {
        let emails: Vec<String> = emails
            .iter()
            .map(|address| crate::email(address, "email"))
            .collect::<Result<_, _>>()?;
        data.insert("emails".into(), json!(emails));
    }
    let phones = inputs.repeated("phone");
    if !phones.is_empty() {
        data.insert("phones".into(), json!(phones));
    }
    Ok(data)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let data = fields(inputs)?;
    let report = crate::correspondence(
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
        &Action::PartyCreate {
            id: inputs.value("id").map(str::to_owned),
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
        "created party {} in {}\n{}",
        data["party"]["id"].as_str().unwrap_or("?"),
        data["project"].as_str().unwrap_or("?"),
        super::party_line(&data["party"]),
    )
}
