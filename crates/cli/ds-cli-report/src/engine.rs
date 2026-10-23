//! `ds report engine` — which reporter is installed here.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{ENGINE_LANE, capture_engine};

pub static COMMAND: Command = Command {
    id: "report.engine",
    path: &["report", "engine"],
    contract: 2,
    summary: "Report the installed reporter engine's identity.",
    purpose: "\
Returns the release identity of the `reporter` addon this machine will use: \
its package version and the exact source it was built from. Artifacts are \
bound to that identity, so it is the field to record alongside a deliverable \
and the first thing to compare when two machines disagree.",
    chapter: Chapter::Reports,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[ENGINE_LANE],
    output: "Service and binary name, package version, source SHA, and the combined engine version string.",
    examples: &[Example {
        command: "ds report engine --output json",
        note: "Record .data.engine_version alongside an exported artifact.",
        runnable: true,
    }],
    refusals: &[
        crate::ENGINE_MISSING,
        crate::ENGINE_RECEIPT_INVALID,
        Refusal {
            code: "callee_contract_mismatch",
            when: "the engine answered, but not with the document this build expects",
            remedy: "update `ds` and the reporter to matching releases",
        },
    ],
    reference: Some("docs/reference/report.md"),
    search: &[],
    requires: Requires::Server,
    availability,
};

fn availability() -> Availability {
    Availability::Available
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    run_captured(capture_engine(inputs)?)
}

pub(crate) fn run_captured(engine: crate::Reporter) -> Result<Value, Failure> {
    let identity = engine.build_info()?;
    Ok(json!({
        "binary": "reporter",
        "path": engine.executable().display().to_string(),
        "identity": identity,
    }))
}

pub fn render(data: &Value) -> String {
    let identity = &data["identity"];
    format!(
        "{} {}\n  source {}\n  {}",
        data["binary"].as_str().unwrap_or("reporter"),
        identity["package_version"].as_str().unwrap_or("?"),
        identity["source_sha"].as_str().unwrap_or("?"),
        data["path"].as_str().unwrap_or(""),
    )
}
