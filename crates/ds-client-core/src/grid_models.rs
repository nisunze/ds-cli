//! Authenticated, bounded project model reads. Signed locators never leave this owner.
//!
//! A project model has versions (v1, v2, …); every save is a revision of the
//! version current when it was saved. Revisions are immutable content; each
//! may carry immutable export records (derived files such as a delivered
//! PLS-CADD `.bak`) pinned to its exact model digest.
use crate::{
    authorized::{classify_tile_error, Authority},
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind, GridModelsCall, ServiceRefusal,
    TransportResponse,
};
use ds_command_kernel::grid_publication::{
    catalog_identifier, sha256_hex, APPROVAL_STATES, MAX_MILESTONE,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
pub const MAX_BYTES: usize = 256 * 1024 * 1024;
pub const PATH: &str = "/api/v1/grid/models";
/// Catalog pages one revision-position walk may read (100 rows each). A
/// version with more saves than this reports its count as a lower bound and
/// claims no ordinal.
pub const MAX_POSITION_PAGES: usize = 20;
/// Every action this owner sends. The gateway registry must allow each.
pub const ACTIONS: [&str; 18] = [
    "list_models",
    "get_model",
    "list_versions",
    "get_version",
    "list_exports",
    "get_export_artifact",
    "get_backup_artifact",
    "start_upload",
    "create_version",
    "bump_version",
    "fork_example",
    "update_model",
    "set_approval",
    "delete_model",
    "restore_model",
    "publish_export",
    "publish_linked",
    "get_linked_generation",
];
/// The refusal code for an action or field a Server predates.
pub const SERVER_ACTION_UNSUPPORTED: &str = "server_action_unsupported";
pub mod catalogue;
pub mod exports;
pub mod governance;
pub mod linked_publication;
pub mod publication;
pub enum Command {
    PublishLinked {
        intent: Box<linked_publication::LinkedIntent>,
        participants: Vec<linked_publication::Participant>,
        checkpoint: Vec<u8>,
    },
    LinkedGeneration {
        graph: String,
        generation: u64,
    },
    List {
        limit: u16,
        cursor: Option<String>,
        include_deleted: bool,
    },
    ListVersions {
        model: String,
        limit: u16,
        cursor: Option<String>,
    },
    Download {
        model: String,
        revision: String,
    },
    Delete {
        model: String,
        expected_revision: String,
        expected_digest: String,
        reason: String,
    },
    Restore {
        model: String,
        expected_revision: String,
        expected_digest: String,
        reason: String,
    },
    Publish {
        /// Boxed: the intent carries the revision's governance fields.
        intent: Box<ds_command_kernel::grid_publication::Intent>,
        bytes: Vec<u8>,
    },
    /// Copy one exact global example revision into a project model.
    ///
    /// The fork body is the governed request the backend validates; this owner
    /// bounds it to one JSON object so the action cannot carry a second
    /// action or an unbounded payload, and names the project separately so the
    /// request can never disagree with the project it was authorized for.
    ForkExample {
        fork: Value,
    },
    /// One model head as the catalog holds it, and where its head sits in
    /// the current version: the version, its revision count and the head's
    /// ordinal within it.
    Show {
        model: String,
    },
    /// One revision's metadata. The signed locator is dropped and no byte is
    /// fetched: parent, reason, milestone, approval, digest, publisher, time
    /// and lineage sources only.
    ShowVersion {
        model: String,
        revision: String,
    },
    /// Where one revision sits in its version: its exact ordinal and the
    /// version's revision count, from a bounded walk of the catalog.
    Position {
        model: String,
        revision: String,
    },
    /// The immutable export records pinned to one revision.
    ListExports {
        model: String,
        revision: String,
    },
    /// Upload one file and record it as a new immutable export of one
    /// revision, pinned to that revision's model digest. The head never moves.
    PublishExport {
        model: String,
        revision: String,
        export: String,
        output: String,
        format: String,
        bytes: Vec<u8>,
    },
    /// Read one export item's exact bytes, verified against the digest its
    /// record declares. A Server without `get_export_artifact` is refused by
    /// name ([`SERVER_ACTION_UNSUPPORTED`]).
    DownloadExport {
        model: String,
        revision: String,
        export: String,
        output: String,
    },
    /// Start the next version whose first revision re-pins the current head's
    /// exact bytes: no upload, no content change. Fenced on the head.
    BumpVersion {
        model: String,
        expected_head: String,
        reason: String,
        milestone: Option<String>,
    },
    /// Rename or re-describe the head. Revisions keep the name they saved.
    UpdateModel {
        model: String,
        expected_head: String,
        display_name: Option<String>,
        description: Option<String>,
        reason: Option<String>,
    },
    /// Record a review decision on one existing revision. The revision stays
    /// immutable; the decision is appended beside it.
    SetApproval {
        model: String,
        revision: String,
        expected_head: String,
        approval: ds_command_kernel::grid_publication::Approval,
    },
    /// Read the head's retirement backup, digest-verified.
    DownloadBackup {
        model: String,
    },
}
pub struct Receipt {
    pub data: Value,
    pub bytes: Option<Vec<u8>>,
}
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
fn id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.trim() == s
        && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\'))
}
fn invalid() -> ClientError {
    ClientError::new(
        ErrorKind::InvalidInput,
        "invalid project model selection or page bound",
    )
}
fn bad() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "model response, storage locator or exact package bytes failed verification",
    )
}
impl Command {
    pub fn validate(&self, project: &str) -> Result<(), ClientError> {
        if !id(project) {
            return Err(invalid());
        }
        match self {
            Self::List { limit, cursor, .. }
                if (1..=100).contains(limit)
                    && cursor.as_ref().is_none_or(|c| {
                        !c.is_empty() && c.len() <= 4096 && !c.chars().any(char::is_control)
                    }) =>
            {
                Ok(())
            }
            Self::ListVersions {
                model,
                limit,
                cursor,
            } if id(model)
                && (1..=100).contains(limit)
                && cursor.as_ref().is_none_or(|c| {
                    !c.is_empty() && c.len() <= 4096 && !c.chars().any(char::is_control)
                }) =>
            {
                Ok(())
            }
            Self::Download { model, revision } if id(model) && id(revision) => Ok(()),
            Self::Delete {
                model,
                expected_revision,
                expected_digest,
                reason,
            }
            | Self::Restore {
                model,
                expected_revision,
                expected_digest,
                reason,
            } if id(model)
                && id(expected_revision)
                && expected_digest.len() == 64
                && expected_digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                && !reason.trim().is_empty()
                && reason.len() <= 2000 =>
            {
                Ok(())
            }
            Self::ForkExample { fork }
                if fork.is_object()
                    && serde_json::to_vec(fork)
                        .is_ok_and(|bytes| !bytes.is_empty() && bytes.len() <= 1024 * 1024) =>
            {
                Ok(())
            }
            Self::Publish { intent, bytes } => intent
                .plan(project, bytes)
                .map(|_| ())
                .map_err(|message| ClientError::new(ErrorKind::InvalidInput, message)),
            Self::PublishLinked {
                intent,
                participants,
                checkpoint,
            } => intent.plan(project, participants, checkpoint).map(|_| ()),
            Self::LinkedGeneration { graph, generation }
                if id(graph) && *generation <= i64::MAX as u64 =>
            {
                Ok(())
            }
            Self::Show { model } if id(model) => Ok(()),
            Self::ShowVersion { model, revision }
            | Self::Position { model, revision }
            | Self::ListExports { model, revision }
                if id(model) && id(revision) =>
            {
                Ok(())
            }
            Self::PublishExport {
                model,
                revision,
                export,
                output,
                format,
                bytes,
            } if id(model)
                && id(revision)
                && [export, output, format]
                    .iter()
                    .all(|value| catalog_identifier(value))
                && !bytes.is_empty()
                && bytes.len() <= MAX_BYTES =>
            {
                Ok(())
            }
            Self::DownloadExport {
                model,
                revision,
                export,
                output,
            } if id(model)
                && id(revision)
                && catalog_identifier(export)
                && catalog_identifier(output) =>
            {
                Ok(())
            }
            Self::BumpVersion { .. }
            | Self::UpdateModel { .. }
            | Self::SetApproval { .. }
            | Self::DownloadBackup { .. } => governance::validate(self),
            Self::PublishExport { .. } | Self::DownloadExport { .. } => Err(ClientError::new(
                ErrorKind::InvalidInput,
                "export, output and format ids must match ^[a-z0-9][a-z0-9_-]{1,127}$ and the file must hold 1 byte to 256 MiB",
            )),
            _ => Err(invalid()),
        }
    }
}
/// One grid-models round trip. Refusals keep the route's own bounded
/// sentence ([`refusal`]); success must be the DS envelope with a data object.
pub(crate) fn request<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    payload: Value,
) -> Result<Value, ClientError> {
    let optional_sent = payload["version"]["bump_version"] == true;
    let action = payload["action"].as_str().unwrap_or_default().to_owned();
    let response = transport.grid_models(GridModelsCall {
        profile,
        authority: authority.call(),
        payload,
    })?;
    decode_response(response, &action, optional_sent)
}

