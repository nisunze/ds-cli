//! Status and recovery of the paired Desktop's native publication store, through
//! the kernel's `ds.sync-recovery/v1` control that Sync Center also sends.
use crate::discover::Descriptor;
use crate::ops::{self, BridgeOp, DESCRIPTOR_ARG, TARGET_ARG};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, ArgKind, Authority, Chapter, Command, Effect, Execution, Refusal, Requires},
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
    &crate::published::OP,
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
        when: "the retained snapshot or the owner changed after preview, or a live worker holds the project",
        remedy: "preview sanitation again under the intended account and project, then apply its exact new digest",
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
        when: "the paired application has no signed-in session that owns the store",
        remedy: "sign in to DS GridDesign with the account that produced the local results",
    },
    Refusal {
        code: "sync_account_changed",
        when: "the signed-in account changes while the local queue operation is running",
        remedy: "read status again under the intended account before retrying a row",
    },
    Refusal {
        code: "sync_row_not_found",
        when: "the exact row is absent from this account's store for the echoed project",
        remedy: "run `ds desktop sync status --project <project-id>` and use its exact row id",
    },
    Refusal {
        code: "sync_row_not_retryable",
        when: "the row has no recoverable typed authority verdict or readable immutable bytes",
        remedy: "follow the retained verdict in status; stale, integrity and permission failures cannot be retried",
    },
];

pub static STATUS_COMMAND: Command = Command {
    id: "desktop.sync.status",
    path: &["desktop", "sync", "status"],
    contract: 1,
    summary: "See which local results still wait to publish, across projects.",
    purpose: "Reads the native publication store Sync Center renders, for every engine (Network Reporter and Solar). Each row gives its project, engine, operation, variant, state (held, uploading, published, conflict or refused) with the stored reason, and its output digests and sizes. Byte locators and upload sessions never leave the application; a project whose store cannot be read is reported unavailable, never as an empty queue.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[PROJECT_ARG, LIMIT_ARG, TARGET_ARG, DESCRIPTOR_ARG],
    output: "Optional project filter, per-state summary, completeness with unavailable projects, total/more and bounded path-free rows.",
    examples: &[],
    refusals: REFUSALS,
    reference: None,
    search: &[],
    requires: Requires::Window,
    availability: ops::paired_availability,
};

pub static RETRY_COMMAND: Command = Command {
    id: "desktop.sync.retry",
    path: &["desktop", "sync", "retry"],
    contract: 1,
    summary: "Retry one refused retained publication (needs --yes).",
    purpose: "Sends the kernel recovery control for one exact account- and project-fenced row under the Desktop's signed-in session. Only a conflict or refusal with a typed authority verdict, readable immutable bytes and exact producer identity is requeued. The same outputs, digests and producer build are published again; old work is never regenerated or relabelled, and the server rechecks authority.",
    chapter: Chapter::Operations,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[REQUIRED_PROJECT_ARG, ROW_ARG, TARGET_ARG, DESCRIPTOR_ARG],
    output: "The kernel recovery result for the row. A retry is queued, not reported as published.",
    examples: &[],
    refusals: REFUSALS,
    reference: None,
    search: &[],
    requires: Requires::Window,
    availability: ops::paired_availability,
};

pub static SANITIZE_PREVIEW_COMMAND: Command = Command {
    id: "desktop.sync.sanitize.preview",
    path: &["desktop", "sync", "sanitize", "preview"],
    contract: 1,
    summary: "Preview recovery and archival of one project’s retained results.",
    purpose: "Uses the kernel recovery control Sync Center and `ds server sync sanitize` send. Published rows and failures fully superseded by newer local outputs are archived; recoverable failures are requeued; active uploads, unregistered engines and failures that need attention are kept. Files and producer identities are never deleted. The digest covers the complete retained snapshot even when the display is truncated.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[REQUIRED_PROJECT_ARG, LIMIT_ARG, TARGET_ARG, DESCRIPTOR_ARG],
    output: "Digest for the complete snapshot, total/more and bounded per-row decisions with reasons. No rows or files change.",
    examples: &[],
    refusals: REFUSALS,
    reference: None,
    search: &[],
    requires: Requires::Window,
    availability: ops::paired_availability,
};

pub static SANITIZE_APPLY_COMMAND: Command = Command {
    id: "desktop.sync.sanitize.apply",
    path: &["desktop", "sync", "sanitize", "apply"],
    contract: 1,
    summary: "Apply inspected sync sanitation without deleting files (needs --yes).",
    purpose: "Applies the decisions of an unchanged preview through the kernel recovery control: archives published and fully superseded rows and requeues recoverable failures. Files and producer identities are never deleted; any change since the preview refuses.",
    chapter: Chapter::Operations,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        REQUIRED_PROJECT_ARG,
        Arg::value(
            "digest",
            "<sha256>",
            "Exact complete preview digest; refuses any changed snapshot.",
        )
        .required(),
        TARGET_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "Applied decisions and bounded rows. Recovery is queued, not reported as published; archived rows and bytes remain in history.",
    examples: &[],
    refusals: REFUSALS,
    reference: None,
    search: &[],
    requires: Requires::Window,
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
        args.insert(
            "project".into(),
            json!(crate::project_id(project, "sync_invalid_input")?),
        );
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
    let project = crate::project_id(inputs.require("project")?, "sync_invalid_input")?;
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
    let project = crate::project_id(inputs.require("project")?, "sync_invalid_input")?;
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
    let project = crate::project_id(inputs.require("project")?, "sync_invalid_input")?;
    let digest = text(inputs.require("digest")?, "digest", 64)?;
    if !ds_cli_contract::util::is_sha256_hex(&digest, ds_cli_contract::util::HexCase::Lower) {
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
