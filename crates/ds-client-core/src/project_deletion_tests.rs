use super::*;

const DIGEST: &str = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const SHA: &str = "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";

fn project() -> String {
    "zz_delete_fixture".into()
}

#[test]
fn every_action_sends_exactly_the_keys_the_handler_reads() {
    let cases = [
        (
            Command::Plan {
                project_id: project(),
            },
            json!({"action":"delete_plan","data":{"project_id":"zz_delete_fixture"}}),
        ),
        (
            Command::Export {
                project_id: project(),
                source: "objects".into(),
                cursor: Some("3:a".into()),
            },
            json!({"action":"delete_export","data":{"project_id":"zz_delete_fixture","source":"objects","cursor":"3:a"}}),
        ),
        (
            Command::Export {
                project_id: project(),
                source: "firestore".into(),
                cursor: None,
            },
            json!({"action":"delete_export","data":{"project_id":"zz_delete_fixture","source":"firestore"}}),
        ),
        (
            Command::ArchiveOpen {
                project_id: project(),
                plan_digest: DIGEST.into(),
                size: 9,
                sha256: SHA.into(),
                crc32c: "AAAAAA==".into(),
                index_sha256: SHA.into(),
            },
            json!({"action":"delete_archive_open","data":{"project_id":"zz_delete_fixture","plan_digest":DIGEST,
                "size":9,"sha256":SHA,"crc32c":"AAAAAA==","index_sha256":SHA}}),
        ),
        (
            Command::ArchiveReceipt {
                project_id: project(),
                plan_digest: DIGEST.into(),
                generation: 7,
                sha256: SHA.into(),
            },
            json!({"action":"delete_archive_receipt","data":{"project_id":"zz_delete_fixture","plan_digest":DIGEST,"generation":7,"sha256":SHA}}),
        ),
        (
            Command::Apply {
                project_id: project(),
                plan_digest: DIGEST.into(),
            },
            json!({"action":"delete_apply","data":{"project_id":"zz_delete_fixture","plan_digest":DIGEST,"confirm_delete":true}}),
        ),
    ];
    for (command, body) in cases {
        command.validate().expect("valid command");
        assert_eq!(command.payload(), body);
    }
}

#[test]
fn malformed_commands_never_leave() {
    let bad = [
        Command::Plan {
            project_id: "a/b".into(),
        },
        Command::Plan {
            project_id: String::new(),
        },
        Command::Export {
            project_id: project(),
            source: "bigquery".into(),
            cursor: None,
        },
        Command::Apply {
            project_id: project(),
            plan_digest: "sha256:XYZ".into(),
        },
        Command::ArchiveReceipt {
            project_id: project(),
            plan_digest: DIGEST.into(),
            generation: 0,
            sha256: SHA.into(),
        },
        Command::ArchiveOpen {
            project_id: project(),
            plan_digest: DIGEST.into(),
            size: 0,
            sha256: SHA.into(),
            crc32c: "AAAAAA==".into(),
            index_sha256: SHA.into(),
        },
    ];
    for command in bad {
        assert_eq!(
            command.validate().unwrap_err().kind(),
            ErrorKind::InvalidInput,
            "{command:?}"
        );
    }
}

#[test]
fn refusals_carry_the_route_code_and_sentence() {
    let body = serde_json::to_vec(&json!({
        "success": false,
        "error": {"code": "project_delete_plan_changed", "message": "the project changed", "details": {"plan_digest": DIGEST}},
    }))
    .unwrap();
    let error = refusal(409, &body);
    assert_eq!(error.kind(), ErrorKind::InvalidInput);
    let carried = error.service_refusal().expect("route refusal");
    assert_eq!(carried.code(), Some("project_delete_plan_changed"));
    assert_eq!(carried.message(), Some("the project changed"));

    let forbidden = refusal(
        403,
        br#"{"success":false,"error":{"code":"project_delete_forbidden","message":"owner only"}}"#,
    );
    assert_eq!(forbidden.kind(), ErrorKind::AuthenticationRejected);
    let missing = refusal(
        404,
        br#"{"success":false,"error":{"code":"project_delete_not_found","message":"gone"}}"#,
    );
    assert_eq!(missing.kind(), ErrorKind::ResourceNotFound);
    assert_eq!(
        refusal(404, b"not json").kind(),
        ErrorKind::RouteUnavailable
    );
    assert_eq!(refusal(503, b"{}").kind(), ErrorKind::Transient);
}

#[test]
fn only_a_success_envelope_decodes() {
    let data = decode(br#"{"success":true,"data":{"plan":{"plan_digest":"x"}}}"#).unwrap();
    assert_eq!(data["plan"]["plan_digest"], "x");
    assert!(decode(br#"{"success":false,"data":{}}"#).is_err());
    assert!(decode(b"[]").is_err());
}
