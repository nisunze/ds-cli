use super::*;
use ds_cli_contract::ExitClass;

fn receipt() -> Receipt {
    serde_json::from_value(json!({
        "schema": "ds-account-deletion/v1", "receipt_id": "r-1", "state": "deleted",
        "already_deleted": false, "started_at": "2026-10-24T09:00:00Z",
        "deleted_at": "2026-10-24T09:00:02Z",
        "deleted": {"profile_documents": 8, "project_memberships": 2, "storage_objects": 4,
                    "device_credentials": 2, "messaging_documents": 0, "sign_in_account": true},
        "retained": ["project_records", "audit_records"]
    }))
    .unwrap()
}

fn service_error(status: u16, body: Value) -> ClientError {
    ds_client_core::account_deletion::refusal(status, &serde_json::to_vec(&body).unwrap())
}

#[test]
fn the_command_is_a_confirmed_global_write_on_the_lane_alone() {
    assert_eq!(DELETE_COMMAND.id, "account.delete");
    assert_eq!(DELETE_COMMAND.effect, Effect::GlobalWrite);
    assert!(DELETE_COMMAND.requires_confirmation());
    assert_eq!(
        DELETE_COMMAND
            .args
            .iter()
            .map(|a| a.name)
            .collect::<Vec<_>>(),
        ["lane"]
    );
    assert!(DELETE_COMMAND.summary.chars().count() <= 70);
    let inputs = ds_cli_contract::parse(&DELETE_COMMAND, &[]).unwrap();
    assert!(DELETE_COMMAND.confirmation_required_for(&inputs));
}

#[test]
fn each_service_refusal_maps_to_its_declared_code_and_class() {
    let cases = [
        (
            500,
            "account_delete_incomplete",
            "account_delete_incomplete",
            ExitClass::Unavailable,
        ),
        (
            503,
            "account_delete_unavailable",
            "account_delete_unavailable",
            ExitClass::Unavailable,
        ),
        (
            403,
            "account_delete_identity_invalid",
            "account_delete_identity_invalid",
            ExitClass::Unauthorized,
        ),
        (
            400,
            "account_delete_confirmation_required",
            "account_delete_request_invalid",
            ExitClass::InvalidInput,
        ),
        (
            400,
            "account_delete_request_invalid",
            "account_delete_request_invalid",
            ExitClass::InvalidInput,
        ),
        (
            401,
            "UNAUTHORIZED",
            "account_delete_not_accepted",
            ExitClass::Unauthorized,
        ),
    ];
    for (status, service, code, class) in cases {
        let failure = map(service_error(
            status,
            json!({"success": false, "error": {"code": service, "message": "m",
                   "details": {"step": "profile", "retryable": true}}}),
        ));
        assert_eq!(
            (failure.code(), failure.class()),
            (code, class),
            "{service}"
        );
        assert!(REFUSALS.iter().any(|r| r.code == failure.code()));
    }
    let incomplete = map(service_error(
        500,
        json!({"success": false, "error": {"code": "account_delete_incomplete",
               "details": {"step": "storage_objects"}}}),
    ));
    assert_eq!(
        incomplete.detail_value().unwrap()["step"],
        "storage_objects"
    );
    assert_eq!(incomplete.remedy_text(), Some(INCOMPLETE.remedy));

    // A gateway that was never taught the route: no code, a 404.
    let unrouted = map(service_error(404, json!({"message": "not defined"})));
    assert_eq!(unrouted.code(), "account_delete_route_unavailable");
}

#[test]
fn the_answer_carries_the_receipt_and_names_a_logout_when_local_cleanup_failed() {
    let done = answer(Lane::Stable, &receipt(), "ds_device", true);
    assert_eq!(done["receipt"]["receipt_id"], "r-1");
    assert_eq!(done["local_credentials_forgotten"], true);
    assert!(done.get("next").is_none());
    let text = render_delete(&done);
    assert!(text.contains("account deleted (stable)  receipt r-1"));
    assert!(text.contains("8 profile records, 2 memberships, 4 files, 2 devices"));

    let partial = answer(Lane::Canary, &receipt(), "firebase", false);
    assert_eq!(partial["next"], "ds auth logout --lane canary");
    assert!(render_delete(&partial).contains("run: ds auth logout --lane canary"));
}
