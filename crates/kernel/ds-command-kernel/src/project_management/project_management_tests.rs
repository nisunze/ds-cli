use super::*;
use serde_json::json;

#[test]
fn browser_canonical_graph_uses_the_same_lateness_and_timestamps() {
    let reply = run(json!({
        "schema": SCHEMA, "action": "plan", "today": "2026-10-02",
        "graph_format": "canonical", "graph": {"projectId": "p", "revision": 7,
            "tasks": [
                {"id": "late", "title": "Late", "finishDate": "2026-10-01",
                 "deliveryState": "in_progress", "updatedAt": 1790812800000i64, "updatedBy": "owner"},
                {"id": "inbox", "title": "Unscheduled", "finishDate": "", "placement": "inbox"},
                {"id": "done", "title": "Done", "finishDate": "2026-10-01", "deliveryState": "complete"}
            ], "permissions": {"view": true, "editSchedule": true}
        }
    })).unwrap();
    assert_eq!(reply["project"], "p");
    assert_eq!(reply["dashboard"]["pastPlannedFinish"], 1);
    assert_eq!(reply["attentionTotal"], 1);
    assert_eq!(reply["recent"][0]["updatedBy"], "owner");
    assert_eq!(reply["permissions"]["editSchedule"], true);
}

#[test]
fn wasm_write_planners_keep_exact_model_pins_and_share_native_refusals() {
    let graph = json!({"projectId": "p", "revision": 7,
        "tasks": [{"id": "parent", "title": "Parent", "responsibleEmail": "old@example.com"}],
        "permissions": {"view": true, "editSchedule": true, "contribute": true}
    });
    let link = json!({"kind": "ds_object", "projectId": "p", "objectType": "mv_model",
        "entityId": "model-1", "objectRevision": "revision-1", "modelVersion": 3,
        "modelDigest": "a".repeat(64), "attachedBy": "owner@example.com"});
    let create = run(json!({"schema": SCHEMA, "action": "task.create",
        "graph_format": "canonical", "graph": graph, "now": 123,
        "input": {"id": "new", "kind": "child", "parentTaskId": "parent",
            "title": " New ", "links": [link], "startDate": "2026-10-01", "finishDate": "2026-10-02"}
    })).unwrap();
    assert_eq!(create["prepared"]["baseRevision"], 7);
    assert_eq!(create["draftCommand"]["task"]["durationMinutes"], 960);
    assert_eq!(create["draftCommand"]["task"]["links"][0], link);
    let encoded = run(json!({"schema": SCHEMA, "action": "command.encode",
        "command": create["draftCommand"]}))
    .unwrap();
    assert_eq!(encoded, create["prepared"]["command"]);
    assert_eq!(
        encoded["create_task"]["links"][0]["object_revision"],
        "revision-1"
    );
    assert_eq!(
        encoded["create_task"]["links"][0]["model_digest"],
        "a".repeat(64)
    );
    let update = run(json!({"schema": SCHEMA, "action": "task.update",
        "graph_format": "canonical", "graph": graph,
        "input": {"task": "parent", "progress": 50, "links": [link]}}))
    .unwrap();
    assert_eq!(
        update["prepared"]["commands"][0]["update_task_fields"]["links"][0]["model_version"],
        3
    );
    let assign = run(
        json!({"schema": SCHEMA, "action": "task.assign", "graph_format": "canonical",
        "graph": graph, "input": {"task": "parent", "mode": "owner", "owner": "NEW@example.com"}}),
    )
    .unwrap();
    assert_eq!(
        assign["prepared"]["command"]["assign_task"]["responsible_email"],
        "new@example.com"
    );
    assert_eq!(
        assign["prepared"]["command"]["assign_task"]["collaborator_emails"],
        json!(["old@example.com"])
    );
    let respond = run(
        json!({"schema": SCHEMA, "action": "task.respond", "graph_format": "canonical",
        "graph": graph, "input": {"task": "parent", "response": "accept"}}),
    )
    .unwrap();
    assert_eq!(respond["prepared"]["command"]["kind"], "accept_assignment");
    let mut readonly = graph;
    readonly["permissions"]["editSchedule"] = json!(false);
    let refusal = run(
        json!({"schema": SCHEMA, "action": "task.create", "graph_format": "canonical",
        "graph": readonly, "input": {"id": "new", "title": "New", "kind": "parent"}}),
    )
    .unwrap_err();
    assert!(refusal.contains("editor access"));
}

