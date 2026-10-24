//! Messages the person sent while they could not leave, held in the outbox's
//! `messaging_send` queue for the signed-in account. Each conversation is one
//! record, so its messages arrive in the order they were sent; Brain dedups a
//! send on its client message id, so a send whose answer was lost is sent
//! again safely. The drain runs when the person sends, retries or reconnects,
//! never on a timer: a message waiting for a retry is due at a time the next
//! drain respects.

use serde::Serialize;
use serde_json::{json, Map, Value};

use super::messaging::{Messaging, MessagingEndpoint};
use super::retained::Edge;
use super::{Refusal, NETWORK_OFFLINE, SESSION_CHANGED, SIGNED_OUT};
use crate::durable_store::{Call, Request};
use crate::outbox::{Entry, Lane, Operation, Queue, Settlement, EXISTS, LEASE_LOST};

const QUEUE: Queue = Queue::MessagingSend;
/// The row id of a message: its client message id under this prefix.
const PREFIX: &str = "messaging-send:";
pub const INVALID: &str = "messaging_outbox_invalid";
/// Brain paused messaging for the account: messages wait, nothing fails.
pub const PAUSED: &str = "MESSAGING_PAUSED";
/// Brain holds a different message under the same client id.
pub const CONFLICT: &str = "IDEMPOTENCY_CONTENT_CONFLICT";
/// Sends one drain makes at most.
const MAX_SENDS: usize = 100;
/// Attempts before a failure that could pass is left to the person.
const MAX_ATTEMPTS: u64 = 8;
const LEASE_MS: u64 = 60_000;
const MAX_BACKOFF_MS: u64 = 30_000;

fn invalid(message: &str) -> Refusal {
    Refusal::new(INVALID, message)
}
fn text<'a>(payload: &'a Value, field: &str) -> Option<&'a str> {
    payload
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
}
fn row_id(client_message_id: &str) -> String {
    format!("{PREFIX}{client_message_id}")
}

