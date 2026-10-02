//! Account-wide, content-free activity reception. No history reads or durable ledger.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::VecDeque;
use zeroize::Zeroizing;

pub const ACCOUNT_STREAM_PATH: &str =
    "/api/v1/messaging/account-stream?wire_contract=messages-v1&ready=v2";
pub const RESPONSE_LIMIT: usize = 1024 * 1024;
pub const FRAME_LIMIT: usize = 16 * 1024;
pub const DEADLINE_SECONDS: u64 = 55;
const DEDUPE_LIMIT: usize = 256;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct AccountSignal {
    pub schema: u8,
    pub conversation_id: String,
    pub message_id: String,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}
fn opaque(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 256
        && s.trim() == s
        && !s.chars().any(char::is_control)
        && !s.contains('/')
}
impl AccountSignal {
    fn validate(&self) -> Result<(), &'static str> {
        if self.schema != 1
            || !opaque(&self.conversation_id)
            || !opaque(&self.message_id)
            || !matches!(self.kind.as_str(), "direct" | "project" | "notifications")
            || (self.kind == "project") != self.project_id.is_some()
            || self.project_id.as_ref().is_some_and(|p| !opaque(p))
        {
            return Err("account_signal_invalid");
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AccountEvent {
    Ready {
        revision: String,
        last_signal: Option<AccountSignal>,
    },
    Invalidation {
        signal: AccountSignal,
    },
    Heartbeat,
    Revoked,
    Reconnect,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    schema: u8,
    revision: String,
    #[serde(default)]
    last_signal: Option<AccountSignal>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Heartbeat {
    at: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reason {
    reason: String,
}

#[derive(Default)]
pub struct AccountSseDecoder {
    pending: Vec<u8>,
    total: usize,
    pub retry_ms: u64,
}
impl AccountSseDecoder {
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<AccountEvent>, &'static str> {
        self.total = self
            .total
            .checked_add(bytes.len())
            .ok_or("stream_response_limit")?;
        if self.total > RESPONSE_LIMIT {
            return Err("stream_response_limit");
        }
        self.pending.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(end) = self.pending.windows(2).position(|v| v == b"\n\n") {
            if end > FRAME_LIMIT {
                return Err("stream_frame_limit");
            }
            let frame = std::str::from_utf8(&self.pending[..end])
                .map_err(|_| "stream_utf8")?
                .to_owned();
            self.pending.drain(..end + 2);
            if let Some(event) = self.frame(&frame)? {
                events.push(event);
            }
        }
        if self.pending.len() > FRAME_LIMIT {
            return Err("stream_frame_limit");
        }
        Ok(events)
    }
    pub fn finish(&self) -> Result<(), &'static str> {
        if self.pending.iter().any(|b| !b.is_ascii_whitespace()) {
            Err("stream_partial_frame")
        } else {
            Ok(())
        }
    }
    fn frame(&mut self, raw: &str) -> Result<Option<AccountEvent>, &'static str> {
        let (mut event, mut id, mut data) = (String::new(), String::new(), String::new());
        for line in raw.lines() {
            if line.starts_with(':') || line.is_empty() {
                continue;
            }
            let (field, value) = line.split_once(':').unwrap_or((line, ""));
            let value = value.strip_prefix(' ').unwrap_or(value);
            match field {
                "event" => event = value.into(),
                "id" => id = value.into(),
                "data" => {
                    if !data.is_empty() {
                        data.push('\n');
                    }
                    data.push_str(value);
                }
                "retry" => {
                    self.retry_ms = value
                        .parse::<u64>()
                        .map_err(|_| "stream_retry")?
                        .clamp(1000, 30000);
                }
                _ => return Err("stream_field"),
            }
        }
        if event.is_empty() && data.is_empty() {
            return Ok(None);
        }
        let result = match event.as_str() {
            "ready" => {
                let row: Ready = serde_json::from_str(&data).map_err(|_| "stream_ready")?;
                if row.schema != 1
                    || !id.is_empty()
                    || (!row.revision.is_empty()
                        && time::OffsetDateTime::parse(
                            &row.revision,
                            &time::format_description::well_known::Rfc3339,
                        )
                        .is_err())
                {
                    return Err("stream_ready");
                }
                if let Some(signal) = &row.last_signal {
                    signal.validate()?;
                }
                AccountEvent::Ready {
                    revision: row.revision,
                    last_signal: row.last_signal,
                }
            }
            "invalidation" => {
                let signal: AccountSignal =
                    serde_json::from_str(&data).map_err(|_| "stream_signal")?;
                signal.validate()?;
                if id != signal.message_id {
                    return Err("stream_signal_id");
                }
                AccountEvent::Invalidation { signal }
            }
            "heartbeat" => {
                let row: Heartbeat = serde_json::from_str(&data).map_err(|_| "stream_heartbeat")?;
                if !id.is_empty()
                    || time::OffsetDateTime::parse(
                        &row.at,
                        &time::format_description::well_known::Rfc3339,
                    )
                    .is_err()
                {
                    return Err("stream_heartbeat");
                }
                AccountEvent::Heartbeat
            }
            "reconnect" | "revoked" => {
                let row: Reason = serde_json::from_str(&data).map_err(|_| "stream_reason")?;
                if !id.is_empty()
                    || row.reason.is_empty()
                    || row.reason.len() > 96
                    || row.reason.chars().any(char::is_control)
                {
                    return Err("stream_reason");
                }
                if event == "revoked" {
                    AccountEvent::Revoked
                } else {
                    AccountEvent::Reconnect
                }
            }
            _ => return Err("stream_event"),
        };
        Ok(Some(result))
    }
}

/// The shell can hand over only its own current session. These checks fence
/// accidental crossed UID/audience handovers; the gateway verifies the signature.
pub struct AccountCredential {
    pub uid: String,
    pub bearer: Zeroizing<String>,
    pub expires_at: u64,
}
impl AccountCredential {
    pub fn new(
        uid: String,
        bearer: String,
        audience: &str,
        now: u64,
    ) -> Result<Self, &'static str> {
        let bearer = Zeroizing::new(bearer);
        if !opaque(&uid) || bearer.len() > 16 * 1024 {
            return Err("account_credential_invalid");
        }
        let parts: Vec<_> = bearer.split('.').collect();
        if parts.len() != 3 {
            return Err("account_credential_invalid");
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(parts[1])
            .map_err(|_| "account_credential_invalid")?;
        let claims: Value =
            serde_json::from_slice(&bytes).map_err(|_| "account_credential_invalid")?;
        let expires_at = claims["exp"].as_u64().ok_or("account_credential_invalid")?;
        if claims["sub"].as_str() != Some(&uid)
            || claims["aud"].as_str() != Some(audience)
            || claims["iss"].as_str()
                != Some(format!("https://securetoken.google.com/{audience}").as_str())
            || expires_at <= now
        {
            return Err("account_credential_invalid");
        }
        Ok(Self {
            uid,
            bearer,
            expires_at,
        })
    }
}

/// One in-memory actor policy per shell account. Window visibility never owns it.
#[derive(Default)]
pub struct AccountReceiver {
    uid: Option<String>,
    generation: u64,
    seeded: bool,
    seen: VecDeque<String>,
    failures: u32,
    auth_failures: u32,
}
impl AccountReceiver {
    pub fn replace_account(
        &mut self,
        uid: Option<&str>,
        primary: bool,
    ) -> Result<u64, &'static str> {
        if !primary {
            return Err("account_receiver_primary_only");
        }
        if self.uid.as_deref() != uid {
            self.generation = self.generation.wrapping_add(1);
            self.uid = uid.map(str::to_owned);
            self.seeded = false;
            self.seen.clear();
            self.failures = 0;
            self.auth_failures = 0;
        }
        Ok(self.generation)
    }
    pub fn current(&self, uid: &str, generation: u64) -> bool {
        self.uid.as_deref() == Some(uid) && self.generation == generation
    }
    fn remember(&mut self, signal: &AccountSignal) -> bool {
        let key = format!("{}:{}", signal.conversation_id, signal.message_id);
        if self.seen.contains(&key) {
            return false;
        }
        if self.seen.len() == DEDUPE_LIMIT {
            self.seen.pop_front();
        }
        self.seen.push_back(key);
        true
    }
    pub fn receive(
        &mut self,
        uid: &str,
        generation: u64,
        event: &AccountEvent,
    ) -> Option<AccountSignal> {
        if !self.current(uid, generation) {
            return None;
        }
        self.failures = 0;
        self.auth_failures = 0;
        match event {
            AccountEvent::Ready { last_signal, .. } => {
                let first = !self.seeded;
                self.seeded = true;
                last_signal
                    .as_ref()
                    .filter(|s| self.remember(s) && !first)
                    .cloned()
            }
            AccountEvent::Invalidation { signal } => self.remember(signal).then(|| signal.clone()),
            AccountEvent::Revoked => {
                self.generation = self.generation.wrapping_add(1);
                self.uid = None;
                self.seen.clear();
                self.seeded = false;
                None
            }
            _ => None,
        }
    }
    pub fn may_refresh_rejected_credential(&mut self) -> bool {
        self.auth_failures += 1;
        self.auth_failures <= 1
    }
    pub fn retry_delay_ms(&mut self, advertised: u64) -> u64 {
        self.failures = (self.failures + 1).min(5);
        advertised
            .clamp(1000, 30000)
            .max((1 << (self.failures - 1)) * 1000)
            .min(30000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn signal(id: &str) -> AccountSignal {
        AccountSignal {
            schema: 1,
            conversation_id: "dm_a".into(),
            message_id: id.into(),
            kind: "direct".into(),
            project_id: None,
        }
    }
    #[test]
    fn own_session_handover_refuses_crossed_uid_audience_and_expiry() {
        let claims = serde_json::json!({"sub":"account-a","aud":"own-project","iss":"https://securetoken.google.com/own-project","exp":2000});
        let token = format!(
            "header.{}.signature",
            URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap())
        );
        assert!(
            AccountCredential::new("account-a".into(), token.clone(), "own-project", 1000).is_ok()
        );
        assert!(
            AccountCredential::new("account-b".into(), token.clone(), "own-project", 1000).is_err()
        );
        assert!(
            AccountCredential::new("account-a".into(), token.clone(), "other-project", 1000)
                .is_err()
        );
        assert!(AccountCredential::new("account-a".into(), token, "own-project", 2000).is_err());
    }
    #[test]
    fn receiver_uses_the_declared_account_stream_budget_and_route() {
        let operation = crate::gateway_operation("domains.messaging.account_stream").unwrap();
        assert_eq!(operation.method, "GET");
        assert_eq!(operation.transport, "sse");
        assert_eq!(ACCOUNT_STREAM_PATH.split('?').next(), Some(operation.path));
        assert_eq!(
            ACCOUNT_STREAM_PATH.split('?').nth(1),
            Some("wire_contract=messages-v1&ready=v2")
        );
        assert_eq!(DEADLINE_SECONDS, u64::from(operation.timeout_s));
        assert!((RESPONSE_LIMIT as u64) <= operation.response_cap_bytes);
    }
    #[test]
    fn hidden_or_closed_primary_receives_and_secondaries_cannot_replace() {
        let mut r = AccountReceiver::default();
        let g = r.replace_account(Some("a"), true).unwrap();
        assert!(r.replace_account(Some("b"), false).is_err());
        assert_eq!(
            r.receive(
                "a",
                g,
                &AccountEvent::Invalidation {
                    signal: signal("m1")
                }
            ),
            Some(signal("m1"))
        );
    }
    #[test]
    fn reconnect_dedupes_live_and_recovers_exact_unseen_identity() {
        let mut r = AccountReceiver::default();
        let g = r.replace_account(Some("a"), true).unwrap();
        let ready = |id| AccountEvent::Ready {
            revision: "".into(),
            last_signal: Some(signal(id)),
        };
        assert_eq!(r.receive("a", g, &ready("old")), None);
        assert!(r
            .receive(
                "a",
                g,
                &AccountEvent::Invalidation {
                    signal: signal("m1")
                }
            )
            .is_some());
        assert_eq!(r.replace_account(Some("a"), true).unwrap(), g); // renewal retains dedupe
        assert_eq!(r.receive("a", g, &ready("m1")), None);
        assert!(r.receive("a", g, &ready("m2")).is_some());
        assert_eq!(r.receive("a", g, &ready("m2")), None);
    }
    #[test]
    fn logout_and_account_change_fence_old_completions() {
        let mut r = AccountReceiver::default();
        let g = r.replace_account(Some("a"), true).unwrap();
        r.replace_account(None, true).unwrap();
        r.replace_account(Some("b"), true).unwrap();
        assert_eq!(
            r.receive(
                "a",
                g,
                &AccountEvent::Invalidation {
                    signal: signal("m1")
                }
            ),
            None
        );
        assert!(!r.current("a", g));
    }
    #[test]
    fn rejected_credentials_refresh_once_and_revocation_fences_effects() {
        let mut r = AccountReceiver::default();
        let g = r.replace_account(Some("a"), true).unwrap();
        assert!(r.may_refresh_rejected_credential());
        r.replace_account(Some("a"), true).unwrap();
        assert!(!r.may_refresh_rejected_credential());
        assert_eq!(r.receive("a", g, &AccountEvent::Revoked), None);
        assert!(!r.current("a", g));
    }
    #[test]
    fn decoder_accepts_old_ready_new_ready_and_refuses_unknown_fields() {
        let mut d = AccountSseDecoder::default();
        assert!(d
            .push(b"event: ready\nid: \ndata: {\"schema\":1,\"revision\":\"\"}\n\n")
            .is_ok());
        let json = serde_json::to_string(&signal("m1")).unwrap();
        let frame = format!("event: ready\nid: \ndata: {{\"schema\":1,\"revision\":\"\",\"last_signal\":{json}}}\n\n");
        assert!(matches!(
            d.push(frame.as_bytes()).unwrap()[0],
            AccountEvent::Ready {
                last_signal: Some(_),
                ..
            }
        ));
        assert!(d
            .push(b"event: ready\ndata: {\"schema\":1,\"revision\":\"\",\"text\":\"secret\"}\n\n")
            .is_err());
    }
}
