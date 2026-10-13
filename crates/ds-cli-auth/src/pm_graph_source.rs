//! Fixed live/retained Plan and exact-note rooms through the existing native PM authority.
use super::*;
use ds_client_core::project_management::Command;
use ds_command_kernel::project_management::project_notes::{
    Command as NoteCommand, Context as NoteContext, Query as NoteQuery, Read as NoteRead,
};
use ds_layer_store::pm_graph_catalogue::{self, Binding, Source};
fn local(message: impl ToString) -> Failure {
    Failure::unavailable("pm_retained_unavailable", message.to_string()).remedy(
        "read the exact PM selection online under the same native account to retain its capture",
    )
}
fn binding(lane: &str, project: &str) -> Result<(LayerScopeFence, Binding), Failure> {
    let fence = capture_layer_scope_fence_for_project(lane, project)?;
    let binding = Binding {
        uid: fence.uid.clone(),
        lane: lane.into(),
        audience: fence.audience.clone(),
        credential_instance: fence.credential.clone(),
        project: fence.project.clone(),
    };
    Ok((fence, binding))
}
fn denied(error: &Failure) -> bool {
    error.class() == ds_cli_contract::outcome::ExitClass::Unauthorized
        || matches!(
            error.code(),
            "auth_revoked" | "auth_identity_mismatch" | "project_not_found"
        )
}
struct Provider<'a> {
    lane: &'a str,
    project: &'a str,
    fence: LayerScopeFence,
    binding: Binding,
    note: Option<NoteCommand>,
    task: Option<pm_graph_catalogue::TaskRead>,
}
impl Source for Provider<'_> {
    type Error = Failure;
    fn check_binding(&mut self) -> Result<(), Failure> {
        verify_layer_scope_fence_for_project(
            self.lane,
            &self.fence,
            &self.binding.uid,
            self.project,
        )
    }
    fn capture(&mut self) -> Result<Value, Failure> {
        if let Some(selection) = self.task.clone() {
            return acquire_task(self.project, &selection, |request| {
                self.check_binding()?;
                let captured = match request {
                    TaskRequest::Graph => {
                        project_management_for_project(self.lane, self.project, &Command::Graph)?
                    }
                    TaskRequest::Records => project_management_for_project(
                        self.lane,
                        self.project,
                        &Command::TaskRecords {
                            task_id: selection.task_id.clone(),
                            limit: 100,
                        },
                    )?,
                    TaskRequest::Timeline => {
                        super::correspondence::project_correspondence_for_project(
                            self.lane,
                            self.project,
                            &ds_client_core::project_correspondence::Action::TaskRead {
                                task_id: selection.task_id.clone(),
                                timeline: true,
                            },
                        )?
                    }
                };
                if captured.identity().uid() != self.binding.uid
                    || captured.identity().credential_audience_sha256() != self.binding.audience
                    || captured.lane() != self.binding.lane
                    || captured.project_id() != self.binding.project
                {
                    return Err(Failure::conflict(
                        "project_context_changed",
                        "the task source capture differs from its protected native identity",
                    ));
                }
                self.check_binding()?;
                Ok(captured.into_result())
            });
        }
        let command =
            self.note
                .as_ref()
                .map_or(Command::Graph, |selection| Command::ProjectNotes {
                    command: selection.clone(),
                });
        let captured = project_management_for_project(self.lane, self.project, &command)?;
        if captured.identity().uid() != self.binding.uid
            || captured.identity().credential_audience_sha256() != self.binding.audience
            || captured.lane() != self.binding.lane
            || captured.project_id() != self.binding.project
        {
            return Err(Failure::conflict(
                "project_context_changed",
                "the acquired PM capture differs from its protected capture identity",
            ));
        }
        Ok(captured.into_result())
    }
    fn verify(&self, value: &Value) -> Result<(), Failure> {
        if let Some(selection) = &self.task {
            return verify_task_capture(self.project, selection, value);
        }
        if let Some(selection) = &self.note {
            selection
                .verify_response(self.project, value)
                .map_err(|why| local(why))
        } else {
            ds_client_core::project_management::verify_graph(self.project, value)
                .map_err(map_client)
        }
    }
    fn access_refused(&self, error: &Failure) -> bool {
        denied(error)
    }
}
/// Offline probes protected local identity only, with no session restore, directory or freshness calls.
pub fn read(lane: &str, project: &str, offline: bool) -> Result<Value, Failure> {
    selected_read(lane, project, None, None, offline)
}
/// Read one exact note selection through the same protected retained authority as Plan.
pub fn read_note(
    lane: &str,
    project: &str,
    selection: &NoteRead,
    offline: bool,
) -> Result<Value, Failure> {
    NoteCommand::Read(selection.clone())
        .payload(project)
        .map_err(|why| Failure::invalid("project_note_invalid", why))?;
    selected_read(
        lane,
        project,
        Some(NoteCommand::Read(selection.clone())),
        None,
        offline,
    )
}
/// Read or retain one exact shared-note query page, never all pages implicitly.
pub fn read_query(
    lane: &str,
    project: &str,
    selection: &NoteQuery,
    offline: bool,
) -> Result<Value, Failure> {
    let command = NoteCommand::Query(selection.clone());
    command
        .payload(project)
        .map_err(|why| Failure::invalid("project_note_invalid", why))?;
    selected_read(lane, project, Some(command), None, offline)
}
/// Reuse one exact scoped context; this receipt does not establish Graph completeness.
pub fn read_context(
    lane: &str,
    project: &str,
    selection: &NoteContext,
    offline: bool,
) -> Result<Value, Failure> {
    let command = NoteCommand::Context(selection.clone());
    command
        .payload(project)
        .map_err(|why| Failure::invalid("project_note_invalid", why))?;
    selected_read(lane, project, Some(command), None, offline)
}
/// One bounded task read uses canonical Graph and selected records receipts; timeline is explicit.
pub fn read_task(
    lane: &str,
    project: &str,
    task_id: &str,
    timeline: bool,
    offline: bool,
) -> Result<Value, Failure> {
    Command::TaskRecords {
        task_id: task_id.into(),
        limit: 100,
    }
    .validate()
    .map_err(map_client)?;
    let selection = pm_graph_catalogue::TaskRead {
        task_id: task_id.into(),
        timeline,
    };
    selected_read(lane, project, None, Some(selection), offline)
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TaskRequest {
    Graph,
    Records,
    Timeline,
}
fn acquire_task(
    project: &str,
    selection: &pm_graph_catalogue::TaskRead,
    mut acquire: impl FnMut(TaskRequest) -> Result<Value, Failure>,
) -> Result<Value, Failure> {
    let graph = acquire(TaskRequest::Graph)?;
    ds_client_core::project_management::verify_graph(project, &graph).map_err(map_client)?;
    let decoded = ds_command_kernel::project_management::decode_project_graph(&graph, project);
    if !decoded
        .tasks
        .iter()
        .any(|task| task.id == selection.task_id)
    {
        return Err(Failure::invalid(
            "task_not_found",
            "The selected task is absent from this project's captured plan.",
        )
        .detail(json!({"task":selection.task_id,"project":project}))
        .remedy("read this project's task list and choose its exact task ID"));
    }
    let records = acquire(TaskRequest::Records)?;
    ds_client_core::project_management::validate_task_records(
        &records,
        project,
        &selection.task_id,
        100,
    )
    .map_err(map_client)?;
    let mut captured =
        json!({"project_id":project,"selection":selection,"graph":graph,"records_context":records});
    if selection.timeline {
        captured["timeline_receipt"] = acquire(TaskRequest::Timeline)?;
    }
    verify_task_capture(project, selection, &captured)?;
    Ok(captured)
}
fn verify_task_capture(
    project: &str,
    selection: &pm_graph_catalogue::TaskRead,
    value: &Value,
) -> Result<(), Failure> {
    if value["project_id"] != project
        || value["selection"] != json!(selection)
        || selection.timeline != value.get("timeline_receipt").is_some()
    {
        return Err(local(
            "task receipt selection differs from the admitted read",
        ));
    }
    ds_client_core::project_management::verify_graph(project, &value["graph"])
        .map_err(map_client)?;
    ds_client_core::project_management::validate_task_records(
        &value["records_context"],
        project,
        &selection.task_id,
        100,
    )
    .map_err(map_client)?;
    if selection.timeline {
        let receipt = &value["timeline_receipt"];
        if receipt["task"]["id"] != selection.task_id
            || receipt["revision"].as_i64().is_none_or(|n| n < 0)
            || receipt["schedule_revision"].as_i64().is_none_or(|n| n < 0)
            || receipt["timeline"]["task_id"] != selection.task_id
            || !receipt["timeline"]["entries"].is_array()
            || receipt["timeline"]["total"].as_u64().is_none_or(|n| {
                n < receipt["timeline"]["entries"]
                    .as_array()
                    .map_or(0, Vec::len) as u64
            })
            || !receipt["timeline"]["truncated"].is_boolean()
        {
            return Err(local(
                "the timeline does not prove this exact task and bounded completeness",
            ));
        }
    }
    Ok(())
}
fn selected_read(
    lane: &str,
    project: &str,
    note: Option<NoteCommand>,
    task: Option<pm_graph_catalogue::TaskRead>,
    offline: bool,
) -> Result<Value, Failure> {
    let (fence, binding) = binding(lane, project)?;
    let root = ds_layer_store::default_root().map_err(local)?;
    let mut source = Provider {
        lane,
        project,
        fence,
        binding: binding.clone(),
        note: note.clone(),
        task: task.clone(),
    };
    let time =
        i64::try_from(now().saturating_mul(1000)).map_err(|_| local("invalid capture clock"))?;
    let result = if let Some(selection) = &task {
        pm_graph_catalogue::consume_task(&root, &binding, selection, offline, time, &mut source)
    } else {
        match &note {
            Some(NoteCommand::Read(selection)) => pm_graph_catalogue::consume_note(
                &root,
                &binding,
                selection,
                offline,
                time,
                &mut source,
            ),
            Some(NoteCommand::Query(selection)) => pm_graph_catalogue::consume_query(
                &root,
                &binding,
                selection,
                offline,
                time,
                &mut source,
            ),
            Some(NoteCommand::Context(selection)) => pm_graph_catalogue::consume_context(
                &root,
                &binding,
                selection,
                offline,
                time,
                &mut source,
            ),
            Some(_) => {
                return Err(local(
                    "only explicit read selections may enter retained PM rooms",
                ))
            }
            None => pm_graph_catalogue::consume(&root, &binding, offline, time, &mut source),
        }
    };
    match result {
        Ok(value) => Ok(value),
        Err(pm_graph_catalogue::Error::Source(error)) => Err(error),
        Err(pm_graph_catalogue::Error::Local(error)) => Err(local(error)),
        Err(pm_graph_catalogue::Error::AcquisitionRequired) => Err(Failure::unavailable("pm_offline_acquisition_required", "no admitted PM capture matches this protected account, lane, audience, credential, exact project and selected read").remedy("repeat this exact PM read online once; offline never acquires missing, replaced or denied data")),
        Err(pm_graph_catalogue::Error::Superseded) => Err(Failure::conflict("project_context_changed", "the PM capture was superseded by a local mutation or authority change").remedy("repeat the intentional online PM read")),
    }
}
fn may_write(command: &Command) -> bool {
    match command {
        Command::Authored { command, .. } => may_write(command),
        Command::Graph
        | Command::Overview { .. }
        | Command::PublicProjects { .. }
        | Command::TaskComments { .. }
        | Command::TaskRecords { .. }
        | Command::Assignees
        | Command::Context { .. } => false,
        Command::ProjectNotes { command } => !matches!(
            command,
            ds_client_core::project_management::project_notes::Command::Read(_)
                | ds_client_core::project_management::project_notes::Command::PrepareMigration(_)
                | ds_client_core::project_management::project_notes::Command::Query(_)
                | ds_client_core::project_management::project_notes::Command::Context(_)
                | ds_client_core::project_management::project_notes::Command::Versions(_)
        ),
        _ => true,
    }
}
fn correspondence_may_write(action: &ds_client_core::project_correspondence::Action) -> bool {
    use ds_client_core::project_correspondence::Action;
    match action {
        Action::Authored { action, .. } => correspondence_may_write(action),
        Action::NoteChecklistRead { .. }
        | Action::PartyList(_)
        | Action::RecordRead { .. }
        | Action::RecordThread { .. }
        | Action::RecordList(_)
        | Action::TaskRead { .. }
        | Action::DeletionInventory { .. }
        | Action::DeletionRead { .. } => false,
        Action::CopyDocument { dry_run, .. } | Action::MailThreadIngest { dry_run, .. } => !dry_run,
        _ => true,
    }
}
pub(super) fn before_correspondence(
    lane: &str,
    project: &str,
    action: &ds_client_core::project_correspondence::Action,
) -> Result<Binding, Failure> {
    let (_, binding) = binding(lane, project)?;
    if correspondence_may_write(action) {
        pm_graph_catalogue::mark(
            &ds_layer_store::default_root().map_err(local)?,
            &binding,
            false,
        )
        .map_err(local)?;
    }
    Ok(binding)
}
/// A write attempt can partially land before a later error: mark only an existing holding.
pub(super) fn before_command(
    lane: &str,
    project: &str,
    command: &Command,
) -> Result<Binding, Failure> {
    let (_, binding) = binding(lane, project)?;
    if may_write(command) {
        pm_graph_catalogue::mark(
            &ds_layer_store::default_root().map_err(local)?,
            &binding,
            false,
        )
        .map_err(local)?;
    }
    Ok(binding)
}
pub(super) fn after_refusal(binding: &Binding, error: &Failure) -> Result<(), Failure> {
    if !denied(error) {
        return Ok(());
    }
    pm_graph_catalogue::mark(
        &ds_layer_store::default_root().map_err(local)?,
        binding,
        true,
    )
    .map_err(local)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn read_commands_do_not_invalidate_unused_pm_graphs() {
        assert!(!may_write(&Command::Graph));
        assert!(!may_write(&Command::TaskRecords {
            task_id: "t".into(),
            limit: 100
        }));
        assert!(may_write(&Command::Commit {
            command_id: "c".into(),
            base_revision: 1,
            command: json!({})
        }));
        assert!(denied(&Failure::unauthorized("forbidden", "denied")));
        assert!(!denied(&Failure::unavailable("transient", "retry")));
        assert!(!denied(&Failure::unavailable(
            "not_found",
            "selected note is absent"
        )));
        assert!(denied(&Failure::unavailable(
            "project_not_found",
            "project no longer admitted"
        )));
    }
    fn task_receipts(timeline: bool) -> (pm_graph_catalogue::TaskRead, Value, Value, Value) {
        let selection = pm_graph_catalogue::TaskRead {
            task_id: "t".into(),
            timeline,
        };
        let graph = json!({"project_id":"p","revision":7,"schedule_revision":2,"tasks":[{"id":"t","data":{"title":"Review"}}],"dependencies":[],"residual_items":[],"calendar":{},"permissions":{"view":true,"comment":false,"contribute":false,"edit_schedule":false,"admin":false}});
        let context = json!({"project_id":"p","task_id":"t","records":[],"loaded_collections":["pm_records"],"truncated_by_collection":{"pm_records":true},"next_cursors":{"pm_records":"boundary"}});
        let receipt = json!({"task":{"id":"t"},"revision":8,"schedule_revision":3,"timeline":{"task_id":"t","entries":[],"total":0,"truncated":true}});
        (selection, graph, context, receipt)
    }
    #[test]
    fn task_capture_consumes_only_selected_records_and_requested_timeline() {
        for timeline in [false, true] {
            let (selection, graph, context, receipt) = task_receipts(timeline);
            let mut calls = vec![];
            let capture = acquire_task("p", &selection, |request| {
                calls.push(request);
                Ok(match request {
                    TaskRequest::Graph => graph.clone(),
                    TaskRequest::Records => context.clone(),
                    TaskRequest::Timeline => receipt.clone(),
                })
            })
            .unwrap();
            assert_eq!(
                calls,
                if timeline {
                    vec![
                        TaskRequest::Graph,
                        TaskRequest::Records,
                        TaskRequest::Timeline,
                    ]
                } else {
                    vec![TaskRequest::Graph, TaskRequest::Records]
                }
            );
            assert_eq!(capture["graph"]["revision"], 7);
            assert!(capture["records_context"].get("revision").is_none());
            assert_eq!(
                capture["records_context"]["truncated_by_collection"]["pm_records"],
                true
            );
            assert_eq!(capture.get("timeline_receipt").is_some(), timeline);
            verify_task_capture("p", &selection, &capture).unwrap();
        }
    }
    #[test]
    fn task_missing_or_failed_records_cannot_acquire_unused_sources() {
        let (mut selection, graph, context, _) = task_receipts(true);
        selection.task_id = "missing".into();
        let mut calls = vec![];
        let error = acquire_task("p", &selection, |request| {
            calls.push(request);
            assert_eq!(request, TaskRequest::Graph);
            Ok(graph.clone())
        })
        .unwrap_err();
        assert_eq!(error.code(), "task_not_found");
        assert_eq!(calls, vec![TaskRequest::Graph]);
        assert!(!denied(&error));
        selection.task_id = "t".into();
        calls.clear();
        let error = acquire_task("p", &selection, |request| {
            calls.push(request);
            match request {
                TaskRequest::Graph => Ok(graph.clone()),
                TaskRequest::Records => Err(Failure::unauthorized("forbidden", "Denied")),
                TaskRequest::Timeline => panic!("denied records do not acquire timeline"),
            }
        })
        .unwrap_err();
        assert_eq!(calls, vec![TaskRequest::Graph, TaskRequest::Records]);
        assert!(denied(&error));
        let mut wrong = context;
        wrong["task_id"] = json!("other");
        assert!(acquire_task("p", &selection, |request| match request {
            TaskRequest::Graph => Ok(graph.clone()),
            TaskRequest::Records => Ok(wrong.clone()),
            TaskRequest::Timeline => panic!("wrong scope blocks timeline"),
        })
        .is_err());
    }
    #[test]
    fn task_receipt_admission_refuses_selection_scope_and_missing_completeness() {
        let (selection, graph, context, receipt) = task_receipts(true);
        let valid = json!({"project_id":"p","selection":selection,"graph":graph,"records_context":context,"timeline_receipt":receipt});
        verify_task_capture("p", &selection, &valid).unwrap();
        for pointer in [
            "/selection/task_id",
            "/graph/project_id",
            "/records_context/task_id",
            "/timeline_receipt/task/id",
            "/timeline_receipt/timeline/task_id",
        ] {
            let mut wrong = valid.clone();
            *wrong.pointer_mut(pointer).unwrap() = json!("other");
            assert!(
                verify_task_capture("p", &selection, &wrong).is_err(),
                "{pointer}"
            );
        }
        let mut wrong = valid.clone();
        wrong["records_context"]["truncated_by_collection"] = Value::Null;
        assert!(verify_task_capture("p", &selection, &wrong).is_err());
        let mut wrong = valid.clone();
        wrong.as_object_mut().unwrap().remove("timeline_receipt");
        assert!(verify_task_capture("p", &selection, &wrong).is_err());
        let other = pm_graph_catalogue::TaskRead {
            task_id: "t".into(),
            timeline: false,
        };
        assert!(verify_task_capture("p", &other, &valid).is_err());
    }
    #[test]
    fn shared_note_migration_prepare_is_read_only_and_apply_invalidates() {
        use ds_command_kernel::project_management::project_notes::{
            migration, Command as NoteCommand, MEMORY_SCHEMA,
        };
        assert!(!may_write(&Command::ProjectNotes {
            command: NoteCommand::PrepareMigration(migration::Prepare { id: "n".into() })
        }));
        let plan:migration::Plan=serde_json::from_value(json!({"schema":"ds.project-note-canonicalize/v1","project_id":"p","id":"n","expected_version":7,"source_sha256":"a".repeat(64),"source_update_time":"2026-10-13T10:15:00Z","target_sha256":"b".repeat(64),"retain_observed_current":true,"memory":{"schema":MEMORY_SCHEMA,"note_type":"observation","lifecycle":"active","provenance":{"kind":"observation"},"scope":{"kind":"conditions","conditions":["unclassified historical observation"]},"claims":[],"evidence":[]}})).unwrap();
        plan.validate().unwrap();
        assert!(may_write(&Command::ProjectNotes {
            command: NoteCommand::ApplyMigration(migration::Apply {
                operation_id: "adopt".into(),
                plan
            })
        }));
    }
    #[test]
    fn correspondence_task_writes_invalidate_but_intentional_reads_do_not() {
        use ds_client_core::project_correspondence::Action;
        assert!(!correspondence_may_write(&Action::TaskRead {
            task_id: "t".into(),
            timeline: true
        }));
        assert!(!correspondence_may_write(&Action::NoteChecklistRead {
            id: "n".into()
        }));
        assert!(!correspondence_may_write(&Action::DeletionInventory {
            limit: None,
            cursor: None
        }));
        assert!(!correspondence_may_write(&Action::RecordRead {
            record_id: "r".into()
        }));
        assert!(correspondence_may_write(&Action::TaskBlock {
            command_id: "c".into(),
            base_revision: 1,
            task_id: "t".into(),
            record_id: "r".into()
        }));
        assert!(correspondence_may_write(&Action::TaskUnblock {
            command_id: "c".into(),
            base_revision: 1,
            task_id: "t".into(),
            record_id: "r".into(),
            reason: "resolved".into()
        }));
        assert!(correspondence_may_write(&Action::RecordUpdate {
            id: "r".into(),
            expected_version: 1,
            data: serde_json::Map::new()
        }));
    }
    #[test]
    fn note_page_bounds_refuse_before_any_credential_or_project_acquisition() {
        let query: NoteQuery=serde_json::from_value(json!({"terms":[],"tags":[],"tag_match":"any","note_types":[],"lifecycle":["active"],"sort":"updated","direction":"desc","limit":51})).unwrap();
        assert_eq!(
            read_query("invalid-unused-lane", "p", &query, false)
                .unwrap_err()
                .code(),
            "project_note_invalid"
        );
        assert_eq!(
            read_context(
                "invalid-unused-lane",
                "p",
                &NoteContext {
                    query: query.clone(),
                    scope: None
                },
                true
            )
            .unwrap_err()
            .code(),
            "project_note_invalid"
        );
        let mut valid = query;
        valid.limit = 20;
        assert!(!may_write(&Command::ProjectNotes {
            command: NoteCommand::Query(valid.clone())
        }));
        assert!(!may_write(&Command::ProjectNotes {
            command: NoteCommand::Context(NoteContext {
                query: valid,
                scope: None
            })
        }));
    }
}
