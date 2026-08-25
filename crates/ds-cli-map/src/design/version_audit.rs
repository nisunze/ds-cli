//! Read-only proof of the feature lineage payload the next save would submit.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;
use crate::design::TRANSFORMER_ARG;

pub static COMMAND: Command = Command {
    id: "map.design.version.audit",
    path: &["map", "design", "version", "audit"],
    contract: 1,
    summary: "Preview feature lineage at the next transformer save.",
    purpose: "Computes the exact save-boundary comparison without saving. It reports the deliberate Design Status version separately from the internal cloud concurrency generation and returns bounded v_first/v_last histograms, never raw features.",
    effect: Effect::ReadOnly,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[TRANSFORMER_ARG, DESCRIPTOR_ARG],
    output: "The transformer, deliberate design version, current and prospective concurrency generations, feature and stamp counts, per-layer stamp counts, and v_first/v_last histograms. persisted is always false.",
    examples: &[Example {
        command: "ds map design version audit --transformer agasharu --output json",
        note: "Proves lineage before the separately confirmed project save.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        super::DESIGN_REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::SIGNED_OUT,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let transformer = inputs.require("transformer")?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::DESIGN_VERSION_AUDIT,
        json!({ "transformer": transformer }),
        crate::DESIGN_READ_TIMEOUT,
    )
    .map_err(crate::classify_design_failure)?;
    Ok(json!({
        "project": result["project"],
        "transformer": result["transformer"],
        "dirty": result["dirty"],
        "design_version": result["designVersion"],
        "current_concurrency_generation": result["currentConcurrencyGeneration"],
        "would_concurrency_generation": result["wouldConcurrencyGeneration"],
        "features": result["featureCount"],
        "stamped_features": result["stampedFeatureCount"],
        "stamped_by_layer": result["stampedByLayer"],
        "v_first": result["vFirst"],
        "v_last": result["vLast"],
        "persisted": false,
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "version audit  {}\n  design v{}  -  concurrency generation {} -> {}\n  {} feature(s)  -  {} stamp change(s)  -  persisted false\n",
        data["transformer"].as_str().unwrap_or("?"),
        data["design_version"].as_u64().unwrap_or(0),
        data["current_concurrency_generation"].as_u64().unwrap_or(0),
        data["would_concurrency_generation"].as_u64().unwrap_or(0),
        data["features"].as_u64().unwrap_or(0),
        data["stamped_features"].as_u64().unwrap_or(0),
    )
}
