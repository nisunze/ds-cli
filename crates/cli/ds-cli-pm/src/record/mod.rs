//! `ds pm record` — the project's correspondence: what was said, sent,
//! asked and decided with the outside world, and who owes the next answer.
//!
//! A record is one externally facing exchange (correspondence.md): an
//! email, a letter, a chat message, a call, a site note, a meeting with an
//! outside party, a submission or a transmittal. It is always authored
//! against something — the ingested asset, the message, the meeting note, or
//! the record it replies to — names its parties by id, and may name who owes
//! the next answer by when. Replies share the parent's thread; a reply from
//! the owing side settles the parent's response and clears every task
//! blocked on it in the same commit.

pub mod create;
pub mod list;
pub mod read;
pub mod reply;
pub mod thread;
pub mod update;

use ds_cli_contract::Inputs;
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Arg;
use serde_json::{Map, Value, json};

pub const RECORD_ARG: Arg = Arg::value(
    "record",
    "<record-id>",
    "The record, by the id `ds pm record list` reports.",
)
.required();
pub const CATEGORY_ARG: Arg = Arg::value(
    "category",
    "<category>",
    "What kind of exchange: instruction, request_for_information, review, submission, transmittal, decision, meeting, response… (`ds pm plan` publishes the list).",
);
pub const CHANNEL_ARG: Arg = Arg::value(
    "channel",
    "<channel>",
    "How it travelled: email, letter, chat, call, meeting, site… (`ds pm plan` publishes the list; never system).",
);
pub const DIRECTION_ARG: Arg = Arg::value(
    "direction",
    "<inbound|outbound|internal>",
    "inbound when the outside party wrote to the project, outbound when the project wrote.",
);
pub const SUBJECT_ARG: Arg = Arg::value(
    "subject",
    "<text>",
    "The subject line, as the exchange carried it.",
);
pub const BODY_ARG: Arg = Arg::value(
    "body",
    "<text>",
    "The text of the record. Mutually exclusive with --body-file.",
);
pub const BODY_FILE_ARG: Arg = Arg::value(
    "body-file",
    "<path>",
    "A UTF-8 text file whose contents are the body (at most 64 KiB).",
);
pub const HAPPENED_AT_ARG: Arg = Arg::value(
    "happened-at",
    "<yyyy-mm-dd | instant>",
    "When the exchange happened; a day, or an RFC 3339 instant. Defaults to now on the server.",
);
pub const REFERENCE_ARG: Arg = Arg::value(
    "reference",
    "<text>",
    "The counterparty's reference number for the exchange, when it carries one.",
);
pub const PARTY_ARG: Arg = Arg::repeated(
    "party",
    "<party-id>",
    "A party the exchange involves, from `ds pm party list`. Repeat for several.",
);
pub const EXTERNAL_NAME_ARG: Arg = Arg::repeated(
    "external-name",
    "<text>",
    "A named participant not yet a party. Repeat for several.",
);
pub const SOURCE_ASSET_ARG: Arg = Arg::value(
    "source-asset",
    "<asset-id>",
    "The ingested asset the record is about — the .eml, the screenshot, the letter (`ds assets ingest`).",
);
pub const SOURCE_MESSAGE_ARG: Arg = Arg::value(
    "source-message",
    "<conversation-id,message-id>",
    "The in-app message the record promotes.",
);
pub const MEETING_NOTE_ARG: Arg = Arg::value(
    "meeting-note",
    "<asset-id>",
    "The note asset the record is about (an asset of kind note).",
);
pub const RESPONSE_OWED_BY_ARG: Arg = Arg::value(
    "response-owed-by",
    "<party-id | email>",
    "Who owes the next answer: a party, or a project member by email. Naming one is owing a response.",
);
pub const RESPONSE_DUE_ARG: Arg = Arg::value(
    "response-due",
    "<yyyy-mm-dd>",
    "The day the answer is due. Without it the answer is outstanding until given or waived.",
);
pub const AFFECTS_ARG: Arg = Arg::value(
    "affects",
    "<scope,schedule,quality,cost>",
    "What the exchange bears on, comma-separated.",
);
pub const DOCUMENT_ARG: Arg = Arg::repeated(
    "document",
    "<asset-id>",
    "A registered document the record carries (`ds assets classify --document-number …`). A submission or transmittal needs at least one.",
);
pub const ID_ARG: Arg = Arg::value(
    "id",
    "<record-id>",
    "Mint this id. Reuse it on a retry; a second create is refused as existing.",
);

