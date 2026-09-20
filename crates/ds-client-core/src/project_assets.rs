//! Project Assets, headlessly: the catalogue door `ds assets` walks through.
//!
//! Until 2026-09-20 nine `ds assets` commands — list, tree, read, preview,
//! classify, promote, attach, ingest, folder — relayed through the paired
//! desktop to the page's own adapter (`ds-web/src/lib/desktop/cli-assets.ts`),
//! so a server with no window could not file a document at all. Nothing about
//! the catalogue needs a window: `POST /api/v1/assets` authenticates from the
//! bearer, ds-brain resolves the caller's class authority against the project
//! named in the body, and the bytes travel through the same resumable uploader
//! and signed reads the browser used. `shared_assets` beside this module
//! already walked that door for map publication; this module is the general
//! form of it.
//!
//! **This module is the DOOR, not the decisions.** It sends a closed set of
//! actions, verifies the answer's identity where bytes are involved, and
//! returns what the server said. What a byte stream IS — its format, its kind,
//! its members, its preview — belongs to `ds_command_kernel::assets`; what a
//! caller may do belongs to ds-brain.
//!
//! Closed by construction: [`Command`] is the complete set of actions this
//! owner can send, every payload is built here, and ds-brain decodes with
//! `DisallowUnknownFields`, so a key this file does not write is a key the
//! request cannot carry.

use std::io::Read;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use crate::{
    authorized::Authority, shared_assets::DownloadCall, AuthorizedTransport, ClientError,
    ClientProfile, ErrorKind, ServiceRefusal, SharedAssetsCall, UploadBytesCall,
};

/// The largest page one catalogue listing returns — ds-brain's
/// `AssetListMaxPageSize`.
pub const MAX_PAGE_SIZE: u16 = 200;
/// The most bytes one asset read fetches into memory (contract §4.2, the
/// preview ceiling). A larger document is refused by name with the number;
/// it opens from its own surface, which streams.
pub const MAX_READ_BYTES: u64 = 32 * 1024 * 1024;
/// The most bytes one ingest streams. Above it the file is not a document
/// this catalogue holds.
pub const MAX_INGEST_BYTES: u64 = 256 * 1024 * 1024;
/// How much of a file's head the kernel recognises a format from.
pub const RECOGNISE_HEAD_BYTES: usize = 64 * 1024;
/// The longest catalogue answer this owner reads.
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}
fn bad() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "project assets did not answer in a shape this build understands",
    )
}

fn token(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.trim() == s
        && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
}
fn minted(id: &str) -> bool {
    id.strip_prefix("a_").is_some_and(|rest| {
        rest.len() == 12
            && rest
                .bytes()
                .all(|b| b.is_ascii_digit() || b.is_ascii_lowercase())
    })
}
fn folder_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}
fn bounded_text(s: &str, max: usize) -> bool {
    !s.is_empty() && s.chars().count() <= max && !s.chars().any(char::is_control)
}
fn optional(s: &Option<String>, max: usize) -> bool {
    s.as_deref().is_none_or(|s| bounded_text(s, max))
}
const KINDS: [&str; 8] = [
    "doc", "sheet", "mail", "note", "geo", "image", "pack", "other",
];
const STATUSES: [&str; 3] = ["fresh", "durable", "archive"];
const SENSITIVITIES: [&str; 4] = ["open", "internal", "restricted", "confidential"];
fn choice(s: &Option<String>, allowed: &[&str]) -> bool {
    s.as_deref().is_none_or(|s| allowed.contains(&s))
}
fn folder_path(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 8 * 65
        && !path.starts_with('/')
        && !path.ends_with('/')
        && path.split('/').all(|segment| {
            !segment.is_empty()
                && segment != "."
                && segment != ".."
                && segment.trim() == segment
                && segment.chars().count() <= 64
                && !segment.chars().any(char::is_control)
        })
}
fn link_valid(link: &Value) -> bool {
    let Some(object) = link.as_object() else {
        return false;
    };
    let text = |key: &str| object.get(key).and_then(Value::as_str);
    match text("kind") {
        Some("pm_task") | Some("pm_record") => object.len() == 2 && text("id").is_some_and(token),
        Some("ds_object") => {
            object.len() == 3
                && text("object_type").is_some_and(token)
                && text("entity_id").is_some_and(token)
        }
        _ => false,
    }
}

