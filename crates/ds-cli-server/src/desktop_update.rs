//! Read-only inspection, shared with the installed shell. No acquire/activation.
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::Value;
use std::io::Read;

pub static INSPECT: Command = Command {
    id: "core.update-inspect",
    path: &["core", "update-inspect"],
    contract: 1,
    summary: "Inspect shell or module update identity without installation changes.",
    purpose: "Read bounded held release and installed identity JSON. The Kernel admits exact component/lane/platform metadata, projects recorded Version, Build number and Release date, and decides main application update availability. Missing installed publication fields stay null. Build time is never a release date. This grants no module installation trust and performs no network, download, process launch or activation.",
    chapter: Chapter::Design,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value("component", "<shell|solar|reporter|network>", "Exact existing update owner.").choices(&["shell", "solar", "reporter", "network"]).required(),
        Arg::value("channel", "<stable|canary|local>", "Exact public release lane.").choices(&["stable", "canary", "local"]).required(),
        Arg::value("file", "<json-file>", "Held release, installed identity and platform JSON; bounded regular file at most 160 KiB.").required(),
    ],
    output: "Inspection with component, installed/published {version,buildNumber,releaseDate}, admitted release and optional main application update. Null fields mean not recorded; modules never carry an activation instruction.",
    examples: &[],
    refusals: &[Refusal { code: "desktop_update_invalid", when: "held metadata, component/lane/platform or exact installer identity is malformed, crossed or excessive", remedy: "retain the installed application; supply the exact lane/platform metadata and real recorded identity" }],
    reference: Some("docs/reference/core.md"),
    search: &["automatic", "update", "version", "build", "release"],
    requires: Requires::Server,
    availability: || Availability::Available,
};

pub fn inspect(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    const LIMIT: u64 = 160 * 1024;
    let invalid = |message: String| Failure::invalid("desktop_update_invalid", message);
    let path = inputs.require("file")?;
    let metadata = std::fs::symlink_metadata(path).map_err(|e| invalid(e.to_string()))?;
    if !metadata.is_file() || metadata.len() > LIMIT {
        return Err(invalid(
            "update inspection requires a bounded regular file".into(),
        ));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| invalid(e.to_string()))?
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| invalid(e.to_string()))?;
    if bytes.len() as u64 > LIMIT {
        return Err(invalid(
            "held update inspection grew beyond its bound".into(),
        ));
    }
    let held = serde_json::from_slice(&bytes).map_err(|e| invalid(e.to_string()))?;
    let inspection = ds_command_kernel::desktop_update::inspect(
        inputs.require("component")?,
        inputs.require("channel")?,
        held,
    )
    .map_err(invalid)?;
    serde_json::to_value(inspection)
        .map_err(|e| Failure::invalid("desktop_update_invalid", e.to_string()))
}
