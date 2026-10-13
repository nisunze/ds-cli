//! Project management, headlessly.
//!
//! The plan, its tasks, milestones, notes and records were reachable from
//! exactly one place: a browser window. All nine `ds pm` commands declared
//! `Requires::Window` and relayed through the desktop bridge to the page's own
//! adapters — so an operator on a server, or an agent with no display, could
//! not read a project's plan at all.
//!
//! Nothing about these reads needs a window. `POST /api/v1/pm` is already
//! published on both gateway lanes with its full action vocabulary, and
//! ds-brain's PM handler authenticates from the bearer with no pairing and no
//! device. The window was habit, not necessity.
//!
//! **This module is the DOOR, not the decisions.** It sends a closed set of
//! actions and returns what the server said. What a graph MEANS — the
//! dashboard, the phases, the attention list, the vocabulary — belongs to
//! `ds_command_kernel::project_management`, so the CLI, the Server and the
//! browser fold one answer rather than three.
//!
//! The path is written here rather than declared in the client profile, like
//! `/api/v1/assets` and `/api/v1/governance` beside it. The profile supplies
//! the gateway origin and the credentials; declaring a route there would bump
//! `CLIENT_PROFILE_SCHEMA` and strand every installed `ds` until a package is
//! cut, for nothing this needs.
//!
//! Closed by construction, like every other governed door in this crate: the
//! [`Command`] enum is the complete set of actions this owner can send, and
//! ds-brain decodes with `DisallowUnknownFields`, so a key this file does not
//! write is a key the request cannot carry.

use serde_json::{Map, Value, json};

pub mod checklist;
pub mod create_attached;
pub mod documents;
mod model_references;
mod overview;
pub mod project_notes;

use crate::{
    AuthorizedTransport, ClientError, ClientProfile, ErrorKind, ProjectManagementCall,
    ServiceRefusal, authorized::Authority,
};

/// The largest plan answer this owner will read. A project's whole graph —
/// tasks, milestones, records, residuals — is bigger than an inventory page
/// and smaller than a design room.
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// The most context rows per collection one `get_context` may ask for —
/// ds-brain's `maxPMContextLimit`. Above it the server refuses by name, so the
/// door refuses first and says the same number.
pub const MAX_CONTEXT_LIMIT: i64 = 100;
/// The longest command id one commit may carry: a UUID, or the `pm-…` form the
/// browser minted before it had one.
const MAX_COMMAND_ID_BYTES: usize = 128;
/// Bound a minted ID while keeping the server's stable-reference alphabet.
const MAX_PROJECT_NOTE_ID_BYTES: usize = 256;
/// The most commands one saved draft may carry — ds-brain's own batch bound,
/// republished per project as `field_model.max_batch_commands`.
pub const MAX_BATCH_COMMANDS: usize = 200;
/// The largest file one task attachment may carry — ds-brain's
/// `maxPMAttachmentBytes` (100 MiB).
pub const MAX_ATTACHMENT_BYTES: usize = 100 << 20;
/// A stored attachment name is at most this many UTF-8 bytes.
const MAX_ATTACHMENT_NAME_BYTES: usize = 255;

/// One file a task attachment uploads, checked once when it is made: a plain
/// file name, 1 byte to 100 MiB, and a media type (given, or named from the
/// file extension the way a browser names `File.type`). `Debug` prints the
/// size, never the bytes.
#[derive(Clone, PartialEq)]
pub struct AttachmentFile {
    name: String,
    content_type: String,
    bytes: Vec<u8>,
}

impl std::fmt::Debug for AttachmentFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AttachmentFile")
            .field("name", &self.name)
            .field("content_type", &self.content_type)
            .field("bytes", &self.bytes.len())
            .finish()
    }
}

impl AttachmentFile {
    pub fn new(
        name: &str,
        content_type: Option<&str>,
        bytes: Vec<u8>,
    ) -> Result<Self, ClientError> {
        let invalid = |message: &'static str| ClientError::new(ErrorKind::InvalidInput, message);
        let name = name.trim();
        if name.is_empty()
            || name.len() > MAX_ATTACHMENT_NAME_BYTES
            || name == "."
            || name == ".."
            || name
                .chars()
                .any(|c| c.is_control() || matches!(c, '/' | '\\'))
        {
            return Err(invalid(
                "a task attachment needs a plain file name of at most 255 UTF-8 bytes",
            ));
        }
        if bytes.is_empty() || bytes.len() > MAX_ATTACHMENT_BYTES {
            return Err(invalid("a task attachment is from 1 byte to 100 MiB"));
        }
        let content_type = match content_type.map(str::trim) {
            Some(given) => {
                let lowered = given.to_ascii_lowercase();
                let valid = !lowered.is_empty()
                    && lowered.len() <= 200
                    && lowered.split_once('/').is_some_and(|(kind, sub)| {
                        !kind.is_empty() && !sub.is_empty() && !sub.contains('/')
                    })
                    && !lowered
                        .chars()
                        .any(|c| c.is_whitespace() || c.is_control() || c == ';');
                if !valid {
                    return Err(invalid(
                        "a task attachment content type is one `type/subtype`",
                    ));
                }
                lowered
            }
            None => attachment_media_type(name).to_owned(),
        };
        Ok(Self {
            name: name.to_owned(),
            content_type,
            bytes,
        })
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn content_type(&self) -> &str {
        &self.content_type
    }
    pub fn len(&self) -> usize {
        self.bytes.len()
    }
    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }
    /// Lowercase hex SHA-256 of the bytes this attachment uploads.
    pub fn sha256(&self) -> String {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(&self.bytes))
    }
}

/// The media type a browser would name for this file extension; anything
/// unrecognised is `application/octet-stream`, as the server defaults it.
fn attachment_media_type(name: &str) -> &'static str {
    let extension = name
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase())
        .unwrap_or_default();
    match extension.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "tif" | "tiff" => "image/tiff",
        "csv" => "text/csv",
        "txt" => "text/plain",
        "md" => "text/markdown",
        "json" => "application/json",
        "geojson" => "application/geo+json",
        "zip" => "application/zip",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "kml" => "application/vnd.google-earth.kml+xml",
        "kmz" => "application/vnd.google-earth.kmz",
        _ => "application/octet-stream",
    }
}

/// One governed project-management action.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Voluntary labels on one explicit supported write, never ambient identity.
    Authored {
        model: ds_command_kernel::project_management::authoring_model::AuthoringModel,
        command: Box<Command>,
    },
    /// Closed shared project memory through the same fixed PM authority.
    ProjectNotes { command: project_notes::Command },
    /// The project's whole canonical graph, at its current revision.
    ///
    /// There is no narrower read: ds-brain answers the graph whole, and every
    /// projection — the plan, a task list, one task — is a fold over it. A
    /// per-task endpoint would be a second answer that could disagree with
    /// this one.
    Graph,
    /// Project-wide task/milestone/record/project-note overview, two exact reads.
    Overview {
        options: ds_command_kernel::project_management::overview::Options,
    },
    /// Existing authenticated collaboration index, independent of saved selection.
    PublicProjects {
        status: String,
        cursor: String,
        limit: i64,
    },
    /// Existing task comment action; no project-chat authoring surface.
    TaskCommentCreate {
        task_id: String,
        id: String,
        body: String,
    },
    TaskComments {
        task_id: String,
        cursor: String,
        limit: i64,
    },
    /// The bounded project-side directory of people the server will accept
    /// for a Project Work assignment. It is separate from graph hydration so
    /// ordinary plan reads do not pay for a roster scan.
    Assignees,
    /// Exact model references from one graph and one readable pm_records page.
    /// Both reads use the same captured authority; context has its own versions.
    ModelReferences {
        model_id: String,
        revision_id: Option<String>,
        records_cursor: Option<String>,
    },
    /// Only the selected task's readable correspondence, using the existing
    /// indexed related_task_ids query; unused context collections are not read.
    TaskRecords { task_id: String, limit: i64 },
    /// One project command against one base revision — `commit_command`, the
    /// same body the Plan sheet sends for one gesture. The command is the
    /// kernel's (`ds_command_kernel::project_management::writes`), already
    /// validated against the graph it was authored on; ds-brain rechecks
    /// permission, membership and revision and applies it or refuses it whole.
    Commit {
        command_id: String,
        base_revision: i64,
        command: Value,
    },
    /// Several commands as one saved draft — `commit_batch`: one base
    /// revision, applied atomically or refused whole naming the command that
    /// failed.
    CommitBatch {
        command_id: String,
        base_revision: i64,
        commands: Vec<Value>,
    },
    Propose {
        command_id: String,
        base_revision: i64,
        task_id: String,
        title: String,
        description: Option<String>,
        discipline: Option<String>,
        responsible_email: String,
        estimated_hours: f64,
        estimated_days: Option<i64>,
    },
    RequestAdmission {
        command_id: String,
        base_revision: i64,
        task_id: String,
        note: Option<String>,
    },
    Admit {
        command_id: String,
        base_revision: i64,
        task_id: String,
        parent_task_id: String,
        target_index: Option<i64>,
    },
    DeclineAdmission {
        command_id: String,
        base_revision: i64,
        task_id: String,
        reason: String,
    },
    LogHours {
        command_id: String,
        base_revision: i64,
        task_id: String,
        hours: f64,
        note: Option<String>,
    },
    /// A task creation followed by verified task-owned uploads. Creation is
    /// durable independently of upload success; never silently retry either.
    TaskCreateAttached {
        command_id: String,
        base_revision: i64,
        command: Value,
        task_id: String,
        files: Vec<AttachmentFile>,
    },
    /// Attach one file to one task through the task attachment routes, as one
    /// command: `start_attachment_upload` reserves the row and mints one
    /// resumable session, the bytes go straight to storage, and
    /// `finalize_attachment_upload` makes ds-brain verify the stored object
    /// (size and type) before the row turns `ready`.
    TaskAttach {
        task_id: String,
        file: AttachmentFile,
    },
    /// One exact task relationship detach; retained file bytes are not deleted.
    TaskDetach {
        request: ds_command_kernel::project_management::task_attachment_detach::Request,
    },
}

