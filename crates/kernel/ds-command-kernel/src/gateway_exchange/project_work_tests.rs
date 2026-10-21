use super::*;
use crate::gateway_exchange::{resume, start, Facts, Observed, Step};
use serde_json::json;

fn call(value: Value) -> ProjectWork {
    ProjectWork {
        request: value.as_object().cloned().unwrap(),
    }
}

fn planned(work: ProjectWork) -> Result<Plan, Refusal> {
    work.plan()
}

fn answered(work: ProjectWork, body: Value) -> Result<Value, Refusal> {
    let Step::Fetch { continuation, .. } = start(work) else {
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
fn a_read_is_sent_as_brain_reads_it_and_may_be_sent_twice() {
    let plan = planned(call(
        json!({"action": "get_graph", "ds_project": "it_rwanda", "cursor": null}),
    ))
    .unwrap();
    assert_eq!(plan.request.path, "/api/v1/pm");
    assert_eq!(
        plan.request.body,
        Some(json!({"action": "get_graph", "ds_project": "it_rwanda"}))
    );
    assert_eq!(plan.retries, 1);
    let write = planned(call(
        json!({"action": "commit_command", "ds_project": "it_rwanda",
        "command_id": "c-1", "base_revision": 4, "command": {"type": "task.create"}}),
    ))
    .unwrap();
    assert_eq!(write.retries, 0);
    let index = planned(call(
        json!({"action": "list_public_projects", "status": "active", "limit": 100}),
    ))
    .unwrap();
    assert_eq!(index.retries, 1);
}

#[test]
fn an_unknown_action_member_or_project_is_never_sent() {
    for request in [
        json!({"action": "drop_everything", "ds_project": "it_rwanda"}),
        json!({"action": "get_graph", "ds_project": "it_rwanda", "sql": "select"}),
        json!({"action": "get_graph"}),
        json!({"action": "get_graph", "ds_project": "../x"}),
        json!({"action": "list_public_projects", "ds_project": "it_rwanda"}),
    ] {
        let refusal = planned(call(request.clone())).unwrap_err();
        assert!(
            [INPUT_INVALID, crate::gateway_exchange::PROJECT_INVALID]
                .contains(&refusal.code.as_str()),
            "{request}: {refusal:?}"
        );
    }
    let huge = json!({"action": "commit_batch", "ds_project": "it_rwanda", "commands": ["x".repeat(1 << 20)]});
    assert_eq!(planned(call(huge)).unwrap_err().code, INPUT_INVALID);
}

#[test]
fn an_answer_is_brains_data_unless_it_was_cancelled() {
    let read = call(json!({"action": "get_graph", "ds_project": "it_rwanda"}));
    let graph = answered(
        read.clone(),
        json!({"success": true, "data": {"tasks": [], "revision": 3}}),
    )
    .unwrap();
    assert_eq!(graph["revision"], 3);
    let cancelled = answered(read, json!({"data": {"cancelled": true}})).unwrap_err();
    assert_eq!(cancelled.code, INVALID);
}

#[cfg(feature = "durable-store")]
mod held_graph {
    use super::super::{held_graph, keep_graph};
    use crate::gateway_exchange::retained::testing::{block_on, TestEdge};
    use serde_json::json;

    #[test]
    fn a_kept_graph_opens_offline_for_its_project_and_account_only() {
        let edge = TestEdge::new();
        assert_eq!(block_on(held_graph(&edge, "it_rwanda")).unwrap(), None);
        let graph = json!({"projectId": "it_rwanda", "revision": 7, "tasks": []});
        block_on(keep_graph(&edge, "it_rwanda", &graph)).unwrap();
        edge.online.set(false);
        assert_eq!(
            block_on(held_graph(&edge, "it_rwanda")).unwrap(),
            Some(graph)
        );
        assert_eq!(block_on(held_graph(&edge, "it_tchad")).unwrap(), None);
        edge.sign_in("someone-else");
        assert_eq!(block_on(held_graph(&edge, "it_rwanda")).unwrap(), None);
        assert!(edge.requests().is_empty());
        assert!(block_on(held_graph(&edge, "a b")).is_err());
    }
}
