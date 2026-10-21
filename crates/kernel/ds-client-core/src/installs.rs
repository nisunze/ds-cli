//! The installation inventory, headlessly.
//!
//! Until now this inventory was reachable from exactly one place: a browser on
//! the Governance page. `ds capabilities --search installation` answered with
//! linked devices, which are a different thing entirely, so an operator on a
//! server — the host most likely to be asking which installations exist and
//! which licences are blocked — could not read it at all.
//!
//! Nothing about these reads needs a window. They are global: an installation
//! belongs to the product, not to a project, so nothing here carries a project
//! id, acquires a context lease, or could leak one into a request. The identity
//! is the restored user, and ds-brain gates every action behind `platform.admin`
//! or `app.admin`.
//!
//! Closed by construction, like every other governed door in this crate: the
//! [`Command`] enum is the complete set of actions this owner can send, and the
//! backend decodes with `DisallowUnknownFields`, so a key this file does not
//! write is a key the request cannot carry.

use serde_json::{json, Value};

use crate::{
    authorized::Authority, AuthorizedTransport, ClientError, ClientProfile, ErrorKind, InstallsCall,
};

/// The largest inventory answer this owner will read.
pub const MAX_RESPONSE_BYTES: usize = 1024 * 1024;
/// The most installation rows one page returns. ds-brain's own bound.
pub const MAX_PAGE: u32 = 100;
/// The longest reason a governed transition may carry. ds-brain's own bound.
pub const MAX_REASON_CHARS: usize = 1000;

/// One governed inventory action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    List {
        cursor: Option<String>,
        limit: u32,
    },
    Get {
        install_id: String,
        user_cursor: Option<String>,
        event_cursor: Option<String>,
        limit: u32,
    },
    /// Block or unblock the device and licence tokens together, with a reason
    /// that is appended to the install's immutable history.
    SetPolicy {
        install_id: String,
        expected_revision: i64,
        device_status: String,
        license_status: String,
        reason: String,
    },
    /// Record that an installation is gone, or that it is back. This is the
    /// operator's explicit verb for the case no machine can report: a wiped
    /// host, or a product removed where no uninstall hook ran. It never
    /// deletes the row, and it is never inferred from silence.
    SetRetirement {
        install_id: String,
        expected_revision: i64,
        retired: bool,
        reason: String,
    },
}

fn invalid(message: &'static str) -> ClientError {
    ClientError::new(ErrorKind::InvalidInput, message)
}

fn unreadable() -> ClientError {
    ClientError::new(
        ErrorKind::UnreadableResponse,
        "installation inventory response violates its envelope contract",
    )
}

fn cursor(value: &Option<String>) -> bool {
    match value {
        None => true,
        Some(value) => {
            !value.is_empty()
                && value.len() <= 128
                && value.trim() == value
                && !value
                    .chars()
                    .any(|c| c.is_control() || matches!(c, '/' | '\\'))
        }
    }
}

fn install_id(value: &str) -> bool {
    value.len() == 36
        && value.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        && value.to_lowercase() == value
}

fn reason(value: &str) -> bool {
    !value.trim().is_empty() && value.chars().count() <= MAX_REASON_CHARS
}

fn token(value: &str) -> bool {
    value == "active" || value == "blocked"
}

