//! Pure, header-only Survey mutation replay authority.
use serde::{Deserialize, Serialize};

pub const SCHEMA: &str = "ds.survey-entry-outbox/v1";
const MAX_INPUT: usize = 1024 * 1024;
const MAX_ROWS: usize = 10_000;
const MAX_BATCH: usize = 10;
const AUTH_CAP: u32 = 3;
const BASE_BACKOFF: u64 = 2_000;
const MAX_BACKOFF: u64 = 300_000;
const MAX_SAFE_INT: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Create,
    Update,
    Delete,
    Restore,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Pending,
    Syncing,
    Synced,
    Conflict,
    Failed,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Dispatch,
    RefreshAuth,
    ReadEntryVersion,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub id: String,
    pub project_id: String,
    pub form_id: String,
    pub doc_id: String,
    pub operation: Operation,
    pub idempotency_key: String,
    pub created_at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_uid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    #[serde(default)]
    pub lease: u64,
    pub status: Status,
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub next_attempt_at: u64,
    #[serde(default)]
    pub conflict_rebased: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failure_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subeffect_phase: Option<Phase>,
    #[serde(default)]
    pub legacy_confirmed: bool,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Fence {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_uid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
    pub lease: u64,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subeffect_phase: Option<Phase>,
}
impl From<&Header> for Fence {
    fn from(h: &Header) -> Self {
        Self {
            id: h.id.clone(),
            owner_uid: h.owner_uid.clone(),
            generation: h.generation.clone(),
            lease: h.lease,
            status: h.status.clone(),
            subeffect_phase: h.subeffect_phase.clone(),
        }
    }
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub now: u64,
    pub headers: Vec<serde_json::Value>,
    pub event: Event,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    Admit {
        current_uid: String,
        header: Header,
    },
    Schedule {
        current_uid: String,
        online: bool,
    },
    Recover {
        current_uid: String,
    },
    Result {
        current_uid: String,
        id: String,
        generation: String,
        lease: u64,
        outcome: Outcome,
    },
    AuthResult {
        current_uid: String,
        id: String,
        generation: String,
        lease: u64,
        refreshed: bool,
    },
    VersionResult {
        current_uid: String,
        id: String,
        generation: String,
        lease: u64,
        server_version: Option<u64>,
    },
    ManualRetry {
        current_uid: String,
        ids: Vec<String>,
        generation: String,
    },
    Discard {
        current_uid: String,
        ids: Vec<String>,
    },
    RecoverLegacy {
        current_uid: String,
        id: String,
        project_id: String,
        generation: String,
        operator_confirmed: bool,
    },
}
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Outcome {
    Response {
        status: u16,
        #[serde(default)]
        network: bool,
    },
    Success,
    AuthRejected,
    Conflict {
        #[serde(default)]
        server_version: Option<u64>,
    },
    Invalid,
    Transient {
        #[serde(default)]
        retry_after_ms: Option<u64>,
    },
    AmbiguousReceipt,
}
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    Persist {
        expected: Fence,
        header: Header,
        #[serde(skip_serializing_if = "Option::is_none")]
        payload_patch: Option<PayloadPatch>,
    },
    Admitted {
        header: Header,
    },
    Dispatch {
        header: Header,
    },
    Remove {
        expected: Fence,
    },
    WakeAt {
        at: u64,
    },
    RefreshAuth {
        header: Header,
    },
    ReadEntryVersion {
        header: Header,
    },
    Surface {
        id: String,
        code: String,
    },
}
#[derive(Debug, Serialize)]
pub struct PayloadPatch {
    pub expected_version: u64,
}
#[derive(Debug, Serialize)]
pub struct Reply {
    pub schema: &'static str,
    pub actions: Vec<Action>,
    pub eligible_again: bool,
}

fn valid_text(v: &str, max: usize) -> bool {
    !v.is_empty() && v.len() <= max && v.trim() == v && !v.chars().any(char::is_control)
}
fn valid_generation(v: &str) -> bool {
    v.len() == 36
        && v.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}
