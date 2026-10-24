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

use serde_json::{json, Map, Value};

mod attachment_file;
mod attachment_upload;
pub mod checklist;
pub mod create_attached;
pub mod documents;
mod model_references;
mod overview;
pub mod project_notes;

use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    ProjectManagementCall, ServiceRefusal,
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
/// The largest file one task or record attachment may carry: Cloud Storage's
/// own 5 TiB object maximum, the same bound ds-brain states before it mints a
/// session (`maxPMAttachmentBytes`). There is no product cap; files stream.
pub const MAX_ATTACHMENT_BYTES: u64 =
    ds_command_kernel::project_management::attachment_upload::MAX_ATTACHMENT_BYTES;
pub use attachment_file::AttachmentFile;

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
    /// Attach one file to one task through the Kernel attachment upload flow:
    /// it completes the caller's own interrupted reservation of the same file
    /// or reserves a new row, sends the bytes to storage, and accepts only the
    /// row ds-brain verified `ready`.
    TaskAttach {
        task_id: String,
        file: AttachmentFile,
    },
    /// The same resumable upload flow onto one record (a correspondence record
    /// or a project note): a retry completes the caller's interrupted row.
    RecordAttach {
        record_id: String,
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
            Self::TaskAttach { .. } | Self::RecordAttach { .. } => "start_attachment_upload",
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
            Self::RecordAttach { record_id, .. } => {
                if !project_note_id_valid(record_id) {
                    return Err(invalid("a record attachment names one stable record id"));
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
            | Self::ModelReferences { .. }
            | Self::TaskAttach { .. }
            | Self::RecordAttach { .. } => {}
            Self::TaskDetach { request } => return request.payload(project),
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
        return attachment_upload::attach(
            profile,
            transport,
            authority,
            project,
            attachment_upload::Entity::Task(task_id),
            file,
        );
    }
    if let Command::RecordAttach { record_id, file } = command {
        return attachment_upload::attach(
            profile,
            transport,
            authority,
            project,
            attachment_upload::Entity::Record(record_id),
            file,
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
        | Command::RecordAttach { .. }
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
        if !c["truncated"].is_boolean()
            || total < records.len() as u64
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
#[path = "project_management_authoring_tests.rs"]
mod authoring_tests;
#[cfg(test)]
#[path = "project_management_collaboration_tests.rs"]
mod collaboration_contract_tests;
#[cfg(test)]
#[path = "project_management_retained_graph_tests.rs"]
mod retained_graph_tests;
#[cfg(test)]
#[path = "project_management_tests.rs"]
mod tests;
