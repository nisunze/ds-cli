//! The published, cross-client conversion capability matrix.
//!
//! The JSON document is deliberately data rather than a second conversion
//! planner. Owners still decide whether a particular source is ready during
//! inspection; this command states the installed surfaces and file boundaries.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Availability, Chapter, Command, Effect, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

const MATRIX: &str = include_str!("../../../docs/contracts/conversion-capability-matrix.json");

pub static COMMAND: Command = Command {
    id: "data.conversion-matrix",
    path: &["data", "conversion-matrix"],
    contract: 1,
    summary: "Show the truthful format, runtime and local-file conversion matrix.",
    purpose: "Returns the shared conversion capability matrix used by the DS Grid browser and `ds`: supported source and destination formats, browser and native Desktop support, local file movement or streaming, constraints, and whether a map or UI is required. It is an installed-surface guide, not a source preflight: use `dsgrid-exchange inspect` or `ds data inspect` before any conversion.",
    chapter: Chapter::Data,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "The versioned matrix rows. Each row names source and destination formats, browser/Desktop support, file handling, streaming, map/UI requirements and constraints.",
    examples: &[],
    refusals: &[],
    reference: Some("docs/contracts/conversion-capability-matrix.json"),
    availability: available,
};

fn available() -> Availability {
    Availability::Available
}

pub fn run(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    // The document is compiled into this binary, so malformed JSON is a build
    // defect rather than a caller refusal. Keeping it here makes the runtime
    // projection consume exactly the same source that cross-client tests copy.
    Ok(serde_json::from_str(MATRIX).expect("embedded conversion matrix is valid JSON"))
}

pub fn render(data: &Value) -> String {
    let mut output = format!(
        "{}\n\n",
        data["title"]
            .as_str()
            .unwrap_or("Conversion capability matrix")
    );
    for row in data["rows"].as_array().into_iter().flatten() {
        let sources = row["sources"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let destinations = row["destinations"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        output.push_str(&format!(
            "{sources} → {destinations}\n  browser: {} · Desktop: {} · map: {} · UI: {}\n  {}\n",
            row["browser"].as_str().unwrap_or("unknown"),
            row["desktop"].as_str().unwrap_or("unknown"),
            row["map_required"].as_bool().unwrap_or(false),
            row["ui_required"].as_bool().unwrap_or(false),
            row["constraints"].as_str().unwrap_or("")
        ));
    }
    output
}
