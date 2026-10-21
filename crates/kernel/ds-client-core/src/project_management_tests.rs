use super::*;

#[test]
fn task_records_ids_accept_stable_references_and_refuse_invalid_bounds() {
    for id in [
        "550e8400-e29b-41d4-a716-446655440000".into(),
        "note:review@example.com+2026_09-30.é".into(),
        "a".repeat(256),
        "é".repeat(128),
    ] {
        Command::TaskRecords {
            task_id: id,
            limit: 100,
        }
        .validate()
        .expect("a server-valid bounded stable id");
    }
    for id in [
        "",
        ".",
        "..",
        "a/b",
        " a",
        "a ",
        "a b",
        "a\t",
        "a\n",
        "a\0",
        "a\u{2003}b",
    ]
    .into_iter()
    .map(str::to_owned)
    .chain(["a".repeat(257), "é".repeat(129)])
    {
        let error = Command::TaskRecords {
            task_id: id,
            limit: 100,
        }
        .validate()
        .expect_err("an invalid stable id is refused locally");
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
    }
}

#[test]
fn proposal_and_admission_payloads_keep_the_governed_shape() {
    let proposal = Command::Propose {
        command_id: "cmd-propose-0001".into(),
        base_revision: 4,
        task_id: "p1".into(),
        title: "Survey the extension".into(),
        description: None,
        discipline: Some("survey".into()),
        responsible_email: "c@x.rw".into(),
        estimated_hours: 12.0,
        estimated_days: Some(3),
    }
    .payload("proj");
    assert_eq!(
        proposal,
        json!({
            "action": "commit_command", "ds_project": "proj",
            "command_id": "cmd-propose-0001", "base_revision": 4,
            "command": {"kind": "create_task", "create_task": {
                "id": "p1", "title": "Survey the extension", "placement": "inbox",
                "schedule_state": "unscheduled", "responsible_email": "c@x.rw",
                "estimated_hours": 12.0, "estimated_days": 3, "discipline": "survey"
            }}
        })
    );
    let admit = Command::Admit {
        command_id: "cmd-admit-0001".into(),
        base_revision: 5,
        task_id: "p1".into(),
        parent_task_id: String::new(),
        target_index: None,
    }
    .payload("proj");
    assert!(admit.get("parent_task_id").is_none());
    assert!(admit.get("target_index").is_none());
}