/// The record fields a create or a reply carries, from the flags given.
/// `source` says whether the source flags are read (a reply takes only
/// `--source-asset`).
pub fn fields(inputs: &Inputs, source: bool) -> Result<Map<String, Value>, Failure> {
    let mut data = Map::new();
    for (flag, key) in [
        ("category", "category"),
        ("channel", "channel"),
        ("direction", "direction"),
        ("subject", "subject"),
        ("reference", "reference_number"),
    ] {
        if let Some(value) = inputs.value(flag) {
            data.insert(key.into(), json!(value.trim()));
        }
    }
    if let Some(body) = crate::body(inputs)? {
        data.insert("body".into(), json!(body));
    }
    if let Some(stamp) = inputs.value("happened-at") {
        data.insert(
            "happened_at".into(),
            json!(crate::stamp(stamp, "happened-at")?),
        );
    }
    let parties = inputs.repeated("party");
    if !parties.is_empty() {
        data.insert("external_party_ids".into(), json!(parties));
    }
    let names = inputs.repeated("external-name");
    if !names.is_empty() {
        data.insert("external_participant_names".into(), json!(names));
    }
    if let Some(asset) = inputs.value("source-asset") {
        data.insert("source_asset_id".into(), json!(asset.trim()));
    }
    if source {
        if let Some(message) = inputs.value("source-message") {
            let (conversation, message_id) = message.split_once(',').ok_or_else(|| {
                Failure::invalid(
                    crate::INVALID_VALUE.code,
                    "`--source-message` is `<conversation-id>,<message-id>`",
                )
                .remedy("pass e.g. --source-message conv_ab12,msg_cd34")
            })?;
            data.insert("source_conversation_id".into(), json!(conversation.trim()));
            data.insert("source_message_id".into(), json!(message_id.trim()));
        }
        if let Some(note) = inputs.value("meeting-note") {
            data.insert("meeting_note_asset_id".into(), json!(note.trim()));
        }
    }
    if let Some(owner) = inputs.value("response-owed-by") {
        let (key, value) = crate::response_owner(owner)?;
        data.insert(key.into(), json!(value));
    }
    if let Some(due) = inputs.value("response-due") {
        data.insert(
            "response_due_date".into(),
            json!(crate::date(due, "response-due")?),
        );
    }
    if let Some(affects) = inputs.value("affects") {
        for (key, flag) in crate::affects(affects)? {
            data.insert(key.into(), json!(flag));
        }
    }
    let documents = inputs.repeated("document");
    if !documents.is_empty() {
        data.insert("document_ids".into(), json!(documents));
    }
    Ok(data)
}

/// Fold one record view the door answered into `ds pm record read`'s shape.
pub fn view(report: ds_cli_auth::HeadlessNamedProject<Value>) -> Result<Value, Failure> {
    let project = report.project_id().to_owned();
    let folded = ds_command_kernel::project_management::correspondence::record_view(
        &project,
        &report.into_result(),
    )
    .ok_or_else(|| {
        Failure::internal(
            crate::PLAN_UNREADABLE.code,
            "the record answered is not a record view",
        )
        .remedy(crate::PLAN_UNREADABLE.remedy)
    })?;
    crate::data(&folded)
}

/// Render one record's head the same way in every projection.
pub fn head(record: &Value) -> String {
    let mut out = format!(
        "{} {}\n  {} · {} · {} · {}{}\n",
        record["id"].as_str().unwrap_or("?"),
        record["subject"].as_str().unwrap_or("(no subject)"),
        record["category"].as_str().unwrap_or("—"),
        record["channel"].as_str().unwrap_or("—"),
        record["direction"].as_str().unwrap_or("—"),
        record["happenedAt"]
            .as_str()
            .unwrap_or("—")
            .get(..10)
            .unwrap_or("—"),
        record["sensitivity"]
            .as_str()
            .filter(|class| *class != "open")
            .map(|class| format!(" · {class}"))
            .unwrap_or_default(),
    );
    if let Some(status) = record["responseStatus"].as_str().filter(|s| *s != "none") {
        let owed = record["responseOwnerPartyId"]
            .as_str()
            .map(|party| format!(" by party {party}"))
            .or_else(|| {
                record["responseOwnerId"]
                    .as_str()
                    .map(|_| " by the project".to_string())
            })
            .unwrap_or_default();
        let due = record["responseDueDate"]
            .as_str()
            .map(|due| format!(" · due {due}"))
            .unwrap_or_default();
        out.push_str(&format!("  response {status}{owed}{due}\n"));
    }
    out
}

/// Render the projections a record view carries.
pub fn projections(data: &Value) -> String {
    let mut out = String::new();
    if let Some(rows) = data["attachments"]
        .as_array()
        .filter(|rows| !rows.is_empty())
    {
        out.push_str(&format!(
            "  attachments ({} of {})\n",
            rows.len(),
            data["attachmentsTotal"]
                .as_u64()
                .unwrap_or(rows.len() as u64)
        ));
        for row in rows {
            out.push_str(&format!(
                "    {:<14} {:<6} {:<36}{}\n",
                row["assetId"].as_str().unwrap_or("?"),
                row["kind"].as_str().unwrap_or("—"),
                crate::truncate(row["name"].as_str().unwrap_or("?"), 36),
                if row["bytesHeld"].as_bool() == Some(false) {
                    format!(" → {}", row["externalUrl"].as_str().unwrap_or("(link)"))
                } else if let Some(parts) = row["mailParts"].as_array().filter(|p| !p.is_empty()) {
                    format!(" · {} parts", parts.len())
                } else {
                    String::new()
                },
            ));
        }
    }
    if let Some(rows) = data["documents"].as_array().filter(|rows| !rows.is_empty()) {
        out.push_str("  documents\n");
        for row in rows {
            out.push_str(&format!(
                "    {:<14} {} {} {}\n",
                row["assetId"].as_str().unwrap_or("?"),
                row["document"]["number"].as_str().unwrap_or("—"),
                row["document"]["revision_label"].as_str().unwrap_or(""),
                row["document"]["state"].as_str().unwrap_or(""),
            ));
        }
    }
    if let Some(rows) = data["blockedTasks"]
        .as_array()
        .filter(|rows| !rows.is_empty())
    {
        out.push_str("  tasks waiting on this record\n");
        for row in rows {
            out.push_str(&format!(
                "    {:<14} {}\n",
                row["taskId"].as_str().unwrap_or("?"),
                crate::truncate(row["title"].as_str().unwrap_or(""), 50)
            ));
        }
    }
    if let Some(rows) = data["resultingTasks"]
        .as_array()
        .filter(|rows| !rows.is_empty())
    {
        out.push_str("  tasks made from this record\n");
        for row in rows {
            out.push_str(&format!(
                "    {:<14} {}\n",
                row["taskId"].as_str().unwrap_or("?"),
                crate::truncate(row["title"].as_str().unwrap_or(""), 50)
            ));
        }
    }
    out
}
