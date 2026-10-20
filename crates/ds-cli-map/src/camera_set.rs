//! `ds map camera set` — move the paired map to an exact camera.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::map_active_view::{self, REQUEST_SCHEMA};
use serde_json::{Value, json};

use crate::{DESCRIPTOR_ARG, TARGET_ARG};

pub static COMMAND: Command = Command {
    id: "map.camera.set",
    path: &["map", "camera", "set"],
    contract: 1,
    summary: "Set the paired map's camera.",
    purpose: "Moves the paired application's map to an exact WGS84 center, zoom, pitch and bearing. The shared command kernel validates the same camera contract for native and browser callers.",
    chapter: Chapter::MapPresentation,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("center", "<lon,lat>", "WGS84 longitude and latitude.").required(),
        Arg::value("zoom", "<level>", "Map zoom, 0..24.").required(),
        Arg::value("pitch", "<degrees>", "Camera pitch, 0..85.").required(),
        Arg::value("bearing", "<degrees>", "Camera bearing, -360..360.").required(),
        TARGET_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The exact camera the paired map moved to: WGS84 center, zoom, pitch and bearing, as the shared command kernel validated them.",
    examples: &[Example {
        command: "ds map camera set --center 30.0619,-1.9441 --zoom 18.5 --pitch 68 --bearing -32",
        note: "Frame a close, tilted view of one site.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "the application has no open map",
            remedy: "open a project map in DS GridDesign, then retry",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::INVALID_NUMBER,
        Refusal {
            code: "invalid_camera",
            when: "the center or camera violates the shared camera contract",
            remedy: "supply --center longitude,latitude, --zoom 0..24, --pitch 0..85 and --bearing -360..360",
        },
    ],
    reference: Some("docs/reference/map.md"),
    search: &[],
    requires: Requires::Window,
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let center = center(inputs.require("center")?)?;
    let request = json!({
        "schema": REQUEST_SCHEMA,
        "operation": "camera.set",
        "center": center,
        "zoom": crate::number(inputs.require("zoom")?, "zoom", 0.0, 24.0)?,
        "pitch": crate::number(inputs.require("pitch")?, "pitch", 0.0, 85.0)?,
        "bearing": crate::number(inputs.require("bearing")?, "bearing", -360.0, 360.0)?,
    });
    let plan = map_active_view::evaluate(&serde_json::to_vec(&request).unwrap())
        .map_err(|error| Failure::invalid("invalid_camera", error))?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    crate::invoke(
        &descriptor,
        &crate::CAMERA_SET,
        plan["camera"].clone(),
        crate::UI_TIMEOUT,
    )
}

fn center(raw: &str) -> Result<[f64; 2], Failure> {
    let values: Vec<&str> = raw.split(',').map(str::trim).collect();
    if values.len() != 2 || values.iter().any(|value| value.is_empty()) {
        return Err(Failure::invalid(
            "invalid_camera",
            "--center must be longitude,latitude",
        ));
    }
    Ok([
        crate::number(values[0], "center longitude", -180.0, 180.0)?,
        crate::number(values[1], "center latitude", -85.0, 85.0)?,
    ])
}

pub fn render(data: &Value) -> String {
    let camera = &data["camera"];
    format!(
        "camera set  {}, {}  zoom {}  pitch {}  bearing {}\n",
        camera["center"][0],
        camera["center"][1],
        camera["zoom"],
        camera["pitch"],
        camera["bearing"],
    )
}

#[cfg(test)]
#[path = "camera_set_tests.rs"]
mod tests;
