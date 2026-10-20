use super::*;
use crate::durable_store::{execute, Call, Request as Store};
use crate::gateway_exchange::retained::testing::{block_on, Answer, TestEdge};
use std::cell::RefCell;

const PATH: &str = "kigali/forms/poles/e1/photo.jpg";
const THUMB: &str = "kigali/forms/poles/e1/photo_thunder.jpeg";
const ORIGINAL: &[u8] = b"original-jpeg";
const COMPANION: &[u8] = b"thumb";
const SESSION: &str = "https://storage.googleapis.com/upload/storage/v1/b/ds-media/o?uploadType=resumable&upload_id=abc";

/// The real store on the test edge's database, bytes included.
struct Port<'a>(&'a TestEdge);

impl StorePort for Port<'_> {
    async fn call(&self, request: Store, expect_owner: Option<String>, bytes: Option<Vec<u8>>) -> Result<Reply, String> {
        let call = Call {
            request,
            expect_owner,
        };
        let answer = execute(&mut self.0.db.borrow_mut(), Some(&self.0.identity.borrow()), call, bytes)?;
        Ok(Reply {
            bytes: answer.bytes.clone(),
            answer: serde_json::to_value(&answer).map_err(|e| e.to_string())?,
        })
    }
}

/// A storage transfer that reads every byte it is handed and answers from
/// a script (last first), `Done` once the script runs out.
#[derive(Default)]
struct Wire {
    script: RefCell<Vec<Sent>>,
    sent: RefCell<Vec<(String, Vec<u8>)>>,
}

impl Transfer for Wire {
    async fn send(&self, session_uri: &str, size: u64, _content_type: &str, parts: &impl Parts) -> Sent {
        let bytes = parts.read(0, size).await.expect("pending bytes read back whole");
        self.sent.borrow_mut().push((session_uri.into(), bytes));
        self.script.borrow_mut().pop().unwrap_or(Sent::Done)
    }
}

fn store(edge: &TestEdge, operation: Value, bytes: Option<Vec<u8>>) -> Reply {
    let request = Store::MediaOutbox {
        operation: Box::new(serde_json::from_value(operation).unwrap()),
    };
    block_on(Port(edge).call(request, None, bytes)).unwrap()
}
fn admit(edge: &TestEdge) {
    let bytes = [ORIGINAL, COMPANION].concat();
    store(
        edge,
        json!({"action": "admit", "project": "kigali", "path": PATH, "surface": "survey-media", "owner_ref": "entry-1",
            "content_type": "image/jpeg", "size": ORIGINAL.len(),
            "companion": {"path": THUMB, "content_type": "image/jpeg", "size": COMPANION.len()},
            "generation": "gen-00000001", "payload": {"byte_length": bytes.len()}, "now": 1}),
        Some(bytes),
    );
}
fn stored(path: &str, answer: Value) -> Value {
    let mut body = json!({"bucket": "ds-media", "blob_path": path});
    body.as_object_mut().unwrap().extend(answer.as_object().unwrap().clone());
    json!({"data": body})
}
fn session(path: &str) -> Value {
    stored(path, json!({"already_present": false, "session_uri": SESSION}))
}
fn present(path: &str, generation: &str) -> Value {
    stored(path, json!({"already_present": true, "generation": generation}))
}
fn photo(edge: &TestEdge, path: &str) -> Reply {
    let request = Store::ByteCache {
        operation: serde_json::from_value(json!({"action": "get", "key": {"kind": "photo", "key": path}})).unwrap(),
    };
    block_on(Port(edge).call(request, None, None)).unwrap()
}

#[test]
fn an_upload_carries_its_plan_in_order_and_settles_into_the_cache() {
    let edge = TestEdge::new();
    admit(&edge);
    edge.json(session(PATH)).json(present(PATH, "11")).json(present(THUMB, "12"));
    let wire = Wire::default();
    let drained = block_on(drain(&edge, &Port(&edge), &wire)).unwrap();
    assert_eq!((drained.uploaded, drained.failed, drained.wake_at), (1, 0, None));
    let requests = edge.requests();
    let body = |at: usize| requests[at].body.clone().unwrap();
    assert_eq!((body(0)["action"].clone(), body(0)["file_name"].clone()), (json!("survey_upload_start"), json!("photo.jpg")));
    assert_eq!(body(0)["sha256"], crate::durable_store::sha256(ORIGINAL));
    assert_eq!(body(1)["action"], "survey_upload_complete");
    assert_eq!(body(2)["file_name"], "photo_thunder.jpeg");
    assert_eq!(
        (body(2)["source_file_name"].clone(), body(2)["source_generation"].clone()),
        (json!("photo.jpg"), json!("11")),
        "the thumbnail names its original's verified generation"
    );
    assert_eq!(*wire.sent.borrow(), vec![(SESSION.to_string(), ORIGINAL.to_vec())]);
    let cached = photo(&edge, PATH);
    assert_eq!((cached.answer["pending_upload"].clone(), cached.bytes), (json!(false), Some(ORIGINAL.to_vec())));
}

#[test]
fn a_refused_start_keeps_the_upload_for_its_retry() {
    let edge = TestEdge::new();
    admit(&edge);
    edge.answer(Answer::Json(503, json!({"error": "unavailable"})));
    let failed = block_on(drain(&edge, &Port(&edge), &Wire::default())).unwrap();
    assert_eq!((failed.uploaded, failed.failed), (0, 1));
    let due = failed.wake_at.expect("a retry is scheduled");
    assert!(due > edge.now.get());
    assert_eq!(photo(&edge, PATH).answer["pending_upload"], true, "the bytes wait, readable");
    let early = block_on(drain(&edge, &Port(&edge), &Wire::default())).unwrap();
    assert_eq!((early.uploaded, edge.requests().len()), (0, 1), "nothing leaves before the retry is due");
    edge.now.set(due);
    edge.json(present(PATH, "11")).json(present(THUMB, "12"));
    assert_eq!(block_on(drain(&edge, &Port(&edge), &Wire::default())).unwrap().uploaded, 1);
}

#[test]
fn a_lost_session_starts_again_within_the_drain() {
    let edge = TestEdge::new();
    admit(&edge);
    edge.json(session(PATH))
        .json(session(PATH))
        .json(present(PATH, "11"))
        .json(present(THUMB, "12"));
    let wire = Wire::default();
    wire.script.borrow_mut().push(Sent::Reopen);
    let drained = block_on(drain(&edge, &Port(&edge), &wire)).unwrap();
    assert_eq!((drained.uploaded, drained.failed), (1, 1));
    assert_eq!(wire.sent.borrow().len(), 2);
}

#[test]
fn offline_sends_nothing_and_strands_nothing() {
    let edge = TestEdge::new();
    admit(&edge);
    edge.online.set(false);
    let drained = block_on(drain(&edge, &Port(&edge), &Wire::default())).unwrap();
    assert_eq!((drained.uploaded, drained.failed), (0, 0));
    assert!(edge.requests().is_empty());
    let counts = store(&edge, json!({"action": "counts"}), None);
    assert_eq!(counts.answer["pending"], 1);
}
