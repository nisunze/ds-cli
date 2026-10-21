use super::*;
#[test]
fn shared_task_controls_are_discoverable_for_milestones_without_aliases() {
    for command in [&READ, &EDIT] {
        assert!(command.summary.contains("milestone"));
        assert!(command.purpose.contains("milestone"));
        assert_eq!(command.path[1], "task");
    }
    for action in ["add", "edit", "check", "uncheck", "move", "remove"] {
        assert!(EDIT.purpose.contains(action));
    }
}
#[test]
fn concise_read_preserves_server_completion_and_explicit_truncation() {
    let raw = json!({"revision":7,"checklist":{"kind":"issues","items":[{"id":"i1","text":"first","state":"answered","body":"details","geometry":{"type":"Point","coordinates":[30,-2]}},{"id":"i2","text":"second","state":"open"}],"total":2,"completed":1,"overlay":{"type":"MultiPoint","coordinates":[[30,-2]]}}});
    let result = read_projection(&raw, "p", "t", 1, false).unwrap();
    assert_eq!(result["completed"], 1);
    assert_eq!(result["more"], true);
    assert_eq!(result["items"][0]["state"], "answered");
    assert!(result["items"][0].get("body").is_none());
    assert_eq!(result["items"][0]["has_geometry"], true);
    let detailed = read_projection(&raw, "p", "t", 200, true).unwrap();
    assert_eq!(detailed["items"], raw["checklist"]["items"]);
    assert_eq!(detailed["overlay"], raw["checklist"]["overlay"]);
    assert_eq!(detailed["more"], false);
}
#[test]
fn edit_json_is_closed_and_empty_arrays_remain_explicit() {
    let edit = decode_edit(
        br#"{"action":"edit","item_id":"i","attachment_ids":[],"links":[],"body":""}"#,
    )
    .unwrap();
    assert_eq!(edit.attachment_ids, Some(vec![]));
    assert_eq!(edit.body, Some(String::new()));
    assert!(decode_edit(br#"{"action":"edit","item_id":"i","task_id":"another"}"#).is_err());
    assert!(decode_edit(br#"{"action":"edit","item_id":"i","checked":true}"#).is_err());
}
#[test]
fn promotion_has_exact_replay_identity_and_one_existing_command() {
    let inputs = ds_cli_contract::args::parse(
        &PROMOTE,
        &[
            "--project",
            "p",
            "--task",
            "parent",
            "--item",
            "i",
            "--child",
            "child",
            "--id",
            "retry-0001",
            "--base-revision",
            "7",
            "--request",
            "FIELD@example.com",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>(),
    )
    .unwrap();
    let (command, promoted, id, revision) = promotion(&inputs).unwrap();
    assert_eq!(id, "retry-0001");
    assert_eq!(revision, 7);
    assert_eq!(promoted.request_emails, vec!["field@example.com"]);
    assert_eq!(command, promotion(&inputs).unwrap().0);
    let project_management::Command::Commit { command, .. } = command else {
        panic!()
    };
    assert_eq!(command["kind"], "promote_checklist_item");
    assert_eq!(command["promote_checklist_item"]["task_id"], "parent");
    assert_eq!(command["promote_checklist_item"]["item_id"], "i");
}
