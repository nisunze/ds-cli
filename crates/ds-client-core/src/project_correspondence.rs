//! Correspondence, headlessly: parties, records, threads and the blockers a
//! task carries (ds-brain `docs/contracts/correspondence.md`, §ds-brain API
//! bodies).
//!
//! The same door as [`crate::project_management`] — `POST /api/v1/pm`, the
//! bearer as the authority, no pairing and no device — carrying the
//! correspondence actions that contract added. Every `ds pm party …`, `ds pm
//! record …` and `ds pm task block|unblock|read --timeline` request is one
//! [`Action`] here; what the answer MEANS is the kernel's and the CLI's.
//!
//! **This module is the DOOR, not the decisions.** It sends a closed set of
//! actions with the fields each action reads — ds-brain decodes the envelope
//! under `DisallowUnknownFields` and refuses a field an action does not read
//! BY NAME, so a key this file does not write is a key the request cannot
//! carry — and returns what the server said. A refusal crosses the boundary
//! as the server's own token: the contract's `PM_REFUSED` envelope carries
//! `details.reason` (`record_source_required`, `party_exists`, …), and that
//! reason is the code a caller plans against, with the ids and numbers the
//! rule named beside it.

use serde_json::{json, Map, Value};

use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind,
    ProjectManagementCall, ServiceRefusal,
};

/// The largest answer this door will read: a thread of records with their
/// attachments, or a task timeline, is smaller than a whole graph.
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
/// ds-brain's page bound for `party_list` and `record_list`.
pub const MAX_PAGE: i64 = 100;
/// The longest reason a manual unblock or a waiver carries (contract: ≤ 300).
pub const MAX_REASON_CHARS: usize = 300;
/// The longest command id one blocker command may carry.
const MAX_COMMAND_ID_BYTES: usize = 128;

/// The filters of `record_list`; every member optional, sent only when set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecordFilters {
    pub query: Option<String>,
    pub category: Option<String>,
    pub thread_id: Option<String>,
    pub direction: Option<String>,
    pub channel: Option<String>,
    pub party_id: Option<String>,
    pub awaiting: bool,
    pub overdue: bool,
    pub since: Option<String>,
    pub limit: Option<i64>,
    pub page: Option<i64>,
}

/// The filters of `party_list`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PartyFilters {
    pub query: Option<String>,
    pub role: Option<String>,
    pub kind: Option<String>,
    pub include_archived: bool,
    pub limit: Option<i64>,
    pub page: Option<i64>,
}

/// One governed correspondence action — the closed vocabulary of this door.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    PartyList(PartyFilters),
    /// `data` is the party's authored fields, exactly as the contract lists
    /// them; `id` mints (`--id`).
    PartyCreate {
        id: Option<String>,
        data: Map<String, Value>,
    },
    PartyUpdate {
        id: String,
        expected_version: i64,
        data: Map<String, Value>,
    },
    RecordCreate {
        id: Option<String>,
        data: Map<String, Value>,
    },
    /// `record_id` is the parent; the reply inherits its thread.
    RecordReply {
        record_id: String,
        id: Option<String>,
        data: Map<String, Value>,
    },
    RecordUpdate {
        id: String,
        expected_version: i64,
        data: Map<String, Value>,
    },
    RecordRead {
        record_id: String,
    },
    RecordThread {
        record_id: String,
    },
    RecordList(RecordFilters),
    TaskBlock {
        command_id: String,
        base_revision: i64,
        task_id: String,
        record_id: String,
    },
    TaskUnblock {
        command_id: String,
        base_revision: i64,
        task_id: String,
        record_id: String,
        reason: String,
    },
    TaskRead {
        task_id: String,
        timeline: bool,
    },
    /// One ordinary project command against one base revision —
    /// `commit_command`, as the Plan sheet sends it. The command is already
    /// shaped `{kind, <kind>: {…}}`; ds-brain rechecks permission, membership
    /// and revision. `create_task` carries `from_record_id` here.
    Commit {
        command_id: String,
        base_revision: i64,
        command: Value,
    },
}

