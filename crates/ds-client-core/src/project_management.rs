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

use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    ProjectManagementCall, ServiceRefusal,
};

/// The largest plan answer this owner will read. A project's whole graph —
/// tasks, milestones, records, residuals — is bigger than an inventory page
/// and smaller than a design room.
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

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
    /// The context rows the graph does not carry: records, work episodes and
    /// attachments.
    ///
    /// These are a SEPARATE read on purpose, and the browser never made it:
    /// `decodeProjectGraph` hard-codes `records: []`, so `ds pm record list`
    /// answers nothing however many records a project holds. A window does not
    /// fix that; asking for them does.
    Context,
    /// Task proposals (ds-brain `docs/contracts/task-proposals.md`). Each
    /// write is one governed command against a graph revision; the command id
    /// is the idempotency key, so a retry after a lost answer replays the
    /// ledger rather than committing twice.
    ///
    /// A proposal: one `create_task` shaped as the contract's proposal — an
    /// Inbox root about the caller's own work, with the estimate. The gateway
    /// admits a contributor exactly for this shape.
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
    /// `parent_task_id` empty admits at the top level of the WBS.
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

impl Command {
    fn action(&self) -> &'static str {
        match self {
            Self::Graph => "get_graph",
            Self::Context => "get_context",
            Self::Propose { .. } => "commit_command",
            Self::RequestAdmission { .. } => "task_request_admission",
            Self::Admit { .. } => "task_admit",
            Self::DeclineAdmission { .. } => "task_decline_admission",
            Self::LogHours { .. } => "task_log_hours",
        }
    }

    /// The whole body the gateway decodes with `DisallowUnknownFields`: a key
    /// this function does not write is a key the request cannot carry.
    fn payload(&self, project: &str) -> Value {
        let mut body = Map::new();
        body.insert("action".into(), json!(self.action()));
        body.insert("ds_project".into(), json!(project));
        let mut optional = |key: &str, value: Option<Value>| {
            if let Some(value) = value {
                body.insert(key.into(), value);
            }
        };
        match self {
            Self::Graph | Self::Context => {}
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
                optional("command_id", Some(json!(command_id)));
                optional("base_revision", Some(json!(base_revision)));
                optional(
                    "command",
                    Some(json!({"kind": "create_task", "create_task": Value::Object(create)})),
                );
            }
            Self::RequestAdmission {
                command_id,
                base_revision,
                task_id,
                note,
            } => {
                optional("command_id", Some(json!(command_id)));
                optional("base_revision", Some(json!(base_revision)));
                optional("task_id", Some(json!(task_id)));
                optional("note", note.as_ref().map(|text| json!(text)));
            }
            Self::Admit {
                command_id,
                base_revision,
                task_id,
                parent_task_id,
                target_index,
            } => {
                optional("command_id", Some(json!(command_id)));
                optional("base_revision", Some(json!(base_revision)));
                optional("task_id", Some(json!(task_id)));
                if !parent_task_id.is_empty() {
                    optional("parent_task_id", Some(json!(parent_task_id)));
                }
                optional("target_index", target_index.map(|index| json!(index)));
            }
            Self::DeclineAdmission {
                command_id,
                base_revision,
                task_id,
                reason,
            } => {
                optional("command_id", Some(json!(command_id)));
                optional("base_revision", Some(json!(base_revision)));
                optional("task_id", Some(json!(task_id)));
                optional("reason", Some(json!(reason)));
            }
            Self::LogHours {
                command_id,
                base_revision,
                task_id,
                hours,
                note,
            } => {
                optional("command_id", Some(json!(command_id)));
                optional("base_revision", Some(json!(base_revision)));
                optional("task_id", Some(json!(task_id)));
                optional("hours", Some(json!(hours)));
                optional("note", note.as_ref().map(|text| json!(text)));
            }
        }
        Value::Object(body)
    }

    const fn is_write(&self) -> bool {
        !matches!(self, Self::Graph | Self::Context)
    }
}

