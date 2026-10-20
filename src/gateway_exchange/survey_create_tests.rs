use super::*;

fn header(owner: &str) -> Queued<'_> {
    Queued {
        project: "it_rwanda",
        form: "poles",
        doc: "e_0123456789abcdef",
        idempotency_key: "survey_key_1",
        created_at: 1_760_000_000_000,
        owner_uid: owner,
    }
}
fn payload() -> Value {
    json!({"project_id": "it_rwanda", "form_id": "poles", "doc_id": "e_0123456789abcdef",
        "idempotency_key": "survey_key_1", "created_at": "2026-10-09T08:53:20Z", "origin": "web",
        "data": {"height": 9, "material": "wood"},
        "geometry": {"type": "Point", "coordinates": [30.06, -1.95]}})
}
fn author(uid: &str) -> Author {
    Author {
        firebase_project: "ds-test-project".into(),
        uid: uid.into(),
        email: "surveyor@example.test".into(),
        lane: "canary".into(),
    }
}
fn receipt() -> Vec<u8> {
    serde_json::to_vec(&json!({"commitTime": "2026-10-20T10:00:01.000001Z",
        "writeResults": [{"updateTime": "2026-10-20T10:00:01.000001Z",
        "transformResults": [{"timestampValue": "2026-10-20T10:00:01Z"}]}]}))
    .unwrap()
}

#[test]
fn a_create_is_one_create_only_write_to_this_projects_firestore() {
    let create = Create::prepare(&header("u1"), &payload(), &author("u1")).expect("a create");
    assert_eq!(
        create.url(),
        "https://firestore.googleapis.com/v1/projects/ds-test-project/databases/(default)/documents:commit"
    );
    let write = &create.body()["writes"][0];
    assert_eq!(write["currentDocument"], json!({"exists": false}));
    assert!(write["update"]["name"]
        .as_str()
        .unwrap()
        .ends_with("/eds_project/it_rwanda/forms/poles/entries/e_0123456789abcdef"));
    assert_eq!(
        write["update"]["fields"]["metadata"]["mapValue"]["fields"]["created_by_uid"],
        json!({"stringValue": "u1"})
    );
}

#[test]
fn only_a_proven_receipt_delivers_the_create() {
    let create = Create::prepare(&header("u1"), &payload(), &author("u1")).unwrap();
    assert_eq!(
        create.outcome(Sent::Answer {
            status: 200,
            body: receipt()
        }),
        json!({"kind": "success"})
    );
    assert_eq!(
        create.outcome(Sent::Answer {
            status: 200,
            body: b"{}".to_vec()
        }),
        json!({"kind": "ambiguous_receipt"})
    );
    assert_eq!(
        create.outcome(Sent::Failed),
        json!({"kind": "response", "status": 0, "network": true})
    );
    let exists =
        serde_json::to_vec(&json!({"error": {"code": 409, "status": "ALREADY_EXISTS"}})).unwrap();
    assert_eq!(
        create.outcome(Sent::Answer {
            status: 409,
            body: exists
        }),
        json!({"kind": "response", "status": 409, "details": null})
    );
}

#[test]
fn another_accounts_row_or_a_malformed_payload_is_never_sent() {
    assert!(Create::prepare(&header("u1"), &payload(), &author("u2")).is_none());
    let mut broken = payload();
    broken["data"] = json!("not a map");
    assert!(Create::prepare(&header("u1"), &broken, &author("u1")).is_none());
    let mut foreign = payload();
    foreign["project_id"] = json!("another_project");
    assert!(Create::prepare(&header("u1"), &foreign, &author("u1")).is_none());
}

#[test]
fn a_native_build_routes_creates_through_brain_and_a_pending_create_waits() {
    #[cfg(not(feature = "platform-web"))]
    assert_eq!(ROUTE, Route::Mutate);
    #[cfg(feature = "platform-web")]
    assert_eq!(ROUTE, Route::FirestoreDirect);
    assert_eq!(pending()["kind"], "transient");
    assert!(pending()["retry_after_ms"].as_u64().unwrap() >= 60_000);
}
