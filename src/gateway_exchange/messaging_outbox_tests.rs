use super::*;
use crate::gateway_exchange::retained::testing::{block_on, Answer, TestEdge};
use crate::gateway_exchange::Failure;

fn message(conversation: &str, client: &str, text: &str) -> Value {
    json!({
        "schema": 1, "conversation_id": conversation, "client_message_id": client,
        "local_id": format!("local-{client}"), "text": text,
        "attachments": [{"attachmentId": format!("up-{client}"), "name": "a.png", "mediaType": "image/png", "sizeBytes": 3, "sha256": "x"}],
        "queued_at_ms": 1
    })
}
fn delivered_answer(client: &str, text: &str) -> Value {
    json!({"success": true, "data": {"created": true, "message": {
        "client_message_id": client, "text": text,
        "attachments": [{"attachment_id": format!("up-{client}")}]
    }}})
}
fn sent_bodies(edge: &TestEdge) -> Vec<Value> {
    edge.requests()
        .into_iter()
        .map(|request| request.body.expect("a body"))
        .collect()
}
fn statuses(edge: &TestEdge) -> Vec<(String, String)> {
    block_on(held(edge))
        .unwrap()
        .iter()
        .map(|row| {
            (
                row["meta"]["client_message_id"]
                    .as_str()
                    .unwrap()
                    .to_owned(),
                row["status"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn a_conversation_s_messages_leave_in_the_order_sent() {
    let edge = TestEdge::new();
    for (conversation, client) in [("c1", "m1"), ("c1", "m2"), ("c2", "m3")] {
        block_on(enqueue(&edge, message(conversation, client, client))).unwrap();
    }
    block_on(enqueue(&edge, message("c1", "m1", "m1"))).unwrap();
    assert_eq!(
        block_on(held(&edge)).unwrap().len(),
        3,
        "the same client id is the same message"
    );

    for client in ["m1", "m2", "m3"] {
        edge.json(delivered_answer(client, client));
    }
    let drained = block_on(drain(&edge, "tab")).unwrap();
    let order: Vec<_> = sent_bodies(&edge)
        .iter()
        .map(|body| body["client_message_id"].clone())
        .collect();
    assert_eq!(order, [json!("m1"), json!("m2"), json!("m3")]);
    let body = &sent_bodies(&edge)[0];
    assert_eq!(
        (body["action"].as_str(), body["upload_ids"].clone()),
        (Some("send_message"), json!(["up-m1"]))
    );
    assert_eq!(drained.sent.len(), 3);
    assert_eq!(drained.sent[0]["answer"]["data"]["created"], true);
    assert!(
        block_on(held(&edge)).unwrap().is_empty(),
        "delivered messages leave"
    );
}

#[test]
fn no_link_or_paused_messaging_waits_without_spending_an_attempt() {
    let edge = TestEdge::new();
    block_on(enqueue(&edge, message("c1", "m1", "hi"))).unwrap();
    edge.online.set(false);
    let offline = block_on(drain(&edge, "tab")).unwrap();
    assert_eq!(
        (offline.stopped.as_deref(), edge.requests().len()),
        (Some(NETWORK_OFFLINE), 0)
    );

    edge.online.set(true);
    edge.answer(Answer::Json(
        503,
        json!({"success": false, "error": {"code": PAUSED, "message": "paused"}}),
    ));
    let paused = block_on(drain(&edge, "tab")).unwrap();
    assert_eq!(paused.stopped.as_deref(), Some(PAUSED));
    let row = &block_on(held(&edge)).unwrap()[0];
    assert_eq!(
        (row["status"].as_str(), row["attempts"].as_u64()),
        (Some("pending"), Some(0))
    );
}

#[test]
fn a_passing_failure_waits_for_its_time_and_the_drain_never_sets_a_timer() {
    let edge = TestEdge::new();
    block_on(enqueue(&edge, message("c1", "m1", "hi"))).unwrap();
    edge.answer(Answer::Failed(Failure::Timeout));
    let first = block_on(drain(&edge, "tab")).unwrap();
    let due = first.due_at.expect("the next try's time");
    assert!(first.failed.is_empty() && first.sent.is_empty());
    let sends = edge.requests().len();

    let early = block_on(drain(&edge, "tab")).unwrap();
    assert_eq!(
        (edge.requests().len(), early.due_at),
        (sends, Some(due)),
        "not due: nothing sent"
    );
    edge.now.set(due);
    edge.json(delivered_answer("m1", "hi"));
    assert_eq!(block_on(drain(&edge, "tab")).unwrap().sent.len(), 1);
}

#[test]
fn a_failure_no_retry_fixes_is_parked_with_its_remedy() {
    let edge = TestEdge::new();
    block_on(enqueue(&edge, message("c1", "m1", "hi"))).unwrap();
    block_on(enqueue(&edge, message("c2", "m2", "yo"))).unwrap();
    edge.json(delivered_answer("m1", "a different text"));
    edge.answer(Answer::Json(
        400,
        json!({"success": false, "error": {"code": "TEXT_INVALID", "message": "too long"}}),
    ));
    let drained = block_on(drain(&edge, "tab")).unwrap();
    let parked: Vec<_> = drained
        .failed
        .iter()
        .map(|row| {
            (
                row["client_message_id"].clone(),
                row["code"].clone(),
                row["remedy"].clone(),
            )
        })
        .collect();
    assert_eq!(
        parked,
        [
            (json!("m1"), json!(CONFLICT), json!("discard")),
            (json!("m2"), json!("TEXT_INVALID"), json!("discard")),
        ]
    );
    assert_eq!(
        statuses(&edge),
        [
            ("m1".into(), "failed".into()),
            ("m2".into(), "failed".into())
        ]
    );

    assert!(block_on(retry(&edge, "m2")).unwrap());
    edge.json(delivered_answer("m2", "yo"));
    assert_eq!(
        block_on(drain(&edge, "tab")).unwrap().sent.len(),
        1,
        "the person's retry sends it again"
    );
    assert!(block_on(discard(&edge, "m1")).unwrap());
    assert!(block_on(held(&edge)).unwrap().is_empty());
}

#[test]
fn a_message_is_never_another_account_s() {
    let edge = TestEdge::new();
    block_on(enqueue(&edge, message("c1", "m1", "hi"))).unwrap();
    edge.sign_in("someone_else");
    assert!(block_on(held(&edge)).unwrap().is_empty());
    let drained = block_on(drain(&edge, "tab")).unwrap();
    assert_eq!((drained.sent.len(), edge.requests().len()), (0, 0));
    assert!(block_on(enqueue(&edge, json!({"text": "no conversation"}))).is_err());
}