#[test]
fn command_codec_keeps_geojson_properties_and_model_pin_updates() {
    let geometry = json!({"type": "Feature", "properties": {"operatorKey": "unchanged"},
        "geometry": {"type": "Point", "coordinates": [30, -2]}});
    let command = json!({"kind": "update_task_fields", "taskId": "t",
        "fields": {"geometry": geometry, "links": [{"objectRevision": "rev", "modelVersion": 3}]}});
    let encoded =
        run(json!({"schema": SCHEMA, "action": "command.encode", "command": command})).unwrap();
    assert_eq!(encoded["update_task_fields"]["geometry"], geometry);
    assert_eq!(
        encoded["update_task_fields"]["links"][0]["object_revision"],
        "rev"
    );
    assert!(run(
        json!({"schema": SCHEMA, "action": "command.encode", "command":
        {"kind": "update_dependency", "dependencyId": "d", "type": "FS", "lagMinutes": 15}})
    )
    .is_err());
}

fn run(request: Value) -> Result<Value, String> {
    evaluate(&serde_json::to_vec(&request).expect("request serialises"))
        .map(|reply| serde_json::from_str(&reply).expect("reply parses"))
}

#[test]
fn browser_task_filter_is_the_cli_rule_over_the_held_graph() {
    let graph = json!({"projectId": "p", "revision": 4, "tasks": [
        {"id": "mapped", "title": "Survey span", "discipline": "Survey",
         "geometry": {"type": "Point", "coordinates": [30.06, -1.95]},
         "responsibleEmail": "Lead@ds.rw"},
        {"id": "desk", "title": "Write report", "discipline": "Design", "placement": "inbox"},
        {"id": "mapped2", "title": "Stake poles", "discipline": "Survey",
         "geometry": {"type": "LineString", "coordinates": [[30, -2], [30.1, -2]]}}
    ], "permissions": {"view": true}});
    let ids = |filter: Value| -> Value {
        let mut request = json!({"schema": SCHEMA, "action": "task.list",
            "graph_format": "canonical", "graph": graph.clone()});
        for (key, value) in filter.as_object().unwrap() {
            request[key] = value.clone();
        }
        run(request).unwrap()["ids"].clone()
    };
    assert_eq!(ids(json!({})), json!(["desk", "mapped", "mapped2"]));
    assert_eq!(
        ids(json!({"geometry": "with"})),
        json!(["mapped", "mapped2"])
    );
    assert_eq!(ids(json!({"geometry": "without"})), json!(["desk"]));
    assert_eq!(
        ids(json!({"discipline": "Survey", "query": "stake"})),
        json!(["mapped2"])
    );
    // The holder match is the CLI's: case-insensitive.
    assert_eq!(ids(json!({"assignee": "lead@DS.rw"})), json!(["mapped"]));
    assert_eq!(ids(json!({"placement": "inbox"})), json!(["desk"]));
    let refused = run(json!({"schema": SCHEMA, "action": "task.list",
        "graph_format": "canonical", "graph": graph, "geometry": "some"}));
    assert!(refused.unwrap_err().contains("Unsupported geometry"));
}

/// 2026-09-18T00:00:00Z. Every stamp in the fixture is stated as an offset
/// from this one day, so a reader can check the day counts by eye.
const TODAY_MS: i64 = 1_789_689_600_000;
const DAY: i64 = 86_400_000;

