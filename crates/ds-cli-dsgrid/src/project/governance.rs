//! `ds dsgrid project bump-version|update|set-approval|backup download` —
//! governance of one project model that uploads nothing.
//!
//! Every write is fenced on the head the caller reviewed (`--expected-head`)
//! and refused, never retried, when the head moved. A bump starts the next
//! version from the head's exact bytes; a rename touches the head only;
//! a review decision is appended beside an immutable revision. The backup
//! read is the retirement backup, verified against the head's digest.
use super::{LANE, LOCAL, PROJECT, SHARED, with_shared};
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

const REFUSALS: &[Refusal] = &with_shared([LOCAL; 1 + SHARED], 1);
const MODEL: Arg = Arg::value("model", "<id>", "Exact project model ID.").required();
const EXPECTED_HEAD: Arg = Arg::value(
    "expected-head",
    "<revision-id>",
    "The head you reviewed; a moved head is refused, never retried.",
)
.required();

pub static BUMP_VERSION: Command = Command {
    id: "dsgrid.project.bump-version",
    path: &["dsgrid", "project", "bump-version"],
    contract: 1,
    summary: "Start the next version from the current head (needs --yes).",
    purpose: "Close the current version for new saves and start v(n+1) when the design goes out, without uploading anything: the new version's first revision re-pins the head's exact bytes. Use it when the content to submit is already the head; to submit new content, publish it with dsgrid publish-version --bump-version instead. The approval restarts as draft. One head starts at most one next version; an exact retry returns it.",
    chapter: Chapter::GridModel,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        MODEL,
        EXPECTED_HEAD,
        Arg::value(
            "reason",
            "<text>",
            "Why the version starts (at most 2000 characters).",
        )
        .required(),
        Arg::value("milestone", "<text>", "Milestone label, e.g. Submission 2."),
    ],
    output: "status version_started, the new version, its first revision id (ordinal 1), the head it was cut from, the unchanged model digest, milestone, reason and draft approval.",
    examples: &[Example {
        command: "ds dsgrid project bump-version --project <p> --model <m> --expected-head <rev> --reason \"Submitted to REG\" --milestone \"Submission 2\" --yes",
        note: "Start v2 from the reviewed head without new content.",
        runnable: false,
    }],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["new version", "submission", "close version"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static UPDATE: Command = Command {
    id: "dsgrid.project.update",
    path: &["dsgrid", "project", "update"],
    contract: 1,
    summary: "Rename or re-describe one project model (needs --yes).",
    purpose: "Change the display name or description a project model is known by, fenced on the reviewed head. Revisions are never touched: each keeps the name it was saved under, and the next save takes the new one. An empty --description clears it. A retired model is refused; restore it first.",
    chapter: Chapter::GridModel,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        MODEL,
        EXPECTED_HEAD,
        Arg::value("name", "<text>", "New display name (1 to 200 characters)."),
        Arg::value("description", "<text>", "New description; empty clears it."),
        Arg::value("reason", "<text>", "Why, for the audit record."),
    ],
    output: "The model head after the change, changed, and changes[] naming the fields moved; revisions_changed=false.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["rename model", "model name"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static SET_APPROVAL: Command = Command {
    id: "dsgrid.project.set-approval",
    path: &["dsgrid", "project", "set-approval"],
    contract: 1,
    summary: "Record a review decision on one model revision (needs --yes).",
    purpose: "Approve, reject, submit or return to draft an existing revision after the fact — the head or a historic one — fenced on the reviewed head. The revision stays immutable: the decision is appended beside it with who, when and what it replaced, and dsgrid project show --revision reports the effective approval and every decision. Every decision needs a reason and the approving capability; approved and rejected also need a level.",
    chapter: Chapter::GridModel,
    effect: Effect::GlobalWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        MODEL,
        Arg::value("revision", "<id>", "Exact revision the decision is about.").required(),
        EXPECTED_HEAD,
        Arg::value("status", "<status>", "The decision.")
            .choices(&["draft", "submitted", "approved", "rejected"])
            .required(),
        Arg::value("reason", "<text>", "Why (1 to 2000 characters).").required(),
        Arg::value(
            "level",
            "<id>",
            "Approval level id; required for approved or rejected.",
        ),
    ],
    output: "revision, its version and ordinal, model digest, effective_approval, the decision appended (or the matching one on a repeat) and changed.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["approve revision", "reject revision", "review decision"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub static BACKUP_DOWNLOAD: Command = Command {
    id: "dsgrid.project.backup.download",
    path: &["dsgrid", "project", "backup", "download"],
    contract: 1,
    summary: "Download a retired model's backup, digest-verified.",
    purpose: "Read the byte-verified backup taken when a model head was retired (it stays after a restore) and write it to a new local .dsgrid, checked against the head's SHA-256 and byte length. Needs the same capability as retire and restore; a model never retired has no backup and is refused.",
    chapter: Chapter::GridModel,
    effect: Effect::LocalFileWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: &[
        PROJECT,
        LANE,
        MODEL,
        Arg::value("out", "<file.dsgrid>", "Fresh local package path.").required(),
    ],
    output: "model, state, head revision, verified sha256, byte count, backup generation and the local path. No signed locator.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/dsgrid.md"),
    search: &["retired model backup", "recover retired"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

fn call(i: &Inputs, command: &ds_cli_auth::GridModelsCommand) -> Result<Value, Failure> {
    Ok(
        ds_cli_auth::grid_models_for_project(i.require("lane")?, i.require("project")?, command)?
            .data,
    )
}

pub fn bump_version(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    call(
        i,
        &ds_cli_auth::GridModelsCommand::BumpVersion {
            model: i.require("model")?.into(),
            expected_head: i.require("expected-head")?.into(),
            reason: i.require("reason")?.into(),
            milestone: i.value("milestone").map(str::to_owned),
        },
    )
}

pub fn update(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    if i.value("name").is_none() && i.value("description").is_none() {
        return Err(Failure::invalid(
            "grid_project_output_invalid",
            "name --name, --description or both",
        )
        .remedy("pass the field to change; nothing else is written"));
    }
    call(
        i,
        &ds_cli_auth::GridModelsCommand::UpdateModel {
            model: i.require("model")?.into(),
            expected_head: i.require("expected-head")?.into(),
            display_name: i.value("name").map(str::to_owned),
            description: i.value("description").map(str::to_owned),
            reason: i.value("reason").map(str::to_owned),
        },
    )
}

pub fn set_approval(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    call(
        i,
        &ds_cli_auth::GridModelsCommand::SetApproval {
            model: i.require("model")?.into(),
            revision: i.require("revision")?.into(),
            expected_head: i.require("expected-head")?.into(),
            approval: ds_command_kernel::grid_publication::Approval {
                status: i.require("status")?.into(),
                level_id: i.value("level").map(str::to_owned),
                decision_reason: Some(i.require("reason")?.into()),
            },
        },
    )
}

pub fn backup_download(i: &Inputs, _: &Context) -> Result<Value, Failure> {
    let out = std::path::Path::new(i.require("out")?);
    if std::fs::symlink_metadata(out).is_ok() {
        return Err(
            Failure::invalid("grid_project_output_invalid", "destination already exists")
                .remedy(LOCAL.remedy),
        );
    }
    let mut receipt = ds_cli_auth::grid_models_for_project(
        i.require("lane")?,
        i.require("project")?,
        &ds_cli_auth::GridModelsCommand::DownloadBackup {
            model: i.require("model")?.into(),
        },
    )?;
    let bytes = receipt.bytes.take().ok_or_else(|| {
        Failure::invalid(
            "grid_project_output_invalid",
            "verified owner returned no bytes",
        )
        .remedy(LOCAL.remedy)
    })?;
    super::exports::write_new(out, &bytes)?;
    receipt.data["out"] = json!(out);
    Ok(receipt.data)
}

pub fn render(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}
