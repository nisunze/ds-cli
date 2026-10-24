use super::*;

#[derive(Clone, Copy)]
enum InvalidRequest {
    ProjectWork,
    ProjectWorkCommand,
    Correspondence,
    CorrespondenceCommand,
    MemberGrant,
    MemberProject,
    MemberPlan,
}

fn rejected_request(
    client: &mut Client<FixtureTransport, MemoryStore>,
    request: InvalidRequest,
    now: u64,
) -> Result<(), ds_client_core::ClientError> {
    match request {
        InvalidRequest::ProjectWork => client
            .project_management("", &ds_client_core::project_management::Command::Graph, now)
            .map(|_| ()),
        InvalidRequest::ProjectWorkCommand => client
            .project_management(
                "",
                &ds_client_core::project_management::Command::PublicProjects {
                    status: "active".into(),
                    cursor: String::new(),
                    limit: 0,
                },
                now,
            )
            .map(|_| ()),
        InvalidRequest::Correspondence => client
            .project_correspondence(
                "",
                &ds_client_core::project_correspondence::Action::PartyList(Default::default()),
                now,
            )
            .map(|_| ()),
        InvalidRequest::CorrespondenceCommand => client
            .project_correspondence(
                "project-a",
                &ds_client_core::project_correspondence::Action::PartyList(
                    ds_client_core::project_correspondence::PartyFilters {
                        limit: Some(0),
                        ..Default::default()
                    },
                ),
                now,
            )
            .map(|_| ()),
        InvalidRequest::MemberGrant => client
            .member_form_grant(
                "project-a",
                &ds_client_core::member_form_grants::Request::Read {
                    member: "INVALID@example.com".into(),
                },
                now,
            )
            .map(|_| ()),
        InvalidRequest::MemberProject => client
            .member_form_grant(
                &"a".repeat(129),
                &ds_client_core::member_form_grants::Request::Read {
                    member: "member@example.com".into(),
                },
                now,
            )
            .map(|_| ()),
        InvalidRequest::MemberPlan => client
            .member_form_grant(
                "project-a",
                &ds_client_core::member_form_grants::Request::Plan {
                    member: "member@example.com".into(),
                    forms: vec![],
                    all: false,
                },
                now,
            )
            .map(|_| ()),
    }
}

fn local_rejection_needs_no_refresh(request: InvalidRequest) {
    let mut current = signed_in(
        FixtureTransport::with_sign_in(SIGN_IN),
        MemoryStore::default(),
    );
    let expected = rejected_request(&mut current, request, NOW).unwrap_err();
    assert_eq!(expected.kind(), ErrorKind::InvalidInput);

    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_refresh(200, REFRESH);
    let store = MemoryStore::default();
    let mut expired = signed_in(transport.clone(), store.clone());
    let before = {
        let state = store.0.lock().unwrap();
        (
            state.value.clone(),
            state.writes.clone(),
            state.keys.clone(),
            state.leased,
        )
    };
    let actual = rejected_request(&mut expired, request, NOW + 4000).unwrap_err();
    assert_eq!(actual.kind(), expected.kind());
    assert_eq!(actual.to_string(), expected.to_string());
    assert_eq!(transport.count_calls("refresh "), 0);
    assert_eq!(transport.0.lock().unwrap().refresh.len(), 1);
    let state = store.0.lock().unwrap();
    assert_eq!(
        (
            state.value.clone(),
            state.writes.clone(),
            state.keys.clone(),
            state.leased
        ),
        before,
    );
}

#[test]
fn invalid_project_work_does_not_refresh_expired_authority() {
    local_rejection_needs_no_refresh(InvalidRequest::ProjectWork);
    local_rejection_needs_no_refresh(InvalidRequest::ProjectWorkCommand);
}

#[test]
fn invalid_correspondence_does_not_refresh_expired_authority() {
    local_rejection_needs_no_refresh(InvalidRequest::Correspondence);
    local_rejection_needs_no_refresh(InvalidRequest::CorrespondenceCommand);
}

#[test]
fn invalid_member_grant_does_not_refresh_expired_authority() {
    local_rejection_needs_no_refresh(InvalidRequest::MemberGrant);
    local_rejection_needs_no_refresh(InvalidRequest::MemberProject);
    local_rejection_needs_no_refresh(InvalidRequest::MemberPlan);
}

#[test]
fn accepted_project_shapes_still_refresh_expired_authority() {
    for case in 0..3 {
        let transport = FixtureTransport::with_sign_in(SIGN_IN);
        transport.push_refresh(200, REFRESH);
        let mut client = signed_in(transport.clone(), MemoryStore::default());
        let result = match case {
            0 => client.project_management(
                "  project-a  ",
                &ds_client_core::project_management::Command::Graph,
                NOW + 4000,
            ),
            1 => client.project_management(
                "",
                &ds_client_core::project_management::Command::PublicProjects {
                    status: "active".into(),
                    cursor: String::new(),
                    limit: 1,
                },
                NOW + 4000,
            ),
            _ => client.project_correspondence(
                "  project-a  ",
                &ds_client_core::project_correspondence::Action::PartyList(Default::default()),
                NOW + 4000,
            ),
        };
        // This transport omits the PM handler; accepted inputs reach it after
        // refreshing, rather than acquiring a new local project restriction.
        assert!(result
            .as_ref()
            .err()
            .is_none_or(|error| error.kind() != ErrorKind::InvalidInput));
        assert_eq!(transport.count_calls("refresh "), 1);
    }
}

#[test]
fn a_structural_grant_plan_still_checks_its_actor_after_refresh() {
    use ds_client_core::member_form_grants::{EffectiveForm, Plan, Request};
    let plan = Plan {
        schema: 1,
        operation: "set".into(),
        project_id: "project-a".into(),
        target_user_email: "member@example.com".into(),
        actor: "other@example.com".into(),
        base_version: 0,
        roles: vec![],
        before_forms: vec![],
        after_forms: vec!["pole".into()],
        bound_forms: vec!["pole".into()],
        snapshot: "a".repeat(64),
        hash: "b".repeat(64),
        effective_before: vec![EffectiveForm {
            form: "pole".into(),
            read_allowed: false,
            only_own: false,
            edit_allowed: false,
        }],
        effective_after: vec![EffectiveForm {
            form: "pole".into(),
            read_allowed: true,
            only_own: false,
            edit_allowed: true,
        }],
    };
    let request = Request::Apply {
        member: plan.target_user_email.clone(),
        plan,
    };
    request.validate("project-a").unwrap();
    let transport = FixtureTransport::with_sign_in(SIGN_IN);
    transport.push_refresh(200, REFRESH);
    let mut client = signed_in(transport.clone(), MemoryStore::default());
    assert_eq!(
        client
            .member_form_grant("project-a", &request, NOW + 4000)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(transport.count_calls("refresh "), 1);
}
