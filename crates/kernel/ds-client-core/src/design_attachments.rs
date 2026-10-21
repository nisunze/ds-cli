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
/// The largest page of the project-wide listing ds-brain serves.
pub const MAX_PROJECT_PAGE: u16 = 500;
/// ds-brain's bounds on the free text one field carries: a reason, a
/// provenance reference and kind, a media type, and a paging cursor.
const MAX_REASON: usize = 2000;
const MAX_SOURCE_REFERENCE: usize = 200;
const MAX_SOURCE_KIND: usize = 80;
const MAX_MEDIA_TYPE: usize = 200;
const MAX_CURSOR: usize = 512;
#[derive(Clone)]
pub struct Object {
    pub kind: String,
    pub id: String,
    pub version: Option<String>,
    /// The pinned MV content revision's model digest. ds-brain compares it,
    /// never fills it in; when an MV publish pins a revision without one, the
    /// digest is read from the catalogue first so the binding names the exact
    /// package it was attached to.
    pub version_digest: Option<String>,
}
pub enum Command {
    List {
        object: Object,
        archived: bool,
    },
    /// One logical file with its revisions (`get`).
    Show {
        attachment: String,
        archived: bool,
    },
    /// One page of every file in the project, heads only (`list_project`).
    ListProject {
        archived: bool,
        limit: u16,
        cursor: Option<String>,
    },
    /// Every object version one file was ever attached to
    /// (`list_object_versions`): which model versions carry this `.bak`.
    Versions {
        attachment: String,
    },
    Publish {
        object: Object,
        attachment: Option<String>,
        file: String,
        label: Option<String>,
        purpose: Option<String>,
        /// Declared media type; `application/octet-stream` when absent.
        media_type: Option<String>,
        /// Provenance ds-brain records and never interprets.
        source_kind: Option<String>,
        source_reference: Option<String>,
        /// `false` records the revision without moving `latest`, which is how
        /// a historical file is back-filled.
        make_latest: bool,
        bytes: Vec<u8>,
    },
    Download {
        attachment: String,
        revision: Option<String>,
    },
    /// Move `latest` to one ready revision (`set_latest`) under the head's
    /// version fence: the caller's, or the one read immediately before.
    SetLatest {
        attachment: String,
        revision: String,
        expected_version: Option<u64>,
    },
    Retire {
        attachment: String,
        revision: Option<String>,
        restore: bool,
        reason: Option<String>,
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
/// Present, trimmed, bounded in characters, and free of NUL — ds-brain's own
/// rule for a free-text field.
fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.chars().count() <= max && !value.contains('\0')
}
fn digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
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
                return Err(invalid(
                    "LV attachment versions require assigned vN; MV attachments require the exact content revision id. Read design version list/get metadata and use source_revision for MV, never its governance vN.",
                ));
            }
        }
        if let Some(pinned) = &self.version_digest {
            if self.kind != "mv_model" || self.version.is_none() || !digest(pinned) {
                return Err(invalid(
                    "a version digest pins an MV content revision: it needs --version and is the lowercase 64-hex model digest",
                ));
            }
        }
        Ok(())
    }
    /// The anchor the ids are derived from. The digest is deliberately
    /// absent, so a replay derives the same attachment and revision ids
    /// whether or not the pin was resolved.
    fn key(&self) -> Value {
        let mut value = json!({"kind":self.kind,"id":self.id});
        if let Some(version) = &self.version {
            value["version_id"] = json!(version);
        }
        value
    }
    fn value(&self) -> Value {
        let mut value = self.key();
        if let Some(pinned) = &self.version_digest {
            value["version_digest"] = json!(pinned);
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
            Self::Show { attachment, .. } | Self::Versions { attachment } => {
                if id(attachment) {
                    Ok(())
                } else {
                    Err(invalid("invalid attachment identity"))
                }
            }
            Self::ListProject { limit, cursor, .. } => {
                if (1..=MAX_PROJECT_PAGE).contains(limit)
                    && cursor.as_ref().is_none_or(|value| {
                        !value.is_empty()
                            && value.len() <= MAX_CURSOR
                            && !value.chars().any(char::is_control)
                    })
                {
                    Ok(())
                } else {
                    Err(invalid(
                        "a project attachment page is 1-500 files with an exact cursor from the previous page",
                    ))
                }
            }
            Self::Publish {
                object,
                attachment,
                file,
                label,
                purpose,
                media_type,
                source_kind,
                source_reference,
                bytes,
                ..
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
                if media_type.as_ref().is_some_and(|value| {
                    value.is_empty()
                        || value.len() > MAX_MEDIA_TYPE
                        || !value.contains('/')
                        || value.chars().any(|c| c.is_whitespace() || c.is_control())
                }) || source_kind
                    .as_ref()
                    .is_some_and(|value| !text(value, MAX_SOURCE_KIND))
                    || source_reference
                        .as_ref()
                        .is_some_and(|value| !text(value, MAX_SOURCE_REFERENCE))
                {
                    return Err(invalid(
                        "invalid media type (type/subtype, 200) or provenance (kind 80, reference 200 characters)",
                    ));
                }
                Ok(())
            }
            Self::SetLatest {
                attachment,
                revision,
                expected_version,
            } => {
                if id(attachment) && id(revision) && expected_version.is_none_or(|n| n > 0) {
                    Ok(())
                } else {
                    Err(invalid(
                        "invalid attachment, revision or expected version (a positive head version)",
                    ))
                }
            }
            Self::Download {
                attachment,
                revision,
            } => {
                if id(attachment) && revision.as_ref().is_none_or(|value| id(value)) {
                    Ok(())
                } else {
                    Err(invalid("invalid attachment or revision identity"))
                }
            }
            Self::Retire {
                attachment,
                revision,
                reason,
                ..
            } => {
                if id(attachment)
                    && revision.as_ref().is_none_or(|value| id(value))
                    && reason.as_ref().is_none_or(|value| text(value, MAX_REASON))
                {
                    Ok(())
                } else {
                    Err(invalid(
                        "invalid attachment, revision identity or reason (2000 characters)",
                    ))
                }
            }
        }
    }
}
/// One revision's signed read, built only after the locator was checked
/// against the revision's own stored generation.
pub struct DownloadCall<'a> {
    url: &'a str,
    size: usize,
}
impl DownloadCall<'_> {
    pub fn url(&self) -> &str {
        self.url
    }
    pub fn response_limit(&self) -> usize {
        self.size
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
        Command::Show {
            attachment,
            archived,
        } => {
            let data = request(
                profile,
                transport,
                authority,
                json!({"action":"get","project_id":project,"attachment_id":attachment,"include_archived":archived}),
            )?;
            check(&data["attachment"], project, Some(attachment), None)?;
            if !data["attachment"]["version"]
                .as_u64()
                .is_some_and(|n| n > 0)
            {
                return Err(bad());
            }
            let revisions = if data["revisions"].is_null() {
                Vec::new()
            } else {
                data["revisions"].as_array().ok_or_else(bad)?.clone()
            };
            if revisions.len() > 200 {
                return Err(bad());
            }
            for revision in &revisions {
                check(revision, project, Some(attachment), None)?;
            }
            Ok(
                json!({"project":project,"attachment":data["attachment"],"revisions":revisions,"truncated":data["truncated"]==true}),
            )
        }
        Command::ListProject {
            archived,
            limit,
            cursor,
        } => {
            let mut payload = json!({"action":"list_project","project_id":project,"page_size":limit,"include_archived":archived});
            if let Some(cursor) = cursor {
                payload["cursor"] = json!(cursor);
            }
            let data = request(profile, transport, authority, payload)?;
            let rows = if data["attachments"].is_null() {
                Vec::new()
            } else {
                data["attachments"].as_array().ok_or_else(bad)?.clone()
            };
            let more = data["has_more"].as_bool().ok_or_else(bad)?;
            let next = data["next_cursor"].as_str().unwrap_or("");
            if rows.len() > usize::from(*limit)
                || (more && next.is_empty())
                || next.len() > MAX_CURSOR
            {
                return Err(bad());
            }
            let mut heads = Vec::with_capacity(rows.len());
            for row in &rows {
                check(&row["attachment"], project, None, None)?;
                if !archived && row["attachment"]["state"] == "archived" {
                    return Err(bad());
                }
                heads.push(row["attachment"].clone());
            }
            Ok(
                json!({"project":project,"total":heads.len(),"attachments":heads,"more":more,"next_cursor":(!next.is_empty()).then_some(next)}),
            )
        }
        Command::Versions { attachment } => {
            let data = request(
                profile,
                transport,
                authority,
                json!({"action":"list_object_versions","project_id":project,"attachment_id":attachment}),
            )?;
            let rows = if data["object_versions"].is_null() {
                Vec::new()
            } else {
                data["object_versions"].as_array().ok_or_else(bad)?.clone()
            };
            if rows.len() > 200 {
                return Err(bad());
            }
            // One file stays anchored to the object it was created on; only
            // the version binding differs between rows.
            let anchor = rows.first().map(|row| &row["object"]);
            for row in &rows {
                let ids = row["revision_ids"].as_array().ok_or_else(bad)?;
                if !matches!(
                    row["object"]["kind"].as_str(),
                    Some("lv_transformer" | "mv_model")
                ) || !row["object"]["id"].as_str().is_some_and(id)
                    || anchor.is_some_and(|first| {
                        first["kind"] != row["object"]["kind"] || first["id"] != row["object"]["id"]
                    })
                    || ids.is_empty()
                    || !ids.iter().all(|value| value.as_str().is_some_and(id))
                    || row["revision_count"].as_u64() != Some(ids.len() as u64)
                {
                    return Err(bad());
                }
            }
            Ok(
                json!({"project":project,"attachment":attachment,"total":rows.len(),"object_versions":rows}),
            )
        }
        Command::SetLatest {
            attachment,
            revision,
            expected_version,
        } => {
            let expected = match expected_version {
                Some(version) => json!(version),
                None => current(profile, transport, authority, project, attachment)?["attachment"]
                    ["version"]
                    .clone(),
            };
            let after = request(
                profile,
                transport,
                authority,
                json!({"action":"set_latest","project_id":project,"latest":{"project_id":project,"attachment_id":attachment,"revision_id":revision,"expected_version":expected}}),
            )?;
            check(&after["attachment"], project, Some(attachment), None)?;
            if after["attachment"]["latest_revision_id"] != *revision
                || !after["attachment"]["version"]
                    .as_u64()
                    .zip(expected.as_u64())
                    .is_some_and(|(now, before)| now > before)
            {
                return Err(bad());
            }
            Ok(
                json!({"project":project,"attachment":attachment,"latest":revision,"ordinal":after["attachment"]["latest_revision"],"previous_version":expected,"version":after["attachment"]["version"]}),
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
            reason,
        } => {
            let before = current(profile, transport, authority, project, attachment)?;
            let expected = &before["attachment"]["version"];
            let mut change = json!({"project_id":project,"attachment_id":attachment,"expected_version":expected,"restore":restore});
            if let Some(reason) = reason {
                change["reason"] = json!(reason.trim());
            }
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
            media_type,
            source_kind,
            source_reference,
            make_latest,
            bytes,
        } => {
            // An MV pin names a content revision; its model digest is read
            // from the catalogue and sent, so ds-brain compares the binding
            // against the package it names rather than recording a bare id.
            let mut object = object.clone();
            if object.kind == "mv_model" && object.version_digest.is_none() {
                if let Some(version) = &object.version {
                    object.version_digest = Some(crate::grid_models::revision_digest(
                        profile, transport, authority, project, &object.id, version,
                    )?);
                }
            }
            let object = &object;
            let digest = format!("{:x}", Sha256::digest(bytes));
            let key =
                serde_json::to_vec(&(project, object.key(), file, &digest)).map_err(|_| bad())?;
            let assigned = format!("att_{:x}", Sha256::digest(&key));
            let attachment = attachment.as_deref().unwrap_or(&assigned);
            let revision = format!("rev_{:x}", Sha256::digest(&key));
            let media_type = media_type
                .as_deref()
                .unwrap_or("application/octet-stream")
                .to_ascii_lowercase();
            let mut declaration = json!({"project_id":project,"attachment_id":attachment,"revision_id":revision,"object":object.value(),"file_name":file,"media_type":media_type,"byte_size":bytes.len(),"byte_digest":digest,"label":label.as_deref().unwrap_or(file),"purpose":purpose.as_deref().unwrap_or("")});
            if source_kind.is_some() || source_reference.is_some() {
                let mut source = json!({});
                if let Some(kind) = source_kind {
                    source["kind"] = json!(kind.trim());
                }
                if let Some(reference) = source_reference {
                    source["reference"] = json!(reference.trim());
                }
                declaration["source"] = source;
            }
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
            // Finalize alone carries the pointer choice; the upload grant's
            // declaration has no such field.
            if !make_latest {
                declaration["make_latest"] = json!(false);
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
            let latest = committed["attachment"]["latest_revision_id"] == revision;
            if stored["byte_digest"] != digest
                || stored["byte_size"].as_u64() != Some(bytes.len() as u64)
                || stored["object"]["version_id"] != object.value()["version_id"]
                || stored["object"]["version_digest"] != object.value()["version_digest"]
                || stored["media_type"] != media_type
                || !stored["storage_generation"].as_u64().is_some_and(|n| n > 0)
            {
                return Err(bad());
            }
            Ok(
                json!({"project":project,"object":object.id,"object_version":object.version,"object_version_digest":object.version_digest,"attachment":attachment,"revision":revision,"ordinal":stored["revision"],"bytes":bytes.len(),"digest":digest,"media_type":media_type,"latest":latest,"version":committed["attachment"]["version"],"verified":true}),
            )
        }
    }
}
/// One revision's bytes, fetched through the server-signed read the
/// `download` action authorized and verified against the revision's
/// server-verified size and SHA-256 before the caller sees a byte. The signed
/// locator is spent here and never returned.
pub(crate) fn fetch<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    attachment: &str,
    revision: Option<&str>,
) -> Result<(Value, Vec<u8>), ClientError> {
    let mut receipt = run(
        profile,
        transport,
        authority,
        project,
        &Command::Download {
            attachment: attachment.to_owned(),
            revision: revision.map(str::to_owned),
        },
    )?;
    let size = receipt["bytes"]
        .as_u64()
        .filter(|n| *n > 0 && *n <= MAX_BYTES as u64)
        .ok_or_else(bad)? as usize;
    let expected = receipt["digest"]
        .as_str()
        .filter(|value| digest(value))
        .ok_or_else(bad)?
        .to_owned();
    let url = receipt["url"].as_str().ok_or_else(bad)?.to_owned();
    let mut response = transport.design_attachment_bytes(DownloadCall { url: &url, size })?;
    if response.status != 200
        || response.body.len() != size
        || format!("{:x}", Sha256::digest(&response.body)) != expected
    {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "the fetched attachment bytes do not match the revision's server-verified size and SHA-256",
        ));
    }
    if let Some(receipt) = receipt.as_object_mut() {
        receipt.remove("url");
        receipt.remove("expiresAt");
        receipt.insert("verified".into(), json!(true));
    }
    Ok((receipt, std::mem::take(&mut response.body)))
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
            version_digest: None,
        };
        assert!(object("lv_transformer", "v2").validate().is_ok());
        assert!(object("lv_transformer", "rev_native").validate().is_err());
        assert!(object("mv_model", "v2").validate().is_err());
        assert!(object("mv_model", "rev_native").validate().is_ok());
    }

    #[test]
    fn a_version_digest_pins_only_an_mv_content_revision() {
        let pin = |kind: &str, version: Option<&str>, digest: &str| Object {
            kind: kind.into(),
            id: "line".into(),
            version: version.map(str::to_owned),
            version_digest: Some(digest.into()),
        };
        let exact = "a".repeat(64);
        assert!(pin("mv_model", Some("rev_native"), &exact)
            .validate()
            .is_ok());
        // Without the revision it pins nothing; ds-brain refuses the same.
        assert!(pin("mv_model", None, &exact).validate().is_err());
        // A transformer version carries no content digest.
        assert!(pin("lv_transformer", Some("v2"), &exact)
            .validate()
            .is_err());
        // Only the lowercase bare hex ds-brain compares against.
        assert!(pin("mv_model", Some("rev_native"), &"A".repeat(64))
            .validate()
            .is_err());
        assert!(
            pin("mv_model", Some("rev_native"), &format!("sha256:{exact}"))
                .validate()
                .is_err()
        );
    }

    #[test]
    fn the_derived_ids_ignore_the_resolved_digest() {
        let bare = Object {
            kind: "mv_model".into(),
            id: "line".into(),
            version: Some("rev_native".into()),
            version_digest: None,
        };
        let pinned = Object {
            version_digest: Some("b".repeat(64)),
            ..bare.clone()
        };
        assert_eq!(bare.key(), pinned.key());
        assert_ne!(bare.value(), pinned.value());
        assert_eq!(pinned.value()["version_digest"], "b".repeat(64));
    }

    #[test]
    fn new_reads_and_the_pointer_move_refuse_malformed_requests_locally() {
        let project = "project_a";
        assert!(Command::ListProject {
            archived: false,
            limit: 0,
            cursor: None
        }
        .validate(project)
        .is_err());
        assert!(Command::ListProject {
            archived: false,
            limit: MAX_PROJECT_PAGE + 1,
            cursor: None
        }
        .validate(project)
        .is_err());
        assert!(Command::ListProject {
            archived: true,
            limit: MAX_PROJECT_PAGE,
            cursor: Some("x".repeat(MAX_CURSOR + 1))
        }
        .validate(project)
        .is_err());
        assert!(Command::SetLatest {
            attachment: "file".into(),
            revision: "rev".into(),
            expected_version: Some(0)
        }
        .validate(project)
        .is_err());
        assert!(Command::Versions {
            attachment: "a/b".into()
        }
        .validate(project)
        .is_err());
        assert!(Command::Retire {
            attachment: "file".into(),
            revision: None,
            restore: false,
            reason: Some("   ".into())
        }
        .validate(project)
        .is_err());
        assert!(Command::Retire {
            attachment: "file".into(),
            revision: None,
            restore: false,
            reason: Some("x".repeat(MAX_REASON + 1))
        }
        .validate(project)
        .is_err());
    }
}
