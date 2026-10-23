//! Closed messaging and durable-notification client. Brain owns identity,
//! membership, ordering, idempotency and cursor state; this module only transports.
use crate::{authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind};
use serde_json::{json, Value};

#[derive(Clone, PartialEq, Eq)]
pub enum Command {
    Config,
    Installations,
    RevokeInstallation {
        installation_hash: String,
        epoch: String,
    },
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
impl std::fmt::Debug for Command {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Self::RevokeInstallation {
            installation_hash, ..
        } = self
        {
            f.debug_struct("RevokeInstallation")
                .field("installation_hash", installation_hash)
                .field("epoch", &"[REDACTED]")
                .finish()
        } else {
            f.debug_tuple("MessagingCommand")
                .field(&self.payload())
                .finish()
        }
    }
}
fn hex_lower(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl Command {
    fn generated_operation(&self) -> ds_gateway_client::Operation {
        match self {
            Self::Notifications { .. } | Self::Acknowledge { .. } => {
                ds_gateway_client::Operation::NotificationsAction
            }
            _ => ds_gateway_client::Operation::MessagingCommands,
        }
    }

    /// Installation revocation requires the exact receipt held by this client.
    /// Admission precedes credential restoration; other messaging admission stays with Brain.
    pub fn validate(&self) -> Result<(), ClientError> {
        if let Self::RevokeInstallation {
            installation_hash,
            epoch,
        } = self
        {
            if !hex_lower(installation_hash, 64) || !hex_lower(epoch, 32) {
                return Err(ClientError::new(
                    ErrorKind::InvalidInput,
                    "messaging_request_invalid",
                ));
            }
        }
        Ok(())
    }

    pub fn path(&self) -> &'static str {
        match self {
            Self::Notifications { .. } | Self::Acknowledge { .. } => {
                self.generated_operation().path_template()
            }
            _ => {
                static PATH: std::sync::OnceLock<String> = std::sync::OnceLock::new();
                PATH.get_or_init(|| {
                    format!(
                        "{}?wire_contract=messages-v1",
                        self.generated_operation().path_template()
                    )
                })
                .as_str()
            }
        }
    }
    pub fn payload(&self) -> Value {
        match self {
            Self::Config => json!({"action":"config"}),
            Self::Installations => json!({"action":"list_installations"}),
            Self::RevokeInstallation {
                installation_hash,
                epoch,
            } => {
                json!({"action":"revoke_installation","installation_hash":installation_hash,"epoch":epoch})
            }
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
    command.validate()?;
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
    let mut data = envelope
        .get("data")
        .filter(|v| v.is_object())
        .ok_or_else(unreadable)?
        .clone();
    // Shape checks catch a wrong route/envelope without interpreting domain rules.
    let valid = match command {
        Command::Config => data["wire_contract"] == "messages-v1",
        Command::Installations => validate_installations(&data),
        Command::RevokeInstallation { .. } => data
            .as_object()
            .is_some_and(|o| o.len() == 1 && data["revoked"].is_boolean()),
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
    match command {
        Command::Installations => {
            // Brain caps this inventory but supplies no source completeness receipt.
            // A platform is only a stored label, never proof of live push delivery.
            data["max_rows"] = json!(10);
            data["completion"] = json!("unknown");
            data["delivery_state"] = json!("not_proven");
        }
        Command::RevokeInstallation {
            installation_hash, ..
        } => {
            data["installation_hash"] = json!(installation_hash);
        }
        _ => {}
    }
    Ok(data)
}
fn validate_installations(data: &Value) -> bool {
    let Some(object) = data.as_object() else {
        return false;
    };
    if object.len() != 1 {
        return false;
    }
    let Some(rows) = data["installations"].as_array() else {
        return false;
    };
    if rows.len() > 10 {
        return false;
    }
    let mut hashes = std::collections::HashSet::new();
    rows.iter().all(|row| {
        let Some(fields) = row.as_object() else {
            return false;
        };
        let Some(hash) = row["installation_hash"].as_str() else {
            return false;
        };
        hex_lower(hash, 64)
            && hashes.insert(hash)
            && fields.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "installation_hash" | "platform" | "user_agent" | "refreshed_at"
                )
            })
            && ["platform", "user_agent"].iter().all(|key| {
                row.get(key)
                    .is_none_or(|v| v.as_str().is_some_and(|s| s.chars().count() <= 256))
            })
            && row
                .get("platform")
                .is_none_or(|v| v.as_str().is_some_and(|s| !s.chars().any(char::is_control)))
            && row["refreshed_at"].as_str().is_some_and(|s| {
                s.len() <= 64
                    && time::OffsetDateTime::parse(
                        s,
                        &time::format_description::well_known::Rfc3339,
                    )
                    .is_ok()
            })
    })
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
    pub(crate) fn generated_operation(&self) -> ds_gateway_client::Operation {
        self.command.generated_operation()
    }

    crate::gateway_compat::gateway_common_accessors! {
        gateway_origin: normal,
        content_type: normal,
        gateway_api_key: normal,
        client_id: normal,
        bearer_token: normal,
        canonical_email: normal,
        canonical_uid: normal,
        device_id: normal,
    }
    pub fn path(&self) -> &'static str {
        self.command.path()
    }
    pub fn method(&self) -> &'static str {
        self.generated_operation().method().as_str()
    }

    pub fn response_limit(&self) -> usize {
        MAX_RESPONSE_BYTES
    }
    pub fn timeout_seconds(&self) -> u64 {
        30
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
    #[test]
    fn installation_revoke_validates_exact_receipt_without_repair_or_debug_epoch() {
        let good = Command::RevokeInstallation {
            installation_hash: "a".repeat(64),
            epoch: "b".repeat(32),
        };
        good.validate().unwrap();
        assert_eq!(good.path(), "/api/v1/messaging?wire_contract=messages-v1");
        assert_eq!(
            good.payload(),
            json!({"action":"revoke_installation", "installation_hash":"a".repeat(64), "epoch":"b".repeat(32)})
        );
        assert!(!format!("{good:?}").contains(&"b".repeat(32)));
        for (hash, epoch) in [
            ("A".repeat(64), "b".repeat(32)),
            ("a".repeat(63), "b".repeat(32)),
            ("a".repeat(64), "B".repeat(32)),
            ("a".repeat(64), "".into()),
            (format!(" {}", "a".repeat(64)), "b".repeat(32)),
        ] {
            let bad = Command::RevokeInstallation {
                installation_hash: hash,
                epoch,
            };
            assert_eq!(
                bad.validate().unwrap_err().to_string(),
                "messaging_request_invalid"
            );
        }
    }
    #[test]
    fn installation_inventory_is_redacted_bounded_and_never_claims_delivery_or_completeness() {
        let command = Command::Installations;
        assert_eq!(command.payload(), json!({"action":"list_installations"}));
        let row = json!({"installation_hash":"a".repeat(64),"platform":"legacy-unknown","user_agent":"label","refreshed_at":"2026-10-13T00:00:00Z"});
        let data = json!({"installations":[row]});
        let accepted =
            decode(&command, response(200, json!({"success":true,"data":data}))).unwrap();
        assert_eq!(accepted["completion"], "unknown");
        assert_eq!(accepted["delivery_state"], "not_proven");
        assert_eq!(accepted["max_rows"], 10);
        for field in ["fid", "token", "epoch", "uid", "installation_id"] {
            let mut bad = data.clone();
            bad["installations"][0][field] = json!("secret");
            assert!(decode(&command, response(200, json!({"success":true,"data":bad}))).is_err());
        }
        let mut duplicated = data.clone();
        duplicated["installations"] = json!([row.clone(), row.clone()]);
        assert!(decode(
            &command,
            response(200, json!({"success":true,"data":duplicated}))
        )
        .is_err());
        let too_many = json!({"installations":(0..11).map(|i| { let mut r = row.clone(); r["installation_hash"] = json!(format!("{i:064x}")); r }).collect::<Vec<_>>()});
        assert!(decode(
            &command,
            response(200, json!({"success":true,"data":too_many}))
        )
        .is_err());
        assert!(decode(
            &command,
            response(
                200,
                json!({"success":true,"data":{"installations":[],"epoch":"secret"}})
            )
        )
        .is_err());
    }
    #[test]
    fn installation_revoke_preserves_noop_and_rejects_sensitive_reply() {
        let command = Command::RevokeInstallation {
            installation_hash: "a".repeat(64),
            epoch: "b".repeat(32),
        };
        let receipt = decode(
            &command,
            response(200, json!({"success":true,"data":{"revoked":false}})),
        )
        .unwrap();
        assert_eq!(
            receipt,
            json!({"revoked":false,"installation_hash":"a".repeat(64)})
        );
        assert!(decode(
            &command,
            response(
                200,
                json!({"success":true,"data":{"revoked":true,"epoch":"secret"}})
            )
        )
        .is_err());
        assert!(decode(
            &command,
            response(200, json!({"success":true,"data":{"revoked":"true"}}))
        )
        .is_err());
    }
}
