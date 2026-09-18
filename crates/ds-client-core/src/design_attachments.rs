//! Closed native design-attachment workflow. The server owns storage grants,
//! digest verification and pointer transactions; hosts supply opaque bytes.
use crate::{
    authorized::{self, Authority},
    AuthorizedTransport, ClientError, ClientProfile, DesignAttachmentsCall, ErrorKind,
    ServiceRefusal, TransportResponse,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

pub const MAX_BYTES: usize = 512 * 1024 * 1024;
#[derive(Clone)]
pub struct Object {
    pub kind: String,
    pub id: String,
    pub version: Option<String>,
}
pub enum Command {
    List {
        object: Object,
        archived: bool,
    },
    Publish {
        object: Object,
        attachment: Option<String>,
        file: String,
        label: Option<String>,
        purpose: Option<String>,
        bytes: Vec<u8>,
    },
    Download {
        attachment: String,
        revision: Option<String>,
    },
    Retire {
        attachment: String,
        revision: Option<String>,
        restore: bool,
    },
}
fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}
fn bad() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "attachment response changed project, object, revision or verified byte identity",
    )
}
fn id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && value.trim() == value
        && !value
            .chars()
            .any(|c| c.is_control() || matches!(c, '/' | '\\'))
}
impl Object {
    pub fn validate(&self) -> Result<(), ClientError> {
        if !matches!(self.kind.as_str(), "lv_transformer" | "mv_model") || !id(&self.id) {
            return Err(invalid("invalid attachment object identity"));
        }
        if let Some(version) = &self.version {
            let assigned = version.starts_with('v')
                && ds_command_kernel::design_versions::valid_target(version);
            if (self.kind == "lv_transformer" && !assigned)
                || (self.kind == "mv_model" && (assigned || !id(version)))
            {
                return Err(invalid("LV attachment versions require assigned vN; MV attachments require the exact content revision id. Read design version list/get metadata and use source_revision for MV, never its governance vN."));
            }
        }
        Ok(())
    }
    fn value(&self) -> Value {
        let mut value = json!({"kind":self.kind,"id":self.id});
        if let Some(version) = &self.version {
            value["version_id"] = json!(version);
        }
        value
    }
}
impl Command {
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        if !id(project) {
            return Err(invalid("invalid explicit project identity"));
        }
        match self {
            Self::List { object, .. } => object.validate(),
            Self::Publish {
                object,
                attachment,
                file,
                label,
                purpose,
                bytes,
            } => {
                object.validate()?;
                if attachment.as_ref().is_some_and(|value| !id(value))
                    || !id(file)
                    || file.len() > 240
                    || bytes.is_empty()
                    || bytes.len() > MAX_BYTES
                    || [label, purpose].iter().any(|value| {
                        value.as_ref().is_some_and(|text| {
                            text.is_empty()
                                || text.len() > 240
                                || text.chars().any(char::is_control)
                        })
                    })
                {
                    return Err(invalid(
                        "invalid attachment file, metadata or byte bound (512 MiB)",
                    ));
                }
                Ok(())
            }
            Self::Download {
                attachment,
                revision,
            }
            | Self::Retire {
                attachment,
                revision,
                ..
            } => {
                if id(attachment) && revision.as_ref().is_none_or(|value| id(value)) {
                    Ok(())
                } else {
                    Err(invalid("invalid attachment or revision identity"))
                }
            }
        }
    }
}
pub struct UploadCall<'a> {
    uri: String,
    bytes: &'a [u8],
    digest: String,
}
impl UploadCall<'_> {
    pub fn uri(&self) -> &str {
        &self.uri
    }
    pub fn bytes(&self) -> &[u8] {
        self.bytes
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
}
fn decode(response: TransportResponse) -> Result<Value, ClientError> {
    if !matches!(response.status, 200 | 201) {
        let body: Value = serde_json::from_slice(&response.body).unwrap_or(Value::Null);
        let detail = &body["error"];
        if response.status == 409 {
            return Err(ClientError::with_service_refusal(
                ErrorKind::InvalidInput,
                "attachment pointer conflict; request was not changed or retried",
                ServiceRefusal::new(409, detail["code"].as_str(), detail["message"].as_str()),
            ));
        }
        return Err(authorized::classify_tile_error(response.status));
    }
    if response.body.len() > 8 * 1024 * 1024 {
        return Err(bad());
    }
    let value: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    if value["success"] != true || !value["data"].is_object() {
        return Err(bad());
    }
    Ok(value["data"].clone())
}
fn request<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    payload: Value,
) -> Result<Value, ClientError> {
    decode(transport.design_attachments(DesignAttachmentsCall {
        profile,
        authority: authority.call(),
        payload,
    })?)
}
fn check(
    row: &Value,
    project: &str,
    attachment: Option<&str>,
    object: Option<&Object>,
) -> Result<(), ClientError> {
    if row["project_id"] != project
        || !row["attachment_id"].as_str().is_some_and(id)
        || attachment.is_some_and(|expected| row["attachment_id"] != expected)
        || object.is_some_and(|expected| {
            row["object"]["kind"] != expected.kind || row["object"]["id"] != expected.id
        })
    {
        return Err(bad());
    }
    Ok(())
}
fn current<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    attachment: &str,
) -> Result<Value, ClientError> {
    let data = request(
        profile,
        transport,
        authority,
        json!({"action":"get","project_id":project,"attachment_id":attachment,"include_archived":true}),
    )?;
    check(&data["attachment"], project, Some(attachment), None)?;
    if !data["attachment"]["version"]
        .as_u64()
        .is_some_and(|n| n > 0)
    {
        return Err(bad());
    }
    Ok(data)
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Value, ClientError> {
    command.validate(project)?;
    match command {
        Command::List { object, archived } => {
            let data = request(
                profile,
                transport,
                authority,
                json!({"action":"list","project_id":project,"object":object.value(),"include_archived":archived,"include_revisions":true}),
            )?;
            let rows = data["attachments"].as_array().ok_or_else(bad)?;
            if rows.len() > 200 {
                return Err(bad());
            }
            let mut selected = Vec::new();
            for row in rows {
                check(&row["attachment"], project, None, Some(object))?;
                let mut row = row.clone();
                let revisions = row["revisions"].as_array().ok_or_else(bad)?;
                if revisions.len() > 200 {
                    return Err(bad());
                }
                for revision in revisions {
                    check(
                        revision,
                        project,
                        row["attachment"]["attachment_id"].as_str(),
                        Some(object),
                    )?;
                }
                if let Some(version) = &object.version {
                    row["revisions"] = json!(revisions
                        .iter()
                        .filter(|revision| revision["object"]["version_id"] == *version)
                        .cloned()
                        .collect::<Vec<_>>());
                }
                selected.push(row);
            }
            Ok(
                json!({"project":project,"object":object.value(),"total":selected.len(),"attachments":selected,"truncated":rows.len()==200}),
            )
        }
        Command::Download {
            attachment,
            revision,
        } => {
            let data = request(
                profile,
                transport,
                authority,
                json!({"action":"download","project_id":project,"attachment_id":attachment,"revision_id":revision.as_deref().unwrap_or("")}),
            )?;
            check(&data["attachment"], project, Some(attachment), None)?;
            check(&data["revision"], project, Some(attachment), None)?;
            if revision
                .as_ref()
                .is_some_and(|id| data["revision"]["revision_id"] != *id)
                || !data["revision"]["storage_generation"]
                    .as_u64()
                    .is_some_and(|n| n > 0)
            {
                return Err(bad());
            }
            let url = data["download_url"].as_str().ok_or_else(bad)?;
            let parsed = url::Url::parse(url).map_err(|_| bad())?;
            if parsed.scheme() != "https"
                || parsed.host_str() != Some("storage.googleapis.com")
                || parsed.username() != ""
                || parsed.password().is_some()
                || parsed.port().is_some()
                || parsed.fragment().is_some()
                || url.len() > 8192
                || !parsed.query_pairs().any(|(key, value)| {
                    key == "generation"
                        && value.parse::<u64>().ok()
                            == data["revision"]["storage_generation"].as_u64()
                })
            {
                return Err(bad());
            }
            Ok(
                json!({"project":project,"attachment":attachment,"revision":data["revision"]["revision_id"],"file":data["revision"]["file_name"],"bytes":data["revision"]["byte_size"],"digest":data["revision"]["byte_digest"],"generation":data["revision"]["storage_generation"],"url":url,"expiresAt":data["expires_at"]}),
            )
        }
        Command::Retire {
            attachment,
            revision,
            restore,
        } => {
            let before = current(profile, transport, authority, project, attachment)?;
            let expected = &before["attachment"]["version"];
            let mut change = json!({"project_id":project,"attachment_id":attachment,"expected_version":expected,"restore":restore});
            let payload = if let Some(revision) = revision {
                change["revision_id"] = json!(revision);
                json!({"action":"archive_revision","project_id":project,"revision":change})
            } else {
                json!({"action":"archive","project_id":project,"archive":change})
            };
            let after = request(profile, transport, authority, payload)?;
            check(&after["attachment"], project, Some(attachment), None)?;
            Ok(
                json!({"project":project,"attachment":attachment,"revision":revision,"state":after["attachment"]["state"],"latest":after["attachment"]["latest_revision_id"],"version":after["attachment"]["version"]}),
            )
        }
        Command::Publish {
            object,
            attachment,
            file,
            label,
            purpose,
            bytes,
        } => {
            let digest = format!("{:x}", Sha256::digest(bytes));
            let key =
                serde_json::to_vec(&(project, object.value(), file, &digest)).map_err(|_| bad())?;
            let assigned = format!("att_{:x}", Sha256::digest(&key));
            let attachment = attachment.as_deref().unwrap_or(&assigned);
            let revision = format!("rev_{:x}", Sha256::digest(&key));
            let mut declaration = json!({"project_id":project,"attachment_id":attachment,"revision_id":revision,"object":object.value(),"file_name":file,"media_type":"application/octet-stream","byte_size":bytes.len(),"byte_digest":digest,"label":label.as_deref().unwrap_or(file),"purpose":purpose.as_deref().unwrap_or("")});
            let expected = if matches!(
                command,
                Command::Publish {
                    attachment: Some(_),
                    ..
                }
            ) {
                let before = current(profile, transport, authority, project, attachment)?;
                check(
                    &before["attachment"],
                    project,
                    Some(attachment),
                    Some(object),
                )?;
                Some(before["attachment"]["version"].clone())
            } else {
                None
            };
            let grant = request(
                profile,
                transport,
                authority,
                json!({"action":"start_upload","project_id":project,"upload":declaration}),
            )?;
            let expected_blob = format!(
                "design-attachments/v1/projects/{project}/{}/{}/{attachment}/{revision}/{file}",
                object.kind, object.id
            );
            if grant["attachment_id"] != attachment
                || grant["revision_id"] != revision
                || grant["blob_path"] != expected_blob
            {
                return Err(bad());
            }
            let uri = grant["session_uri"].as_str().ok_or_else(bad)?;
            let parsed = url::Url::parse(uri).map_err(|_| bad())?;
            if parsed.scheme() != "https"
                || parsed.host_str() != Some("storage.googleapis.com")
                || parsed.username() != ""
                || parsed.password().is_some()
                || parsed.port().is_some()
                || parsed.fragment().is_some()
                || !parsed.path().starts_with("/upload/storage/v1/b/")
                || !parsed.path().ends_with("/o")
                || !parsed
                    .query_pairs()
                    .any(|(key, value)| key == "upload_id" && !value.is_empty())
                || uri.len() > 8192
            {
                return Err(bad());
            }
            transport.design_attachment_upload(UploadCall {
                uri: uri.into(),
                bytes,
                digest: digest.clone(),
            })?;
            if let Some(expected) = expected {
                declaration["expected_version"] = expected;
            }
            let committed = request(
                profile,
                transport,
                authority,
                json!({"action":"finalize_upload","project_id":project,"finalize":declaration}),
            )?;
            check(
                &committed["attachment"],
                project,
                Some(attachment),
                Some(object),
            )?;
            let stored = committed["revisions"]
                .as_array()
                .and_then(|rows| rows.iter().find(|row| row["revision_id"] == revision))
                .ok_or_else(bad)?;
            check(stored, project, Some(attachment), Some(object))?;
            if stored["byte_digest"] != digest
                || stored["byte_size"].as_u64() != Some(bytes.len() as u64)
                || stored["object"]["version_id"] != object.value()["version_id"]
                || !stored["storage_generation"].as_u64().is_some_and(|n| n > 0)
            {
                return Err(bad());
            }
            Ok(
                json!({"project":project,"object":object.id,"attachment":attachment,"revision":revision,"ordinal":stored["revision"],"bytes":bytes.len(),"digest":digest,"version":committed["attachment"]["version"],"verified":true}),
            )
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pins_distinguish_governance_from_content_revision() {
        let object = |kind: &str, version: &str| Object {
            kind: kind.into(),
            id: "line".into(),
            version: Some(version.into()),
        };
        assert!(object("lv_transformer", "v2").validate().is_ok());
        assert!(object("lv_transformer", "rev_native").validate().is_err());
        assert!(object("mv_model", "v2").validate().is_err());
        assert!(object("mv_model", "rev_native").validate().is_ok());
    }
}
