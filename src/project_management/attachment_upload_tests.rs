use super::*;
use serde_json::json;

const SESSION: &str = "https://storage.googleapis.com/upload/storage/v1/b/bucket/o?uploadType=resumable&name=n&upload_id=abc";
const NAME: &str = "09.10.2026. EPC  Checking.doc";
const SIZE: u64 = 19;

fn upload() -> Upload {
    Upload {
        project: "project-a".into(),
        linked_entity_type: "task".into(),
        linked_entity_id: "task-7".into(),
        principal: " Me@Example.com".into(),
        name: NAME.into(),
        content_type: "application/msword".into(),
        size_bytes: SIZE,
        metadata: Map::new(),
    }
}

/// Brain's canonical envelope for one attachment row.
fn row(id: &str, version: i64, state: &str, by: &str, created_at: i64) -> Value {
    json!({"id": id, "version": version, "is_deleted": false,
        "metadata": {"created_by": by, "created_at": created_at, "updated_by": by, "updated_at": created_at},
        "data": {"linked_entity_type": "task", "linked_entity_id": "task-7", "original_name": NAME,
            "content_type": "application/msword", "size_bytes": SIZE, "state": state,
            "storage_path": "project-a/project-work/task/task-7/x/n", "bucket": "b"}})
}

fn context(rows: Vec<Value>, next: Option<&str>) -> Value {
    json!({"context": {"project_id": "project-a", "task_id": "task-7", "attachments": rows,
        "truncated_by_collection": {"pm_attachments": next.is_some()},
        "next_cursors": next.map_or(json!({}), |cursor| json!({"pm_attachments": cursor}))}})
}

fn detached(id: &str, prior: i64) -> Value {
    json!({"project_id": "project-a", "task_id": "task-7", "attachment_id": id,
        "version": prior + 1, "detached": true, "changed": true,
        "item": {"id": id, "version": prior + 1, "task_detached": true,
            "task_detached_prior_version": prior, "task_detached_by": "me@example.com",
            "task_detached_at": "2026-10-20T06:00:00Z",
            "data": {"linked_entity_type": "task", "linked_entity_id": "task-7", "state": "uploading"}}})
}

fn pm(step: Step) -> (Value, Continuation) {
    match step {
        Step::Pm { body, continuation } => (body, continuation),
        other => panic!("expected a PM action, got {other:?}"),
    }
}

fn send(step: Step) -> (String, u64, Continuation) {
    match step {
        Step::Upload {
            session_uri,
            size_bytes,
            continuation,
            ..
        } => (session_uri, size_bytes, continuation),
        other => panic!("expected an upload, got {other:?}"),
    }
}

fn answer(continuation: Continuation, data: Value) -> Step {
    resume(continuation, Observed::Pm { data })
}