/// One small but complete project: a finished survey, a blocked design
/// with two children, a milestone, an unfiled inbox task carrying two open
/// residuals, a deleted row and a nameless row. It exercises all four
/// projections at once, which is the point — the plan is one fold and its
/// parts have to agree with each other.
fn fixture() -> Value {
    json!({
        "project_id": "ds-plan-fixture",
        "revision": 42,
        "schedule_revision": 7,
        "permissions": {"view": true, "comment": true, "contribute": true, "edit_schedule": true},
        "field_model": {
            "delivery_states": ["not_started", "in_progress", "complete"],
            "review_states": ["not_required", "under_review"],
            "closeout_states": ["open", "closed"],
            "priorities": ["low", "normal", "high", "critical"],
            "task_types": ["task", "milestone"],
            "scheduling_modes": ["manual", "auto"],
            "placements": ["wbs", "inbox"],
            "dependency_types": ["FS", "SS"],
            "residual_kinds": ["blocker", "other"],
            "residual_statuses": ["open", "resolved"],
            "scope_classifications": ["within_original_scope"],
            "work_classifications": ["bounded_residual", "scope_review_required"],
            "max_assignees": 5,
            "max_batch_commands": 200
        },
        "tasks": [
            {"id": "survey-1",
             "data": {"title": "Route survey", "discipline": "Survey", "sort_key": "0100",
                      "start_date": "2026-09-01", "finish_date": "2026-09-10",
                      "delivery_state": "complete"},
             "metadata": {"updated_at": TODAY_MS - 20 * DAY, "updated_by": "ops@ds.rw"}},
            {"id": "design-1",
             "data": {"title": "LV design", "discipline": "Design", "sort_key": "0200",
                      "start_date": "2026-09-05", "finish_date": "2026-09-15", "status": "doing"},
             "computed": {"open_blocker_count": 2, "descendant_blocker_count": 1},
             "metadata": {"updated_at": TODAY_MS - 3 * DAY, "updated_by": "lead@ds.rw"}},
            {"id": "design-1a",
             "data": {"title": "Pole schedule", "discipline": "Design", "sort_key": "0210",
                      "parent_task_id": "design-1", "finish_date": "2026-09-12",
                      "delivery_state": "paused", "progress": 0.5},
             "metadata": {"updated_at": TODAY_MS - DAY}},
            {"id": "review-1",
             "data": {"title": "Client review", "discipline": "Design", "sort_key": "0220",
                      "parent_task_id": "design-1", "start_date": "2026-09-16",
                      "finish_date": "2026-09-30", "review_state": "under_review"},
             "metadata": {"updated_at": TODAY_MS}},
            {"id": "handover",
             "data": {"title": "Handover", "discipline": "Design", "sort_key": "0300",
                      "type": "milestone", "finish_date": "2026-09-30"},
             "metadata": {"updated_at": TODAY_MS - 5 * DAY}},
            {"id": "inbox-1",
             "data": {"title": "Unfiled query", "sort_key": "0400", "placement": "inbox"},
             "metadata": {"updated_at": 1}},
            {"id": "gone", "data": {"title": "Removed"}, "is_deleted": true},
            {"id": "nameless", "data": {"description": "no title"}}
        ],
        "residual_items": [
            {"id": "r1", "task_id": "inbox-1", "title": "Await client drawing",
             "status": "awaiting_response", "blocks_acceptance": true, "blocks_closeout": true},
            {"id": "r2", "task_id": "design-1a", "title": "Signature", "status": "resolved"},
            {"id": "r3", "task_id": "inbox-1", "title": "Second query", "kind": "blocker"}
        ]
    })
}

fn plan_of(limit: Option<i64>) -> Value {
    let mut request = json!({
        "schema": SCHEMA, "action": "plan", "today": "2026-09-18",
        "ds_project": "ds-session-project", "graph": fixture(),
    });
    if let Some(limit) = limit {
        request["limit"] = json!(limit);
    }
    run(request).expect("plan")
}

#[test]
fn the_schema_and_the_action_are_each_refused_by_name() {
    // A host that reaches the wrong module, or asks for a verb this one
    // does not have, must be told WHICH of the two it got wrong.
    let error = run(json!({"schema": "ds.other/v1", "action": "plan"})).unwrap_err();
    assert!(
        error.starts_with("Unsupported project management schema `ds.other/v1`"),
        "{error}"
    );
    let error = run(json!({"schema": SCHEMA, "action": "sweep"})).unwrap_err();
    assert!(
        error.starts_with("Unsupported project management action `sweep`"),
        "{error}"
    );
    let error = run(json!({"schema": SCHEMA})).unwrap_err();
    assert!(
        error.starts_with("Unsupported project management action ``"),
        "{error}"
    );
    assert!(evaluate(b"not json")
        .unwrap_err()
        .starts_with("Invalid project management request"));
    assert!(evaluate(&vec![b' '; MAX_INPUT_BYTES + 1])
        .unwrap_err()
        .starts_with("Project management request exceeds its bound"));
}

