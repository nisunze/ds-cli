//! `ds solar engine` — which solar engine is installed here.

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
    contract: 2,
    summary: "Report the installed solar engine's version and location.",
    purpose: "\
Returns the immutable build identity of the `ds-solar` engine this machine \
will use and where it was found. Run it to confirm the packaged sidecar before \
starting a batch, and to record which exact engine produced a result.",
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "The engine's ds.engine-build/v1 identity and the path `ds` resolved.",
    examples: &[Example {
        command: "ds solar engine --output json",
        note: "Fails with solar_engine_missing where the component is not installed.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "solar_engine_missing",
            when: "the packaged `ds-solar` sidecar cannot be found",
            remedy: "reinstall DS GridDesign, or set DS_SOLAR_BIN for development",
        },
        Refusal {
            code: "callee_contract_mismatch",
            when: "`ds-solar build-info` does not return its engine identity document",
            remedy: "update ds and DS GridDesign to one matching release",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    availability,
};

fn availability() -> Availability {
    DS_SOLAR.availability()
}

pub fn run(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut identity = DS_SOLAR.call_json("build-info", &[], DISCOVERY_TIMEOUT)?;
    let object = identity.as_object_mut().ok_or_else(|| {
        Failure::failed(
            "callee_contract_mismatch",
            "`ds-solar build-info` did not return an identity object",
        )
        .remedy("update ds and DS GridDesign to one matching release")
    })?;
    if object.get("name").and_then(Value::as_str) != Some("ds-solar-engine") {
        return Err(Failure::failed(
            "callee_contract_mismatch",
            "`ds-solar build-info` returned the wrong engine identity",
        )
        .remedy("update ds and DS GridDesign to one matching release"));
    }
    object.insert(
        "path".to_string(),
        json!(DS_SOLAR.locate().map(|path| path.display().to_string())),
    );
    Ok(identity)
}

pub fn render(data: &Value) -> String {
    format!(
        "{} {}\n  {}{}",
        data["name"].as_str().unwrap_or("ds-solar-engine"),
        data["version"].as_str().unwrap_or("?"),
        data["path"].as_str().unwrap_or(""),
        data["source_sha"]
            .as_str()
            .map(|sha| format!("\n  source {sha}"))
            .unwrap_or_default(),
    )
}