fn decode_response(
    response: TransportResponse,
    action: &str,
    optional_sent: bool,
) -> Result<Value, ClientError> {
    if !matches!(response.status, 200 | 201) {
        return Err(refusal(&response, &action, optional_sent));
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

/// Actions a deployed Server may predate. Its decoder refuses an unknown
/// action, or an unknown field, as a 400 with one of two fixed sentences.
const LATER_ACTIONS: [&str; 5] = [
    "get_export_artifact",
    "get_backup_artifact",
    "bump_version",
    "update_model",
    "set_approval",
];

/// Classify one grid-models refusal, carrying the Server's bounded sentence.
///
/// A 409 is a stored-state conflict (a moved head, an immutable id reused
/// with different content). A 404 names a model, revision or export this
/// project does not hold. A 400 is the Server's own validation, EXCEPT when
/// the request used an action or field a Server may predate and the Server
/// answered with its decoder's fixed sentence: that is reported as
/// [`SERVER_ACTION_UNSUPPORTED`] so the caller learns to update the Server,
/// not to change the request.
pub(crate) fn refusal(
    response: &TransportResponse,
    action: &str,
    optional_sent: bool,
) -> ClientError {
    #[derive(serde::Deserialize, Default)]
    struct Wire {
        #[serde(default)]
        message: String,
        #[serde(default)]
        code: String,
    }
    #[derive(serde::Deserialize)]
    struct Envelope {
        error: Option<Wire>,
    }
    let wire = (response.body.len() <= 64 * 1024)
        .then(|| serde_json::from_slice::<Envelope>(&response.body).ok())
        .flatten()
        .and_then(|envelope| envelope.error)
        .unwrap_or_default();
    let sentence = wire.message;
    let said = (!sentence.is_empty()).then_some(sentence.as_str());
    match response.status {
        409 if wire.code == "composite_model_protected" => ClientError::with_service_refusal(
            ErrorKind::InvalidInput,
            "the automatic combined model is protected; edit its owning submodels",
            ServiceRefusal::new(409, Some("composite_model_protected"), said),
        ),
        409 => ClientError::with_service_refusal(
            ErrorKind::InvalidInput,
            "grid publication conflicts with stored state; the request was not changed or retried",
            ServiceRefusal::new(
                409,
                Some(if action == "publish_export" {
                    "grid_export_conflict"
                } else {
                    "grid_publication_conflict"
                }),
                said,
            ),
        ),
        404 => ClientError::with_service_refusal(
            ErrorKind::ResourceNotFound,
            "the project model, revision or export does not exist in this project",
            ServiceRefusal::new(404, Some("grid_model_not_found"), said),
        ),
        400 if (LATER_ACTIONS.contains(&action) || optional_sent)
            && matches!(
                sentence.as_str(),
                "Unknown DS Grid model action" | "Invalid DS Grid model request"
            ) =>
        {
            ClientError::with_service_refusal(
                ErrorKind::InvalidInput,
                "this Server predates the requested DS Grid model action or field",
                ServiceRefusal::new(400, Some(SERVER_ACTION_UNSUPPORTED), said),
            )
        }
        400 => ClientError::with_service_refusal(
            ErrorKind::InvalidInput,
            "the DS Grid model service refused the request",
            ServiceRefusal::new(400, Some("grid_request_refused"), said),
        ),
        status => classify_tile_error(status),
    }
}

/// The metadata of one revision read with `get_version`; the signed locator
/// is removed before anything leaves this owner.
fn revision_record<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    model: &str,
    revision: &str,
) -> Result<Value, ClientError> {
    revision_read(profile, transport, authority, project, model, revision).map(|(r, _)| r)
}

/// The post-creation approval a Server that records decisions reports beside
/// one revision.
type ApprovalFields = Vec<(&'static str, Value)>;

/// [`revision_record`] plus `effective_approval` and `approval_decisions`,
/// when the Server reports them.
fn revision_read<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    model: &str,
    revision: &str,
) -> Result<(Value, ApprovalFields), ClientError> {
    let data = request(
        profile,
        transport,
        authority,
        json!({"action":"get_version","project_id":project,"model_id":model,"revision_id":revision}),
    )?;
    let record = data
        .get("revision")
        .or_else(|| data.get("version"))
        .filter(|v| v.is_object())
        .ok_or_else(bad)?;
    if record["project_id"] != project
        || record["model_id"] != model
        || record["revision_id"] != revision
        || !record["version"].as_u64().is_some_and(|v| v > 0)
        || !record["model"]["digest"].as_str().is_some_and(sha256_hex)
    {
        return Err(bad());
    }
    let mut approval = Vec::new();
    if data["effective_approval"].is_object() {
        approval.push(("effective_approval", data["effective_approval"].clone()));
    }
    if let Some(decisions) = data["approval_decisions"].as_array() {
        if decisions.len() > 200
            || decisions
                .iter()
                .any(|d| d["revision_id"] != revision || d["model_id"] != model)
        {
            return Err(bad());
        }
        approval.push(("approval_decisions", json!(decisions)));
    }
    Ok((record.clone(), approval))
}

/// Walk at most [`MAX_POSITION_PAGES`] to the version's newest revision,
/// whose stored ordinal is its revision count.
#[allow(clippy::too_many_arguments)]
fn walk<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    model: &str,
    revision: &str,
    version: u64,
    stored: Option<u64>,
) -> Result<Value, ClientError> {
    let ordinal = stored.ok_or_else(bad)?;
    let mut cursor = String::new();
    let mut pages = 0;
    while pages < MAX_POSITION_PAGES {
        let data = request(
            profile,
            transport,
            authority,
            json!({"action":"list_versions","project_id":project,"model_id":model,"page_size":100,"cursor":cursor}),
        )?;
        pages += 1;
        let page = data["versions"].as_array().ok_or_else(bad)?;
        let more = data["has_more"].as_bool().ok_or_else(bad)?;
        if page.len() > 100
            || page
                .iter()
                .any(|row| row["project_id"] != project || row["model_id"] != model)
        {
            return Err(bad());
        }
        let lower = page
            .iter()
            .any(|row| row["version"].as_u64().is_some_and(|v| v < version));
        if let Some(row) = page
            .iter()
            .find(|row| row["version"].as_u64() == Some(version))
        {
            let count = ds_command_kernel::grid_versions::stored_ordinal(row).ok_or_else(bad)?;
            return Ok(json!({"version":version,"revision_id":revision,
                "revision_ordinal_within_version":ordinal,"version_revision_count":count,
                "count_exact":true,"pages_read":pages}));
        }
        if lower || !more {
            break;
        }
        cursor = data["next_cursor"]
            .as_str()
            .filter(|c| !c.is_empty() && c.len() <= 4096)
            .ok_or_else(bad)?
            .to_owned();
    }
    Err(ClientError::new(
        ErrorKind::UnreadableResponse,
        "the revision is not listed in its own version within the bounded catalog walk",
    ))
}

