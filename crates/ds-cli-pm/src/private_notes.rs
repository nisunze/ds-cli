//! Explicit account-private note controls using the existing native owner.
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Execution, Requires};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_client_core::personal_notes::{Action, Draft};
use serde_json::{Value, json};
use std::io::Read;

const INPUT: Arg = Arg::value("input", "<json-file>", "Closed complete authored draft JSON, at most 512 KiB (markdown at most 128 KiB); no identity, author, timestamps or transport envelope.").required();

pub(super) const NOTE: Arg = Arg::value(
    "note",
    "<note-id>",
    "One selected account-private note; no project discovery.",
)
.required();
pub(super) const VERSION: Arg = Arg::value(
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
pub(super) const ID: Arg = Arg::value(
    "id",
    "<command-id>",
    "Stable 8-128 character id; retain exact restoration intent on retry.",
)
.required();
const LIMIT: Arg = Arg::value(
    "limit",
    "<count>",
    "Source rows to acquire on this page (1-100); filtering may return fewer and continuation stays explicit.",
)
.default("50");
const TERM: Arg = Arg::value(
    "term",
    "<text>",
    "Case-insensitive substring in this page's title, folder or tags only (1-256 characters); never searches bodies or fetches another page.",
);
const CURSOR: Arg = Arg::value(
    "cursor",
    "<cursor>",
    "Opaque next_cursor from the same account and selected note/query; fetches only this explicit page.",
);
const ARCHIVED: Arg = Arg::switch(
    "include-archived",
    "Include organizationally archived notes; these remain editable.",
);
const TRASHED: Arg = Arg::switch(
    "include-retired",
    "Include notes in recoverable private trash.",
);

pub(super) const fn declaration(
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
    "Reads one metadata-only page ordered by stable note ID, at most --limit source rows plus one header lookahead. --cursor explicitly requests the next page; nothing auto-pages or refreshes. --term searches title/folder/tags on this page only, never bodies or the whole collection. Filtered pages can be empty with a next cursor. Source completeness is distinct from matched count; collection totals are unknown.",
    Effect::ReadOnly,
    &[LIMIT, CURSOR, TERM, ARCHIVED, TRASHED, crate::LANE_ARG],
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
    "Reads one immutable version-header page, with --limit and explicit --cursor; no eager next page. The first observed head remains the cursor cutoff if the live note changes. Enumeration complete/truncated is separate from history_chain_status: unseen history is unknown, observed missing history is incomplete, and complete requires a fully proven retained chain. Never fetches historical bodies.",
    Effect::ReadOnly,
    &[NOTE, LIMIT, CURSOR, crate::LANE_ARG],
);
pub static READ_VERSION: Command = declaration(
    "notes.version.read",
    &["notes", "version", "read"],
    "Read one private note revision and its exact SHA256.",
    "Reads one selected immutable retained snapshot. Missing current baselines require administrator recovery from verified source evidence; unavailable earlier history is never synthesized. No historical attachment bytes are downloaded.",
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
            limit: limit(inputs)?,
            cursor: inputs.value("cursor").map(str::to_owned),
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
        "notes.versions" => Action::Versions {
            id: note()?,
            limit: limit(inputs)?,
            cursor: inputs.value("cursor").map(str::to_owned),
        },
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
fn term(inputs: &Inputs) -> Result<Option<String>, Failure> {
    inputs
        .value("term")
        .map(|value| {
            let value = value.trim();
            if value.is_empty() || value.chars().count() > 256 {
                return Err(invalid("term must have 1-256 characters"));
            }
            Ok(value.to_lowercase())
        })
        .transpose()
}
fn project_list(data: Value, term: Option<&str>) -> Value {
    let notes = data["notes"]
        .as_array()
        .expect("verified private metadata list");
    let matching: Vec<_> = notes
        .iter()
        .filter(|note| {
            term.is_none_or(|term| {
                [note["title"].as_str(), note["folder"].as_str()]
                    .into_iter()
                    .flatten()
                    .chain(
                        note["tags"]
                            .as_array()
                            .into_iter()
                            .flatten()
                            .filter_map(Value::as_str),
                    )
                    .any(|text| text.to_lowercase().contains(term))
            })
        })
        .collect();
    let summaries: Vec<_> = matching.iter().cloned().collect();
    json!({"notes":summaries,"returned":summaries.len(),"acquired":notes.len(),"scanned_on_page":data["scanned_on_page"],
        "matched_on_page":matching.len(),"limit":data["limit"],"next_cursor":data.get("next_cursor"),
        "complete":data["complete"],"truncated":data["truncated"],"total_unknown":true,
        "search_scope":"title_folder_tags_on_page","term":term})
}
fn execute(inputs: &Inputs, command: &str) -> Result<Value, Failure> {
    let selected_term = if command == "notes.list" {
        term(inputs)?
    } else {
        None
    };
    let action = action(inputs, command)?;
    let mut data =
        ds_cli_auth::personal_notes::execute(inputs.value("lane").unwrap_or("stable"), &action)?;
    if command == "notes.list" {
        return Ok(project_list(data, selected_term.as_deref()));
    }
    if command == "notes.versions" {
        data["returned"] = json!(
            data["versions"]
                .as_array()
                .expect("verified version page")
                .len()
        );
        data["total_unknown"] = json!(true);
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
        let data = json!({"notes":[{"id":"one","version":1,"title":"A","tags":[]},{"id":"two","version":2,"title":"B","tags":["field"]}],"limit":2,"scanned_on_page":2,"complete":true,"truncated":false});
        let view = project_list(data.clone(), None);
        assert_eq!(view["returned"], 2);
        assert_eq!(view["complete"], true);
        assert_eq!(
            project_list(data.clone(), Some("field"))["notes"][0]["id"],
            "two"
        );
        assert_eq!(
            project_list(data, Some("private body"))["matched_on_page"],
            0
        );
        assert!(term(&inputs(&LIST, &["--term", " "])).is_err());
        assert!(term(&inputs(&LIST, &["--term", &"x".repeat(257)])).is_err());
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