/// One governed catalogue action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// The declared (user) folders with per-caller counts — ds-brain's
    /// `tree`. The system folders are the kernel's projection, never a row.
    Folders,
    /// One paged catalogue read with a strict cursor.
    List {
        folder_id: Option<String>,
        kind: Option<String>,
        status: Option<String>,
        sensitivity: Option<String>,
        since: Option<String>,
        limit: u16,
        cursor: Option<String>,
    },
    /// One catalogue row.
    Get { asset_id: String },
    /// One signed read: the row, a V4 URL pinned to the verified generation,
    /// and its expiry. The bytes are fetched by [`read_bytes`].
    Download { asset_id: String },
    /// Declare a folder with the defaults its children inherit.
    FolderCreate {
        path: String,
        default_sensitivity: Option<String>,
        default_status: Option<String>,
    },
    /// Rename a folder or change its defaults, under `expected_version`.
    FolderUpdate {
        folder_id: String,
        rename_to: Option<String>,
        default_sensitivity: Option<String>,
        default_status: Option<String>,
        expected_version: u64,
    },
    /// Change kind, status, owner, folder or sensitivity, under
    /// `expected_version`; always audited.
    Classify {
        asset_id: String,
        kind: Option<String>,
        status: Option<String>,
        sensitivity: Option<String>,
        folder_id: Option<String>,
        owner: Option<String>,
        reason: Option<String>,
        expected_version: u64,
    },
    /// Link an asset to a task, a record or a DS object — or remove that
    /// link. The link lives on the asset row, one way.
    Attach {
        asset_id: String,
        link: Value,
        detach: bool,
    },
    /// `ingest_start` → the resumable uploader → `ingest_finalize`, as one
    /// transaction. `kind` and `format` are the kernel's recognition of the
    /// head; `sha256` and `size` are of the whole file, computed by the
    /// caller in one streaming pass so a 200 MiB pack never sits in memory
    /// twice.
    Ingest(IngestRequest),
}

/// What an ingest names. The bytes are streamed from `reader` by [`run`];
/// the caller opens the file and computes its digest and size first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IngestRequest {
    pub name: String,
    pub size: u64,
    pub sha256: String,
    pub content_type: Option<String>,
    pub folder_id: Option<String>,
    pub sensitivity: Option<String>,
    pub kind: String,
    pub format: String,
}

