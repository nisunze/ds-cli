//! One explicit live/retained Plan capture through the existing native PM authority.
use super::*;
use ds_client_core::project_management::Command;
use ds_layer_store::pm_graph_catalogue::{self, Binding, Source};
fn local(message: impl ToString) -> Failure {
    Failure::unavailable("pm_retained_unavailable", message.to_string())
        .remedy("read this exact project online under the same native account to retain its plan")
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
            "auth_revoked" | "auth_identity_mismatch" | "not_found" | "project_not_found"
        )
}
struct Provider<'a> {
    lane: &'a str,
    project: &'a str,
    fence: LayerScopeFence,
    binding: Binding,
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
        let captured = project_management_for_project(self.lane, self.project, &Command::Graph)?;
        if captured.identity().uid() != self.binding.uid
            || captured.identity().credential_audience_sha256() != self.binding.audience
            || captured.lane() != self.binding.lane
            || captured.project_id() != self.binding.project
        {
            return Err(Failure::conflict(
                "project_context_changed",
                "the acquired PM graph differs from its protected capture identity",
            ));
        }
        Ok(captured.into_result())
    }
    fn verify(&self, value: &Value) -> Result<(), Failure> {
        ds_client_core::project_management::verify_graph(self.project, value).map_err(map_client)
    }
    fn access_refused(&self, error: &Failure) -> bool {
        denied(error)
    }
}
/// Offline probes protected local identity only, with no session restore, directory or freshness calls.
pub fn read(lane: &str, project: &str, offline: bool) -> Result<Value, Failure> {
    let (fence, binding) = binding(lane, project)?;
    let root = ds_layer_store::default_root().map_err(local)?;
    let mut source = Provider {
        lane,
        project,
        fence,
        binding: binding.clone(),
    };
    let time =
        i64::try_from(now().saturating_mul(1000)).map_err(|_| local("invalid capture clock"))?;
    match pm_graph_catalogue::consume(&root, &binding, offline, time, &mut source) {
        Ok(value) => Ok(value),
        Err(pm_graph_catalogue::Error::Source(error)) => Err(error),
        Err(pm_graph_catalogue::Error::Local(error)) => Err(local(error)),
        Err(pm_graph_catalogue::Error::AcquisitionRequired) => Err(Failure::unavailable("pm_offline_acquisition_required", "no admitted graph capture matches this protected account, lane, audience, credential and exact project").remedy("run ds pm plan for this project online once; offline never acquires missing or denied data")),
        Err(pm_graph_catalogue::Error::Superseded) => Err(Failure::conflict("project_context_changed", "the PM graph capture was superseded by a local mutation or authority change").remedy("repeat the intentional online plan read")),
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
                | ds_client_core::project_management::project_notes::Command::Query(_)
                | ds_client_core::project_management::project_notes::Command::Context(_)
                | ds_client_core::project_management::project_notes::Command::Versions(_)
        ),
        _ => true,
    }
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
    }
}
