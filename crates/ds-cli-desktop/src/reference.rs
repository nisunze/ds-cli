//! Governed Rwanda reference catalog through the paired Desktop.
use crate::ops::{self, BridgeOp, DESCRIPTOR_ARG};
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Authority, Chapter, Command, Effect, Execution},
};
use serde_json::{Map, Value, json};
use std::time::Duration;

pub const STATUS_OP: BridgeOp = BridgeOp {
    operation: "reference.rwanda.status",
    arguments: &[],
};
pub const SEED_OP: BridgeOp = BridgeOp {
    operation: "reference.rwanda.seed",
    arguments: &["resources"],
};

const RESOURCE_ARG: ds_cli_contract::spec::Arg = ds_cli_contract::spec::Arg::repeated(
    "resource",
    "<sha256-id>",
    "Catalog resource to install; repeat for a subset. Omit to install every available Rwanda resource.",
);

const REFUSALS: &[ds_cli_contract::spec::Refusal] = &[
    ops::NOT_PAIRED,
    ops::AMBIGUOUS,
    ops::UNREACHABLE,
    ops::PAIRING_REJECTED,
    ops::REFUSED,
    ops::UNSUPPORTED,
    ops::UNREADABLE,
    ops::SIGNED_OUT,
];

pub static STATUS_COMMAND: Command = Command {
    id: "desktop.reference.rwanda.status",
    path: &["desktop", "reference", "rwanda", "status"],
    contract: 1,
    summary: "Prove local completeness of the active project's Rwanda catalog.",
    purpose: "Refreshes the active project's governed Brain catalog and checks the exact installed version and spatial index of every published Rwanda resource. The resource list is dynamic; no road, settlement, school, infrastructure or later layer is embedded in this command.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[DESCRIPTOR_ARG],
    output: "Project and country, resource counts, exact ready/incomplete/unavailable totals, aggregate installed bytes/features and a per-resource receipt.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/desktop.reference.md"),
    availability: ops::paired_availability,
};

pub static SEED_COMMAND: Command = Command {
    id: "desktop.reference.rwanda.seed",
    path: &["desktop", "reference", "rwanda", "seed"],
    contract: 1,
    summary: "Download and index selected governed Rwanda catalog resources.",
    purpose: "Refreshes the active project's Brain catalog, downloads all available Rwanda resources or an explicit repeated --resource subset, and asks the Desktop to build and verify each local SQLite RTree. Completed resources resume safely. Uninstalled catalog rows remain explicit and never block map or print rendering.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[RESOURCE_ARG, DESCRIPTOR_ARG],
    output: "The bounded catalog receipt, including compressed transfer requirements and each resource's expanded cache, spatial-index and total disk sizes.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/desktop.reference.md"),
    availability: ops::paired_availability,
};

fn invoke(inputs: &Inputs, operation: &BridgeOp, timeout: Duration) -> Result<Value, Failure> {
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(&descriptor, operation, json!({}), timeout).map_err(ops::classify_signed_out)
}
pub fn status(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(inputs, &STATUS_OP, Duration::from_secs(180))
}
pub fn seed(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let resources = inputs.repeated("resource");
    if resources.iter().any(|value| {
        value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }) {
        return Err(Failure::invalid(
            "invalid_reference_resource",
            "--resource must be a 64-character lowercase SHA-256 catalog ID",
        ));
    }
    let mut arguments = Map::new();
    if !resources.is_empty() {
        arguments.insert("resources".into(), json!(resources));
    }
    let descriptor = ops::paired(inputs.value("desktop-descriptor"))?;
    ops::invoke(
        &descriptor,
        &SEED_OP,
        Value::Object(arguments),
        Duration::from_secs(7200),
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
    fn catalog_is_dynamic_and_bridge_arguments_are_closed() {
        assert_eq!(STATUS_OP.arguments, &[] as &[&str]);
        assert_eq!(SEED_OP.arguments, &["resources"]);
        assert_eq!(STATUS_COMMAND.effect, Effect::ReadOnly);
        assert_eq!(SEED_COMMAND.effect, Effect::LocalFileWrite);
    }
}