impl Action {
    fn name(&self) -> &'static str {
        match self {
            Self::PartyList(_) => "party_list",
            Self::PartyCreate { .. } => "party_create",
            Self::PartyUpdate { .. } => "party_update",
            Self::RecordCreate { .. } => "record_create",
            Self::RecordReply { .. } => "record_reply",
            Self::RecordUpdate { .. } => "record_update",
            Self::RecordRead { .. } => "record_read",
            Self::RecordThread { .. } => "record_thread",
            Self::RecordList(_) => "record_list",
            Self::TaskBlock { .. } => "task_block",
            Self::TaskUnblock { .. } => "task_unblock",
            Self::TaskRead { .. } => "task_read",
            Self::Commit { .. } => "commit_command",
        }
    }

    /// Refuse locally what the server would refuse by name, before a round
    /// trip is spent on it.
    pub fn validate(&self) -> Result<(), ClientError> {
        let invalid = |message: &'static str| ClientError::new(ErrorKind::InvalidInput, message);
        let page_ok = |limit: Option<i64>, page: Option<i64>| {
            limit.is_none_or(|limit| (1..=MAX_PAGE).contains(&limit))
                && page.is_none_or(|page| page >= 1)
        };
        match self {
            Self::PartyList(filters) => {
                if !page_ok(filters.limit, filters.page) {
                    return Err(invalid(
                        "a party list reads 1 through 100 rows per page, from page 1",
                    ));
                }
            }
            Self::RecordList(filters) => {
                if !page_ok(filters.limit, filters.page) {
                    return Err(invalid(
                        "a record list reads 1 through 100 rows per page, from page 1",
                    ));
                }
            }
            Self::PartyCreate { id, data } | Self::RecordCreate { id, data } => {
                if id.as_deref().is_some_and(|id| !identifier(id)) {
                    return Err(invalid("an id to mint is one bounded identifier"));
                }
                if data.is_empty() {
                    return Err(invalid("a create carries at least one authored field"));
                }
            }
            Self::RecordReply {
                record_id,
                id,
                data,
            } => {
                if !identifier(record_id) {
                    return Err(invalid("a reply names the record it answers"));
                }
                if id.as_deref().is_some_and(|id| !identifier(id)) {
                    return Err(invalid("an id to mint is one bounded identifier"));
                }
                if data.is_empty() {
                    return Err(invalid("a reply carries at least one authored field"));
                }
            }
            Self::PartyUpdate {
                id,
                expected_version,
                data,
            }
            | Self::RecordUpdate {
                id,
                expected_version,
                data,
            } => {
                if !identifier(id) {
                    return Err(invalid("an update names what it updates"));
                }
                if *expected_version < 1 {
                    return Err(invalid("an update carries the positive version it read"));
                }
                if data.is_empty() {
                    return Err(invalid("an update carries at least one field to change"));
                }
            }
            Self::RecordRead { record_id } | Self::RecordThread { record_id } => {
                if !identifier(record_id) {
                    return Err(invalid("a record read names one record"));
                }
            }
            Self::TaskBlock {
                command_id,
                base_revision,
                task_id,
                record_id,
            } => {
                if !command_id_valid(command_id) || *base_revision < 0 {
                    return Err(invalid(
                        "a blocker command needs one bounded command id and a non-negative base revision",
                    ));
                }
                if !identifier(task_id) || !identifier(record_id) {
                    return Err(invalid("a blocker names one task and one record"));
                }
            }
            Self::TaskUnblock {
                command_id,
                base_revision,
                task_id,
                record_id,
                reason,
            } => {
                if !command_id_valid(command_id) || *base_revision < 0 {
                    return Err(invalid(
                        "a blocker command needs one bounded command id and a non-negative base revision",
                    ));
                }
                if !identifier(task_id) || !identifier(record_id) {
                    return Err(invalid("a blocker names one task and one record"));
                }
                let reason = reason.trim();
                if reason.is_empty() || reason.chars().count() > MAX_REASON_CHARS {
                    return Err(invalid(
                        "a manual clear carries a reason of 1 through 300 characters",
                    ));
                }
            }
            Self::TaskRead { task_id, .. } => {
                if !identifier(task_id) {
                    return Err(invalid("a task read names one task"));
                }
            }
            Self::Commit {
                command_id,
                base_revision,
                command,
            } => {
                if !command_id_valid(command_id) || *base_revision < 0 {
                    return Err(invalid(
                        "a project command needs one bounded command id and a non-negative base revision",
                    ));
                }
                let shaped = command.as_object().is_some_and(|object| {
                    object
                        .get("kind")
                        .and_then(Value::as_str)
                        .is_some_and(|kind| {
                            !kind.is_empty()
                                && object.len() == 2
                                && object.get(kind).is_some_and(Value::is_object)
                        })
                });
                if !shaped {
                    return Err(invalid(
                        "a project command is one `{kind, <kind>: {…}}` object",
                    ));
                }
            }
        }
        Ok(())
    }

    fn payload(&self, project: &str) -> Value {
        let mut payload = json!({ "action": self.name(), "ds_project": project });
        let set = |payload: &mut Value, key: &str, value: Option<&String>| {
            if let Some(value) = value.map(|v| v.trim()).filter(|v| !v.is_empty()) {
                payload[key] = json!(value);
            }
        };
        match self {
            Self::PartyList(filters) => {
                set(&mut payload, "query", filters.query.as_ref());
                set(&mut payload, "role", filters.role.as_ref());
                set(&mut payload, "kind", filters.kind.as_ref());
                if filters.include_archived {
                    payload["include_archived"] = json!(true);
                }
                if let Some(limit) = filters.limit {
                    payload["limit"] = json!(limit);
                }
                if let Some(page) = filters.page {
                    payload["page"] = json!(page);
                }
            }
            Self::PartyCreate { id, data } | Self::RecordCreate { id, data } => {
                set(&mut payload, "id", id.as_ref());
                payload["data"] = Value::Object(data.clone());
            }
            Self::RecordReply {
                record_id,
                id,
                data,
            } => {
                payload["record_id"] = json!(record_id);
                set(&mut payload, "id", id.as_ref());
                payload["data"] = Value::Object(data.clone());
            }
            Self::PartyUpdate {
                id,
                expected_version,
                data,
            }
            | Self::RecordUpdate {
                id,
                expected_version,
                data,
            } => {
                payload["id"] = json!(id);
                payload["expected_version"] = json!(expected_version);
                payload["data"] = Value::Object(data.clone());
            }
            Self::RecordRead { record_id } | Self::RecordThread { record_id } => {
                payload["record_id"] = json!(record_id);
            }
            Self::RecordList(filters) => {
                set(&mut payload, "query", filters.query.as_ref());
                set(&mut payload, "category", filters.category.as_ref());
                set(&mut payload, "thread_id", filters.thread_id.as_ref());
                set(&mut payload, "direction", filters.direction.as_ref());
                set(&mut payload, "channel", filters.channel.as_ref());
                set(&mut payload, "party_id", filters.party_id.as_ref());
                set(&mut payload, "since", filters.since.as_ref());
                if filters.awaiting {
                    payload["awaiting"] = json!(true);
                }
                if filters.overdue {
                    payload["overdue"] = json!(true);
                }
                if let Some(limit) = filters.limit {
                    payload["limit"] = json!(limit);
                }
                if let Some(page) = filters.page {
                    payload["page"] = json!(page);
                }
            }
            Self::TaskBlock {
                command_id,
                base_revision,
                task_id,
                record_id,
            } => {
                payload["command_id"] = json!(command_id);
                payload["base_revision"] = json!(base_revision);
                payload["task_id"] = json!(task_id);
                payload["record_id"] = json!(record_id);
            }
            Self::TaskUnblock {
                command_id,
                base_revision,
                task_id,
                record_id,
                reason,
            } => {
                payload["command_id"] = json!(command_id);
                payload["base_revision"] = json!(base_revision);
                payload["task_id"] = json!(task_id);
                payload["record_id"] = json!(record_id);
                payload["reason"] = json!(reason.trim());
            }
            Self::TaskRead { task_id, timeline } => {
                payload["task_id"] = json!(task_id);
                if *timeline {
                    payload["timeline"] = json!(true);
                }
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
        }
        payload
    }
}

