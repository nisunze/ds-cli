//! A project_settings row a project may author before it exists must resolve
//! through its owner, and a malformed one never saves.
use super::*;
use serde_json::json;

#[test]
fn the_context_tolerance_is_authored_before_its_row_exists_and_bounded() {
    let original =
        json!({"sheets":{"project_settings":[{"parameter":"report_locale","value":"fr"}]}});
    let updated = set_parameter(
        &original,
        "project_settings",
        crate::project_dataset_cache::CONTEXT_TOLERANCE_PARAMETER,
        &json!("350"),
    )
    .unwrap();
    assert_eq!(
        crate::project_dataset_cache::context_tolerance_m(&json!({"project_settings": updated}))
            .unwrap(),
        350.0
    );
    // The held input is not changed by authoring.
    assert_eq!(
        original["sheets"]["project_settings"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    for raw in ["-5", "5001", "wide"] {
        assert_eq!(
            set_parameter(
                &original,
                "project_settings",
                crate::project_dataset_cache::CONTEXT_TOLERANCE_PARAMETER,
                &json!(raw),
            )
            .unwrap_err(),
            "config_parameter_invalid",
            "{raw}"
        );
    }
    // Only the published default is admitted before a row exists.
    assert!(set_parameter(
        &original,
        "project_settings",
        "project_context_buffer_m",
        &json!("350")
    )
    .is_err());
}
