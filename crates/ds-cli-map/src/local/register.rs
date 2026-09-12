//! `ds map local register` — prepare one local layer from a GeoJSON file here.

use std::path::PathBuf;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
use ds_layer_store::prepared::{self, Op, Register};
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "map.local.register",
    path: &["map", "local", "register"],
    contract: 1,
    summary: "Prepare one local layer from a GeoJSON file on this host.",
    purpose: "Copies a bounded GeoJSON FeatureCollection into this lane and account's payload directory and asks the shared kernel to admit the descriptor: it mints the id, picks the colour, fills every default and records the origin. The file you name is read on the host running `ds` — a remote Server never reads a client path — and is never moved, modified or deleted. Machine-local: this publishes nothing and touches no project catalogue.",
    chapter: Chapter::Survey,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "file",
            "<geojson>",
            "GeoJSON FeatureCollection on this host; up to 64 MiB.",
        )
        .required(),
        Arg::value("name", "<text>", "Layer name; 1 to 200 characters.").required(),
        Arg::value(
            "geometry",
            "<point|linestring|polygon>",
            "The geometry every feature in the file carries.",
        )
        .required()
        .choices(&["point", "linestring", "polygon"]),
        Arg::value(
            "source-name",
            "<text>",
            "Container to file the layer under; the file name by default.",
        ),
        super::LANE_ARG,
        super::ACCOUNT_ARG,
    ],
    output: "The minted layer id, the admitted descriptor's name, source, geometry, colour and feature count, and the copied payload's path, feature count and size.",
    examples: &[Example {
        command: "ds map local register --file ./roads.geojson --name 'Access roads' --geometry linestring --output json",
        note: "The original file stays exactly where and as it is.",
        runnable: false,
    }],
    refusals: &[
        super::STORE_REFUSED,
        super::INVALID_PAYLOAD,
        super::MALFORMED_DESCRIPTOR,
        super::DUPLICATE_LAYER,
        super::SCOPE_MISMATCH,
        super::UNSUPPORTED_SOURCE_KIND,
    ],
    reference: Some("docs/reference/map.md"),
    availability: super::availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let word = inputs.require("geometry")?;
    let geometry = prepared::geometry_type(word).ok_or_else(|| {
        Failure::invalid(
            "invalid_payload",
            format!("`{word}` is not a prepared local layer geometry"),
        )
        .remedy(super::INVALID_PAYLOAD.remedy)
    })?;
    let scope = super::scope(inputs)?;
    let answer = prepared::execute(
        &scope,
        Op::Register(Register {
            name: inputs.require("name")?.trim().to_owned(),
            geometry,
            source_name: inputs
                .value("source-name")
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_owned),
            from: Some(PathBuf::from(inputs.require("file")?)),
            now_ms: None,
        }),
    )
    .map_err(super::refuse)?;

    let receipt = &answer["receipt"];
    Ok(super::stamped(
        &scope,
        &answer,
        json!({
            "layer": receipt["id"].clone(),
            "name": receipt["name"].clone(),
            "source_name": receipt["sourceName"].clone(),
            "source_kind": receipt["sourceKind"].clone(),
            "geometry_type": receipt["geometryType"].clone(),
            "color": receipt["color"].clone(),
            "feature_count": receipt["featureCount"].clone(),
            "visible": receipt["visible"].clone(),
            "payload": answer["payload"].clone(),
        }),
    ))
}

pub fn render(data: &Value) -> String {
    format!(
        "prepared {} · {} ({} {} features)\n  filed under {}\n  payload    {}\n",
        data["layer"].as_str().unwrap_or("?"),
        data["name"].as_str().unwrap_or("?"),
        data["feature_count"],
        data["geometry_type"].as_str().unwrap_or("?"),
        data["source_name"].as_str().unwrap_or("?"),
        data["payload"]["path"].as_str().unwrap_or("?"),
    )
}
