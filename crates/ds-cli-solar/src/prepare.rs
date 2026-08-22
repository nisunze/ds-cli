//! `ds solar prepare` — prepare selected city contexts through DS GridDesign.
//!
//! Preparation belongs at the desktop/cache boundary. The paired application
//! owns the selected project, its authenticated data sources, and its
//! cache-first capture of the complete city input. This command deliberately
//! does not accept cache paths, URLs, credentials, or an IndexedDB location:
//! it asks one named operation to prepare selected contexts and returns the
//! application's bounded receipt.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Map, Value, json};

use crate::{PREPARE_TIMEOUT, paired};

/// The bridge operation is a closed product contract, not caller-supplied
/// transport data. DS GridDesign captures cacheable city input before it
/// invokes the local Rust Solar pipeline.
const PREPARE_OPERATION: &str = "solar.prepare";

pub static COMMAND: Command = Command {
    id: "solar.prepare",
    path: &["solar", "prepare"],
    // v2 moves preparation to the paired desktop/cache boundary and removes
    // the former caller-owned URL/cache/output arguments.
    contract: 2,
    summary: "Prepare selected Solar city inputs through DS GridDesign.",
    purpose: "\
Asks the paired DS GridDesign application to prepare the selected city contexts \
cache-first. The application owns the project, cached weather and PV reference \
inputs, and any authenticated refresh on a cache miss; `ds` receives only the \
bounded preparation receipt. It never reads or scrapes IndexedDB and never \
accepts a URL, credential, cache path, project id, or filesystem root.",
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::repeated(
            "city",
            "<id>",
            "Canonical city context id. Repeat to prepare several cities.",
        )
        .required(),
        Arg::switch(
            "overwrite",
            "Refresh selected cached city inputs before preparing them.",
        ),
        Arg::value(
            "language",
            "<tag>",
            "Requested document language, for example fr or en.",
        ),
        Arg::value(
            "desktop-descriptor",
            "<path>",
            "Use this bridge descriptor instead of discovering one.",
        ),
    ],
    output: "\
The paired application's bounded preparation receipt: selected contexts, their \
cache/preparation status, and any workspace identity the application chooses to \
publish. The receipt contains no credential, cache path, or raw city input.",
    examples: &[
        Example {
            command: "ds solar prepare --city kigali --output json",
            note: "Prepares one selected city through the paired desktop.",
            runnable: false,
        },
        Example {
            command: "ds solar prepare --city kigali --city butare --overwrite --language fr --output json",
            note: "Refreshes and prepares two selected contexts.",
            runnable: false,
        },
    ],
    refusals: paired::REFUSALS,
    reference: Some("docs/reference/solar.md"),
    availability: paired::available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let mut arguments = Map::new();
    arguments.insert("contexts".into(), json!(inputs.repeated("city")));
    if inputs.switch("overwrite") {
        arguments.insert("overwrite".into(), Value::Bool(true));
    }
    if let Some(language) = inputs.value("language") {
        arguments.insert("language".into(), Value::String(language.to_string()));
    }

    paired::invoke(
        inputs,
        PREPARE_OPERATION,
        Value::Object(arguments),
        PREPARE_TIMEOUT,
    )
}

/// Human presentation of the same bounded receipt carried by JSON output.
pub fn render(data: &Value) -> String {
    let contexts = data["contexts"]
        .as_array()
        .or_else(|| data["cities"].as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    let mut out = format!(
        "{}\ncontexts  {}\n",
        data["status"].as_str().unwrap_or("prepared"),
        contexts
    );
    if let Some(root) = data["root"].as_str() {
        out.push_str(&format!("root      {root}\n"));
    }
    if let Some(note) = data["note"].as_str() {
        out.push_str(&format!("{note}\n"));
    }
    out
}
