//! Governed model discovery and exact-byte download through the native owner.
//!
//! A project model has versions (v1, v2, …) and every save is a revision of
//! the version current when it was saved; only an explicit bump starts the
//! next version. `show`, `versions` and `compare` read that history; each
//! revision may also carry immutable exports ([`exports`]).
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use std::io::Write;

pub mod compare;
pub mod exports;
pub mod governance;
pub use compare::COMMAND as COMPARE;

const LOCAL: Refusal = Refusal {
    code: "grid_project_output_invalid",
    when: "the destination exists or cannot be written, or paging input is invalid",
    remedy: "use a fresh .dsgrid path and a page limit from 1 to 100",
};
/// Refusals every project-model call can return beyond its own.
pub(crate) const SHARED: usize =
    ds_cli_auth::GRID_MODEL_REFUSALS.len() + ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len();
/// Fill `r[at..]` with the shared refusals; `r[..at]` keeps the command's own.
pub(crate) const fn with_shared<const M: usize>(mut r: [Refusal; M], at: usize) -> [Refusal; M] {
    let mut g = 0;
    while g < ds_cli_auth::GRID_MODEL_REFUSALS.len() {
        r[at + g] = ds_cli_auth::GRID_MODEL_REFUSALS[g];
        g += 1;
    }
    let mut p = 0;
    while p < ds_cli_auth::PROJECT_STATUS_COMMAND.refusals.len() {
        r[at + g + p] = ds_cli_auth::PROJECT_STATUS_COMMAND.refusals[p];
        p += 1;
    }
    r
}
const REFUSALS: &[Refusal] = &with_shared([LOCAL; 1 + SHARED], 1);
pub(crate) const LANE: Arg = Arg::value("lane", "<stable|canary>", "Native authentication lane.")
    .default("stable")
    .choices(&["stable", "canary"]);
pub(crate) const PROJECT: Arg =
    Arg::value("project", "<ds-project>", "Exact project for this request.").required();
pub static RETIRE: Command = Command {
    id: "dsgrid.project.retire",
    path: &["dsgrid", "project", "retire"],
    contract: 1,
    summary: "Retire one superseded project DS Grid model (needs --yes).",
    purpose: "Retires the exact model head in the explicitly named project after comparing its revision and digest. Immutable revisions and model bytes remain available for lineage. A changed head is refused.",
    chapter: Chapter::GridModel,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("model", "<id>", "Exact project model ID.").required(),
        Arg::value(
            "expected-head",
            "<revision-id>",
            "Head revision inspected before retirement.",
        )
        .required(),
        Arg::value(
            "expected-digest",
            "<sha256>",
            "64-character head model digest inspected before retirement.",
        )
        .required(),
        Arg::value("reason", "<text>", "Why this model is superseded.").required(),
    ],
    output: "The retired model and pinned head, deletion time, and confirmation that immutable revisions and model bytes were retained.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["retire model", "superseded model"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static RESTORE: Command = Command {
    id: "dsgrid.project.restore",
    path: &["dsgrid", "project", "restore"],
    contract: 1,
    summary: "Restore one backed-up project DS Grid model (needs --yes).",
    purpose: "Reactivates the exact retired head after the Server verifies its separate backup and immutable model bytes. Attachments remain pinned to their original version. A changed head is refused.",
    chapter: Chapter::GridModel,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("model", "<id>", "Exact retired project model ID.").required(),
        Arg::value("expected-head", "<revision-id>", "Retired head revision.").required(),
        Arg::value("expected-digest", "<sha256>", "Retired head digest.").required(),
        Arg::value("reason", "<text>", "Why this model is being restored.").required(),
    ],
    output: "Restored model and pinned head, verified backup identity, and tile invalidation.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["restore model", "recover model"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static LIST: Command = Command {
    id: "dsgrid.project.list",
    path: &["dsgrid", "project", "list"],
    contract: 1,
    summary: "List one explicitly named project's saved MV models headlessly.",
    purpose: "Discover governed DS Grid model heads for MV maps and Solar network seeding without a Desktop. Returns exact revision and digest identifiers and each model's current version (head_version, the app's v1/v2), approval, stage and update/retirement fields. Follow next_cursor when more is true, including an empty page. Local unpublished Desktop models are outside this inventory.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("limit", "<1..100>", "Maximum catalog rows scanned.").default("50"),
        Arg::value(
            "cursor",
            "<opaque>",
            "Exact next cursor from the previous page.",
        ),
        Arg::switch(
            "include-deleted",
            "Include retired model heads so they can be restored.",
        ),
    ],
    output: "Selected project, bounded models with head revision/digest, head_version, approval_status, updated_at/by, deleted_at/by and delete_reason, more and next_cursor.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static VERSIONS: Command = Command {
    id: "dsgrid.project.versions",
    path: &["dsgrid", "project", "versions"],
    contract: 2,
    summary: "List a project DS Grid model's versions and their saved revisions.",
    purpose: "Groups one catalog page by version, newest version first; inside a version its revisions run in save order (parent chain) with revision_ordinal_within_version. Includes versions of a retired model. Attachments and exports stay with their own revision; download one exact revision with dsgrid project download.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("model", "<id>", "Exact project model ID.").required(),
        Arg::value("limit", "<1..100>", "Maximum versions in one page.").default("50"),
        Arg::value(
            "cursor",
            "<opaque>",
            "Exact next cursor from the previous page.",
        ),
    ],
    output: "versions[] {version, revisions_listed, ordinals_exact, revisions[] (each catalog row verbatim plus revision_ordinal_within_version)}, revisions_listed, more and next_cursor. The last group of a page with more=true may continue on the next page; its ordinals are null.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["model history", "model versions"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static DOWNLOAD: Command = Command {
    id: "dsgrid.project.download",
    path: &["dsgrid", "project", "download"],
    contract: 1,
    summary: "Download and verify one project MV model without a Desktop.",
    purpose: "Resolve an exact governed revision under the selected project, download its immutable .dsgrid bytes and verify the declared SHA-256 and byte count before creating a new local file. Use the resulting package for model inspection, tagged MV quantities and map composition. No storage URL or project override is accepted.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("model", "<id>", "Exact model ID from project list.").required(),
        Arg::value("revision", "<id>", "Exact immutable revision ID.").required(),
        Arg::value("out", "<file.dsgrid>", "Fresh local package path.").required(),
    ],
    output: "Selected project, model/revision, verified SHA-256, byte count and local path. No signed locator.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &[],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub static SHOW: Command = Command {
    id: "dsgrid.project.show",
    path: &["dsgrid", "project", "show"],
    contract: 1,
    summary: "Show one project DS Grid model: current version and head.",
    purpose: "Read one model head without downloading bytes: its current version, how many revisions that version holds and the head's ordinal in it, plus approval, stage, detail and update fields. With --revision, read one revision's metadata instead: parent, reason, milestone, approval, digest, publisher, time, migration and composition sources. A retired model reads as not found; restore it first or name one of its revisions.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        Arg::value("model", "<id>", "Exact project model ID.").required(),
        Arg::value(
            "revision",
            "<id>",
            "Show this revision's metadata instead of the head (no bytes fetched).",
        ),
    ],
    output: "Head: model {head_revision_id, head_version, head_model_digest, approval_status, design_stage_id, updated_at, …} and current_version {version, version_revision_count, revision_ordinal_within_version, count_exact}. Revision: the revision record and its position in its version. Never a signed locator.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["model head", "current version", "revision metadata"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};
