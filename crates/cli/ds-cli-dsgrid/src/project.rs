//! Governed model discovery and exact-byte download through the native owner.
//!
//! A project model has versions (v1, v2, …) and every save is a revision of
//! the version current when it was saved; only an explicit bump starts the
//! next version. `show` and `versions` read that history; each
//! revision may also carry immutable exports ([`exports`]).
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};
use std::io::Write;

pub mod exports;
pub mod governance;

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
const fn list_refusals() -> [Refusal; 4 + SHARED] {
    let mut r = [LOCAL; 4 + SHARED];
    r[1] = Refusal {
        code: "project_model_catalogue_acquisition_required",
        when: "current project heads are not retained for this authenticated scope",
        remedy: "repeat this consuming read while connected",
    };
    r[2] = Refusal {
        code: "mv_model_catalogue_invalid",
        when: "project heads are incomplete, duplicated or invalid",
        remedy: "explicitly refresh this project's model catalogue",
    };
    r[3] = Refusal {
        code: "mv_model_source_stale",
        when: "captured inventory account/project/history differs",
        remedy: "capture a new declared held inventory",
    };
    with_shared(r, 4)
}
const LIST_REFUSALS: &[Refusal] = &list_refusals();
const RETIRE_BACKUP: Refusal = Refusal {
    code: "grid_retire_backup_unconfirmed",
    when: "the Server did not confirm a byte-verified backup of the exact head (it could not take one, or its answer was lost or lacked it); nothing is reported retired",
    remedy: "run dsgrid project list --include-deleted: retry a head still active unchanged; read a retired head's backup with dsgrid project backup download",
};
const RETIRE_REFUSALS: &[Refusal] = &with_shared([RETIRE_BACKUP; 1 + SHARED], 1);
pub(crate) const LANE: ds_cli_contract::spec::Arg =
    ds_cli_contract::spec::LANE.summary("Native authentication lane.");
pub(crate) const PROJECT: Arg =
    Arg::value("project", "<ds-project>", "Exact project for this request.").required();
