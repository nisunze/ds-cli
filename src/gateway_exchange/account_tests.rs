use super::*;
use crate::gateway_exchange::{resume, start, Facts, Method, Observed, Step};
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

fn request<C: Exchange>(call: C) -> Request {
    match start(call) {
        Step::Fetch { request, .. } => request,
        _ => panic!("expected a fetch"),
    }
}

fn repair() -> IdentityRepair {
    IdentityRepair {
        uid: "uid-1".into(),
        email: "Ana@Example.org".into(),
    }
}

#[test]
fn identity_repair_accepts_only_this_uid_email_and_a_recorded_binding() {
    let sent = request(repair());
    assert_eq!(
        (sent.method, sent.path.as_str()),
        (Method::Get, "/api/v1/user/info")
    );
    assert_eq!(sent.timeout_ms, 15_000);
    let bound = json!({"success": true, "data": {"uid": "uid-1", "email": "ana@example.org", "firestore_updated": true}});
    assert_eq!(answered(repair(), 200, bound), Ok(true));
    let other =
        json!({"data": {"uid": "uid-2", "email": "ana@example.org", "firestore_updated": true}});
    assert_eq!(answered(repair(), 200, other), Ok(false));
    let unrecorded =
        json!({"data": {"uid": "uid-1", "email": "ana@example.org", "firestore_updated": false}});
    assert_eq!(answered(repair(), 200, unrecorded), Ok(false));
    assert!(
        matches!(start(IdentityRepair { uid: " x".into(), email: "a@b.c".into() }),
        Step::Refused(r) if r.code == INPUT_INVALID)
    );
}

#[test]
fn the_profile_is_read_and_written_as_one_document() {
    assert_eq!(request(Profile).method, Method::Get);
    let profile = answered(
        Profile,
        200,
        json!({"success": true, "data": {"first_name": "Ana", "roles": ["designer"]}}),
    )
    .unwrap();
    assert_eq!(profile["first_name"], "Ana");
    let mut fields = Map::new();
    fields.insert("theme".into(), json!("dark"));
    let write = ProfileWrite { fields };
    let sent = request(write.clone());
    assert_eq!(
        (sent.method, sent.path.as_str()),
        (Method::Put, "/api/v1/user/profile")
    );
    assert_eq!(sent.body, Some(json!({"theme": "dark"})));
    let stored = answered(write, 200, json!({"data": {"theme": "dark"}})).unwrap();
    assert_eq!(stored["theme"], "dark");
    assert!(
        matches!(start(ProfileWrite { fields: Map::new() }), Step::Refused(r) if r.code == INPUT_INVALID)
    );
    assert_eq!(
        answered(Profile, 200, json!({"data": [1]}))
            .unwrap_err()
            .code,
        INVALID
    );
}

#[test]
fn capabilities_name_their_project_and_admit_a_snapshot() {
    let sent = request(Capabilities {
        project: Some("it_rwanda".into()),
    });
    assert_eq!(sent.body, Some(json!({"project_id": "it_rwanda"})));
    assert_eq!(sent.timeout_ms, 120_000);
    assert_eq!(request(Capabilities::default()).body, Some(json!({})));
    let snapshot = answered(
        Capabilities::default(),
        200,
        json!({"data": {"user": {"email": "a@b.c"}, "app": {"roles": ["all"]}}}),
    )
    .unwrap();
    assert_eq!(snapshot["app"]["roles"][0], "all");
    assert_eq!(
        answered(Capabilities::default(), 200, json!({"data": {}}))
            .unwrap_err()
            .code,
        INVALID
    );
}

const FINGERPRINT: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn approval(confirm: bool, decision: &str) -> DeviceApproval {
    DeviceApproval {
        decision: DeviceDecision {
            request_id: "req-1".into(),
            device_fingerprint: FINGERPRINT.into(),
            decision: decision.into(),
            confirm,
        },
        uid: "uid-1".into(),
        email: "ana@example.org".into(),
    }
}