#[test]
fn an_unknown_request_member_is_a_contract_break() {
    // The browser ignored what it did not recognise. The kernel cannot:
    // a member nobody reads is a caller believing something is happening.
    let error = run(json!({
        "schema": SCHEMA, "action": "plan", "today": "2026-09-18",
        "graph": {}, "page": 2,
    }))
    .unwrap_err();
    assert!(
        error.starts_with("Invalid project management request"),
        "{error}"
    );
}

#[test]
fn a_malformed_day_and_an_out_of_range_limit_are_each_refused_by_name() {
    // Every date comparison in the fold is a STRING comparison, so a day
    // that is not `YYYY-MM-DD` would quietly mis-sort the whole plan
    // rather than fail. The kernel takes `today` from a host, so it checks.
    let error = run(json!({
        "schema": SCHEMA, "action": "plan", "today": "18/09/2026", "graph": {},
    }))
    .unwrap_err();
    assert!(
        error.starts_with("Project management refuses today `18/09/2026`"),
        "{error}"
    );
    for limit in [0, 101] {
        let error = run(json!({
            "schema": SCHEMA, "action": "plan", "today": "2026-09-18",
            "graph": {}, "limit": limit,
        }))
        .unwrap_err();
        assert!(
            error.starts_with(&format!("Project management refuses limit `{limit}`")),
            "{error}"
        );
    }
}

#[test]
fn the_plan_folds_one_graph_into_four_agreeing_projections() {
    // The frozen fixture. Each number below is checkable against the
    // fixture by hand, and the four projections have to add up against one
    // another — that is what makes this one fold rather than four reports.
    let plan = plan_of(None);
    assert_eq!(plan["schema"], SCHEMA);
    assert_eq!(plan["project"], "ds-plan-fixture");
    assert_eq!(plan["revision"], 42);
    assert_eq!(plan["today"], "2026-09-18");

    let dashboard = &plan["dashboard"];
    // Six live nodes: the deleted row and the nameless row never arrive.
    assert_eq!(dashboard["planNodes"], 6);
    assert_eq!(dashboard["totalTasks"], 5);
    assert_eq!(dashboard["milestones"], 1);
    assert_eq!(dashboard["primaryComplete"], 1);
    assert_eq!(dashboard["underReview"], 1);
    assert_eq!(dashboard["inProgress"], 1);
    assert_eq!(dashboard["blocked"], 0);
    assert_eq!(dashboard["paused"], 1);
    assert_eq!(dashboard["blockedOrPaused"], 1);
    // 1 (complete, undeclared) + 0.5 (declared) over six plan nodes.
    assert_eq!(dashboard["overallProgress"], 0.25);
    assert_eq!(dashboard["openResiduals"], 2);
    assert_eq!(dashboard["acceptanceBlocked"], 1);
    assert_eq!(dashboard["closeoutBlocked"], 1);
    assert_eq!(dashboard["residualTaskCount"], 1);

    // Biggest discipline first, the nameless bucket last, whatever its size.
    assert_eq!(plan["phaseTotal"], 3);
    let phases: Vec<(&str, u64)> = plan["phases"]
        .as_array()
        .expect("phases")
        .iter()
        .map(|phase| {
            (
                phase["discipline"].as_str().unwrap_or_default(),
                phase["total"].as_u64().unwrap_or_default(),
            )
        })
        .collect();
    assert_eq!(phases, vec![("Design", 4), ("Survey", 1), ("", 1)]);
    assert_eq!(plan["phases"][1]["progress"], 1.0);
    assert!(plan["phases"][2]["unassigned"].as_bool().unwrap_or(false));

    // One row per task that needs a decision, worst reason first, each
    // carrying the WBS the Table shows and the magnitude to act on.
    assert_eq!(plan["attentionTotal"], 4);
    assert_eq!(
        plan["attention"],
        json!([
            {"taskId": "design-1", "title": "LV design", "kind": "blocked", "wbs": "2",
             "blockerCount": 3, "daysLate": 3, "hoursOver": 0.0, "daysSinceChange": 3, "residualCount": 0},
            {"taskId": "design-1a", "title": "Pole schedule", "kind": "late", "wbs": "2.1",
             "blockerCount": 0, "daysLate": 6, "hoursOver": 0.0, "daysSinceChange": 1, "residualCount": 0},
            {"taskId": "review-1", "title": "Client review", "kind": "review", "wbs": "2.2",
             "blockerCount": 0, "daysLate": 0, "hoursOver": 0.0, "daysSinceChange": 0, "residualCount": 0},
            {"taskId": "inbox-1", "title": "Unfiled query", "kind": "residual", "wbs": "Inbox 1",
             "blockerCount": 0, "daysLate": 0, "hoursOver": 0.0, "daysSinceChange": 0, "residualCount": 2},
        ])
    );
    // The fixture proposes nothing and logs nothing: the proposal rows
    // and counters are present and empty, never absent.
    assert_eq!(plan["proposalsTotal"], 0);
    assert_eq!(plan["proposals"], json!([]));
    assert_eq!(dashboard["proposalsPending"], 0);
    assert_eq!(dashboard["overEstimate"], 0);
    assert_eq!(plan["vocabulary"]["admissionStates"], json!([]));

    // Recent is the canonical update stamp, newest first, and never an
    // actor the graph did not name.
    assert_eq!(plan["recentTotal"], 6);
    let recent: Vec<&str> = plan["recent"]
        .as_array()
        .expect("recent")
        .iter()
        .map(|row| row["taskId"].as_str().unwrap_or_default())
        .collect();
    assert_eq!(
        recent,
        vec![
            "review-1",
            "design-1a",
            "design-1",
            "handover",
            "survey-1",
            "inbox-1"
        ]
    );
    assert_eq!(plan["recent"][2]["updatedBy"], "lead@ds.rw");
    assert!(plan["recent"][0].get("updatedBy").is_none());

    assert_eq!(plan["permissions"]["editSchedule"], true);
    assert_eq!(plan["permissions"]["admin"], false);
    assert_eq!(plan["vocabulary"]["maxAssignees"], 5);
    assert_eq!(plan["vocabulary"]["placements"], json!(["wbs", "inbox"]));
    // The plan republishes only the eight vocabularies the write commands
    // take their values from; the residual lists are not among them.
    assert!(plan["vocabulary"].get("residualKinds").is_none());
    // A fixture from before the correspondence contract publishes neither
    // its vocabularies nor a projection — absent, not empty.
    assert!(plan["vocabulary"].get("channels").is_none());
    assert_eq!(plan["correspondence"], Value::Null);
}

