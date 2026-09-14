//! Publish a reviewed MV print through the existing report-artifact authority.
//! The host never receives a storage session or supplies a destination URL.
use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    ReportArtifactCall, ServiceRefusal, UploadBytesCall,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
pub struct Command {
    pub file_name: String,
    pub bytes: Vec<u8>,
    pub layout: String,
    pub paper: String,
    pub orientation: String,
    pub source_receipt: String,
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "invalid MV print declaration or file bytes",
    )
}
fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "MV print attachment response failed scope or integrity verification",
    )
}
fn text(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.trim() == s && !s.chars().any(char::is_control)
}
impl Command {
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        if !text(project, 128)
            || !text(&self.file_name, 200)
            || self.file_name.contains(['/', '\\'])
            || !text(&self.layout, 120)
            || !matches!(self.paper.as_str(), "A0" | "A1" | "A2" | "A3" | "A4")
            || !matches!(self.orientation.as_str(), "portrait" | "landscape")
            || self.bytes.len() > 128 * 1024 * 1024
            || self.bytes.is_empty()
            || (!self.source_receipt.is_empty()
                && (self.source_receipt.len() != 64
                    || !self.source_receipt.bytes().all(|b| b.is_ascii_hexdigit())))
        {
            return Err(invalid());
        }
        if !(self.file_name.ends_with(".pdf") && self.bytes.starts_with(b"%PDF-"))
            && !(self.file_name.ends_with(".png") && self.bytes.starts_with(b"\x89PNG\r\n\x1a\n"))
        {
            return Err(invalid());
        }
        Ok(())
    }
}
// Preserve bounded route diagnostics: a missing endpoint is not a bad login.
fn refusal(status: u16, body: &[u8]) -> ClientError {
    let value: Value = if body.len() <= 1024 * 1024 {
        serde_json::from_slice(body).unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let kind = match status {
        401 | 403 => ErrorKind::AuthenticationRejected,
        400 | 409 | 413 | 422 | 423 => ErrorKind::InvalidInput,
        408 | 425 | 429 | 500..=599 => ErrorKind::Transient,
        _ => ErrorKind::UnreadableResponse,
    };
    ClientError::with_service_refusal(
        kind,
        "MV print attachment refused",
        ServiceRefusal::new(
            status,
            value["error"]["code"].as_str(),
            value["error"]["message"]
                .as_str()
                .or_else(|| value["message"].as_str()),
        ),
    )
}
fn call<T: AuthorizedTransport>(
    profile: &ClientProfile,
    t: &mut T,
    a: Authority<'_>,
    payload: Value,
) -> Result<Value, ClientError> {
    let response = t.report_artifact(ReportArtifactCall {
        profile,
        authority: a.call(),
        payload,
    })?;
    if response.status != 200 {
        return Err(refusal(response.status, &response.body));
    }
    if response.body.len() > 1024 * 1024 {
        return Err(unreadable());
    }
    let response: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    if response["success"] != true || !response["data"].is_object() {
        return Err(unreadable());
    }
    Ok(response["data"].clone())
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    t: &mut T,
    a: Authority<'_>,
    project: &str,
    c: &Command,
) -> Result<Value, ClientError> {
    c.validate(project)?;
    let sha = format!("{:x}", Sha256::digest(&c.bytes));
    let content = if c.file_name.ends_with(".pdf") {
        "application/pdf"
    } else {
        "image/png"
    };
    let mut declaration = json!({"action":"upload_start","project_id":project,"scope":"mv","transformer":"mv_data","file_name":c.file_name,"size_bytes":c.bytes.len(),"content_type":content,"sha256":sha,"map_family":"mv-map","layout_name":c.layout,"paper_size":c.paper,"orientation":c.orientation,"page_role":"sheet","source_receipt_sha256":c.source_receipt});
    let start = call(profile, t, a, declaration.clone())?;
    let uri = start["session_uri"].as_str().ok_or_else(unreadable)?;
    let url = url::Url::parse(uri).map_err(|_| unreadable())?;
    if uri.len() > 8192
        || url.scheme() != "https"
        || url.host_str() != Some("storage.googleapis.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !url.path().starts_with("/upload/storage/v1/b/")
        || !url.path().ends_with("/o")
        || !url
            .query_pairs()
            .any(|(k, v)| k == "upload_id" && !v.is_empty())
    {
        return Err(unreadable());
    }
    let blob = start["blob_path"]
        .as_str()
        .filter(|s| {
            text(s, 2048) && !s.starts_with('/') && !s.split('/').any(|p| p == ".." || p == ".")
        })
        .ok_or_else(unreadable)?;
    let mut reader = std::io::Cursor::new(&c.bytes);
    crate::project_data::uploaded(t.upload_bytes(UploadBytesCall {
        uri,
        size: c.bytes.len() as u64,
        reader: &mut reader,
    })?)?;
    declaration["action"] = json!("attach");
    declaration["blob_path"] = json!(blob);
    let attached = call(profile, t, a, declaration)?;
    if attached["project_id"] != project
        || attached["scope"] != "mv"
        || attached["transformer"] != "mv_data"
        || attached["file_name"] != c.file_name
        || attached["sha256"] != sha
        || attached["map_family"] != "mv-map"
    {
        return Err(unreadable());
    }
    let path = attached["gcs_path"].as_str().ok_or_else(unreadable)?;
    if !path.starts_with("gs://") || !path.ends_with(&format!("/{blob}")) {
        return Err(unreadable());
    }
    Ok(attached)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_routes_are_not_authentication_failures_and_status_survives() {
        let missing = refusal(404, b"not found");
        assert_eq!(missing.kind(), ErrorKind::UnreadableResponse);
        assert_eq!(missing.service_refusal().unwrap().status(), 404);
        let denied = refusal(
            403,
            br#"{"error":{"code":"INSUFFICIENT_PERMISSIONS","message":"Requires design.report"}}"#,
        );
        assert_eq!(denied.kind(), ErrorKind::AuthenticationRejected);
        assert_eq!(
            denied.service_refusal().unwrap().message(),
            Some("Requires design.report")
        );
        assert_eq!(refusal(422, b"{}").kind(), ErrorKind::InvalidInput);
        assert_eq!(refusal(503, b"{}").kind(), ErrorKind::Transient);
    }
    #[test]
    fn rejects_disguised_files_and_unbounded_metadata_before_network() {
        let mut c = Command {
            file_name: "map.pdf".into(),
            bytes: b"%PDF-1.7".to_vec(),
            layout: "District".into(),
            paper: "A3".into(),
            orientation: "portrait".into(),
            source_receipt: String::new(),
        };
        assert!(c.validate("project").is_ok());
        c.file_name = "map.png".into();
        assert!(c.validate("project").is_err());
        c.file_name = "../map.pdf".into();
        assert!(c.validate("project").is_err());
    }
}
