use super::*;
use crate::durable_store::{execute, Call};
use crate::gateway_exchange::retained::testing::{block_on, Answer, TestEdge};
use crate::gateway_exchange::survey_media::{Parts, Sent};
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::task::{Context, Poll, Waker};

const SESSION: &str = "https://storage.googleapis.com/upload/storage/v1/b/ds-media/o?uploadType=resumable&upload_id=";
const VIDEO: &str = "kigali/forms/poles/e_big/survey.mp4";
const LARGE: usize = 1536 * 1024;

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

/// A storage transfer that reads every byte it is handed. A transfer of
/// `held` waits for the test to open its gate before it reads, and again
/// after, so a test sees it in flight.
#[derive(Default)]
struct Wire {
    held: Option<&'static str>,
    gate: Cell<u32>,
    sent: RefCell<Vec<(String, usize)>>,
}

impl Wire {
    fn holding(path: &'static str) -> Self {
        Self {
            held: Some(path),
            ..Self::default()
        }
    }
    async fn pass(&self, at: u32) {
        std::future::poll_fn(|_| if self.gate.get() >= at { Poll::Ready(()) } else { Poll::Pending }).await
    }
    fn sent_of(&self, path: &str) -> usize {
        self.sent.borrow().iter().filter(|(sent, _)| sent == path).count()
    }
}

impl Transfer for Wire {
    async fn send(&self, session_uri: &str, size: u64, _content_type: &str, parts: &impl Parts) -> Sent {
        let path = session_uri.rsplit_once("upload_id=").map_or(String::new(), |(_, id)| id.replace('~', "/"));
        let held = self.held == Some(path.as_str());
        if held {
            self.pass(1).await;
        }
        let bytes = parts.read(0, size).await.expect("held bytes read back whole");
        self.sent.borrow_mut().push((path, bytes.len()));
        if held {
            self.pass(2).await;
        }
        Sent::Done
    }
}

/// The web's create route: commits recorded, answered by a receipt.
#[derive(Default)]
struct Fire {
    commits: RefCell<Vec<Value>>,
}

impl Firestore for Fire {
    async fn author(&self) -> Result<(Author, String), String> {
        let author = Author {
            firebase_project: "ds-test-project".into(),
            uid: "owner".into(),
            email: "surveyor@example.test".into(),
            lane: "canary".into(),
        };
        Ok((author, "id-token".into()))
    }
    async fn commit(&self, _url: &str, _bearer: &str, body: &Value) -> Posted {
        self.commits.borrow_mut().push(body.clone());
        let receipt = json!({"commitTime": "2026-10-20T10:00:01.000001Z",
            "writeResults": [{"updateTime": "2026-10-20T10:00:01.000001Z",
            "transformResults": [{"timestampValue": "2026-10-20T10:00:01Z"}]}]});
        Posted::Answer {
            status: 200,
            body: serde_json::to_vec(&receipt).unwrap(),
        }
    }
}

fn store(edge: &TestEdge, request: Store, bytes: Option<Vec<u8>>) -> Value {
    block_on(Port(edge).call(request, None, bytes)).unwrap().answer
}
fn media(edge: &TestEdge, operation: Value, bytes: Option<Vec<u8>>) -> Value {
    let request = Store::MediaOutbox {
        operation: Box::new(serde_json::from_value(operation).unwrap()),
    };
    store(edge, request, bytes)
}
fn outbox(edge: &TestEdge, operation: Value) -> Value {
    let request = Store::Outbox {
        operation: serde_json::from_value(operation).unwrap(),
    };
    store(edge, request, None)
}
fn rows(edge: &TestEdge) -> Vec<Value> {
    outbox(edge, json!({"action": "list", "queue": "survey"}))["rows"].as_array().cloned().unwrap()
}
/// Hold one file's bytes for its upload.
fn admit(edge: &TestEdge, path: &str, size: usize) {
    media(
        edge,
        json!({"action": "admit", "project": "kigali", "path": path, "surface": "survey-media", "owner_ref": "entry",
            "content_type": "video/mp4", "size": size, "generation": "gen-00000001",
            "payload": {"byte_length": size}, "now": 1}),
        Some(vec![7; size]),
    );
}
fn photo(entry: usize) -> String {
    format!("kigali/forms/poles/e_{entry:02}/survey.jpg")
}
/// The gateway's answers to one upload: a session, then its verification.
/// The session names the path, so the wire knows which transfer it carries.
fn uploads(edge: &TestEdge, path: &str) {
    started(edge, path);
    stored(edge, path);
}
fn answer(path: &str, extra: Value) -> Value {
    let mut body = json!({"bucket": "ds-media", "blob_path": path});
    body.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
    json!({"data": body})
}
fn started(edge: &TestEdge, path: &str) {
    edge.json(answer(path, json!({"already_present": false, "session_uri": format!("{SESSION}{}", path.replace('/', "~"))})));
}
fn stored(edge: &TestEdge, path: &str) {
    edge.json(answer(path, json!({"already_present": true, "generation": "11"})));
}
fn starts_of(edge: &TestEdge, path: &str) -> usize {
    let file = path.rsplit('/').next().unwrap();
    let entry = path.split('/').nth(3).unwrap();
    edge.requests()
        .iter()
        .filter_map(|request| request.body.clone())
        .filter(|body| body["action"] == "survey_upload_start" && body["file_name"] == file && body["entry_id"] == entry)
        .count()
}
fn poll<F: Future>(future: std::pin::Pin<&mut F>) -> Poll<F::Output> {
    future.poll(&mut Context::from_waker(Waker::noop()))
}