/// A record, party or task id as ds-brain mints or accepts one: bounded,
/// printable, no separators that could reach a path.
fn identifier(id: &str) -> bool {
    let id = id.trim();
    !id.is_empty()
        && id.len() <= 128
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

fn command_id_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_COMMAND_ID_BYTES
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// Classify one refused action, carrying the server's own code, sentence and
/// named details.
///
/// ds-brain's PM handler answers every refusal in the DS envelope
/// (`{success: false, error: {code, message, details}}`). A named
/// correspondence refusal is `PM_REFUSED` with `details.reason` set to the
/// contract's token — `record_source_required`, `party_exists`, … — and THAT
/// token is the code carried, because it is what a caller plans against;
/// every other envelope code (`PM_REVISION_CONFLICT`, `INSUFFICIENT_PERMISSIONS`,
/// `VALIDATION_FAILED`, `NOT_FOUND`, `ASSET_CLASS_FORBIDDEN`) crosses in lower
/// case as the code it is. The ids and numbers the rule named cross as
/// bounded details.
/// The DS refusal envelope as a bounded [`ServiceRefusal`], or `None` when
/// the body carried no envelope at all (a gateway page, an outage).
///
/// A `PM_REFUSED` / `ASSET_REFUSED` envelope's `details.reason` IS the code:
/// it is the contract's own token and what a caller plans against. Every
/// other envelope code crosses in lower case as the code it is.
pub(crate) fn envelope(status: u16, body: &[u8]) -> Option<ServiceRefusal> {
    let value: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let error = &value["error"];
    let envelope_code = error["code"].as_str().map(str::to_ascii_lowercase);
    let details = error["details"].as_object();
    let reason = details
        .and_then(|details| details.get("reason"))
        .and_then(Value::as_str)
        .filter(|reason| !reason.is_empty());
    let code = match (envelope_code.as_deref(), reason) {
        (Some("pm_refused") | Some("asset_refused"), Some(reason)) => Some(reason.to_string()),
        (other, _) => other.map(str::to_string),
    };
    let served =
        ServiceRefusal::with_details(status, code.as_deref(), error["message"].as_str(), details);
    if served.code().is_none() && served.message().is_none() {
        return None;
    }
    Some(served)
}

pub(crate) fn refusal(status: u16, body: &[u8]) -> ClientError {
    let served = envelope(status, body);
    let (kind, message) = match status {
        400 | 422 => (
            ErrorKind::InvalidInput,
            "correspondence refused this request",
        ),
        401 | 403 => (
            ErrorKind::AuthenticationRejected,
            "correspondence refused this verified identity or permission",
        ),
        404 => (
            ErrorKind::ResourceNotFound,
            "correspondence could not find what the request named",
        ),
        409 => (
            ErrorKind::InvalidInput,
            "correspondence refused a request against state that moved or already exists",
        ),
        _ => (
            ErrorKind::Transient,
            "correspondence is temporarily unavailable",
        ),
    };
    match served {
        Some(served) => ClientError::with_service_refusal(kind, message, served),
        None => ClientError::new(kind, message),
    }
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "correspondence did not answer in a shape this build understands",
    )
}

