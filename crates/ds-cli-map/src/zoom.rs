//! `ds map zoom` — move the paired map to a bounding box.
//!
//! The box is validated here, against the same bounds the application
//! applies: four finite degrees, longitudes inside ±180, latitudes inside
//! ±90, west below east and south below north. A transposed box is the most
//! common way this call goes wrong, and it is worth naming locally rather
//! than sending and having refused.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Command, Effect, Example, Execution, Refusal};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

/// The application's own default padding, in pixels.
const DEFAULT_PADDING: &str = "48";

pub static COMMAND: Command = Command {
    id: "map.zoom",
    path: &["map", "zoom"],
    contract: 1,
    summary: "Move the paired map to a bounding box.",
    purpose: "\
Fits the running map to a geographic bounding box, the way a person dragging \
a box would. Degrees, west,south,east,north. The box is checked here before \
it is sent, so a transposed or out-of-range box is a typed refusal naming the \
problem rather than an operation the application declines.",
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("bbox", "<w,s,e,n>", "Degrees: west,south,east,north.").required(),
        Arg::value(
            "padding",
            "<px>",
            "Pixels of margin around the box; 0..240.",
        )
        .default(DEFAULT_PADDING),
        DESCRIPTOR_ARG,
    ],
    output: "The bounding box the map was moved to, and the padding applied.",
    examples: &[Example {
        command: "ds map zoom --bbox 29.9,-2.1,30.2,-1.85",
        note: "Kigali, roughly.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "the application has no map open to move",
            remedy: "open a project map in DS GridDesign; `ds map view` reports whether one is open",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::INVALID_BBOX,
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let bbox = crate::bbox(inputs.require("bbox")?)?;
    let padding = crate::number(inputs.require("padding")?, "padding", 0.0, 240.0)?;

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let moved = crate::invoke(
        &descriptor,
        &crate::ZOOM_TO,
        json!({ "bbox": bbox, "padding": padding }),
        crate::UI_TIMEOUT,
    )?;

    Ok(json!({
        "bbox": moved["bbox"].as_array().cloned().unwrap_or_else(|| {
            bbox.iter().map(|value| json!(value)).collect()
        }),
        "padding": moved["padding"].as_f64().unwrap_or(padding),
    }))
}

pub fn render(data: &Value) -> String {
    let empty = Vec::new();
    let bbox = data["bbox"].as_array().unwrap_or(&empty);
    if bbox.len() != 4 {
        return "map moved\n".to_string();
    }
    format!(
        "map moved to  {}, {} .. {}, {}  (padding {})\n",
        bbox[0], bbox[1], bbox[2], bbox[3], data["padding"]
    )
}