pub fn show(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let lane = i.require("lane")?;
    let project = i.require("project")?;
    let model = i.require("model")?;
    let Some(revision) = i.value("revision") else {
        return Ok(ds_cli_auth::grid_models_for_project(
            lane,
            project,
            &ds_cli_auth::GridModelsCommand::Show {
                model: model.into(),
            },
        )?
        .data);
    };
    let mut shown = ds_cli_auth::grid_models_for_project(
        lane,
        project,
        &ds_cli_auth::GridModelsCommand::ShowVersion {
            model: model.into(),
            revision: revision.into(),
        },
    )?
    .data;
    // The position is a second, bounded read. It cannot change what the
    // revision is, so a walk that fails is reported, not raised.
    shown["position"] = match ds_cli_auth::grid_models_for_project(
        lane,
        project,
        &ds_cli_auth::GridModelsCommand::Position {
            model: model.into(),
            revision: revision.into(),
        },
    ) {
        Ok(position) => position.data,
        Err(error) => json!({"unavailable": error.code(), "message": error.to_string()}),
    };
    Ok(shown)
}
fn failure(e: impl std::fmt::Display) -> Failure {
    Failure::invalid("grid_project_output_invalid", e.to_string()).remedy(LOCAL.remedy)
}
pub fn list(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = i.require("limit")?.parse::<u16>().map_err(failure)?;
    let r = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::GridModelsCommand::List {
            limit,
            cursor: i.value("cursor").map(str::to_owned),
            include_deleted: i.switch("include-deleted"),
        },
    )?;
    Ok(r.data)
}
pub fn versions(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = i.require("limit")?.parse::<u16>().map_err(failure)?;
    let r = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::GridModelsCommand::ListVersions {
            model: i.require("model")?.into(),
            limit,
            cursor: i.value("cursor").map(str::to_owned),
        },
    )?;
    Ok(r.data)
}
pub fn download(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let out = std::path::Path::new(i.require("out")?);
    if std::fs::symlink_metadata(out).is_ok() {
        return Err(failure("destination already exists"));
    }
    let r = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::GridModelsCommand::Download {
            model: i.require("model")?.into(),
            revision: i.require("revision")?.into(),
        },
    )?;
    let mut r = r;
    let bytes = r
        .bytes
        .take()
        .ok_or_else(|| failure("verified owner returned no package"))?;
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(parent).map_err(failure)?;
    let mut staged = tempfile::NamedTempFile::new_in(parent).map_err(failure)?;
    staged.write_all(&bytes).map_err(failure)?;
    staged.as_file().sync_all().map_err(failure)?;
    staged.persist_noclobber(out).map_err(failure)?;
    r.data["out"] = serde_json::json!(out);
    Ok(r.data)
}
pub fn retire(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = i.require("project")?;
    let receipt = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        project,
        &ds_cli_auth::GridModelsCommand::Delete {
            model: i.require("model")?.into(),
            expected_revision: i.require("expected-head")?.into(),
            expected_digest: i.require("expected-digest")?.into(),
            reason: i.require("reason")?.into(),
        },
    )?;
    Ok(receipt.data)
}
pub fn restore(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let receipt = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::GridModelsCommand::Restore {
            model: i.require("model")?.into(),
            expected_revision: i.require("expected-head")?.into(),
            expected_digest: i.require("expected-digest")?.into(),
            reason: i.require("reason")?.into(),
        },
    )?;
    Ok(receipt.data)
}
pub fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}
