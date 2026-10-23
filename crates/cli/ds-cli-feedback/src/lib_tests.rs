use super::*;

#[test]
fn every_feedback_endpoint_command_declares_service_and_existing_auth_refusals() {
    for command in [
        &list::COMMAND,
        &submit::COMMAND,
        &note::COMMAND,
        &close::COMMAND,
    ] {
        let refusal = command
            .refusals
            .iter()
            .find(|refusal| refusal.code == ds_cli_auth::FEEDBACK_SERVICE_UNAVAILABLE_REFUSAL.code)
            .expect("feedback service refusal is discoverable");
        assert_eq!(
            refusal.remedy,
            ds_cli_auth::FEEDBACK_SERVICE_UNAVAILABLE_REFUSAL.remedy
        );
        for existing in ds_cli_auth::PROJECT_STATUS_COMMAND.refusals {
            assert!(
                command
                    .refusals
                    .iter()
                    .any(|refusal| refusal.code == existing.code)
            );
        }
    }
    assert!(
        !draft::COMMAND.refusals.iter().any(|refusal| {
            refusal.code == ds_cli_auth::FEEDBACK_SERVICE_UNAVAILABLE_REFUSAL.code
        }),
        "local drafts never call the feedback endpoint"
    );
}