fn command_id_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_COMMAND_ID_BYTES
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

pub(crate) fn project_note_id_valid(id: &str) -> bool {
    // Match `validPMReference`, rather than the narrower command-ID alphabet:
    // stable record IDs may contain Unicode and punctuation such as ':' or '@'.
    !id.is_empty()
        && id.len() <= MAX_PROJECT_NOTE_ID_BYTES
        && id != "."
        && id != ".."
        && !id.contains('/')
        && !id.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn wire_command_valid(command: &Value) -> bool {
    // `{kind, <kind>: {…}}` and nothing else: the shape `projectCommandToWire`
    // produces and ds-brain decodes under `DisallowUnknownFields`.
    let Some(object) = command.as_object() else {
        return false;
    };
    let Some(kind) = object.get("kind").and_then(Value::as_str) else {
        return false;
    };
    !kind.is_empty() && object.len() == 2 && object.get(kind).is_some_and(Value::is_object)
}

impl Command {
    pub fn with_authoring(
        self,
        model: Option<ds_command_kernel::project_management::authoring_model::AuthoringModel>,
    ) -> Result<Self, ClientError> {
        let Some(model) = model else {
            return Ok(self);
        };
        model.validate().map_err(|_| {
            ClientError::new(
                ErrorKind::InvalidInput,
                "invalid self-declared authoring model",
            )
        })?;
        if !self.supports_authoring() {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "this PM action does not support authoring declarations",
            ));
        }
        Ok(Self::Authored {
            model,
            command: Box::new(self),
        })
    }
    fn supports_authoring(&self) -> bool {
        matches!(
            self,
            Self::Commit { .. }
                | Self::CommitBatch { .. }
                | Self::TaskCommentCreate { .. }
                | Self::Propose { .. }
                | Self::TaskCreateAttached { .. }
                | Self::RequestAdmission { .. }
                | Self::LogHours { .. }
        ) || matches!(self, Self::ProjectNotes{command} if matches!(command, project_notes::Command::Write(_)|project_notes::Command::Consolidate(_)|project_notes::Command::Backup(_)|project_notes::Command::Retire(_)|project_notes::Command::Restore(_)))
    }
    fn base(&self) -> &Self {
        if let Self::Authored { command, .. } = self {
            command
        } else {
            self
        }
    }
    fn authoring_model(
        &self,
    ) -> Option<&ds_command_kernel::project_management::authoring_model::AuthoringModel> {
        if let Self::Authored { model, .. } = self {
            Some(model)
        } else {
            None
        }
    }

    fn action(&self) -> &'static str {
        match self {
            Self::Authored { command, .. } => command.action(),
            Self::ProjectNotes { command } => command.action(),
            Self::Graph | Self::Overview { .. } | Self::ModelReferences { .. } => "get_graph",
            Self::PublicProjects { .. } => "list_public_projects",
            Self::TaskCommentCreate { .. } => "create_comment",
            Self::TaskComments { .. } => "get_context",
            Self::Assignees => "get_assignees",
            Self::TaskRecords { .. } => "get_context",
            Self::Commit { .. } | Self::TaskCreateAttached { .. } => "commit_command",
            Self::CommitBatch { .. } => "commit_batch",
            Self::Propose { .. } => "commit_command",
            Self::RequestAdmission { .. } => "task_request_admission",
            Self::Admit { .. } => "task_admit",
            Self::DeclineAdmission { .. } => "task_decline_admission",
            Self::LogHours { .. } => "task_log_hours",
            Self::TaskAttach { .. } => "start_attachment_upload",
            Self::TaskDetach { .. } => "detach_task_attachment",
        }
    }

    /// Refuse locally what the server would refuse by name, before a round
    /// trip is spent on it.
    pub fn validate(&self) -> Result<(), ClientError> {
        let invalid = |message: &'static str| ClientError::new(ErrorKind::InvalidInput, message);
        match self {
            Self::Authored { model, command } => {
                model
                    .validate()
                    .map_err(|_| invalid("invalid self-declared authoring model"))?;
                if !command.supports_authoring() {
                    return Err(invalid("unsupported or nested authoring declaration"));
                }
                command.validate()
            }
            Self::ProjectNotes { command } => command
                .validate()
                .map_err(|_| invalid("Project Notes request is invalid")),
            Self::Graph | Self::Assignees => Ok(()),
            Self::Overview { options } => options.validate().map_err(|_| {
                ClientError::new(
                    ErrorKind::InvalidInput,
                    "project overview options are invalid",
                )
            }),
            Self::TaskCreateAttached {
                command_id,
                base_revision,
                command,
                task_id,
                files,
            } => create_attached::validate(command_id, *base_revision, command, task_id, files),
            Self::TaskDetach { request } => request
                .validate()
                .map_err(|message| ClientError::new(ErrorKind::InvalidInput, message)),
            Self::TaskAttach { task_id, .. } => {
                if !project_note_id_valid(task_id) {
                    return Err(invalid("a task attachment names one stable task id"));
                }
                Ok(())
            }
            Self::PublicProjects {
                status,
                cursor,
                limit,
            } => {
                if !matches!(
                    status.as_str(),
                    "active" | "archived" | "testing" | "template"
                ) || !(1..=100).contains(limit)
                    || cursor.len() > 256
                    || cursor.trim() != cursor
                {
                    return Err(invalid(
                        "public Project Work index needs a status, 1..100 limit and bounded cursor",
                    ));
                }
                Ok(())
            }
            Self::TaskCommentCreate { task_id, id, body } => {
                if !project_note_id_valid(task_id)
                    || !project_note_id_valid(id)
                    || body.trim().is_empty()
                    || body.trim().len() > 4000
                {
                    return Err(invalid(
                        "task comment needs stable task/id and 1..4000 UTF-8 bytes of text",
                    ));
                }
                Ok(())
            }
            Self::TaskComments {
                task_id,
                cursor,
                limit,
            } => {
                if !project_note_id_valid(task_id)
                    || !(1..=100).contains(limit)
                    || (!cursor.is_empty() && !project_note_id_valid(cursor))
                {
                    return Err(invalid(
                        "task comments need one task, 1..100 limit and stable cursor",
                    ));
                }
                Ok(())
            }
            Self::TaskRecords { task_id, limit } => {
                if !project_note_id_valid(task_id) || !(1..=MAX_CONTEXT_LIMIT).contains(limit) {
                    return Err(invalid("task records need one task and 1..100 limit"));
                }
                Ok(())
            }
            Self::ModelReferences {
                model_id,
                revision_id,
                records_cursor,
            } => ds_command_kernel::project_management::model_links::validate_reference_selection(
                model_id,
                revision_id.as_deref(),
                records_cursor.as_deref(),
            )
            .map_err(|_| {
                invalid("model references need exact catalog IDs and a bounded records cursor")
            }),
            Self::Commit {
                command_id,
                base_revision,
                command,
            } => {
                if !command_id_valid(command_id) {
                    return Err(invalid("a project command needs one bounded command id"));
                }
                if *base_revision < 0 {
                    return Err(invalid(
                        "a project command needs a non-negative base revision",
                    ));
                }
                if !wire_command_valid(command) {
                    return Err(invalid(
                        "a project command is one `{kind, <kind>: {…}}` object",
                    ));
                }
                Ok(())
            }
            Self::CommitBatch {
                command_id,
                base_revision,
                commands,
            } => {
                if !command_id_valid(command_id) {
                    return Err(invalid("a saved draft needs one bounded command id"));
                }
                if *base_revision < 0 {
                    return Err(invalid("a saved draft needs a non-negative base revision"));
                }
                if commands.is_empty() || commands.len() > MAX_BATCH_COMMANDS {
                    return Err(invalid("a saved draft carries 1 through 200 commands"));
                }
                if !commands.iter().all(wire_command_valid) {
                    return Err(invalid(
                        "every command of a draft is one `{kind, <kind>: {…}}` object",
                    ));
                }
                Ok(())
            }
            Self::Propose {
                command_id,
                base_revision,
                estimated_hours,
                estimated_days,
                ..
            } => {
                if !command_id_valid(command_id)
                    || *base_revision < 0
                    || !estimated_hours.is_finite()
                    || *estimated_hours < 0.0
                    || estimated_days.is_some_and(|days| days < 0)
                {
                    return Err(invalid(
                        "a task proposal needs a valid command id, revision and estimate",
                    ));
                }
                Ok(())
            }
            Self::RequestAdmission {
                command_id,
                base_revision,
                ..
            }
            | Self::Admit {
                command_id,
                base_revision,
                ..
            }
            | Self::DeclineAdmission {
                command_id,
                base_revision,
                ..
            }
            | Self::LogHours {
                command_id,
                base_revision,
                ..
            } => {
                if !command_id_valid(command_id) || *base_revision < 0 {
                    return Err(invalid(
                        "a task action needs a valid command id and revision",
                    ));
                }
                Ok(())
            }
        }
    }

    fn payload(&self, project: &str) -> Value {
        let mut payload = json!({ "action": self.action(), "ds_project": project });
        match self {
            Self::Authored { model, command } => {
                let mut payload = command.payload(project);
                payload["authoring_model"] =
                    serde_json::to_value(model).expect("bounded authoring declaration");
                return payload;
            }
            Self::ProjectNotes { command } => {
                return command
                    .payload(project)
                    .expect("validated closed Project Notes request");
            }
            Self::Graph
            | Self::Assignees
            | Self::Overview { .. }
            | Self::ModelReferences { .. } => {}
            Self::TaskDetach { request } => return request.payload(project),
            Self::TaskAttach { task_id, file } => {
                return attachment_start_payload(project, task_id, file);
            }
            Self::PublicProjects {
                status,
                cursor,
                limit,
            } => {
                payload.as_object_mut().unwrap().remove("ds_project");
                payload["status"] = json!(status);
                payload["limit"] = json!(limit);
                if !cursor.is_empty() {
                    payload["cursor"] = json!(cursor);
                }
            }
            Self::TaskCommentCreate { task_id, id, body } => {
                payload["id"] = json!(id);
                payload["data"] =
                    json!({"task_id":task_id,"body":body,"kind":"comment","state":"open"});
            }
            Self::TaskComments {
                task_id,
                cursor,
                limit,
            } => {
                payload["task_id"] = json!(task_id);
                payload["limit"] = json!(limit);
                if !cursor.is_empty() {
                    payload["cursors"] = json!({"pm_comments":cursor});
                }
            }
            Self::TaskRecords { task_id, limit } => {
                payload["task_id"] = json!(task_id);
                payload["limit"] = json!(limit);
                payload["collections"] = json!(["pm_records"]);
            }
            Self::Commit {
                command_id,
                base_revision,
                command,
            }
            | Self::TaskCreateAttached {
                command_id,
                base_revision,
                command,
                ..
            } => {
                payload["command_id"] = json!(command_id);
                payload["base_revision"] = json!(base_revision);
                payload["command"] = command.clone();
            }
            Self::CommitBatch {
                command_id,
                base_revision,
                commands,
            } => {
                payload["command_id"] = json!(command_id);
                payload["base_revision"] = json!(base_revision);
                payload["commands"] = Value::Array(commands.clone());
            }
            Self::Propose {
                command_id,
                base_revision,
                task_id,
                title,
                description,
                discipline,
                responsible_email,
                estimated_hours,
                estimated_days,
            } => {
                let mut create = Map::new();
                create.insert("id".into(), json!(task_id));
                create.insert("title".into(), json!(title));
                create.insert("placement".into(), json!("inbox"));
                create.insert("schedule_state".into(), json!("unscheduled"));
                create.insert("responsible_email".into(), json!(responsible_email));
                create.insert("estimated_hours".into(), json!(estimated_hours));
                if let Some(days) = estimated_days {
                    create.insert("estimated_days".into(), json!(days));
                }
                if let Some(text) = description {
                    create.insert("description".into(), json!(text));
                }
                if let Some(text) = discipline {
                    create.insert("discipline".into(), json!(text));
                }
                payload["command_id"] = json!(command_id);
                payload["base_revision"] = json!(base_revision);
                payload["command"] =
                    json!({"kind": "create_task", "create_task": Value::Object(create)});
            }
            Self::RequestAdmission {
                command_id,
                base_revision,
                task_id,
                note,
            } => {
                payload["command_id"] = json!(command_id);
                payload["base_revision"] = json!(base_revision);
                payload["task_id"] = json!(task_id);
                if let Some(note) = note {
                    payload["note"] = json!(note);
                }
            }
            Self::Admit {
                command_id,
                base_revision,
                task_id,
                parent_task_id,
                target_index,
            } => {
                payload["command_id"] = json!(command_id);
                payload["base_revision"] = json!(base_revision);
                payload["task_id"] = json!(task_id);
                if !parent_task_id.is_empty() {
                    payload["parent_task_id"] = json!(parent_task_id);
                }
                if let Some(index) = target_index {
                    payload["target_index"] = json!(index);
                }
            }
            Self::DeclineAdmission {
                command_id,
                base_revision,
                task_id,
                reason,
            } => {
                payload["command_id"] = json!(command_id);
                payload["base_revision"] = json!(base_revision);
                payload["task_id"] = json!(task_id);
                payload["reason"] = json!(reason);
            }
            Self::LogHours {
                command_id,
                base_revision,
                task_id,
                hours,
                note,
            } => {
                payload["command_id"] = json!(command_id);
                payload["base_revision"] = json!(base_revision);
                payload["task_id"] = json!(task_id);
                payload["hours"] = json!(hours);
                if let Some(note) = note {
                    payload["note"] = json!(note);
                }
            }
        }
        payload
    }
}

