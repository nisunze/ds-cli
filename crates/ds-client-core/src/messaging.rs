//! Closed messaging and durable-notification client. Brain owns identity,
//! membership, ordering, idempotency and cursor state; this module only transports.
use crate::{authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind};
use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Config,
    People {
        query: String,
        project: Option<String>,
        limit: i64,
    },
    Conversations {
        project: Option<String>,
        cursor: Option<String>,
        limit: i64,
    },
    ResolveProject {
        project: String,
    },
    ResolveDirect {
        uid: String,
    },
    ResolveNotifications,
    Read {
        conversation: String,
        cursor: Option<String>,
        since: Option<String>,
        limit: i64,
    },
    Send {
        conversation: String,
        key: String,
        text: String,
        uploads: Vec<String>,
    },
    MarkRead {
        conversation: String,
        cursor: String,
    },
    Notifications {
        cursor: Option<String>,
        limit: i64,
    },
    Acknowledge {
        ids: Vec<String>,
    },
}
impl Command {
    pub fn path(&self) -> &'static str {
        match self {
            Self::Notifications { .. } | Self::Acknowledge { .. } => "/api/v1/notifications",
            _ => "/api/v1/messaging?wire_contract=messages-v1",
        }
    }
    pub fn payload(&self) -> Value {
        match self {
            Self::Config => json!({"action":"config"}),
            Self::People {
                query,
                project,
                limit,
            } => {
                let mut v = json!({"action":if project.is_some() {"search_project_people"} else {"search_people"},"query":query,"limit":limit});
                if let Some(p) = project {
                    v["ds_project"] = json!(p);
                }
                v
            }
            Self::Conversations {
                project,
                cursor,
                limit,
            } => {
                let mut v = json!({"action":"list_conversations","limit":limit});
                if let Some(p) = project {
                    v["ds_project"] = json!(p);
                }
                if let Some(c) = cursor {
                    v["direct_cursor"] = json!(c);
                }
                v
            }
            Self::ResolveProject { project } => {
                json!({"action":"resolve_project","ds_project":project})
            }
            Self::ResolveDirect { uid } => json!({"action":"resolve_direct","target_uid":uid}),
            Self::ResolveNotifications => json!({"action":"resolve_notifications"}),
            Self::Read {
                conversation,
                cursor,
                since,
                limit,
            } => {
                let mut v =
                    json!({"action":"list_messages","conversation_id":conversation,"limit":limit});
                if let Some(c) = cursor {
                    v["cursor"] = json!(c);
                }
                if let Some(c) = since {
                    v["since_cursor"] = json!(c);
                }
                v
            }
            Self::Send {
                conversation,
                key,
                text,
                uploads,
            } => {
                json!({"action":"send_message","conversation_id":conversation,"client_message_id":key,"text":text,"upload_ids":uploads})
            }
            Self::MarkRead {
                conversation,
                cursor,
            } => {
                json!({"action":"advance_read","conversation_id":conversation,"read_cursor":cursor})
            }
            Self::Notifications { cursor, limit } => {
                let mut v = json!({"action":"list","limit":limit});
                if let Some(c) = cursor {
                    v["cursor"] = json!(c);
                }
                v
            }
            Self::Acknowledge { ids } => json!({"action":"mark_read","ids":ids}),
        }
    }
}

