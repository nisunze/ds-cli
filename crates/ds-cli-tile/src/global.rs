//! Governed global reference publication, owned by the paired application's workflow.
use ds_cli_contract::{
    Context, Failure, Inputs,
    spec::{Arg, Authority, Chapter, Command, Effect, Execution, Refusal},
};
use ds_cli_desktop::ops::{self, BridgeOp, DESCRIPTOR_ARG};
use serde_json::{Value, json};
use std::time::Duration;

pub const CATALOG_OP: BridgeOp = BridgeOp {
    operation: "tile.global.catalog",
    arguments: &["domain", "search", "limit", "page_token"],
};
pub const GENERATE_OP: BridgeOp = BridgeOp {
    operation: "tile.global.generate",
    arguments: &["domain", "name", "country", "sources", "maxzoom", "apply"],
};
pub const STATUS_OP: BridgeOp = BridgeOp {
    operation: "tile.global.status",
    arguments: &["tile_id"],
};
const DOMAIN: Arg = Arg::value("domain", "<domain>", "Governed Reference Layers domain.")
    .required()
    .choices(&["network_template", "solar_template"]);
const REFUSALS: &[Refusal] = &[
    ops::NOT_PAIRED,
    ops::AMBIGUOUS,
    ops::UNREACHABLE,
    ops::PAIRING_REJECTED,
    ops::REFUSED,
    ops::UNSUPPORTED,
    ops::UNREADABLE,
    ops::SIGNED_OUT,
    Refusal {
        code: "invalid_number",
        when: "limit or maxzoom is outside its declared integer range",
        remedy: "use limit 1..200 and maxzoom 1..22",
    },
];

pub static CATALOG: Command = Command {
    id: "tile.global.catalog",
    path: &["tile", "global", "catalog"],
    contract: 1,
    summary: "Discover geographic sources for global reference publication.",
    purpose: "Reads one bounded page of the same source catalog as Reference Layers. The server enforces global_tiles.manage and the selected domain allowlist. This includes eligible sources that have not yet been published and therefore are absent from the Desktop data catalog.",
    chapter: Chapter::VectorTiles,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        DOMAIN,
        Arg::value(
            "search",
            "<text>",
            "Filter source names; at most 200 characters.",
        ),
        Arg::value("limit", "<n>", "Maximum tables per page, 1..200.").default("50"),
        Arg::value(
            "page-token",
            "<token>",
            "Continue the preceding page; at most 2048 characters.",
        ),
        DESCRIPTOR_ARG,
    ],
    output: "Catalog datasets and exact source table identities, row counts, geography columns, and has_more/next_page_token when another page exists.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/tile.md"),
    availability: ops::paired_availability,
};
pub static GENERATE: Command = Command {
    id: "tile.global.generate",
    path: &["tile", "global", "generate"],
    contract: 1,
    summary: "Publish a new global reference tile archive and Desktop datasets.",
    purpose: "Starts the existing governed reference worker for 1..50 explicit catalog project.dataset.table sources. Creates a new publication; does not replace an existing tile or change its access policy. Layer names are the source table names and must be distinct. Uses minzoom 0, explicit maxzoom/basezoom and no density dropping. The server validates source eligibility and global_tiles.manage. Follow the returned tile_id with status; started is not completed.",
    chapter: Chapter::VectorTiles,
    effect: Effect::GlobalWrite,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        DOMAIN,
        Arg::value(
            "name",
            "<text>",
            "Publication name, at most 200 characters.",
        )
        .required(),
        Arg::value(
            "country",
            "<name>",
            "Catalog country, at most 200 characters.",
        )
        .required(),
        Arg::repeated(
            "source",
            "<project.dataset.table>",
            "Exact source from global catalog; repeat 1..50 times.",
        )
        .required(),
        Arg::value("maxzoom", "<zoom>", "Maximum and base zoom, integer 1..22.").required(),
        DESCRIPTOR_ARG,
    ],
    output: "Publication tile_id, status, phase and message. Poll global status for completion or failure before reconciling the Desktop catalog.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/tile.md"),
    availability: ops::paired_availability,
};
pub static STATUS: Command = Command {
    id: "tile.global.status",
    path: &["tile", "global", "status"],
    contract: 1,
    summary: "Read one governed reference publication job.",
    purpose: "Reads the durable job state through Reference Layers, without starting or retrying publication. Returns the actual failure if the worker could not publish.",
    chapter: Chapter::VectorTiles,
    effect: Effect::ReadOnly,
    authority: Authority::DesktopUser,
    execution: Execution::Sync,
    args: &[
        Arg::value(
            "tile",
            "<tile-id>",
            "Exact tile_id returned by global generate.",
        )
        .required(),
        DESCRIPTOR_ARG,
    ],
    output: "Status, phase, tile_id, feature count and completed artifact locations, or last_error.",
    examples: &[],
    refusals: REFUSALS,
    reference: Some("docs/reference/tile.md"),
    availability: ops::paired_availability,
};

fn invoke(inputs: &Inputs, operation: &BridgeOp, args: Value) -> Result<Value, Failure> {
    ops::invoke(
        &ops::paired(inputs.value("desktop-descriptor"))?,
        operation,
        args,
        Duration::from_secs(180),
    )
    .map_err(ops::classify_signed_out)
}
pub fn catalog(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let limit = ops::integer(inputs.require("limit")?, "limit", 1, 200)?;
    let mut args = json!({"domain": inputs.require("domain")?, "limit": limit});
    if let Some(value) = inputs.value("search") {
        args["search"] = json!(value);
    }
    if let Some(value) = inputs.value("page-token") {
        args["page_token"] = json!(value);
    }
    invoke(inputs, &CATALOG_OP, args)
}
pub fn generate(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    let maxzoom = ops::integer(inputs.require("maxzoom")?, "maxzoom", 1, 22)?;
    invoke(
        inputs,
        &GENERATE_OP,
        json!({"domain": inputs.require("domain")?, "name": inputs.require("name")?, "country": inputs.require("country")?, "sources": inputs.repeated("source"), "maxzoom": maxzoom, "apply": true}),
    )
}
pub fn status(inputs: &Inputs, _: &Context) -> Result<Value, Failure> {
    invoke(
        inputs,
        &STATUS_OP,
        json!({"tile_id": inputs.require("tile")?}),
    )
}
pub fn render(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_default()
}