fn receipt(status: &str, preview: bool, decision: Option<Value>) -> Value {
    let mut data = json!({
        "request": {"request_id": "req-1", "status": status,
            "device": {"name": "Ana's laptop", "platform": "linux", "fingerprint": FINGERPRINT},
            "user_code": "ABCD-EFGH", "scopes": ["ds.api"],
            "binding": {"lane": "stable", "audience": "a".repeat(64),
                "profile_digest": FINGERPRINT, "catalog_digest": FINGERPRINT},
            "created_at": "2026-10-20T10:00:00Z", "expires_at": "2026-10-20T10:10:00Z", "renewable": true},
        "preview": preview,
        "principal": {"uid": "uid-1", "email": "Ana@example.org"}
    });
    if let Some(decision) = decision {
        data["decision"] = decision;
    }
    json!({"success": true, "data": data})
}

#[test]
fn a_device_preview_and_decision_are_admitted_for_this_request_and_account() {
    let sent = request(approval(false, "approve"));
    assert_eq!(sent.path, "/api/v1/auth/device/approve");
    assert_eq!(
        sent.body,
        Some(
            json!({"request_id": "req-1", "device_fingerprint": FINGERPRINT,
        "decision": "approve", "confirm": false})
        )
    );
    let preview = answered(
        approval(false, "approve"),
        200,
        receipt("pending", true, None),
    )
    .unwrap();
    assert_eq!(
        (preview.status.as_str(), preview.preview),
        ("pending", true)
    );
    assert_eq!(preview.user_code, "ABCD-EFGH");
    let decided = answered(
        approval(true, "deny"),
        200,
        receipt(
            "denied",
            false,
            Some(json!({"value": "deny", "decided_at": "2026-10-20T10:01:00Z"})),
        ),
    )
    .unwrap();
    assert_eq!(decided.decision.unwrap().value, "deny");
}

#[test]
fn a_device_receipt_for_another_account_request_or_phase_is_refused() {
    let mut other = receipt("pending", true, None);
    other["data"]["principal"]["uid"] = json!("uid-2");
    assert_eq!(
        answered(approval(false, "approve"), 200, other)
            .unwrap_err()
            .code,
        DEVICE_RECEIPT_INVALID
    );
    let mut moved = receipt("pending", true, None);
    moved["data"]["request"]["request_id"] = json!("req-2");
    assert_eq!(
        answered(approval(false, "approve"), 200, moved)
            .unwrap_err()
            .code,
        DEVICE_RECEIPT_INVALID
    );
    let phase = receipt("pending", false, None);
    assert_eq!(
        answered(approval(false, "approve"), 200, phase)
            .unwrap_err()
            .code,
        DEVICE_RECEIPT_INVALID
    );
    let undecided = receipt("approved", false, None);
    assert_eq!(
        answered(approval(true, "approve"), 200, undecided)
            .unwrap_err()
            .code,
        DEVICE_RECEIPT_INVALID
    );
    let mut scoped = receipt("pending", true, None);
    scoped["data"]["request"]["scopes"] = json!(["ds.api", "admin"]);
    assert_eq!(
        answered(approval(false, "approve"), 200, scoped)
            .unwrap_err()
            .code,
        DEVICE_RECEIPT_INVALID
    );
}

#[test]
fn a_device_decision_outside_its_contract_is_never_sent() {
    let refused =
        |call: DeviceApproval| matches!(start(call), Step::Refused(r) if r.code == INPUT_INVALID);
    assert!(refused(approval(false, "maybe")));
    let mut short = approval(false, "approve");
    short.decision.device_fingerprint = "sha256:abc".into();
    assert!(refused(short));
    let mut anonymous = approval(false, "approve");
    anonymous.uid.clear();
    assert!(refused(anonymous));
}

#[cfg(feature = "durable-store")]
#[test]
fn the_profile_is_kept_from_a_read_and_from_a_write() {
    use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
    let edge = TestEdge::new();
    assert_eq!(block_on(held_profile(&edge)), None);
    edge.json(json!({"success": true, "data": {"display_name": "Ana"}}));
    block_on(kept_profile(&edge, Profile)).unwrap();
    assert_eq!(
        block_on(held_profile(&edge)).unwrap().get("display_name"),
        Some(&json!("Ana"))
    );
    edge.json(json!({"success": true, "data": {"display_name": "Ana M"}}));
    let fields = json!({"display_name": "Ana M"}).as_object().cloned().unwrap();
    block_on(kept_profile(&edge, ProfileWrite { fields })).unwrap();
    assert_eq!(
        block_on(held_profile(&edge)).unwrap().get("display_name"),
        Some(&json!("Ana M"))
    );
}
