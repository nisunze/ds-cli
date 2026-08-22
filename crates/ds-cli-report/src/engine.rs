//! `ds report engine` — which reporter is installed here.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Authority, Availability, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{DISCOVERY_TIMEOUT, DS_REPORT};

pub static COMMAND: Command = Command {
    id: "report.engine",
    path: &["report", "engine"],
    contract: 1,
    summary: "Report the installed reporter engine's identity.",
    purpose: "\
Returns the release identity of the `ds-report` engine this machine will use: \
its package version and the exact source it was built from. Artifacts are \
bound to that identity, so it is the field to record alongside a deliverable \
and the first thing to compare when two machines disagree.",
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "Service and binary name, package version, source SHA, and the combined engine version string.",
    examples: &[Example {
        command: "ds report engine --output json",
        note: "Record .data.engine_version alongside an exported artifact.",
        runnable: true,
    }],
    refusals: &[
        Refusal {
            code: "reporter_engine_missing",
            when: "`ds-report` is not installed next to `ds`",
            remedy: "install the desktop, or set DS_REPORT_BIN to a built ds-report",
        },
        Refusal {
            code: "callee_contract_mismatch",
            when: "the engine answered, but not with the document this build expects",
            remedy: "update `ds` and the reporter to matching releases",
        },
    ],
    reference: Some("docs/reference/report.md"),
    availability,
};

fn availability() -> Availability {
    DS_REPORT.availability()
}

pub fn run(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let identity = DS_REPORT.call_json("build-info", &[], DISCOVERY_TIMEOUT)?;
    Ok(json!({
        "binary": "ds-report",
        "path": DS_REPORT.locate().map(|path| path.display().to_string()),
        "identity": identity,
    }))
}

pub fn render(data: &Value) -> String {
    let identity = &data["identity"];
    format!(
        "{} {}\n  source {}\n  {}",
        identity["binary"].as_str().unwrap_or("ds-report"),
        identity["package_version"].as_str().unwrap_or("?"),
        identity["source_sha"].as_str().unwrap_or("?"),
        data["path"].as_str().unwrap_or(""),
    )
}
