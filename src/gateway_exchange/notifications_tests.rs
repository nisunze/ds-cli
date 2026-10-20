use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn answered<C: Exchange>(call: C, status: u16, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else {
        panic!("expected a fetch")
    };
    let observed = Observed::Response {
        status,
        content_type: Some("application/json".into()),
        retry_after: None,
    };
    match resume(
        continuation,
        observed,
        body.to_string().as_bytes(),
        &Facts::default(),
    ) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

fn sent<C: Exchange>(call: C) -> Option<Value> {
    match start(call) {
        Step::Fetch { request, .. } => request.body,
        Step::Refused(refusal) => panic!("refused: {refusal:?}"),
        _ => panic!("expected a fetch"),
    }
}

#[test]
fn a_page_keeps_well_formed_rows_and_drops_unsafe_routes() {
    let call = Page {
        limit: Some(50),
        cursor: Some("c1".into()),
    };
    assert_eq!(
        sent(call.clone()),
        Some(json!({"action": "list", "limit": 50, "cursor": "c1"}))
    );
    let page = answered(call, 200, json!({"success": true, "data": {"limit": 50, "has_more": true,
        "next_cursor": "c2", "notifications": [
            {"id": "n1", "title": "Report ready", "severity": "success", "created_at": 5, "read": false,
             "route": "/projects/p/work", "project_id": "p", "schema": 1},
            {"id": "n2", "title": "Phish", "severity": "loud", "created_at": 4, "read": true,
             "route": "//evil.example/x"},
            {"id": "", "title": "no id"},
            {"id": "n3", "title": " "}]}}))
    .unwrap();
    assert_eq!(page.notifications.len(), 2);
    assert_eq!(
        page.notifications[0].route.as_deref(),
        Some("/projects/p/work")
    );
    assert_eq!(page.notifications[1].severity, NotificationSeverity::Info);
    assert_eq!(page.notifications[1].route, None);
    assert_eq!(
        (page.next_cursor.as_deref(), page.has_more, page.limit),
        (Some("c2"), true, 50)
    );
    assert!(
        matches!(start(Page { limit: Some(101), cursor: None }), Step::Refused(r) if r.code == INPUT_INVALID)
    );
}

#[test]
fn routes_are_rooted_same_origin_paths_only() {
    for safe in ["/", "/projects/a", "/sre?tab=1"] {
        assert!(safe_route(safe), "{safe}");
    }
    for unsafe_route in ["", "projects", "//evil", "/a\\b", "https://x", "/a\u{0}b"] {
        assert!(!safe_route(unsafe_route), "{unsafe_route:?}");
    }
    assert!(!safe_route(&format!("/{}", "a".repeat(512))));
}

#[test]
fn a_mark_names_its_action_and_at_most_a_hundred_ids() {
    let call = Marked {
        mark: NotificationMark::MarkRead,
        ids: vec!["a".into(), "b".into()],
    };
    assert_eq!(
        sent(call.clone()),
        Some(json!({"action": "mark_read", "ids": ["a", "b"]}))
    );
    let outcome = answered(
        call,
        200,
        json!({"data": {"requested": 2, "changed": 1, "unchanged": 1, "missing": 0}}),
    )
    .unwrap();
    assert_eq!(outcome.changed, 1);
    let many: Vec<String> = (0..250).map(|i| format!("n{i}")).collect();
    assert_eq!(
        chunks(&many).iter().map(Vec::len).collect::<Vec<_>>(),
        [100, 100, 50]
    );
    assert!(
        matches!(start(Marked { mark: NotificationMark::Dismiss, ids: many }), Step::Refused(r) if r.code == INPUT_INVALID)
    );
    assert!(
        matches!(start(Marked { mark: NotificationMark::Dismiss, ids: vec![] }), Step::Refused(r) if r.code == INPUT_INVALID)
    );
}

#[test]
fn push_config_registration_and_revoke_are_read_without_secrets() {
    let config = answered(
        Config,
        200,
        json!({"data": {"enabled": true, "vapid_public_key": "BPk",
        "payload_schema_version": 1, "max_subscriptions": 10, "notification_cap": 200,
        "max_page_size": 100, "default_page_size": 50, "max_ids_per_mutation": 100,
        "subscriptions": [{"id": "s1", "host": "fcm.googleapis.com"}, {"host": "no-id"}]}}),
    )
    .unwrap();
    assert!(config.enabled);
    assert_eq!(config.subscriptions.len(), 1);
    let refusal = answered(
        Config,
        503,
        json!({"error": {"code": "PUSH_NOT_CONFIGURED", "message": "Push is off"}}),
    )
    .unwrap_err();
    assert_eq!(refusal.code, PUSH_NOT_CONFIGURED);

    let subscription = BrowserPushSubscription {
        endpoint: "https://fcm.googleapis.com/fcm/send/abc".into(),
        keys: PushSubscriptionKeys {
            p256dh: "p".into(),
            auth: "a".into(),
        },
    };
    let register = Register { subscription };
    assert_eq!(
        sent(register.clone()).unwrap()["subscription"]["keys"]["auth"],
        "a"
    );
    let registered = answered(
        register,
        200,
        json!({"data": {"subscription": {"id": "s1"}, "vapid_public_key": "BPk"}}),
    )
    .unwrap();
    assert_eq!(registered.subscription.id, "s1");

    let revoke = Revoke {
        subscription_id: None,
        endpoint: Some("https://fcm.googleapis.com/fcm/send/abc".into()),
    };
    assert_eq!(
        sent(revoke.clone()),
        Some(
            json!({"action": "revoke_push", "endpoint": "https://fcm.googleapis.com/fcm/send/abc"})
        )
    );
    assert_eq!(
        answered(revoke, 200, json!({"data": {"revoked": true}})),
        Ok(true)
    );
    let both = Revoke {
        subscription_id: Some("s1".into()),
        endpoint: Some("https://x".into()),
    };
    assert!(matches!(start(both), Step::Refused(r) if r.code == INPUT_INVALID));
    let plain = Register {
        subscription: BrowserPushSubscription {
            endpoint: "http://insecure".into(),
            keys: PushSubscriptionKeys {
                p256dh: "p".into(),
                auth: "a".into(),
            },
        },
    };
    assert!(matches!(start(plain), Step::Refused(r) if r.code == INPUT_INVALID));
}
