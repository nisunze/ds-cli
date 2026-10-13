//! Explicit account-private note controls using the existing native owner.
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution, Requires};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_client_core::personal_notes::{Action, Draft};
use serde_json::{Value, json};
use std::io::Read;

const INPUT: Arg = Arg::value("input", "<json-file>", "Closed complete authored draft JSON, at most 512 KiB (markdown at most 128 KiB); no identity, author, timestamps or transport envelope.").required();

const NOTE: Arg = Arg::value(
    "note",
    "<note-id>",
    "One selected account-private note; no project discovery.",
)
.required();
const VERSION: Arg = Arg::value(
    "version",
    "<version>",
    "Positive current version you reviewed; stale writes refuse.",
)
.required();
const SOURCE: Arg = Arg::value(
    "source-version",
    "<version>",
    "One positive historical version; unavailable history is never fabricated.",
)
.required();
const SHA: Arg = Arg::value(
    "source-sha256",
    "<sha256>",
    "Exact lowercase SHA256 returned by the selected historical read.",
)
.required();
const ID: Arg = Arg::value(
    "id",
    "<command-id>",
    "Stable 8-128 character id; retain exact restoration intent on retry.",
)
.required();
const LIMIT: Arg = Arg::value(
    "limit",
    "<count>",
    "Metadata rows to show (1-100); more and acquisition completeness remain explicit.",
)
.default("50");
const ARCHIVED: Arg = Arg::switch(
    "include-archived",
    "Include organizationally archived notes; these remain editable.",
);
const TRASHED: Arg = Arg::switch(
    "include-retired",
    "Include notes in recoverable private trash.",
);

const fn declaration(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    purpose: &'static str,
    effect: Effect,
    args: &'static [Arg],
) -> Command {
    Command {
        id,
        path,
        summary,
        purpose,
        effect,
        authority: Authority::HeadlessUser,
        args,
        refusals: &crate::checklist_notes::PRIVATE_REFUSALS,
        contract: 1,
        chapter: Chapter::Project,
        execution: Execution::Sync,
        output: "Account-private note metadata, selected content or owner-verified version/state receipt. No hidden owner UID or project catalogue.",
        examples: &[],
        reference: Some("docs/reference/pm.md"),
        search: &["private", "notes", "history", "restore", "authoring"],
        requires: Requires::Server,
        availability: ds_cli_auth::native_availability,
    }
}
pub static CREATE: Command = declaration(
    "notes.create",
    &["notes", "create"],
    "Create one explicitly account-private note.",
    "Use only for deliberately private personal material; project decisions, constraints and delivery evidence belong in shared pm note create with explicit --project. Reads a closed complete authored draft JSON and creates version 1 with --yes. Markdown/checklist/links are deliberate; no automatic note hook or private-to-project copy. Existing reminder effects apply when authored. Creation allocates a fresh id and is not idempotent: after an uncertain outcome inspect notes list before retrying. No model attribution field is invented.",
    Effect::GlobalWrite,
    &[INPUT, crate::LANE_ARG],
);
pub static UPDATE: Command = declaration(
    "notes.update",
    &["notes", "update"],
    "Replace one reviewed private note's authored fields.",
    "Closed complete authored draft JSON replaces title, content, folder, tags, checklist, links, geometry, reminder, canvas, pin and archive against --version with --yes. Omitted authored fields clear or take owner defaults; review notes read first and preserve every field you intend to keep. Legacy simple checklist rows retain richer matching item fields; explicit rich clears use notes checklist edit. Authenticated author, history, trash and attachment state remain owner-controlled. Existing reminder effects apply. After an uncertain outcome read the selected note before retrying; a successful write advances version and stale retry refuses. Meaningful project rulings belong in shared pm note commands.",
    Effect::GlobalWrite,
    &[NOTE, VERSION, INPUT, crate::LANE_ARG],
);
pub static LIST: Command = declaration(
    "notes.list",
    &["notes", "list"],
    "List account-private note metadata.",
    "Explicitly reads this account's bounded notes, showing concise metadata only. The legacy owner may not prove enumeration completeness; that limit stays visible. Nothing is refreshed in the background.",
    Effect::ReadOnly,
    &[LIMIT, ARCHIVED, TRASHED, crate::LANE_ARG],
);
pub static READ: Command = declaration(
    "notes.read",
    &["notes", "read"],
    "Read one account-private note.",
    "Reads the selected current note, its attachment descriptors and owner-computed checklist. No attachment bytes, project graph or other notes are acquired.",
    Effect::ReadOnly,
    &[NOTE, crate::LANE_ARG],
);
pub static RETIRE: Command = declaration(
    "notes.retire",
    &["notes", "retire"],
    "Move one private note into recoverable trash.",
    "Soft-retirement of current private content against its reviewed version, with --yes. This is distinct from organizational archive. Repeating an unchanged state does not create a revision; a changed state creates one. Recovery uses notes restore.",
    Effect::GlobalWrite,
    &[NOTE, VERSION, crate::LANE_ARG],
);
pub static RESTORE: Command = declaration(
    "notes.restore",
    &["notes", "restore"],
    "Restore a private note's current content from trash.",
    "Clears recoverable trash against the reviewed version, with --yes. Keeps the current authored content; this does not select a historical revision. Existing owner reminder rules remain in force. Historical content recovery uses notes version restore.",
    Effect::GlobalWrite,
    &[NOTE, VERSION, crate::LANE_ARG],
);
pub static VERSIONS: Command = declaration(
    "notes.versions",
    &["notes", "versions"],
    "List one private note's available revision metadata.",
    "Reads only selected history metadata. Enumeration completeness and fully retained history are separate: a legacy current observation, a gap or truncation never claims a recoverable full chain. At most 100 owner descriptors are acquired.",
    Effect::ReadOnly,
    &[NOTE, LIMIT, crate::LANE_ARG],
);
pub static READ_VERSION: Command = declaration(
    "notes.version.read",
    &["notes", "version", "read"],
    "Read one private note revision and its exact SHA256.",
    "Reads one selected immutable snapshot or legacy current observation. retained=false declares an observed current head, not an immutable older snapshot. No historical attachment bytes are downloaded.",
    Effect::ReadOnly,
    &[NOTE, SOURCE, crate::LANE_ARG],
);
pub static RESTORE_VERSION: Command = declaration(
    "notes.version.restore",
    &["notes", "version", "restore"],
    "Restore selected private authored content as a fresh revision.",
    "With --yes, restores the selected source version and exact SHA256 against the reviewed current version. Keeps current identity, creation attribution, archive and reminder, clears trash, and rechecks ready evidence. Never schedules or fires a reminder. Exact command retry returns its original receipt.",
    Effect::GlobalWrite,
    &[NOTE, VERSION, SOURCE, SHA, ID, crate::LANE_ARG],
);