impl Command {
    fn action(&self) -> &'static str {
        match self {
            Self::Folders => "tree",
            Self::List { .. } => "list",
            Self::Get { .. } => "get",
            Self::Download { .. } => "download",
            Self::FolderCreate { .. } => "folder_create",
            Self::FolderUpdate { .. } => "folder_update",
            Self::Classify { .. } => "classify",
            Self::Attach { detach: false, .. } => "attach",
            Self::Attach { detach: true, .. } => "detach",
            Self::Ingest(_) => "ingest_start",
        }
    }

    /// Refuse locally what the server would refuse by name, before a round
    /// trip is spent on it.
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        if !token(project) {
            return Err(invalid("project assets need the project they act on"));
        }
        let ok = match self {
            Self::Folders => true,
            Self::List {
                folder_id,
                kind,
                status,
                sensitivity,
                since,
                limit,
                cursor,
            } => {
                (1..=MAX_PAGE_SIZE).contains(limit)
                    && folder_id.as_deref().is_none_or(folder_id_valid)
                    && choice(kind, &KINDS)
                    && choice(status, &STATUSES)
                    && choice(sensitivity, &SENSITIVITIES)
                    && optional(since, 64)
                    && optional(cursor, 512)
            }
            Self::Get { asset_id } | Self::Download { asset_id } => minted(asset_id),
            Self::FolderCreate {
                path,
                default_sensitivity,
                default_status,
            } => {
                folder_path(path)
                    && choice(default_sensitivity, &SENSITIVITIES)
                    && choice(default_status, &STATUSES)
            }
            Self::FolderUpdate {
                folder_id,
                rename_to,
                default_sensitivity,
                default_status,
                expected_version,
            } => {
                folder_id_valid(folder_id)
                    && rename_to
                        .as_deref()
                        .is_none_or(|name| folder_path(name) && !name.contains('/'))
                    && choice(default_sensitivity, &SENSITIVITIES)
                    && choice(default_status, &STATUSES)
                    && *expected_version > 0
                    && (rename_to.is_some()
                        || default_sensitivity.is_some()
                        || default_status.is_some())
            }
            Self::Classify {
                asset_id,
                kind,
                status,
                sensitivity,
                folder_id,
                owner,
                reason,
                expected_version,
            } => {
                minted(asset_id)
                    && choice(kind, &KINDS)
                    && choice(status, &STATUSES)
                    && choice(sensitivity, &SENSITIVITIES)
                    && folder_id.as_deref().is_none_or(folder_id_valid)
                    && optional(owner, 200)
                    && optional(reason, 1_000)
                    && *expected_version > 0
                    && (kind.is_some()
                        || status.is_some()
                        || sensitivity.is_some()
                        || folder_id.is_some()
                        || owner.is_some())
            }
            Self::Attach { asset_id, link, .. } => minted(asset_id) && link_valid(link),
            Self::Ingest(ingest) => {
                token(&ingest.name)
                    && ingest.size > 0
                    && ingest.size <= MAX_INGEST_BYTES
                    && ingest.sha256.len() == 64
                    && ingest
                        .sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                    && optional(&ingest.content_type, 200)
                    && ingest.folder_id.as_deref().is_none_or(folder_id_valid)
                    && choice(&ingest.sensitivity, &SENSITIVITIES)
                    && KINDS.contains(&ingest.kind.as_str())
                    && bounded_text(&ingest.format, 16)
            }
        };
        if ok {
            Ok(())
        } else {
            Err(invalid(
                "invalid project assets request: an id, a path, a class, a bound or a link is outside the catalogue's vocabulary",
            ))
        }
    }

    fn payload(&self, project: &str) -> Value {
        let mut payload = json!({ "action": self.action(), "ds_project": project });
        let put = |payload: &mut Value, key: &str, value: &Option<String>| {
            if let Some(value) = value {
                payload[key] = json!(value);
            }
        };
        match self {
            Self::Folders => {}
            Self::List {
                folder_id,
                kind,
                status,
                sensitivity,
                since,
                limit,
                cursor,
            } => {
                put(&mut payload, "folder_id", folder_id);
                put(&mut payload, "kind", kind);
                put(&mut payload, "status", status);
                put(&mut payload, "sensitivity", sensitivity);
                put(&mut payload, "since", since);
                payload["limit"] = json!(limit);
                put(&mut payload, "cursor", cursor);
            }
            Self::Get { asset_id } => payload["asset_id"] = json!(asset_id),
            Self::Download { asset_id } => {
                payload["asset_id"] = json!(asset_id);
                payload["surface"] = json!("cli");
            }
            Self::FolderCreate {
                path,
                default_sensitivity,
                default_status,
            } => {
                let mut folder = json!({ "path": path });
                put(&mut folder, "default_sensitivity", default_sensitivity);
                put(&mut folder, "default_status", default_status);
                payload["folder"] = folder;
            }
            Self::FolderUpdate {
                folder_id,
                rename_to,
                default_sensitivity,
                default_status,
                expected_version,
            } => {
                payload["folder_id"] = json!(folder_id);
                let mut patch = json!({ "expected_version": expected_version });
                put(&mut patch, "rename_to", rename_to);
                put(&mut patch, "default_sensitivity", default_sensitivity);
                put(&mut patch, "default_status", default_status);
                payload["patch"] = patch;
            }
            Self::Classify {
                asset_id,
                kind,
                status,
                sensitivity,
                folder_id,
                owner,
                reason,
                expected_version,
            } => {
                payload["asset_id"] = json!(asset_id);
                let mut classify = json!({ "expected_version": expected_version });
                put(&mut classify, "kind", kind);
                put(&mut classify, "status", status);
                put(&mut classify, "sensitivity", sensitivity);
                put(&mut classify, "folder_id", folder_id);
                if let Some(owner) = owner {
                    classify["owner"] = json!({ "kind": "user", "id": owner });
                }
                put(&mut classify, "reason", reason);
                payload["classify"] = classify;
            }
            Self::Attach { asset_id, link, .. } => {
                payload["asset_id"] = json!(asset_id);
                payload["link"] = link.clone();
            }
            Self::Ingest(ingest) => {
                let mut start = json!({
                    "name": ingest.name,
                    "size": ingest.size,
                    "sha256": ingest.sha256,
                });
                put(&mut start, "content_type", &ingest.content_type);
                put(&mut start, "folder_id", &ingest.folder_id);
                put(&mut start, "sensitivity", &ingest.sensitivity);
                payload["ingest"] = start;
            }
        }
        payload
    }
}