pub static RETIRE: Command = Command {
    id: "dsgrid.project.retire",
    path: &["dsgrid", "project", "retire"],
    contract: 1,
    summary: "Retire one superseded project DS Grid model (needs --yes).",
    purpose: "Retires the exact model head in the explicitly named project after comparing its revision and digest. The Server first copies and verifies the head's bytes into a separate backup; without one nothing is retired. Immutable revisions and model bytes remain available for lineage. A changed head is refused.",
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
    output: "The retired model and pinned head, deletion time, confirmation that immutable revisions and model bytes were retained, and backup {bucket, object, generation, digest, byte_length}: the verified copy restore and backup download read.",
    examples: &[],
    refusals: RETIRE_REFUSALS,
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
    contract: 2,
    summary: "List one explicitly named project's saved MV models headlessly.",
    purpose: "Discover governed DS Grid model heads for MV maps and Solar network seeding without a Desktop. Returns exact revision and digest identifiers and each model's current version (head_version, the app's v1/v2), approval, stage and update/retirement fields. Follow next_cursor when more is true, including an empty page. Local unpublished models are outside it. --mv-picker reads bounded complete head metadata through the native cache; --held-inventory maps exact current working copies without opening or changing them.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalAuthState,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "source-binding",
            "<json>",
            "Exact native selected source; requires --mv-picker and --held-inventory.",
        ),
        Arg::switch(
            "mv-picker",
            "Current logical MV heads and exact held-copy choices.",
        ),
        Arg::value(
            "held-inventory",
            "<json>",
            "Declared map.profile.view --include-held output; requires --mv-picker.",
        ),
        Arg::switch(
            "refresh",
            "Explicitly refresh --mv-picker project-head metadata.",
        ),
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
    output: "Selected project, bounded heads with revision/digest/version and governance fields; more/next_cursor. --mv-picker returns ds.mv-model-catalogue/v1 logical heads, copy state and exact source/candidates (optional selected_source); no model download.",
    examples: &[],
    refusals: LIST_REFUSALS,
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
    purpose: "Read one model head without downloading bytes: its current version, that version's revision count and the head's ordinal in it, plus approval, stage, detail and update fields. With --revision, read one revision's metadata instead: parent, reason, milestone, approval, digest, publisher, time, migration and composition sources. A retired model reads as not found; restore it first or name one of its revisions.",
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
    output: "Head: model {head_revision_id, head_version, head_model_digest, approval_status, design_stage_id, updated_at, …}, current_version {version, version_revision_count, revision_ordinal_within_version, count_exact}, and management {working_head {revision_id, digest, revision_count}, named_version {version, label}, content_version, mutated:false}. named_version is the LV/MV marker (0 = Unversioned); content_version is the legacy source group. Revision: the record and its position in its version. Never a signed locator.",
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
fn owned_json(path: &str, limit: u64) -> Result<Value, Failure> {
    use std::io::Read;
    let metadata = std::fs::symlink_metadata(path).map_err(failure)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > limit {
        return Err(failure(
            "The declared input must be a bounded regular JSON file.",
        ));
    }
    let mut bytes = Vec::new();
    ds_layer_store::private::open_options()
        .read(true)
        .open(path)
        .map_err(failure)?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(failure)?;
    if bytes.len() as u64 > limit {
        return Err(failure("The declared input grew beyond its byte bound."));
    }
    serde_json::from_slice(&bytes).map_err(failure)
}
fn held_inventory(value: Value) -> Result<ds_command_kernel::grid_held_models::Inventory, Failure> {
    let value = if value.get("command").is_some() {
        if value["command"] != "map.profile.view" || value["status"] != "ok" {
            return Err(failure(
                "Use a successful map.profile.view --include-held result.",
            ));
        }
        value["data"]["held_models"].clone()
    } else {
        value.get("held_models").cloned().unwrap_or(value)
    };
    ds_command_kernel::grid_held_models::admit(value)
        .map_err(|e| Failure::invalid("mv_model_source_stale", e))
}
pub fn list(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    if i.switch("mv-picker") {
        let source: Option<ds_command_kernel::printing::mv_models::SourceBinding> = i
            .value("source-binding")
            .map(|path| {
                let value = owned_json(path, 16 * 1024)?;
                let source = serde_json::from_value(value).map_err(failure)?;
                ds_command_kernel::printing::mv_models::admit_context(
                    i.require("project")?,
                    &source,
                )
                // The kernel admission has one refusal; the message stays its code.
                .map_err(|code| Failure::invalid("mv_model_source_stale", code))?;
                Ok::<_, Failure>(source)
            })
            .transpose()?;

        if i.value("cursor").is_some() || i.switch("include-deleted") {
            return Err(failure("mv-picker excludes cursor and retired heads"));
        }
        if source.is_some() && i.value("held-inventory").is_none() {
            return Err(failure(
                "source-binding requires its declared held-inventory",
            ));
        }
        let inventory = i
            .value("held-inventory")
            .map(|path| held_inventory(owned_json(path, 4 * 1024 * 1024)?))
            .transpose()?;
        let captured = ds_cli_auth::grid_mv_catalogue::read(
            i.require("lane")?,
            i.require("project")?,
            ds_command_kernel::printing::retained_reads::Demand {
                online: true,
                acquire: true,
                refresh: i.switch("refresh"),
            },
            inventory,
            source,
        )?;
        return Ok(captured.into_result());
    }
    if i.value("held-inventory").is_some()
        || i.value("source-binding").is_some()
        || i.switch("refresh")
    {
        return Err(failure("held-inventory and refresh require mv-picker"));
    }
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
    let expected_digest = i.require("expected-digest")?;
    let answer = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        project,
        &ds_cli_auth::GridModelsCommand::Delete {
            model: i.require("model")?.into(),
            expected_revision: i.require("expected-head")?.into(),
            expected_digest: expected_digest.into(),
            reason: i.require("reason")?.into(),
        },
    )
    .map(|receipt| receipt.data);
    retired(answer, expected_digest)
}
/// A retirement is reported only with the backup of the exact head.
///
/// The Server copies the head's bytes into its separate backup bucket and
/// verifies them before it retires the head; when it cannot, it answers 5xx
/// and leaves the head active. That answer used to reach the caller as
/// `auth_transient`, and nothing checked the answer named its backup.
fn retired(answer: Result<Value, Failure>, expected_digest: &str) -> Result<Value, Failure> {
    let unconfirmed = |message: &str, detail: Value| {
        Failure::unavailable(RETIRE_BACKUP.code, message)
            .detail(detail)
            .remedy(RETIRE_BACKUP.remedy)
            .next("ds dsgrid project list --project <exact-id> --include-deleted")
    };
    let data = answer.map_err(|failure| match failure.code() {
        "auth_transient" | "auth_response_unreadable" => unconfirmed(
            "the Server did not confirm a verified backup of the head; the retirement is not confirmed",
            json!({"cause_code": failure.code(), "cause_message": failure.message()}),
        ),
        _ => failure,
    })?;
    let backup = &data["backup"];
    let named = |key: &str| backup[key].as_str().is_some_and(|v| !v.is_empty());
    let positive = |key: &str| backup[key].as_i64().is_some_and(|v| v > 0);
    if backup["digest"] != expected_digest
        || !named("bucket")
        || !named("object")
        || !positive("generation")
        || !positive("byte_length")
    {
        return Err(unconfirmed(
            "the retirement answer named no verified backup of the exact head",
            json!({"backup": backup}),
        ));
    }
    Ok(data)
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

#[cfg(test)]
#[path = "project_tests.rs"]
mod tests;