impl Command {
    pub fn action(&self) -> &'static str {
        match self {
            Self::List { .. } => "list_installs",
            Self::Get { .. } => "get_install",
            Self::SetPolicy { .. } => "set_install_policy",
            Self::SetRetirement { .. } => "set_install_retirement",
        }
    }

    /// Does this action change governed state? Reads need the same admin
    /// capability, but only a write appends to an install's history.
    pub const fn writes(&self) -> bool {
        matches!(self, Self::SetPolicy { .. } | Self::SetRetirement { .. })
    }

    pub fn validate(&self) -> Result<(), ClientError> {
        let ok = match self {
            Self::List { cursor: c, limit } => cursor(c) && (1..=MAX_PAGE).contains(limit),
            Self::Get {
                install_id: id,
                user_cursor,
                event_cursor,
                limit,
            } => {
                install_id(id)
                    && cursor(user_cursor)
                    && cursor(event_cursor)
                    && (1..=MAX_PAGE).contains(limit)
            }
            Self::SetPolicy {
                install_id: id,
                expected_revision,
                device_status,
                license_status,
                reason: why,
            } => {
                install_id(id)
                    && *expected_revision >= 0
                    && token(device_status)
                    && token(license_status)
                    && reason(why)
            }
            Self::SetRetirement {
                install_id: id,
                expected_revision,
                reason: why,
                ..
            } => install_id(id) && *expected_revision >= 0 && reason(why),
        };
        if ok {
            Ok(())
        } else {
            Err(invalid(
                "installation selection, revision, status token or reason violates its bounded contract",
            ))
        }
    }

    fn payload(&self) -> Value {
        let action = self.action();
        match self {
            Self::List { cursor, limit } => json!({
                "action": action,
                "data": { "cursor": cursor.clone().unwrap_or_default(), "limit": limit },
            }),
            Self::Get {
                install_id,
                user_cursor,
                event_cursor,
                limit,
            } => json!({
                "action": action,
                "data": {
                    "install_id": install_id,
                    "user_cursor": user_cursor.clone().unwrap_or_default(),
                    "event_cursor": event_cursor.clone().unwrap_or_default(),
                    "limit": limit,
                },
            }),
            Self::SetPolicy {
                install_id,
                expected_revision,
                device_status,
                license_status,
                reason,
            } => json!({
                "action": action,
                "data": {
                    "install_id": install_id,
                    "expected_revision": expected_revision,
                    "device_status": device_status,
                    "license_status": license_status,
                    "reason": reason.trim(),
                },
            }),
            Self::SetRetirement {
                install_id,
                expected_revision,
                retired,
                reason,
            } => json!({
                "action": action,
                "data": {
                    "install_id": install_id,
                    "expected_revision": expected_revision,
                    "retired": retired,
                    "reason": reason.trim(),
                },
            }),
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
    let response = transport.installs(InstallsCall {
        profile,
        authority: authority.call(),
        payload: command.payload(),
    })?;
    match response.status {
        200 => {}
        404 => {
            return Err(ClientError::new(
                ErrorKind::ResourceNotFound,
                "no such installation",
            ));
        }
        // 409 is the optimistic-concurrency fence: somebody else changed this
        // install's governed state, or it is already in the state asked for.
        // Both are a re-read, never a retry.
        400 | 409 => {
            return Err(invalid(
                "the installation inventory refused this request or its expected revision",
            ));
        }
        408 | 425 | 429 | 500..=599 => {
            return Err(ClientError::new(
                ErrorKind::Transient,
                "the installation inventory is temporarily unavailable",
            ));
        }
        _ => {
            return Err(ClientError::new(
                ErrorKind::AuthenticationRejected,
                "the installation inventory refused the verified user for this action",
            ));
        }
    }
    if response.body.len() > MAX_RESPONSE_BYTES {
        return Err(unreadable());
    }
    let body: Value = serde_json::from_slice(&response.body).map_err(|_| unreadable())?;
    if body["success"] != true {
        return Err(unreadable());
    }
    let data = body.get("data").cloned().ok_or_else(unreadable)?;
    if data.is_null() {
        return Err(unreadable());
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0cd65a76-a5be-440c-b82e-c3442b8c1e40";

    #[test]
    fn every_action_sends_only_the_keys_its_backend_decoder_accepts() {
        let cases: Vec<(Command, &str, Vec<&str>)> = vec![
            (
                Command::List {
                    cursor: None,
                    limit: 50,
                },
                "list_installs",
                vec!["cursor", "limit"],
            ),
            (
                Command::Get {
                    install_id: ID.into(),
                    user_cursor: None,
                    event_cursor: None,
                    limit: 50,
                },
                "get_install",
                vec!["event_cursor", "install_id", "limit", "user_cursor"],
            ),
            (
                Command::SetPolicy {
                    install_id: ID.into(),
                    expected_revision: 0,
                    device_status: "active".into(),
                    license_status: "blocked".into(),
                    reason: "licence suspended".into(),
                },
                "set_install_policy",
                vec![
                    "device_status",
                    "expected_revision",
                    "install_id",
                    "license_status",
                    "reason",
                ],
            ),
            (
                Command::SetRetirement {
                    install_id: ID.into(),
                    expected_revision: 2,
                    retired: true,
                    reason: "laptop wiped".into(),
                },
                "set_install_retirement",
                vec!["expected_revision", "install_id", "reason", "retired"],
            ),
        ];
        for (command, action, keys) in cases {
            command.validate().expect("valid command");
            let payload = command.payload();
            assert_eq!(payload["action"], action);
            let mut found: Vec<&str> = payload["data"]
                .as_object()
                .expect("data object")
                .keys()
                .map(String::as_str)
                .collect();
            found.sort_unstable();
            assert_eq!(found, keys, "{action}");
        }
    }

    #[test]
    fn a_governed_transition_without_a_reason_never_leaves_this_crate() {
        // The reason IS the evidence: this whole family exists to investigate
        // use, and an unexplained block or retirement is a hole in the record.
        for reason in ["", "   "] {
            assert!(Command::SetRetirement {
                install_id: ID.into(),
                expected_revision: 0,
                retired: true,
                reason: reason.into(),
            }
            .validate()
            .is_err());
        }
        assert!(
            Command::SetPolicy {
                install_id: ID.into(),
                expected_revision: 0,
                device_status: "retired".into(),
                license_status: "active".into(),
                reason: "why".into(),
            }
            .validate()
            .is_err(),
            "device status is a closed two-value token"
        );
        assert!(Command::List {
            cursor: None,
            limit: MAX_PAGE + 1
        }
        .validate()
        .is_err());
        assert!(Command::Get {
            install_id: "not-a-uuid".into(),
            user_cursor: None,
            event_cursor: None,
            limit: 1
        }
        .validate()
        .is_err());
    }

    #[test]
    fn only_the_two_transitions_declare_themselves_writes() {
        assert!(!Command::List {
            cursor: None,
            limit: 1
        }
        .writes());
        assert!(Command::SetRetirement {
            install_id: ID.into(),
            expected_revision: 0,
            retired: false,
            reason: "back in service".into(),
        }
        .writes());
    }
}
