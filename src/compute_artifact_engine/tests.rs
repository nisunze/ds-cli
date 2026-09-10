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
            cause: None,
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
            cause: None,
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
            cause: None,
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
            cause: None,
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

/// Helper: reach the state where a probe is the pending effect.
fn awaiting_probe() -> Value {
    let ready = prepared();
    let pending = opened(ready.row);
    let restarted = step(pending.row, Event::Start);
    assert!(matches!(restarted.effect, Effect::Probe { .. }));
    restarted.row
}

fn probe(row: Value, status: &str, cause: Option<crate::transfer::Cause>) -> Failure {
    apply(Request {
        owner: "owner".into(),
        now: 1000,
        row,
        event: Event::Probed {
            output_id: "pdf__huye".into(),
            status: status.into(),
            offset: 0,
            cause,
        },
        reopens: 0,
        lease_id: "worker-a".into(),
    })
    .expect_err("an unresolved probe must not advance")
}

#[test]
fn an_unresolved_probe_names_its_cause_instead_of_saying_nothing() {
    use crate::transfer::{Cause, Phase};
    // Every one of these used to render the identical sentence
    // "resumable session could not be checked", which is what made a real
    // Stable failure loop 29 times with nothing to act on.
    let cases = [
        (Cause::Offline, "offline mode"),
        (Cause::Dns, "resolve"),
        (Cause::Connect, "could not reach"),
        (Cause::Tls, "secure connection"),
        (Cause::Timeout { phase: Phase::Idle }, "progress"),
        (Cause::ConnectionClosed, "closed early"),
        (Cause::RangeMalformed, "could not read"),
        (Cause::BlockedByHostPolicy, "given no reason"),
        (Cause::HttpStatus { status: 403 }, "refused"),
        (Cause::HttpStatus { status: 500 }, "answered 500"),
    ];
    let mut seen = std::collections::BTreeSet::new();
    for (cause, fragment) in cases {
        let failure = probe(awaiting_probe(), "inconclusive", Some(cause.clone()));
        assert_eq!(failure.status, "pending", "{cause:?} stays retryable");
        assert!(
            failure.message.contains(fragment),
            "{cause:?} -> {}",
            failure.message
        );
        assert!(
            failure.message.starts_with("pdf__huye: "),
            "the output is named: {}",
            failure.message
        );
        assert!(
            seen.insert(failure.message.clone()),
            "two causes rendered the same sentence: {}",
            failure.message
        );
    }
}

#[test]
fn an_unresolved_probe_keeps_its_session_so_nothing_is_re_sent() {
    // The whole point: "I could not tell" must not discard a session that may
    // already hold most of the artifact.
    let before = awaiting_probe();
    let session = before["outputs"][0]["session_uri"].clone();
    assert!(session.is_string());
    let failure = probe(before, "inconclusive", Some(crate::transfer::Cause::Dns));
    assert_eq!(failure.status, "pending");
    assert!(failure.retry_at.is_none());
    // The refusal path returns no row, so the caller's durable row — with its
    // session_uri — is untouched by construction. Assert the engine did not
    // instead take the reopen branch, which is the branch that clears it.
    assert!(!failure.message.contains("expired"));
}

#[test]
fn a_probe_the_host_proved_hopeless_is_terminal_not_a_loop() {
    let failure = probe(
        awaiting_probe(),
        "failed",
        Some(crate::transfer::Cause::HttpStatus { status: 403 }),
    );
    assert_eq!(failure.status, "failed");
    assert!(failure.message.contains("refused"));
}

#[test]
fn an_unknown_probe_state_is_a_host_defect_not_a_silent_retry() {
    let failure = probe(awaiting_probe(), "banana", None);
    assert_eq!(failure.status, "failed");
    assert!(failure.message.contains("unknown state"));
    assert!(failure.message.contains("banana"));
    // A malformed host cannot write an unbounded string into a durable row.
    let long = probe(awaiting_probe(), &"x".repeat(4096), None);
    assert!(long.message.len() < 200);
}

#[test]
fn probe_causes_never_leak_a_session_or_a_credential() {
    use crate::transfer::Cause;
    for cause in [
        Cause::HttpStatus { status: 401 },
        Cause::SessionExpired,
        Cause::BlockedByHostPolicy,
        Cause::Inconclusive,
    ] {
        for status in ["inconclusive", "failed"] {
            let failure = probe(awaiting_probe(), status, Some(cause.clone()));
            assert!(!failure.message.contains("http"), "{}", failure.message);
            assert!(!failure.message.contains("://"), "{}", failure.message);
            assert!(!failure.message.contains("Bearer"), "{}", failure.message);
            assert!(
                !failure.message.contains("session_uri"),
                "{}",
                failure.message
            );
        }
    }
}
