//! `ds map random-points` — spaced random points inside a layer's area.
//!
//! Polygons define their own area. Points and lines do not, so they need
//! `--buffer-m` to make one — the application refuses them without it, and
//! this command says so in its refusals rather than letting a caller find out
//! by trying.
//!
//! `--seed` is what makes the call worth having twice: the same seed over the
//! same layer is the same sample, so a run can be reproduced.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::{BOOL_CHOICES, DESCRIPTOR_ARG};

/// The application's own defaults, as its tool dock opens with them.
const DEFAULT_MIN_SPACING: &str = "50";
const DEFAULT_MAX_SPACING: &str = "100";
const DEFAULT_BUFFER: &str = "25";

pub static COMMAND: Command = Command {
    id: "map.random-points",
    path: &["map", "random-points"],
    contract: 1,
    summary: "Scatter spaced random points inside a layer's area.",
    purpose: "\
Samples random points inside the areas a layer defines, no closer together \
than --min-spacing-m and no further than --max-spacing-m, and adds them to the \
map as a new layer. Polygons are their own area; point and line layers need \
--buffer-m to make one. Pass --seed to make the sample reproducible.",
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
        Arg::value(
            "min-spacing-m",
            "<m>",
            "Closest two points may be; 0.01..1000000.",
        )
        .default(DEFAULT_MIN_SPACING),
        Arg::value(
            "max-spacing-m",
            "<m>",
            "Furthest apart to aim for; 0.01..1000000.",
        )
        .default(DEFAULT_MAX_SPACING),
        Arg::value(
            "buffer-m",
            "<m>",
            "Buffer non-polygon parts into areas; 0..1000000.",
        )
        .default(DEFAULT_BUFFER),
        Arg::value(
            "sample-elevation",
            "<bool>",
            "Read terrain elevation at each point.",
        )
        .default("false")
        .choices(BOOL_CHOICES),
        Arg::value("seed", "<n>", "Fix the sample so the run repeats."),
        DESCRIPTOR_ARG,
    ],
    output: "\
The new layer's id and name, the point count, and how the areas were made: \
source features, sampling areas, how many were buffered and how many parts \
were skipped for lack of a buffer. With elevation, how many points had none.",
    examples: &[Example {
        command: "ds map random-points --layer sketch:abc --min-spacing-m 30 --seed 7 --output json",
        note: "Reproducible: the same seed samples the same points.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "no such analysis layer, it is empty, or a non-polygon layer got --buffer-m 0",
            remedy: "run `ds map view`; a point or line layer needs --buffer-m above 0",
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
    let min = crate::number(
        inputs.require("min-spacing-m")?,
        "min-spacing-m",
        0.01,
        1_000_000.0,
    )?;
    let max = crate::number(
        inputs.require("max-spacing-m")?,
        "max-spacing-m",
        0.01,
        1_000_000.0,
    )?;
    let buffer = crate::number(inputs.require("buffer-m")?, "buffer-m", 0.0, 1_000_000.0)?;
    let elevation = crate::boolean(inputs.value("sample-elevation"), false);

    let mut settings = json!({
        "minSpacingM": min,
        "maxSpacingM": max,
        "bufferDistanceM": buffer,
        "sampleElevation": elevation,
    });
    // Omitted rather than defaulted: the application's schema makes `seed`
    // optional, and sending a fixed one would quietly make every unseeded run
    // identical.
    if let Some(raw) = inputs.value("seed") {
        settings["seed"] = json!(crate::integer(raw, "seed", -2_147_483_648, 2_147_483_647)?);
    }

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::RANDOM_POINTS,
        json!({ "layerId": layer, "settings": settings }),
        crate::TOOL_TIMEOUT,
    )?;

    Ok(json!({
        "source": layer,
        "min_spacing_m": min,
        "max_spacing_m": max,
        "buffer_m": buffer,
        "seed": settings.get("seed").cloned().unwrap_or(Value::Null),
        "layer": result["layerId"],
        "name": result["layerName"],
        "points": result["pointCount"].as_u64().unwrap_or(0),
        "source_features": result["sourceFeatureCount"].as_u64().unwrap_or(0),
        "areas": result["areaCount"].as_u64().unwrap_or(0),
        "buffered_areas": result["bufferedCount"].as_u64().unwrap_or(0),
        "skipped_no_buffer": result["skippedNoBuffer"].as_u64().unwrap_or(0),
        "elevation_sampled": result["elevationSampled"].as_bool().unwrap_or(false),
        "elevation_gaps": result["elevationGaps"].as_u64().unwrap_or(0),
    }))
}

pub fn render(data: &Value) -> String {
    let points = data["points"].as_u64().unwrap_or(0);
    let skipped = data["skipped_no_buffer"].as_u64().unwrap_or(0);
    if points == 0 {
        let mut out = format!(
            "no points  — {} produced no sampling area\n",
            data["source"].as_str().unwrap_or("")
        );
        if skipped > 0 {
            out.push_str(&format!(
                "  → {} skipped for lack of a buffer; pass --buffer-m\n",
                crate::plural(skipped, "part")
            ));
        }
        return out;
    }
    let mut out = format!(
        "{}  in {}\n  from {}\n  new layer  {}\n",
        crate::plural(points, "point"),
        crate::plural(data["areas"].as_u64().unwrap_or(0), "area"),
        crate::plural(data["source_features"].as_u64().unwrap_or(0), "feature"),
        data["name"].as_str().unwrap_or(""),
    );
    if skipped > 0 {
        out.push_str(&format!(
            "  {} skipped for lack of a buffer\n",
            crate::plural(skipped, "part")
        ));
    }
    if data["elevation_sampled"].as_bool().unwrap_or(false) {
        out.push_str(&format!(
            "  elevation sampled; {} without data\n",
            data["elevation_gaps"]
        ));
    }
    out
}
