use super::*;
use crate::durable_store::{execute as store, Call, Identity, Principal, Request};
use serde_json::json;
use std::cell::RefCell;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

fn block_on<T>(future: impl Future<Output = T>) -> T {
    fn raw() -> RawWaker {
        fn clone(_: *const ()) -> RawWaker {
            raw()
        }
        fn noop(_: *const ()) {}
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, noop, noop, noop);
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    // SAFETY: every vtable entry is a no-op over a null pointer.
    let waker = unsafe { Waker::from_raw(raw()) };
    let mut future = pin!(future);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut Context::from_waker(&waker)) {
            return value;
        }
    }
}

fn identity() -> Identity {
    Identity {
        principal: Principal::Account("uid".into()),
        lane: "canary".into(),
        audience: "a".repeat(64),
    }
}
fn status_call(db: &mut Connection) -> Result<Value, String> {
    let request: Request =
        serde_json::from_value(json!({"store": STORE, "operation": {"action": "status"}}))
            .map_err(|e| e.to_string())?;
    let answer = store(
        db,
        Some(&identity()),
        Call {
            request,
            expect_owner: None,
        },
        None,
    )?;
    serde_json::to_value(answer).map_err(|e| e.to_string())
}

#[test]
fn core_store_reports_persisted_usage_and_free_disk() {
    let directory = std::env::temp_dir().join(format!("ds-store-status-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let file = directory.join("store.sqlite");
    let mut db = Connection::open(&file).unwrap();
    db.execute_batch("CREATE TABLE t(x BLOB); INSERT INTO t VALUES (zeroblob(100000));")
        .unwrap();
    let answer = status_call(&mut db).unwrap();
    assert_eq!(answer["schema"], SCHEMA);
    assert_eq!(answer["persisted"], true);
    assert_eq!(answer["backend"], "core_store");
    assert!(answer["usage_bytes"].as_u64().unwrap() >= 100_000);
    #[cfg(unix)]
    assert!(answer["quota_bytes"].as_u64().unwrap() > 0);
    drop(db);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn desktop_native_store_is_named_by_its_file() {
    let directory = std::env::temp_dir().join(format!("ds-store-status-d-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let mut db = Connection::open(directory.join(DESKTOP_FILE)).unwrap();
    let answer = status_call(&mut db).unwrap();
    assert_eq!(answer["backend"], "native_sqlite");
    assert_eq!(answer["persisted"], true);
    drop(db);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn memory_store_has_no_quota_and_the_record_is_closed() {
    let mut db = Connection::open_in_memory().unwrap();
    let answer = status_call(&mut db).unwrap();
    assert!(answer["quota_bytes"].is_null());
    let record: StoreStatus = serde_json::from_value(
        json!({"schema": answer["schema"], "persisted": answer["persisted"], "usage_bytes": answer["usage_bytes"], "quota_bytes": null, "backend": answer["backend"]}),
    )
    .unwrap();
    assert_eq!(record.persisted, Persisted::Known(true));
    let extra = serde_json::from_value::<StoreStatus>(
        json!({"schema": SCHEMA, "persisted": true, "usage_bytes": 0, "quota_bytes": null, "backend": "opfs", "surprise": 1}),
    );
    assert!(extra.is_err());
}

#[derive(Default)]
struct Mock {
    persist: Option<bool>,
    persisted: Option<bool>,
    estimate: Option<(u64, Option<u64>)>,
    persist_calls: RefCell<u32>,
}
impl StorageApi for Mock {
    async fn persist(&self) -> Option<bool> {
        *self.persist_calls.borrow_mut() += 1;
        self.persist
    }
    async fn persisted(&self) -> Option<bool> {
        self.persisted
    }
    async fn estimate(&self) -> Option<(u64, Option<u64>)> {
        self.estimate
    }
}

#[test]
fn web_status_comes_from_the_browser_estimate() {
    let api = Mock {
        persisted: Some(true),
        estimate: Some((1234, Some(9_000_000))),
        ..Mock::default()
    };
    let status = block_on(web_status(&api, None));
    assert_eq!(
        serde_json::to_value(&status).unwrap(),
        json!({"schema": SCHEMA, "persisted": true, "usage_bytes": 1234, "quota_bytes": 9_000_000, "backend": "opfs"})
    );
}

#[test]
fn web_persisted_is_unknown_when_the_browser_cannot_say() {
    let status = block_on(web_status(&Mock::default(), None));
    assert_eq!(
        serde_json::to_value(&status).unwrap()["persisted"],
        "unknown"
    );
    assert!(status.quota_bytes.is_none());
    let denied = block_on(web_status(
        &Mock {
            persisted: Some(false),
            ..Mock::default()
        },
        Some(Persisted::Known(true)),
    ));
    assert_eq!(denied.persisted, Persisted::Known(false));
    let recorded = block_on(web_status(&Mock::default(), Some(Persisted::Known(true))));
    assert_eq!(recorded.persisted, Persisted::Known(true));
}

#[test]
fn persist_is_requested_once_whatever_the_answer() {
    for answer in [Some(true), Some(false), None] {
        let api = Mock {
            persist: answer,
            ..Mock::default()
        };
        let persistence = Persistence::default();
        let first = block_on(persistence.ensure(&api));
        for _ in 0..5 {
            assert_eq!(block_on(persistence.ensure(&api)), first);
        }
        assert_eq!(*api.persist_calls.borrow(), 1);
        assert_eq!(persistence.recorded(), Some(first));
    }
}

#[test]
fn a_full_device_is_a_typed_quota_refusal_with_a_remedy() {
    for raw in [
        "durable_store_invalid: database or disk is full",
        "durable_store_storage_unavailable: QuotaExceededError: The quota has been exceeded.",
        "work_drafts_invalid: No space left on device (os error 28)",
    ] {
        let refused = refine(raw.into());
        assert!(refused.starts_with("store_quota_exhausted: "), "{refused}");
        assert!(refused.contains("Remedy:"));
        assert_eq!(refine(refused.clone()), refused);
    }
    let other = "outbox_full: the queue is full".to_string();
    assert_eq!(refine(other.clone()), other);
    assert!(at_quota(99, Some(100)) && !at_quota(50, Some(100)) && !at_quota(100, None));
}

#[test]
fn an_sqlite_full_error_surfaces_through_execute() {
    let mut db = Connection::open_in_memory().unwrap();
    db.execute_batch("PRAGMA max_page_count = 8;").unwrap();
    let request: Request = serde_json::from_value(json!({
        "store": "work_drafts",
        "operation": {"action": "put", "kind": "form_editor", "key": "k", "value": {"a": "x".repeat(60_000)}, "now": 1}
    }))
    .unwrap();
    let call = || Call {
        request: request.clone(),
        expect_owner: None,
    };
    let mut refused = None;
    for _ in 0..40 {
        if let Err(error) = store(&mut db, Some(&identity()), call(), None) {
            refused = Some(error);
            break;
        }
    }
    let refused = refused.expect("the database fills");
    assert!(
        refused.starts_with("store_quota_exhausted: ") || refused.starts_with("work_drafts_"),
        "{refused}"
    );
}

#[test]
fn a_native_answer_becomes_the_typed_record_and_a_browser_one_is_refused() {
    let mut db = Connection::open_in_memory().unwrap();
    let answer = status_call(&mut db).unwrap();
    let record = from_native_answer(&answer.to_string()).unwrap();
    assert_eq!(record.persisted, Persisted::Known(true));
    let browser = json!({"schema": SCHEMA, "persisted": true, "usage_bytes": 1, "quota_bytes": 2, "backend": "opfs"});
    assert!(from_native_answer(&browser.to_string()).is_err());
    assert!(from_native_answer("{}").is_err());
}
