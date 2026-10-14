//! `ds solar engine` — which solar engine is installed here.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DISCOVERY_TIMEOUT, ENGINE_LANE, capture_engine};

pub static COMMAND: Command = Command {
    id: "solar.engine",
    path: &["solar", "engine"],
    contract: 3,
    summary: "Report the installed solar engine's version and location.",
    purpose: "\
Returns the immutable build identity of the `solar` addon this machine \
will use and where it was found. Run it to inspect the addon before \
a prepared-city task, and to record which exact engine produced a result. \
The declared native lane selects its verified addon without reading an account \
or project. One captured path supplies both identity and location.",
    chapter: Chapter::Solar,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[ENGINE_LANE],
    output: "The engine's ds.engine-build/v1 identity and the path `ds` resolved.",
    examples: &[Example {
        command: "ds solar engine --output json",
        note: "Fails with solar_engine_missing where the component is not installed.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "module_receipt_invalid",
            when: "the selected lane's managed addon receipt or executable bytes are invalid",
            remedy: "retain the selected lane; repair or roll back its verified module installation",
        },
        Refusal {
            code: "solar_engine_missing",
            when: "the `solar` addon cannot be found",
            remedy: "install the Solar addon, or set DS_SOLAR_ENGINE_BIN for development",
        },
        Refusal {
            code: "callee_contract_mismatch",
            when: "`solar build-info` does not return its engine identity document",
            remedy: "install compatible ds and Solar addon versions",
        },
    ],
    reference: Some("docs/reference/solar.md"),
    search: &[],
    requires: Requires::Server,
    availability,
};

fn availability() -> Availability {
    // The explicit invocation resolves its lane; discovery cannot choose one.
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    run_captured(capture_engine(inputs)?)
}

pub(crate) fn run_captured(engine: ds_cli_exec::CapturedExternal) -> Result<Value, Failure> {
    let mut identity = engine.call_json("build-info", &[], DISCOVERY_TIMEOUT)?;
    let object = identity.as_object_mut().ok_or_else(|| {
        Failure::failed(
            "callee_contract_mismatch",
            "`solar build-info` did not return an identity object",
        )
        .remedy("install compatible ds and Solar addon versions")
    })?;
    if object.get("name").and_then(Value::as_str) != Some("ds-solar-engine") {
        return Err(Failure::failed(
            "callee_contract_mismatch",
            "`solar build-info` returned the wrong engine identity",
        )
        .remedy("install compatible ds and Solar addon versions"));
    }
    object.insert(
        "path".to_string(),
        json!(engine.executable().display().to_string()),
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
