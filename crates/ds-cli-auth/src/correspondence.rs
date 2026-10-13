//! The correspondence door (ds-brain `docs/contracts/correspondence.md`):
//! parties, records, threads and task blockers on `POST /api/v1/pm`, for only
//! the saved, audience-fenced selected project — and the one rule for how a
//! named refusal from that door, or from the assets catalogue, becomes the
//! failure a `ds pm` or `ds assets` command documents.
//!
//! ## The refusal rule
//!
//! Both routes refuse BY NAME: `PM_REFUSED` / `ASSET_REFUSED` carry
//! `details.reason` set to a token the contract lists —
//! `record_source_required`, `party_exists`, `asset_bytes_not_held`, … — and
//! the ids or numbers the rule names (`record_id`, `bound`, `external_url`).
//! The native client relays that token AS THE CODE, so a caller plans
//! against the contract's own vocabulary rather than a paraphrase, and puts
//! every named detail under `detail`. The token is not chosen here: the
//! closed list, its `when` and its remedy live with the commands that
//! declare it (`ds-cli-pm::CORRESPONDENCE_REFUSALS`, the assets domain's
//! own), and each domain's classifier renames a token it does not declare to
//! its generic refusal — so a token the server gains tomorrow can never ship
//! undocumented through here. The two concurrency envelopes keep `ds pm`'s
//! names: `PM_REVISION_CONFLICT` and `PM_CONTEXT_VERSION_CONFLICT` are both
//! `work_revision_conflict` (re-read, decide again); a 401/403 is
//! `work_not_permitted`; a 404 with no token is `project_not_visible`.

use ds_cli_contract::Failure;
use serde_json::Value;

use super::{headless_named_project_with, map_client, now, pm_graph_source, HeadlessNamedProject};

/// One correspondence action for the project named on this request.
pub fn project_correspondence_for_project(
    lane_value: &str,
    project: &str,
    action: &ds_client_core::project_correspondence::Action,
) -> Result<HeadlessNamedProject<Value>, Failure> {
    let binding = pm_graph_source::before_correspondence(lane_value, project, action)?;
    let result = headless_named_project_with(
        lane_value,
        project,
        |error| map_selected_refusal(action, error),
        |device, project| device.project_correspondence(project, action),
        |client, project| client.project_correspondence(project, action, now()),
    );
    if let Err(error) = &result {
        pm_graph_source::after_refusal(&binding, error)?;
    }
    result
}

/// Only the named selected-task refusal is row absence; generic HTTP status never changes authority.
fn map_selected_refusal(
    action: &ds_client_core::project_correspondence::Action,
    error: ds_client_core::ClientError,
) -> Failure {
    if selected_task_absence(action, error.service_refusal()) {
        return Failure::invalid(
            "task_not_found",
            "The selected task read was not found at its source.",
        )
        .remedy("read the current project task list and choose its exact task ID");
    }
    map_client(error)
}
fn selected_task_absence(
    action: &ds_client_core::project_correspondence::Action,
    refusal: Option<&ds_client_core::ServiceRefusal>,
) -> bool {
    matches!(
        action,
        ds_client_core::project_correspondence::Action::TaskRead { .. }
    ) && refusal
        .is_some_and(|refusal| refusal.status() == 404 && refusal.code() == Some("task_not_found"))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn task_read_resource_absence_requires_exact_named_server_receipt() {
        let action = ds_client_core::project_correspondence::Action::TaskRead {
            task_id: "t".into(),
            timeline: true,
        };
        let refusal = ds_client_core::ServiceRefusal::new(
            404,
            Some("task_not_found"),
            Some("Selected task not found"),
        );
        assert!(selected_task_absence(&action, Some(&refusal)));
        assert!(!selected_task_absence(&action, None));
        for status in [401, 403, 409] {
            let refusal = ds_client_core::ServiceRefusal::new(status, Some("task_not_found"), None);
            assert!(!selected_task_absence(&action, Some(&refusal)));
        }
        let generic = ds_client_core::ServiceRefusal::new(404, Some("not_found"), None);
        assert!(!selected_task_absence(&action, Some(&generic)));
        let other = ds_client_core::project_correspondence::Action::RecordRead {
            record_id: "r".into(),
        };
        assert!(!selected_task_absence(&other, Some(&refusal)));
    }
}