#[test]
fn the_plan_publishes_correspondence_counters_and_attention_grouped_by_party() {
    // The server's awaiting rows, folded under the party the answer is
    // owed to. Overdue parties first; within a party overdue rows first,
    // then the soonest due; a blocked task sits under every party it
    // waits on; a row outstanding with no date is counted but not listed.
    let mut graph = fixture();
    graph["field_model"]["channels"] = json!(["email", "letter"]);
    graph["field_model"]["party_roles"] = json!(["client", "consultant", "contractor"]);
    graph["correspondence"] = json!({
        "as_of": "2026-09-18", "due_window_days": 7,
        "records": [
            {"record_id": "R5", "thread_id": "R1", "subject": "Questions on spans", "category": "request_for_information",
             "channel": "email", "direction": "outbound", "happened_at": "2026-09-16T09:00:00Z",
             "response_status": "outstanding", "response_due_date": "2026-09-22", "days_overdue": 0, "days_until_due": 4,
             "due_within_window": true, "response_owner_party_id": "p_tbea", "party_id": "p_tbea", "party_name": "TBEA",
             "external_party_ids": ["p_tbea"], "blocked_task_ids": ["design-1a"], "sensitivity": "internal"},
            {"record_id": "R2", "thread_id": "R1", "subject": "Five review points", "category": "instruction",
             "channel": "email", "direction": "inbound", "happened_at": "2026-09-10T09:00:00Z",
             "response_status": "overdue", "response_due_date": "2026-09-15", "days_overdue": 3, "days_until_due": 0,
             "due_within_window": false, "response_owner_id": "uid-lead", "party_id": "p_shaker", "party_name": "Shaker",
             "external_party_ids": ["p_shaker"], "blocked_task_ids": [], "sensitivity": "open"},
            {"record_id": "R9", "thread_id": "R9", "subject": "No date", "category": "review",
             "channel": "letter", "direction": "inbound", "happened_at": "2026-09-01T09:00:00Z",
             "response_status": "outstanding", "days_overdue": 0, "days_until_due": 0,
             "due_within_window": false, "response_owner_id": "uid-lead", "party_id": "p_tbea", "party_name": "TBEA",
             "external_party_ids": ["p_tbea"], "blocked_task_ids": [], "sensitivity": "open"},
        ],
        "records_total": 3, "truncated": false,
        "blocked_tasks": [{"task_id": "design-1a", "title": "Pole schedule", "record_ids": ["R5"], "party_ids": ["p_tbea"]}],
        "counters": {"recordsOutstanding": 2, "recordsOverdue": 1, "tasksAwaitingCorrespondence": 1},
    });
    let plan = run(json!({
        "schema": SCHEMA, "action": "plan", "today": "2026-09-18",
        "ds_project": "p", "graph": graph,
    }))
    .expect("plan");
    assert_eq!(plan["vocabulary"]["channels"], json!(["email", "letter"]));
    assert_eq!(
        plan["vocabulary"]["partyRoles"],
        json!(["client", "consultant", "contractor"])
    );
    let correspondence = &plan["correspondence"];
    assert_eq!(correspondence["asOf"], "2026-09-18");
    assert_eq!(correspondence["counters"]["recordsOutstanding"], 2);
    assert_eq!(correspondence["counters"]["recordsOverdue"], 1);
    assert_eq!(correspondence["counters"]["tasksAwaitingCorrespondence"], 1);
    assert_eq!(correspondence["partyTotal"], 2);
    assert_eq!(correspondence["recordsTotal"], 3);
    let parties = correspondence["parties"].as_array().expect("parties");
    assert_eq!(parties[0]["partyId"], "p_shaker", "the overdue party leads");
    assert_eq!(parties[0]["overdue"], 1);
    assert_eq!(parties[0]["records"][0]["owedBy"], "project");
    assert_eq!(parties[1]["partyId"], "p_tbea");
    assert_eq!(parties[1]["partyName"], "TBEA");
    assert_eq!(
        parties[1]["recordTotal"], 1,
        "R9 has no date and is not an attention row"
    );
    assert_eq!(parties[1]["records"][0]["recordId"], "R5");
    assert_eq!(parties[1]["records"][0]["owedBy"], "party");
    assert_eq!(parties[1]["records"][0]["daysUntilDue"], 4);
    assert_eq!(parties[1]["blockedTasks"][0]["taskId"], "design-1a");
    assert_eq!(parties[1]["blockedTaskTotal"], 1);
}

