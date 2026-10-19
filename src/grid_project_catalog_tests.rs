use super::*;

pub(crate) fn scope(project: &str) -> Scope {
    Scope {
        principal: "owner".into(),
        lane: "canary".into(),
        audience: "a".repeat(64),
        project: project.into(),
    }
}
pub(crate) fn head(model: &str, revision: &str, name: &str) -> Value {
    json!({"model_id":model,"project_id":"template","display_name":name,"model_kind":"mv_line","lifecycle":"promoted","state":"active",
            "head_revision_id":revision,"head_version":1,"head_model_digest":"a".repeat(64),"updated_at":"2026-10-18T07:07:51Z","backup":{"ignored":true}})
}
fn listing(models: Vec<Value>) -> Value {
    json!({"project":"template","models":models.iter().map(head_projection).collect::<Vec<_>>(),"more":false,"next_cursor":""})
}

#[test]
fn listing_admission_is_canonical_and_refuses_incomplete_mv_heads() {
    let (rows, basis) = admit_listing(
        "template",
        &listing(vec![head("m-2", "r2", "B"), head("m-1", "r1", "A")]),
    )
    .unwrap();
    assert_eq!(
        rows.iter().map(|r| r.model_id.as_str()).collect::<Vec<_>>(),
        ["m-1", "m-2"]
    );
    assert!(!rows[0].head.contains("backup"));
    let (_, same) = admit_listing(
        "template",
        &listing(vec![head("m-1", "r1", "A"), head("m-2", "r2", "B")]),
    )
    .unwrap();
    assert_eq!(
        basis, same,
        "order of arrival never changes the source basis"
    );
    let mut bad = head("m-3", "r3", "C");
    bad["head_model_digest"] = json!("missing");
    assert!(admit_listing("template", &listing(vec![bad])).is_err());
    let mut more = listing(vec![]);
    more["more"] = json!(true);
    assert!(admit_listing("template", &more).is_err());
    assert!(admit_listing("other", &listing(vec![])).is_err());
}

#[test]
fn pages_walk_completely_and_refuse_cycles_crossed_projects_and_duplicates() {
    let page = |rows: Vec<Value>, more: bool, cursor: &str| json!({"success":true,"data":{"models":rows,"has_more":more,"next_cursor":cursor}});
    let walk = Walk::new("template").unwrap();
    assert_eq!(
        walk.body(),
        json!({"action":"list_models","project_id":"template","page_size":100,"cursor":"","include_deleted":false})
    );
    let first = decode_page("template", 200, &page(vec![], true, "next")).unwrap();
    let Walked::More(walk) = walk.accept(first).unwrap() else {
        panic!()
    };
    assert_eq!(walk.cursor(), Some("next"));
    let second = decode_page(
        "template",
        200,
        &page(vec![head("m-1", "r", "A")], false, ""),
    )
    .unwrap();
    let Walked::Complete(listing) = walk.accept(second).unwrap() else {
        panic!()
    };
    assert_eq!(listing["models"][0]["model_id"], "m-1");
    assert!(listing["models"][0].get("backup").is_none());
    assert_eq!(
        decode_page("template", 401, &json!({})),
        Err(PageError::Unauthorized)
    );
    assert_eq!(
        decode_page("template", 404, &json!({})),
        Err(PageError::Failed)
    );
    assert_eq!(
        decode_page("other", 200, &page(vec![head("m-1", "r", "A")], false, "")),
        Err(PageError::Invalid)
    );
    let walk = Walk::new("template").unwrap();
    let Walked::More(walk) = walk
        .accept(decode_page("template", 200, &page(vec![], true, "same")).unwrap())
        .unwrap()
    else {
        panic!()
    };
    assert!(walk
        .accept(decode_page("template", 200, &page(vec![], true, "same")).unwrap())
        .is_err());
    let walk = Walk::new("template").unwrap();
    let Walked::More(walk) = walk
        .accept(
            decode_page(
                "template",
                200,
                &page(vec![head("m-1", "r", "A")], true, "c"),
            )
            .unwrap(),
        )
        .unwrap()
    else {
        panic!()
    };
    assert!(walk
        .accept(
            decode_page(
                "template",
                200,
                &page(vec![head("m-1", "r", "A")], false, "")
            )
            .unwrap()
        )
        .is_err());
}

