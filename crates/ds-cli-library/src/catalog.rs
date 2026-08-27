use ds_cli_contract::spec::{
    Arg, Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Failure, Inputs};
use ds_grid_exchange::parse_catalog;
use serde_json::{Value, json};

use crate::{engine_failure, read, sha256};

pub static COMMAND: Command = Command {
    id: "library.catalog",
    path: &["library", "catalog"],
    contract: 1,
    summary: "Read and validate a library catalogue without downloading releases.",
    purpose: "Parses one explicit catalogue and returns its exact immutable release coordinates. It never chooses latest or performs network access.",
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::value("catalog", "<path>", "Catalogue JSON path.").required()],
    output: "Catalogue id and bounded release descriptors with exact digests.",
    examples: &[Example {
        command: "ds library catalog --catalog ./catalog.json --output json",
        note: "Inspect local catalogue metadata.",
        runnable: false,
    }],
    refusals: &[Refusal {
        code: "library_catalog_invalid",
        when: "the catalogue is malformed or not schema v1",
        remedy: "use a catalogue emitted by the governed library service",
    }],
    reference: Some("docs/reference/library.md"),
    availability: || Availability::Available,
};

pub fn run(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let path = inputs.require("catalog")?;
    let bytes = read(path)?;
    let catalog =
        parse_catalog(&bytes).map_err(|error| engine_failure("library_catalog_invalid", error))?;
    Ok(json!({
        "path": path,
        "catalog_id": catalog.catalog_id,
        "schema_version": catalog.catalog_schema_version,
        "releases": catalog.releases,
        "execution_owner": "ds",
        "deterministic_completion": "the explicit catalogue bytes were parsed and bounded; no release was selected or downloaded",
        "pls_cadd_ui_handoff": { "required": false, "condition": "Only a later native solver/check or explicit visual acceptance requires PLS-CADD.", "artifact": path, "digest": format!("sha256:{}", sha256(&bytes)), "post_ui_reimport": "Re-import any native-saved workspace as a new authority candidate." },
        "engineer_decision": "Engineer selects the exact release and its certification scope; catalogue order is not authority."
    }))
}
pub fn render(data: &Value) -> String {
    format!(
        "catalog {} — {} release(s)",
        data["catalog_id"].as_str().unwrap_or("?"),
        data["releases"].as_array().map_or(0, Vec::len)
    )
}