#[test]
fn a_capped_list_still_reports_how_much_it_was_capped_from() {
    // A limit bounds what is painted, never what is counted — otherwise
    // an operator reading "1 needs attention" would act on a lie.
    let plan = plan_of(Some(1));
    assert_eq!(plan["phases"].as_array().map(Vec::len), Some(1));
    assert_eq!(plan["phaseTotal"], 3);
    assert_eq!(plan["attention"].as_array().map(Vec::len), Some(1));
    assert_eq!(plan["attentionTotal"], 4);
    assert_eq!(plan["recent"].as_array().map(Vec::len), Some(1));
    assert_eq!(plan["recentTotal"], 6);
}

#[test]
fn a_graph_with_no_field_model_publishes_no_vocabulary_rather_than_an_empty_one() {
    // Absent and empty are different answers. An empty list would tell the
    // CLI the engine accepts nothing, which is the opposite of the truth.
    let plan = run(json!({
        "schema": SCHEMA, "action": "plan", "today": "2026-09-18",
        "ds_project": "ds-session-project", "graph": {"revision": 3},
    }))
    .expect("plan");
    assert_eq!(plan["vocabulary"], Value::Null);
    // And a graph that names no project of its own is the host's.
    assert_eq!(plan["project"], "ds-session-project");
    assert_eq!(plan["dashboard"]["planNodes"], 0);
    assert_eq!(plan["dashboard"]["overallProgress"], 0.0);
}
