use super::*;
#[test]
fn collaboration_commands_declare_their_response_and_page_refusals() {
    for command in [&PROJECTS, &COMMENT, &COMMENTS] {
        assert!(
            command
                .refusals
                .iter()
                .any(|refusal| refusal.code == unreadable().code()
                    && refusal.remedy == UNREADABLE.remedy)
        );
    }
    for command in [&PROJECTS, &COMMENTS] {
        assert!(
            command
                .refusals
                .iter()
                .any(|refusal| refusal.code == PAGE_LIMIT.code
                    && refusal.remedy == PAGE_LIMIT.remedy)
        );
    }
}
#[test]
fn public_project_page_must_match_status_and_bounds() {
    let page = json!({"status":"active","next_cursor":"next","projects":[{"id":"p","name":"Project","lifecycle_state":"active"}]});
    validate_projects(&page, "active", 1).unwrap();
    assert!(validate_projects(&page, "archived", 1).is_err());
    assert!(validate_projects(&page, "active", 0).is_err());
    assert!(validate_projects(&json!({"projects":[]}), "active", 50).is_err());
}
