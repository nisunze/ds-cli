//! `ds solar engine` — which solar engine is installed here.

use std::ffi::OsString;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DISCOVERY_TIMEOUT, DS_SOLAR};

pub static COMMAND: Command = Command {
    id: "solar.engine",
    path: &["solar", "engine"],
    contract: 1,
    summary: "Report the installed solar engine's version and location.",
    purpose: "\
Returns the version of the `ds-solar` engine this machine will use and where \
it was found. Run it to confirm the solar component is installed before \
starting a batch, and to record which engine produced a set of results.",
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "\
The engine's version string and the path `ds` resolved. Note that `ds-solar` \
reports a package version only — it publishes no source-SHA identity, so a \
result cannot be bound to an exact commit the way a reporter artifact can.",
    examples: &[Example {
        command: "ds solar engine --output json",
        note: "Fails with solar_engine_missing where the component is not installed.",
        runnable: false,
    }],
    refusals: &[Refusal {
        code: "solar_engine_missing",
        when: "`ds-solar` cannot be found; it is not a bundled sidecar",
        remedy: "set DS_SOLAR_BIN to a built ds-solar; see docs/reference/solar.md",
    }],
    reference: Some("docs/reference/solar.md"),
    availability,
};

fn availability() -> Availability {
    DS_SOLAR.availability()
}

pub fn run(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // `ds-solar` has no build-info subcommand; clap's `--version` is the only
    // identity it publishes. Saying so in `output` is better than inventing a
    // richer identity that the engine does not actually attest to.
    let completed = DS_SOLAR.call("--version", &[] as &[OsString], DISCOVERY_TIMEOUT)?;
    if !completed.succeeded() {
        return Err(DS_SOLAR.failure_from(&completed, "--version"));
    }
    Ok(json!({
        "binary": "ds-solar",
        "version": completed.stdout.trim(),
        "path": DS_SOLAR.locate().map(|path| path.display().to_string()),
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "{}\n  {}",
        data["version"].as_str().unwrap_or("?"),
        data["path"].as_str().unwrap_or(""),
    )
}
