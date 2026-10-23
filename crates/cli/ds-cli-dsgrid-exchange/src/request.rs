//! The inputs `plan` and `convert` share, declared once.
//!
//! `plan` and `convert` take the same inputs and must hand the exchange owner
//! the same choices, because the whole point of planning first is that the
//! plan you read is the plan that runs. If the two commands captured their
//! choices separately, a flag handled slightly differently in one of them
//! would make the plan a lie — and it would be a quiet lie, since both calls
//! would still succeed. So the choices are captured here and both commands
//! call it; the Network addon's exchange owner builds the one request from
//! them (DECOUPLING-003).
//!
//! Every closed value this module accepts is declared in `SHARED_ARGS` from
//! the Network contract's vocabulary and enforced by the contract parser
//! before a handler ever runs. A conversion target is not a free-text field:
//! an unrecognized one has to fail at the door with the accepted list, not
//! deep inside the engine.

use ds_cli_contract::Inputs;
use ds_cli_contract::spec::{Arg, ArgKind, Refusal};
use ds_network_contracts::grid_exchange::{
    EXCHANGE_CONTAINERS, EXCHANGE_GIS_LAYER_IDS, EXCHANGE_MODES, EXCHANGE_NETWORK_SOURCE_ROLES,
    EXCHANGE_TARGETS, ExchangeChoicesV1,
};

/// The accepted `--target` values, in the order help prints them. These are
/// the CLI's spellings, not the engine's variant names: `shp` and `gpkg` are
/// what an operator types, and the owner maps them so the engine's Rust
/// identifiers stay out of an interface people have to memorize.
pub const TARGETS: &[&str] = EXCHANGE_TARGETS;

pub const MODES: &[&str] = EXCHANGE_MODES;
pub const CONTAINERS: &[&str] = EXCHANGE_CONTAINERS;
pub const NETWORK_SOURCE_ROLES: &[&str] = EXCHANGE_NETWORK_SOURCE_ROLES;

/// The inputs `plan` and `convert` share. `convert` splices `--out` and
/// `--yes` onto this; nothing else differs between them, which is the
/// property that makes "plan it, then run exactly that" true.
pub const SHARED_ARGS: &[Arg] = &[
    crate::sources::SOURCE_ARG,
    Arg::value("target", "<format>", "The format to produce.")
        .required()
        .choices(TARGETS),
    Arg::value(
        "mode",
        "<mode>",
        "Separate outputs, compose a .dsgrid, combine natively, or merge-gis into one GIS file without changing source models.",
    )
    .default("separate")
    .choices(MODES),
    Arg::value(
        "container",
        "<kind>",
        "For a PLS-CADD target: a workspace folder, or a .bak backup.",
    )
    .default("folder")
    .choices(CONTAINERS),
    Arg::value(
        "select-project",
        "<don-leaf>",
        "Pick one PLS project by its .don leaf when the sources hold several.",
    ),
    Arg::value(
        "crs",
        "<code>",
        "Declare a native PLS source CRS, or the projected metric model CRS for GIS import. `rwanda-tm` is the Rwanda TM (EDCL) grid.",
    ),
    Arg::value(
        "alignment-layer",
        "<name>",
        "For GIS → .dsgrid, map this LineString layer to canonical alignments.",
    ),
    Arg::value(
        "alignment-label-property",
        "<field>",
        "For GIS → .dsgrid, label alignments from this optional source field.",
    ),
    Arg::value(
        "network-source-layer",
        "<name>",
        "For GIS → .dsgrid, select the Point/Polygon layer that identifies explicit source terminals.",
    ),
    Arg::value(
        "network-source-role",
        "<role>",
        "Assign the selected GIS source as a tapping, substation, plant, or generic network_source.",
    )
    .choices(NETWORK_SOURCE_ROLES),
    Arg::value(
        "network-snap-tolerance-m",
        "<metres>",
        "Metric tolerance for joining route endpoints and matching them to explicit source geometry (default 0.5).",
    ),
    Arg::value(
        "terrain-layer",
        "<name>",
        "Map this WGS84 Point layer to canonical terrain observations.",
    ),
    Arg::value(
        "terrain-elevation-property",
        "<field>",
        "Source field carrying terrain elevation in metres.",
    ),
    Arg::value(
        "terrain-feature-class-property",
        "<field>",
        "Optional source field carrying the terrain feature class.",
    ),
    Arg::value("terrain-provider", "<token>", "Terrain provider identity."),
    Arg::value("terrain-dataset", "<name>", "Terrain dataset/version note."),
    Arg::value(
        "terrain-resolution-m",
        "<metres>",
        "Nominal terrain source resolution.",
    ),
    Arg::value(
        "terrain-horizontal-crs",
        "<code>",
        "CRS in which the terrain samples were acquired.",
    ),
    Arg::value(
        "terrain-acquired-at",
        "<rfc3339>",
        "Host-supplied terrain acquisition timestamp.",
    ),
    Arg::value(
        "standards-source-index",
        "<index>",
        "Zero-based --source index of the native standards source used to complete the GIS model.",
    ),
    Arg::value("source-structure-type-id", "<id>", "Exact source support type id."),
    Arg::value(
        "transformer-structure-type-id",
        "<id>",
        "Exact transformer support type id.",
    ),
    Arg::value("tapping-structure-type-id", "<id>", "Exact tapping support type id."),
    Arg::value("support-structure-type-id", "<id>", "Exact inline support type id."),
    Arg::value(
        "maximum-span-m",
        "<metres>",
        "Maximum conceptual support spacing used for deterministic intermediate placement.",
    ),
    Arg::value("phase-cable-id", "<id>", "Exact phase conductor definition id."),
    Arg::value("criterion-set-id", "<id>", "Exact sag/tension criterion set id."),
    Arg::value(
        "sag-weather-state-id",
        "<id>",
        "Exact weather state for the authored initial sag reference.",
    ),
    Arg::value(
        "phase-catenary-constant-m",
        "<metres>",
        "Explicit conceptual catenary constant; never presented as an AutoSag criterion result.",
    ),
    Arg::value(
        "phase-attachment-set",
        "<label>",
        "Exact attachment set label shared by the selected structures.",
    ),
    Arg::value(
        "phase-attachment-slots",
        "<csv>",
        "Comma-separated phase attachment slot ordinals.",
    ),
    // The engine owns the layer set; the parser enforces it and refines a
    // miss into `unknown_gis_layer` (ds-cli-contract `choice_refusal_code`).
    Arg::repeated(
        "include-layer",
        "<layer>",
        "Keep a layer a client kmz/shp leaves out (tension_sections, terrain_points). Repeatable.",
    )
    .choices(EXCHANGE_GIS_LAYER_IDS),
    Arg::repeated(
        "omit-unused-structure-type",
        "<type-id>",
        "Explicitly omit this reviewed unplaced type from native PLS output only. Repeatable; native placement/dependency fences apply.",
    ),
    Arg::switch("swap-xy", "Treat source coordinates as (y, x)."),
    Arg::value(
        "expect-lon",
        "<degrees>",
        "Assert where the result should land. Requires --expect-lat and --expect-radius-km.",
    ),
    Arg::value("expect-lat", "<degrees>", "See --expect-lon."),
    Arg::value(
        "expect-radius-km",
        "<km>",
        "How far from --expect-lon/--expect-lat is still acceptable.",
    ),
];

