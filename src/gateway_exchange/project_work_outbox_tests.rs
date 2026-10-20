use super::*;
use crate::gateway_exchange::retained::testing::{block_on, Answer, TestEdge};
use crate::gateway_exchange::Failure;

const P: &str = "p1";

fn task(id: &str) -> Value {
    json!({"id": id, "title": format!("Task {id}"), "parentTaskId": null, "type": "task"})
}
fn graph(revision: i64, tasks: &[&str]) -> Value {
    json!({"projectId": P, "revision": revision, "scheduleRevision": 1, "tasks": tasks.iter().map(|id| task(id)).collect::<Vec<_>>(), "dependencies": [], "residualItems": []})
}
fn receipt(command_id: &str, committed: i64, task_id: &str) -> Value {
    json!({"success": true, "data": {"result": {
        "applied": true, "command_id": command_id, "committed_revision": committed, "schedule_revision": 1,
        "task_patches": [{"id": task_id, "data": {"title": format!("Task {task_id}")}}],
        "violations": [], "warnings": []
    }}})
}
fn bodies(edge: &TestEdge) -> Vec<Value> {
    edge.requests()
        .into_iter()
        .map(|request| request.body.expect("a body"))
        .collect()
}
fn queue(edge: &TestEdge, ids: &[&str]) {
    for id in ids {
        block_on(enqueue(edge, P, &format!("cmd-{id}"), task(id), None)).unwrap();
    }
}
fn task_ids(graph: &Value) -> Vec<String> {
    graph["tasks"]
        .as_array()
        .unwrap()
        .iter()
        .map(|task| task["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn queued_creates_replay_in_order_each_on_what_the_one_before_committed() {
    let edge = TestEdge::new();
    queue(&edge, &["a", "b"]);
    block_on(enqueue(&edge, P, "cmd-a", task("a"), None)).unwrap();
    let shown = block_on(overlay(&edge, P, &graph(5, &["x"]))).unwrap();
    assert_eq!(
        task_ids(&shown),
        ["x", "a", "b"],
        "the queued creates show at once"
    );

    edge.json(receipt("cmd-a", 6, "a"))
        .json(receipt("cmd-b", 7, "b"));
    let drained = block_on(drain(&edge, P, shown, "tab")).unwrap();
    let sent = bodies(&edge);
    assert_eq!(
        (
            sent[0]["action"].as_str(),
            sent[0]["base_revision"].as_i64()
        ),
        (Some("commit_command"), Some(5))
    );
    assert_eq!(
        (
            sent[0]["command_id"].as_str(),
            sent[1]["base_revision"].as_i64()
        ),
        (Some("cmd-a"), Some(6))
    );
    assert_eq!(
        sent[0]["command"]["create_task"]["id"], "a",
        "the task travels whole, its client id included"
    );
    assert_eq!(
        (
            drained.drained,
            drained.deferred,
            drained.graph["revision"].as_i64()
        ),
        (2, 0, Some(7))
    );
    assert_eq!(task_ids(&drained.graph), ["x", "a", "b"]);
    assert!(block_on(held(&edge, P)).unwrap().is_empty());
    let kept = block_on(crate::gateway_exchange::project_work::held_graph(&edge, P)).unwrap();
    assert_eq!(
        kept.unwrap()["revision"],
        7,
        "the graph is kept for the next offline open"
    );
}

#[test]
fn an_answer_that_decides_nothing_keeps_the_create_with_the_command_it_sent() {
    let edge = TestEdge::new();
    queue(&edge, &["a", "b"]);
    edge.answer(Answer::Failed(Failure::Timeout));
    let drained = block_on(drain(&edge, P, graph(5, &["x"]), "tab")).unwrap();
    assert_eq!((drained.drained, drained.deferred), (0, 2));
    assert_eq!(task_ids(&drained.graph), ["x", "a", "b"]);
    let sends = edge.requests().len();

    // The graph moved meanwhile; the create sends the very command it sent before.
    edge.json(receipt("cmd-a", 6, "a"))
        .json(receipt("cmd-b", 9, "b"));
    let again = block_on(drain(&edge, P, graph(8, &["x", "y"]), "tab")).unwrap();
    let resent = &bodies(&edge)[sends];
    assert_eq!(
        (
            resent["command_id"].as_str(),
            resent["base_revision"].as_i64()
        ),
        (Some("cmd-a"), Some(5))
    );
    assert_eq!(
        bodies(&edge)[sends + 1]["base_revision"],
        8,
        "a create first sent now takes the freshest graph"
    );
    assert_eq!(again.drained, 2);
}

#[test]
fn a_revision_refusal_rebuilds_the_create_once_on_a_fresh_graph() {
    let edge = TestEdge::new();
    queue(&edge, &["a"]);
    edge.answer(Answer::Json(
        409,
        json!({"success": false, "error": {"code": "PM_REVISION_CONFLICT", "message": "stale"}}),
    ));
    edge.json(json!({"success": true, "data": {"graph": {"project_id": P, "revision": 9, "schedule_revision": 1,
        "tasks": [{"id": "x", "data": {"title": "Task x"}}, {"id": "z", "data": {"title": "Task z"}}]}}}));
    edge.json(receipt("cmd-a", 10, "a"));
    let drained = block_on(drain(&edge, P, graph(5, &["x"]), "tab")).unwrap();
    let sent = bodies(&edge);
    assert_eq!(sent[1]["action"], "get_graph");
    assert_eq!(
        (
            sent[2]["base_revision"].as_i64(),
            sent[2]["command_id"].as_str()
        ),
        (Some(9), Some("cmd-a"))
    );
    assert_eq!(
        (drained.drained, drained.graph["revision"].as_i64()),
        (1, Some(10))
    );
    assert_eq!(task_ids(&drained.graph), ["x", "z", "a"]);
}

#[test]
fn a_refused_create_leaves_the_queue_and_is_named_for_the_person() {
    let edge = TestEdge::new();
    queue(&edge, &["a", "b"]);
    edge.answer(Answer::Json(400, json!({"success": false, "error": {"code": "PM_TASK_INVALID", "message": "A title is required."}})));
    edge.json(receipt("cmd-b", 6, "b"));
    let drained = block_on(drain(&edge, P, graph(5, &[]), "tab")).unwrap();
    assert_eq!(
        drained.failed,
        [json!({"id": "project-work:cmd-a", "title": "Task a", "message": "A title is required."})]
    );
    assert_eq!((drained.drained, drained.deferred), (1, 0));
    assert_eq!(task_ids(&drained.graph), ["b"]);
    assert!(block_on(held(&edge, P)).unwrap().is_empty());
}

#[test]
fn another_account_never_replays_a_queued_create() {
    let edge = TestEdge::new();
    queue(&edge, &["a"]);
    edge.sign_in("someone_else");
    let drained = block_on(drain(&edge, P, graph(5, &[]), "tab")).unwrap();
    assert_eq!((drained.drained, edge.requests().len()), (0, 0));
    assert!(block_on(enqueue(&edge, P, "", task("a"), None)).is_err());
}
