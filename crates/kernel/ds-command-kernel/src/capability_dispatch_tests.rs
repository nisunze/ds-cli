//! Shared exports admit each request by its row. Every action of a shared
//! export's closed vocabulary names one manifest row (or is declared by no
//! row and refused); every row the export owns is reached by some request,
//! or is listed as assembled from other rows' requests; and a request whose
//! row is not offered here, or is owned by another export, is refused before
//! anything is sent.
use super::*;
use crate::gateway_exchange::{design_collab, forms, messaging, project_work, projects, styles};
use crate::gateway_exchange::Refusal as Refused;
use serde_json::{json, Map, Value};
use std::collections::BTreeSet;

fn owned_by(export: &str) -> BTreeSet<&'static str> {
    CAPABILITIES
        .iter()
        .filter(|capability| capability.export == Some(export))
        .map(|capability| capability.id)
        .collect()
}

/// Every reached id is a row; every row the export owns is reached or
/// performed as other rows' requests (`assembled`).
fn holds(export: &str, reached: &BTreeSet<&'static str>, assembled: &[&str]) {
    for id in reached {
        assert!(capability(id).is_ok(), "{export} names `{id}`, which no row declares");
    }
    let owned = owned_by(export);
    assert!(!owned.is_empty(), "no row names {export}");
    let unreached: Vec<_> = owned
        .iter()
        .filter(|id| !reached.contains(*id) && !assembled.contains(id))
        .collect();
    assert!(unreached.is_empty(), "{export} owns rows no request reaches: {unreached:?}");
    for id in assembled {
        assert!(owned.contains(id), "{id} is not a row {export} owns");
        assert!(!reached.contains(id), "{id} is reached directly; it is not assembled");
    }
}

fn action(action: &str) -> Map<String, Value> {
    Map::from_iter([("action".to_owned(), json!(action))])
}

fn code(result: Result<impl std::fmt::Debug, Refused>) -> String {
    result.unwrap_err().code
}

#[test]
fn design_collaboration_requests_name_their_rows() {
    use design_collab::Surface::{Annotations, Attachments, Selections, Versions};
    let vocabulary = [
        (Selections, design_collab::SELECTIONS),
        (Attachments, design_collab::ATTACHMENTS),
        (Annotations, design_collab::ANNOTATIONS),
        (Versions, design_collab::VERSIONS),
        (Versions, design_collab::REVISIONS),
    ];
    let mut reached = BTreeSet::new();
    for (surface, actions) in vocabulary {
        for name in actions {
            let id = design_collab::capability(surface, name)
                .unwrap_or_else(|| panic!("{surface:?} `{name}` names no row"));
            reached.insert(id);
        }
    }
    // The page assembles a batch begin from one begin per object.
    holds(design_collab::EXPORT, &reached, &["design.version.begin-batch"]);

    let call = |surface, name: &str| design_collab::DesignCollab {
        surface,
        request: action(name),
    };
    assert_eq!(call(Annotations, "create_thread").admit_in_page("web").unwrap(), "design.comment.post");
    assert_eq!(call(Annotations, "add_comment").admit_in_page("desktop").unwrap(), "design.comment.post");
    // A row the page does not run, a row another export owns, a platform that
    // does not offer it, and an action outside the vocabulary.
    assert_eq!(code(call(Annotations, "apply_tag_group").admit_in_page("web")), CODE_EXPORT_MISMATCH);
    assert_eq!(code(call(Versions, "list_versions").admit_in_page("web")), CODE_EXPORT_MISMATCH);
    assert_eq!(code(call(Annotations, "create_thread").admit_in_page("android")), CODE_DISCARDED);
    assert_eq!(code(call(Annotations, "create_thread").admit_in_page("cli")), CODE_NOT_IN_PAGE);
    assert_eq!(code(call(Selections, "drop_everything").admit_in_page("web")), design_collab::INPUT_INVALID);
}