/// The head projection `show` and `list` share: the catalog row without
/// backup storage coordinates, which are an owner-internal locator.
fn head_projection(row: &Value) -> Value {
    let mut head = json!({});
    for key in [
        "model_id",
        "display_name",
        "description",
        "model_kind",
        "state",
        "lifecycle",
        "head_revision_id",
        "head_version",
        "head_model_digest",
        "manifest_model_id",
        "manifest_model_revision",
        "design_stage_id",
        "detail_level_id",
        "approval_status",
        "approval_level_id",
        "created_at",
        "created_by",
        "updated_at",
        "updated_by",
        "deleted_at",
        "deleted_by",
        "delete_reason",
    ] {
        if let Some(value) = row.get(key).filter(|v| !v.is_null()) {
            head[key] = value.clone();
        }
    }
    head
}

pub(crate) fn locator<'a>(
    url: &'a str,
    object: &str,
    size: usize,
) -> Result<DownloadCall<'a>, ClientError> {
    let u = url::Url::parse(url).map_err(|_| bad())?;
    if url.len() > 8192
        || u.scheme() != "https"
        || u.host_str() != Some("storage.googleapis.com")
        || u.port().is_some()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
        || size == 0
        || size > MAX_BYTES
    {
        return Err(bad());
    }
    let path = u.path().trim_start_matches('/');
    let (_, tail) = path.split_once('/').ok_or_else(bad)?;
    if tail != object
        || !object.starts_with("ds-grid/objects/sha256/")
        || object
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(bad());
    }
    Ok(DownloadCall { url, size })
}