#[test]
fn one_large_upload_in_flight_lets_twenty_photos_drain_and_a_second_drain_never_dispatches_it_again() {
    let edge = TestEdge::new();
    let port = Port(&edge);
    let wire = Wire::holding(VIDEO);
    let fire = Fire::default();
    admit(&edge, VIDEO, LARGE);
    started(&edge, VIDEO);
    let start = edge.now.get();

    let mut first = std::pin::pin!(drain(&edge, &port, &wire, &fire, Route::Mutate, None, "drain-a"));
    assert!(poll(first.as_mut()).is_pending(), "the large transfer is in flight");
    let video = rows(&edge);
    assert_eq!((video.len(), video[0]["lane"].clone(), video[0]["status"].clone()), (1, json!("large"), json!("syncing")));

    // Twenty photos captured while the video uploads; a touch drains again.
    for entry in 0..20 {
        admit(&edge, &photo(entry), 4096);
        uploads(&edge, &photo(entry));
    }
    edge.now.set(start + 9 * 60 * 1000);
    let second = block_on(drain(&edge, &port, &wire, &fire, Route::Mutate, None, "drain-b")).unwrap();
    assert_eq!((second.delivered, second.failed, second.stopped), (20, 0, false));
    assert_eq!(starts_of(&edge, VIDEO), 1, "the second drain never dispatches the leased transfer");
    for entry in 0..20 {
        assert_eq!(wire.sent_of(&photo(entry)), 1);
    }
    let left = rows(&edge);
    assert_eq!((left.len(), left[0]["payload"]["path"].clone()), (1, json!(VIDEO)));

    // The transfer reads its bytes nine minutes in: its lease is renewed.
    wire.gate.set(1);
    assert!(poll(first.as_mut()).is_pending());
    assert_eq!(rows(&edge)[0]["lease_until"], json!(start + 9 * 60 * 1000 + LARGE_LEASE_MS));
    stored(&edge, VIDEO);
    wire.gate.set(2);
    let Poll::Ready(first) = poll(first.as_mut()) else {
        panic!("the transfer finished");
    };
    assert_eq!(first.unwrap().delivered, 1);
    assert_eq!((wire.sent_of(VIDEO), starts_of(&edge, VIDEO)), (1, 1));
    assert!(rows(&edge).is_empty());
    assert_eq!(media(&edge, json!({"action": "counts"}), None)["pending"], 0);
}

fn create_row(id: &str, doc: &str) -> Value {
    json!({"project": "kigali", "id": id, "record": crate::survey_capture::record("poles", doc),
        "meta": {"operation": "create", "form": "poles", "doc": doc},
        "payload": {"operation": "create", "form": "poles", "doc": doc, "idempotency_key": id,
            "created_at": 1_760_000_000_000u64, "uid": "owner", "overlay": {"type": "Feature"},
            "wire": {"project_id": "kigali", "form_id": "poles", "doc_id": doc, "idempotency_key": id,
                "created_at": "2026-10-09T08:53:20Z", "origin": "web", "data": {"height": 9},
                "geometry": {"type": "Point", "coordinates": [30.06, -1.95]}}}})
}

