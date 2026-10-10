//! Explicit scoped copies and selected ingested EML manifests. No acquisition.
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_client_core::{
    project_correspondence::Action,
    project_management::documents::{CopyRequest, MAX_INPUT_BYTES, MailThreadRequest},
};
use serde_json::{Value, json};
const INPUT: Arg = Arg::value(
    "input",
    "<json-file>",
    "Closed selected source/destination or EML pins JSON (at most 64 KiB).",
)
.required();
const ID: Arg = Arg::value(
    "id",
    "<command-id>",
    "Stable command id; exact retries preserve all original pins and intent.",
)
.required();
const REVISION: Arg = Arg::value(
    "base-revision",
    "<revision>",
    "Reviewed project graph revision; required for a task destination, use 0 for a note-only copy.",
)
.required();
const DRY_RUN: Arg = Arg::switch(
    "dry-run",
    "Validate the selected source and destination without applying a new copy or record.",
);
const INVALID: Refusal = Refusal {
    code: "document_request_invalid",
    when: "selected source pins, destination or explicit item count are invalid",
    remedy: "use the closed JSON contract, exact source versions and hashes, and explicit attachment inclusion or exclusion",
};
const REFUSALS: &[Refusal] = &crate::correspondence_refusals::<25>(&[
    INVALID,
    crate::checklist_notes::UNREADABLE,
    crate::INVALID_NUMBER,
    crate::task::proposals::INVALID_COMMAND_ID,
]);
pub static COPY: Command = Command {
    id: "pm.document.copy",
    path: &["pm", "document", "copy"],
    contract: 1,
    summary: "Copy one pinned task or note without changing its source.",
    purpose: "Copies one selected task, project note or account-private note to an explicit new task/note id through the existing Project Work owner. Input declares source_kind, source_id, source_version, target_kind, target_id and selected attachment_ids or exclude_attachments. The owner preserves portable body, checklist and geometry while revalidating privacy, evidence and source version atomically. Private/project file copies require prior admitted ingest. Source stays unchanged. No catalogue or source read precedes this request. Preview is read-only; a replay may return its original applied receipt.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        INPUT,
        ID,
        REVISION,
        DRY_RUN,
        crate::LANE_ARG,
        crate::PROJECT_ARG,
    ],
    output: "Source/destination kinds, ids and pinned source version, source_retained=true, explicit copied/excluded attachment ids, applied and the owner's new task/note receipt.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/pm.md"),
    search: &["copy", "task", "note", "private", "provenance"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static MAIL: Command = Command {
    id: "pm.mail-thread.ingest",
    path: &["pm", "mail-thread", "ingest"],
    contract: 1,
    summary: "Adopt selected ingested EMLs as one verified correspondence record.",
    purpose: "Submits explicit already-ingested .eml asset_id/version/sha256/generation pins, record_id and exact message_count (1-32). Brain verifies complete immutable MIME and header lineage, privacy and source authority before atomic record creation. Missing metadata or unselected references refuse; no mailbox, URL, private chat or unselected asset is fetched. No client catalogue or asset read precedes the request. Exact replay returns its original actor-private receipt. Preview creates nothing.",
    chapter: Chapter::Project,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[INPUT, ID, DRY_RUN, crate::LANE_ARG, crate::PROJECT_ARG],
    output: "Applied, immutable communication record, complete source-pinned manifest with exact message/part counts and member hashes; no silently truncated success.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/pm.md"),
    search: &["mail", "eml", "thread", "provenance", "ingest"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
fn invalid(error: impl std::fmt::Display) -> Failure {
    Failure::invalid(INVALID.code, error.to_string()).remedy(INVALID.remedy)
}
fn read_input<T: serde::de::DeserializeOwned>(path: &str) -> Result<T, Failure> {
    crate::checklist_notes::bounded_json(path, MAX_INPUT_BYTES).map_err(|failure| {
        if failure.code() == crate::checklist_notes::INVALID.code {
            invalid(failure.message())
        } else {
            failure
        }
    })
}
fn request(inputs: &Inputs, mail: bool) -> Result<Action, Failure> {
    let command_id = crate::task::proposals::command_id(inputs, "document")?;
    let dry_run = inputs.switch("dry-run");
    let action = if mail {
        let request: MailThreadRequest = read_input(inputs.require("input")?)?;
        Action::MailThreadIngest {
            command_id,
            request,
            dry_run,
        }
    } else {
        let request: CopyRequest = read_input(inputs.require("input")?)?;
        Action::CopyDocument {
            command_id,
            base_revision: crate::integer(
                inputs.require("base-revision")?,
                "base-revision",
                0,
                i64::MAX - 1,
            )?,
            request,
            dry_run,
        }
    };
    action.validate().map_err(invalid)?;
    Ok(action)
}
fn execute(inputs: &Inputs, mail: bool) -> Result<Value, Failure> {
    let action = request(inputs, mail)?;
    let report = crate::correspondence(
        inputs.value("lane").unwrap_or("stable"),
        inputs.require("project")?,
        &action,
    )?;
    Ok(
        json!({"project":report.project_id(),"commandId":inputs.require("id")?,"result":report.result()}),
    )
}
pub fn copy(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    execute(i, false)
}
pub fn mail(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    execute(i, true)
}
pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default() + "\n"
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_intent_cannot_override_confirmation_project_or_principal() {
        for value in [
            json!({"source_kind":"task","source_id":"t","source_version":3,"target_kind":"project_note","target_id":"n","dry_run":false}),
            json!({"source_kind":"task","source_id":"t","source_version":3,"target_kind":"project_note","target_id":"n","ds_project":"other"}),
        ] {
            assert!(serde_json::from_value::<CopyRequest>(value).is_err());
        }
        assert!(serde_json::from_value::<MailThreadRequest>(json!({"record_id":"r","message_count":1,"sources":[],"url":"https://example.invalid/mail.eml"})).is_err());
    }
}