#[test]
fn project_work_requests_and_commands_name_their_rows() {
    let undeclared = ["link_message", "download_submission_bundle_url"];
    let mut reached = BTreeSet::new();
    for name in project_work::ACTIONS.iter().chain(std::iter::once(&project_work::PUBLIC_INDEX)) {
        if matches!(*name, "commit_command" | "commit_batch") {
            continue;
        }
        match project_work::capability(&action(name)) {
            Some(id) => {
                reached.insert(id);
            }
            None => assert!(undeclared.contains(name), "`{name}` names no row"),
        }
    }
    let with = |name: &str, key: &str, value: Value| {
        let mut request = action(name);
        request.insert(key.to_owned(), value);
        project_work::capability(&request).unwrap()
    };
    assert_eq!(with("get_context", "record_id", json!("r")), "pm.record.read");
    assert_eq!(with("get_context", "task_id", json!("t")), "pm.task.read");
    assert_eq!(with("get_context", "residual_id", json!("x")), "pm.task.read");
    assert_eq!(with("get_context", "task_id", Value::Null), "pm.overview");
    let record = with("start_attachment_upload", "data", json!({"linked_entity_type": "record"}));
    assert_eq!(record, "pm.record.attach");
    reached.extend(["pm.record.read", "pm.task.read", record]);
    for kind in [
        "create_task", "assign_task", "request_assignment", "accept_assignment",
        "decline_assignment", "update_task_fields", "set_delivery_state", "set_review_state",
        "set_closeout_state", "move_task", "resize_task", "set_progress", "delete_task",
        "add_dependency", "update_dependency", "remove_dependency", "reparent_task",
        "reorder_task", "reopen_task", "set_calendar", "add_residual_item",
        "update_residual_item", "resolve_residual_item", "promote_residual_item",
    ] {
        reached.insert(project_work::command_capability(kind).unwrap_or_else(|| panic!("{kind}")));
    }
    // A subdivision commits a create and an assignment; task lists and task
    // geometry are views of the plan graph read.
    holds(
        project_work::EXPORT,
        &reached,
        &["pm.task.subdivide", "pm.task.list", "pm.task.geometry.read"],
    );

    let request = |map: Map<String, Value>| project_work::ProjectWork { request: map };
    let mut batch = action("commit_batch");
    batch.insert(
        "commands".into(),
        json!([{"kind": "create_task"}, {"kind": "request_assignment"}, {"kind": "create_task"}]),
    );
    assert_eq!(
        request(batch.clone()).admit_in_page("web").unwrap(),
        ["pm.task.create", "pm.task.assign"]
    );
    // One command outside the vocabulary refuses the whole batch.
    batch.insert("commands".into(), json!([{"kind": "create_task"}, {"kind": "launch_rocket"}]));
    assert_eq!(code(request(batch.clone()).admit_in_page("web")), project_work::INPUT_INVALID);
    batch.insert("commands".into(), json!([]));
    assert_eq!(code(request(batch).admit_in_page("web")), project_work::INPUT_INVALID);
    let mut commit = action("commit_command");
    commit.insert("command".into(), json!({"kind": "set_progress"}));
    assert_eq!(request(commit).admit_in_page("desktop").unwrap(), ["pm.task.progress"]);
    assert_eq!(request(action("create_record")).admit_in_page("web").unwrap(), ["pm.record.create"]);
    // Core parity the page never runs, dead vocabulary, a platform without it.
    assert_eq!(code(request(action("party_list")).admit_in_page("desktop")), CODE_EXPORT_MISMATCH);
    assert_eq!(code(request(action("link_message")).admit_in_page("web")), project_work::INPUT_INVALID);
    assert_eq!(code(request(action("get_graph")).admit_in_page("android")), CODE_DISCARDED);
}

#[test]
fn messaging_commands_name_their_rows() {
    use messaging::MessagingEndpoint::{Attachments, Commands};
    let mut reached = BTreeSet::new();
    for endpoint in [Commands, Attachments] {
        for name in endpoint.actions() {
            reached.insert(
                messaging::capability(endpoint, name)
                    .unwrap_or_else(|| panic!("{endpoint:?} `{name}` names no row")),
            );
        }
    }
    holds(messaging::EXPORT, &reached, &[]);
    let call = |endpoint, name: &str| messaging::Messaging {
        endpoint,
        payload: action(name),
        timeout_ms: None,
        accepts: Vec::new(),
    };
    assert_eq!(call(Commands, " Advance_Read ").admit_in_page("web").unwrap(), "messaging.mark-read");
    assert_eq!(call(Attachments, "start").admit_in_page("desktop").unwrap(), "messaging.attachment.upload");
    // Sending is the outbox drain's; listing installations is Core's.
    assert_eq!(code(call(Commands, "send_message").admit_in_page("web")), CODE_EXPORT_MISMATCH);
    assert_eq!(code(call(Commands, "list_installations").admit_in_page("desktop")), CODE_EXPORT_MISMATCH);
    assert_eq!(code(call(Commands, "launch_rocket").admit_in_page("web")), messaging::INPUT_INVALID);
}

