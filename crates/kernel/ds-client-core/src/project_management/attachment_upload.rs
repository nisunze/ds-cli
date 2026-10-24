//! Core's host for the Kernel attachment upload flow
//! (`ds_command_kernel::project_management::attachment_upload`).
//!
//! It executes the PM action or the byte upload each step names over the
//! authorized native transport, and decides nothing. The browser runs the
//! same flow through WASM with its own host.

use super::{send, AttachmentFile};
use crate::{authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind};
use ds_command_kernel::project_management::attachment_upload::{
    self as flow, Observed, Step, Upload,
};
use serde_json::{json, Map, Value};

/// The Project Work entity a file is attached to.
#[derive(Clone, Copy, Debug)]
pub(super) enum Entity<'a> {
    Task(&'a str),
    Record(&'a str),
}

/// Attach one file to one task or record: the verified ready row, the exact
/// local bytes and SHA-256, and whether an interrupted reservation was
/// completed or detached on the way.
pub(super) fn attach<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    entity: Entity<'_>,
    file: &AttachmentFile,
) -> Result<Value, ClientError> {
    let (linked_entity_type, linked_entity_id) = match entity {
        Entity::Task(id) => ("task", id),
        Entity::Record(id) => ("record", id),
    };
    let mut step = flow::start(Upload {
        project: project.to_owned(),
        linked_entity_type: linked_entity_type.into(),
        linked_entity_id: linked_entity_id.to_owned(),
        principal: authority.email.to_owned(),
        name: file.name().to_owned(),
        content_type: file.content_type().to_owned(),
        size_bytes: file.len(),
        metadata: Map::new(),
    });
    let mut failure = None;
    for _ in 0..flow::MAX_STEPS {
        step = match step {
            Step::Pm { body, continuation } => {
                let observed = match send(profile, transport, authority, body) {
                    Ok(data) => Observed::Pm { data },
                    Err(error) => {
                        failure = Some(error);
                        Observed::PmFailed
                    }
                };
                flow::resume(continuation, observed)
            }
            Step::Upload {
                session_uri,
                size_bytes,
                continuation,
                ..
            } => {
                // The file streams from disk in bounded reads; the second
                // hash taken on the way must name the bytes checked when the
                // attachment was made, or nothing is reported as uploaded.
                let mut reader = file.open()?;
                let sent = transport.upload_bytes(crate::UploadBytesCall {
                    uri: &session_uri,
                    size: size_bytes,
                    reader: &mut reader,
                });
                let same = file.same_bytes(reader.finish());
                let observed = match sent {
                    Ok(_) if !same => return Err(CHANGED),
                    Ok(response) => Observed::Upload {
                        status: response.status,
                    },
                    Err(_) => Observed::UploadFailed { cancelled: false },
                };
                flow::resume(continuation, observed)
            }
            Step::Done {
                row,
                restarted,
                reclaimed,
                unreclaimed,
                ..
            } => {
                return Ok(
                    json!({"item": row, "bytes": file.len(), "sha256": file.sha256(),
                    "content_type": file.content_type(), "restarted": restarted,
                    "reclaimed": reclaimed, "unreclaimed": unreclaimed}),
                );
            }
            Step::HostFailed => return Err(failure.unwrap_or(UNREADABLE)),
            Step::Refused { code, message } => {
                let kind = match code {
                    flow::BYTES_INCOMPLETE | flow::CANCELLED => ErrorKind::Transient,
                    flow::INPUT_INVALID => ErrorKind::InvalidInput,
                    _ => ErrorKind::UnreadableResponse,
                };
                return Err(ClientError::new(kind, message));
            }
        };
    }
    Err(UNREADABLE)
}

/// The file was edited between its check and its upload. The reservation stays
/// uploading, so attaching the file again completes the same row.
const CHANGED: ClientError = ClientError::new(
    ErrorKind::InvalidInput,
    "the file changed after it was checked; attach it again",
);

const UNREADABLE: ClientError = ClientError::new(
    ErrorKind::UnreadableResponse,
    "the attachment upload did not settle within its bounded steps",
);
