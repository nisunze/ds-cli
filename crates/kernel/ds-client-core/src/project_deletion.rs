//! The governed whole-project delete, headlessly.
//!
//! ds-brain owns the decision (docs/contracts/project-delete.md): an owner-only
//! (`platform.admin`) plan pinned by digest, bounded export pages, one archive
//! upload, a verified receipt, and an apply that refuses without that receipt.
//! **This module is the door.** It sends a closed set of actions on
//! `POST /api/v1/projects`, writes only the keys the handler reads, and
//! returns the server's data. The archive itself is built by the host.
//!
//! The path is written here rather than declared in the client profile, like
//! `project_properties` beside it: declaring a route there would bump
//! `CLIENT_PROFILE_SCHEMA` and strand every installed `ds`.

use serde_json::{json, Value};

use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    ProjectDeletionCall, ServiceRefusal,
};

/// The route every action is sent to.
pub const PATH: &str = ds_gateway_client::Operation::ProjectsAction.path_template();
/// One export page carries at most ~4 MiB of native documents; a plan carries
/// bounded groups and at most a thousand retained paths.
pub const MAX_RESPONSE_BYTES: usize = 24 * 1024 * 1024;
/// An apply works within a 75-second server budget behind a 120-second gateway.
pub const TIMEOUT_SECONDS: u64 = 150;

/// The export sources an archive builder pages through.
pub const SOURCES: [&str; 4] = ["firestore", "trash", "messaging", "objects"];

/// One governed project-delete action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Plan {
        project_id: String,
    },
    Export {
        project_id: String,
        source: String,
        cursor: Option<String>,
    },
    ArchiveOpen {
        project_id: String,
        plan_digest: String,
        size: u64,
        sha256: String,
        crc32c: String,
        index_sha256: String,
    },
    ArchiveRead {
        project_id: String,
        plan_digest: String,
    },
    ArchiveReceipt {
        project_id: String,
        plan_digest: String,
        generation: i64,
        sha256: String,
    },
    Apply {
        project_id: String,
        plan_digest: String,
    },
}

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