fn folder_id_valid(id: &str) -> bool {
    folder_id(id)
}

/// Classify one refused action, carrying the server's own code and sentence.
///
/// ds-brain's assets handler answers every refusal in the DS envelope; the
/// code is the actionable half (`ASSET_REFUSED` with `details.reason`,
/// `CONFLICT`, `ASSET_CLASS_FORBIDDEN`, `INSUFFICIENT_PERMISSIONS`,
/// `NOT_FOUND` — which is also a confidential row the caller may not know
/// exists, by design). The status decides the class; the words cross intact.
fn refusal(status: u16, body: &[u8]) -> ClientError {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let error = &value["error"];
    let code = error["code"].as_str().map(str::to_ascii_lowercase);
    let message = error["message"].as_str().map(|sentence| {
        // `details.reason` names the rule an `ASSET_REFUSED` broke; a caller
        // reading the sentence alone would not know which.
        match error["details"]["reason"].as_str() {
            Some(reason) => format!("{sentence} [{reason}]"),
            None => sentence.to_owned(),
        }
    });
    let envelope = ServiceRefusal::new(status, code.as_deref(), message.as_deref());
    let (kind, owner) = match status {
        400 | 422 => (
            ErrorKind::InvalidInput,
            "project assets refused this request",
        ),
        401 | 403 => (
            ErrorKind::AuthenticationRejected,
            "project assets refused this verified identity or capability",
        ),
        404 => (
            ErrorKind::ResourceNotFound,
            "project assets could not find the asset, folder or project",
        ),
        409 => (
            ErrorKind::InvalidInput,
            "project assets refused a write against a row that moved",
        ),
        501 => (
            ErrorKind::RouteUnavailable,
            "project assets does not serve this action on this deployment",
        ),
        _ => (
            ErrorKind::Transient,
            "project assets is temporarily unavailable",
        ),
    };
    // Always carried, even bare: the status alone tells a gateway 503 from a
    // refusal the catalogue authored, and a caller reading "temporarily
    // unavailable" deserves the number.
    ClientError::with_service_refusal(kind, owner, envelope)
}

