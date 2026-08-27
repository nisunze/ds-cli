//! `ds map line-difference` — extract extension line work on local layers.
//!
//! The CLI names the authoritative base, incoming source, and metric
//! tolerances. Geometry comparison and contact healing stay in ds-network;
//! local-layer reads and writes stay in the running application.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

pub static COMMAND: Command = Command {
    id: "map.line-difference",
    path: &["map", "line-difference"],
    contract: 1,
    summary: "Extract incoming line portions absent from an authoritative base.",
    purpose: "\
Compares two local LineString layers. Portions of --source-layer already \
covered by directionally aligned --base-layer geometry are removed; remaining \
extension endpoints are healed onto the base within --heal-tolerance-m. The \
Rust/WASM geometry kernel computes the result and the application adds it as \
one new local layer. No project design data is changed.",
    chapter: Chapter::Survey,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("source-layer", "<id>", "Incoming local line layer id.").required(),
        Arg::value("base-layer", "<id>", "Authoritative local line layer id.").required(),
        Arg::value("name", "<text>", "Name of the resulting local layer.").required(),
        Arg::value(
            "coverage-tolerance-m",
            "<m>",
            "Aligned source within this distance is treated as covered; 0.01..25. Parallel lines can be distinct infrastructure, so widen only after review.",
        )
        .default("0.5"),
        Arg::value(
            "heal-tolerance-m",
            "<m>",
            "Snap remaining endpoints to the base within this distance; 0..25.",
        )
        .default("1"),
        DESCRIPTOR_ARG,
    ],
    output: "The result layer id and feature count plus source, covered, and difference lengths and healed endpoint count.",
    examples: &[Example {
        command: "ds map line-difference --source-layer sketch-incoming --base-layer sketch-base --name 'agasharu extension difference' --coverage-tolerance-m 0.5 --heal-tolerance-m 1 --output json",
        note: "Compute a reviewable extension layer; it does not stage design changes.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "a layer is absent, not CLI-owned, unloaded, empty, or not LineString",
            remedy: "run `ds map view` and use layer ids marked this_session=true",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let source = inputs.require("source-layer")?;
    let base = inputs.require("base-layer")?;
    let name = inputs.require("name")?;
    let coverage = crate::number(
        inputs.require("coverage-tolerance-m")?,
        "coverage-tolerance-m",
        0.01,
        25.0,
    )?;
    let heal = crate::number(
        inputs.require("heal-tolerance-m")?,
        "heal-tolerance-m",
        0.0,
        25.0,
    )?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::LINE_EXTENSION_DIFFERENCE,
        json!({
            "sourceLayer": source,
            "baseLayer": base,
            "name": name,
            "coverageToleranceM": coverage,
            "healToleranceM": heal,
        }),
        crate::TOOL_TIMEOUT,
    )?;
    let metrics = &result["metrics"];
    Ok(json!({
        "layer": result["layerId"],
        "name": result["name"],
        "features": result["featureCount"],
        "source_layer": result["sourceLayer"],
        "base_layer": result["baseLayer"],
        "source_features": metrics["source_feature_count"],
        "base_features": metrics["base_feature_count"],
        "source_length_m": metrics["source_length_m"],
        "covered_length_m": metrics["covered_length_m"],
        "difference_length_m": metrics["difference_length_m"],
        "healed_endpoints": metrics["healed_endpoint_count"],
        "coverage_tolerance_m": metrics["coverage_tolerance_m"],
        "heal_tolerance_m": metrics["heal_tolerance_m"],
        "persisted": result["persistedToProject"],
    }))
}

pub fn render(data: &Value) -> String {
    format!(
        "{}\n  {} from {} source feature(s)\n  {:.3} m source · {:.3} m covered · {:.3} m difference\n  {} endpoint(s) healed\n",
        data["name"].as_str().unwrap_or("line difference"),
        crate::plural(data["features"].as_u64().unwrap_or(0), "feature"),
        data["source_features"],
        data["source_length_m"].as_f64().unwrap_or(0.0),
        data["covered_length_m"].as_f64().unwrap_or(0.0),
        data["difference_length_m"].as_f64().unwrap_or(0.0),
        data["healed_endpoints"],
    )
}