/// A governed refusal, bounded at this boundary. `PM_REFUSED` carries the
/// contract's reason token in `details.reason`; that token is the code a host
/// plans against (`task_already_placed`, `admission_not_requested`, …). Every
/// other envelope code crosses lower-cased (`pm_revision_conflict`), and the
/// gateway's sentence travels as the message.
fn refusal_from(status: u16, body: &[u8]) -> ClientError {
    let envelope: Option<Value> = serde_json::from_slice(body).ok();
    let error = envelope.as_ref().and_then(|value| value.get("error"));
    let code = error
        .and_then(|value| value.get("code"))
        .and_then(Value::as_str);
    let reason = error
        .and_then(|value| value.get("details"))
        .and_then(|details| details.get("reason"))
        .and_then(Value::as_str);
    let message = error
        .and_then(|value| value.get("message"))
        .and_then(Value::as_str)
        .or_else(|| {
            envelope
                .as_ref()
                .and_then(|value| value.get("message"))
                .and_then(Value::as_str)
        });
    let token = match (code, reason) {
        (Some("PM_REFUSED"), Some(reason)) => Some(reason.to_ascii_lowercase()),
        (Some(code), _) => Some(code.to_ascii_lowercase()),
        (None, _) => None,
    };
    ClientError::with_service_refusal(
        crate::authorized::classify_tile_error(status).kind(),
        "project management refused the command",
        ServiceRefusal::new(status, token.as_deref(), message),
    )
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
pub fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    command: &Command,
) -> Result<Value, ClientError> {
    let project = readable_project(project)?;
    let response = transport.project_management(ProjectManagementCall {
        profile,
        authority: authority.call(),
        payload: command.payload(project),
    })?;
    if response.status != 200 {
        if command.is_write() && response.body.len() <= MAX_RESPONSE_BYTES {
            return Err(refusal_from(response.status, &response.body));
        }
        return Err(crate::authorized::classify_tile_error(response.status));
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
        // A write answers `{result}`: the ordinary command result, whose
        // `applied` and `violations` the host reads. It is handed back whole.
        Command::Context
        | Command::Propose { .. }
        | Command::RequestAdmission { .. }
        | Command::Admit { .. }
        | Command::DeclineAdmission { .. }
        | Command::LogHours { .. } => data,
    };
    Ok(graph)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_action_vocabulary_is_closed_and_named() {
        assert_eq!(Command::Graph.action(), "get_graph");
        assert_eq!(Command::Context.action(), "get_context");
        assert_eq!(
            Command::RequestAdmission {
                command_id: "c".into(),
                base_revision: 1,
                task_id: "t".into(),
                note: None
            }
            .action(),
            "task_request_admission"
        );
        assert_eq!(
            Command::Graph.payload("p"),
            json!({"action": "get_graph", "ds_project": "p"})
        );
    }

    /// The proposal body is exactly the contract's shape: an Inbox root,
    /// unscheduled, about the caller, with the estimate; absent options are
    /// absent keys, never nulls the gateway would refuse by name.
    #[test]
    fn a_proposal_is_one_create_task_of_the_contracts_shape() {
        let body = Command::Propose {
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
            body,
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
        assert!(
            admit.get("parent_task_id").is_none(),
            "root admission names no parent"
        );
        assert!(admit.get("target_index").is_none());
        let hours = Command::LogHours {
            command_id: "cmd-hours-0001".into(),
            base_revision: 6,
            task_id: "p1".into(),
            hours: 4.5,
            note: Some("first day".into()),
        }
        .payload("proj");
        assert_eq!(hours["hours"], 4.5);
        assert_eq!(hours["note"], "first day");
    }

    /// A `PM_REFUSED` crosses as the contract's reason token; another code
    /// crosses lower-cased; the sentence travels as the message.
    #[test]
    fn a_governed_refusal_crosses_as_the_contracts_reason() {
        let refused = refusal_from(
            409,
            br#"{"success":false,"error":{"code":"PM_REFUSED","message":"Survey is already placed in the plan","details":{"reason":"task_already_placed","task_ids":["phase"]}}}"#,
        );
        let refusal = refused.service_refusal().expect("a bounded refusal");
        assert_eq!(refusal.status(), 409);
        assert_eq!(refusal.code(), Some("task_already_placed"));
        assert!(refusal
            .message()
            .is_some_and(|m| m.contains("already placed")));
        let conflict = refusal_from(
            409,
            br#"{"success":false,"error":{"code":"PM_REVISION_CONFLICT","message":"The project schedule changed. Refresh and retry.","details":{"base_revision":3,"current_revision":4}}}"#,
        );
        assert_eq!(
            conflict.service_refusal().and_then(ServiceRefusal::code),
            Some("pm_revision_conflict")
        );
        let forbidden = refusal_from(403, br#"{"success":false,"error":{"code":"FORBIDDEN","message":"Project schedule editor access required"}}"#);
        let refusal = forbidden.service_refusal().expect("a bounded refusal");
        assert_eq!((refusal.status(), refusal.code()), (403, Some("forbidden")));
        let unreadable = refusal_from(500, b"not json");
        assert_eq!(
            unreadable.service_refusal().map(ServiceRefusal::status),
            Some(500)
        );
        assert_eq!(unreadable.kind(), ErrorKind::Transient);
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