fn valid(h: &Header) -> bool {
    valid_text(&h.id, 4096)
        && valid_text(&h.project_id, 1500)
        && valid_text(&h.form_id, 256)
        && valid_text(&h.doc_id, 1500)
        && valid_text(&h.idempotency_key, 512)
        && h.owner_uid.as_ref().is_none_or(|v| valid_text(v, 512))
        && h.generation.as_ref().is_none_or(|v| valid_generation(v))
        && h.created_at <= MAX_SAFE_INT
        && h.next_attempt_at <= MAX_SAFE_INT
        && h.lease <= MAX_SAFE_INT
}
fn owned(h: &Header, uid: &str) -> bool {
    h.owner_uid.as_deref() == Some(uid) && h.generation.as_deref().is_some_and(valid_generation)
}
fn persist(actions: &mut Vec<Action>, old: &Header, next: Header) {
    actions.push(Action::Persist {
        expected: old.into(),
        header: next,
        payload_patch: None,
    });
}
fn persist_version(actions: &mut Vec<Action>, old: &Header, next: Header, expected_version: u64) {
    actions.push(Action::Persist {
        expected: old.into(),
        header: next,
        payload_patch: Some(PayloadPatch { expected_version }),
    });
}
fn fail(h: &mut Header, code: &str) {
    h.status = Status::Failed;
    h.next_attempt_at = 0;
    h.error = Some(code.into());
    h.failure_code = Some(code.into());
    h.subeffect_phase = None;
}
fn backoff(attempts: u32) -> u64 {
    BASE_BACKOFF
        .saturating_mul(1u64 << attempts.saturating_sub(1).min(7))
        .min(MAX_BACKOFF)
}
fn find<'a>(headers: &'a mut [Header], id: &str) -> Option<&'a mut Header> {
    headers.iter_mut().find(|h| h.id == id)
}
pub fn evaluate(input: &[u8]) -> Result<String, String> {
    if input.len() > MAX_INPUT {
        return Err("Survey entry outbox command exceeds its byte bound.".into());
    }
    let req: Request = serde_json::from_slice(input)
        .map_err(|_| "Invalid Survey entry outbox command.".to_string())?;
    if req.schema != SCHEMA || req.headers.len() > MAX_ROWS || req.now > MAX_SAFE_INT {
        return Err("Unsupported or oversized Survey entry outbox command.".into());
    }
    let mut headers = Vec::new();
    let mut actions = Vec::new();
    let mut malformed_predecessors = std::collections::BTreeSet::new();
    for raw in req.headers {
        match serde_json::from_value::<Header>(raw.clone()) {
            Ok(h) if valid(&h) => headers.push(h),
            Ok(h) => {
                malformed_predecessors
                    .insert(format!("{}\0{}\0{}", h.project_id, h.form_id, h.doc_id));
                actions.push(Action::Surface {
                    id: h.id,
                    code: "invalid_header".into(),
                });
            }
            Err(_) => {
                let id = raw
                    .get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown")
                    .to_owned();
                let address = (
                    raw.get("project_id").and_then(|v| v.as_str()),
                    raw.get("form_id").and_then(|v| v.as_str()),
                    raw.get("doc_id").and_then(|v| v.as_str()),
                );
                if let (Some(p), Some(f), Some(d)) = address {
                    malformed_predecessors.insert(format!("{p}\0{f}\0{d}"));
                }
                actions.push(Action::Surface {
                    id,
                    code: "invalid_header".into(),
                });
            }
        }
    }
    let mut eligible = false;
    match req.event {
        Event::Admit {
            current_uid,
            mut header,
        } => {
            if !valid_text(&current_uid, 512) || !valid(&header) || !owned(&header, &current_uid) {
                return Err(
                    "Survey entry outbox admission is outside its closed grammar or bounds.".into(),
                );
            }
            header.status = Status::Pending;
            header.attempts = 0;
            header.lease = 0;
            header.next_attempt_at = 0;
            header.conflict_rebased = false;
            header.error = None;
            header.failure_code = None;
            header.subeffect_phase = None;
            header.legacy_confirmed = false;
            actions.push(Action::Admitted { header });
        }
        Event::Schedule {
            current_uid,
            online,
        } => {
            if !valid_text(&current_uid, 512) {
                return Err("Survey entry outbox needs a current UID.".into());
            }
            if !online {
                actions.push(Action::Surface {
                    id: "host".into(),
                    code: "host_unavailable".into(),
                });
                return serde_json::to_string(&Reply {
                    schema: SCHEMA,
                    actions,
                    eligible_again: false,
                })
                .map_err(|_| "Survey entry outbox result encoding failed".into());
            }
            headers.sort_by(|a, b| (a.created_at, &a.id).cmp(&(b.created_at, &b.id)));
            let mut seen = std::collections::BTreeSet::new();
            for h in &mut headers {
                // A committed row can survive the crash gap before its fenced
                // remove. It is never a FIFO predecessor. Foreign-account rows
                // likewise remain durable but cannot block this account.
                if h.status == Status::Synced || (h.owner_uid.is_some() && !owned(h, &current_uid))
                {
                    continue;
                }
                let key = format!("{}\0{}\0{}", h.project_id, h.form_id, h.doc_id);
                if malformed_predecessors.contains(&key) {
                    continue;
                }
                if !seen.insert(key) {
                    continue;
                }
                if h.owner_uid.is_none() || h.generation.is_none() {
                    let old = h.clone();
                    fail(h, "legacy_scope_required");
                    persist(&mut actions, &old, h.clone());
                    actions.push(Action::Surface {
                        id: h.id.clone(),
                        code: "legacy_scope_required".into(),
                    });
                    continue;
                }
                if h.status != Status::Pending
                    || h.next_attempt_at > req.now
                    || h.subeffect_phase.is_some()
                {
                    if h.status == Status::Pending && h.next_attempt_at > req.now {
                        actions.push(Action::WakeAt {
                            at: h.next_attempt_at,
                        })
                    }
                    continue;
                }
                if actions
                    .iter()
                    .filter(|a| matches!(a, Action::Dispatch { .. }))
                    .count()
                    >= MAX_BATCH
                {
                    break;
                }
                let old = h.clone();
                h.status = Status::Syncing;
                h.attempts = h.attempts.saturating_add(1);
                h.lease = h.lease.saturating_add(1);
                h.subeffect_phase = Some(Phase::Dispatch);
                persist(&mut actions, &old, h.clone());
                actions.push(Action::Dispatch { header: h.clone() });
                eligible = true;
            }
        }
        Event::Recover { current_uid } => {
            for h in &mut headers {
                if owned(h, &current_uid)
                    && (h.status == Status::Syncing || h.subeffect_phase.is_some())
                {
                    let old = h.clone();
                    h.status = Status::Pending;
                    h.subeffect_phase = None;
                    h.next_attempt_at = req.now;
                    persist(&mut actions, &old, h.clone());
                    eligible = true
                }
            }
        }
        Event::Result {
            current_uid,
            id,
            generation,
            lease,
            outcome,
        } => {
            if let Some(h) = find(&mut headers, &id) {
                if owned(h, &current_uid)
                    && h.generation.as_deref() == Some(&generation)
                    && h.lease == lease
                    && h.status == Status::Syncing
                    && h.subeffect_phase == Some(Phase::Dispatch)
                {
                    let old = h.clone();
                    match outcome {
                        Outcome::Response { status, network } => {
                            if network || status == 408 || status == 429 || status >= 500 {
                                h.status = Status::Pending;
                                h.subeffect_phase = None;
                                h.next_attempt_at = req.now.saturating_add(backoff(h.attempts));
                                h.error = Some("transient".into());
                                persist(&mut actions, &old, h.clone());
                                actions.push(Action::WakeAt {
                                    at: h.next_attempt_at,
                                });
                            } else if status == 401 || status == 403 {
                                if h.attempts >= AUTH_CAP {
                                    fail(h, "auth_rejected");
                                    actions.push(Action::Surface {
                                        id,
                                        code: "auth_rejected".into(),
                                    });
                                } else {
                                    h.status = Status::Pending;
                                    h.subeffect_phase = Some(Phase::RefreshAuth);
                                }
                                persist(&mut actions, &old, h.clone());
                                if h.status == Status::Pending {
                                    actions.push(Action::RefreshAuth { header: h.clone() });
                                }
                            } else if status == 409 {
                                if h.operation == Operation::Create || h.conflict_rebased {
                                    h.status = Status::Conflict;
                                    h.subeffect_phase = None;
                                    h.error = Some("conflict".into());
                                    actions.push(Action::Surface {
                                        id,
                                        code: "conflict".into(),
                                    });
                                    persist(&mut actions, &old, h.clone());
                                } else {
                                    h.subeffect_phase = Some(Phase::ReadEntryVersion);
                                    persist(&mut actions, &old, h.clone());
                                    actions.push(Action::ReadEntryVersion { header: h.clone() });
                                }
                            } else {
                                fail(h, "invalid");
                                persist(&mut actions, &old, h.clone());
                                actions.push(Action::Surface {
                                    id,
                                    code: "invalid".into(),
                                });
                            }
                        }
                        Outcome::Success => {
                            h.status = Status::Synced;
                            h.subeffect_phase = None;
                            persist(&mut actions, &old, h.clone());
                            actions.push(Action::Remove {
                                expected: Fence::from(&*h),
                            });
                        }
                        Outcome::AuthRejected => {
                            if h.attempts >= AUTH_CAP {
                                fail(h, "auth_rejected");
                                persist(&mut actions, &old, h.clone());
                                actions.push(Action::Surface {
                                    id,
                                    code: "auth_rejected".into(),
                                })
                            } else {
                                h.status = Status::Pending;
                                h.subeffect_phase = Some(Phase::RefreshAuth);
                                persist(&mut actions, &old, h.clone());
                                actions.push(Action::RefreshAuth { header: h.clone() })
                            }
                        }
                        Outcome::Conflict { server_version } => {
                            if h.operation == Operation::Create || h.conflict_rebased {
                                h.status = Status::Conflict;
                                h.subeffect_phase = None;
                                h.error = Some("conflict".into());
                                persist(&mut actions, &old, h.clone());
                                actions.push(Action::Surface {
                                    id,
                                    code: "conflict".into(),
                                })
                            } else if let Some(v) = server_version.filter(|v| *v <= MAX_SAFE_INT) {
                                h.status = Status::Pending;
                                h.conflict_rebased = true;
                                h.subeffect_phase = None;
                                h.error = Some(format!("expected_version:{v}"));
                                persist_version(&mut actions, &old, h.clone(), v);
                                eligible = true
                            } else {
                                h.subeffect_phase = Some(Phase::ReadEntryVersion);
                                persist(&mut actions, &old, h.clone());
                                actions.push(Action::ReadEntryVersion { header: h.clone() })
                            }
                        }
                        Outcome::Invalid => {
                            fail(h, "invalid");
                            persist(&mut actions, &old, h.clone());
                            actions.push(Action::Surface {
                                id,
                                code: "invalid".into(),
                            })
                        }
                        Outcome::Transient { retry_after_ms } => {
                            h.status = Status::Pending;
                            h.subeffect_phase = None;
                            h.next_attempt_at = req.now.saturating_add(
                                retry_after_ms
                                    .unwrap_or_else(|| backoff(h.attempts))
                                    .min(MAX_BACKOFF),
                            );
                            h.error = Some("transient".into());
                            persist(&mut actions, &old, h.clone());
                            actions.push(Action::WakeAt {
                                at: h.next_attempt_at,
                            })
                        }
                        Outcome::AmbiguousReceipt => {
                            h.status = Status::Pending;
                            h.subeffect_phase = None;
                            h.next_attempt_at = req.now.saturating_add(backoff(h.attempts));
                            h.error = Some("ambiguous_receipt".into());
                            persist(&mut actions, &old, h.clone());
                            actions.push(Action::WakeAt {
                                at: h.next_attempt_at,
                            })
                        }
                    }
                }
            }
        }
        Event::AuthResult {
            current_uid,
            id,
            generation,
            lease,
            refreshed,
        } => {
            if let Some(h) = find(&mut headers, &id) {
                if owned(h, &current_uid)
                    && h.generation.as_deref() == Some(&generation)
                    && h.lease == lease
                    && h.subeffect_phase == Some(Phase::RefreshAuth)
                {
                    let old = h.clone();
                    if refreshed {
                        h.subeffect_phase = None;
                        h.status = Status::Pending;
                        h.next_attempt_at = req.now;
                        eligible = true
                    } else {
                        fail(h, "auth_rejected");
                        actions.push(Action::Surface {
                            id,
                            code: "auth_rejected".into(),
                        })
                    }
                    persist(&mut actions, &old, h.clone())
                }
            }
        }
        Event::VersionResult {
            current_uid,
            id,
            generation,
            lease,
            server_version,
        } => {
            if let Some(h) = find(&mut headers, &id) {
                if owned(h, &current_uid)
                    && h.generation.as_deref() == Some(&generation)
                    && h.lease == lease
                    && h.subeffect_phase == Some(Phase::ReadEntryVersion)
                {
                    let old = h.clone();
                    if let Some(v) = server_version.filter(|v| *v <= MAX_SAFE_INT) {
                        h.status = Status::Pending;
                        h.subeffect_phase = None;
                        h.conflict_rebased = true;
                        h.error = Some(format!("expected_version:{v}"));
                        eligible = true
                    } else {
                        h.status = Status::Conflict;
                        h.subeffect_phase = None;
                        h.error = Some("conflict".into());
                        actions.push(Action::Surface {
                            id,
                            code: "conflict".into(),
                        })
                    }
                    if let Some(v) = server_version.filter(|v| *v <= MAX_SAFE_INT) {
                        persist_version(&mut actions, &old, h.clone(), v)
                    } else {
                        persist(&mut actions, &old, h.clone())
                    }
                }
            }
        }
        Event::ManualRetry {
            current_uid,
            ids,
            generation,
        } => {
            if !valid_generation(&generation) {
                return Err("Survey entry outbox generation must be a UUID.".into());
            }
            for h in &mut headers {
                if ids.contains(&h.id)
                    && owned(h, &current_uid)
                    && matches!(h.status, Status::Failed | Status::Conflict)
                {
                    let old = h.clone();
                    h.generation = Some(generation.clone());
                    h.lease = h.lease.saturating_add(1);
                    h.status = Status::Pending;
                    h.attempts = 0;
                    h.next_attempt_at = 0;
                    h.conflict_rebased = false;
                    h.subeffect_phase = None;
                    h.error = None;
                    h.failure_code = None;
                    persist(&mut actions, &old, h.clone());
                    eligible = true
                }
            }
        }
        Event::Discard { current_uid, ids } => {
            for h in &headers {
                if ids.contains(&h.id)
                    && owned(h, &current_uid)
                    && matches!(h.status, Status::Failed | Status::Conflict)
                {
                    actions.push(Action::Remove { expected: h.into() })
                }
            }
        }
        Event::RecoverLegacy {
            current_uid,
            id,
            project_id,
            generation,
            operator_confirmed,
        } => {
            if !operator_confirmed
                || !valid_generation(&generation)
                || !valid_text(&current_uid, 512)
            {
                return Err(
                    "Survey entry legacy recovery needs an operator confirmation and UUID.".into(),
                );
            }
            if let Some(h) = find(&mut headers, &id) {
                if h.owner_uid.is_none() && h.status != Status::Synced && h.project_id == project_id
                {
                    let old = h.clone();
                    h.owner_uid = Some(current_uid);
                    h.generation = Some(generation);
                    h.legacy_confirmed = true;
                    h.status = Status::Pending;
                    h.lease = h.lease.saturating_add(1);
                    h.attempts = 0;
                    h.next_attempt_at = 0;
                    h.error = None;
                    h.failure_code = None;
                    h.subeffect_phase = None;
                    h.conflict_rebased = false;
                    persist(&mut actions, &old, h.clone());
                    eligible = true
                }
            }
        }
    }
    serde_json::to_string(&Reply {
        schema: SCHEMA,
        actions,
        eligible_again: eligible,
    })
    .map_err(|_| "Survey entry outbox result encoding failed".into())
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn h(id: &str, status: &str) -> serde_json::Value {
        let mut row = json!({"id":id,"project_id":"p","form_id":"form","doc_id":"d","operation":"update","idempotency_key":"survey_a","created_at":1,"owner_uid":"u","generation":"11111111-1111-1111-1111-111111111111","lease":0,"status":status,"attempts":0,"next_attempt_at":0,"conflict_rebased":false});
        if status == "syncing" {
            row["subeffect_phase"] = json!("dispatch");
        }
        row
    }
    fn run(headers: Vec<serde_json::Value>, event: serde_json::Value) -> serde_json::Value {
        serde_json::from_str(
            &evaluate(
                &serde_json::to_vec(
                    &json!({"schema":SCHEMA,"now":10,"headers":headers,"event":event}),
                )
                .unwrap(),
            )
            .unwrap(),
        )
        .unwrap()
    }
    #[test]
    fn fifo_and_legacy_are_fenced() {
        let r = run(
            vec![
                h("a", "failed"),
                h("b", "pending"),
                json!({"id":"l","project_id":"q","form_id":"f","doc_id":"x","operation":"create","idempotency_key":"k","created_at":2,"status":"pending"}),
            ],
            json!({"kind":"schedule","current_uid":"u","online":true}),
        );
        assert!(!r["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["kind"] == "dispatch"));
        assert!(r["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["code"] == "legacy_scope_required"));
    }
    #[test]
    fn stale_result_and_conflict_are_fenced() {
        let r = run(
            vec![h("a", "syncing")],
            json!({"kind":"result","current_uid":"u","id":"a","generation":"other","lease":0,"outcome":{"kind":"success"}}),
        );
        assert!(r["actions"].as_array().unwrap().is_empty());
        let r = run(
            vec![h("a", "syncing")],
            json!({"kind":"result","current_uid":"u","id":"a","generation":"11111111-1111-1111-1111-111111111111","lease":0,"outcome":{"kind":"conflict","server_version":7}}),
        );
        assert!(r["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|a| a["kind"] == "persist"));
    }
}