pub(crate) fn run<T: AuthorizedTransport>(
    profile: &ClientProfile,
    transport: &mut T,
    authority: Authority<'_>,
    command: &Command,
) -> Result<Value, ClientError> {
    let response = transport.messaging(MessagingCall {
        profile,
        authority: authority.call(),
        command,
    })?;
    decode(command, response)
}
fn decode(command: &Command, response: crate::TransportResponse) -> Result<Value, ClientError> {
    if response.body.len() > MAX_RESPONSE_BYTES {
        return Err(unreadable());
    }
    let envelope: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    if response.status != 200 || envelope["success"] != true {
        let code = envelope["error"]["code"].as_str().unwrap_or("");
        let (kind, reason) = match (response.status, code) {
            (_, "MESSAGING_PAUSED") => (ErrorKind::RouteUnavailable, "messaging_paused"),
            (_, "MESSAGING_NOT_CONFIGURED") => {
                (ErrorKind::RouteUnavailable, "messaging_not_configured")
            }
            (_, "MESSAGING_IDEMPOTENCY_MISMATCH") => {
                (ErrorKind::InvalidInput, "messaging_idempotency_mismatch")
            }
            (401 | 403, _) => (ErrorKind::AuthenticationRejected, "messaging_not_permitted"),
            (404, "NOT_FOUND") => (ErrorKind::ResourceNotFound, "messaging_not_found"),
            (404, _) => (ErrorKind::RouteUnavailable, "messaging_route_unavailable"),
            (429, _) => (ErrorKind::Transient, "messaging_rate_limited"),
            (400 | 413, _) => (ErrorKind::InvalidInput, "messaging_request_invalid"),
            (409, _) => (ErrorKind::InvalidInput, "messaging_conflict"),
            _ => (ErrorKind::Transient, "messaging_unavailable"),
        };
        return Err(ClientError::new(kind, reason));
    }
    let data = envelope
        .get("data")
        .filter(|v| v.is_object())
        .ok_or_else(unreadable)?
        .clone();
    // Shape checks catch a wrong route/envelope without interpreting domain rules.
    let valid = match command {
        Command::Config => data["wire_contract"] == "messages-v1",
        Command::People { .. } => data["people"].is_array(),
        Command::Conversations { .. } => {
            data["conversations"].is_array() && data["directs_truncated"].is_boolean()
        }
        Command::ResolveProject { .. }
        | Command::ResolveDirect { .. }
        | Command::ResolveNotifications => data["id"].is_string(),
        Command::Read { since, .. } => {
            data["messages"].is_array()
                && if since.is_some() {
                    data["truncated"].is_boolean()
                } else {
                    data["has_more"].is_boolean()
                }
        }
        Command::Send { .. } => {
            data["message"]["id"].is_string()
                && data["message"]["read_cursor"].is_string()
                && data["created"].is_boolean()
        }
        Command::MarkRead { .. } => data["read_cursor"].is_string(),
        Command::Notifications { .. } => {
            data["notifications"].is_array() && data["has_more"].is_boolean()
        }
        Command::Acknowledge { .. } => data["requested"].is_number() && data["changed"].is_number(),
    };
    if !valid {
        return Err(unreadable());
    }
    Ok(data)
}
fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "messaging_response_unreadable",
    )
}
pub const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub struct MessagingCall<'a> {
    pub(crate) profile: &'a ClientProfile,
    pub(crate) authority: crate::transport::CallAuthority<'a>,
    pub(crate) command: &'a Command,
}
impl MessagingCall<'_> {
    pub fn gateway_origin(&self) -> &str {
        self.profile.gateway_origin()
    }
    pub fn path(&self) -> &'static str {
        self.command.path()
    }
    pub fn method(&self) -> &'static str {
        "POST"
    }
    pub fn content_type(&self) -> &'static str {
        "application/json"
    }
    pub fn response_limit(&self) -> usize {
        MAX_RESPONSE_BYTES
    }
    pub fn timeout_seconds(&self) -> u64 {
        30
    }
    pub fn gateway_api_key(&self) -> &str {
        self.profile.gateway_api_key()
    }
    pub fn client_id(&self) -> &'static str {
        self.profile.client_id()
    }
    pub fn bearer_token(&self) -> &str {
        self.authority.bearer_token
    }
    pub fn canonical_email(&self) -> &str {
        self.authority.canonical_email
    }
    pub fn canonical_uid(&self) -> &str {
        self.authority.canonical_uid
    }
    pub fn device_id(&self) -> Option<&str> {
        self.authority.device_id
    }
    pub fn body(&self) -> crate::SecretRequestBody {
        crate::SecretRequestBody::new(
            serde_json::to_vec(&self.command.payload()).expect("closed messaging request"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn response(status: u16, data: Value) -> crate::TransportResponse {
        crate::TransportResponse::new(status, serde_json::to_vec(&data).unwrap())
    }
    #[test]
    fn messaging_fixed_paths_never_take_caller_routes_or_identity() {
        let send = Command::Send {
            conversation: "c".into(),
            key: "k".into(),
            text: "hello".into(),
            uploads: vec!["u".into()],
        };
        assert_eq!(send.path(), "/api/v1/messaging?wire_contract=messages-v1");
        assert_eq!(
            send.payload(),
            json!({"action":"send_message","conversation_id":"c","client_message_id":"k","text":"hello","upload_ids":["u"]})
        );
        let ack = Command::Acknowledge {
            ids: vec!["n".into()],
        };
        assert_eq!(ack.path(), "/api/v1/notifications");
        assert_eq!(ack.payload(), json!({"action":"mark_read","ids":["n"]}));
    }
    #[test]
    fn messaging_since_and_history_keep_distinct_wire_cursors() {
        let command = Command::Read {
            conversation: "c".into(),
            cursor: None,
            since: Some("opaque".into()),
            limit: 3,
        };
        assert_eq!(
            command.payload(),
            json!({"action":"list_messages","conversation_id":"c","since_cursor":"opaque","limit":3})
        );
        assert!(decode(&command,response(200,json!({"success":true,"data":{"messages":[],"truncated":true,"latest_read_cursor":"last","limit":3}}))).is_ok());
        assert!(decode(
            &command,
            response(
                200,
                json!({"success":true,"data":{"messages":[],"has_more":true}})
            )
        )
        .is_err());
    }
    #[test]
    fn messaging_original_retry_and_server_read_positions_are_preserved() {
        let command = Command::Send {
            conversation: "c".into(),
            key: "k".into(),
            text: "hello".into(),
            uploads: vec![],
        };
        let data = json!({"message":{"id":"m","read_cursor":"server-issued","text":"hello"},"created":false});
        assert_eq!(
            decode(&command, response(200, json!({"success":true,"data":data}))).unwrap(),
            data
        );
    }
    #[test]
    fn messaging_refusals_are_closed_and_do_not_echo_backend_text() {
        let command = Command::Config;
        for (status, code, expected) in [
            (403, "INSUFFICIENT_PERMISSIONS", "messaging_not_permitted"),
            (
                409,
                "MESSAGING_IDEMPOTENCY_MISMATCH",
                "messaging_idempotency_mismatch",
            ),
            (503, "MESSAGING_PAUSED", "messaging_paused"),
            (404, "NOT_FOUND", "messaging_not_found"),
            (404, "", "messaging_route_unavailable"),
            (429, "RATE_LIMITED", "messaging_rate_limited"),
        ] {
            assert_eq!(
                decode(
                    &command,
                    response(
                        status,
                        json!({"success":false,"error":{"code":code,"message":"secret bearer"}})
                    )
                )
                .unwrap_err()
                .to_string(),
                expected
            );
        }
    }
}
