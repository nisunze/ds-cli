//! Account-owned compute publication status and exact-row recovery through the
//! paired application's Sync Center owner.
use crate::discover::Descriptor;
use crate::ops::{self, BridgeOp, DESCRIPTOR_ARG};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, ArgKind, Authority, Chapter, Command, Effect, Execution, Refusal},
};
use serde_json::{Map, Value, json};
use std::time::Duration;

pub const STATUS_OP: BridgeOp = BridgeOp {
    operation: "compute.sync.status",
    arguments: &["project", "limit"],
};
pub const RETRY_OP: BridgeOp = BridgeOp {
    operation: "compute.sync.retry",
    arguments: &["project", "row"],
};
pub const SANITIZE_PREVIEW_OP: BridgeOp = BridgeOp {
    operation: "compute.sync.sanitize.preview",
    arguments: &["project", "limit"],
};
pub const SANITIZE_APPLY_OP: BridgeOp = BridgeOp {
    operation: "compute.sync.sanitize.apply",
    arguments: &["project", "digest"],
};
pub const BRIDGE_OPS: &[&BridgeOp] = &[
    &STATUS_OP,
    &RETRY_OP,
    &SANITIZE_PREVIEW_OP,
    &SANITIZE_APPLY_OP,
];

const PROJECT_ARG: Arg = Arg::value(
    "project",
    "<project-id>",
    "Filter status to one retained project. Required for retry as an exact project echo.",
);
const REQUIRED_PROJECT_ARG: Arg = Arg::value(
    "project",
    "<project-id>",
    "Exact project identity returned on the retained row by status.",
)
.required();
const ROW_ARG: Arg = Arg::value(
    "row",
    "<id>",
    "Exact retained publication row identity returned by status.",
)
.required();
const LIMIT_ARG: Arg = Arg {
    name: "limit",
    kind: ArgKind::Value,
    value: "<count>",
    required: false,
    default: Some("50"),
    choices: &[],
    summary: "Rows in one account-owned page (1-200); total and more are always reported.",
};

const REFUSALS: &[Refusal] = &[
    Refusal {
        code: "sync_sanitation_refused",
        when: "the inspected queue changed, the owner changed, or its bounded plan is unavailable",
        remedy: "inspect sanitation again under the intended account/project, then apply its exact unchanged digest",
    },
    ops::NOT_PAIRED,
    ops::AMBIGUOUS,
    ops::UNREACHABLE,
    ops::PAIRING_REJECTED,
    ops::REFUSED,
    ops::UNSUPPORTED,
    ops::UNREADABLE,
    ops::SIGNED_OUT,
    Refusal {
        code: "sync_invalid_input",
        when: "a limit, project identity, or retained row identity is malformed",
        remedy: "use the exact project and row fields returned by `ds desktop sync status`",
    },
    Refusal {
        code: "sync_signed_out",
        when: "the paired application no longer has an account that owns the queue",
        remedy: "sign in to DS GridDesign with the account that produced the local reports",
    },
    Refusal {
        code: "sync_account_changed",
        when: "the signed-in account changes while the local queue operation is running",
        remedy: "read status again under the intended account before retrying a row",
    },
    Refusal {
        code: "sync_row_not_found",
        when: "the exact row is absent or does not belong to the echoed account and project",
        remedy: "run `ds desktop sync status --project <project-id>` and use its exact row id",
    },
    Refusal {
        code: "sync_row_not_retryable",
        when: "the row is not an eligible retained Network Reporter admission or storage-comparison failure",
        remedy: "follow status guidance: repair exact build admission or install the server storage-comparison fix; stale or integrity failures remain blocked",
    },
];

pub static STATUS_COMMAND: Command = Command {
    id: "desktop.sync.status",
    path: &["desktop", "sync", "status"],
    contract: 1,
    summary: "Inspect retained local compute publications across projects.",
    purpose: "Reads the same account-owned compute-artifact outbox rendered by Sync Center. It returns exact producer release and build-manifest identities, project and native batch identity, attempts, upload progress and bounded errors. Local artifact locators and resumable upload credentials never leave the application.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, LIMIT_ARG, DESCRIPTOR_ARG],
    output: "Optional project filter, queue summary, total/more and bounded path-free publication rows with exact producer identities and retry guidance.",
    examples: &[],
    refusals: REFUSALS,
    reference: None,
    availability: ops::paired_availability,
};

pub static RETRY_COMMAND: Command = Command {
    id: "desktop.sync.retry",
    path: &["desktop", "sync", "retry"],
    contract: 1,
    summary: "Retry one recoverable retained Network Reporter failure (needs --yes).",
    purpose: "Requeues only the exact account- and project-fenced row after its exact producer build has been admitted or the server storage-comparison bug has been repaired. The server rechecks immutable declarations and publication authority. The same client run, native batch, output declarations, byte digests and resumable progress are preserved; this command never relabels or regenerates old work.",
    chapter: Chapter::Operations,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[REQUIRED_PROJECT_ARG, ROW_ARG, DESCRIPTOR_ARG],
    output: "The exact requeued receipt and the current post-drain row when retained; an explicit unknown-status note is returned if the row was removed.",
    examples: &[],
    refusals: REFUSALS,
    reference: None,
    availability: ops::paired_availability,
};