/// Send one governed action for one project and return its `data`.
pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    project: &str,
    action: &Action,
) -> Result<Value, ClientError> {
    let project = project.trim();
    if project.is_empty() {
        return Err(ClientError::new(
            ErrorKind::InvalidInput,
            "correspondence needs the project it is reading",
        ));
    }
    action.validate()?;
    let response = transport.project_management(ProjectManagementCall {
        profile,
        authority: authority.call(),
        payload: action.payload(project),
    })?;
    if response.status != 200 {
        return Err(refusal(response.status, &response.body));
    }
    if response.body.len() > MAX_RESPONSE_BYTES {
        return Err(unreadable());
    }
    let body: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    if body["success"] != true {
        return Err(unreadable());
    }
    let data = body.get("data").cloned().unwrap_or(Value::Null);
    if data.is_null() {
        return Err(unreadable());
    }
    // A blocker command answers `{result: {…}}`; the result is what a caller
    // reads. Every other action's data IS the answer.
    Ok(match action {
        Action::TaskBlock { .. } | Action::TaskUnblock { .. } | Action::Commit { .. } => data
            .get("result")
            .filter(|result| result.is_object())
            .cloned()
            .unwrap_or(data),
        _ => data,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn object(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("object")
    }

    #[test]
    fn every_action_names_itself_and_writes_only_the_fields_it_reads() {
        let list = Action::RecordList(RecordFilters {
            awaiting: true,
            party_id: Some("p_1".into()),
            limit: Some(20),
            ..RecordFilters::default()
        });
        let payload = list.payload("proj");
        assert_eq!(payload["action"], "record_list");
        assert_eq!(payload["ds_project"], "proj");
        assert_eq!(payload["awaiting"], true);
        assert_eq!(payload["party_id"], "p_1");
        assert_eq!(payload["limit"], 20);
        assert!(
            payload.get("overdue").is_none(),
            "an unset flag is not sent as false"
        );
        assert!(payload.get("query").is_none());

        let reply = Action::RecordReply {
            record_id: "R1".into(),
            id: None,
            data: object(json!({"direction": "outbound", "subject": "Re"})),
        };
        let payload = reply.payload("proj");
        assert_eq!(payload["action"], "record_reply");
        assert_eq!(payload["record_id"], "R1");
        assert!(payload.get("id").is_none());
        assert_eq!(payload["data"]["subject"], "Re");

        let unblock = Action::TaskUnblock {
            command_id: "c1".into(),
            base_revision: 4,
            task_id: "T1".into(),
            record_id: "R1".into(),
            reason: "  answered by phone ".into(),
        };
        let payload = unblock.payload("proj");
        assert_eq!(payload["action"], "task_unblock");
        assert_eq!(payload["reason"], "answered by phone");
        assert_eq!(payload["base_revision"], 4);

        let read = Action::TaskRead {
            task_id: "T1".into(),
            timeline: false,
        }
        .payload("proj");
        assert!(read.get("timeline").is_none());
        let parties = Action::PartyList(PartyFilters {
            include_archived: true,
            ..PartyFilters::default()
        })
        .payload("proj");
        assert_eq!(parties["include_archived"], true);
    }

    #[test]
    fn what_the_server_would_refuse_by_name_is_refused_here_first() {
        let refused = |action: Action| action.validate().expect_err("refused").kind();
        assert_eq!(
            refused(Action::RecordList(RecordFilters {
                limit: Some(101),
                ..RecordFilters::default()
            })),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            refused(Action::PartyList(PartyFilters {
                page: Some(0),
                ..PartyFilters::default()
            })),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            refused(Action::RecordUpdate {
                id: "R1".into(),
                expected_version: 0,
                data: object(json!({"state": "closed"}))
            }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            refused(Action::TaskUnblock {
                command_id: "c".into(),
                base_revision: 0,
                task_id: "T".into(),
                record_id: "R".into(),
                reason: "x".repeat(301),
            }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            refused(Action::RecordCreate {
                id: Some("../x".into()),
                data: object(json!({"subject": "s"}))
            }),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            refused(Action::Commit {
                command_id: "c".into(),
                base_revision: 1,
                command: json!({"kind": "create_task"})
            }),
            ErrorKind::InvalidInput
        );
        assert!(Action::Commit {
            command_id: "c".into(),
            base_revision: 1,
            command: json!({"kind": "create_task", "create_task": {"id": "T", "title": "t"}}),
        }
        .validate()
        .is_ok());
        assert!(Action::RecordThread {
            record_id: "R1".into()
        }
        .validate()
        .is_ok());
    }

    /// The contract's token is the code; the ids the rule named ride beside it.
    #[test]
    fn a_named_refusal_crosses_as_its_token_with_the_ids_the_rule_named() {
        let error = refusal(
            400,
            br#"{"success":false,"error":{"code":"PM_REFUSED","message":"record R5 awaits nothing","details":{"reason":"record_not_awaiting","record_id":"R5","response_status":"none","nested":{"x":1}}}}"#,
        );
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
        let served = error.service_refusal().expect("carried");
        assert_eq!(served.code(), Some("record_not_awaiting"));
        assert_eq!(served.detail_text("record_id"), Some("R5"));
        assert_eq!(served.detail_text("response_status"), Some("none"));
        assert_eq!(served.detail_text("reason"), Some("record_not_awaiting"));
        assert!(
            served.detail_text("nested").is_none(),
            "a nested value never crosses"
        );

        let bound = refusal(
            400,
            br#"{"success":false,"error":{"code":"PM_REFUSED","message":"thread is full","details":{"reason":"bound_exceeded","what":"thread","bound":500}}}"#,
        );
        let served = bound.service_refusal().expect("carried");
        assert_eq!(served.code(), Some("bound_exceeded"));
        assert_eq!(served.detail_integer("bound"), Some(500));

        let conflict = refusal(
            409,
            br#"{"success":false,"error":{"code":"PM_REVISION_CONFLICT","message":"moved","details":{"base_revision":3,"current_revision":4}}}"#,
        );
        assert_eq!(
            conflict.service_refusal().and_then(|r| r.code()),
            Some("pm_revision_conflict")
        );
        assert_eq!(
            conflict
                .service_refusal()
                .and_then(|r| r.detail_integer("current_revision")),
            Some(4)
        );

        let forbidden = refusal(
            403,
            br#"{"success":false,"error":{"code":"ASSET_CLASS_FORBIDDEN","message":"restricted","details":{"sensitivity":"restricted","capability":"assets.read.commercial"}}}"#,
        );
        assert_eq!(forbidden.kind(), ErrorKind::AuthenticationRejected);
        assert_eq!(
            forbidden
                .service_refusal()
                .and_then(|r| r.detail_text("capability")),
            Some("assets.read.commercial")
        );

        let missing = refusal(404, br#"{"success":false,"error":{"code":"PM_REFUSED","message":"no such record","details":{"reason":"record_not_found","record_id":"R9"}}}"#);
        assert_eq!(missing.kind(), ErrorKind::ResourceNotFound);
        assert_eq!(
            missing.service_refusal().and_then(|r| r.code()),
            Some("record_not_found")
        );

        let outage = refusal(503, b"upstream unavailable");
        assert_eq!(outage.kind(), ErrorKind::Transient);
        assert!(outage.service_refusal().is_none());
    }
}
