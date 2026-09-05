//! Same device-local offline command as Settings. No alternate policy in CLI.
use crate::ops::{self, BridgeOp, DESCRIPTOR_ARG};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal},
};
use serde_json::{Value, json};
use std::time::Duration;

pub const STATUS_OP: BridgeOp = BridgeOp {
    operation: "desktop.offline.status",
    arguments: &[],
};
pub const SET_OP: BridgeOp = BridgeOp {
    operation: "desktop.offline.set",
    arguments: &["enabled"],
};
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
        purpose: "Use the paired application's shared command-kernel offline test policy. Enabling requires app-level all governance. Disabling remains available to recover connectivity. Local data is retained and ordinary remote authorization still applies after reconnecting.",
        chapter: Chapter::Project,
        effect,
        authority: Authority::DesktopUser,
        execution: Execution::Sync,
        args,
        output: "enabled (explicit offline mode), online (effective connectivity).",
        examples: &[],
        refusals: &[
            ops::NOT_PAIRED,
            ops::AMBIGUOUS,
            ops::UNREACHABLE,
            ops::PAIRING_REJECTED,
            ops::REFUSED,
            ops::UNSUPPORTED,
            ops::UNREADABLE,
            ops::SIGNED_OUT,
            Refusal {
                code: "invalid_argument",
                when: "enabled is not a boolean value",
                remedy: "pass --enabled true or --enabled false",
            },
        ],
        reference: None,
        availability: ops::paired_availability,
    }
}
pub static STATUS: Command = command(
    "desktop.offline.status",
    &["desktop", "offline", "status"],
    "Inspect the paired application's offline test switch.",
    Effect::ReadOnly,
    &[DESCRIPTOR_ARG],
);
pub static SET: Command = command(
    "desktop.offline.set",
    &["desktop", "offline", "set"],
    "Enable or disable the governed offline test switch.",
    Effect::LocalFileWrite,
    &[
        Arg::value("enabled", "<true|false>", "Enable isolation or reconnect.")
            .choices(&["true", "false"])
            .required(),
        DESCRIPTOR_ARG,
    ],
);
pub fn status(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(&descriptor, &STATUS_OP, json!({}), Duration::from_secs(10))
}
pub fn set(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let enabled = match inputs.require("enabled")? {
        "true" => true,
        "false" => false,
        _ => {
            return Err(Failure::invalid(
                "invalid_argument",
                "enabled must be true or false",
            ));
        }
    };
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(
        &descriptor,
        &SET_OP,
        json!({"enabled":enabled}),
        Duration::from_secs(15),
    )
}
pub fn render(data: &Value) -> String {
    serde_json::to_string_pretty(data).unwrap_or_default()
}
