use super::*;
use crate::gateway_exchange::Exchange;

#[test]
fn save_acknowledgements_are_distinct_from_read_documents_and_keep_compatible_metadata() {
    for value in [
        json!({"message":"Updated project_settings","records":1}),
        json!({"success":true}),
        json!({"message":"Updated project_settings","records":1,"future":true}),
    ] {
        admit_project_configuration_save(&value).unwrap();
        assert!(admit_project_configuration_document(value).is_err());
    }
}

#[test]
fn real_design_save_handler_admits_the_same_acknowledgement_and_refuses_incompatible_replies() {
    let call = || DesignCall::SaveConfigSheet {
        project: "project_a".into(),
        sheet: "project_settings".into(),
        rows: json!([]),
        expected: None,
    };
    let reply = json!({"message":"Updated project_settings","records":0,"future":true});
    assert!(matches!(call().admit(200,reply.clone()),Ok(Admitted::Done(value)) if value==reply));
    for invalid in [
        json!([]),
        json!(42),
        json!(null),
        json!({"success":false}),
        json!({"success":"yes"}),
        json!({"success":false,"data":{"message":"Updated project_settings","records":0}}),
    ] {
        assert!(call().admit(200, invalid).is_err());
    }
}