/// The refusals shared by request building. Spliced into both commands after
/// the source refusals, so the two help screens enumerate the same failures
/// in the same order.
pub const REQUEST_REFUSALS: &[Refusal] = &[
    Refusal {
        code: "structure_omission_invalid",
        when: "an explicit omitted structure type ID cannot be decoded by the native request",
        remedy: "use exact structure type IDs from library.model.show and preview the native plan",
    },
    Refusal {
        code: "incomplete_expected_location",
        when: "only some of --expect-lon, --expect-lat and --expect-radius-km were given",
        remedy: "pass all three, or none",
    },
    Refusal {
        code: "invalid_expected_location",
        when: "an --expect-* value is not a number",
        remedy: "pass decimal degrees for lon/lat and kilometres for the radius",
    },
    Refusal {
        code: "project_not_resolvable",
        when: "--select-project names a .don leaf the sources do not hold, or hold ambiguously",
        remedy: "run `ds dsgrid-exchange inspect` to list what the sources actually contain",
    },
    Refusal {
        code: "incomplete_gis_network_selection",
        when: "network source role/tolerance is supplied without a source layer, or network authoring is requested without an alignment layer",
        remedy: "pass --alignment-layer and --network-source-layer together; role and tolerance are optional",
    },
    Refusal {
        code: "invalid_network_snap_tolerance",
        when: "--network-snap-tolerance-m is not a finite positive number",
        remedy: "pass a positive distance in metres, normally 0.5",
    },
    Refusal {
        code: "incomplete_gis_mv_selection",
        when: "--standards-source-index is present without the complete explicit MV authoring selection",
        remedy: "supply terrain provenance plus exact structure, conductor, criteria and attachment selections",
    },
    Refusal {
        code: "invalid_gis_mv_selection",
        when: "an MV source index, distance, or attachment slot is malformed",
        remedy: "use a zero-based source index, finite positive metre values, and comma-separated slot ordinals",
    },
    Refusal {
        code: "unknown_gis_layer",
        when: "--include-layer names a layer the GIS projection does not have",
        remedy: "pass a layer id --include-layer lists, e.g. tension_sections or terrain_points",
    },
];

/// The operator's choices exactly as the declared flags carried them, for the
/// addon's exchange owner to interpret.
///
/// There is deliberately no `--version` flag: this build authors native
/// output at PLS-CADD 16.81 only, and the owner states that in its plan. Every
/// value here passed this command's parser, which enforced each closed
/// `choices` list; the owner maps them onto its request and answers a value
/// it has no mapping for as `unmapped_choice` (internal-only, registered in
/// `refusal_coverage.rs`), and every other refusal above unchanged.
pub fn choices(inputs: &Inputs) -> ExchangeChoicesV1 {
    let mut choices = ExchangeChoicesV1::default();
    for arg in SHARED_ARGS {
        match arg.kind {
            ArgKind::Value => {
                if let Some(value) = inputs.value(arg.name) {
                    choices.values.insert(arg.name.to_owned(), value.to_owned());
                }
            }
            ArgKind::Repeated if arg.name != "source" => {
                let values = inputs.repeated(arg.name);
                if !values.is_empty() {
                    choices
                        .repeated
                        .insert(arg.name.to_owned(), values.to_vec());
                }
            }
            ArgKind::Switch => {
                if inputs.switch(arg.name) {
                    choices.switches.insert(arg.name.to_owned());
                }
            }
            _ => {}
        }
    }
    choices
}