/// A short-lived signed read of one exact storage object the server named by
/// bucket and object — a verified delete backup or a listed report artifact.
/// The locator must be an https URL on the storage host whose path is exactly
/// that bucket and object, with no credentials or fragment; `size` bounds the
/// read. It travels the same signed-object byte transport as a model package.
pub(crate) fn signed_object_locator<'a>(
    url: &'a str,
    bucket: &str,
    object: &str,
    size: usize,
) -> Result<DownloadCall<'a>, ClientError> {
    let u = url::Url::parse(url).map_err(|_| bad())?;
    if url.len() > 8192
        || u.scheme() != "https"
        || u.host_str() != Some("storage.googleapis.com")
        || u.port().is_some()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
        || size == 0
        || size > MAX_BYTES
        || bucket.is_empty()
        || bucket.contains('/')
        || object.is_empty()
        || object
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(bad());
    }
    let mut expected = url::Url::parse("https://storage.googleapis.com/").map_err(|_| bad())?;
    expected
        .path_segments_mut()
        .map_err(|_| bad())?
        .clear()
        .push(bucket)
        .extend(object.split('/'));
    if u.path() != expected.path() {
        return Err(bad());
    }
    Ok(DownloadCall { url, size })
}
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Receipt, ClientError> {
    command.validate(project)?;
    match command {
        Command::PublishLinked {
            intent,
            participants,
            checkpoint,
        } => {
            return linked_publication::run(
                profile,
                transport,
                authority,
                project,
                intent,
                participants,
                checkpoint,
            );
        }
        Command::LinkedGeneration { graph, generation } => {
            return linked_publication::read(
                profile,
                transport,
                authority,
                project,
                graph,
                *generation,
            );
        }
        Command::Publish { intent, bytes } => {
            return publication::run(profile, transport, authority, project, intent, bytes)
        }
        Command::Show { model } => {
            let data = request(
                profile,
                transport,
                authority,
                json!({"action":"get_model","project_id":project,"model_id":model}),
            )?;
            let row = data
                .get("model")
                .filter(|m| m.is_object())
                .ok_or_else(bad)?;
            let head = row["head_revision_id"].as_str().filter(|h| id(h));
            let version = row["head_version"].as_u64().filter(|v| *v > 0);
            if row["project_id"] != project || row["model_id"] != *model {
                return Err(bad());
            }
            // The head is its version's newest revision; the Server supplies its count.
            let count = row["head_version_revision_count"]
                .as_u64()
                .filter(|n| *n > 0);
            let current = match (head, version, count) {
                (Some(head), Some(version), Some(count)) => {
                    json!({"version":version,"revision_id":head,
                        "revision_ordinal_within_version":count,"version_revision_count":count,
                        "count_exact":true,"pages_read":0})
                }
                (Some(_), Some(_), None) => return Err(bad()),
                _ => Value::Null,
            };
            // Deliberate LV/MV version markers are independent of legacy
            // content revision groups. Both reads retain this exact authority.
            let named = crate::design_versions::run(
                profile,
                transport,
                authority,
                project,
                &crate::design_versions::Command::Object {
                    kind: "mv_model".into(),
                    command: Box::new(crate::design_versions::Command::Status {
                        transformer: model.clone(),
                    }),
                },
            )?;
            let management = ds_command_kernel::grid_model_management::project(
                &json!({"project":project,"model":model,"head":row,"named_head":named["head"]}),
            )
            .map_err(|_| bad())?;
            return Ok(Receipt {
                data: json!({"project":project,"model":head_projection(row),"current_version":current,"management":management}),
                bytes: None,
            });
        }
        Command::ShowVersion { model, revision } => {
            let (record, approval) =
                revision_read(profile, transport, authority, project, model, revision)?;
            let mut data = json!({"project":project,"revision":record,"bytes_fetched":false});
            // A Server that records approval decisions after creation names
            // the effective approval and the decision history beside the
            // immutable revision; an older one names neither.
            for (key, value) in approval {
                data[key] = value;
            }
            return Ok(Receipt { data, bytes: None });
        }
        Command::Position { model, revision } => {
            let record = revision_record(profile, transport, authority, project, model, revision)?;
            let version = record["version"].as_u64().ok_or_else(bad)?;
            let stored = ds_command_kernel::grid_versions::stored_ordinal(&record);
            let mut data = walk(
                profile, transport, authority, project, model, revision, version, stored,
            )?;
            data["project"] = json!(project);
            data["model_id"] = json!(model);
            return Ok(Receipt { data, bytes: None });
        }
        Command::ListExports { model, revision } => {
            let exports = exports::list(profile, transport, authority, project, model, revision)?;
            return Ok(Receipt {
                data: json!({"project":project,"model_id":model,"revision_id":revision,"exports":exports}),
                bytes: None,
            });
        }
        Command::PublishExport { .. } | Command::DownloadExport { .. } => {
            return exports::run(profile, transport, authority, project, command)
        }
        Command::BumpVersion { .. }
        | Command::UpdateModel { .. }
        | Command::SetApproval { .. }
        | Command::DownloadBackup { .. } => {
            return governance::run(profile, transport, authority, project, command)
        }
        _ => {}
    }
    let payload = match command {
        Command::List {
            limit,
            cursor,
            include_deleted,
        } => list_payload(project, *limit, cursor.as_deref(), *include_deleted),
        Command::ListVersions {
            model,
            limit,
            cursor,
        } => {
            json!({"action":"list_versions","project_id":project,"model_id":model,"page_size":limit,"cursor":cursor.as_deref().unwrap_or("")})
        }
        Command::Download { model, revision } => {
            json!({"action":"get_version","project_id":project,"model_id":model,"revision_id":revision})
        }
        Command::Delete {
            model,
            expected_revision,
            expected_digest,
            reason,
        } => {
            json!({"action":"delete_model","project_id":project,"delete":{"model_id":model,"expected_head_revision_id":expected_revision,"expected_model_digest":expected_digest,"reason":reason}})
        }
        Command::Restore {
            model,
            expected_revision,
            expected_digest,
            reason,
        } => {
            json!({"action":"restore_model","project_id":project,"restore":{"model_id":model,"expected_head_revision_id":expected_revision,"expected_model_digest":expected_digest,"reason":reason}})
        }
        Command::ForkExample { fork } => {
            json!({"action":"fork_example","project_id":project,"fork":fork})
        }
        _ => unreachable!("composite commands dispatched before single reads"),
    };
    let response = transport.grid_models(GridModelsCall {
        profile,
        authority: authority.call(),
        payload,
    })?;
    if response.status != 200 {
        return Err(refusal(&response, "", false));
    }
    if response.body.len() > 8 * 1024 * 1024 {
        return Err(bad());
    }
    let body: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    if body["success"] != true {
        return Err(bad());
    }
    let data = &body["data"];
    match command {
        Command::Delete {
            model,
            expected_revision,
            expected_digest,
            ..
        } => {
            let deletion = data.get("deletion").ok_or_else(bad)?;
            if deletion["project_id"] != project
                || deletion["model_id"] != *model
                || deletion["head_revision_id"] != *expected_revision
                || deletion["head_model_digest"] != *expected_digest
                || deletion["revisions_retained"] != true
                || deletion["model_object_retained"] != true
                || deletion["backup"]["digest"] != *expected_digest
                || deletion["backup"]["generation"]
                    .as_i64()
                    .is_none_or(|value| value <= 0)
                || deletion["backup"]["bucket"]
                    .as_str()
                    .is_none_or(str::is_empty)
                || deletion["backup"]["object"]
                    .as_str()
                    .is_none_or(str::is_empty)
            {
                return Err(bad());
            }
            Ok(Receipt {
                data: deletion.clone(),
                bytes: None,
            })
        }
        Command::Restore {
            model,
            expected_revision,
            expected_digest,
            ..
        } => {
            let restoration = data.get("restoration").ok_or_else(bad)?;
            if restoration["project_id"] != project
                || restoration["model_id"] != *model
                || restoration["head_revision_id"] != *expected_revision
                || restoration["head_model_digest"] != *expected_digest
                || restoration["backup"]["digest"] != *expected_digest
            {
                return Err(bad());
            }
            Ok(Receipt {
                data: restoration.clone(),
                bytes: None,
            })
        }
        // The fork's answer is the created project version, verified by the
        // backend that created it. This owner adds no second opinion; it only
        // refuses an envelope that does not carry one.
        Command::ForkExample { .. } => {
            let version = data
                .get("version")
                .filter(|value| value.is_object())
                .cloned()
                .unwrap_or_else(|| data.clone());
            if !version.is_object() || version["project_id"] != project {
                return Err(bad());
            }
            Ok(Receipt {
                data: version,
                bytes: None,
            })
        }
        Command::ListVersions { model, limit, .. } => {
            let rows = data["versions"].as_array().ok_or_else(bad)?;
            let more = data["has_more"].as_bool().ok_or_else(bad)?;
            let cursor = data["next_cursor"].as_str().unwrap_or("");
            if rows.len() > usize::from(*limit)
                || (more && cursor.is_empty())
                || cursor.len() > 4096
                || rows.iter().any(|row| {
                    row["project_id"] != project
                        || row["model_id"] != *model
                        || !row["revision_id"].is_string()
                })
            {
                return Err(bad());
            }
            // Revisions are grouped by the version they were saved into;
            // the kernel orders each group by its stored ordinals.
            let versions = ds_command_kernel::grid_versions::group_page(rows).map_err(|_| bad())?;
            Ok(Receipt {
                data: json!({"project":project,"model_id":model,"versions":versions,
                    "revisions_listed":rows.len(),"more":more,"next_cursor":cursor}),
                bytes: None,
            })
        }
        Command::List { limit, .. } => list_receipt(project, *limit, &data),
        Command::Download { model, revision } => {
            let v = data
                .get("revision")
                .or_else(|| data.get("version"))
                .ok_or_else(bad)?;
            if v["project_id"] != project
                || v["model_id"] != *model
                || v["revision_id"] != *revision
            {
                return Err(bad());
            }
            let digest = v["model"]["digest"]
                .as_str()
                .ok_or_else(bad)?
                .trim_start_matches("sha256:");
            if digest.len() != 64
                || !digest
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            {
                return Err(bad());
            }
            let size = v["model"]["byte_length"]
                .as_u64()
                .filter(|n| *n > 0 && *n <= MAX_BYTES as u64)
                .ok_or_else(bad)? as usize;
            let object = v["model"]["object"].as_str().ok_or_else(bad)?;
            let url = data["download_url"].as_str().ok_or_else(bad)?;
            let mut response = transport.grid_model_bytes(locator(url, object, size)?)?;
            if response.status != 200
                || response.body.len() != size
                || format!("{:x}", Sha256::digest(&response.body)) != digest
            {
                return Err(bad());
            }
            Ok(Receipt {
                data: json!({"project":project,"model_id":model,"revision_id":revision,"sha256":digest,"bytes":size,"verified":true}),
                bytes: Some(std::mem::take(&mut response.body)),
            })
        }
        _ => unreachable!("composite commands dispatched before single reads"),
    }
}
/// The model digest of one exact governed revision, read from the catalogue
/// and never fetched as bytes: what a design attachment pins beside the
/// content revision id, so the binding names the package it was published
/// against. The signed locator in the same answer is discarded here.
pub(crate) fn revision_digest<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    model: &str,
    revision: &str,
) -> Result<String, ClientError> {
    if !id(project) || !id(model) || !id(revision) {
        return Err(invalid());
    }
    let response = transport.grid_models(GridModelsCall {
        profile,
        authority: authority.call(),
        payload: json!({"action":"get_version","project_id":project,"model_id":model,"revision_id":revision}),
    })?;
    if response.status != 200 {
        return Err(classify_tile_error(response.status));
    }
    if response.body.len() > 8 * 1024 * 1024 {
        return Err(bad());
    }
    let body: Value = serde_json::from_slice(&response.body).map_err(|_| bad())?;
    let v = body["data"]
        .get("revision")
        .or_else(|| body["data"].get("version"))
        .filter(|_| body["success"] == true)
        .ok_or_else(bad)?;
    if v["project_id"] != project || v["model_id"] != model || v["revision_id"] != revision {
        return Err(bad());
    }
    let digest = v["model"]["digest"]
        .as_str()
        .ok_or_else(bad)?
        .trim_start_matches("sha256:");
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(bad());
    }
    Ok(digest.to_owned())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_action_this_owner_sends_is_a_declared_gateway_action() {
        let operation = crate::gateway::operation("domains.grid.models.action").unwrap();
        for action in ACTIONS {
            assert!(operation.allows_action(action), "{action}");
        }
    }
    #[test]
    fn refusals_keep_the_route_sentence_and_name_a_server_that_predates_an_action() {
        let body = |message: &str| {
            serde_json::to_vec(
                &json!({"success":false,"error":{"code":"VALIDATION_FAILED","message":message}}),
            )
            .unwrap()
        };
        let unknown = TransportResponse::new(400, body("Unknown DS Grid model action"));
        let refused = refusal(&unknown, "get_export_artifact", false);
        assert_eq!(
            refused.service_refusal().unwrap().code(),
            Some(SERVER_ACTION_UNSUPPORTED)
        );
        // The same sentence for an action every Server serves is plain validation.
        let refused = refusal(&unknown, "list_exports", false);
        assert_eq!(
            refused.service_refusal().unwrap().code(),
            Some("grid_request_refused")
        );
        // An explicit bump sent to a Server whose decoder predates the field.
        let undecodable = TransportResponse::new(400, body("Invalid DS Grid model request"));
        let refused = refusal(&undecodable, "create_version", true);
        assert_eq!(
            refused.service_refusal().unwrap().code(),
            Some(SERVER_ACTION_UNSUPPORTED)
        );
        let advance = TransportResponse::new(
            400,
            body("invalid project grid model: manifest_model_revision must advance beyond 4"),
        );
        let refused = refusal(&advance, "create_version", false);
        assert!(refused
            .service_refusal()
            .unwrap()
            .message()
            .unwrap()
            .contains("must advance"));
        let missing = refusal(&TransportResponse::new(404, b"{}"), "get_model", false);
        assert_eq!(missing.kind(), ErrorKind::ResourceNotFound);
        assert_eq!(
            missing.service_refusal().unwrap().code(),
            Some("grid_model_not_found")
        );
        let reused = refusal(&TransportResponse::new(409, b"{}"), "publish_export", false);
        assert_eq!(
            reused.service_refusal().unwrap().code(),
            Some("grid_export_conflict")
        );
    }
    #[test]
    fn protected_combined_refusal_preserves_every_owner_and_code() {
        let response = TransportResponse::new(409, serde_json::to_vec(&json!({
            "error": {"code": "composite_model_protected", "message": "edit north, south, additional"}
        })).unwrap());
        for action in [
            "create_version",
            "update_model",
            "set_approval",
            "bump_version",
            "delete_model",
            "restore_model",
        ] {
            let error = refusal(&response, action, false);
            let refusal = error.service_refusal().unwrap();
            assert_eq!(refusal.code(), Some("composite_model_protected"));
            assert_eq!(refusal.message(), Some("edit north, south, additional"));
        }
        // An unrecognized server code never acquires this policy by its message.
        let ordinary = TransportResponse::new(
            409,
            serde_json::to_vec(&json!({"error":{"code":"CONFLICT","message":"edit north, south"}}))
                .unwrap(),
        );
        assert_eq!(
            refusal(&ordinary, "create_version", false)
                .service_refusal()
                .unwrap()
                .code(),
            Some("grid_publication_conflict")
        );
    }

    #[test]
    fn export_ids_follow_the_catalog_grammar() {
        let publish = |export: &str| Command::PublishExport {
            model: "model-1".into(),
            revision: "rev-1".into(),
            export: export.into(),
            output: "pls-delivered-workspace".into(),
            format: "pls_cadd_bak".into(),
            bytes: b"bak".to_vec(),
        };
        assert!(publish("submission-2").validate("project").is_ok());
        assert!(publish("Submission 2").validate("project").is_err());
        assert!(Command::DownloadExport {
            model: "model-1".into(),
            revision: "rev-1".into(),
            export: "submission-2".into(),
            output: "../x".into(),
        }
        .validate("project")
        .is_err());
    }
    #[test]
    fn storage_locator_cannot_redirect_credentials_or_substitute_objects() {
        let object = "ds-grid/objects/sha256/ab/model.dsgrid";
        assert!(locator(
            &format!("https://storage.googleapis.com/bucket/{object}?X-Goog-Signature=issued"),
            object,
            12
        )
        .is_ok());
        for url in [
            "http://storage.googleapis.com/bucket/x",
            "https://evil.example/x",
            "https://storage.googleapis.com/bucket/wrong",
            "https://user@storage.googleapis.com/bucket/x",
        ] {
            assert!(locator(url, object, 12).is_err());
        }
        assert!(Command::List {
            limit: 101,
            cursor: None,
            include_deleted: true,
        }
        .validate("project")
        .is_err());
        assert!(Command::Download {
            model: "../x".into(),
            revision: "r".into()
        }
        .validate("project")
        .is_err());
        assert!(Command::Delete {
            model: "model-1".into(),
            expected_revision: "revision-1".into(),
            expected_digest: "a".repeat(64),
            reason: "Superseded by the reviewed model".into(),
        }
        .validate("project")
        .is_ok());
        assert!(Command::Restore {
            model: "model-1".into(),
            expected_revision: "revision-1".into(),
            expected_digest: "a".repeat(64),
            reason: "Needed again".into(),
        }
        .validate("project")
        .is_ok());
        assert!(Command::ListVersions {
            model: "model-1".into(),
            limit: 50,
            cursor: None,
        }
        .validate("project")
        .is_ok());
        assert!(Command::Delete {
            model: "model-1".into(),
            expected_revision: "revision-1".into(),
            expected_digest: "a".repeat(64),
            reason: " ".into(),
        }
        .validate("project")
        .is_err());
    }
}

