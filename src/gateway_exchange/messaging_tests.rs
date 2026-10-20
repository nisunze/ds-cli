use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn command(endpoint: MessagingEndpoint, payload: Value) -> Messaging {
    Messaging {
        endpoint,
        payload: payload.as_object().cloned().unwrap(),
        timeout_ms: None,
    }
}

#[test]
fn a_command_names_its_contract_and_carries_only_the_admitted_headers() {
    let plan = command(
        MessagingEndpoint::Commands,
        json!({"action": "send_message", "conversation_id": "c1",
        "client_message_id": "m1", "text": "hi", "reply_to": null}),
    )
    .plan()
    .unwrap();
    assert_eq!(plan.request.path, "/api/v1/messaging");
    assert_eq!(
        plan.request.query,
        [("wire_contract".to_owned(), "messages-v1".to_owned())]
    );
    assert!(plan.request.minimal_headers);
    assert_eq!(plan.retries, 0);
    assert!(plan.request.body.unwrap().get("reply_to").is_none());
    let upload = command(MessagingEndpoint::Attachments, json!({"action": "start"}))
        .plan()
        .unwrap();
    assert_eq!(upload.request.path, "/api/v1/messaging/attachments");
}

#[test]
fn an_action_of_another_route_or_an_unbounded_wait_is_never_sent() {
    assert_eq!(
        command(
            MessagingEndpoint::Attachments,
            json!({"action": "send_message"})
        )
        .plan()
        .unwrap_err()
        .code,
        INPUT_INVALID
    );
    assert_eq!(
        command(MessagingEndpoint::Commands, json!({"text": "no action"}))
            .plan()
            .unwrap_err()
            .code,
        INPUT_INVALID
    );
    let slow = Messaging {
        timeout_ms: Some(600_000),
        ..command(MessagingEndpoint::Commands, json!({"action": "config"}))
    };
    assert_eq!(slow.plan().unwrap_err().code, INPUT_INVALID);
}

#[test]
fn the_envelope_is_answered_as_served() {
    let Step::Fetch { continuation, .. } = start(command(
        MessagingEndpoint::Commands,
        json!({"action": "config"}),
    )) else {
        panic!()
    };
    let body = json!({"success": true, "data": {"wire_contract": "messages-v1"}});
    let observed = Observed::Response {
        status: 200,
        content_type: Some("application/json".into()),
        retry_after: None,
    };
    let Step::Done(answer) = resume(
        continuation,
        observed,
        body.to_string().as_bytes(),
        &Facts::default(),
    ) else {
        panic!()
    };
    assert_eq!(answer, body);
}

#[cfg(feature = "durable-store")]
mod held_cache {
    use super::super::{forget_cache, held_cache, keep_cache, HeldTimeline, MessagingCache};
    use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
    use serde_json::json;

    fn timeline(id: &str) -> HeldTimeline {
        HeldTimeline {
            conversation_id: id.into(),
            timeline: json!({"messages": [{"id": format!("{id}-1")}]}),
        }
    }

    #[test]
    fn the_cache_keeps_exactly_the_pages_timelines_for_its_account() {
        let edge = TestEdge::new();
        assert_eq!(
            block_on(held_cache(&edge)).unwrap(),
            MessagingCache::default()
        );
        let first = MessagingCache {
            conversations: Some(json!([{"id": "c1"}, {"id": "c2"}])),
            timelines: vec![timeline("c1"), timeline("c2")],
        };
        block_on(keep_cache(&edge, &first)).unwrap();
        let second = MessagingCache {
            conversations: Some(json!([{"id": "c2"}])),
            timelines: vec![timeline("c2")],
        };
        block_on(keep_cache(&edge, &second)).unwrap();
        let held = block_on(held_cache(&edge)).unwrap();
        assert_eq!(held.conversations, second.conversations);
        assert_eq!(held.timelines, vec![timeline("c2")]);
        edge.sign_in("someone-else");
        assert_eq!(
            block_on(held_cache(&edge)).unwrap(),
            MessagingCache::default()
        );
        edge.sign_in("owner");
        block_on(forget_cache(&edge)).unwrap();
        assert_eq!(
            block_on(held_cache(&edge)).unwrap(),
            MessagingCache::default()
        );
    }
}