fn project_id_ok(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A plan digest as ds-brain prints it: `sha256:` and 64 lowercase hex.
pub fn digest_ok(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(hex64)
}

impl Command {
    pub const fn action(&self) -> &'static str {
        match self {
            Self::Plan { .. } => "delete_plan",
            Self::Export { .. } => "delete_export",
            Self::ArchiveOpen { .. } => "delete_archive_open",
            Self::ArchiveRead { .. } => "delete_archive_read",
            Self::ArchiveReceipt { .. } => "delete_archive_receipt",
            Self::Apply { .. } => "delete_apply",
        }
    }

    fn project_id(&self) -> &str {
        match self {
            Self::Plan { project_id }
            | Self::Export { project_id, .. }
            | Self::ArchiveOpen { project_id, .. }
            | Self::ArchiveRead { project_id, .. }
            | Self::ArchiveReceipt { project_id, .. }
            | Self::Apply { project_id, .. } => project_id,
        }
    }

    pub fn validate(&self) -> Result<(), ClientError> {
        if !project_id_ok(self.project_id()) {
            return Err(invalid(
                "the project id must be letters, digits and underscores",
            ));
        }
        match self {
            Self::Plan { .. } => Ok(()),
            Self::Export { source, cursor, .. } => {
                if !SOURCES.contains(&source.as_str()) {
                    return Err(invalid("the export source is not one of the four sources"));
                }
                if cursor.as_ref().is_some_and(|c| c.len() > 2048) {
                    return Err(invalid("the export cursor is too long"));
                }
                Ok(())
            }
            Self::ArchiveOpen {
                plan_digest,
                size,
                sha256,
                crc32c,
                index_sha256,
                ..
            } => {
                if !digest_ok(plan_digest)
                    || *size == 0
                    || *size > (1 << 53)
                    || !hex64(sha256)
                    || !hex64(index_sha256)
                    || crc32c.len() != 8
                    || !crc32c.ends_with("==")
                {
                    return Err(invalid("the archive declaration is malformed"));
                }
                Ok(())
            }
            Self::ArchiveRead { plan_digest, .. } | Self::Apply { plan_digest, .. } => {
                digest_ok(plan_digest)
                    .then_some(())
                    .ok_or_else(|| invalid("the plan digest must be sha256:<64 hex>"))
            }
            Self::ArchiveReceipt {
                plan_digest,
                generation,
                sha256,
                ..
            } => {
                if !digest_ok(plan_digest) || *generation < 1 || !hex64(sha256) {
                    return Err(invalid("the archive receipt is malformed"));
                }
                Ok(())
            }
        }
    }

    /// The exact `{action, data}` body the handler reads.
    pub fn payload(&self) -> Value {
        let data = match self {
            Self::Plan { project_id } => json!({ "project_id": project_id }),
            Self::Export {
                project_id,
                source,
                cursor,
            } => {
                let mut data = json!({ "project_id": project_id, "source": source });
                if let Some(cursor) = cursor.as_ref().filter(|c| !c.is_empty()) {
                    data["cursor"] = json!(cursor);
                }
                data
            }
            Self::ArchiveOpen {
                project_id,
                plan_digest,
                size,
                sha256,
                crc32c,
                index_sha256,
            } => json!({
                "project_id": project_id, "plan_digest": plan_digest, "size": size,
                "sha256": sha256, "crc32c": crc32c, "index_sha256": index_sha256,
            }),
            Self::ArchiveRead {
                project_id,
                plan_digest,
            } => json!({ "project_id": project_id, "plan_digest": plan_digest }),
            Self::ArchiveReceipt {
                project_id,
                plan_digest,
                generation,
                sha256,
            } => json!({
                "project_id": project_id, "plan_digest": plan_digest,
                "generation": generation, "sha256": sha256,
            }),
            Self::Apply {
                project_id,
                plan_digest,
            } => json!({
                "project_id": project_id, "plan_digest": plan_digest, "confirm_delete": true,
            }),
        };
        json!({ "action": self.action(), "data": data })
    }
}

/// The server's refusal, carried with its own code and sentence: every
/// `project_delete_*` code names its remedy in the command's help.
pub(crate) fn refusal(status: u16, body: &[u8]) -> ClientError {
    let wire: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let error = &wire["error"];
    let kind = match status {
        400 | 409 => ErrorKind::InvalidInput,
        401 | 403 => ErrorKind::AuthenticationRejected,
        404 if error["code"]
            .as_str()
            .is_some_and(|c| c.starts_with("project_delete_")) =>
        {
            ErrorKind::ResourceNotFound
        }
        404 => ErrorKind::RouteUnavailable,
        _ => ErrorKind::Transient,
    };
    ClientError::with_service_refusal(
        kind,
        "the project delete was refused",
        ServiceRefusal::with_details(
            status,
            error["code"].as_str(),
            error["message"].as_str(),
            error["details"].as_object(),
        ),
    )
}

/// The `data` of a success envelope.
pub(crate) fn decode(body: &[u8]) -> Result<Value, ClientError> {
    let wire: Value = serde_json::from_slice(body).map_err(|_| {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "the project delete answer is unreadable",
        )
    })?;
    if wire["success"] != json!(true) || !wire["data"].is_object() {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "the project delete answer is not a success envelope",
        ));
    }
    Ok(wire["data"].clone())
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    command: &Command,
) -> Result<Value, ClientError> {
    command.validate()?;
    let response = transport.project_deletion(ProjectDeletionCall {
        profile,
        authority: authority.call(),
        payload: command.payload(),
    })?;
    if response.status != 200 {
        return Err(refusal(response.status, &response.body));
    }
    decode(&response.body)
}

#[cfg(test)]
#[path = "project_deletion_tests.rs"]
mod tests;
