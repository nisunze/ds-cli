//! Project GIS upload lifecycle. No caller-provided route, storage URL or bearer.
use crate::{ClientError, ErrorKind, TransportResponse};
use serde_json::{json, Value};
use std::io::Read;
pub const MAX_UPLOAD_BYTES: u64 = 1024 * 1024 * 1024;
pub enum ProjectDataCommand<'a> {
    List,
    Remove {
        upload_id: &'a str,
    },
    Upload {
        file_name: &'a str,
        size: u64,
        reader: &'a mut dyn Read,
    },
}
#[derive(Clone)]
pub struct ProjectDataReceipt {
    pub(crate) data: Value,
}
impl ProjectDataReceipt {
    pub fn data(&self) -> &Value {
        &self.data
    }
}
pub(crate) fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "invalid project GIS file or upload id",
    )
}
pub(crate) fn bad() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "invalid project GIS response",
    )
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 500
        && s.trim() == s
        && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
}
impl ProjectDataCommand<'_> {
    pub(crate) fn validate(&self) -> Result<(), ClientError> {
        match self {
            Self::List => Ok(()),
            Self::Remove { upload_id } if identifier(upload_id) => Ok(()),
            Self::Upload {
                file_name, size, ..
            } if identifier(file_name) && *size > 0 && *size <= MAX_UPLOAD_BYTES => Ok(()),
            _ => Err(invalid()),
        }
    }
}
pub(crate) fn list(data: Value) -> Result<Value, ClientError> {
    let rows = if data["uploads"].is_null() && data.get("uploads").is_some() {
        vec![]
    } else {
        data["uploads"].as_array().ok_or_else(bad)?.clone()
    };
    let mut ids = std::collections::HashSet::new();
    let mut result = vec![];
    for row in &rows {
        let id = row["upload_id"]
            .as_str()
            .filter(|s| identifier(s))
            .ok_or_else(bad)?;
        if !ids.insert(id) {
            return Err(bad());
        }
        if result.len() < 500 {
            result.push(json!({"upload_id":id,"file_name":row["file_name"],"tile_status":row["tile_status"],"total_features":row["total_features"],"owns_storage":row["owns_storage"],"created_at":row["created_at"]}));
        }
    }
    Ok(json!({"uploads":result,"total":rows.len(),"more":rows.len()>500}))
}
pub(crate) struct IssuedUpload {
    pub uri: String,
    pub blob: String,
    pub id: String,
}
pub(crate) fn issued(data: &Value, project: &str) -> Result<IssuedUpload, ClientError> {
    let uri = data["session_uri"]
        .as_str()
        .filter(|s| s.len() <= 8192)
        .ok_or_else(bad)?;
    let url = url::Url::parse(uri).map_err(|_| bad())?;
    let bucket = data["bucket"]
        .as_str()
        .filter(|s| identifier(s) && s.len() <= 222)
        .ok_or_else(bad)?;
    let id = data["upload_id"]
        .as_str()
        .filter(|s| identifier(s))
        .ok_or_else(bad)?;
    let blob = data["blob_path"]
        .as_str()
        .filter(|s| s.len() <= 2048)
        .ok_or_else(bad)?;
    let prefix = format!("projects/{project}/project_data/{id}/raw/");
    if url.scheme() != "https"
        || url.host_str() != Some("storage.googleapis.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || url.path() != format!("/upload/storage/v1/b/{bucket}/o")
        || !url
            .query_pairs()
            .any(|(k, v)| k == "upload_id" && !v.is_empty())
        || !blob.starts_with(&prefix)
        || blob.split('/').any(|p| p == ".." || p == ".")
    {
        return Err(bad());
    }
    Ok(IssuedUpload {
        uri: uri.into(),
        blob: blob.into(),
        id: id.into(),
    })
}
/// Only the authenticated response decoder can construct this call. The host
/// sends bytes without DS credentials and refuses redirects; URLs never enter receipts.
pub struct UploadBytesCall<'a> {
    pub(crate) uri: &'a str,
    pub(crate) size: u64,
    pub(crate) reader: &'a mut dyn Read,
}
impl UploadBytesCall<'_> {
    pub fn uri(&self) -> &str {
        self.uri
    }
    pub fn size(&self) -> u64 {
        self.size
    }
    pub fn reader(&mut self) -> &mut dyn Read {
        self.reader
    }
    pub fn response_limit(&self) -> usize {
        64 * 1024
    }
}
pub(crate) fn uploaded(response: TransportResponse) -> Result<(), ClientError> {
    if !matches!(response.status, 200 | 201) || response.body.len() > 64 * 1024 {
        return Err(ClientError::new(
            ErrorKind::Transient,
            "GIS byte transfer did not complete; the file was not registered",
        ));
    }
    Ok(())
}
