//! `ds map camera set` — apply an exact camera to the active renderer.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_command_kernel::map_active_view::{self, REQUEST_SCHEMA};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

pub static COMMAND: Command = Command {
    id: "map.camera.set",
    path: &["map", "camera", "set"],
    contract: 1,
    summary: "Set the paired map's active 2D or 3D camera.",
    purpose: "Moves the renderer already selected in the paired application to an exact WGS84 center, zoom, pitch and bearing. The shared command kernel validates the same camera contract for native and browser callers.",
    chapter: Chapter::Survey,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("center", "<lon,lat>", "WGS84 longitude and latitude."),
        Arg::value("zoom", "<level>", "Map zoom, 0..24."),
        Arg::value("pitch", "<degrees>", "Camera pitch, 0..85."),
        Arg::value("bearing", "<degrees>", "Camera bearing, -360..360."),
        DESCRIPTOR_ARG,
    ],
    output: "The exact camera applied and the active renderer mode/provider that received it.",
    examples: &[Example {
        command: "ds map camera set --center 30.0619,-1.9441 --zoom 18.5 --pitch 68 --bearing -32",
        note: "Frame a close 3D view after selecting a renderer.",
        runnable: false,
    }],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        Refusal {
            code: "desktop_refused",
            when: "the application has no active map renderer",
            remedy: "open a project map in DS GridDesign, then retry",
        },
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/map.md"),
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
        .map_err(|error| Failure::invalid("camera", error))?;
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
            "camera",
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
        "camera set  {}, {}  zoom {}  pitch {}  bearing {}  renderer {}{}\n",
        camera["center"][0],
        camera["center"][1],
        camera["zoom"],
        camera["pitch"],
        camera["bearing"],
        data["mode"].as_str().unwrap_or("unknown"),
        data["provider"]
            .as_str()
            .map(|provider| format!("/{provider}"))
            .unwrap_or_default(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_is_exactly_one_bounded_lon_lat_pair() {
        assert_eq!(center("30.0619,-1.9441").unwrap(), [30.0619, -1.9441]);
        for raw in ["30", "30,-1,8", "181,-1", "30,-86", "a,-1"] {
            assert!(center(raw).is_err(), "{raw}");
        }
    }
}
