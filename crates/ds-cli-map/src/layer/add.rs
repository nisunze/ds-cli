//! `ds map layer add` — persist a third-party XYZ or raster PMTiles overlay.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

use crate::DESCRIPTOR_ARG;

pub static COMMAND: Command = Command {
    id: "map.layer.add",
    path: &["map", "layer", "add"],
    contract: 1,
    summary: "Add a desktop-local XYZ or raster PMTiles overlay.",
    purpose: "Validates and persists one HTTP(S) third-party tile reference in this desktop's IndexedDB. XYZ requires {z}, {x}, and {y}; embedded credentials and non-HTTP schemes are refused. The map need not be open; if it is, the overlay mounts immediately. This never changes the governed project tile catalogue.",
    chapter: Chapter::Survey,
    effect: Effect::LocalUi,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        Arg::value("name", "<text>", "Local display name.").required(),
        Arg::value("kind", "<xyz|pmtiles>", "Remote tile source kind.")
            .required()
            .choices(&["xyz", "pmtiles"]),
        Arg::value(
            "url",
            "<http(s)-url>",
            "XYZ template or raster PMTiles archive URL.",
        )
        .required(),
        Arg::value("tile-size", "<256|512>", "Raster tile size.")
            .default("256")
            .choices(&["256", "512"]),
        Arg::value("attribution", "<text>", "Optional source attribution."),
        Arg::switch("hidden", "Store the overlay hidden instead of visible."),
        DESCRIPTOR_ARG,
    ],
    output: "Local layer id, normalized source fields, visibility, `persisted: desktop_local`, and whether an open map was updated.",
    examples: &[
        Example {
            command: "ds map layer add --name OpenTopo --kind xyz --url 'https://a.tile.opentopomap.org/{z}/{x}/{y}.png'",
            note: "Safe to run while the map page is closed.",
            runnable: false,
        },
        Example {
            command: "ds map layer add --name LocalArchive --kind pmtiles --url 'https://example.org/base.pmtiles' --hidden",
            note: "Raster PMTiles only; vector archives need a governed style/source contract.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NOT_PAIRED,
        crate::AMBIGUOUS,
        crate::UNREACHABLE,
        crate::PAIRING_REJECTED,
        crate::REFUSED,
        crate::UNSUPPORTED,
        crate::UNREADABLE,
        crate::INVALID_NUMBER,
    ],
    reference: Some("docs/reference/map.md"),
    availability: crate::paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let tile_size = crate::integer(inputs.require("tile-size")?, "tile-size", 256, 512)?;
    let descriptor = crate::paired(inputs.value("desktop-descriptor"))?;
    let result = crate::invoke(
        &descriptor,
        &crate::REMOTE_LAYER_ADD,
        json!({
            "name": inputs.require("name")?, "kind": inputs.require("kind")?, "url": inputs.require("url")?,
            "tileSize": tile_size, "attribution": inputs.value("attribution"), "visible": !inputs.switch("hidden"),
        }),
        crate::UI_TIMEOUT,
    )?;
    Ok(super::remote_result(result))
}

pub fn render(data: &Value) -> String {
    format!(
        "added {} ({}) · {} · map updated: {}\n",
        data["name"].as_str().unwrap_or("?"),
        data["kind"].as_str().unwrap_or("?"),
        data["persisted"].as_str().unwrap_or("?"),
        data["map_updated"].as_bool().unwrap_or(false)
    )
}
