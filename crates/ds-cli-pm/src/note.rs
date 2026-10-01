//! `ds pm note create` — a short, project-visible human review note.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_client_core::project_management::Command as PMCommand;
use serde_json::{Value, json};

use crate::LANE_ARG;

const MAX_NOTE_BODY_BYTES: usize = 128 * 1024;

const TITLE_ARG: Arg = Arg::value(
    "title",
    "<text>",
    "Short title of this project-visible note (at most 300 UTF-8 bytes).",
);
const BODY_ARG: Arg = Arg::value(
    "body",
    "<markdown>",
    "Markdown review text, at most 128 KiB. Use this or --body-file.",
);
const BODY_FILE_ARG: Arg = Arg::value(
    "body-file",
    "<path>",
    "Read the Markdown review text from one UTF-8 file, at most 128 KiB. Use this or --body.",
);
const ID_ARG: Arg = Arg::value(
    "id",
    "<note-id>",
    "Mint this stable record id. Reuse it after an uncertain response; a second create is refused as existing.",
);

const NOTE_INVALID: Refusal = Refusal {
    code: "note_invalid",
    when: "the title or id is empty or over its bound, the Markdown body is over 128 KiB, or both body forms were given",
    remedy: "give a short title and at most one body source; use a stable id of at most 256 UTF-8 bytes without whitespace, controls or slash",
};
const NOTE_BODY_UNREADABLE: Refusal = Refusal {
    code: "note_body_unreadable",
    when: "--body-file cannot be read as UTF-8 text within 128 KiB",
    remedy: "pass a readable UTF-8 Markdown file no larger than 128 KiB, or use --body",
};
const NOTE_EXISTS: Refusal = Refusal {
    code: "note_exists",
    when: "the same --id already names a Project Work context record",
    remedy: "the create may already have landed; read that id with `ds pm record read` before choosing a new id for different work",
};

pub static CREATE: Command = Command {
    id: "pm.note.create",
    path: &["pm", "note", "create"],
    contract: 1,
    summary: "Create a project-visible Markdown note for human review or editing.",
    purpose: "\
Files a quick project note in Project Work, with its title and concise Markdown \
body. The signed-in native credential must be allowed to contribute to the \
explicitly named project. This creates a record with category project_note; \
it does not send correspondence. An existing report is linked afterward with \
`ds assets attach --record <note-id>`; confirm that link by reading the asset \
or record. Reuse --id after an uncertain response: duplicate creation is \
refused without a second note. Headless; no window.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        TITLE_ARG.required(),
        BODY_ARG,
        BODY_FILE_ARG,
        ID_ARG,
        LANE_ARG,
        crate::PROJECT_ARG,
    ],
    output: "The explicit `project` and created `note` receipt: `id`, `version`, `title` and `category`. The note is project-visible; the receipt does not claim an asset link.",
    examples: &[Example {
        command: "ds pm note create --project <exact-id> --id review-poles-2026-09-30 --title \"Pole modification review\" --body-file review-note.md --yes --output json",
        note: "Read .data.note.id and .data.note.version. To link an existing HTML report, use `ds assets ingest` then `ds assets attach --asset <asset-id> --record <note-id> --yes` and verify by readback.",
        runnable: false,
    }],
    refusals: &crate::correspondence_refusals::<24>(&[
        NOTE_INVALID,
        NOTE_BODY_UNREADABLE,
        NOTE_EXISTS,
    ]),
    reference: Some("docs/reference/pm.md"),
    search: &[
        "project note",
        "modification note",
        "design note",
        "human memo",
        "review record",
        "work journal",
    ],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid(NOTE_INVALID.code, message).remedy(NOTE_INVALID.remedy)
}