/// The attachment ids a message carries, in order: `attachmentId` as the
/// page queues it, `upload_id` as Brain answers it.
fn upload_ids(attachments: &Value) -> Option<Vec<String>> {
    attachments
        .as_array()?
        .iter()
        .map(|attachment| {
            attachment
                .get("attachmentId")
                .or_else(|| attachment.get("upload_id"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

/// Brain's send_message body for a queued message.
fn send_body(payload: &Value) -> Option<Map<String, Value>> {
    let body = json!({
        "action": "send_message",
        "conversation_id": text(payload, "conversation_id")?,
        "client_message_id": text(payload, "client_message_id")?,
        "text": payload.get("text").and_then(Value::as_str)?,
        "upload_ids": upload_ids(payload.get("attachments")?)?,
    });
    body.as_object().cloned()
}

/// Whether Brain's answer is the message that was queued: the same client
/// id, text (absent when only attachments were sent) and attachments.
fn delivered(answer: &Value, body: &Map<String, Value>) -> bool {
    let message = &answer["data"]["message"];
    let ids = upload_ids(message.get("attachments").unwrap_or(&json!([])));
    message["client_message_id"] == body["client_message_id"]
        && message["text"].as_str().unwrap_or_default() == body["text"].as_str().unwrap_or_default()
        && ids.map(|ids| json!(ids)) == Some(body["upload_ids"].clone())
}

/// Queue one message (the page's `schema: 1` send payload). A message
/// already queued under its client id is the same message.
pub async fn enqueue(edge: &impl Edge, payload: Value) -> Result<(), Refusal> {
    let body = send_body(&payload).ok_or_else(|| {
        invalid("A queued message names its conversation, client id, text and attachments.")
    })?;
    let conversation = body["conversation_id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let client = body["client_message_id"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let entry = Entry {
        project: None,
        id: row_id(&client),
        record: Some(conversation.clone()),
        size: None,
        meta: Some(json!({"conversation_id": conversation, "client_message_id": client})),
        payload,
    };
    let call = Call {
        request: Request::Outbox {
            operation: Operation::Enqueue {
                queue: QUEUE,
                rows: vec![entry],
                now: edge.now_ms(),
            },
        },
        expect_owner: None,
    };
    match edge.store(call).await {
        Err(refusal) if refusal.code == EXISTS => Ok(()),
        other => other.map(drop),
    }
}

async fn call(
    edge: &impl Edge,
    owner: Option<&str>,
    operation: Operation,
) -> Result<Value, Refusal> {
    edge.store(Call {
        request: Request::Outbox { operation },
        expect_owner: owner.map(str::to_owned),
    })
    .await
}

/// The account's queued messages, oldest first, for the page to show as
/// sending or failed.
pub async fn held(edge: &impl Edge) -> Result<Vec<Value>, Refusal> {
    let answer = call(
        edge,
        None,
        Operation::List {
            queue: QUEUE,
            project: None,
        },
    )
    .await?;
    Ok(answer["rows"].as_array().cloned().unwrap_or_default())
}

/// The person's retry: the message is due again now.
pub async fn retry(edge: &impl Edge, client_message_id: &str) -> Result<bool, Refusal> {
    let operation = Operation::Retry {
        queue: QUEUE,
        id: Some(row_id(client_message_id)),
        now: edge.now_ms(),
    };
    Ok(call(edge, None, operation).await?["retried"].as_u64() > Some(0))
}

/// The person's discard, or a message the page already saw delivered.
pub async fn discard(edge: &impl Edge, client_message_id: &str) -> Result<bool, Refusal> {
    let operation = Operation::Discard {
        queue: QUEUE,
        id: row_id(client_message_id),
        now: edge.now_ms(),
    };
    Ok(call(edge, None, operation).await?["removed"].as_u64() > Some(0))
}

/// What one drain did.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Drained {
    /// Each delivered message: `client_message_id`, `conversation_id` and
    /// Brain's `answer` envelope.
    pub sent: Vec<Value>,
    /// Each message parked for the person: `client_message_id`,
    /// `conversation_id`, `code`, `message` and the `remedy` offered.
    pub failed: Vec<Value>,
    /// Why the drain stopped with messages still queued: no link, messaging
    /// paused, or a session that ended.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stopped: Option<String>,
    /// When the next waiting message comes due.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub due_at: Option<u64>,
}

/// What a refused send means for its row, and whether the rest waits too.
fn judged(refusal: &Refusal, attempts: u64, now: u64) -> (Settlement, Option<&'static str>) {
    let stop = if refusal.code == PAUSED {
        Some(PAUSED)
    } else if refusal.code == NETWORK_OFFLINE {
        Some(NETWORK_OFFLINE)
    } else if refusal.signed_out || refusal.code == SIGNED_OUT || refusal.code == SESSION_CHANGED {
        Some(SIGNED_OUT)
    } else {
        None
    };
    if stop.is_some() {
        let error = Some(refusal.code.clone());
        return (
            Settlement::Deferred {
                error,
                next_attempt_at: None,
            },
            stop,
        );
    }
    let conflict = refusal.status == Some(409)
        || refusal.code.contains("CONFLICT")
        || refusal.code.contains("IDEMPOTENCY");
    let passing = refusal.transient || refusal.retryable || refusal.outcome_unknown;
    let settlement = if conflict {
        Settlement::Failed {
            error: refusal.code.clone(),
            remedy: "discard".into(),
        }
    } else if passing && attempts + 1 < MAX_ATTEMPTS {
        let backoff = (1_000u64 << attempts.min(15)).min(MAX_BACKOFF_MS);
        Settlement::Retry {
            error: refusal.code.clone(),
            next_attempt_at: now + backoff,
        }
    } else if passing {
        Settlement::Failed {
            error: refusal.code.clone(),
            remedy: "retry".into(),
        }
    } else {
        Settlement::Failed {
            error: refusal.code.clone(),
            remedy: "discard".into(),
        }
    };
    (settlement, None)
}

/// Send every due message, oldest of each conversation first, until none is
/// due, the link drops or messaging pauses. `lease` is a fresh host nonce
/// naming this drain. Every store call after the first is fenced by the
/// account the first answered for, so a sign-out stops the drain.
pub async fn drain(edge: &impl Edge, lease: &str) -> Result<Drained, Refusal> {
    let mut drained = Drained::default();
    let mut owner: Option<String> = None;
    for _ in 0..MAX_SENDS {
        if !edge.online() {
            drained.stopped = Some(NETWORK_OFFLINE.into());
            break;
        }
        let claim = Operation::Claim {
            queue: QUEUE,
            lane: Lane::Small,
            project: None,
            lease: lease.into(),
            lease_ms: LEASE_MS,
            now: edge.now_ms(),
        };
        let answer = call(edge, owner.as_deref(), claim).await?;
        if owner.is_none() {
            owner = answer["owner"].as_str().map(str::to_owned);
        }
        let row = &answer["claimed"];
        if row.is_null() {
            drained.due_at = answer["due_at"].as_u64();
            break;
        }
        let id = row["id"].as_str().unwrap_or_default().to_owned();
        let attempts = row["attempts"].as_u64().unwrap_or(0);
        let meta = |code: &str, message: &str, remedy: &str| {
            json!({
                "client_message_id": row["meta"]["client_message_id"],
                "conversation_id": row["meta"]["conversation_id"],
                "code": code, "message": message, "remedy": remedy,
            })
        };
        let (settlement, stop) = match send_body(&row["payload"]) {
            None => {
                drained.failed.push(meta(
                    "INVALID_OUTBOX_PAYLOAD",
                    "This queued message cannot be read.",
                    "discard",
                ));
                let error = "INVALID_OUTBOX_PAYLOAD".to_owned();
                (
                    Settlement::Failed {
                        error,
                        remedy: "discard".into(),
                    },
                    None,
                )
            }
            Some(body) => {
                let send = Messaging {
                    endpoint: MessagingEndpoint::Commands,
                    payload: body.clone(),
                    timeout_ms: None,
                    accepts: Vec::new(),
                };
                match edge.fetch(send).await {
                    Ok(answer) if delivered(&answer, &body) => {
                        drained.sent.push(json!({
                            "client_message_id": body["client_message_id"],
                            "conversation_id": body["conversation_id"],
                            "answer": answer,
                        }));
                        (Settlement::Sent, None)
                    }
                    Ok(_) => {
                        drained.failed.push(meta(
                            CONFLICT,
                            "The server holds a different message under this id.",
                            "discard",
                        ));
                        (
                            Settlement::Failed {
                                error: CONFLICT.into(),
                                remedy: "discard".into(),
                            },
                            None,
                        )
                    }
                    Err(refusal) => {
                        let (settlement, stop) = judged(&refusal, attempts, edge.now_ms());
                        if let Settlement::Failed { error, remedy } = &settlement {
                            drained.failed.push(meta(error, &refusal.message, remedy));
                        }
                        (settlement, stop)
                    }
                }
            }
        };
        let settle = Operation::Settle {
            queue: QUEUE,
            id,
            lease: lease.into(),
            settlement,
            now: edge.now_ms(),
        };
        match call(edge, owner.as_deref(), settle).await {
            // Another drain took the message over; it settles it.
            Err(refusal) if refusal.code == LEASE_LOST => {}
            other => {
                other?;
            }
        }
        if let Some(stop) = stop {
            drained.stopped = Some(stop.into());
            break;
        }
    }
    Ok(drained)
}

/// The typed browser exports Svelte calls.
#[cfg(all(feature = "wasm", target_arch = "wasm32"))]
mod exports {
    use crate::durable_store::port::StoreHost;
    use crate::gateway_exchange::browser::{rejection, typed, GatewayHost};
    use crate::gateway_exchange::retained::browser::BrowserEdge;
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen(typescript_custom_section)]
    const TYPES: &'static str = r#"
/** One messaging drain: delivered messages with Brain's answer, messages parked for the person with their remedy, why it stopped and when the next waiting message is due. */
export interface MessagingOutboxDrained { sent: { client_message_id: string; conversation_id: string; answer: unknown }[]; failed: { client_message_id: string; conversation_id: string; code: string; message: string; remedy: string }[]; stopped?: string; due_at?: number }
"#;

    /// Queue one message the person sent.
    #[wasm_bindgen(js_name = messagingOutboxEnqueue)]
    pub async fn enqueue(
        host: GatewayHost,
        store: StoreHost,
        #[wasm_bindgen(unchecked_param_type = "Record<string, unknown>")] payload: JsValue,
    ) -> Result<(), JsValue> {
        let payload = tsify::serde_wasm_bindgen::from_value(payload)
            .map_err(|error| js_sys::Error::new(&error.to_string()))?;
        super::enqueue(&BrowserEdge::new(host, store), payload)
            .await
            .map_err(|refusal| rejection(&refusal))
    }

    /// The account's queued messages, oldest first.
    #[wasm_bindgen(js_name = messagingOutboxHeld, unchecked_return_type = "Record<string, unknown>[]")]
    pub async fn held(host: GatewayHost, store: StoreHost) -> Result<JsValue, JsValue> {
        let held = super::held(&BrowserEdge::new(host, store)).await;
        typed(&held.map_err(|refusal| rejection(&refusal))?)
    }

    /// The person's retry of one message.
    #[wasm_bindgen(js_name = messagingOutboxRetry)]
    pub async fn retry(
        host: GatewayHost,
        store: StoreHost,
        client_message_id: String,
    ) -> Result<bool, JsValue> {
        super::retry(&BrowserEdge::new(host, store), &client_message_id)
            .await
            .map_err(|refusal| rejection(&refusal))
    }

    /// Drop one queued message.
    #[wasm_bindgen(js_name = messagingOutboxDiscard)]
    pub async fn discard(
        host: GatewayHost,
        store: StoreHost,
        client_message_id: String,
    ) -> Result<bool, JsValue> {
        super::discard(&BrowserEdge::new(host, store), &client_message_id)
            .await
            .map_err(|refusal| rejection(&refusal))
    }

    /// Send every due message; `lease` is a fresh nonce naming this drain.
    #[wasm_bindgen(js_name = messagingOutboxDrain, unchecked_return_type = "MessagingOutboxDrained")]
    pub async fn drain(
        host: GatewayHost,
        store: StoreHost,
        lease: String,
    ) -> Result<JsValue, JsValue> {
        let drained = super::drain(&BrowserEdge::new(host, store), &lease).await;
        typed(&drained.map_err(|refusal| rejection(&refusal))?)
    }
}

#[cfg(test)]
#[path = "messaging_outbox_tests.rs"]
mod tests;