fn list_payload(project: &str, limit: u16, cursor: Option<&str>, include_deleted: bool) -> Value {
    json!({"action":"list_models","project_id":project,"page_size":limit,"cursor":cursor.unwrap_or(""),"include_deleted":include_deleted})
}

fn list_receipt(project: &str, limit: u16, data: &Value) -> Result<Receipt, ClientError> {
    let rows = if data["models"].is_null() {
        vec![]
    } else {
        data["models"].as_array().ok_or_else(bad)?.clone()
    };
    let more = data["has_more"].as_bool().ok_or_else(bad)?;
    let cursor = data["next_cursor"].as_str().unwrap_or("");
    if rows.len() > usize::from(limit) || cursor.len() > 4096 || (more && cursor.is_empty()) {
        return Err(bad());
    }
    let mut ids = std::collections::HashSet::new();
    let mut models = Vec::new();
    for row in rows {
        let model = row["model_id"].as_str().filter(|v| id(v)).ok_or_else(bad)?;
        if !ids.insert(model.to_owned()) || row["project_id"] != project {
            return Err(bad());
        }
        // The head projection plus the retirement backup identity a
        // restore names. `head_version` is the model's current
        // version (v1, v2, …).
        let mut entry = head_projection(&row);
        entry["backup"] = row["backup"].clone();
        models.push(entry);
    }
    Ok(Receipt {
        data: json!({"project":project,"models":models,"more":more,"next_cursor":cursor}),
        bytes: None,
    })
}
