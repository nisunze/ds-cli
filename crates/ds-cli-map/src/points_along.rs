//! `ds map points-along` — points at a fixed interval along a line layer.
//!
//! The geometry is the application's: it runs the same routine the map's tool
//! dock runs, over the same layer catalogue, and puts the result on the map
//! as a new layer. `ds` supplies the source and the settings and reports what
//! came back.
//!
//! The result layer belongs to the analysis tool, not to this session, so
//! `ds map remove` will refuse it. That is the application's rule about whose
//! work an agent may erase, and it is stated in `output` rather than left for
//! a caller to discover.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{BOOL_CHOICES, DESCRIPTOR_ARG};

/// The application's own defaults, as its tool dock opens with them.
const DEFAULT_INTERVAL: &str = "100";
const DEFAULT_INCLUDE_ENDS: &str = "true";

pub static COMMAND: Command = Command {
    id: "map.points-along",
    path: &["map", "points-along"],
    contract: 1,
    summary: "Place points at a fixed interval along a line layer.",
    purpose: "\
Walks every line in a layer and drops a point every --interval-m metres, \
adding the result to the map as a new layer. The source must be a line layer; \
a point or polygon layer is refused. Takes the `analysis_id` that `ds map \
view` reports — design and survey layers are addressable too, not only ones \
this session drew.",
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "layer",
            "<analysis-id>",
            "The `analysis_id` from `ds map view`.",
        )
        .required(),
        Arg::value("interval-m", "<m>", "Spacing in metres; 0.01..1000000.")
            .default(DEFAULT_INTERVAL),
        Arg::value(
            "include-ends",
            "<bool>",
            "Also place a point at each line end.",
        )
        .default(DEFAULT_INCLUDE_ENDS)
        .choices(BOOL_CHOICES),
        DESCRIPTOR_ARG,
    ],
    output: "\
The new layer's id and name, how many points it holds, and how many source \
features and line parts produced them. The new layer belongs to the analysis \
tool, so `ds map remove` will not remove it.",
    examples: &[Example {
        command: "ds map points-along --layer sketch:abc --interval-m 25 --output json",
        note: "Poles every 25 m along a drawn route.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "no such analysis layer, it is empty, or it is not a line layer",
            remedy: "run `ds map view`; points-along needs a LineString layer with features",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let layer = inputs.require("layer")?;
    let interval = crate::number(
        inputs.require("interval-m")?,
        "interval-m",
        0.01,
        1_000_000.0,
    )?;
    let include_ends = crate::boolean(inputs.value("include-ends"), true);

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::POINTS_ALONG,
        json!({
            "layerId": layer,
            "settings": { "intervalM": interval, "includeEnds": include_ends },
        }),
        crate::TOOL_TIMEOUT,
    )?;

    Ok(json!({
        "source": layer,
        "interval_m": interval,
        "include_ends": include_ends,
        "layer": result["layerId"],
        "name": result["layerName"],
        "points": result["featureCount"].as_u64().unwrap_or(0),
        "source_features": result["sourceFeatureCount"].as_u64().unwrap_or(0),
        "line_parts": result["linePartCount"].as_u64().unwrap_or(0),
    }))
}

pub fn render(data: &Value) -> String {
    let points = data["points"].as_u64().unwrap_or(0);
    if points == 0 {
        return format!(
            "no points  — {} produced nothing at {} m\n  → check the layer holds lines with length\n",
            data["source"].as_str().unwrap_or(""),
            data["interval_m"],
        );
    }
    format!(
        "{}  every {} m\n  from {} across {}\n  new layer  {}\n",
        crate::plural(points, "point"),
        data["interval_m"],
        crate::plural(data["source_features"].as_u64().unwrap_or(0), "feature"),
        crate::plural(data["line_parts"].as_u64().unwrap_or(0), "line part"),
        data["name"].as_str().unwrap_or(""),
    )
}
