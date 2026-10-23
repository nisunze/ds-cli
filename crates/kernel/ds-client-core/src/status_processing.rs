//! Fixed native transport for the design-intake upload effect.
//!
//! ds-brain only receives the uploaded bytes (`upload_start` and the resumable
//! transfer). Processing an upload is edge compute: `ds design intake upload`
//! runs it on this machine through the Network engine and the governed save.
use crate::{ClientError, ErrorKind, TransportResponse, UploadBytesCall};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Read;

pub const STATUS_PROCESSING_PATH: &str = "/process";
pub const STATUS_PROCESSING_METHOD: &str = "POST";
pub const STATUS_PROCESSING_RESPONSE_LIMIT: usize = 8 * 1024 * 1024;
pub const STATUS_PROCESSING_TIMEOUT_SECONDS: u64 = 900;

#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StatusUploadDomain {
    LvDrafting,
    SketchLv,
    LvProcess,
}

impl StatusUploadDomain {
    pub const fn token(self) -> &'static str {
        match self {
            Self::LvDrafting => "lv_drafting",
            Self::SketchLv => "sketch_lv",
            Self::LvProcess => "lv_process",
        }
    }
}

pub enum StatusProcessingCommand<'a> {
    Upload {
        domain: StatusUploadDomain,
        file_name: &'a str,
        size: u64,
        content_type: &'a str,
        reader: &'a mut dyn Read,
    },
}

#[derive(Clone, Debug)]
pub struct StatusProcessingReceipt {
    data: Value,
}

impl StatusProcessingReceipt {
    pub fn data(&self) -> &Value {
        &self.data
    }
}

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "invalid design-intake response",
    )
}

fn file_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 500
        && value.trim() == value
        && !value
            .chars()
            .any(|character| character.is_control() || matches!(character, '/' | '\\'))
}

fn content_type(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn response(response: TransportResponse) -> Result<Value, ClientError> {
    if response.body.is_empty() || response.body.len() > STATUS_PROCESSING_RESPONSE_LIMIT {
        return Err(unreadable());
    }
    if !(200..300).contains(&response.status) {
        let kind = if matches!(response.status, 408 | 425 | 429 | 500..=599) {
            ErrorKind::Transient
        } else {
            ErrorKind::AuthenticationRejected
        };
        return Err(ClientError::new(kind, "Standard design intake was refused"));
    }
    let wire: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    let value = if wire["success"] == true && wire["data"].is_object() {
        wire["data"].clone()
    } else {
        wire
    };
    if !value.is_object() {
        return Err(unreadable());
    }
    Ok(value)
}

pub(crate) fn run<T: crate::AuthorizedTransport>(
    profile: &crate::ClientProfile,
    transport: &mut T,
    authority: crate::authorized::Authority<'_>,
    project: &str,
    command: StatusProcessingCommand<'_>,
) -> Result<StatusProcessingReceipt, ClientError> {
    crate::authorized::validate_tiles(project)?;
    let data = match command {
        StatusProcessingCommand::Upload {
            domain,
            file_name: name,
            size,
            content_type: media_type,
            reader,
        } => {
            if !file_name(name)
                || size == 0
                || size > crate::MAX_UPLOAD_BYTES
                || !content_type(media_type)
            {
                return Err(invalid("invalid design-intake file"));
            }
            let payload = json!({
                "action":"upload_start", "eds_project_id":project,
                "upload_domain":domain.token(), "file_name":name,
                "size":size, "content_type":media_type
            });
            let issued = response(transport.status_processing(crate::StatusProcessingCall {
                profile,
                payload: &payload,
                authority: authority.call(),
            })?)?;
            let kernel_mode = match domain {
                StatusUploadDomain::LvDrafting => {
                    ds_command_kernel::status_upload::UploadMode::LvDrafting
                }
                StatusUploadDomain::SketchLv => {
                    ds_command_kernel::status_upload::UploadMode::SketchLv
                }
                StatusUploadDomain::LvProcess => {
                    ds_command_kernel::status_upload::UploadMode::LvProcess
                }
            };
            let target = ds_command_kernel::status_upload::validate_upload_target(
                project,
                kernel_mode,
                &issued,
            )
            .map_err(|_| unreadable())?;
            let uploaded = transport.upload_bytes(UploadBytesCall {
                uri: &target.session_uri,
                size,
                reader,
            })?;
            crate::project_data::uploaded(uploaded)?;
            json!({"blob_path":target.blob_path})
        }
    };
    Ok(StatusProcessingReceipt { data })
}
