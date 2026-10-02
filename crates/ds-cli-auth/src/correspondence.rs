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

use super::{HeadlessNamedProject, headless_named_project, now};

/// One correspondence action for the project named on this request.
pub fn project_correspondence_for_project(
    lane_value: &str,
    project: &str,
    action: &ds_client_core::project_correspondence::Action,
) -> Result<HeadlessNamedProject<Value>, Failure> {
    headless_named_project(
        lane_value,
        project,
        |device, project| device.project_correspondence(project, action),
        |client, project| client.project_correspondence(project, action, now()),
    )
}
