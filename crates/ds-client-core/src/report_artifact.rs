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
    /// `sheet`, `atlas` or `joined`, always explicit. The service files each
    /// role in its own slot, so a multi-page `joined` plan/profile PDF never
    /// replaces the single sheet of the same paper. A PNG is only a sheet.
    pub page_role: String,
    pub source_receipt: String,
}

pub struct RemoveCommand {
    pub scope: String,
    pub transformer: String,
    pub file_name: String,
    pub gcs_path: String,
    pub sha256: String,
}
impl RemoveCommand {
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        let valid_target = match self.scope.as_str() {
            "mv" => self.transformer == "mv_data",
            "combined" => self.transformer == "combined_transformer",
            "transformer" => !matches!(
                self.transformer.as_str(),
                "mv_data" | "combined_transformer"
            ),
            _ => false,
        };
        if !text(project, 128)
            || project.contains(['/', '\\'])
            || !valid_target
            || !text(&self.transformer, 160)
            || self.transformer.contains(['/', '\\'])
            || !text(&self.file_name, 240)
            || self.file_name.contains(['/', '\\'])
            || !text(&self.gcs_path, 2048)
            || !self.gcs_path.starts_with("gs://")
            || self.sha256.len() != 64
            || !self
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "select one exact published print filename, scope, gcs_path and sha256",
            ));
        }
        Ok(())
    }
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
            || !matches!(self.page_role.as_str(), "sheet" | "atlas" | "joined")
            || self.bytes.len() > 128 * 1024 * 1024
            || self.bytes.is_empty()
            || (!self.source_receipt.is_empty()
                && (self.source_receipt.len() != 64
                    || !self.source_receipt.bytes().all(|b| b.is_ascii_hexdigit())))
        {
            return Err(invalid());
        }
        if !(self.file_name.ends_with(".pdf") && self.bytes.starts_with(b"%PDF-"))
            && !(self.file_name.ends_with(".png")
                && self.bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                && self.page_role == "sheet")
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
pub(crate) fn remove<T: AuthorizedTransport>(
    profile: &ClientProfile,
    t: &mut T,
    a: Authority<'_>,
    project: &str,
    c: &RemoveCommand,
) -> Result<Value, ClientError> {
    c.validate(project)?;
    let result = call(
        profile,
        t,
        a,
        json!({"action":"remove", "project_id":project, "scope":c.scope, "transformer":c.transformer, "file_name":c.file_name, "blob_path":c.gcs_path, "sha256":c.sha256}),
    )?;
    checked_removal_result(project, c, result)
}
fn checked_removal_result(
    project: &str,
    c: &RemoveCommand,
    result: Value,
) -> Result<Value, ClientError> {
    if result["project_id"] != project
        || result["scope"] != c.scope
        || result["transformer"] != c.transformer
        || result["file_name"] != c.file_name
        || result["gcs_path"] != c.gcs_path
        || result["sha256"] != c.sha256
        || result["bytes_deleted"] != false
        || result["removed"].as_u64().is_none()
    {
        return Err(unreadable());
    }
    Ok(result)
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
    let start = call(profile, t, a, declaration(project, c, &sha, None))?;
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
    let attached = call(profile, t, a, declaration(project, c, &sha, Some(blob)))?;
    checked_attach_result(project, c, &sha, blob, attached)
}
// Both calls carry one declaration. The service binds it to the issued object
// at upload_start and refuses an attach that differs from it.
fn declaration(project: &str, c: &Command, sha: &str, blob: Option<&str>) -> Value {
    let content = if c.file_name.ends_with(".pdf") {
        "application/pdf"
    } else {
        "image/png"
    };
    let mut declaration = json!({"action":"upload_start","project_id":project,"scope":"mv","transformer":"mv_data","file_name":c.file_name,"size_bytes":c.bytes.len(),"content_type":content,"sha256":sha,"map_family":"mv-map","layout_name":c.layout,"paper_size":c.paper,"orientation":c.orientation,"page_role":c.page_role,"source_receipt_sha256":c.source_receipt});
    if let Some(blob) = blob {
        declaration["action"] = json!("attach");
        declaration["blob_path"] = json!(blob);
    }
    declaration
}
// The service echoes identity and digest. Page role, paper, orientation,
// layout and size are bound through the issued object's metadata instead;
// any of them the service does echo must agree with the declaration.
fn checked_attach_result(
    project: &str,
    c: &Command,
    sha: &str,
    blob: &str,
    attached: Value,
) -> Result<Value, ClientError> {
    let echoed = |key: &str, declared: Value| attached.get(key).is_none_or(|v| *v == declared);
    if attached["project_id"] != project
        || attached["scope"] != "mv"
        || attached["transformer"] != "mv_data"
        || attached["file_name"] != c.file_name
        || attached["sha256"] != sha
        || attached["map_family"] != "mv-map"
        || !echoed("page_role", json!(c.page_role))
        || !echoed("paper_size", json!(c.paper))
        || !echoed("orientation", json!(c.orientation))
        || !echoed("layout_name", json!(c.layout))
        || !echoed("size_bytes", json!(c.bytes.len()))
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
            page_role: "sheet".into(),
            source_receipt: String::new(),
        };
        assert!(c.validate("project").is_ok());
        c.file_name = "map.png".into();
        assert!(c.validate("project").is_err());
        c.file_name = "../map.pdf".into();
        assert!(c.validate("project").is_err());
    }
    fn joined(bytes: Vec<u8>) -> Command {
        Command {
            file_name: "mv-plan-profile.a3.pdf".into(),
            bytes,
            layout: "MV plan and profile".into(),
            paper: "A3".into(),
            orientation: "landscape".into(),
            page_role: "joined".into(),
            source_receipt: String::new(),
        }
    }
    const BLOB: &str =
        "report-artifacts/v1/projects/p-1/prints/mv/mv_data/r/mv-plan-profile.a3.pdf";
    #[test]
    fn page_role_is_explicit_and_closed_and_only_a_pdf_holds_several_pages() {
        let mut c = joined(b"%PDF-1.7".to_vec());
        assert!(c.validate("p-1").is_ok());
        c.page_role = "atlas".into();
        assert!(c.validate("p-1").is_ok());
        for role in ["", "Joined", " sheet", "book"] {
            c.page_role = role.into();
            assert_eq!(
                c.validate("p-1").unwrap_err().kind(),
                ErrorKind::InvalidInput,
                "{role:?}"
            );
        }
        c.file_name = "mv.a3.png".into();
        c.bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        for role in ["atlas", "joined"] {
            c.page_role = role.into();
            assert!(c.validate("p-1").is_err(), "{role}");
        }
        c.page_role = "sheet".into();
        assert!(c.validate("p-1").is_ok());
    }
    #[test]
    fn a_joined_pdf_is_declared_alike_on_upload_start_and_attach() {
        // Larger than a 32 MiB map asset; an in-memory buffer, never hashed.
        let mut bytes = vec![0; 40 << 20];
        bytes[..5].copy_from_slice(b"%PDF-");
        let c = joined(bytes);
        assert!(c.validate("p-1").is_ok());
        let sha = "b".repeat(64);
        let start = declaration("p-1", &c, &sha, None);
        assert_eq!(start["action"], "upload_start");
        assert_eq!(start["page_role"], "joined");
        assert_eq!(start["size_bytes"], 40 << 20);
        assert_eq!(start["content_type"], "application/pdf");
        assert!(start.get("blob_path").is_none());
        let attach = declaration("p-1", &c, &sha, Some(BLOB));
        assert_eq!(attach["action"], "attach");
        assert_eq!(attach["blob_path"], BLOB);
        let mut restarted = attach;
        restarted["action"] = json!("upload_start");
        restarted.as_object_mut().unwrap().remove("blob_path");
        assert_eq!(restarted, start);

        // An explicit sheet sends exactly the declaration it always did.
        let mut sheet = joined(b"%PDF-1.7".to_vec());
        sheet.page_role = "sheet".into();
        assert_eq!(
            declaration("p-1", &sheet, &sha, None),
            json!({"action":"upload_start","project_id":"p-1","scope":"mv","transformer":"mv_data","file_name":"mv-plan-profile.a3.pdf","size_bytes":8,"content_type":"application/pdf","sha256":sha,"map_family":"mv-map","layout_name":"MV plan and profile","paper_size":"A3","orientation":"landscape","page_role":"sheet","source_receipt_sha256":""})
        );
    }
    #[test]
    fn a_joined_receipt_must_agree_with_everything_the_service_echoes() {
        let c = joined(b"%PDF-1.7".to_vec());
        let sha = "c".repeat(64);
        // The service's attach receipt: identity and digest only.
        let receipt = json!({"project_id":"p-1","scope":"mv","transformer":"mv_data","file_name":"mv-plan-profile.a3.pdf","sha256":sha,"gcs_path":format!("gs://bucket/{BLOB}"),"map_family":"mv-map"});
        assert_eq!(
            checked_attach_result("p-1", &c, &sha, BLOB, receipt.clone()).unwrap(),
            receipt
        );
        let mut echoed = receipt;
        for (key, value) in [
            ("page_role", json!("joined")),
            ("paper_size", json!("A3")),
            ("orientation", json!("landscape")),
            ("layout_name", json!("MV plan and profile")),
            ("size_bytes", json!(8)),
        ] {
            echoed[key] = value;
        }
        assert!(checked_attach_result("p-1", &c, &sha, BLOB, echoed.clone()).is_ok());
        for (key, value) in [
            ("page_role", json!("sheet")),
            ("page_role", Value::Null),
            ("paper_size", json!("A1")),
            ("orientation", json!("portrait")),
            ("layout_name", json!("District")),
            ("size_bytes", json!(9)),
            ("project_id", json!("other")),
            ("sha256", json!("d".repeat(64))),
            (
                "gcs_path",
                json!("gs://bucket/elsewhere/mv-plan-profile.a3.pdf"),
            ),
        ] {
            let mut wrong = echoed.clone();
            wrong[key] = value;
            assert_eq!(
                checked_attach_result("p-1", &c, &sha, BLOB, wrong)
                    .unwrap_err()
                    .kind(),
                ErrorKind::UnreadableResponse,
                "{key}"
            );
        }
    }
    #[test]
    fn removal_requires_exact_identity_and_a_reference_only_receipt() {
        let command = RemoveCommand {
            scope: "mv".into(),
            transformer: "mv_data".into(),
            file_name: "mv.a3.pdf".into(),
            gcs_path: "gs://bucket/exact/mv.a3.pdf".into(),
            sha256: "a".repeat(64),
        };
        assert!(command.validate("p-1").is_ok());
        let receipt = json!({"project_id":"p-1","scope":"mv","transformer":"mv_data","file_name":"mv.a3.pdf","gcs_path":"gs://bucket/exact/mv.a3.pdf","sha256":"a".repeat(64),"removed":1,"bytes_deleted":false});
        assert_eq!(
            checked_removal_result("p-1", &command, receipt.clone()).unwrap(),
            receipt
        );
        let mut wrong = receipt.clone();
        wrong["bytes_deleted"] = json!(true);
        assert_eq!(
            checked_removal_result("p-1", &command, wrong)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
        let mut wrong = receipt;
        wrong["project_id"] = json!("other");
        assert_eq!(
            checked_removal_result("p-1", &command, wrong)
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
        let invalid = RemoveCommand {
            transformer: "other".into(),
            ..command
        };
        assert_eq!(
            invalid.validate("p-1").unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
    }
}