fn call<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    payload: Value,
) -> Result<Value, ClientError> {
    let response = transport.shared_assets(SharedAssetsCall {
        profile,
        authority: authority.call(),
        payload,
    })?;
    if response.status != 200 {
        return Err(refusal(response.status, &response.body));
    }
    if response.body.len() > MAX_RESPONSE_BYTES {
        return Err(bad());
    }
    let reply: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    if reply["success"] != true || !reply["data"].is_object() {
        return Err(bad());
    }
    Ok(reply["data"].clone())
}

/// The identity a catalogue row must carry to be this project's.
fn own_row(row: &Value, project: &str) -> Result<(), ClientError> {
    if row["schema"] != "ds.assets.asset/v1"
        || row["project_id"] != project
        || !row["asset_id"].as_str().is_some_and(minted)
    {
        return Err(bad());
    }
    Ok(())
}

/// Send one governed action for one project and return its `data`.
///
/// `Ingest` is the one transaction: `ingest_start`, the upload, then
/// `ingest_finalize`, verified against the name, digest and size the caller
/// declared. The reader is consumed once.
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
    reader: Option<&mut dyn Read>,
) -> Result<Value, ClientError> {
    command.validate(project)?;
    let data = call(profile, transport, authority, command.payload(project))?;
    match command {
        Command::Folders => {
            if !data["folders"].is_array() {
                return Err(bad());
            }
            Ok(data)
        }
        Command::List { limit, .. } => {
            let rows = data["assets"].as_array().ok_or_else(bad)?;
            if rows.len() > usize::from(*limit) {
                return Err(bad());
            }
            for row in rows {
                own_row(row, project)?;
            }
            Ok(data)
        }
        Command::Get { asset_id } => {
            let row = data
                .get("asset")
                .filter(|row| row.is_object())
                .unwrap_or(&data);
            own_row(row, project)?;
            if row["asset_id"] != *asset_id {
                return Err(bad());
            }
            Ok(row.clone())
        }
        Command::Download { asset_id } => {
            own_row(&data["asset"], project)?;
            if data["asset"]["asset_id"] != *asset_id
                || !data["download_url"].is_string()
                || !data["expires_at"].is_string()
            {
                return Err(bad());
            }
            Ok(data)
        }
        Command::FolderCreate { .. } | Command::FolderUpdate { .. } => {
            let folder = data
                .get("folder")
                .filter(|folder| folder.is_object())
                .unwrap_or(&data);
            if !folder["folder_id"].as_str().is_some_and(folder_id) {
                return Err(bad());
            }
            Ok(json!({ "folder": folder }))
        }
        Command::Classify { asset_id, .. } => {
            own_row(&data["asset"], project)?;
            if data["asset"]["asset_id"] != *asset_id || !data["audit_id"].is_string() {
                return Err(bad());
            }
            Ok(data)
        }
        Command::Attach { asset_id, .. } => {
            let row = data
                .get("asset")
                .filter(|row| row.is_object())
                .unwrap_or(&data);
            own_row(row, project)?;
            if row["asset_id"] != *asset_id {
                return Err(bad());
            }
            Ok(json!({ "asset": row }))
        }
        Command::Ingest(ingest) => {
            let id = data["asset_id"]
                .as_str()
                .filter(|id| minted(id))
                .ok_or_else(bad)?
                .to_owned();
            let uri = data["upload_url"].as_str().ok_or_else(bad)?;
            crate::solar_project::validate_upload_uri(uri)?;
            let reader = reader.ok_or_else(|| invalid("an ingest needs the file's bytes"))?;
            let uploaded = transport.upload_bytes(UploadBytesCall {
                uri,
                size: ingest.size,
                reader,
            })?;
            crate::project_data::uploaded(uploaded)?;
            let finalized = call(
                profile,
                transport,
                authority,
                json!({
                    "action": "ingest_finalize", "ds_project": project, "asset_id": id,
                    "finalize": {
                        "asset_id": id, "sha256": ingest.sha256,
                        "kind": ingest.kind, "format": ingest.format,
                    },
                }),
            )?;
            let row = finalized
                .get("asset")
                .filter(|row| row.is_object())
                .unwrap_or(&finalized);
            own_row(row, project)?;
            if row["asset_id"] != id
                || row["digest"] != format!("sha256:{}", ingest.sha256)
                || row["bytes"].as_u64() != Some(ingest.size)
            {
                return Err(bad());
            }
            Ok(json!({ "asset": row }))
        }
    }
}