fn refusal(step: &Step) -> &str {
    match step {
        Step::Refused { code, .. } => code,
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_first_attach_surveys_the_task_reserves_sends_and_finalizes_one_ready_row() {
    let (survey, next) = pm(start(upload()));
    assert_eq!(
        survey,
        json!({"action": "get_context", "ds_project": "project-a", "task_id": "task-7",
            "limit": 100, "collections": ["pm_attachments"]})
    );
    let foreign = row("theirs", 1, "uploading", "other@example.com", 5);
    let (reserve, next) = pm(answer(next, context(vec![foreign], None)));
    assert_eq!(
        reserve,
        json!({"action": "start_attachment_upload", "ds_project": "project-a", "data": {
            "linked_entity_type": "task", "linked_entity_id": "task-7", "original_name": NAME,
            "content_type": "application/msword", "size_bytes": SIZE}})
    );
    let reserved =
        json!({"item": row("att-1", 1, "uploading", "me@example.com", 9), "session_uri": SESSION});
    let (session, size, next) = send(answer(next, reserved));
    assert_eq!((session.as_str(), size), (SESSION, SIZE));
    let (finalize, next) = pm(resume(next, Observed::Upload { status: 200 }));
    assert_eq!(
        finalize,
        json!({"action": "finalize_attachment_upload", "ds_project": "project-a",
            "id": "att-1", "expected_version": 1})
    );
    let mut ready = row("att-1", 2, "ready", "me@example.com", 9);
    ready["data"]["checksum"] = json!("crc32c:abc");
    match answer(next, json!({"item": ready})) {
        Step::Done {
            row,
            restarted,
            reclaimed,
            unreclaimed,
            ..
        } => {
            assert_eq!(row["id"], "att-1");
            assert_eq!(row["state"], "ready");
            assert_eq!(row["size_bytes"], SIZE);
            assert_eq!(row["linked_entity_id"], "task-7");
            assert_eq!(row["checksum"], "crc32c:abc");
            assert!(!restarted && reclaimed.is_empty() && unreclaimed.is_empty());
        }
        other => panic!("expected done, got {other:?}"),
    }
}

#[test]
fn a_retry_completes_the_newest_interrupted_row_and_detaches_the_older_ones() {
    let (_, next) = pm(start(upload()));
    let mut other_file = row("other-file", 1, "uploading", "me@example.com", 50);
    other_file["data"]["size_bytes"] = json!(SIZE + 1);
    let mut detached_row = row("gone", 1, "uploading", "me@example.com", 60);
    detached_row["task_detached"] = json!(true);
    let rows = vec![
        row("older", 1, "uploading", "ME@example.com", 10),
        row("newest", 1, "uploading", "me@example.com", 30),
        row("failed", 3, "failed", "me@example.com", 40),
        row("ready", 2, "ready", "me@example.com", 45),
        row("someone", 1, "uploading", "other@example.com", 70),
        other_file,
        detached_row,
    ];
    let (restart, next) = pm(answer(next, context(rows, None)));
    assert_eq!(
        restart,
        json!({"action": "restart_attachment_upload", "ds_project": "project-a",
            "id": "newest", "expected_version": 1})
    );
    // A restart answer must name the same row one version on.
    let stale = json!({"item": row("newest", 1, "uploading", "me@example.com", 30), "session_uri": SESSION});
    assert_eq!(refusal(&answer(next.clone(), stale)), ANSWER_UNREADABLE);
    let other =
        json!({"item": row("fresh", 2, "uploading", "me@example.com", 30), "session_uri": SESSION});
    assert_eq!(refusal(&answer(next.clone(), other)), ANSWER_UNREADABLE);
    let restarted = json!({"item": row("newest", 2, "uploading", "me@example.com", 30), "session_uri": SESSION});
    let (_, _, next) = send(answer(next, restarted));
    let (finalize, next) = pm(resume(next, Observed::Upload { status: 201 }));
    assert_eq!(finalize["expected_version"], 2);
    let (first, next) = pm(answer(
        next,
        json!({"item": row("newest", 3, "ready", "me@example.com", 30)}),
    ));
    assert_eq!(
        first,
        json!({"action": "detach_task_attachment", "ds_project": "project-a",
            "task_id": "task-7", "id": "older", "expected_version": 1})
    );
    let (second, next) = pm(answer(next, detached("older", 1)));
    assert_eq!(second["id"], "failed");
    assert_eq!(second["expected_version"], 3);
    match resume(next, Observed::PmFailed) {
        Step::Done {
            row,
            restarted,
            reclaimed,
            unreclaimed,
            ..
        } => {
            assert_eq!(row["id"], "newest");
            assert!(restarted);
            assert_eq!(reclaimed, vec!["older"]);
            assert_eq!(unreclaimed, vec!["failed"]);
        }
        other => panic!("expected done, got {other:?}"),
    }
}

#[test]
fn the_survey_follows_truncated_pages_within_its_bound() {
    let (_, next) = pm(start(upload()));
    let (page, next) = pm(answer(
        next,
        context(
            vec![row("a", 1, "uploading", "me@example.com", 1)],
            Some("a"),
        ),
    ));
    assert_eq!(page["cursors"], json!({"pm_attachments": "a"}));
    let (reserve, _) = pm(answer(
        next,
        context(vec![row("b", 4, "uploading", "me@example.com", 2)], None),
    ));
    assert_eq!(reserve["id"], "b");
    assert_eq!(reserve["expected_version"], 4);

    let mut step = start(upload());
    let mut surveys = 0;
    loop {
        let (body, next) = pm(step);
        if body["action"] != "get_context" {
            assert_eq!(body["action"], "start_attachment_upload");
            break;
        }
        surveys += 1;
        step = answer(next, context(Vec::new(), Some("more")));
    }
    assert_eq!(surveys, MAX_SURVEY_PAGES);
}

#[test]
fn the_flat_legacy_answer_from_2891150c_is_refused_before_any_byte_moves() {
    let (_, next) = pm(start(upload()));
    let (_, next) = pm(answer(next, context(Vec::new(), None)));
    let flat = json!({"item": {"id": "att-1", "version": 1, "linked_entity_type": "task",
        "linked_entity_id": "task-7", "state": "uploading", "size_bytes": SIZE},
        "session_uri": SESSION});
    assert_eq!(refusal(&answer(next.clone(), flat)), ANSWER_UNREADABLE);
    let foreign = json!({"item": row("att-1", 1, "uploading", "me@example.com", 1),
        "session_uri": "https://example.com/upload/storage/v1/b/b/o?upload_id=abc"});
    assert_eq!(refusal(&answer(next.clone(), foreign)), ANSWER_UNREADABLE);
    let missing = json!({"item": row("att-1", 1, "uploading", "me@example.com", 1)});
    assert_eq!(refusal(&answer(next, missing)), ANSWER_UNREADABLE);
}

#[test]
fn failed_bytes_keep_the_row_for_a_retry_and_a_cancel_releases_it() {
    let reserved = |entity: &str| {
        let mut upload = upload();
        upload.linked_entity_type = entity.into();
        let next = match start(upload) {
            Step::Pm { body, continuation } if body["action"] == "get_context" => {
                pm(answer(continuation, context(Vec::new(), None))).1
            }
            Step::Pm { continuation, .. } => continuation,
            other => panic!("expected a PM action, got {other:?}"),
        };
        let mut item = row("att-1", 1, "uploading", "me@example.com", 1);
        item["data"]["linked_entity_type"] = json!(entity);
        send(answer(next, json!({"item": item, "session_uri": SESSION}))).2
    };
    for observed in [
        Observed::Upload { status: 503 },
        Observed::UploadFailed { cancelled: false },
    ] {
        assert_eq!(
            refusal(&resume(reserved("task"), observed)),
            BYTES_INCOMPLETE
        );
    }
    let (detach, next) = pm(resume(
        reserved("task"),
        Observed::UploadFailed { cancelled: true },
    ));
    assert_eq!(detach["action"], "detach_task_attachment");
    assert_eq!(detach["expected_version"], 1);
    assert_eq!(refusal(&resume(next, Observed::PmFailed)), CANCELLED);
    let (failed, next) = pm(resume(
        reserved("record"),
        Observed::UploadFailed { cancelled: true },
    ));
    assert_eq!(
        failed,
        json!({"action": "update_attachment", "ds_project": "project-a", "id": "att-1",
            "expected_version": 1, "data": {"state": "failed"}})
    );
    assert_eq!(refusal(&answer(next, json!({}))), CANCELLED);
}

#[test]
fn an_unverified_finalize_and_host_failures_stop_the_upload() {
    let (_, next) = pm(start(upload()));
    assert_eq!(resume(next.clone(), Observed::PmFailed), Step::HostFailed);
    let (_, next) = pm(answer(next, context(Vec::new(), None)));
    assert_eq!(resume(next.clone(), Observed::PmFailed), Step::HostFailed);
    let reserved =
        json!({"item": row("att-1", 1, "uploading", "me@example.com", 1), "session_uri": SESSION});
    let (_, _, next) = send(answer(next, reserved));
    let (_, next) = pm(resume(next, Observed::Upload { status: 200 }));
    assert_eq!(resume(next.clone(), Observed::PmFailed), Step::HostFailed);
    let mut wrong_size = row("att-1", 2, "ready", "me@example.com", 1);
    wrong_size["data"]["size_bytes"] = json!(SIZE - 1);
    for item in [row("att-1", 2, "failed", "me@example.com", 1), wrong_size] {
        assert_eq!(
            refusal(&answer(next.clone(), json!({"item": item}))),
            UNVERIFIED
        );
    }
    assert_eq!(
        refusal(&resume(next, Observed::Upload { status: 200 })),
        ANSWER_UNREADABLE
    );
}

#[test]
fn other_entities_reserve_directly_with_their_authored_fields() {
    let mut upload = upload();
    upload.linked_entity_type = "record".into();
    upload.linked_entity_id = "rec-1".into();
    upload
        .metadata
        .insert("document_number".into(), json!("EDCL-7"));
    let (body, _) = pm(start(upload));
    assert_eq!(body["action"], "start_attachment_upload");
    assert_eq!(body["data"]["linked_entity_type"], "record");
    assert_eq!(body["data"]["document_number"], "EDCL-7");
}

#[test]
fn invalid_input_is_refused_before_any_effect() {
    let cases: [fn(&mut Upload); 7] = [
        |u| u.size_bytes = 0,
        |u| u.size_bytes = MAX_ATTACHMENT_BYTES + 1,
        |u| u.content_type = "pdf".into(),
        |u| u.principal = " ".into(),
        |u| u.name = "  ".into(),
        |u| u.linked_entity_type = "folder".into(),
        |u| {
            u.metadata.insert("bucket".into(), json!("elsewhere"));
        },
    ];
    for change in cases {
        let mut upload = upload();
        change(&mut upload);
        assert_eq!(refusal(&start(upload)), INPUT_INVALID);
    }
}

#[test]
fn the_browser_entry_round_trips_the_same_steps() {
    let started = super::super::evaluate(
        json!({"schema": super::super::SCHEMA, "action": "attachment.start", "upload": upload()})
            .to_string()
            .as_bytes(),
    )
    .unwrap();
    let step: Value = serde_json::from_str(&started).unwrap();
    assert_eq!(step["kind"], "pm");
    assert_eq!(step["body"]["action"], "get_context");
    let resumed = super::super::evaluate(
        json!({"schema": super::super::SCHEMA, "action": "attachment.resume",
            "continuation": step["continuation"], "observed": {"kind": "pm", "data": context(Vec::new(), None)}})
        .to_string()
        .as_bytes(),
    )
    .unwrap();
    let step: Value = serde_json::from_str(&resumed).unwrap();
    assert_eq!(step["body"]["action"], "start_attachment_upload");
    assert!(super::super::evaluate(
        json!({"schema": super::super::SCHEMA, "action": "attachment.resume",
            "continuation": {"stage": "cancel"}, "observed": {"kind": "teleport"}})
        .to_string()
        .as_bytes(),
    )
    .is_err());
}