#[test]
fn an_entrys_create_leaves_before_its_photo_on_the_webs_direct_route() {
    let edge = TestEdge::new();
    let fire = Fire::default();
    let path = "kigali/forms/poles/e_0123456789abcdef/survey.jpg";
    admit(&edge, path, 4096);
    assert_eq!(block_on(enqueue(&edge, &Port(&edge), vec![create_row("survey_key_1", "e_0123456789abcdef")])).unwrap(), 1);
    let queued = rows(&edge);
    assert_eq!(
        queued.iter().map(|row| row["payload"]["operation"].clone()).collect::<Vec<_>>(),
        vec![json!("create"), json!("media")],
        "the photo is scheduled after its entry's create"
    );
    assert_eq!(queued[0]["record"], queued[1]["record"]);
    uploads(&edge, path);
    let drained = block_on(drain(&edge, &Port(&edge), &Wire::default(), &fire, Route::FirestoreDirect, Some("kigali"), "d")).unwrap();
    assert_eq!(drained.delivered, 2);
    let commits = fire.commits.borrow();
    assert_eq!(commits.len(), 1);
    assert_eq!(commits[0]["writes"][0]["currentDocument"], json!({"exists": false}));
    assert!(rows(&edge).is_empty());
}

#[test]
fn a_native_create_waits_for_its_route_and_holds_its_entrys_later_steps() {
    let edge = TestEdge::new();
    let path = "kigali/forms/poles/e_0123456789abcdef/survey.jpg";
    admit(&edge, path, 4096);
    block_on(enqueue(&edge, &Port(&edge), vec![create_row("survey_key_1", "e_0123456789abcdef")])).unwrap();
    let drained = block_on(drain(&edge, &Port(&edge), &Wire::default(), &Fire::default(), Route::Mutate, None, "d")).unwrap();
    assert_eq!((drained.delivered, drained.failed), (0, 0));
    assert!(edge.requests().is_empty(), "nothing is sent while Brain takes no creates");
    let held = rows(&edge);
    assert_eq!((held[0]["status"].clone(), held[0]["error"].clone()), (json!("pending"), json!(ROUTE_PENDING)));
    assert_eq!(held[0]["attempts"], 0, "waiting spends no attempt");
    assert_eq!(drained.due_at, Some(edge.now.get() + ROUTE_PENDING_MS));
    assert_eq!(held[1]["status"], "pending", "the photo waits behind its create");
}

fn mutation_row(id: &str, operation: &str) -> Value {
    json!({"project": "kigali", "id": id, "record": crate::survey_capture::record("poles", "e_1"),
        "payload": {"operation": operation, "form": "poles", "doc": "e_1", "idempotency_key": id,
            "created_at": 1, "uid": "owner", "overlay": null,
            "wire": {"project_id": "kigali", "form_id": "poles", "doc_id": "e_1", "operation": operation,
                "idempotency_key": id, "expected_version": 3, "updated_data": {"height": 10}}}})
}

#[test]
fn a_stale_correction_goes_again_once_against_the_version_brain_named() {
    let edge = TestEdge::new();
    block_on(enqueue(&edge, &Port(&edge), vec![mutation_row("m1", "update")])).unwrap();
    let stale = json!({"error": {"code": "VERSION_CONFLICT", "message": "stale", "details": {"server_version": 7}}});
    edge.answer(Answer::Json(409, stale.clone()))
        .json(json!({"data": {"ok": true}}));
    let drained = block_on(drain(&edge, &Port(&edge), &Wire::default(), &Fire::default(), Route::Mutate, None, "d")).unwrap();
    assert_eq!(drained.delivered, 1);
    let sent = edge.requests();
    assert_eq!(sent.len(), 2);
    assert_eq!(sent[1].body.as_ref().unwrap()["expected_version"], 7);

    block_on(enqueue(&edge, &Port(&edge), vec![mutation_row("m2", "delete")])).unwrap();
    edge.answer(Answer::Json(409, stale.clone())).answer(Answer::Json(409, stale));
    let parked = block_on(drain(&edge, &Port(&edge), &Wire::default(), &Fire::default(), Route::Mutate, None, "d")).unwrap();
    assert_eq!(parked.failed, 1);
    let row = &rows(&edge)[0];
    assert_eq!((row["status"].clone(), row["remedy"].clone()), (json!("failed"), json!("review")));
}

