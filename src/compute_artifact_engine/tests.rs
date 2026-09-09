use super::*;
fn row() -> Value {
    json!({"kind":"compute_artifact","id":"row","owner_uid":"owner","project_id":"project","client_run_id":"run","engine":"network_reporter","operation":"export-agasharu","variant":"default","engine_version":format!("ds-network-reporter@0.2.1+{}","a".repeat(40)),"engine_build_manifest_sha256":"c".repeat(64),"input_base_fingerprint":"b".repeat(64),"batch_id":"d".repeat(64),"client_publish_id":"publish-11111111-1111-4111-8111-111111111111","status":"pending","phase":"pending","attempts":0,"outputs":[{"output_id":"pdf__huye","format":"pdf__huye","content_type":"application/pdf","local_path":"native-private","expected_sha256":"e".repeat(64),"expected_size_bytes":123,"sha256":"","size_bytes":0,"state":"preparing","paper_size":"A0","presentation":{"layout_id":"huye","layout_name":"Huye","paper_name":"A0","orientation":"landscape","page_mm":[1189,841]}}]})
}
fn step(row: Value, event: Event) -> Reply {
    apply(Request {
        owner: "owner".into(),
        now: 1000,
        row,
        event,
        reopens: 0,
        lease_id: "worker-a".into(),
    })
    .unwrap()
}
fn opened(row: Value) -> Reply {
    step(
        row,
        Event::Opened {
            response: json!({"work_id":"work","head_revision":0,"outputs":[{"output_id":"pdf__huye","status":"upload","session_uri":"https://upload.invalid/session"}]}),
        },
    )
}
fn prepared() -> Reply {
    let r = step(row(), Event::Start);
    step(
        r.row,
        Event::Bytes {
            output_id: "pdf__huye".into(),
            sha256: "e".repeat(64),
            size_bytes: 123,
            offset: None,
        },
    )
}
#[test]
fn restart_at_each_persisted_boundary_keeps_bytes_and_work_identity() {
    let initial = row();
    let prepare = step(initial.clone(), Event::Start);
    assert!(matches!(prepare.effect, Effect::Prepare { .. }));
    let restarted = step(prepare.row, Event::Start);
    assert!(matches!(restarted.effect, Effect::Prepare { .. }));
    let ready = prepared();
    let Effect::Open { ref request } = ready.effect else {
        panic!("must open")
    };
    assert_eq!(
        request["outputs"][0]["presentation"],
        initial["outputs"][0]["presentation"]
    );
    assert!(request.to_string().find("native-private").is_none());
    let pending = opened(ready.row);
    let restarted = step(pending.row, Event::Start);
    assert!(matches!(restarted.effect, Effect::Probe { .. }));
    let partial = step(
        restarted.row,
        Event::Probed {
            output_id: "pdf__huye".into(),
            status: "active".into(),
            offset: 64,
        },
    );
    let upload = step(
        partial.row,
        Event::Bytes {
            output_id: "pdf__huye".into(),
            sha256: "e".repeat(64),
            size_bytes: 123,
            offset: Some(64),
        },
    );
    assert!(matches!(upload.effect, Effect::Upload { offset: 64, .. }));
    let restarted = step(upload.row, Event::Start);
    assert!(matches!(restarted.effect, Effect::Probe { .. }));
    // The server's completed probe recovers the crash after upload but before
    // local acknowledgement, without uploading those bytes again.
    let complete = step(
        restarted.row,
        Event::Probed {
            output_id: "pdf__huye".into(),
            status: "completed".into(),
            offset: 123,
        },
    );
    assert!(matches!(complete.effect, Effect::Finalize { .. }));
    let restarted = step(complete.row, Event::Start);
    assert!(matches!(restarted.effect, Effect::Finalize { .. }));
    let published = step(
        restarted.row,
        Event::Finalized {
            response: json!({"work_id":"work","head_revision":2,"state":"published"}),
        },
    );
    assert_eq!(published.row["status"], "synced");
    assert_eq!(published.row["outputs"][0]["local_path"], "native-private");
    assert!(matches!(
        published.effect,
        Effect::Done { state: "published" }
    ));
}
#[test]
fn refuses_changed_bytes_outputs_work_identity_owner_and_terminal_replay() {
    let mut wrong = row();
    wrong["owner_uid"] = json!("other");
    assert!(apply(Request {
        owner: "owner".into(),
        now: 100,
        row: wrong,
        event: Event::Start,
        reopens: 0,
        lease_id: "worker-a".into(),
    })
    .is_err());
    for state in ["synced", "conflict", "failed"] {
        let mut wrong = row();
        wrong["status"] = json!(state);
        assert!(apply(Request {
            owner: "owner".into(),
            now: 100,
            row: wrong,
            event: Event::Start,
            reopens: 0,
            lease_id: "worker-a".into(),
        })
        .is_err());
    }
    let ready = step(row(), Event::Start);
    let changed = apply(Request {
        owner: "owner".into(),
        now: 100,
        row: ready.row,
        event: Event::Bytes {
            output_id: "pdf__huye".into(),
            sha256: "f".repeat(64),
            size_bytes: 123,
            offset: None,
        },
        reopens: 0,
        lease_id: "worker-a".into(),
    })
    .unwrap_err();
    assert_eq!(changed.status, "conflict");
    let ready = prepared();
    assert!(apply(Request {
        owner: "owner".into(),
        now: 100,
        row: ready.row.clone(),
        event: Event::Opened {
            response: json!({"work_id":"work","head_revision":0,"outputs":[]})
        },
        reopens: 0,
        lease_id: "worker-a".into(),
    })
    .is_err());
    let pending = opened(ready.row);
    assert!(apply(Request {
        owner: "owner".into(),
        now: 100,
        row: pending.row,
        event: Event::Finalized {
            response: json!({"state":"published","work_id":"work","head_revision":2})
        },
        reopens: 0,
        lease_id: "worker-a".into(),
    })
    .is_err());
}
#[test]
fn expired_session_reopens_same_run_only_once_per_attempt() {
    let pending = opened(prepared().row);
    let expired = step(
        pending.row,
        Event::Probed {
            output_id: "pdf__huye".into(),
            status: "expired".into(),
            offset: 0,
        },
    );
    assert_eq!(expired.reopens, 1);
    let Effect::Open { request } = expired.effect else {
        panic!("reopen")
    };
    assert_eq!(request["client_run_id"], "run");
    let opened = opened(expired.row);
    let failure = apply(Request {
        owner: "owner".into(),
        now: 100,
        row: opened.row,
        event: Event::Probed {
            output_id: "pdf__huye".into(),
            status: "expired".into(),
            offset: 0,
        },
        reopens: 1,
        lease_id: "worker-a".into(),
    })
    .unwrap_err();
    assert_eq!(failure.status, "pending");
}
#[test]
fn uploaded_is_not_published_and_stale_is_not_success() {
    let mut ready = opened(prepared().row).row;
    ready["outputs"][0]["state"] = json!("uploaded");
    let finalizing = step(ready, Event::Start);
    assert_eq!(finalizing.row["status"], "syncing");
    let failure = apply(Request {
        owner: "owner".into(),
        now: 100,
        row: finalizing.row.clone(),
        event: Event::Finalized {
            response: json!({"work_id":"work","head_revision":0,"state":"pending"}),
        },
        reopens: 0,
        lease_id: "worker-a".into(),
    })
    .unwrap_err();
    assert_eq!(failure.status, "pending");
    let stale = step(
        finalizing.row,
        Event::Finalized {
            response: json!({"work_id":"work","head_revision":0,"state":"stored_stale","conflict":"design changed"}),
        },
    );
    assert_eq!(stale.row["phase"], "stored_stale");
    assert_eq!(stale.row["status"], "conflict");
}

#[test]
fn another_worker_waits_and_expired_workers_cannot_commit_late_responses() {
    let first = step(row(), Event::Start);
    let busy = apply(Request {
        owner: "owner".into(),
        now: 2000,
        row: first.row.clone(),
        event: Event::Start,
        reopens: 0,
        lease_id: "worker-b".into(),
    })
    .unwrap_err();
    assert_eq!(busy.retry_at, Some(301000));
    let second = apply(Request {
        owner: "owner".into(),
        now: 301001,
        row: first.row,
        event: Event::Start,
        reopens: 0,
        lease_id: "worker-b".into(),
    })
    .unwrap();
    let late = apply(Request {
        owner: "owner".into(),
        now: 301002,
        row: second.row,
        event: Event::Bytes {
            output_id: "pdf__huye".into(),
            sha256: "e".repeat(64),
            size_bytes: 123,
            offset: None,
        },
        reopens: 0,
        lease_id: "worker-a".into(),
    })
    .unwrap_err();
    assert_eq!(late.retry_at, Some(301002));
}