/// Classify one refused action, carrying the server's own code and sentence.
///
/// ds-brain's PM handler answers every refusal in the DS envelope
/// (`{success: false, error: {code, message, details}}`), and the code is the
/// actionable half: `PM_REVISION_CONFLICT` means re-read and decide again,
/// `INSUFFICIENT_PERMISSIONS` means ask an admin, and `VALIDATION_FAILED` or
/// `PM_REFUSED` carry a sentence the operator can act on. Collapsing those
/// into a status class would send a caller to guess at three different next
/// steps from one word.
pub(crate) fn refusal(status: u16, body: &[u8]) -> ClientError {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let error = &value["error"];
    // ds-brain spells its codes in upper case; the boundary carries
    // identifiers in lower case, as every other governed door does.
    let code = match error["code"].as_str() {
        Some("PM_REFUSED") => error["details"]["reason"].as_str().or(Some("PM_REFUSED")),
        other => other,
    }
    .map(str::to_ascii_lowercase);
    let envelope = ServiceRefusal::new(status, code.as_deref(), error["message"].as_str());
    let (kind, message) = match status {
        400 | 422 => (
            ErrorKind::InvalidInput,
            "project management refused this command",
        ),
        401 | 403 => (
            ErrorKind::AuthenticationRejected,
            "project management refused this verified identity or permission",
        ),
        404 => (
            ErrorKind::ResourceNotFound,
            "project management could not find the selected project",
        ),
        409 => (
            ErrorKind::InvalidInput,
            "project management refused a command authored against a revision that moved",
        ),
        _ => (
            ErrorKind::Transient,
            "project management is temporarily unavailable",
        ),
    };
    // Always carried, even bare: the status alone tells a gateway 503 from a
    // refusal the route authored.
    ClientError::with_service_refusal(kind, message, envelope)
}

fn refused() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "project management did not answer in a shape this build understands",
    )
}

/// The project a read is for, or the refusal an operator can act on.
///
/// Separate from [`run`] so it is testable without a profile and a transport:
/// a request that cannot succeed should not cost a round trip, and the remedy
/// is local.
fn readable_project(project: &str) -> Result<&str, ClientError> {
    let trimmed = project.trim();
    if trimmed.is_empty() {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "project management needs the project it is reading",
        ));
    }
    Ok(trimmed)
}

/// Send one payload on the PM route and return the envelope's non-null `data`.
fn send<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    payload: Value,
) -> Result<Value, ClientError> {
    let response = transport.project_management(ProjectManagementCall {
        profile,
        authority: authority.call(),
        payload,
    })?;
    if response.status != 200 {
        return Err(refusal(response.status, &response.body));
    }
    if response.body.len() > MAX_RESPONSE_BYTES {
        return Err(refused());
    }
    let body: Value = serde_json::from_slice(&response.body).map_err(|_| refused())?;
    if body["success"] != true {
        return Err(refused());
    }
    let data = body.get("data").cloned().unwrap_or(Value::Null);
    if data.is_null() {
        return Err(refused());
    }
    Ok(data)
}

/// The reserved attachment row and its upload session, checked against what
/// was asked: the same task, an `uploading` row with a positive version, and
/// a resumable session URI on the storage upload host.
fn started_attachment<'a>(
    started: &'a Value,
    task_id: &str,
) -> Result<(&'a str, i64, &'a str), ClientError> {
    let item = &started["item"];
    let id = item["id"]
        .as_str()
        .filter(|id| project_note_id_valid(id))
        .ok_or_else(refused)?;
    let version = item["version"]
        .as_i64()
        .filter(|version| *version >= 1)
        .ok_or_else(refused)?;
    if item["linked_entity_type"] != "task"
        || item["linked_entity_id"] != task_id
        || item["state"] != "uploading"
    {
        return Err(refused());
    }
    let uri = started["session_uri"].as_str().ok_or_else(refused)?;
    crate::solar_project::validate_upload_uri(uri).map_err(|_| refused())?;
    Ok((id, version, uri))
}

/// The finalized row, accepted only when ds-brain verified the stored object
/// as this attachment on this task.
fn ready_attachment(
    finalized: &Value,
    attachment_id: &str,
    task_id: &str,
    file: &AttachmentFile,
) -> Result<Value, ClientError> {
    let item = &finalized["item"];
    if item["id"] != attachment_id
        || item["state"] != "ready"
        || item["linked_entity_type"] != "task"
        || item["linked_entity_id"] != task_id
        || item["size_bytes"].as_u64() != Some(file.bytes.len() as u64)
    {
        return Err(ClientError::new(
            ErrorKind::UnreadableResponse,
            "project management did not verify the uploaded attachment as ready on this task",
        ));
    }
    Ok(item.clone())
}

fn attachment_start_payload(project: &str, task_id: &str, file: &AttachmentFile) -> Value {
    json!({"action": "start_attachment_upload", "ds_project": project, "data": {
        "linked_entity_type": "task", "linked_entity_id": task_id,
        "original_name": file.name, "content_type": file.content_type,
        "size_bytes": file.bytes.len()}})
}

/// `start_attachment_upload` → the bytes to the resumable session →
/// `finalize_attachment_upload`. The session URI is transport authority: it
/// is spent here and never returned.
fn attach_to_task<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    start: Value,
    task_id: &str,
    file: &AttachmentFile,
) -> Result<Value, ClientError> {
    let started = send(profile, transport, authority, start)?;
    let (attachment_id, version, uri) = started_attachment(&started, task_id)?;
    let mut reader = std::io::Cursor::new(file.bytes.as_slice());
    let uploaded = transport.upload_bytes(crate::UploadBytesCall {
        uri,
        size: file.bytes.len() as u64,
        reader: &mut reader,
    })?;
    if !matches!(uploaded.status, 200 | 201) {
        return Err(ClientError::new(
            ErrorKind::Transient,
            "the attachment bytes did not reach storage; the reserved row stays uploading and is reclaimed on your next attachment",
        ));
    }
    let finalized = send(
        profile,
        transport,
        authority,
        json!({
            "action": "finalize_attachment_upload",
            "ds_project": project,
            "id": attachment_id,
            "expected_version": version,
        }),
    )?;
    let item = ready_attachment(&finalized, attachment_id, task_id, file)?;
    Ok(json!({
        "item": item,
        "bytes": file.bytes.len(),
        "sha256": file.sha256(),
        "content_type": file.content_type,
    }))
}

