use ds_command_kernel::domain_value::{
    DomainValueInput, DomainValueResponse, evaluate, validate_response,
};
use serde_json::json;

#[test]
fn native_application_caller_and_handler_share_domain_options() {
    let request: DomainValueInput = serde_json::from_value(json!({"action":"options", "domain":{"type":"categorical_number","values":[50,100],"unit":"kVA"}})).unwrap();
    let result = evaluate(request.clone()).unwrap();
    let decoded = serde_json::from_slice(&serde_json::to_vec(&result).unwrap()).unwrap();
    let result = validate_response(&request, decoded).unwrap();
    let DomainValueResponse::Options(options) = result else {
        panic!("wrong response variant")
    };
    assert_eq!(options[0].value, "50");
    assert_eq!(options[0].label, "50 kVA");
}

#[test]
fn a_response_from_another_operation_is_not_accepted_as_options() {
    let request: DomainValueInput = serde_json::from_value(json!({"action":"options"})).unwrap();
    let response: DomainValueResponse =
        serde_json::from_value(json!({"action":"format","value":"wrong operation"})).unwrap();
    assert!(
        validate_response(&request, response)
            .unwrap_err()
            .to_string()
            .contains("answer belongs to a different operation")
    );
}