#[test]
fn proposal_refusal_carries_its_reason_token() {
    let refused = refusal(409, br#"{"success":false,"error":{"code":"PM_REFUSED","message":"Survey is already placed in the plan","details":{"reason":"task_already_placed"}}}"#);
    let detail = refused.service_refusal().expect("bounded refusal");
    assert_eq!(detail.code(), Some("task_already_placed"));
    assert_eq!(detail.status(), 409);
}

#[test]
fn the_action_vocabulary_is_closed_and_named() {
    assert_eq!(Command::Graph.action(), "get_graph");
    assert_eq!(Command::Assignees.action(), "get_assignees");
    assert_eq!(
        Command::Assignees.payload("p"),
        json!({"action":"get_assignees","ds_project":"p"})
    );
    assert_eq!(
        Command::ModelReferences {
            model_id: "model-a".into(),
            revision_id: None,
            records_cursor: None
        }
        .action(),
        "get_graph"
    );
    let commit = Command::Commit {
        command_id: "c1".into(),
        base_revision: 3,
        command: json!({"kind": "accept_assignment", "accept_assignment": {"task_id": "t"}}),
    };
    assert_eq!(commit.action(), "commit_command");
    let payload = commit.payload("p");
    assert_eq!(payload["command_id"], "c1");
    assert_eq!(payload["base_revision"], 3);
    assert_eq!(payload["command"]["kind"], "accept_assignment");
    let batch = Command::CommitBatch {
        command_id: "c2".into(),
        base_revision: 0,
        commands: vec![
            json!({"kind": "set_progress", "set_progress": {"task_id": "t", "progress": 0.5}}),
        ],
    };
    assert_eq!(batch.action(), "commit_batch");
    assert_eq!(
        batch.payload("p")["commands"].as_array().map(Vec::len),
        Some(1)
    );
}

/// The bounds the server enforces are refused here first, by the same
/// numbers, so a round trip is never spent to learn them.
#[test]
fn a_command_outside_its_bounds_is_refused_before_it_is_sent() {
    let kind = |command: Command| command.validate().expect_err("refused").kind();
    assert_eq!(
        kind(Command::ModelReferences {
            model_id: "Display label".into(),
            revision_id: None,
            records_cursor: None
        }),
        ErrorKind::InvalidInput
    );
    let good = json!({"kind": "accept_assignment", "accept_assignment": {"task_id": "t"}});
    assert!(Command::Commit {
        command_id: "pm-1_a.b".into(),
        base_revision: 0,
        command: good.clone()
    }
    .validate()
    .is_ok());
    assert_eq!(
        kind(Command::Commit {
            command_id: "".into(),
            base_revision: 0,
            command: good.clone()
        }),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        kind(Command::Commit {
            command_id: "c".into(),
            base_revision: -1,
            command: good.clone()
        }),
        ErrorKind::InvalidInput
    );
    for bad in [
        json!({"kind": "accept_assignment"}),
        json!({"kind": "accept_assignment", "decline_assignment": {}}),
        json!({"kind": "accept_assignment", "accept_assignment": {}, "extra": 1}),
        json!([]),
    ] {
        assert_eq!(
            kind(Command::Commit {
                command_id: "c".into(),
                base_revision: 0,
                command: bad
            }),
            ErrorKind::InvalidInput
        );
    }
    assert_eq!(
        kind(Command::CommitBatch {
            command_id: "c".into(),
            base_revision: 0,
            commands: vec![]
        }),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        kind(Command::CommitBatch {
            command_id: "c".into(),
            base_revision: 0,
            commands: vec![good; MAX_BATCH_COMMANDS + 1],
        }),
        ErrorKind::InvalidInput
    );
}

/// The server's code and sentence cross the boundary; the status alone
/// decides the class.
#[test]
fn a_refusal_carries_the_servers_own_code_and_sentence() {
    let body = br#"{"success":false,"error":{"code":"PM_REVISION_CONFLICT","message":"The plan moved to revision 9.","details":{"current_revision":9}}}"#;
    let error = refusal(409, body);
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    let served = error.service_refusal().expect("carried");
    assert_eq!(served.status(), 409);
    assert_eq!(served.code(), Some("pm_revision_conflict"));
    assert_eq!(served.message(), Some("The plan moved to revision 9."));

    let forbidden = refusal(403, br#"{"success":false,"error":{"code":"INSUFFICIENT_PERMISSIONS","message":"Project schedule editor access required"}}"#);
    assert_eq!(forbidden.kind(), ErrorKind::AuthenticationRejected);
    assert_eq!(
        forbidden.service_refusal().and_then(|r| r.code()),
        Some("insufficient_permissions")
    );

    let missing = refusal(
        404,
        br#"{"success":false,"error":{"code":"NOT_FOUND","message":"Project not found"}}"#,
    );
    assert_eq!(missing.kind(), ErrorKind::ResourceNotFound);

    // A gateway 503 with no DS envelope: the class and the status,
    // nothing invented.
    let outage = refusal(503, b"upstream unavailable");
    assert_eq!(outage.kind(), ErrorKind::Transient);
    let served = outage.service_refusal().expect("the status is carried");
    assert_eq!(served.status(), 503);
    assert!(served.code().is_none() && served.message().is_none());
}

/// A projectless call is refused before anything is sent, and the project
/// is trimmed rather than sent with the whitespace an operator pasted.
#[test]
fn a_blank_project_is_refused_and_a_padded_one_is_trimmed() {
    for blank in ["", "   ", "\t\n"] {
        let error = readable_project(blank).expect_err("a blank project is refused");
        assert_eq!(error.kind(), ErrorKind::InvalidInput);
    }
    assert_eq!(
        readable_project("  arjgpydw_nyam  ").expect("a padded project still reads"),
        "arjgpydw_nyam",
    );
}

/// A transport that answers the attachment survey, start or restart, the byte
/// upload and the finalize in turn, recording what was sent. Rows use ds-brain's
/// canonical envelope and echo the requested file name.
struct AttachTransport {
    created: Option<Value>,
    fail_upload_at: Option<usize>,
    started: Value,
    upload_status: u16,
    finalized: Value,
    /// The task's attachment context the survey reads.
    attachments: Value,
    names: std::collections::BTreeMap<String, Value>,
    payloads: Vec<Value>,
    authorities: Vec<(String, String)>,
    uploaded: Vec<(String, Vec<u8>)>,
}
macro_rules! no_other_call {
    ($name:ident, $call:ident) => {
        fn $name(
            &mut self,
            _: crate::$call<'_>,
        ) -> Result<crate::TransportResponse, crate::TransportError> {
            panic!("a task attachment made an unrelated call")
        }
    };
}
impl AuthorizedTransport for AttachTransport {
    no_other_call!(list_projects, ProjectListCall);
    no_other_call!(transformer_context, TransformerContextCall);
    no_other_call!(project_forms, ProjectFormsCall);
    no_other_call!(project_form_editor, ProjectFormEditorCall);
    no_other_call!(solar_snapshot, SolarSnapshotCall);
    no_other_call!(survey_query, SurveyQueryCall);
    no_other_call!(survey_entries_select, SurveyEntriesSelectCall);
    no_other_call!(survey_entries_changes, SurveyEntriesChangesCall);
    no_other_call!(survey_entry_create, SurveyEntryCreateCall);
    no_other_call!(project_data, ProjectDataCall);
    no_other_call!(survey_control, SurveyControlCall);
    no_other_call!(styles, StylesCall);
    no_other_call!(layers, LayersCall);
    no_other_call!(tiles, TileCall);
    no_other_call!(project_report, ProjectReportCall);
    fn upload_bytes(
        &mut self,
        mut call: crate::UploadBytesCall<'_>,
    ) -> Result<crate::TransportResponse, crate::TransportError> {
        let mut bytes = Vec::new();
        call.reader()
            .read_to_end(&mut bytes)
            .expect("upload reader");
        assert_eq!(bytes.len() as u64, call.size());
        self.uploaded.push((call.uri().to_owned(), bytes));
        Ok(crate::TransportResponse::new(
            if self.fail_upload_at == Some(self.uploaded.len()) {
                500
            } else {
                self.upload_status
            },
            Vec::new(),
        ))
    }
    fn project_management(
        &mut self,
        call: ProjectManagementCall<'_>,
    ) -> Result<crate::TransportResponse, crate::TransportError> {
        self.authorities.push((
            call.authority.canonical_uid.into(),
            call.authority.canonical_email.into(),
        ));
        if matches!(
            call.payload["action"].as_str(),
            Some("get_graph" | "get_context")
        ) && self.upload_status == 403
        {
            self.payloads.push(call.payload);
            return Ok(crate::TransportResponse::new(403, br#"{"success":false,"error":{"code":"INSUFFICIENT_PERMISSIONS","message":"Project unavailable"}}"#.to_vec()));
        }
        let data = match call.payload["action"].as_str() {
            Some("commit_command") => self.created.clone().expect("explicit creation fixture"),
            Some("detach_task_attachment") => self.started.clone(),
            Some("get_graph") => self.started.clone(),
            Some("get_context") if call.payload["collections"] == json!(["pm_attachments"]) => {
                self.attachments.clone()
            }
            Some("get_context") => self.finalized.clone(),
            Some("start_attachment_upload") => {
                let id = format!("att-{}", self.uploaded.len() + 1);
                let name = call.payload["data"]["original_name"].clone();
                self.names.insert(id.clone(), name.clone());
                let mut started = self.started.clone();
                started["item"]["id"] = json!(id);
                started["item"]["data"]["original_name"] = name;
                started
            }
            Some("restart_attachment_upload") => {
                let id = call.payload["id"].as_str().unwrap().to_owned();
                let mut started = self.started.clone();
                started["item"]["id"] = json!(id);
                started["item"]["version"] =
                    json!(call.payload["expected_version"].as_i64().unwrap() + 1);
                started["item"]["data"]["original_name"] = self.names[&id].clone();
                started
            }
            Some("finalize_attachment_upload") => {
                let id = call.payload["id"].as_str().unwrap();
                let mut finalized = self.finalized.clone();
                finalized["item"]["id"] = json!(id);
                finalized["item"]["data"]["original_name"] = self.names[id].clone();
                finalized
            }
            other => panic!("unexpected PM action {other:?}"),
        };
        self.payloads.push(call.payload);
        Ok(crate::TransportResponse::new(
            200,
            serde_json::to_vec(&json!({"success": true, "data": data})).unwrap(),
        ))
    }
}

fn create_with_files(
    transport: &mut AttachTransport,
    files: Vec<AttachmentFile>,
) -> Result<Value, ClientError> {
    run(
        &crate::data_distribution::tests::profile_for_tests(),
        transport,
        crate::authorized::Authority {
            bearer_token: "bearer",
            uid: "uid",
            email: "user@example.com",
            device_id: None,
        },
        "project-a",
        &Command::TaskCreateAttached {
            command_id: "create-7".into(),
            base_revision: 7,
            command: json!({"kind":"create_task","create_task":{"id":"task-7","title":"Evidence"}}),
            task_id: "task-7".into(),
            files,
        },
    )
}

/// ds-brain sends `violations: null` for a clean commit (a nil Go slice).
fn create_fixture(applied: bool) -> Value {
    json!({"result":{"applied":applied,"command_id":"create-7","committed_revision":8,
        "violations":if applied {Value::Null} else {json!([{"code":"PROJECT_WORK_REVISION_CONFLICT","message":"Plan moved"}])}}})
}

fn records_transport(finalized: Value) -> AttachTransport {
    AttachTransport {
        created: None,
        fail_upload_at: None,
        started: json!({}),
        upload_status: 200,
        finalized,
        attachments: Value::Null,
        names: Default::default(),
        payloads: vec![],
        authorities: vec![],
        uploaded: vec![],
    }
}

fn task_records_fixture(truncated: bool) -> Value {
    json!({"context":{"project_id":"project-a","task_id":"task-7",
        "records":[{"id":"record-1","data":{"related_task_ids":["task-7"]}}],
        "loaded_collections":["pm_records"],
        "truncated_by_collection":{"pm_records":truncated},
        "next_cursors":if truncated {json!({"pm_records":"record-1"})} else {json!({})}}})
}

fn selected_records(transport: &mut AttachTransport, limit: i64) -> Result<Value, ClientError> {
    run(
        &crate::data_distribution::tests::profile_for_tests(),
        transport,
        crate::authorized::Authority {
            bearer_token: "bearer",
            uid: "uid",
            email: "user@example.com",
            device_id: None,
        },
        "project-a",
        &Command::TaskRecords {
            task_id: "task-7".into(),
            limit,
        },
    )
}

#[test]
fn task_records_one_selected_request_preserves_authoritative_completeness() {
    for truncated in [false, true] {
        let mut transport = records_transport(task_records_fixture(truncated));
        let context = selected_records(&mut transport, 100).unwrap();
        assert_eq!(
            transport.payloads,
            vec![
                json!({"action":"get_context","ds_project":"project-a","task_id":"task-7","limit":100,"collections":["pm_records"]})
            ]
        );
        assert_eq!(context["truncated_by_collection"]["pm_records"], truncated);
        assert_eq!(
            context["next_cursors"].get("pm_records").is_some(),
            truncated
        );
        assert!(transport.uploaded.is_empty());
    }
}

#[test]
fn task_records_denied_or_unproven_page_never_becomes_empty_success() {
    let fixture = task_records_fixture(false);
    let mut cases = Vec::new();
    for key in [
        "project_id",
        "task_id",
        "records",
        "loaded_collections",
        "truncated_by_collection",
        "next_cursors",
    ] {
        let mut altered = fixture.clone();
        altered["context"].as_object_mut().unwrap().remove(key);
        cases.push(altered);
    }
    for (key, wrong) in [
        ("project_id", json!("other")),
        ("task_id", json!("other")),
        (
            "loaded_collections",
            json!(["pm_records", "pm_attachments"]),
        ),
        (
            "records",
            json!([{"id":"record-1","data":{"related_task_ids":["other"]}}]),
        ),
        ("next_cursors", json!({"pm_records":"record-1"})),
    ] {
        let mut altered = fixture.clone();
        altered["context"][key] = wrong;
        cases.push(altered);
    }
    for bad in cases {
        let mut transport = records_transport(bad);
        assert_eq!(
            selected_records(&mut transport, 100).unwrap_err().kind(),
            ErrorKind::UnreadableResponse
        );
        assert_eq!(transport.payloads.len(), 1);
    }
    let mut denied = records_transport(fixture);
    denied.upload_status = 403;
    assert_eq!(
        selected_records(&mut denied, 100).unwrap_err().kind(),
        ErrorKind::AuthenticationRejected
    );
    assert_eq!(denied.payloads.len(), 1);
    let mut invalid = records_transport(task_records_fixture(false));
    assert_eq!(
        selected_records(&mut invalid, 0).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
    assert!(invalid.payloads.is_empty());
}

fn two_files() -> Vec<AttachmentFile> {
    vec![
        AttachmentFile::new("source.eml", None, vec![1; 4]).unwrap(),
        AttachmentFile::new("drawing.pdf", None, vec![2; 4]).unwrap(),
    ]
}

#[test]
fn composed_task_creation_verifies_two_files_without_unrelated_reads() {
    let mut transport = attach_transport("ready", 4);
    transport.created = Some(create_fixture(true));
    let answer = create_with_files(&mut transport, two_files()).unwrap();
    assert_eq!(answer["complete"], true);
    assert_eq!(answer["task_id"], "task-7");
    assert_eq!(answer["result"]["committed_revision"], 8);
    assert_eq!(answer["attachments"].as_array().unwrap().len(), 2);
    assert_eq!(answer["attachments"][0]["item"]["id"], "att-1");
    assert_eq!(answer["attachments"][1]["item"]["id"], "att-2");
    assert_eq!(transport.uploaded.len(), 2);
    assert_eq!(
        transport
            .payloads
            .iter()
            .map(|p| p["action"].as_str().unwrap())
            .collect::<Vec<_>>(),
        [
            "commit_command",
            "get_context",
            "start_attachment_upload",
            "finalize_attachment_upload",
            "get_context",
            "start_attachment_upload",
            "finalize_attachment_upload"
        ]
    );
    assert!(transport
        .payloads
        .iter()
        .all(|p| p["ds_project"] == "project-a"));
    assert!(!answer.to_string().contains("upload_id"));
    assert!(!answer.to_string().contains("bearer"));
}

#[test]
fn composed_upload_failure_retains_the_created_task_and_only_ready_files() {
    let mut transport = attach_transport("ready", 4);
    transport.created = Some(create_fixture(true));
    transport.fail_upload_at = Some(2);
    let mut files = two_files();
    files.push(AttachmentFile::new("later.txt", None, vec![3; 4]).unwrap());
    let answer = create_with_files(&mut transport, files).unwrap();
    assert_eq!(answer["complete"], false);
    assert_eq!(answer["task_id"], "task-7");
    assert_eq!(answer["result"]["applied"], true);
    assert_eq!(answer["failed_attachment_index"], 1);
    assert_eq!(answer["requested_attachment_count"], 3);
    assert_eq!(answer["attachments"].as_array().unwrap().len(), 1);
    assert_eq!(transport.uploaded.len(), 2);
    // Survey and reserve per file; no retry, third reservation or failed finalize.
    assert_eq!(transport.payloads.len(), 6);
}

#[test]
fn refused_task_creation_never_reserves_or_uploads_any_file() {
    let mut transport = attach_transport("ready", 4);
    transport.created = Some(create_fixture(false));
    let answer = create_with_files(&mut transport, two_files()).unwrap();
    assert_eq!(answer["result"]["applied"], false);
    assert_eq!(answer["complete"], false);
    assert!(transport.uploaded.is_empty());
    assert_eq!(transport.payloads.len(), 1);
}

#[test]
fn composed_attachment_bounds_and_mismatched_task_refuse_before_any_effect() {
    for files in [
        vec![],
        vec![two_files()[0].clone(); 11],
        vec![two_files()[0].clone(); 2],
    ] {
        let mut transport = attach_transport("ready", 4);
        assert_eq!(
            create_with_files(&mut transport, files).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert!(transport.payloads.is_empty());
    }
    let bad = Command::TaskCreateAttached {
        command_id: "create-7".into(),
        base_revision: 7,
        command: json!({"kind":"create_task","create_task":{"id":"another-task"}}),
        task_id: "task-7".into(),
        files: two_files(),
    };
    assert_eq!(bad.validate().unwrap_err().kind(), ErrorKind::InvalidInput);
}

const SESSION: &str = "https://storage.googleapis.com/upload/storage/v1/b/bucket/o?uploadType=resumable&upload_id=abc";

/// One attachment row in ds-brain's canonical envelope.
fn envelope(id: &str, version: i64, state: &str, name: &str, size: usize) -> Value {
    json!({"id": id, "version": version, "is_deleted": false,
        "metadata": {"created_by": "user@example.com", "created_at": 1, "updated_by": "user@example.com", "updated_at": 1},
        "data": {"linked_entity_type": "task", "linked_entity_id": "task-7", "original_name": name,
            "content_type": "application/octet-stream", "size_bytes": size, "state": state}})
}

fn attachments_context(rows: Vec<Value>) -> Value {
    json!({"context": {"project_id": "project-a", "task_id": "task-7", "attachments": rows,
        "truncated_by_collection": {"pm_attachments": false}, "next_cursors": {}}})
}

fn attach_transport(state: &str, size: usize) -> AttachTransport {
    AttachTransport {
        created: None,
        fail_upload_at: None,
        started: json!({"item": envelope("att-1", 1, "uploading", "", size), "session_uri": SESSION}),
        upload_status: 200,
        finalized: json!({"item": envelope("att-1", 2, state, "", size)}),
        attachments: attachments_context(Vec::new()),
        names: Default::default(),
        payloads: Vec::new(),
        authorities: Vec::new(),
        uploaded: Vec::new(),
    }
}

fn attach(transport: &mut AttachTransport, file: AttachmentFile) -> Result<Value, ClientError> {
    let profile = crate::data_distribution::tests::profile_for_tests();
    run(
        &profile,
        transport,
        crate::authorized::Authority {
            bearer_token: "bearer",
            uid: "uid",
            email: "user@example.com",
            device_id: None,
        },
        "project-a",
        &Command::TaskAttach {
            task_id: "task-7".into(),
            file,
        },
    )
}

#[test]
fn task_attach_starts_uploads_the_exact_bytes_and_finalizes_the_reserved_row() {
    let bytes = b"%PDF-1.7 field plan".to_vec();
    let file = AttachmentFile::new("field plan.pdf", None, bytes.clone()).unwrap();
    let mut transport = attach_transport("ready", bytes.len());
    let receipt = attach(&mut transport, file.clone()).expect("attached");
    assert_eq!(
        transport.payloads[0],
        json!({"action": "get_context", "ds_project": "project-a", "task_id": "task-7",
               "limit": 100, "collections": ["pm_attachments"]})
    );
    assert_eq!(
        transport.payloads[1],
        json!({"action": "start_attachment_upload", "ds_project": "project-a",
               "data": {"linked_entity_type": "task", "linked_entity_id": "task-7",
                        "original_name": "field plan.pdf", "content_type": "application/pdf",
                        "size_bytes": bytes.len()}})
    );
    assert_eq!(
        transport.uploaded,
        vec![(SESSION.to_owned(), bytes.clone())]
    );
    assert_eq!(
        transport.payloads[2],
        json!({"action": "finalize_attachment_upload", "ds_project": "project-a",
               "id": "att-1", "expected_version": 1})
    );
    assert_eq!(receipt["item"]["state"], "ready");
    assert_eq!(receipt["item"]["linked_entity_id"], "task-7");
    assert_eq!(receipt["item"]["original_name"], "field plan.pdf");
    assert_eq!(receipt["restarted"], false);
    assert_eq!(receipt["sha256"], file.sha256());
    assert_eq!(receipt["bytes"], bytes.len());
    assert!(
        !receipt.to_string().contains("upload_id"),
        "the session never leaves the door"
    );
}

#[test]
fn task_attach_refuses_an_unverified_row_a_foreign_session_and_a_failed_upload() {
    let bytes = b"PK\x03\x04".to_vec();
    let file = AttachmentFile::new("pack.zip", None, bytes.clone()).unwrap();
    let mut unverified = attach_transport("failed", bytes.len());
    assert_eq!(
        attach(&mut unverified, file.clone()).unwrap_err().kind(),
        ErrorKind::UnreadableResponse
    );
    let mut foreign = attach_transport("ready", bytes.len());
    foreign.started["session_uri"] = json!("https://example.com/upload?upload_id=abc");
    assert_eq!(
        attach(&mut foreign, file.clone()).unwrap_err().kind(),
        ErrorKind::UnreadableResponse
    );
    assert!(
        foreign.uploaded.is_empty(),
        "no byte leaves for a foreign session"
    );
    let mut other_task = attach_transport("ready", bytes.len());
    other_task.started["item"]["data"]["linked_entity_id"] = json!("task-8");
    assert!(attach(&mut other_task, file.clone()).is_err());
    assert!(other_task.uploaded.is_empty());
    let mut failed = attach_transport("ready", bytes.len());
    failed.upload_status = 503;
    assert_eq!(
        attach(&mut failed, file).unwrap_err().kind(),
        ErrorKind::Transient
    );
    assert_eq!(
        failed.payloads.len(),
        2,
        "an incomplete upload is never finalized"
    );
}

#[test]
fn the_flat_answer_from_feedback_2891150c_is_refused_before_any_byte_moves() {
    let file = AttachmentFile::new("email thread.pdf", None, vec![7; 5]).unwrap();
    let mut flat = attach_transport("ready", 5);
    flat.started = json!({"item": {"id": "att-1", "version": 1, "linked_entity_type": "task",
        "linked_entity_id": "task-7", "state": "uploading", "size_bytes": 5}, "session_uri": SESSION});
    assert_eq!(
        attach(&mut flat, file).unwrap_err().kind(),
        ErrorKind::UnreadableResponse
    );
    assert!(flat.uploaded.is_empty());
}

#[test]
fn a_retry_completes_the_interrupted_row_and_detaches_an_older_duplicate() {
    let bytes = b"# index".to_vec();
    let file = AttachmentFile::new("attachment-index.md", None, bytes.clone()).unwrap();
    let mut transport = attach_transport("ready", bytes.len());
    transport
        .names
        .insert("older".into(), json!("attachment-index.md"));
    transport
        .names
        .insert("newest".into(), json!("attachment-index.md"));
    let mut older = envelope("older", 1, "uploading", "attachment-index.md", bytes.len());
    older["metadata"]["created_at"] = json!(10);
    let mut newest = envelope("newest", 1, "uploading", "attachment-index.md", bytes.len());
    newest["metadata"]["created_at"] = json!(20);
    let mut theirs = envelope("theirs", 1, "uploading", "attachment-index.md", bytes.len());
    theirs["metadata"]["created_by"] = json!("someone@example.com");
    transport.attachments = attachments_context(vec![older, newest, theirs]);
    transport.started = json!({"project_id": "project-a", "task_id": "task-7",
        "attachment_id": "older", "version": 2, "detached": true, "changed": true,
        "item": {"id": "older", "version": 2, "task_detached": true,
            "task_detached_prior_version": 1, "task_detached_by": "user@example.com",
            "task_detached_at": "2026-10-20T06:00:00Z",
            "data": {"linked_entity_type": "task", "linked_entity_id": "task-7", "state": "uploading"}},
        "session_uri": SESSION});
    transport.started["item"]["data"]["size_bytes"] = json!(bytes.len());
    transport.started["item"]["is_deleted"] = json!(false);
    let receipt = attach(&mut transport, file).expect("attached");
    let actions: Vec<_> = transport
        .payloads
        .iter()
        .map(|p| p["action"].as_str().unwrap())
        .collect();
    assert_eq!(
        actions,
        [
            "get_context",
            "restart_attachment_upload",
            "finalize_attachment_upload",
            "detach_task_attachment"
        ]
    );
    assert_eq!(
        transport.payloads[1],
        json!({"action": "restart_attachment_upload", "ds_project": "project-a",
            "id": "newest", "expected_version": 1})
    );
    assert_eq!(transport.payloads[2]["expected_version"], 2);
    assert_eq!(transport.payloads[3]["id"], "older");
    assert_eq!(transport.uploaded, vec![(SESSION.to_owned(), bytes)]);
    assert_eq!(receipt["item"]["id"], "newest");
    assert_eq!(receipt["restarted"], true);
    assert_eq!(receipt["reclaimed"], json!(["older"]));
    assert_eq!(receipt["unreclaimed"], json!([]));
}

#[test]
fn task_detach_sends_one_exact_metadata_write_and_verifies_receipt() {
    use ds_command_kernel::project_management::task_attachment_detach::Request;
    let req = Request {
        task_id: "task-7".into(),
        attachment_id: "att-1".into(),
        expected_version: 3,
    };
    let response = json!({"project_id":"project-a","task_id":"task-7","attachment_id":"att-1","version":4,"changed":true,"detached":true,"item":{"id":"att-1","version":4,"task_detached":true,"task_detached_prior_version":3,"task_detached_by":"user@example.com","task_detached_at":"2026-10-13T00:00:00Z","data":{"linked_entity_type":"task","linked_entity_id":"task-7","state":"ready","generation":29}}});
    let profile = crate::data_distribution::tests::profile_for_tests();
    let mut transport = attach_transport("ready", 1);
    transport.started = response.clone();
    let invoke = |transport: &mut AttachTransport, request: Request| {
        run(
            &profile,
            transport,
            crate::authorized::Authority {
                bearer_token: "bearer",
                uid: "uid",
                email: "user@example.com",
                device_id: None,
            },
            "project-a",
            &Command::TaskDetach { request },
        )
    };
    assert_eq!(invoke(&mut transport, req.clone()).unwrap(), response);
    assert_eq!(transport.payloads, vec![req.payload("project-a")]);
    assert!(
        transport.uploaded.is_empty(),
        "detach has no storage upload"
    );
    for bad in [
        Request {
            expected_version: 0,
            ..req.clone()
        },
        Request {
            task_id: "other/task".into(),
            ..req.clone()
        },
    ] {
        let mut untouched = attach_transport("ready", 1);
        assert_eq!(
            invoke(&mut untouched, bad).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert!(untouched.payloads.is_empty() && untouched.uploaded.is_empty());
    }
    let mut foreign = attach_transport("ready", 1);
    foreign.started = response;
    foreign.started["project_id"] = json!("other-project");
    assert_eq!(
        invoke(&mut foreign, req).unwrap_err().kind(),
        ErrorKind::UnreadableResponse
    );
    assert_eq!(
        foreign.payloads.len(),
        1,
        "mismatched receipt is never retried"
    );
    assert!(foreign.uploaded.is_empty());
}

#[test]
fn project_overview_reads_exact_pair_and_never_turns_refusals_into_empty_work() {
    use ds_command_kernel::project_management::overview::Options;
    let mut transport = attach_transport("ready", 1);
    transport.started = json!({"graph":{"project_id":"project-a","revision":9,"schedule_revision":4,"tasks":[{"id":"inbox","data":{"title":"Inbox without geometry","placement":"inbox","type":"task"}},{"id":"gate","data":{"title":"Milestone","type":"milestone"}}],"dependencies":[],"residual_items":[]}});
    transport.finalized = json!({"context":{"project_id":"project-a","loaded_collections":["pm_records"],"records":[{"id":"record","version":2,"data":{"category":"instruction","subject":"Record"}},{"id":"note","version":7,"data":{"category":"project_note","subject":"Note without geometry"}}],"truncated_by_collection":{"pm_records":false},"next_cursors":{},"personal_notes":[{"body":"PRIVATE_SECRET"}]}});
    let profile = crate::data_distribution::tests::profile_for_tests();
    let invoke = |t: &mut AttachTransport, options: Options| {
        run(
            &profile,
            t,
            crate::authorized::Authority {
                bearer_token: "bearer",
                uid: "uid",
                email: "user@example.com",
                device_id: None,
            },
            "project-a",
            &Command::Overview { options },
        )
    };
    let result = invoke(&mut transport, Options::default()).unwrap();
    assert_eq!(
        transport.payloads,
        vec![
            json!({"action":"get_graph","ds_project":"project-a"}),
            json!({"action":"get_context","ds_project":"project-a","limit":100,"collections":["pm_records"]})
        ]
    );
    assert_eq!(result["tasks"]["count"], 1);
    assert_eq!(result["milestones"]["count"], 1);
    assert_eq!(result["records"]["count"], 1);
    assert_eq!(result["project_notes"]["rows"][0]["version"], 7);
    assert_eq!(result["source_authority"]["uid"], "uid");
    assert_eq!(result["source_authority"]["project_id"], "project-a");
    assert_eq!(result["source_authority"]["lane"], "stable");
    assert!(
        !result.to_string().contains("PRIVATE_SECRET") && !result.to_string().contains("bearer")
    );
    assert!(transport.uploaded.is_empty());
    transport.payloads.clear();
    assert_eq!(
        invoke(
            &mut transport,
            Options {
                limit: 0,
                ..Options::default()
            }
        )
        .unwrap_err()
        .kind(),
        ErrorKind::InvalidInput
    );
    assert!(transport.payloads.is_empty());
    transport.started["graph"]["project_id"] = json!("foreign");
    assert_eq!(
        invoke(&mut transport, Options::default())
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
    assert_eq!(
        transport.payloads.len(),
        1,
        "bad graph never fetches context"
    );
    transport.payloads.clear();
    transport.upload_status = 403;
    assert_eq!(
        invoke(&mut transport, Options::default())
            .unwrap_err()
            .kind(),
        ErrorKind::AuthenticationRejected
    );
    assert_eq!(
        transport.payloads.len(),
        1,
        "denied graph never fetches context"
    );
    transport.upload_status = 200;
    transport.started["graph"]["project_id"] = json!("project-a");
    transport.finalized["context"]["loaded_collections"] = json!(["pm_records", "pm_comments"]);
    transport.payloads.clear();
    assert_eq!(
        invoke(&mut transport, Options::default())
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
    assert_eq!(
        transport.payloads.len(),
        2,
        "unrequested acquisition is never retried"
    );
    transport.finalized["context"]["loaded_collections"] = json!(["pm_records"]);
    transport.finalized["context"]["records"] = Value::Null;
    transport.payloads.clear();
    assert_eq!(
        invoke(&mut transport, Options::default())
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
    assert_eq!(transport.payloads.len(), 2);
    transport.finalized["context"]["project_id"] = json!("foreign");
    transport.payloads.clear();
    assert_eq!(
        invoke(&mut transport, Options::default())
            .unwrap_err()
            .kind(),
        ErrorKind::UnreadableResponse
    );
    assert_eq!(transport.payloads.len(), 2);
}

#[test]
fn model_references_reads_one_records_page_with_same_authority_and_truthful_revisions() {
    let profile = crate::data_distribution::tests::profile_for_tests();
    let link = json!({"kind":"ds_object","object_type":"mv_model","project_id":"project-a","entity_id":"model-a","object_revision":"rev-a","model_version":3});
    let graph = json!({"graph":{"project_id":"project-a","revision":9,"schedule_revision":4,"tasks":[{"id":"task-a","data":{"title":"Model task","type":"task"},"links":[link.clone()]}],"dependencies":[],"residual_items":[],"calendar":{},"permissions":{"view":true,"comment":false,"contribute":false,"edit_schedule":false,"admin":false}}});
    let context = json!({"context":{"project_id":"project-a","loaded_collections":["pm_records"],"records":[{"id":"note-a","version":7,"metadata":{"updated_at":123},"data":{"category":"project_note","subject":"Canonical non-geometric note","memory":{"schema":"ds.project-note-memory/v1"}},"links":[link.clone()]},{"id":"other-category","version":2,"metadata":{"updated_at":120},"data":{"category":"instruction","subject":"Ignore"},"links":[link]}],"truncated_by_collection":{"pm_records":true},"next_cursors":{"pm_records":"boundary"}}});
    let invoke = |t: &mut AttachTransport, cursor: Option<&str>| {
        run(
            &profile,
            t,
            crate::authorized::Authority {
                bearer_token: "bearer",
                uid: "captured-uid",
                email: "reader@example.test",
                device_id: None,
            },
            "project-a",
            &Command::ModelReferences {
                model_id: "model-a".into(),
                revision_id: Some("rev-a".into()),
                records_cursor: cursor.map(str::to_owned),
            },
        )
    };
    let mut t = attach_transport("ready", 1);
    t.started = graph.clone();
    t.finalized = context.clone();
    let got = invoke(&mut t, None).unwrap();
    assert_eq!(
        t.payloads,
        vec![
            json!({"action":"get_graph","ds_project":"project-a"}),
            json!({"action":"get_context","ds_project":"project-a","limit":100,"collections":["pm_records"]})
        ]
    );
    assert_eq!(
        t.authorities,
        vec![("captured-uid".into(), "reader@example.test".into()); 2]
    );
    assert_eq!(got["tasks"].as_array().unwrap().len(), 1);
    assert_eq!(got["notes"].as_array().unwrap().len(), 1);
    assert_eq!(got["notes"][0]["note_version"], 7);
    assert_eq!(got["notes"][0]["note_updated_at"], 123);
    assert_eq!(got["notes_count_kind"], "lower_bound");
    assert_eq!(got["notes_next_cursor"], "boundary");
    assert_eq!(got["revision"], 9);
    assert_eq!(got["schedule_revision"], 4);
    assert!(got["context_graph_revision"].is_null());
    assert_eq!(
        got["read_consistency"],
        "separate_reads_unversioned_context"
    );
    assert!(!got.to_string().contains("bearer"));
    assert!(t.uploaded.is_empty());
    t.payloads.clear();
    t.finalized["context"]["truncated_by_collection"]["pm_records"] = json!(false);
    t.finalized["context"]["next_cursors"] = json!({});
    assert_eq!(invoke(&mut t, None).unwrap()["notes_count_kind"], "exact");
    t.payloads.clear();
    let got = invoke(&mut t, Some("boundary")).unwrap();
    assert_eq!(got["notes_count_kind"], "lower_bound");
    assert_eq!(t.payloads[1]["cursors"], json!({"pm_records":"boundary"}));
    for (pointer, bad) in [
        ("/context/project_id", json!("foreign")),
        (
            "/context/loaded_collections",
            json!(["pm_records", "pm_comments"]),
        ),
        ("/context/truncated_by_collection", json!({"records":false})),
        ("/context/records/0/version", Value::Null),
        ("/context/records/0/metadata/updated_at", Value::Null),
        ("/context/next_cursors", json!({"pm_comments":"cursor"})),
    ] {
        t.finalized = context.clone();
        *t.finalized.pointer_mut(pointer).unwrap() = bad;
        t.payloads.clear();
        assert_eq!(
            invoke(&mut t, None).unwrap_err().kind(),
            ErrorKind::UnreadableResponse,
            "{pointer}"
        );
        assert_eq!(t.payloads.len(), 2);
    }
    t.started["graph"]["project_id"] = json!("foreign");
    t.payloads.clear();
    assert_eq!(
        invoke(&mut t, None).unwrap_err().kind(),
        ErrorKind::UnreadableResponse
    );
    assert_eq!(t.payloads.len(), 1);
    t.started = graph;
    t.upload_status = 403;
    t.payloads.clear();
    assert_eq!(
        invoke(&mut t, None).unwrap_err().kind(),
        ErrorKind::AuthenticationRejected
    );
    assert_eq!(t.payloads.len(), 1);
    t.payloads.clear();
    assert_eq!(
        invoke(&mut t, Some("bad/cursor")).unwrap_err().kind(),
        ErrorKind::InvalidInput
    );
    assert!(t.payloads.is_empty());
}

#[test]
fn attachment_files_are_bounded_named_plainly_and_typed() {
    assert_eq!(
        AttachmentFile::new("Plan.XLSX", None, vec![1])
            .unwrap()
            .content_type(),
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
    );
    assert_eq!(
        AttachmentFile::new("survey.dat", None, vec![1])
            .unwrap()
            .content_type(),
        "application/octet-stream"
    );
    assert_eq!(
        AttachmentFile::new("a.bin", Some("Image/PNG"), vec![1])
            .unwrap()
            .content_type(),
        "image/png"
    );
    for bad in ["", "  ", "../a.pdf", "a\\b.pdf", ".", "..", "a\u{0}.pdf"] {
        assert!(AttachmentFile::new(bad, None, vec![1]).is_err(), "{bad:?}");
    }
    assert!(AttachmentFile::new(&"n".repeat(256), None, vec![1]).is_err());
    assert!(AttachmentFile::new("a.pdf", None, Vec::new()).is_err());
    assert!(AttachmentFile::new("a.pdf", None, vec![0; MAX_ATTACHMENT_BYTES + 1]).is_err());
    for bad in ["pdf", "a/b/c", "text/plain; charset=utf-8", "a /b"] {
        assert!(
            AttachmentFile::new("a.pdf", Some(bad), vec![1]).is_err(),
            "{bad:?}"
        );
    }
    let debug = format!(
        "{:?}",
        AttachmentFile::new("a.pdf", None, vec![7; 4]).unwrap()
    );
    assert!(debug.contains("bytes: 4"), "{debug}");
    assert!(Command::TaskAttach {
        task_id: "a/b".into(),
        file: AttachmentFile::new("a.pdf", None, vec![1]).unwrap()
    }
    .validate()
    .is_err());
}