#[test]
fn outcomes_settle_as_the_queue_reads_them() {
    let now = 1_000;
    let settle = |outcome: Value, attempts| verdict(&outcome, attempts, now);
    assert_eq!(settle(json!({"kind": "success"}), 0), Verdict::sent());
    assert_eq!(
        settle(json!({"kind": "response", "status": 503}), 0).settlement,
        Settlement::Retry {
            error: "transient".into(),
            next_attempt_at: now + BASE_BACKOFF_MS
        }
    );
    assert_eq!(
        settle(json!({"kind": "response", "status": 503}), 30).settlement,
        Settlement::Retry {
            error: "transient".into(),
            next_attempt_at: now + BASE_BACKOFF_MS * 128
        }
    );
    let offline = settle(json!({"kind": "response", "status": 0, "network": true}), 0);
    assert!(offline.stop);
    assert!(matches!(offline.settlement, Settlement::Deferred { .. }));
    assert!(matches!(settle(json!({"kind": "response", "status": 401}), 0).settlement, Settlement::Retry { .. }));
    assert_eq!(settle(json!({"kind": "response", "status": 401}), 2), Verdict::failed("auth_rejected", "sign_in"));
    assert_eq!(settle(json!({"kind": "response", "status": 403}), 0), Verdict::failed("forbidden", "access"));
    assert_eq!(settle(json!({"kind": "response", "status": 422}), 0), Verdict::failed("invalid", "discard"));
    assert_eq!(settle(json!({"kind": "invalid"}), 0), Verdict::failed("invalid", "discard"));
    assert!(matches!(settle(json!({"kind": "ambiguous_receipt"}), 0).settlement, Settlement::Retry { .. }));
}

#[test]
fn offline_claims_nothing_and_strands_nothing() {
    let edge = TestEdge::new();
    block_on(enqueue(&edge, &Port(&edge), vec![mutation_row("m1", "update")])).unwrap();
    edge.online.set(false);
    let drained = block_on(drain(&edge, &Port(&edge), &Wire::default(), &Fire::default(), Route::Mutate, None, "d")).unwrap();
    assert!(drained.stopped);
    assert_eq!(rows(&edge)[0]["status"], "pending");
}

#[test]
fn discarding_a_parked_create_audits_and_drops_its_entrys_every_step() {
    let edge = TestEdge::new();
    let path = "kigali/forms/poles/e_fedcba9876543210/survey.jpg";
    admit(&edge, path, 4096);
    let mut foreign = create_row("survey_key_2", "e_fedcba9876543210");
    foreign["payload"]["uid"] = json!("someone-else");
    block_on(enqueue(&edge, &Port(&edge), vec![foreign])).unwrap();
    let parked = block_on(drain(&edge, &Port(&edge), &Wire::default(), &Fire::default(), Route::FirestoreDirect, None, "d")).unwrap();
    assert_eq!(parked.failed, 1, "another account's create is never written");
    assert!(edge.requests().is_empty());
    let held = rows(&edge);
    assert_eq!((held[0]["status"].clone(), held[0]["remedy"].clone()), (json!("failed"), json!("discard")));
    assert_eq!(held[1]["status"], "pending", "its photo waits behind it");

    edge.online.set(false);
    assert!(block_on(discard(&edge, &Port(&edge), "kigali", None)).is_err(), "nothing leaves unaudited");
    assert_eq!(rows(&edge).len(), 2);
    edge.online.set(true);
    edge.json(json!({"data": {"recorded": 2}}));
    assert_eq!(block_on(discard(&edge, &Port(&edge), "kigali", None)).unwrap(), 2);
    let audit = edge.requests().last().unwrap().body.clone().unwrap();
    assert_eq!(audit["action"], "submit_client_discarded");
    assert_eq!((audit["entries"][0]["doc_id"].clone(), audit["entries"][0]["form_id"].clone()), (json!("e_fedcba9876543210"), json!("poles")));
    assert!(rows(&edge).is_empty());
    assert_eq!(media(&edge, json!({"action": "counts"}), None)["pending"], 0, "the photo's bytes left with it");
    block_on(drain(&edge, &Port(&edge), &Wire::default(), &Fire::default(), Route::FirestoreDirect, None, "d")).unwrap();
    assert!(rows(&edge).is_empty(), "a dropped file is never scheduled again");
}
