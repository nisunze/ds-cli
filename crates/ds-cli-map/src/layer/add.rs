//! `ds map layer add` — persist a third-party XYZ or raster PMTiles overlay.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

pub static COMMAND: Command = Command {
    id: "map.layer.add",
    path: &["map", "layer", "add"],
    contract: 2,
    summary: "Add a machine-local XYZ or raster PMTiles overlay.",
    purpose: "Validates and persists one HTTP(S) third-party tile reference in the shared native local store. XYZ requires {z}, {x}, and {y}; embedded credentials and non-HTTP schemes are refused. The map need not be open; the desktop reconciles this store when open. This never changes the governed project tile catalogue.",
    chapter: Chapter::Survey,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
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
    ],
    output: "Local layer id, normalized source fields, visibility, `persisted: native_local`, and whether the local registry was updated.",
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
    refusals: &[super::LOCAL_STORE_REFUSAL, crate::INVALID_NUMBER],
    reference: Some("docs/reference/map.md"),
    availability: super::local_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let result = super::local_edit(ds_layer_store::OverlayEdit::Add {
        layer: ds_layer_store::Overlay {
            id: ds_layer_store::new_id(),
            name: inputs.require("name")?.trim().into(),
            kind: inputs.require("kind")?.into(),
            url: inputs.require("url")?.trim().into(),
            tile_size: crate::integer(inputs.require("tile-size")?, "tile-size", 256, 512)? as u16,
            attribution: inputs
                .value("attribution")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            visible: !inputs.switch("hidden"),
        },
    })?;
    Ok(super::remote_result(result))
}

pub fn render(data: &Value) -> String {
    format!(
        "added {} · {} ({})\n",
        data["layer"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or("?"),
        data["kind"].as_str().unwrap_or("?")
    )
}
