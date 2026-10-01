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
/// PM subjects are advertised as 300 characters; `copyPMString` measures
/// UTF-8 bytes after trimming. Keep that server bound at this door.
const MAX_PROJECT_NOTE_SUBJECT_BYTES: usize = 300;
const MAX_PROJECT_NOTE_BODY_BYTES: usize = 128 * 1024;
/// Bound a minted ID while keeping the server's stable-reference alphabet.
const MAX_PROJECT_NOTE_ID_BYTES: usize = 256;
/// The most commands one saved draft may carry — ds-brain's own batch bound,
/// republished per project as `field_model.max_batch_commands`.
pub const MAX_BATCH_COMMANDS: usize = 200;

/// One governed project-management action.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// The project's whole canonical graph, at its current revision.
    ///
    /// There is no narrower read: ds-brain answers the graph whole, and every
    /// projection — the plan, a task list, one task — is a fold over it. A
    /// per-task endpoint would be a second answer that could disagree with
    /// this one.
    Graph,
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
    /// The context rows the graph does not carry: records, work episodes and
    /// attachments, at most `limit` per collection (the server's default when
    /// `None`).
    ///
    /// These are a SEPARATE read on purpose, and the browser never made it:
    /// `decodeProjectGraph` hard-codes `records: []`, so `ds pm record list`
    /// answers nothing however many records a project holds. A window does not
    /// fix that; asking for them does.
    Context { limit: Option<i64> },
    /// Create a quick, project-scoped Markdown note through `create_record`.
    /// The server owns defaults, permissions, versioning and ID generation
    /// when no stable ID is supplied.
    ProjectNoteCreate {
        id: Option<String>,
        title: String,
        body: String,
    },
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
}

