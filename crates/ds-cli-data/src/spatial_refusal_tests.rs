use super::*;

fn brain_409(service_code: &str) -> Failure {
    Failure::invalid(
        "auth_input_invalid",
        "Data distribution refused by ds-brain (HTTP 409)",
    )
    .detail(json!({"http_status": 409, "service_code": service_code, "service_message": "plan again"}))
}

#[test]
fn a_tampered_plan_hash_is_spatial_plan_changed_not_auth_input_invalid() {
    let mapped = spatial_service_refusal(brain_409("spatial_plan_changed"));
    assert_eq!(mapped.code(), "spatial_plan_changed");
    assert_eq!(mapped.class(), ds_cli_contract::ExitClass::InvalidInput);
    assert!(!mapped.class().retryable());
    assert_eq!(mapped.remedy_text(), Some(PLAN_CHANGED.remedy));
    assert_eq!(mapped.detail_value().unwrap()["http_status"], 409);
}

#[test]
fn a_changed_source_is_spatial_source_changed() {
    let mapped = spatial_service_refusal(brain_409("spatial_source_changed"));
    assert_eq!(mapped.code(), "spatial_source_changed");
}

#[test]
fn every_other_refusal_passes_through_untouched() {
    let other = spatial_service_refusal(brain_409("something_else"));
    assert_eq!(other.code(), "auth_input_invalid");
    let bare = spatial_service_refusal(Failure::invalid("auth_input_invalid", "x"));
    assert_eq!(bare.code(), "auth_input_invalid");
}