/// Fetch one asset's bytes through its signed read and verify them against
/// the row: the size the row states and the digest it carries. A row above
/// [`MAX_READ_BYTES`] is refused by name before anything is fetched.
pub(crate) fn read_bytes<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    asset_id: &str,
) -> Result<(Value, Vec<u8>), ClientError> {
    let data = run(
        profile,
        transport,
        authority,
        project,
        &Command::Download {
            asset_id: asset_id.to_owned(),
        },
        None,
    )?;
    let asset = data["asset"].clone();
    let size = asset["bytes"].as_u64().filter(|n| *n > 0).ok_or_else(bad)?;
    if size > MAX_READ_BYTES {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "the asset is above the 32 MiB read bound; open it from its own surface",
        ));
    }
    let url = data["download_url"].as_str().ok_or_else(bad)?;
    let u = url::Url::parse(url).map_err(|_| bad())?;
    if url.len() > 8192
        || u.scheme() != "https"
        || u.host_str() != Some("storage.googleapis.com")
        || u.port().is_some()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
        || !u.query_pairs().any(|(k, _)| k == "generation")
    {
        return Err(bad());
    }
    let mut bytes = transport.shared_asset_bytes(DownloadCall::new(url, size as usize))?;
    if bytes.status != 200 || bytes.body.len() != size as usize {
        return Err(bad());
    }
    let digest = format!("sha256:{:x}", Sha256::digest(&bytes.body));
    if asset["digest"]
        .as_str()
        .is_some_and(|expected| expected != digest)
    {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "the fetched bytes do not match the asset's verified digest",
        ));
    }
    // The response scrubs its body on drop; the verified bytes are the
    // caller's now, so they are taken out rather than copied.
    Ok((asset, std::mem::take(&mut bytes.body)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list() -> Command {
        Command::List {
            folder_id: None,
            kind: None,
            status: None,
            sensitivity: None,
            since: None,
            limit: 50,
            cursor: None,
        }
    }

    #[test]
    fn the_action_vocabulary_is_closed_and_the_payloads_are_the_handlers_own() {
        assert_eq!(Command::Folders.action(), "tree");
        assert_eq!(list().action(), "list");
        let attach = Command::Attach {
            asset_id: "a_7kq3nr2v0b1c".into(),
            link: json!({"kind": "pm_task", "id": "t_1"}),
            detach: false,
        };
        assert_eq!(attach.action(), "attach");
        assert_eq!(attach.payload("p")["link"]["id"], "t_1");
        let detach = Command::Attach {
            asset_id: "a_7kq3nr2v0b1c".into(),
            link: json!({"kind": "pm_record", "id": "R1"}),
            detach: true,
        };
        assert_eq!(detach.action(), "detach");
        let classify = Command::Classify {
            asset_id: "a_7kq3nr2v0b1c".into(),
            kind: None,
            status: Some("durable".into()),
            sensitivity: None,
            folder_id: Some("f_1".into()),
            owner: Some("lead@ds.rw".into()),
            reason: Some("settled".into()),
            expected_version: 3,
        };
        let payload = classify.payload("p");
        assert_eq!(payload["classify"]["expected_version"], 3);
        assert_eq!(payload["classify"]["owner"]["kind"], "user");
        assert!(payload["classify"].get("kind").is_none());
        let folder = Command::FolderCreate {
            path: "contracts/2026".into(),
            default_sensitivity: Some("confidential".into()),
            default_status: None,
        };
        assert_eq!(folder.payload("p")["folder"]["path"], "contracts/2026");
        let download = Command::Download {
            asset_id: "a_7kq3nr2v0b1c".into(),
        };
        assert_eq!(download.payload("p")["surface"], "cli");
    }

    #[test]
    fn a_request_outside_the_catalogues_vocabulary_is_refused_before_it_is_sent() {
        assert!(list().validate("p").is_ok());
        let refused = |command: Command| command.validate("p").expect_err("refused").kind();
        let list_with = |limit: u16, kind: Option<&str>| Command::List {
            folder_id: None,
            kind: kind.map(str::to_owned),
            status: None,
            sensitivity: None,
            since: None,
            limit,
            cursor: None,
        };
        assert_eq!(refused(list_with(201, None)), ErrorKind::InvalidInput);
        assert_eq!(refused(list_with(50, Some("pdf"))), ErrorKind::InvalidInput);
        assert_eq!(
            refused(Command::Get {
                asset_id: "sys:transformers:T1".into()
            }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            refused(Command::FolderCreate {
                path: "/contracts".into(),
                default_sensitivity: None,
                default_status: None
            }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            refused(Command::Classify {
                asset_id: "a_7kq3nr2v0b1c".into(),
                kind: None,
                status: None,
                sensitivity: None,
                folder_id: None,
                owner: None,
                reason: Some("nothing changes".into()),
                expected_version: 1,
            }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            refused(Command::Attach {
                asset_id: "a_7kq3nr2v0b1c".into(),
                link: json!({"kind": "tag", "definition_id": "city", "value": "x"}),
                detach: false,
            }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            refused(Command::Ingest(IngestRequest {
                name: "a.pdf".into(),
                size: 0,
                sha256: "0".repeat(64),
                content_type: None,
                folder_id: None,
                sensitivity: None,
                kind: "doc".into(),
                format: "pdf".into(),
            })),
            ErrorKind::InvalidInput
        );
        assert!(Command::Ingest(IngestRequest {
            name: "a.pdf".into(),
            size: 10,
            sha256: "a".repeat(64),
            content_type: Some("application/pdf".into()),
            folder_id: Some("f_1".into()),
            sensitivity: Some("confidential".into()),
            kind: "doc".into(),
            format: "pdf".into(),
        })
        .validate("p")
        .is_ok());
    }

    #[test]
    fn a_refusal_carries_the_servers_code_sentence_and_rule() {
        let body = br#"{"success":false,"error":{"code":"ASSET_REFUSED","message":"an ingest may not loosen its folder","details":{"reason":"implicit_loosening","folder_default":"confidential","requested":"internal"}}}"#;
        let error = refusal(400, body);
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        let served = error.service_refusal().expect("carried");
        assert_eq!(served.code(), Some("asset_refused"));
        assert_eq!(
            served.message(),
            Some("an ingest may not loosen its folder [implicit_loosening]")
        );
        let hidden = refusal(
            404,
            br#"{"success":false,"error":{"code":"NOT_FOUND","message":"Asset not found"}}"#,
        );
        assert_eq!(hidden.kind(), ErrorKind::ResourceNotFound);
        let stale = refusal(409, br#"{"success":false,"error":{"code":"CONFLICT","message":"expected_version 2, found 3"}}"#);
        assert_eq!(
            stale.service_refusal().and_then(|r| r.code()),
            Some("conflict")
        );
        let forbidden = refusal(403, br#"{"success":false,"error":{"code":"ASSET_CLASS_FORBIDDEN","message":"restricted needs assets.read.commercial","details":{"sensitivity":"restricted","capability":"assets.read.commercial"}}}"#);
        assert_eq!(forbidden.kind(), ErrorKind::AuthenticationRejected);
        assert_eq!(refusal(501, b"{}").kind(), ErrorKind::RouteUnavailable);
        assert_eq!(refusal(503, b"upstream").kind(), ErrorKind::Transient);
    }
}