fn command_id_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_COMMAND_ID_BYTES
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn project_note_id_valid(id: &str) -> bool {
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
    fn action(&self) -> &'static str {
        match self {
            Self::Graph => "get_graph",
            Self::PublicProjects { .. } => "list_public_projects",
            Self::TaskCommentCreate { .. } => "create_comment",
            Self::TaskComments { .. } => "get_context",
            Self::Assignees => "get_assignees",
            Self::Context { .. } => "get_context",
            Self::ProjectNoteCreate { .. } => "create_record",
            Self::Commit { .. } => "commit_command",
            Self::CommitBatch { .. } => "commit_batch",
            Self::Propose { .. } => "commit_command",
            Self::RequestAdmission { .. } => "task_request_admission",
            Self::Admit { .. } => "task_admit",
            Self::DeclineAdmission { .. } => "task_decline_admission",
            Self::LogHours { .. } => "task_log_hours",
        }
    }

    /// Refuse locally what the server would refuse by name, before a round
    /// trip is spent on it.
    pub fn validate(&self) -> Result<(), ClientError> {
        let invalid = |message: &'static str| ClientError::new(ErrorKind::InvalidInput, message);
        match self {
            Self::Graph | Self::Assignees => Ok(()),
            Self::PublicProjects {
                status,
                cursor,
                limit,
            } => {
                if !matches!(status.as_str(), "active" | "archived" | "testing")
                    || !(1..=100).contains(limit)
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
            Self::Context { limit } => match limit {
                Some(limit) if !(1..=MAX_CONTEXT_LIMIT).contains(limit) => Err(invalid(
                    "project management reads 1 through 100 context rows per collection",
                )),
                _ => Ok(()),
            },
            Self::ProjectNoteCreate { id, title, body } => {
                let subject = title.trim();
                if subject.is_empty()
                    || subject.len() > MAX_PROJECT_NOTE_SUBJECT_BYTES
                    || subject
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                {
                    return Err(invalid(
                        "a project note needs a meaningful title of at most 300 UTF-8 bytes",
                    ));
                }
                if body.len() > MAX_PROJECT_NOTE_BODY_BYTES {
                    return Err(invalid("a project note body is at most 128 KiB of UTF-8"));
                }
                if id.as_deref().is_some_and(|id| !project_note_id_valid(id)) {
                    return Err(invalid("a project note needs one bounded stable id"));
                }
                Ok(())
            }
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
            Self::Graph | Self::Assignees => {}
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
            Self::Context { limit } => {
                if let Some(limit) = limit {
                    payload["limit"] = json!(limit);
                }
            }
            Self::ProjectNoteCreate { id, title, body } => {
                if let Some(id) = id {
                    payload["id"] = json!(id);
                }
                payload["data"] = json!({
                    "category": "project_note",
                    "record_type": "quick",
                    "subject": title,
                    "body": body,
                });
            }
            Self::Commit {
                command_id,
                base_revision,
                command,
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
fn refusal(status: u16, body: &[u8]) -> ClientError {
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
    let response = transport.project_management(ProjectManagementCall {
        profile,
        authority: authority.call(),
        payload: command.payload(project),
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
        Command::Context { .. } | Command::TaskComments { .. } => data
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
        Command::PublicProjects { .. }
        | Command::TaskCommentCreate { .. }
        | Command::ProjectNoteCreate { .. }
        | Command::Propose { .. }
        | Command::RequestAdmission { .. }
        | Command::Admit { .. }
        | Command::DeclineAdmission { .. }
        | Command::LogHours { .. } => data,
    };
    Ok(graph)
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
    fn project_note_create_payload_routes_the_explicit_project_and_stable_id() {
        let command = Command::ProjectNoteCreate {
            id: Some("note:review@example.com+2026_09-30.é".into()),
            title: "Clearance review".into(),
            body: "## Findings\n\n- Check **pole 12**.\n".into(),
        };
        command.validate().expect("valid project note");
        assert_eq!(command.action(), "create_record");
        for project in ["project-a", "project-b"] {
            assert_eq!(
                command.payload(project),
                json!({
                    "action": "create_record",
                    "ds_project": project,
                    "id": "note:review@example.com+2026_09-30.é",
                    "data": {
                        "category": "project_note",
                        "record_type": "quick",
                        "subject": "Clearance review",
                        "body": "## Findings\n\n- Check **pole 12**.\n",
                    },
                })
            );
        }
    }

    #[test]
    fn project_note_create_payload_omits_an_absent_id() {
        let command = Command::ProjectNoteCreate {
            id: None,
            title: "Follow up".into(),
            body: String::new(),
        };
        command.validate().expect("the server may mint the id");
        let payload = command.payload("project-c");
        assert_eq!(
            payload,
            json!({
                "action": "create_record",
                "ds_project": "project-c",
                "data": {
                    "category": "project_note",
                    "record_type": "quick",
                    "subject": "Follow up",
                    "body": "",
                },
            })
        );
        assert!(payload.get("id").is_none());
    }

    #[test]
    fn project_note_title_is_meaningful_and_within_the_server_subject_bound() {
        for title in ["a".repeat(300), "é".repeat(150), "  Review  ".into()] {
            Command::ProjectNoteCreate {
                id: None,
                title,
                body: String::new(),
            }
            .validate()
            .expect("a bounded meaningful subject");
        }
        for title in [
            String::new(),
            " \t\n\u{2003}".into(),
            "\0".into(),
            "a".repeat(301),
            "é".repeat(151),
        ] {
            let error = Command::ProjectNoteCreate {
                id: None,
                title,
                body: String::new(),
            }
            .validate()
            .expect_err("an invalid subject is refused locally");
            assert_eq!(error.kind(), ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn project_note_body_bound_counts_utf8_bytes() {
        for body in ["a".repeat(128 * 1024), "é".repeat(64 * 1024)] {
            Command::ProjectNoteCreate {
                id: None,
                title: "Review".into(),
                body: body.clone(),
            }
            .validate()
            .expect("exactly 128 KiB is accepted");
            let error = Command::ProjectNoteCreate {
                id: None,
                title: "Review".into(),
                body: format!("{body}a"),
            }
            .validate()
            .expect_err("one byte over the body bound is refused locally");
            assert_eq!(error.kind(), ErrorKind::InvalidInput);
        }
    }

    #[test]
    fn project_note_ids_accept_stable_references_and_refuse_invalid_bounds() {
        for id in [
            "550e8400-e29b-41d4-a716-446655440000".into(),
            "note:review@example.com+2026_09-30.é".into(),
            "a".repeat(256),
            "é".repeat(128),
        ] {
            Command::ProjectNoteCreate {
                id: Some(id),
                title: "Review".into(),
                body: String::new(),
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
            let error = Command::ProjectNoteCreate {
                id: Some(id),
                title: "Review".into(),
                body: String::new(),
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
        assert_eq!(Command::Context { limit: None }.action(), "get_context");
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
        let context = Command::Context { limit: Some(100) }.payload("p");
        assert_eq!(context["limit"], 100);
        assert!(
            Command::Context { limit: None }
                .payload("p")
                .get("limit")
                .is_none()
        );
    }

    /// The bounds the server enforces are refused here first, by the same
    /// numbers, so a round trip is never spent to learn them.
    #[test]
    fn a_command_outside_its_bounds_is_refused_before_it_is_sent() {
        let kind = |command: Command| command.validate().expect_err("refused").kind();
        assert_eq!(
            kind(Command::Context { limit: Some(0) }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            kind(Command::Context { limit: Some(101) }),
            ErrorKind::InvalidInput
        );
        assert!(Command::Context { limit: Some(100) }.validate().is_ok());
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
}