#[test]
fn form_requests_name_their_rows() {
    use forms::FormsEndpoint::{FormFactory, ProjectForms};
    let undeclared = [
        (FormFactory, "resolve_project"),
        (FormFactory, "get_context_forms"),
        (FormFactory, "get_governance"),
        (FormFactory, "audit_orphans"),
        (FormFactory, "user_exists"),
        (FormFactory, "get_forms_index"),
        (FormFactory, "settings_editor"),
        (FormFactory, "set_active_project"),
        (FormFactory, "get_dashboard"),
        (FormFactory, "get_project_members"),
        (ProjectForms, "diagnose"),
    ];
    let mut reached = BTreeSet::new();
    for endpoint in [FormFactory, ProjectForms] {
        for name in endpoint.actions() {
            match forms::capability(endpoint, name) {
                Some(id) => {
                    reached.insert(id);
                }
                None => assert!(undeclared.contains(&(endpoint, *name)), "{endpoint:?} `{name}`"),
            }
        }
    }
    // The settings view and the editor are one request.
    holds(forms::EXPORT, &reached, &["survey.project-form.settings"]);
    assert_eq!(forms::admit_in_page("web", FormFactory, &action("duplicate")).unwrap(), "survey.form.lifecycle");
    assert_eq!(forms::admit_in_page("desktop", ProjectForms, &action("bulk_save")).unwrap(), "survey.project-forms.apply");
    // Listing is formsAcquire's; purging is Core's; the rest is no row's.
    assert_eq!(code(forms::admit_in_page("web", FormFactory, &action("list"))), CODE_EXPORT_MISMATCH);
    assert_eq!(code(forms::admit_in_page("web", FormFactory, &action("purge"))), CODE_EXPORT_MISMATCH);
    assert_eq!(code(forms::admit_in_page("web", FormFactory, &action("get_dashboard"))), forms::INPUT_INVALID);
}

#[test]
fn project_actions_name_their_rows() {
    use projects::ProjectEndpoint::{Access, Governance, Projects, Templates};
    let undeclared = [
        (Projects, "enable_form"),
        (Projects, "disable_form"),
        (Projects, "get_dataset_info"),
        (Access, "share_project"),
        (Access, "revoke_project"),
        (Access, "member_form_grant_read"),
        (Access, "member_form_grant_plan"),
        (Access, "member_form_grant_apply"),
        (Access, "share_form"),
        (Access, "revoke_form"),
        (Access, "share_solar"),
        (Access, "revoke_solar"),
        (Access, "list_solar_users"),
    ];
    let mut reached = BTreeSet::new();
    for endpoint in [Projects, Templates, Access, Governance] {
        for name in endpoint.actions() {
            match projects::capability(endpoint, name) {
                Some(id) => {
                    reached.insert(id);
                }
                None => assert!(undeclared.contains(&(endpoint, *name)), "{endpoint:?} `{name}`"),
            }
        }
    }
    holds(projects::EXPORT, &reached, &[]);
    let call = |endpoint, name: &str| projects::ProjectAction {
        endpoint,
        action: name.to_owned(),
        data: Map::new(),
    };
    assert_eq!(call(Governance, "archive_project").admit_in_page("web").unwrap(), "auth.project.archive");
    assert_eq!(call(Access, "copy_users").admit_in_page("desktop").unwrap(), "auth.project.members.copy");
    // Deleting a project is the native host's; sharing one has no row.
    assert_eq!(code(call(Projects, "delete").admit_in_page("web")), CODE_EXPORT_MISMATCH);
    assert_eq!(code(call(Access, "share_project").admit_in_page("web")), projects::INPUT_INVALID);
}

#[test]
fn style_publishes_name_their_rows() {
    let kind = |value: Value| {
        serde_json::from_value::<crate::style_plan::StyleInstruction>(value)
            .unwrap()
            .capability()
    };
    assert_eq!(kind(json!({"kind": "seed"})), "style.seed.create");
    assert_eq!(kind(json!({"kind": "print_variant"})), "style.print.create");
    assert_eq!(kind(json!({"kind": "clear_dimension"})), "style.dimension.clear");
    assert_eq!(kind(json!({"kind": "preset", "name": "x"})), "style.preset.set");
    let reached: BTreeSet<&'static str> = [
        "style.instruction.set", "style.preset.set", "style.seed.create", "style.print.create",
        "style.appearance.set", "style.categorical.set", "style.color-range.set", "style.zoom.set",
        "style.label.set", "style.dimension.set", "style.dimension.clear", "style.cartography.set",
        "style.document.publish",
    ]
    .into();
    holds(styles::PUBLISH_EXPORT, &reached, &[]);
    assert_eq!(styles::admit_publish("web", Some("style.label.set")).unwrap(), "style.label.set");
    assert_eq!(styles::admit_publish("desktop", Some("style.document.publish")).unwrap(), "style.document.publish");
    assert_eq!(code(styles::admit_publish("web", None)), styles::INPUT_INVALID);
    assert_eq!(code(styles::admit_publish("web", Some("notes.create"))), CODE_EXPORT_MISMATCH);
    assert_eq!(code(styles::admit_publish("android", Some("style.label.set"))), CODE_DISCARDED);
}
