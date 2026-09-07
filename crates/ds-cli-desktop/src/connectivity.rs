//! Same device-local offline command as Settings. No alternate policy in CLI,
//! and no reach beyond the application: headless `ds` keeps its own network.
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
        purpose: "\
Use the paired DS GridDesign application's offline test switch. It isolates that \
application's own network IO only: while it is on, headless `ds` commands that reach \
the DS API (auth, report, survey, tile) still use the network, so the switch does not \
simulate a disconnected product. Enabling requires app-level all governance. Disabling \
remains available to recover connectivity. Local data is retained and ordinary remote \
authorization still applies after reconnecting.",
        chapter: Chapter::Project,
        effect,
        authority: Authority::DesktopUser,
        execution: Execution::Sync,
        args,
        output: "enabled (the application's explicit offline switch), online (that \
application's effective connectivity, never this CLI process's).",
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
    "Enable or disable the paired application's offline test switch.",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contract_scopes_the_switch_to_the_application_not_the_whole_product() {
        // The switch was measured isolating the application while every
        // headless command still reached the DS API. "shared command-kernel"
        // read as a policy this process honours; the kernel only adjudicates
        // who may enable it. The extent has to be in the contract text, or the
        // next caller validates offline behaviour against a false result.
        for command in [&STATUS, &SET] {
            assert!(
                !command.purpose.contains("shared"),
                "the purpose must not claim a policy shared with this process"
            );
            assert!(command.purpose.contains("headless"));
            assert!(command.purpose.contains("still use the network"));
            assert!(
                command
                    .purpose
                    .contains("does not simulate a disconnected product")
            );
            assert!(command.summary.contains("paired application's"));
            assert!(command.summary.len() <= 70, "index summaries stay under 70");
        }
        // `online` is the application's connectivity, not the caller's.
        assert!(STATUS.output.contains("never this CLI process's"));
    }
}