fn body(inputs: &Inputs) -> Result<String, Failure> {
    match (inputs.value("body"), inputs.value("body-file")) {
        (Some(_), Some(_)) => Err(invalid("give only one of --body and --body-file")),
        (Some(body), None) => Ok(body.to_owned()),
        (None, None) => Ok(String::new()),
        (None, Some(path)) => {
            let unreadable = |message: String| {
                Failure::invalid(NOTE_BODY_UNREADABLE.code, message)
                    .remedy(NOTE_BODY_UNREADABLE.remedy)
                    .detail(json!({"path": path}))
            };
            let metadata =
                std::fs::metadata(path).map_err(|error| unreadable(error.to_string()))?;
            if metadata.len() > MAX_NOTE_BODY_BYTES as u64 {
                return Err(unreadable(format!(
                    "the file is {} bytes; the note body bound is {MAX_NOTE_BODY_BYTES}",
                    metadata.len()
                )));
            }
            let bytes = std::fs::read(path).map_err(|error| unreadable(error.to_string()))?;
            if bytes.len() > MAX_NOTE_BODY_BYTES {
                return Err(unreadable(format!(
                    "the file is {} bytes; the note body bound is {MAX_NOTE_BODY_BYTES}",
                    bytes.len()
                )));
            }
            String::from_utf8(bytes).map_err(|_| unreadable("the file is not UTF-8 text".into()))
        }
    }
}

fn receipt(project: &str, response: &Value) -> Result<Value, Failure> {
    let item = &response["item"];
    let (Some(id), Some(version), Some(title)) = (
        item["id"].as_str(),
        item["version"].as_i64().filter(|version| *version > 0),
        item["data"]["subject"].as_str(),
    ) else {
        return Err(Failure::internal(
            crate::PLAN_UNREADABLE.code,
            "the created project note did not return an id, version and title",
        )
        .remedy(crate::PLAN_UNREADABLE.remedy));
    };
    if item["data"]["category"] != "project_note" || item["data"]["record_type"] != "quick" {
        return Err(Failure::internal(
            crate::PLAN_UNREADABLE.code,
            "the created record did not return as a quick project note",
        )
        .remedy(crate::PLAN_UNREADABLE.remedy));
    }
    Ok(json!({
        "project": project,
        "note": {"id": id, "version": version, "title": title, "category": "project_note"}
    }))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let body = body(inputs)?;
    let project = inputs.require("project")?;
    let command = PMCommand::ProjectNoteCreate {
        id: inputs.value("id").map(str::to_owned),
        title: inputs.require("title")?.to_owned(),
        body,
    };
    command
        .validate()
        .map_err(|error| invalid(error.to_string()))?;
    let id = inputs.value("id");
    let report = ds_cli_auth::project_management_for_project(
        inputs.value("lane").unwrap_or("stable"),
        project,
        &command,
    )
    .map_err(|failure| {
        if failure.code() == crate::PM_REFUSED.code
            && failure
                .detail_value()
                .and_then(|detail| detail["service_code"].as_str())
                == Some("pm_context_id_conflict")
        {
            Failure::conflict(NOTE_EXISTS.code, "the project note id already exists")
                .detail(json!({"id": id, "project": project}))
                .remedy(NOTE_EXISTS.remedy)
        } else {
            failure
        }
    })?;
    receipt(report.project_id(), report.result())
}

pub fn render(data: &Value) -> String {
    format!(
        "created project note {} (version {}) in {}\n  {}\n",
        data["note"]["id"].as_str().unwrap_or("?"),
        data["note"]["version"].as_i64().unwrap_or(0),
        data["project"].as_str().unwrap_or("?"),
        data["note"]["title"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_note_receipt_requires_server_id_version_and_kind() {
        let response = json!({"item": {
            "id": "review-42", "version": 1,
            "data": {"category": "project_note", "record_type": "quick", "subject": "Pole review"}
        }});
        assert_eq!(
            receipt("project-a", &response).expect("created note"),
            json!({"project": "project-a", "note": {
                "id": "review-42", "version": 1, "title": "Pole review", "category": "project_note"
            }})
        );
        assert!(receipt("project-a", &json!({"item": {"id": "review-42"}})).is_err());
        assert!(receipt("project-a", &json!({"item": {
            "id": "review-42", "version": 1,
            "data": {"category": "communication", "record_type": "quick", "subject": "Pole review"}
        }})).is_err());
    }
}
