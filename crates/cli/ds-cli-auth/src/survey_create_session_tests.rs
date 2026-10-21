use super::*;

#[test]
fn standalone_device_creation_is_not_retryable_and_points_at_the_entry_queue() {
    let failure = map_survey_entry_create_service_code(
        SurveyEntryCreateServiceCode::FirebaseSessionRequired,
    );
    assert_eq!(failure.code(), "survey_create_firebase_session_required");
    assert!(!failure.class().retryable());
    assert_eq!(failure.next_commands(), ["ds core survey queue --help"]);
    assert!(!failure.next_commands().iter().any(|next| next.contains("feedback")));
}