#[test]
fn view_serves_only_observed_unrevoked_rows_in_owner_order() {
    let held = |revoked: bool, basis: Option<&str>| Held {
        observation: Observation {
            basis_sha256: basis.map(str::to_owned),
            observed_at_ms: basis.map(|_| 5),
            needs_refresh: false,
            last_error: None,
            revoked,
            revision: 2,
        },
        heads: vec![
            HeldHead {
                head: head_projection(&head("m-2", "r2", "beta")),
                first_seen_at_ms: 7,
                retained_version: None,
            },
            HeldHead {
                head: head_projection(&head("m-1", "r1", "Alpha")),
                first_seen_at_ms: 3,
                retained_version: Some(
                    json!({"model_id":"m-1","revision_id":"r1","version":1,"model":{"digest":format!("sha256:{}", "a".repeat(64)),"byte_length":9}}),
                ),
            },
        ],
    };
    let basis = "b".repeat(64);
    let ready = view(
        "template",
        Some(&held(false, Some(&basis))),
        Source::Held,
        None,
        None,
    )
    .unwrap();
    assert_eq!(ready["observation"]["state"], "ready");
    assert_eq!(ready["mv_models"][0]["label"], "Alpha");
    assert_eq!(ready["mv_models"][0]["retained_current"], true);
    assert_eq!(ready["mv_models"][1]["first_seen_at_ms"], 7);
    assert_eq!(ready["heads"].as_array().unwrap().len(), 2);
    let failed = view(
        "template",
        Some(&held(false, Some(&basis))),
        Source::Held,
        Some(REFRESH_FAILED),
        None,
    )
    .unwrap();
    assert_eq!(failed["observation"]["state"], "held");
    assert_eq!(
        failed["mv_models"].as_array().unwrap().len(),
        2,
        "a failed refresh keeps rows"
    );
    for (held, state) in [
        (Some(held(true, Some(&basis))), "revoked"),
        (Some(held(false, None)), "unread"),
        (None, "unread"),
    ] {
        let value = view("template", held.as_ref(), Source::Held, None, None).unwrap();
        assert_eq!(value["observation"]["state"], state);
        assert!(value["mv_models"].as_array().unwrap().is_empty());
        assert!(value["heads"].as_array().unwrap().is_empty());
    }
}

#[test]
fn retained_versions_must_name_the_exact_held_head() {
    let version = json!({"model_id":"m-1","revision_id":"r1","version":1,"model":{"digest":"a".repeat(64),"byte_length":12}});
    assert!(admit_version("m-1", &version).is_ok());
    assert!(admit_version("m-2", &version).is_err());
    let mut empty = version.clone();
    empty["model"]["byte_length"] = json!(0);
    assert!(admit_version("m-1", &empty).is_err());
}

#[test]
fn the_sync_step_paints_held_rows_then_refreshes_through_one_gateway_read() {
    let room = scope("template");
    let FlowStep::Storage {
        operation: Operation::Read { .. },
        continuation,
    } = flow_start(room.clone(), ReadMode::Held).unwrap()
    else {
        panic!()
    };
    let FlowStep::Done { catalog } = flow_resume(
        continuation,
        FlowResult::Storage {
            held: None,
            changes: None,
        },
    )
    .unwrap() else {
        panic!()
    };
    assert_eq!(catalog["observation"]["state"], "unread");
    let FlowStep::Storage { continuation, .. } =
        flow_start(room.clone(), ReadMode::Refresh).unwrap()
    else {
        panic!()
    };
    let FlowStep::Fetch {
        operation,
        body,
        continuation,
        ..
    } = flow_resume(
        continuation,
        FlowResult::Storage {
            held: None,
            changes: None,
        },
    )
    .unwrap()
    else {
        panic!()
    };
    assert_eq!(operation, GATEWAY_OPERATION);
    assert_eq!(body["action"], "list_models");
    let refused = flow_resume(
        continuation.clone(),
        FlowResult::Fetch {
            status: 403,
            body: json!({}),
            now: 9,
        },
    )
    .unwrap();
    assert!(matches!(
        refused,
        FlowStep::Storage {
            operation: Operation::Revoke { .. },
            ..
        }
    ));
    let offline = flow_resume(continuation.clone(), FlowResult::Failure).unwrap();
    assert!(matches!(
        offline,
        FlowStep::Storage {
            operation: Operation::MarkFailed { expected: None, .. },
            ..
        }
    ));
    let answer = json!({"success":true,"data":{"models":[head("m-1","r1","A")],"has_more":false,"next_cursor":""}});
    let FlowStep::Storage {
        operation:
            Operation::Commit {
                listing,
                now,
                expected,
                ..
            },
        ..
    } = flow_resume(
        continuation,
        FlowResult::Fetch {
            status: 200,
            body: answer,
            now: 11,
        },
    )
    .unwrap()
    else {
        panic!()
    };
    assert_eq!(
        (listing["models"][0]["model_id"].as_str(), now, expected),
        (Some("m-1"), 11, None)
    );
}

#[test]
fn mv_rows_are_promoted_live_mv_heads_labelled_by_name_or_identity() {
    let row = |head: Value| HeldHead {
        head: head_projection(&head),
        first_seen_at_ms: 1,
        retained_version: None,
    };
    let mut unnamed = head("m-unnamed", "r", "");
    unnamed["display_name"] = json!("");
    let mut deleted = head("m-deleted", "r", "Deleted");
    deleted["state"] = json!("deleted");
    let mut temporary = head("m-temporary", "r", "Legacy");
    temporary["lifecycle"] = json!("temporary");
    let mut lv = head("m-lv", "r", "LV");
    lv["model_kind"] = json!("lv_network");
    let held = Held {
        observation: Observation {
            basis_sha256: Some("b".repeat(64)),
            observed_at_ms: Some(1),
            needs_refresh: false,
            last_error: None,
            revoked: false,
            revision: 1,
        },
        heads: vec![
            row(head("m-1", "r", "Model 1")),
            row(unnamed),
            row(deleted),
            row(temporary),
            row(lv),
        ],
    };
    let catalog = view("template", Some(&held), Source::Held, None, None).unwrap();
    let labels: Vec<_> = catalog["mv_models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| row["label"].as_str().unwrap())
        .collect();
    assert_eq!(labels, ["m-unnamed", "Model 1"]);
    // Every live head stays a head (project state and retirement read them).
    assert_eq!(catalog["heads"].as_array().unwrap().len(), 5);
}
