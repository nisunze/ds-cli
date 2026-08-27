//! `ds map zoom` — move the paired map to a bounding box.
//!
//! The box is validated here, against the same bounds the application
//! applies: four finite degrees, longitudes inside ±180, latitudes inside
//! ±90, west below east and south below north. A transposed box is the most
//! common way this call goes wrong, and it is worth naming locally rather
//! than sending and having refused.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

/// The application's own default padding, in pixels.
const DEFAULT_PADDING: &str = "48";

pub static COMMAND: Command = Command {
    id: "map.zoom",
    path: &["map", "zoom"],
    contract: 1,
    summary: "Move the paired map to a bounding box or local layer.",
    purpose: "\
Fits the running map to either an explicit geographic bounding box or a local \
layer already named by `ds map view`. With --layer the application reads its \
own local-layer geometry and computes the extent; no coordinates or features \
cross the CLI boundary. Give exactly one target.",
    chapter: Chapter::Survey,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("bbox", "<w,s,e,n>", "Degrees: west,south,east,north."),
        Arg::value(
            "layer",
            "<id>",
            "CLI-owned local layer id from `ds map view`; the application computes its extent.",
        ),
        Arg::value(
            "padding",
            "<px>",
            "Pixels of margin around the box; 0..240.",
        )
        .default(DEFAULT_PADDING),
        DESCRIPTOR_ARG,
    ],
    output: "The bounding box the map was moved to, the optional local layer id, and the padding applied.",
    examples: &[
        Example {
            command: "ds map zoom --bbox 29.9,-2.1,30.2,-1.85",
            note: "Kigali, roughly.",
            runnable: false,
        },
        Example {
            command: "ds map zoom --layer sketch-1f3a",
            note: "Focus a CLI-owned review layer without returning its geometry.",
            runnable: false,
        },
    ],
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
        Refusal {
            code: "zoom_target",
            when: "neither --bbox nor --layer was given, or both were",
            remedy: "give exactly one of --bbox or --layer",
        },
        crate::INVALID_BBOX,
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let bbox = inputs.value("bbox").map(crate::bbox).transpose()?;
    let layer = inputs.value("layer");
    if bbox.is_some() == layer.is_some() {
        return Err(Failure::invalid(
            "zoom_target",
            "give exactly one of --bbox or --layer",
        ));
    }
    let padding = crate::number(inputs.require("padding")?, "padding", 0.0, 240.0)?;

    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let mut arguments = json!({ "padding": padding });
    if let Some(bbox) = bbox {
        arguments["bbox"] = json!(bbox);
    }
    if let Some(layer) = layer {
        arguments["layerId"] = json!(layer);
    }
    let moved = crate::invoke(&descriptor, &crate::ZOOM_TO, arguments, crate::UI_TIMEOUT)?;

    Ok(json!({
        "bbox": moved["bbox"],
        "layer": moved["layerId"],
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
