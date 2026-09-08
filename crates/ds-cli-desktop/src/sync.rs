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
pub const BRIDGE_OPS: &[&BridgeOp] = &[&STATUS_OP, &RETRY_OP];

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
        when: "the row is not a retained Network Reporter engine-build admission failure",
        remedy: "repair exact build admission before retry; create a fresh export for stale or integrity failures",
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
    summary: "Retry one retained Network Reporter admission failure (needs --yes).",
    purpose: "Requeues only the exact account- and project-fenced row after its producer release and build-manifest digest have been admitted by the server. The same client run, native batch, output declarations, byte digests and resumable progress are preserved; this command never relabels or regenerates old work.",
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
