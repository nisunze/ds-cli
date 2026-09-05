//! Local durable queue controls use the same kernel storage host as Sync Center.
use crate::{BridgeOp, DESCRIPTOR_ARG, LIMIT_ARG};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution},
};
use serde_json::{Value, json};

pub const STATUS_OP: BridgeOp = BridgeOp {
    operation: "design.sync.status",
    arguments: &["limit"],
};
pub const CANCEL_OP: BridgeOp = BridgeOp {
    operation: "design.sync.cancel",
    arguments: &["operation"],
};
pub const RESUME_OP: BridgeOp = BridgeOp {
    operation: "design.sync.resume",
    arguments: &["operation"],
};
const OPERATION: Arg = Arg::value(
    "operation",
    "<id>",
    "Exact retained operation identity from sync status.",
)
.required();
const fn command(
    id: &'static str,
    path: &'static [&'static str],
    summary: &'static str,
    effect: Effect,
    args: &'static [Arg],
) -> Command {
    Command {
        id,
        path,
        contract: 1,
        summary,
        purpose: "Inspect or control the active project's durable reconciliation queue in the paired application's local storage. The shared Rust kernel owns state transitions and exact request identity. No network is contacted. Cancellation retains payloads and evidence; an attempted remote operation remains cancellation-requested until its outcome is known. Resume preserves the original request and does not promise transport availability.",
        chapter: Chapter::Design,
        effect,
        authority: Authority::Project,
        execution: Execution::Sync,
        args,
        output: "Project, bounded operation rows with phase, attempts and last retained event; status includes counts and explicit more/total. Control returns the exact operation's new state.",
        examples: &[],
        refusals: &[
            crate::NOT_PAIRED,
            crate::AMBIGUOUS,
            crate::UNREACHABLE,
            crate::PAIRING_REJECTED,
            crate::DESIGN_REFUSED,
            crate::UNSUPPORTED,
            crate::UNREADABLE,
            crate::SIGNED_OUT,
            crate::NOT_PERMITTED,
            crate::INVALID_NUMBER,
            crate::CONFLICT,
            crate::CONFIRMATION_REQUIRED,
        ],
        reference: None,
        availability: crate::paired_availability,
    }
}
pub static STATUS: Command = command(
    "design.sync.status",
    &["design", "sync", "status"],
    "Inspect retained reconciliation operations offline.",
    Effect::ReadOnly,
    &[LIMIT_ARG, DESCRIPTOR_ARG],
);
pub static CANCEL: Command = command(
    "design.sync.cancel",
    &["design", "sync", "cancel"],
    "Cancel delivery while retaining local work and evidence.",
    Effect::GlobalWrite,
    &[OPERATION, DESCRIPTOR_ARG],
);
pub static RESUME: Command = command(
    "design.sync.resume",
    &["design", "sync", "resume"],
    "Resume the same immutable retained request.",
    Effect::GlobalWrite,
    &[OPERATION, DESCRIPTOR_ARG],
);
fn invoke(inputs: &Inputs, op: &BridgeOp, args: Value) -> Result<Value, Failure> {
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(&descriptor, op, args, crate::READ_TIMEOUT)
        .map_err(crate::classify_design_failure)
}
pub fn status(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = crate::integer(
        inputs.value("limit").unwrap_or("50"),
        "limit",
        1,
        crate::MAX_PAGE_SIZE,
    )?;
    invoke(inputs, &STATUS_OP, json!({"limit":limit}))
}
pub fn cancel(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(
        inputs,
        &CANCEL_OP,
        json!({"operation":inputs.require("operation")?}),
    )
}
pub fn resume(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(
        inputs,
        &RESUME_OP,
        json!({"operation":inputs.require("operation")?}),
    )
}
pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default()
}