fn integer(inputs: &Inputs, flag: &str) -> Result<i64, Failure> {
    let value = inputs
        .require(flag)?
        .parse::<i64>()
        .map_err(|_| invalid("expected a positive integer"))?;
    if !(1..i64::MAX).contains(&value) {
        return Err(invalid("version is outside its positive bound"));
    }
    Ok(value)
}
fn invalid(reason: impl std::fmt::Display) -> Failure {
    Failure::invalid("notes_request_invalid", reason.to_string())
        .remedy("use one stable private note and the declared reviewed version and source hash")
}
fn action(inputs: &Inputs, command: &str) -> Result<Action, Failure> {
    let note = || inputs.require("note").map(str::to_owned);
    let action = match command {
        "notes.create" => Action::Create {
            draft: selected_draft(inputs)?,
        },
        "notes.update" => Action::Update {
            id: note()?,
            expected_version: integer(inputs, "version")?,
            draft: selected_draft(inputs)?,
        },
        "notes.list" => Action::List {
            include_archived: inputs.switch("include-archived"),
            include_trashed: inputs.switch("include-retired"),
        },
        "notes.read" => Action::Read { id: note()? },
        "notes.retire" => Action::Retire {
            id: note()?,
            expected_version: integer(inputs, "version")?,
        },
        "notes.restore" => Action::Restore {
            id: note()?,
            expected_version: integer(inputs, "version")?,
        },
        "notes.versions" => Action::Versions { id: note()? },
        "notes.version.read" => Action::ReadVersion {
            id: note()?,
            source_version: integer(inputs, "source-version")?,
        },
        "notes.version.restore" => Action::RestoreVersion {
            id: note()?,
            expected_version: integer(inputs, "version")?,
            source_version: integer(inputs, "source-version")?,
            source_sha256: inputs.require("source-sha256")?.to_owned(),
            command_id: inputs.require("id")?.to_owned(),
        },
        _ => unreachable!("fixed private command"),
    };
    action.validate().map_err(invalid)?;
    Ok(action)
}
fn selected_draft(inputs: &Inputs) -> Result<Draft, Failure> {
    let mut bytes = Vec::new();
    std::fs::File::open(inputs.require("input")?)
        .map_err(invalid)?
        .take(ds_client_core::personal_notes::MAX_REQUEST_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(invalid)?;
    if bytes.len() > ds_client_core::personal_notes::MAX_REQUEST_BYTES {
        return Err(invalid("private draft exceeds 512 KiB"));
    }
    serde_json::from_slice(&bytes).map_err(invalid)
}
fn limit(inputs: &Inputs) -> Result<usize, Failure> {
    let value = inputs
        .value("limit")
        .unwrap_or("50")
        .parse::<usize>()
        .map_err(|_| invalid("limit must be 1-100"))?;
    if !(1..=100).contains(&value) {
        return Err(invalid("limit must be 1-100"));
    }
    Ok(value)
}
fn project_list(data: Value, limit: usize) -> Value {
    let notes = data["notes"].as_array().expect("verified private list");
    let summaries: Vec<_> = notes.iter().take(limit).map(|note| json!({
        "id":note["id"],"version":note["version"],"title":note["title"],"folder":note["folder"],
        "pinned":note["pinned"],"archived":note["archived"],"trashed":note["trashed"],"updated_at":note["updated_at"]
    })).collect();
    json!({"notes":summaries,"returned":summaries.len(),"acquired":notes.len(),"max_notes":data["max_notes"],
        "more":notes.len()>limit,"acquisition_complete":data.get("complete"),"acquisition_truncated":data.get("truncated"),
        "completeness_unknown":data.get("complete").is_none()})
}
fn execute(inputs: &Inputs, command: &str) -> Result<Value, Failure> {
    let max = if matches!(command, "notes.list" | "notes.versions") {
        limit(inputs)?
    } else {
        0
    };
    let action = action(inputs, command)?;
    let mut data =
        ds_cli_auth::personal_notes::execute(inputs.value("lane").unwrap_or("stable"), &action)?;
    if command == "notes.list" {
        return Ok(project_list(data, max));
    }
    if command == "notes.versions" {
        let versions = data["versions"]
            .as_array()
            .expect("verified history metadata");
        let count = versions.len();
        data["versions"] = json!(versions.iter().take(max).collect::<Vec<_>>());
        data["more"] = json!(count > max);
        data["acquired"] = json!(count);
        // Preserve the owner's history proof; more concerns only this projection.
    }
    Ok(data)
}
macro_rules! handler {
    ($name:ident, $id:literal) => {
        pub fn $name(i: &Inputs, _: &Context) -> Result<Value, Failure> {
            execute(i, $id)
        }
    };
}
handler!(create, "notes.create");
handler!(update, "notes.update");
handler!(list, "notes.list");
handler!(read, "notes.read");
handler!(retire, "notes.retire");
handler!(restore, "notes.restore");
handler!(versions, "notes.versions");
handler!(read_version, "notes.version.read");
handler!(restore_version, "notes.version.restore");
pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default() + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;
    fn inputs(command: &Command, args: &[&str]) -> Inputs {
        ds_cli_contract::args::parse(
            command,
            &args.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>(),
        )
        .unwrap()
    }
    #[test]
    fn private_restore_maps_reviewed_hash_without_project_or_owner() {
        let sha = "a".repeat(64);
        let i = inputs(
            &RESTORE_VERSION,
            &[
                "--note",
                "private-1",
                "--version",
                "8",
                "--source-version",
                "2",
                "--source-sha256",
                &sha,
                "--id",
                "restore-private-001",
            ],
        );
        let payload = action(&i, RESTORE_VERSION.id).unwrap().payload();
        assert_eq!(
            payload,
            json!({"action":"restore_version","id":"private-1","expected_version":8,"source_version":2,"source_sha256":sha,"command_id":"restore-private-001"})
        );
        assert!(payload.get("project_id").is_none());
        assert!(payload.get("owner").is_none());
        assert!(RESTORE_VERSION.confirmation_required_for(&i));
        assert_eq!(READ.effect, Effect::ReadOnly);
    }
    #[test]
    fn private_list_metadata_is_bounded_and_never_invents_complete_history() {
        let data = json!({"notes":[{"id":"one","version":1,"title":"A","content":{"markdown":"private body"}},{"id":"two","version":2}],"max_notes":1000});
        let view = project_list(data, 1);
        assert_eq!(view["more"], true);
        assert_eq!(view["completeness_unknown"], true);
        assert!(view["notes"][0].get("content").is_none());
        assert!(
            action(
                &inputs(&RETIRE, &["--note", "x", "--version", "0"]),
                RETIRE.id
            )
            .is_err()
        );
        assert!(limit(&inputs(&LIST, &["--limit", "101"])).is_err());
    }
    #[test]
    fn private_authoring_is_explicit_and_refuses_input_before_authentication() {
        let i = inputs(&CREATE, &["--input", "missing-private-note-draft.json"]);
        assert_eq!(
            execute(&i, CREATE.id).unwrap_err().code(),
            "notes_request_invalid"
        );
        assert!(CREATE.confirmation_required_for(&i));
        let i = inputs(
            &UPDATE,
            &[
                "--note",
                "private",
                "--version",
                "0",
                "--input",
                "missing-private-note-draft.json",
            ],
        );
        assert_eq!(
            action(&i, UPDATE.id).unwrap_err().code(),
            "notes_request_invalid"
        );
        assert!(CREATE.purpose.contains("not idempotent"));
        assert!(UPDATE.purpose.contains("Omitted authored fields clear"));
        assert!(CREATE.purpose.contains("shared pm note create"));
    }
    #[test]
    fn declarations_are_private_only_and_distinguish_current_from_historic_restore() {
        for command in [
            &CREATE,
            &UPDATE,
            &LIST,
            &READ,
            &RETIRE,
            &RESTORE,
            &VERSIONS,
            &READ_VERSION,
            &RESTORE_VERSION,
        ] {
            assert!(
                command
                    .args
                    .iter()
                    .all(|arg| arg.name != "project" && arg.name != "owner")
            );
            assert_eq!(command.authority, Authority::HeadlessUser);
            assert!(!command.summary.is_empty());
        }
        assert!(RESTORE.purpose.contains("current authored content"));
        assert!(RESTORE_VERSION.purpose.contains("Never schedules"));
    }
}
