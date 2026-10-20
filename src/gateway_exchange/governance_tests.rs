use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn request<C: Exchange>(call: C) -> Request {
    match start(call) {
        Step::Fetch { request, .. } => request,
        _ => panic!("expected a fetch"),
    }
}

fn answered<C: Exchange>(call: C, body: Value) -> Result<C::Output, Refusal> {
    let Step::Fetch { continuation, .. } = start(call) else {
        panic!("expected a fetch")
    };
    let observed = Observed::Response {
        status: 200,
        content_type: Some("application/json".into()),
        retry_after: None,
    };
    match resume(
        continuation,
        observed,
        body.to_string().as_bytes(),
        &Facts::default(),
    ) {
        Step::Done(value) => Ok(value),
        Step::Refused(refusal) => Err(refusal),
        _ => panic!("expected an outcome"),
    }
}

#[test]
fn every_change_names_its_action_and_one_account() {
    let page = request(PageData);
    assert_eq!(page.path, PATH);
    assert_eq!(page.body, Some(json!({"action": "page_data", "data": {}})));
    assert_eq!(page.response_cap_bytes, PAGE_CAP);

    let roles = request(SetAppRoles {
        email: "ana@example.rw".into(),
        roles: vec!["viewer".into()],
    });
    assert_eq!(
        roles.body,
        Some(
            json!({"action": "set_app_roles", "data": {"email": "ana@example.rw", "roles": ["viewer"]}})
        )
    );

    let removal = request(RemoveFromAllProjects {
        email: "ana@example.rw".into(),
    });
    assert_eq!(removal.timeout_ms, REMOVAL_TIMEOUT_MS);

    let access = request(SetAccessState {
        email: "ana@example.rw".into(),
        account_status: "suspended".into(),
        reason: "left the company".into(),
    });
    assert_eq!(
        access.body,
        Some(json!({"action": "set_user_access_state", "data": {
            "email": "ana@example.rw", "account_status": "suspended", "reason": "left the company"}}))
    );
}

#[test]
fn the_page_reads_what_brain_writes_including_null_lists() {
    let page = answered(
        PageData,
        json!({"success": true, "data": {
            "app_role_defs": [{"name": "admin", "description": "Everything", "capabilities": ["all"],
                "ui_access": ["governance"], "rank": 100, "grantable_by": null}],
            "project_role_defs": [{"name": "member", "description": "", "capabilities": null,
                "rank": 10, "grantable_by": ["admin"]}],
            "users": [{"email": "ana@example.rw", "display_name": "Ana", "roles": null,
                "last_seen": "2026-10-01T08:00:00Z", "login_count": 3, "is_active": true,
                "account_status": "active"}],
            "apps": []
        }}),
    )
    .expect("the page admits");
    assert_eq!(
        page.app_role_defs[0].ui_access,
        Some(vec!["governance".into()])
    );
    assert!(page.app_role_defs[0].grantable_by.is_empty());
    assert!(page.project_role_defs[0].capabilities.is_empty());
    assert_eq!(page.project_role_defs[0].ui_access, None);
    assert_eq!(page.users[0].display_name.as_deref(), Some("Ana"));
    assert!(page.users[0].roles.is_empty());
    assert_eq!(page.users[0].last_sign_in, None);
}

#[test]
fn a_reader_gets_no_registry_and_a_broken_page_is_refused() {
    let page = answered(
        PageData,
        json!({"data": {"app_role_defs": [], "project_role_defs": [], "users": null}}),
    )
    .expect("a reader's page admits");
    assert!(page.users.is_empty());

    let refused = answered(PageData, json!({"data": {"app_role_defs": "nope"}}))
        .expect_err("a malformed page is refused");
    assert_eq!(refused.code, INVALID);
}

#[test]
fn a_change_answers_what_brain_changed() {
    let removed = answered(
        RemoveFromAllProjects {
            email: "ana@example.rw".into(),
        },
        json!({"data": {"email": "ana@example.rw", "matched_project_count": 2,
            "removed_project_count": 2, "project_ids": ["kigali", "huye"]}}),
    )
    .expect("the removal admits");
    assert_eq!(removed.removed_project_count, 2);
    assert_eq!(removed.project_ids, ["kigali", "huye"]);

    let access = answered(
        SetAccessState {
            email: "ana@example.rw".into(),
            account_status: "blocked".into(),
            reason: String::new(),
        },
        json!({"data": {"email": "ana@example.rw", "account_status": "blocked", "is_active": false}}),
    )
    .expect("the access state admits");
    assert_eq!(access.account_status, "blocked");
    assert!(!access.is_active);
}

#[cfg(feature = "durable-store")]
mod held_page {
    use super::super::{held, refresh};
    use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
    use serde_json::json;

    fn page(users: &[&str]) -> serde_json::Value {
        let users: Vec<_> = users
            .iter()
            .map(|email| {
                json!({"email": email, "roles": ["viewer"], "login_count": 1,
                "is_active": true, "account_status": "active"})
            })
            .collect();
        json!({"data": {"app_role_defs": [], "project_role_defs": [], "users": users}})
    }

    #[test]
    fn a_refreshed_page_is_held_for_the_next_open_without_a_request() {
        let edge = TestEdge::new();
        assert_eq!(block_on(held(&edge)).expect("held reads"), None);
        edge.json(page(&["ana@example.rw"]));
        let fresh = block_on(refresh(&edge)).expect("the page refreshes");
        assert_eq!(fresh.users[0].email, "ana@example.rw");
        assert_eq!(block_on(held(&edge)).expect("held reads"), Some(fresh));
        assert_eq!(edge.requests().len(), 1);
    }

    #[test]
    fn offline_the_held_page_stays_and_another_account_sees_none_of_it() {
        let edge = TestEdge::new();
        edge.json(page(&["ana@example.rw"]));
        let fresh = block_on(refresh(&edge)).expect("the page refreshes");
        edge.online.set(false);
        assert!(block_on(refresh(&edge)).is_err());
        assert_eq!(block_on(held(&edge)).expect("held reads"), Some(fresh));
        edge.sign_in("someone-else");
        assert_eq!(block_on(held(&edge)).expect("held reads"), None);
    }
}
