//! One headless task creation with verified task-owned attachments.
//! Uses existing closed PM commit/upload routes; no UI or Assets detour.
//! A failed upload returns the committed task and ready receipts, never a
//! claim of atomic file delivery or permission to repeat an ambiguous write.

use super::{project_note_id_valid, send, AttachmentFile, Command, MAX_ATTACHMENT_BYTES};
use crate::{authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind};
use ds_command_kernel::project_management::writes;
use serde_json::{json, Value};

pub const MAX_CREATE_ATTACHMENTS: usize = 10;

/// Validate all local inputs before fetching a graph or creating a task.
pub fn validate_files(files: &[AttachmentFile]) -> Result<(), ClientError> {
    if files.is_empty() || files.len() > MAX_CREATE_ATTACHMENTS {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "task creation accepts 1..10 distinct attachments, at most 100 MiB combined",
        ));
    }
    let mut unique = std::collections::BTreeSet::new();
    let total: usize = files.iter().map(AttachmentFile::len).sum();
    if total > MAX_ATTACHMENT_BYTES
        || files
            .iter()
            .any(|file| !unique.insert((file.name(), file.sha256())))
    {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "task creation accepts 1..10 distinct attachments, at most 100 MiB combined",
        ));
    }
    Ok(())
}

pub(super) fn validate(
    command_id: &str,
    base_revision: i64,
    command: &Value,
    task_id: &str,
    files: &[AttachmentFile],
) -> Result<(), ClientError> {
    validate_files(files)?;
    if !project_note_id_valid(task_id)
        || command["kind"] != "create_task"
        || command["create_task"]["id"].as_str() != Some(task_id)
    {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "attached task creation requires the same exact new task identity",
        ));
    }
    Command::Commit {
        command_id: command_id.into(),
        base_revision,
        command: command.clone(),
    }
    .validate()
}

pub(super) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    payload: Value,
    task_id: &str,
    files: &[AttachmentFile],
) -> Result<Value, ClientError> {
    let data = send(profile, transport, authority, payload)?;
    let result = data
        .get("result")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or(data);
    let mut receipts = Vec::new();
    // A refused or malformed commit must never create upload reservations.
    // ds-brain answers `violations: null` when there are none, which the
    // canonical receipt decoder reads as an empty list.
    let receipt = writes::decode_operation_result(&result);
    if receipt.applied && receipt.committed_revision < 1 {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "task creation receipt lacks its committed revision; read the task before retrying",
        ));
    }
    let violations = &result["violations"];
    if !receipt.applied
        || !receipt.violations.is_empty()
        || !(violations.is_null() || violations.is_array())
    {
        return Ok(
            json!({"result": result, "task_id": task_id, "attachments": receipts,
            "complete": false, "requested_attachment_count": files.len()}),
        );
    }
    for (index, file) in files.iter().enumerate() {
        match super::attachment_upload::attach(
            profile,
            transport,
            authority,
            project,
            super::attachment_upload::Entity::Task(task_id),
            file,
        ) {
            Ok(receipt) => receipts.push(receipt),
            Err(error) => {
                return Ok(json!({"result": result, "task_id": task_id,
                "attachments": receipts, "complete": false, "failed_attachment_index": index,
                "requested_attachment_count": files.len(),
                "failure_kind": format!("{:?}", error.kind())}));
            }
        }
    }
    Ok(
        json!({"result": result, "task_id": task_id, "attachments": receipts,
        "complete": true, "requested_attachment_count": files.len()}),
    )
}