pub static SANITIZE_PREVIEW_COMMAND: Command = Command {
    id: "desktop.sync.sanitize.preview",
    path: &["desktop", "sync", "sanitize", "preview"],
    contract: 1,
    summary: "Inspect recovery and archival of one project’s retained reports.",
    purpose: "Uses the same sanitation owner as Sync Center. Published history and superseded terminal Network Reporter rows may be archived while all immutable local files, producer identities and queue receipts remain. Current known admission/storage-comparison failures may be requeued; active uploads, unrelated workflows and unresolved current failures remain unchanged. At most 4096 retained project rows are inspected; applying the digest covers the complete plan even when the display is truncated.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[REQUIRED_PROJECT_ARG, LIMIT_ARG, DESCRIPTOR_ARG],
    output: "Digest for the complete queue snapshot, total/more, action counts and bounded decisions. No rows or files change.",
    examples: &[],
    refusals: REFUSALS,
    reference: None,
    availability: ops::paired_availability,
};

pub static SANITIZE_APPLY_COMMAND: Command = Command {
    id: "desktop.sync.sanitize.apply",
    path: &["desktop", "sync", "sanitize", "apply"],
    contract: 1,
    summary: "Apply inspected sync sanitation without deleting files (needs --yes).",
    purpose: "Uses the same sanitation owner as Sync Center. Published history and superseded terminal Network Reporter rows may be archived while all immutable local files, producer identities and queue receipts remain. Current known admission/storage-comparison failures may be requeued; active uploads, unrelated workflows and unresolved current failures remain unchanged. At most 4096 retained project rows are inspected; applying the digest covers the complete plan even when the display is truncated.",
    chapter: Chapter::Operations,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        REQUIRED_PROJECT_ARG,
        Arg::value(
            "digest",
            "<sha256>",
            "Exact complete preview digest; refuses any changed queue.",
        )
        .required(),
        DESCRIPTOR_ARG,
    ],
    output: "Applied decision counts and bounded rows. Recovery is queued, not reported as published; archived rows and bytes remain in history.",
    examples: &[],
    refusals: REFUSALS,
    reference: None,
    availability: ops::paired_availability,
};

fn descriptor(inputs: &Inputs) -> Result<Descriptor, Failure> {
    ops::paired(inputs.value("desktop-descriptor"))
}

fn text(value: &str, label: &str, max: usize) -> Result<String, Failure> {
    let value = value.trim();
    if value.is_empty() || value.len() > max {
        return Err(Failure::invalid(
            "sync_invalid_input",
            format!("--{label} must be 1-{max} characters"),
        ));
    }
    Ok(value.to_string())
}

pub fn status(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = inputs
        .value("limit")
        .unwrap_or("50")
        .parse::<u16>()
        .map_err(|_| {
            Failure::invalid(
                "sync_invalid_input",
                "--limit must be an integer from 1 to 200",
            )
        })?;
    if !(1..=200).contains(&limit) {
        return Err(Failure::invalid(
            "sync_invalid_input",
            "--limit must be an integer from 1 to 200",
        ));
    }
    let mut args = Map::from_iter([("limit".to_string(), json!(limit))]);
    if let Some(project) = inputs.value("project") {
        args.insert("project".into(), json!(text(project, "project", 128)?));
    }
    ops::invoke(
        &descriptor(inputs)?,
        &STATUS_OP,
        Value::Object(args),
        Duration::from_secs(60),
    )
    .map_err(ops::classify_signed_out)
}

pub fn retry(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = text(inputs.require("project")?, "project", 128)?;
    let row = text(inputs.require("row")?, "row", 512)?;
    ops::invoke(
        &descriptor(inputs)?,
        &RETRY_OP,
        json!({"project": project, "row": row}),
        Duration::from_secs(1850),
    )
    .map_err(ops::classify_signed_out)
}

pub fn sanitize_preview(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = text(inputs.require("project")?, "project", 128)?;
    let limit = inputs
        .value("limit")
        .unwrap_or("50")
        .parse::<u16>()
        .ok()
        .filter(|v| (1..=200).contains(v))
        .ok_or_else(|| Failure::invalid("sync_invalid_input", "--limit must be 1-200"))?;
    ops::invoke(
        &descriptor(inputs)?,
        &SANITIZE_PREVIEW_OP,
        json!({"project":project,"limit":limit}),
        Duration::from_secs(60),
    )
    .map_err(ops::classify_signed_out)
}
pub fn sanitize_apply(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = text(inputs.require("project")?, "project", 128)?;
    let digest = text(inputs.require("digest")?, "digest", 64)?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(Failure::invalid(
            "sync_invalid_input",
            "--digest must be the exact preview SHA-256",
        ));
    }
    ops::invoke(
        &descriptor(inputs)?,
        &SANITIZE_APPLY_OP,
        json!({"project":project,"digest":digest}),
        Duration::from_secs(60),
    )
    .map_err(ops::classify_signed_out)
}
pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operations_are_closed_and_retry_preserves_explicit_project_fence() {
        assert_eq!(STATUS_OP.arguments, &["project", "limit"]);
        assert_eq!(RETRY_OP.arguments, &["project", "row"]);
        assert_eq!(STATUS_COMMAND.effect, Effect::ReadOnly);
        assert_eq!(STATUS_COMMAND.authority, Authority::DesktopUser);
        assert_eq!(RETRY_COMMAND.effect, Effect::GlobalWrite);
        assert_eq!(RETRY_COMMAND.authority, Authority::DesktopUser);
    }
}
