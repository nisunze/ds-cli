//! Read the existing shared publication through the paired application's owner.
use crate::ops::{self, BridgeOp, DESCRIPTOR_ARG};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, ArgKind, Authority, Chapter, Command, Effect, Execution, Refusal},
};
use serde_json::{Value, json};
use std::time::Duration;
pub const OP: BridgeOp = BridgeOp {
    operation: "compute.sync.published",
    arguments: &["project", "engine", "operation", "variant", "output_id"],
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
        when: "a publication selector exceeds its bound or engine is unsupported",
        remedy: "use the exact project, engine, operation and variant shown by Sync Center",
    },
    Refusal {
        code: "sync_signed_out",
        when: "the paired application is signed out",
        remedy: "sign in to the intended account",
    },
    Refusal {
        code: "sync_account_changed",
        when: "the account changes during the read",
        remedy: "repeat under the intended signed-in account",
    },
    Refusal {
        code: "sync_publication_unavailable",
        when: "the shared head, output, or verified download is unavailable",
        remedy: "inspect Sync Center publication state and reconnect; local readiness alone does not publish an artifact",
    },
];
pub static COMMAND: Command = Command {
    id: "desktop.sync.published",
    path: &["desktop", "sync", "published"],
    contract: 1,
    summary: "Read a shared publication or verify one downloaded output.",
    purpose: "Reads the existing shared compute-artifact head independently of this machine's outbox and local files. With --output-id it downloads that generation-pinned output, checks SHA-256 and byte count, and returns a bounded verification receipt. It never renders or republishes. The same web download parser owns canonical filenames and immutable print metadata. Signed URLs, credentials and bytes do not leave the application; each verification is limited to 64 MiB.",
    chapter: Chapter::Operations,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "project",
            "<project-id>",
            "Exact project whose shared record should be inspected.",
        )
        .required(),
        Arg {
            name: "engine",
            kind: ArgKind::Value,
            value: "<engine>",
            required: false,
            default: Some("network_reporter"),
            choices: &["network_reporter", "solar"],
            summary: "Engine that owns the published head.",
        },
        Arg::value(
            "operation",
            "<operation-id>",
            "Exact operation from Sync Center, such as export-agasharu.",
        )
        .required(),
        Arg {
            name: "variant",
            kind: ArgKind::Value,
            value: "<variant>",
            required: false,
            default: Some("default"),
            choices: &[],
            summary: "Exact output variant returned by Sync Center.",
        },
        Arg::value(
            "output-id",
            "<output-id>",
            "Verify this published output's downloaded bytes; omit to inspect the head only.",
        ),
        DESCRIPTOR_ARG,
    ],
    output: "Shared head and at most 100 declared outputs. With output-id: canonical filename, immutable output metadata, verified SHA-256 and byte count; verified means a real shared download passed integrity checks.",
    examples: &[],
    refusals: REFUSALS,
    reference: None,
    availability: ops::paired_availability,
};
fn bounded(value: &str, max: usize) -> Result<String, Failure> {
    let value = value.trim();
    if value.is_empty() || value.len() > max {
        return Err(Failure::invalid(
            "sync_invalid_input",
            "Publication selector is empty or exceeds its bound",
        ));
    }
    Ok(value.into())
}
pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let project = bounded(inputs.require("project")?, 128)?;
    let operation = bounded(inputs.require("operation")?, 138)?;
    let variant = bounded(inputs.value("variant").unwrap_or("default"), 128)?;
    let engine = inputs.value("engine").unwrap_or("network_reporter");
    if !matches!(engine, "network_reporter" | "solar") {
        return Err(Failure::invalid(
            "sync_invalid_input",
            "Engine must be network_reporter or solar",
        ));
    }
    let mut args =
        json!({"project":project,"engine":engine,"operation":operation,"variant":variant});
    if let Some(output) = inputs.value("output-id") {
        args["output_id"] = json!(bounded(output, 128)?);
    }
    ops::invoke(
        &ops::paired(inputs.value("desktop-descriptor"))?,
        &OP,
        args,
        Duration::from_secs(240),
    )
    .map_err(ops::classify_signed_out)
}