/// Send one governed action for one project and return its `data`.
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Value, ClientError> {
    let project = if matches!(command, Command::PublicProjects { .. }) {
        if !project.is_empty() {
            return Err(ClientError::new(
                ErrorKind::InvalidInput,
                "public Project Work index accepts no project",
            ));
        }
        project
    } else {
        readable_project(project)?
    };
    command.validate()?;
    let request = command.payload(project);
    let authoring_model = command.authoring_model();
    let command = command.base();
    if let Command::ProjectNotes { command } = command {
        return project_notes::run(
            profile,
            transport,
            authority,
            project,
            command,
            authoring_model,
        );
    }
    if let Command::ModelReferences {
        model_id,
        revision_id,
        records_cursor,
    } = command
    {
        return model_references::run(
            profile,
            transport,
            authority,
            project,
            model_id,
            revision_id.as_deref(),
            records_cursor.as_deref(),
        );
    }
    if let Command::Overview { options } = command {
        return overview::run(profile, transport, authority, project, options);
    }
    if let Command::TaskCreateAttached { task_id, files, .. } = command {
        return create_attached::run(
            profile, transport, authority, project, request, task_id, files,
        );
    }
    if let Command::TaskAttach { task_id, file } = command {
        return attach_to_task(
            profile, transport, authority, project, request, task_id, file,
        );
    }
    if let Command::TaskDetach { request } = command {
        request
            .validate_for(project)
            .map_err(|message| ClientError::new(ErrorKind::InvalidInput, message))?;
    }
    let data = send(profile, transport, authority, request)?;
    if let Command::TaskDetach { request } = command {
        request
            .verify(project, &data)
            .map_err(|message| ClientError::new(ErrorKind::UnreadableResponse, message))?;
        return Ok(data);
    }
    // ds-brain answers `{success, data}`, and the graph sits under `data.graph`
    // when it is wrapped and IS the data when it is not. The browser reads it
    // the same way (`response.graph ?? response`, `api/pm.ts:119-121`).
    //
    // Reading only the envelope is not a loud failure — it is an EMPTY PLAN
    // that looks valid: revision 0, no tasks, every permission false. That is
    // indistinguishable from a project with no plan, which is exactly the
    // class of ambiguity this campaign exists to remove, so the unwrap is
    // explicit rather than incidental.
    let graph = match command {
        Command::Graph => data
            .get("graph")
            .filter(|graph| !graph.is_null())
            .cloned()
            .unwrap_or(data),
        Command::Assignees => data
            .get("roster")
            .filter(|roster| roster.is_object())
            .cloned()
            .unwrap_or(data),
        // `{context: {…}}` for a context read; `{result: {…}}` for a single
        // command and the bare result for a draft (`response.result ??
        // response`, api/pm.ts:184 and 208). The wrapper is unwrapped where it
        // exists so every caller reads one shape.
        Command::TaskComments { .. } | Command::TaskRecords { .. } => data
            .get("context")
            .filter(|context| context.is_object())
            .cloned()
            .unwrap_or(data),
        Command::Commit { .. } | Command::CommitBatch { .. } => data
            .get("result")
            .filter(|result| result.is_object())
            .cloned()
            .unwrap_or(data),
        // Context creation returns `{item: <PMDoc>}` intact, including the
        // server-authored id and version for callers to shape.
        Command::Authored { .. }
        | Command::ProjectNotes { .. }
        | Command::Overview { .. }
        | Command::ModelReferences { .. }
        | Command::PublicProjects { .. }
        | Command::TaskCommentCreate { .. }
        | Command::Propose { .. }
        | Command::RequestAdmission { .. }
        | Command::Admit { .. }
        | Command::DeclineAdmission { .. }
        | Command::LogHours { .. }
        | Command::TaskAttach { .. }
        | Command::TaskDetach { .. }
        | Command::TaskCreateAttached { .. } => data,
    };
    if let Command::TaskRecords { task_id, limit } = command {
        validate_task_records(&graph, project, task_id, *limit)?;
    }
    Ok(graph)
}

/// Verify one retained canonical graph without inventing missing legacy collections.
/// This validator serves offline admission; ordinary live graph compatibility stays unchanged.
pub fn verify_graph(project: &str, graph: &Value) -> Result<(), ClientError> {
    let invalid = || {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "PM graph does not prove its project, revisions, arrays and captured permissions",
        )
    };
    if graph["project_id"].as_str() != Some(project)
        || graph["revision"].as_i64().is_none_or(|n| n < 0)
        || graph["schedule_revision"].as_i64().is_none_or(|n| n < 0)
        || ["tasks", "dependencies", "residual_items"]
            .into_iter()
            .any(|key| !graph[key].is_array())
        || !graph["calendar"].is_object()
        || ["view", "comment", "contribute", "edit_schedule", "admin"]
            .into_iter()
            .any(|key| !graph["permissions"][key].is_boolean())
        || graph["permissions"]["view"] != true
        || graph.get("retained").is_some()
        || graph
            .get("correspondence")
            .is_some_and(|c| !c.is_null() && !c.is_object())
    {
        return Err(invalid());
    }
    if let Some(c) = graph.get("correspondence").filter(|c| !c.is_null()) {
        let records = c["records"].as_array().ok_or_else(invalid)?;
        let total = c["records_total"].as_u64().ok_or_else(invalid)?;
        if !c["truncated"].is_boolean() || total < records.len() as u64
            || !c["blocked_tasks"].is_array()
            || c["as_of"].as_str().is_none_or(str::is_empty)
            || c["due_window_days"].as_u64().is_none()
            || [
                "recordsOutstanding",
                "recordsOverdue",
                "tasksAwaitingCorrespondence",
            ]
            .into_iter()
            .any(|key| c["counters"][key].as_u64().is_none())
        {
            return Err(invalid());
        }
    }
    for key in ["tasks", "dependencies", "residual_items"] {
        if graph[key]
            .as_array()
            .unwrap()
            .iter()
            .any(|row| !row.is_object())
        {
            return Err(invalid());
        }
    }
    Ok(())
}

/// Admit only the selected records page, never a legacy unselected or partial
/// answer that could otherwise become an apparently empty related-record list.
pub fn validate_task_records(
    context: &Value,
    project: &str,
    task: &str,
    limit: i64,
) -> Result<(), ClientError> {
    let invalid = || {
        ClientError::new(
            ErrorKind::UnreadableResponse,
            "task records response does not prove the selected scope and completeness",
        )
    };
    let records = context["records"].as_array().ok_or_else(invalid)?;
    let truncated = context["truncated_by_collection"]["pm_records"]
        .as_bool()
        .ok_or_else(invalid)?;
    let cursors = context["next_cursors"].as_object().ok_or_else(invalid)?;
    if context["project_id"].as_str() != Some(project)
        || context["task_id"].as_str() != Some(task)
        || context["loaded_collections"] != json!(["pm_records"])
        || records.len() > limit as usize
        || context
            .get("record_id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty())
        || context
            .get("residual_id")
            .and_then(Value::as_str)
            .is_some_and(|id| !id.is_empty())
        || context["truncated_by_collection"]
            .as_object()
            .is_none_or(|flags| flags.len() != 1)
        || cursors.keys().any(|key| key != "pm_records")
        || cursors
            .get("pm_records")
            .is_some_and(|cursor| cursor.as_str().is_none_or(|id| !project_note_id_valid(id)))
        || (!truncated && cursors.contains_key("pm_records"))
        || records.iter().any(|record| {
            !record.is_object()
                || record["id"]
                    .as_str()
                    .is_none_or(|id| !project_note_id_valid(id))
                || !record["data"]["related_task_ids"]
                    .as_array()
                    .is_some_and(|ids| ids.iter().any(|id| id.as_str() == Some(task)))
        })
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod collaboration_contract_tests {
    use super::*;
    #[test]
    fn public_index_has_no_project_or_write_fields_and_bounds_pages() {
        let command = Command::PublicProjects {
            status: "active".into(),
            cursor: "next-id".into(),
            limit: 50,
        };
        command.validate().unwrap();
        assert_eq!(
            command.payload(""),
            json!({"action":"list_public_projects","status":"active","cursor":"next-id","limit":50})
        );
        assert!(
            Command::PublicProjects {
                status: "all".into(),
                cursor: String::new(),
                limit: 50
            }
            .validate()
            .is_err()
        );
        assert!(
            Command::PublicProjects {
                status: "active".into(),
                cursor: String::new(),
                limit: 101
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn task_comment_is_scoped_and_cursors_use_the_existing_collection_name() {
        let command = Command::TaskCommentCreate {
            task_id: "t".into(),
            id: "comment-1".into(),
            body: "Please check this task".into(),
        };
        command.validate().unwrap();
        assert_eq!(
            command.payload("p"),
            json!({"action":"create_comment","ds_project":"p","id":"comment-1","data":{"task_id":"t","body":"Please check this task","kind":"comment","state":"open"}})
        );
        let page = Command::TaskComments {
            task_id: "t".into(),
            cursor: "old-comment".into(),
            limit: 20,
        };
        assert_eq!(
            page.payload("p")["cursors"],
            json!({"pm_comments":"old-comment"})
        );
        assert!(
            Command::TaskCommentCreate {
                task_id: "t".into(),
                id: "comment-1".into(),
                body: " ".into()
            }
            .validate()
            .is_err()
        );
        assert!(
            Command::TaskCommentCreate {
                task_id: "t".into(),
                id: "comment-1".into(),
                body: "é".repeat(2001)
            }
            .validate()
            .is_err()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_records_ids_accept_stable_references_and_refuse_invalid_bounds() {
        for id in [
            "550e8400-e29b-41d4-a716-446655440000".into(),
            "note:review@example.com+2026_09-30.é".into(),
            "a".repeat(256),
            "é".repeat(128),
        ] {
            Command::TaskRecords {
                task_id: id,
                limit: 100,
            }
            .validate()
            .expect("a server-valid bounded stable id");
        }
        for id in [
            "",
            ".",
            "..",
            "a/b",
            " a",
            "a ",
            "a b",
            "a\t",
            "a\n",
            "a\0",
            "a\u{2003}b",
        ]
        .into_iter()
        .map(str::to_owned)
        .chain(["a".repeat(257), "é".repeat(129)])
        {
            let error = Command::TaskRecords {
                task_id: id,
                limit: 100,
            }
            .validate()
            .expect_err("an invalid stable id is refused locally");
            assert_eq!(error.kind(), ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn proposal_and_admission_payloads_keep_the_governed_shape() {
        let proposal = Command::Propose {
            command_id: "cmd-propose-0001".into(),
            base_revision: 4,
            task_id: "p1".into(),
            title: "Survey the extension".into(),
            description: None,
            discipline: Some("survey".into()),
            responsible_email: "c@x.rw".into(),
            estimated_hours: 12.0,
            estimated_days: Some(3),
        }
        .payload("proj");
        assert_eq!(
            proposal,
            json!({
                "action": "commit_command", "ds_project": "proj",
                "command_id": "cmd-propose-0001", "base_revision": 4,
                "command": {"kind": "create_task", "create_task": {
                    "id": "p1", "title": "Survey the extension", "placement": "inbox",
                    "schedule_state": "unscheduled", "responsible_email": "c@x.rw",
                    "estimated_hours": 12.0, "estimated_days": 3, "discipline": "survey"
                }}
            })
        );
        let admit = Command::Admit {
            command_id: "cmd-admit-0001".into(),
            base_revision: 5,
            task_id: "p1".into(),
            parent_task_id: String::new(),
            target_index: None,
        }
        .payload("proj");
        assert!(admit.get("parent_task_id").is_none());
        assert!(admit.get("target_index").is_none());
    }

    #[test]
    fn proposal_refusal_carries_its_reason_token() {
        let refused = refusal(409, br#"{"success":false,"error":{"code":"PM_REFUSED","message":"Survey is already placed in the plan","details":{"reason":"task_already_placed"}}}"#);
        let detail = refused.service_refusal().expect("bounded refusal");
        assert_eq!(detail.code(), Some("task_already_placed"));
        assert_eq!(detail.status(), 409);
    }

    #[test]
    fn the_action_vocabulary_is_closed_and_named() {
        assert_eq!(Command::Graph.action(), "get_graph");
        assert_eq!(Command::Assignees.action(), "get_assignees");
        assert_eq!(
            Command::Assignees.payload("p"),
            json!({"action":"get_assignees","ds_project":"p"})
        );
        assert_eq!(
            Command::ModelReferences {
                model_id: "model-a".into(),
                revision_id: None,
                records_cursor: None
            }
            .action(),
            "get_graph"
        );
        let commit = Command::Commit {
            command_id: "c1".into(),
            base_revision: 3,
            command: json!({"kind": "accept_assignment", "accept_assignment": {"task_id": "t"}}),
        };
        assert_eq!(commit.action(), "commit_command");
        let payload = commit.payload("p");
        assert_eq!(payload["command_id"], "c1");
        assert_eq!(payload["base_revision"], 3);
        assert_eq!(payload["command"]["kind"], "accept_assignment");
        let batch = Command::CommitBatch {
            command_id: "c2".into(),
            base_revision: 0,
            commands: vec![
                json!({"kind": "set_progress", "set_progress": {"task_id": "t", "progress": 0.5}}),
            ],
        };
        assert_eq!(batch.action(), "commit_batch");
        assert_eq!(
            batch.payload("p")["commands"].as_array().map(Vec::len),
            Some(1)
        );
    }

    /// The bounds the server enforces are refused here first, by the same
    /// numbers, so a round trip is never spent to learn them.
    #[test]
    fn a_command_outside_its_bounds_is_refused_before_it_is_sent() {
        let kind = |command: Command| command.validate().expect_err("refused").kind();
        assert_eq!(
            kind(Command::ModelReferences {
                model_id: "Display label".into(),
                revision_id: None,
                records_cursor: None
            }),
            ErrorKind::InvalidInput
        );
        let good = json!({"kind": "accept_assignment", "accept_assignment": {"task_id": "t"}});
        assert!(
            Command::Commit {
                command_id: "pm-1_a.b".into(),
                base_revision: 0,
                command: good.clone()
            }
            .validate()
            .is_ok()
        );
        assert_eq!(
            kind(Command::Commit {
                command_id: "".into(),
                base_revision: 0,
                command: good.clone()
            }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            kind(Command::Commit {
                command_id: "c".into(),
                base_revision: -1,
                command: good.clone()
            }),
            ErrorKind::InvalidInput
        );
        for bad in [
            json!({"kind": "accept_assignment"}),
            json!({"kind": "accept_assignment", "decline_assignment": {}}),
            json!({"kind": "accept_assignment", "accept_assignment": {}, "extra": 1}),
            json!([]),
        ] {
            assert_eq!(
                kind(Command::Commit {
                    command_id: "c".into(),
                    base_revision: 0,
                    command: bad
                }),
                ErrorKind::InvalidInput
            );
        }
        assert_eq!(
            kind(Command::CommitBatch {
                command_id: "c".into(),
                base_revision: 0,
                commands: vec![]
            }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            kind(Command::CommitBatch {
                command_id: "c".into(),
                base_revision: 0,
                commands: vec![good; MAX_BATCH_COMMANDS + 1],
            }),
            ErrorKind::InvalidInput
        );
    }

    /// The server's code and sentence cross the boundary; the status alone
    /// decides the class.
    #[test]
    fn a_refusal_carries_the_servers_own_code_and_sentence() {
        let body = br#"{"success":false,"error":{"code":"PM_REVISION_CONFLICT","message":"The plan moved to revision 9.","details":{"current_revision":9}}}"#;
        let error = refusal(409, body);
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        let served = error.service_refusal().expect("carried");
        assert_eq!(served.status(), 409);
        assert_eq!(served.code(), Some("pm_revision_conflict"));
        assert_eq!(served.message(), Some("The plan moved to revision 9."));

        let forbidden = refusal(403, br#"{"success":false,"error":{"code":"INSUFFICIENT_PERMISSIONS","message":"Project schedule editor access required"}}"#);
        assert_eq!(forbidden.kind(), ErrorKind::AuthenticationRejected);
        assert_eq!(
            forbidden.service_refusal().and_then(|r| r.code()),
            Some("insufficient_permissions")
        );

        let missing = refusal(
            404,
            br#"{"success":false,"error":{"code":"NOT_FOUND","message":"Project not found"}}"#,
        );
        assert_eq!(missing.kind(), ErrorKind::ResourceNotFound);

        // A gateway 503 with no DS envelope: the class and the status,
        // nothing invented.
        let outage = refusal(503, b"upstream unavailable");
        assert_eq!(outage.kind(), ErrorKind::Transient);
        let served = outage.service_refusal().expect("the status is carried");
        assert_eq!(served.status(), 503);
        assert!(served.code().is_none() && served.message().is_none());
    }

    /// A projectless call is refused before anything is sent, and the project
    /// is trimmed rather than sent with the whitespace an operator pasted.
    #[test]
    fn a_blank_project_is_refused_and_a_padded_one_is_trimmed() {
        for blank in ["", "   ", "\t\n"] {
            let error = readable_project(blank).expect_err("a blank project is refused");
            assert_eq!(error.kind(), ErrorKind::InvalidInput);
        }
        assert_eq!(
            readable_project("  arjgpydw_nyam  ").expect("a padded project still reads"),
            "arjgpydw_nyam",
        );
    }

    /// A transport that answers the attachment start, the byte upload and the
    /// finalize in turn, recording what was sent.
    struct AttachTransport {
        created: Option<Value>,
        fail_upload_at: Option<usize>,
        started: Value,
        upload_status: u16,
        finalized: Value,
        payloads: Vec<Value>,
        authorities: Vec<(String, String)>,
        uploaded: Vec<(String, Vec<u8>)>,
    }
    macro_rules! no_other_call {
        ($name:ident, $call:ident) => {
            fn $name(
                &mut self,
                _: crate::$call<'_>,
            ) -> Result<crate::TransportResponse, crate::TransportError> {
                panic!("a task attachment made an unrelated call")
            }
        };
    }
    impl AuthorizedTransport for AttachTransport {
        no_other_call!(list_projects, ProjectListCall);
        no_other_call!(transformer_context, TransformerContextCall);
        no_other_call!(project_forms, ProjectFormsCall);
        no_other_call!(project_form_editor, ProjectFormEditorCall);
        no_other_call!(solar_snapshot, SolarSnapshotCall);
        no_other_call!(survey_query, SurveyQueryCall);
        no_other_call!(survey_entries_select, SurveyEntriesSelectCall);
        no_other_call!(survey_entries_changes, SurveyEntriesChangesCall);
        no_other_call!(survey_entry_create, SurveyEntryCreateCall);
        no_other_call!(project_data, ProjectDataCall);
        no_other_call!(survey_control, SurveyControlCall);
        no_other_call!(styles, StylesCall);
        no_other_call!(layers, LayersCall);
        no_other_call!(tiles, TileCall);
        no_other_call!(project_report, ProjectReportCall);
        fn upload_bytes(
            &mut self,
            mut call: crate::UploadBytesCall<'_>,
        ) -> Result<crate::TransportResponse, crate::TransportError> {
            let mut bytes = Vec::new();
            call.reader()
                .read_to_end(&mut bytes)
                .expect("upload reader");
            assert_eq!(bytes.len() as u64, call.size());
            self.uploaded.push((call.uri().to_owned(), bytes));
            Ok(crate::TransportResponse::new(
                if self.fail_upload_at == Some(self.uploaded.len()) {
                    500
                } else {
                    self.upload_status
                },
                Vec::new(),
            ))
        }
        fn project_management(
            &mut self,
            call: ProjectManagementCall<'_>,
        ) -> Result<crate::TransportResponse, crate::TransportError> {
            self.authorities.push((
                call.authority.canonical_uid.into(),
                call.authority.canonical_email.into(),
            ));
            if matches!(
                call.payload["action"].as_str(),
                Some("get_graph" | "get_context")
            ) && self.upload_status == 403
            {
                self.payloads.push(call.payload);
                return Ok(crate::TransportResponse::new(403, br#"{"success":false,"error":{"code":"INSUFFICIENT_PERMISSIONS","message":"Project unavailable"}}"#.to_vec()));
            }
            let data = match call.payload["action"].as_str() {
                Some("commit_command") => self.created.clone().expect("explicit creation fixture"),
                Some("detach_task_attachment") => self.started.clone(),
                Some("get_graph") => self.started.clone(),
                Some("get_context") => self.finalized.clone(),
                Some("start_attachment_upload") => {
                    let mut started = self.started.clone();
                    started["item"]["id"] = json!(format!("att-{}", self.uploaded.len() + 1));
                    started
                }
                Some("finalize_attachment_upload") => {
                    let mut finalized = self.finalized.clone();
                    finalized["item"]["id"] = call.payload["id"].clone();
                    finalized
                }
                other => panic!("unexpected PM action {other:?}"),
            };
            self.payloads.push(call.payload);
            Ok(crate::TransportResponse::new(
                200,
                serde_json::to_vec(&json!({"success": true, "data": data})).unwrap(),
            ))
        }
    }

    fn create_with_files(
        transport: &mut AttachTransport,
        files: Vec<AttachmentFile>,
    ) -> Result<Value, ClientError> {
        run(
            &crate::data_distribution::tests::profile_for_tests(),
            transport,
            crate::authorized::Authority {
                bearer_token: "bearer",
                uid: "uid",
                email: "user@example.com",
                device_id: None,
            },
            "project-a",
            &Command::TaskCreateAttached {
                command_id: "create-7".into(),
                base_revision: 7,
                command: json!({"kind":"create_task","create_task":{"id":"task-7","title":"Evidence"}}),
                task_id: "task-7".into(),
                files,
            },
        )
    }

    fn create_fixture(applied: bool) -> Value {
        json!({"result":{"applied":applied,"command_id":"create-7","committed_revision":8,
            "violations":if applied {json!([])} else {json!([{"code":"PROJECT_WORK_REVISION_CONFLICT","message":"Plan moved"}])}}})
    }

    fn records_transport(finalized: Value) -> AttachTransport {
        AttachTransport {
            created: None,
            fail_upload_at: None,
            started: json!({}),
            upload_status: 200,
            finalized,
            payloads: vec![],
            authorities: vec![],
            uploaded: vec![],
        }
    }

    fn task_records_fixture(truncated: bool) -> Value {
        json!({"context":{"project_id":"project-a","task_id":"task-7",
            "records":[{"id":"record-1","data":{"related_task_ids":["task-7"]}}],
            "loaded_collections":["pm_records"],
            "truncated_by_collection":{"pm_records":truncated},
            "next_cursors":if truncated {json!({"pm_records":"record-1"})} else {json!({})}}})
    }

    fn selected_records(transport: &mut AttachTransport, limit: i64) -> Result<Value, ClientError> {
        run(
            &crate::data_distribution::tests::profile_for_tests(),
            transport,
            crate::authorized::Authority {
                bearer_token: "bearer",
                uid: "uid",
                email: "user@example.com",
                device_id: None,
            },
            "project-a",
            &Command::TaskRecords {
                task_id: "task-7".into(),
                limit,
            },
        )
    }

    #[test]
    fn task_records_one_selected_request_preserves_authoritative_completeness() {
        for truncated in [false, true] {
            let mut transport = records_transport(task_records_fixture(truncated));
            let context = selected_records(&mut transport, 100).unwrap();
            assert_eq!(
                transport.payloads,
                vec![
                    json!({"action":"get_context","ds_project":"project-a","task_id":"task-7","limit":100,"collections":["pm_records"]})
                ]
            );
            assert_eq!(context["truncated_by_collection"]["pm_records"], truncated);
            assert_eq!(
                context["next_cursors"].get("pm_records").is_some(),
                truncated
            );
            assert!(transport.uploaded.is_empty());
        }
    }

    #[test]
    fn task_records_denied_or_unproven_page_never_becomes_empty_success() {
        let fixture = task_records_fixture(false);
        let mut cases = Vec::new();
        for key in [
            "project_id",
            "task_id",
            "records",
            "loaded_collections",
            "truncated_by_collection",
            "next_cursors",
        ] {
            let mut altered = fixture.clone();
            altered["context"].as_object_mut().unwrap().remove(key);
            cases.push(altered);
        }
        for (key, wrong) in [
            ("project_id", json!("other")),
            ("task_id", json!("other")),
            (
                "loaded_collections",
                json!(["pm_records", "pm_attachments"]),
            ),
            (
                "records",
                json!([{"id":"record-1","data":{"related_task_ids":["other"]}}]),
            ),
            ("next_cursors", json!({"pm_records":"record-1"})),
        ] {
            let mut altered = fixture.clone();
            altered["context"][key] = wrong;
            cases.push(altered);
        }
        for bad in cases {
            let mut transport = records_transport(bad);
            assert_eq!(
                selected_records(&mut transport, 100).unwrap_err().kind(),
                ErrorKind::UnreadableResponse
            );
            assert_eq!(transport.payloads.len(), 1);
        }
        let mut denied = records_transport(fixture);
        denied.upload_status = 403;
        assert_eq!(
            selected_records(&mut denied, 100).unwrap_err().kind(),
            ErrorKind::AuthenticationRejected
        );
        assert_eq!(denied.payloads.len(), 1);
        let mut invalid = records_transport(task_records_fixture(false));
        assert_eq!(
            selected_records(&mut invalid, 0).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert!(invalid.payloads.is_empty());
    }

    fn two_files() -> Vec<AttachmentFile> {
        vec![
            AttachmentFile::new("source.eml", None, vec![1; 4]).unwrap(),
            AttachmentFile::new("drawing.pdf", None, vec![2; 4]).unwrap(),
        ]
    }

    #[test]
    fn composed_task_creation_verifies_two_files_without_unrelated_reads() {
        let mut transport = attach_transport("ready", 4);
        transport.created = Some(create_fixture(true));
        let answer = create_with_files(&mut transport, two_files()).unwrap();
        assert_eq!(answer["complete"], true);
        assert_eq!(answer["task_id"], "task-7");
        assert_eq!(answer["result"]["committed_revision"], 8);
        assert_eq!(answer["attachments"].as_array().unwrap().len(), 2);
        assert_eq!(answer["attachments"][0]["item"]["id"], "att-1");
        assert_eq!(answer["attachments"][1]["item"]["id"], "att-2");
        assert_eq!(transport.uploaded.len(), 2);
        assert_eq!(
            transport
                .payloads
                .iter()
                .map(|p| p["action"].as_str().unwrap())
                .collect::<Vec<_>>(),
            [
                "commit_command",
                "start_attachment_upload",
                "finalize_attachment_upload",
                "start_attachment_upload",
                "finalize_attachment_upload"
            ]
        );
        assert!(
            transport
                .payloads
                .iter()
                .all(|p| p["ds_project"] == "project-a")
        );
        assert!(!answer.to_string().contains("upload_id"));
        assert!(!answer.to_string().contains("bearer"));
    }

    #[test]
    fn composed_upload_failure_retains_the_created_task_and_only_ready_files() {
        let mut transport = attach_transport("ready", 4);
        transport.created = Some(create_fixture(true));
        transport.fail_upload_at = Some(2);
        let mut files = two_files();
        files.push(AttachmentFile::new("later.txt", None, vec![3; 4]).unwrap());
        let answer = create_with_files(&mut transport, files).unwrap();
        assert_eq!(answer["complete"], false);
        assert_eq!(answer["task_id"], "task-7");
        assert_eq!(answer["result"]["applied"], true);
        assert_eq!(answer["failed_attachment_index"], 1);
        assert_eq!(answer["requested_attachment_count"], 3);
        assert_eq!(answer["attachments"].as_array().unwrap().len(), 1);
        assert_eq!(transport.uploaded.len(), 2);
        assert_eq!(transport.payloads.len(), 4); // no retry, third reservation or failed finalize
    }

    #[test]
    fn refused_task_creation_never_reserves_or_uploads_any_file() {
        let mut transport = attach_transport("ready", 4);
        transport.created = Some(create_fixture(false));
        let answer = create_with_files(&mut transport, two_files()).unwrap();
        assert_eq!(answer["result"]["applied"], false);
        assert_eq!(answer["complete"], false);
        assert!(transport.uploaded.is_empty());
        assert_eq!(transport.payloads.len(), 1);
    }

    #[test]
    fn composed_attachment_bounds_and_mismatched_task_refuse_before_any_effect() {
        for files in [
            vec![],
            vec![two_files()[0].clone(); 11],
            vec![two_files()[0].clone(); 2],
        ] {
            let mut transport = attach_transport("ready", 4);
            assert_eq!(
                create_with_files(&mut transport, files).unwrap_err().kind(),
                ErrorKind::InvalidInput
            );
            assert!(transport.payloads.is_empty());
        }
        let bad = Command::TaskCreateAttached {
            command_id: "create-7".into(),
            base_revision: 7,
            command: json!({"kind":"create_task","create_task":{"id":"another-task"}}),
            task_id: "task-7".into(),
            files: two_files(),
        };
        assert_eq!(bad.validate().unwrap_err().kind(), ErrorKind::InvalidInput);
    }

    const SESSION: &str = "https://storage.googleapis.com/upload/storage/v1/b/bucket/o?uploadType=resumable&upload_id=abc";

    fn attach_transport(state: &str, size: usize) -> AttachTransport {
        let row = |state: &str| {
            json!({"id": "att-1", "version": 1, "linked_entity_type": "task",
                   "linked_entity_id": "task-7", "state": state, "size_bytes": size})
        };
        AttachTransport {
            created: None,
            fail_upload_at: None,
            started: json!({"item": row("uploading"), "session_uri": SESSION}),
            upload_status: 200,
            finalized: json!({"item": row(state)}),
            payloads: Vec::new(),
            authorities: Vec::new(),
            uploaded: Vec::new(),
        }
    }

    fn attach(transport: &mut AttachTransport, file: AttachmentFile) -> Result<Value, ClientError> {
        let profile = crate::data_distribution::tests::profile_for_tests();
        run(
            &profile,
            transport,
            crate::authorized::Authority {
                bearer_token: "bearer",
                uid: "uid",
                email: "user@example.com",
                device_id: None,
            },
            "project-a",
            &Command::TaskAttach {
                task_id: "task-7".into(),
                file,
            },
        )
    }

    #[test]
    fn task_attach_starts_uploads_the_exact_bytes_and_finalizes_the_reserved_row() {
        let bytes = b"%PDF-1.7 field plan".to_vec();
        let file = AttachmentFile::new("field plan.pdf", None, bytes.clone()).unwrap();
        let mut transport = attach_transport("ready", bytes.len());
        let receipt = attach(&mut transport, file.clone()).expect("attached");
        assert_eq!(
            transport.payloads[0],
            json!({"action": "start_attachment_upload", "ds_project": "project-a",
                   "data": {"linked_entity_type": "task", "linked_entity_id": "task-7",
                            "original_name": "field plan.pdf", "content_type": "application/pdf",
                            "size_bytes": bytes.len()}})
        );
        assert_eq!(
            transport.uploaded,
            vec![(SESSION.to_owned(), bytes.clone())]
        );
        assert_eq!(
            transport.payloads[1],
            json!({"action": "finalize_attachment_upload", "ds_project": "project-a",
                   "id": "att-1", "expected_version": 1})
        );
        assert_eq!(receipt["item"]["state"], "ready");
        assert_eq!(receipt["sha256"], file.sha256());
        assert_eq!(receipt["bytes"], bytes.len());
        assert!(
            !receipt.to_string().contains("upload_id"),
            "the session never leaves the door"
        );
    }

    #[test]
    fn task_attach_refuses_an_unverified_row_a_foreign_session_and_a_failed_upload() {
        let bytes = b"PK\x03\x04".to_vec();
        let file = AttachmentFile::new("pack.zip", None, bytes.clone()).unwrap();
        let mut unverified = attach_transport("failed", bytes.len());
        assert_eq!(
            attach(&mut unverified, file.clone()).unwrap_err().kind(),
            ErrorKind::UnreadableResponse
        );
        let mut foreign = attach_transport("ready", bytes.len());
        foreign.started["session_uri"] = json!("https://example.com/upload?upload_id=abc");
        assert_eq!(
            attach(&mut foreign, file.clone()).unwrap_err().kind(),
            ErrorKind::UnreadableResponse
        );
        assert!(
            foreign.uploaded.is_empty(),
            "no byte leaves for a foreign session"
        );
        let mut other_task = attach_transport("ready", bytes.len());
        other_task.started["item"]["linked_entity_id"] = json!("task-8");
        assert!(attach(&mut other_task, file.clone()).is_err());
        assert!(other_task.uploaded.is_empty());
        let mut failed = attach_transport("ready", bytes.len());
        failed.upload_status = 503;
        assert_eq!(
            attach(&mut failed, file).unwrap_err().kind(),
            ErrorKind::Transient
        );
        assert_eq!(
            failed.payloads.len(),
            1,
            "an incomplete upload is never finalized"
        );
    }

    #[test]
    fn task_detach_sends_one_exact_metadata_write_and_verifies_receipt() {
        use ds_command_kernel::project_management::task_attachment_detach::Request;
        let req = Request {
            task_id: "task-7".into(),
            attachment_id: "att-1".into(),
            expected_version: 3,
        };
        let response = json!({"project_id":"project-a","task_id":"task-7","attachment_id":"att-1","version":4,"changed":true,"detached":true,"item":{"id":"att-1","version":4,"task_detached":true,"task_detached_prior_version":3,"task_detached_by":"user@example.com","task_detached_at":"2026-10-13T00:00:00Z","data":{"linked_entity_type":"task","linked_entity_id":"task-7","state":"ready","generation":29}}});
        let profile = crate::data_distribution::tests::profile_for_tests();
        let mut transport = attach_transport("ready", 1);
        transport.started = response.clone();
        let invoke = |transport: &mut AttachTransport, request: Request| {
            run(
                &profile,
                transport,
                crate::authorized::Authority {
                    bearer_token: "bearer",
                    uid: "uid",
                    email: "user@example.com",
                    device_id: None,
                },
                "project-a",
                &Command::TaskDetach { request },
            )
        };
        assert_eq!(invoke(&mut transport, req.clone()).unwrap(), response);
        assert_eq!(transport.payloads, vec![req.payload("project-a")]);
        assert!(
            transport.uploaded.is_empty(),
            "detach has no storage upload"
        );
        for bad in [
            Request {
                expected_version: 0,
                ..req.clone()
            },
            Request {
                task_id: "other/task".into(),
                ..req.clone()
            },
        ] {
            let mut untouched = attach_transport("ready", 1);
            assert_eq!(
                invoke(&mut untouched, bad).unwrap_err().kind(),
                ErrorKind::InvalidInput
            );
            assert!(untouched.payloads.is_empty() && untouched.uploaded.is_empty());
        }
        let mut foreign = attach_transport("ready", 1);
        foreign.started = response;
        foreign.started["project_id"] = json!("other-project");
        assert_eq!(
            invoke(&mut foreign, req).unwrap_err().kind(),
            ErrorKind::UnreadableResponse
        );
        assert_eq!(
            foreign.payloads.len(),
            1,
            "mismatched receipt is never retried"
        );
        assert!(foreign.uploaded.is_empty());
    }

    #[test]
    fn project_overview_reads_exact_pair_and_never_turns_refusals_into_empty_work() {
        use ds_command_kernel::project_management::overview::Options;
        let mut transport = attach_transport("ready", 1);
        transport.started = json!({"graph":{"project_id":"project-a","revision":9,"schedule_revision":4,"tasks":[{"id":"inbox","data":{"title":"Inbox without geometry","placement":"inbox","type":"task"}},{"id":"gate","data":{"title":"Milestone","type":"milestone"}}],"dependencies":[],"residual_items":[]}});
        transport.finalized = json!({"context":{"project_id":"project-a","loaded_collections":["pm_records"],"records":[{"id":"record","version":2,"data":{"category":"instruction","subject":"Record"}},{"id":"note","version":7,"data":{"category":"project_note","subject":"Note without geometry"}}],"truncated_by_collection":{"pm_records":false},"next_cursors":{},"personal_notes":[{"body":"PRIVATE_SECRET"}]}});
        let profile = crate::data_distribution::tests::profile_for_tests();
        let invoke = |t: &mut AttachTransport, options: Options| {
            run(
                &profile,
                t,
                crate::authorized::Authority {
                    bearer_token: "bearer",
                    uid: "uid",
                    email: "user@example.com",
                    device_id: None,
                },
                "project-a",
                &Command::Overview { options },
            )
        };
        let result = invoke(&mut transport, Options::default()).unwrap();
        assert_eq!(
            transport.payloads,
            vec![
                json!({"action":"get_graph","ds_project":"project-a"}),
                json!({"action":"get_context","ds_project":"project-a","limit":100,"collections":["pm_records"]})
            ]
        );
        assert_eq!(result["tasks"]["count"], 1);
        assert_eq!(result["milestones"]["count"], 1);
        assert_eq!(result["records"]["count"], 1);
        assert_eq!(result["project_notes"]["rows"][0]["version"], 7);
        assert_eq!(result["source_authority"]["uid"], "uid");
        assert_eq!(result["source_authority"]["project_id"], "project-a");
        assert_eq!(result["source_authority"]["lane"], "stable");
        assert!(
            !result.to_string().contains("PRIVATE_SECRET")
                && !result.to_string().contains("bearer")
        );
        assert!(transport.uploaded.is_empty());
        transport.payloads.clear();
        assert_eq!(
            invoke(
                &mut transport,
                Options {
                    limit: 0,
                    ..Options::default()
                }
            )
            .unwrap_err()
            .kind(),
            ErrorKind::InvalidInput
        );
        assert!(transport.payloads.is_empty());
        transport.started["graph"]["project_id"] = json!("foreign");
        assert_eq!(
            invoke(&mut transport, Options::default())
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
        assert_eq!(
            transport.payloads.len(),
            1,
            "bad graph never fetches context"
        );
        transport.payloads.clear();
        transport.upload_status = 403;
        assert_eq!(
            invoke(&mut transport, Options::default())
                .unwrap_err()
                .kind(),
            ErrorKind::AuthenticationRejected
        );
        assert_eq!(
            transport.payloads.len(),
            1,
            "denied graph never fetches context"
        );
        transport.upload_status = 200;
        transport.started["graph"]["project_id"] = json!("project-a");
        transport.finalized["context"]["loaded_collections"] = json!(["pm_records", "pm_comments"]);
        transport.payloads.clear();
        assert_eq!(
            invoke(&mut transport, Options::default())
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
        assert_eq!(
            transport.payloads.len(),
            2,
            "unrequested acquisition is never retried"
        );
        transport.finalized["context"]["loaded_collections"] = json!(["pm_records"]);
        transport.finalized["context"]["records"] = Value::Null;
        transport.payloads.clear();
        assert_eq!(
            invoke(&mut transport, Options::default())
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
        assert_eq!(transport.payloads.len(), 2);
        transport.finalized["context"]["project_id"] = json!("foreign");
        transport.payloads.clear();
        assert_eq!(
            invoke(&mut transport, Options::default())
                .unwrap_err()
                .kind(),
            ErrorKind::UnreadableResponse
        );
        assert_eq!(transport.payloads.len(), 2);
    }

    #[test]
    fn model_references_reads_one_records_page_with_same_authority_and_truthful_revisions() {
        let profile = crate::data_distribution::tests::profile_for_tests();
        let link = json!({"kind":"ds_object","object_type":"mv_model","project_id":"project-a","entity_id":"model-a","object_revision":"rev-a","model_version":3});
        let graph = json!({"graph":{"project_id":"project-a","revision":9,"schedule_revision":4,"tasks":[{"id":"task-a","data":{"title":"Model task","type":"task"},"links":[link.clone()]}],"dependencies":[],"residual_items":[],"calendar":{},"permissions":{"view":true,"comment":false,"contribute":false,"edit_schedule":false,"admin":false}}});
        let context = json!({"context":{"project_id":"project-a","loaded_collections":["pm_records"],"records":[{"id":"note-a","version":7,"metadata":{"updated_at":123},"data":{"category":"project_note","subject":"Canonical non-geometric note","memory":{"schema":"ds.project-note-memory/v1"}},"links":[link.clone()]},{"id":"other-category","version":2,"metadata":{"updated_at":120},"data":{"category":"instruction","subject":"Ignore"},"links":[link]}],"truncated_by_collection":{"pm_records":true},"next_cursors":{"pm_records":"boundary"}}});
        let invoke = |t: &mut AttachTransport, cursor: Option<&str>| {
            run(
                &profile,
                t,
                crate::authorized::Authority {
                    bearer_token: "bearer",
                    uid: "captured-uid",
                    email: "reader@example.test",
                    device_id: None,
                },
                "project-a",
                &Command::ModelReferences {
                    model_id: "model-a".into(),
                    revision_id: Some("rev-a".into()),
                    records_cursor: cursor.map(str::to_owned),
                },
            )
        };
        let mut t = attach_transport("ready", 1);
        t.started = graph.clone();
        t.finalized = context.clone();
        let got = invoke(&mut t, None).unwrap();
        assert_eq!(
            t.payloads,
            vec![
                json!({"action":"get_graph","ds_project":"project-a"}),
                json!({"action":"get_context","ds_project":"project-a","limit":100,"collections":["pm_records"]})
            ]
        );
        assert_eq!(
            t.authorities,
            vec![("captured-uid".into(), "reader@example.test".into()); 2]
        );
        assert_eq!(got["tasks"].as_array().unwrap().len(), 1);
        assert_eq!(got["notes"].as_array().unwrap().len(), 1);
        assert_eq!(got["notes"][0]["note_version"], 7);
        assert_eq!(got["notes"][0]["note_updated_at"], 123);
        assert_eq!(got["notes_count_kind"], "lower_bound");
        assert_eq!(got["notes_next_cursor"], "boundary");
        assert_eq!(got["revision"], 9);
        assert_eq!(got["schedule_revision"], 4);
        assert!(got["context_graph_revision"].is_null());
        assert_eq!(
            got["read_consistency"],
            "separate_reads_unversioned_context"
        );
        assert!(!got.to_string().contains("bearer"));
        assert!(t.uploaded.is_empty());
        t.payloads.clear();
        t.finalized["context"]["truncated_by_collection"]["pm_records"] = json!(false);
        t.finalized["context"]["next_cursors"] = json!({});
        assert_eq!(invoke(&mut t, None).unwrap()["notes_count_kind"], "exact");
        t.payloads.clear();
        let got = invoke(&mut t, Some("boundary")).unwrap();
        assert_eq!(got["notes_count_kind"], "lower_bound");
        assert_eq!(t.payloads[1]["cursors"], json!({"pm_records":"boundary"}));
        for (pointer, bad) in [
            ("/context/project_id", json!("foreign")),
            (
                "/context/loaded_collections",
                json!(["pm_records", "pm_comments"]),
            ),
            ("/context/truncated_by_collection", json!({"records":false})),
            ("/context/records/0/version", Value::Null),
            ("/context/records/0/metadata/updated_at", Value::Null),
            ("/context/next_cursors", json!({"pm_comments":"cursor"})),
        ] {
            t.finalized = context.clone();
            *t.finalized.pointer_mut(pointer).unwrap() = bad;
            t.payloads.clear();
            assert_eq!(
                invoke(&mut t, None).unwrap_err().kind(),
                ErrorKind::UnreadableResponse,
                "{pointer}"
            );
            assert_eq!(t.payloads.len(), 2);
        }
        t.started["graph"]["project_id"] = json!("foreign");
        t.payloads.clear();
        assert_eq!(
            invoke(&mut t, None).unwrap_err().kind(),
            ErrorKind::UnreadableResponse
        );
        assert_eq!(t.payloads.len(), 1);
        t.started = graph;
        t.upload_status = 403;
        t.payloads.clear();
        assert_eq!(
            invoke(&mut t, None).unwrap_err().kind(),
            ErrorKind::AuthenticationRejected
        );
        assert_eq!(t.payloads.len(), 1);
        t.payloads.clear();
        assert_eq!(
            invoke(&mut t, Some("bad/cursor")).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert!(t.payloads.is_empty());
    }

    #[test]
    fn attachment_files_are_bounded_named_plainly_and_typed() {
        assert_eq!(
            AttachmentFile::new("Plan.XLSX", None, vec![1])
                .unwrap()
                .content_type(),
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
        );
        assert_eq!(
            AttachmentFile::new("survey.dat", None, vec![1])
                .unwrap()
                .content_type(),
            "application/octet-stream"
        );
        assert_eq!(
            AttachmentFile::new("a.bin", Some("Image/PNG"), vec![1])
                .unwrap()
                .content_type(),
            "image/png"
        );
        for bad in ["", "  ", "../a.pdf", "a\\b.pdf", ".", "..", "a\u{0}.pdf"] {
            assert!(AttachmentFile::new(bad, None, vec![1]).is_err(), "{bad:?}");
        }
        assert!(AttachmentFile::new(&"n".repeat(256), None, vec![1]).is_err());
        assert!(AttachmentFile::new("a.pdf", None, Vec::new()).is_err());
        assert!(AttachmentFile::new("a.pdf", None, vec![0; MAX_ATTACHMENT_BYTES + 1]).is_err());
        for bad in ["pdf", "a/b/c", "text/plain; charset=utf-8", "a /b"] {
            assert!(
                AttachmentFile::new("a.pdf", Some(bad), vec![1]).is_err(),
                "{bad:?}"
            );
        }
        let debug = format!(
            "{:?}",
            AttachmentFile::new("a.pdf", None, vec![7; 4]).unwrap()
        );
        assert!(debug.contains("bytes: 4"), "{debug}");
        assert!(
            Command::TaskAttach {
                task_id: "a/b".into(),
                file: AttachmentFile::new("a.pdf", None, vec![1]).unwrap()
            }
            .validate()
            .is_err()
        );
    }
}

#[cfg(test)]
mod authoring_tests {
    use super::*;
    fn model() -> ds_command_kernel::project_management::authoring_model::AuthoringModel {
        serde_json::from_value(json!({"model":"GPT-6"})).unwrap()
    }
    #[test]
    fn authoring_none_preserves_reads_and_each_write_isolated() {
        let read = Command::Graph;
        assert_eq!(
            read.clone().with_authoring(None).unwrap().payload("p"),
            read.payload("p")
        );
        assert!(read.with_authoring(Some(model())).is_err());
        let write = Command::TaskCommentCreate {
            task_id: "t".into(),
            id: "c".into(),
            body: "Observed".into(),
        };
        let original = write.payload("p");
        assert_eq!(
            write.clone().with_authoring(None).unwrap().payload("p"),
            original
        );
        let declared = write.clone().with_authoring(Some(model())).unwrap();
        declared.validate().unwrap();
        let mut body = declared.payload("p");
        assert_eq!(
            body.as_object_mut().unwrap().remove("authoring_model"),
            Some(json!({"model":"GPT-6"}))
        );
        assert_eq!(body, original);
        assert_eq!(write.payload("p"), original);
        assert!(declared.with_authoring(Some(model())).is_err());
    }
    #[test]
    fn authored_milestone_uses_the_existing_commit_payload_without_new_authority() {
        let write = Command::Commit {
            command_id: "command-1".into(),
            base_revision: 7,
            command: json!({"kind":"create_task","create_task":{"id":"m","kind":"milestone"}}),
        };
        let original = write.payload("p");
        let declared = write.with_authoring(Some(model())).unwrap();
        declared.validate().unwrap();
        let mut payload = declared.payload("p");
        payload.as_object_mut().unwrap().remove("authoring_model");
        assert_eq!(payload, original);
    }
}

#[cfg(test)]
mod retained_graph_tests {
    use super::*;
    fn graph() -> Value {
        json!({"project_id":"p","revision":3,"schedule_revision":2,"calendar":{},"tasks":[],"dependencies":[],"residual_items":[],"permissions":{"view":true,"comment":false,"contribute":false,"edit_schedule":false,"admin":false}})
    }
    #[test]
    fn canonical_graph_requires_explicit_scope_revision_shape_and_captured_read_permission() {
        verify_graph("p", &graph()).unwrap();
        for (key, value) in [
            ("project_id", json!("other")),
            ("revision", json!(-1)),
            ("schedule_revision", Value::Null),
            ("tasks", json!({})),
            ("permissions", json!({})),
            ("retained", json!({})),
            ("correspondence", json!([])),
        ] {
            let mut bad = graph();
            bad[key] = value;
            assert!(verify_graph("p", &bad).is_err(), "{key}");
        }
        let mut denied = graph();
        denied["permissions"]["view"] = json!(false);
        assert!(verify_graph("p", &denied).is_err());
    }
    #[test]
    fn missing_legacy_correspondence_remains_distinct_from_partial_capture() {
        let old = graph();
        verify_graph("p", &old).unwrap();
        assert!(old.get("correspondence").is_none());
        let mut partial = graph();
        partial["correspondence"] = json!({"records":[],"truncated":true,"records_total":10,"as_of":"2026-10-13","due_window_days":7,"blocked_tasks":[],"counters":{"recordsOutstanding":10,"recordsOverdue":1,"tasksAwaitingCorrespondence":0}});
        verify_graph("p", &partial).unwrap();
        assert_eq!(partial["correspondence"]["truncated"], true);
        partial["correspondence"].as_object_mut().unwrap().remove("truncated");
        assert!(verify_graph("p", &partial).is_err());
    }
}
