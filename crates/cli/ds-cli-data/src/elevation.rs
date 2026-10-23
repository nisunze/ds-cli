//! `ds data elevation attach` — shared native Rwanda DEM interpolation.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops::BridgeOp;
use ds_native_data::{bridge, RequestContext};
use serde_json::{json, Map, Value};

pub const OPERATION: BridgeOp = BridgeOp {
    operation: "data.elevation.attach",
    arguments: &[
        "source",
        "out",
        "x_column",
        "y_column",
        "source_crs",
        "separator",
        "common_column",
        "fallback",
    ],
};

const SOURCE_ARG: Arg = Arg {
    name: "source",
    kind: ArgKind::Value,
    value: "<absolute-path>",
    required: true,
    default: None,
    choices: &[],
    summary: "Absolute local point-source path: CSV/TSV, GeoJSON, KML/KMZ, or zipped Shapefile.",
};
const OUT_ARG: Arg = Arg {
    name: "out",
    kind: ArgKind::Value,
    value: "<absolute-path.geojson>",
    required: true,
    default: None,
    choices: &[],
    summary: "Absolute path for a new GeoJSON result. Existing files are never overwritten.",
};
const X_COLUMN_ARG: Arg = Arg {
    name: "x-column",
    kind: ArgKind::Value,
    value: "<name>",
    required: false,
    default: Some("x"),
    choices: &[],
    summary: "X/longitude field for delimited input.",
};
const Y_COLUMN_ARG: Arg = Arg {
    name: "y-column",
    kind: ArgKind::Value,
    value: "<name>",
    required: false,
    default: Some("y"),
    choices: &[],
    summary: "Y/latitude field for delimited input.",
};
const SOURCE_CRS_ARG: Arg = Arg {
    name: "source-crs",
    kind: ArgKind::Value,
    value: "<crs>",
    required: false,
    default: Some("nix_itrf2005"),
    choices: &["nix_itrf2005", "wgs84_lonlat"],
    summary: "Coordinate frame of delimited X/Y fields; geometry sources carry their own frame.",
};
const SEPARATOR_ARG: Arg = Arg::value(
    "separator",
    "<char>",
    "One-character delimiter override for CSV/TSV input.",
);
const COMMON_COLUMN_ARG: Arg = Arg::value(
    "common-column",
    "<name>",
    "Group points into all-or-nothing interpolation surfaces by this field.",
);
pub(crate) const FALLBACK_ARG: Arg = Arg {
    name: "fallback",
    kind: ArgKind::Value,
    value: "<source>",
    required: false,
    default: Some("none"),
    choices: &["none"],
    summary: "Only none: Rwanda coverage holes stay explicit.",
};
/// Attach alone may name AWS Terrarium for a surface group the Rwanda DEM
/// does not wholly cover.
const ATTACH_FALLBACK_ARG: Arg = Arg {
    name: "fallback",
    kind: ArgKind::Value,
    value: "<source>",
    required: false,
    default: Some("none"),
    choices: &["none", ds_native_data::bridge::TERRARIUM_FALLBACK],
    summary: "none keeps Rwanda holes explicit; aws_terrarium_30m fills them.",
};

const ABSOLUTE_PATH_REQUIRED: Refusal = Refusal {
    code: "absolute_path_required",
    when: "--source or --out is not an absolute local path",
    remedy: "resolve both paths on the Desktop machine before invoking the command",
};
const FULL_LOCAL_DEM_REQUIRED: Refusal = Refusal {
    code: "full_local_dem_required",
    when:
        "more than 4,000 parsed points are requested without the verified full Rwanda DEM component",
    remedy: "install and verify the full Rwanda DEM Desktop component, then retry unchanged",
};
const MATERIALIZED_FORMAT_BOUND: Refusal = Refusal {
    code: "materialized_format_bound",
    when: "a non-streamed archive or geometry source exceeds the native materialization bound",
    remedy: "convert the point source to CSV/TSV and use the streamed Desktop lane",
};
const STREAM_GROUP_BOUND: Refusal = Refusal {
    code: "stream_group_bound",
    when: "a large streamed table requests a common-column plan that exceeds the bounded grouping lane",
    remedy: "omit --common-column for one surface, or split the table into explicit surface groups",
};

pub static COMMAND: Command = Command {
    id: "data.elevation.attach",
    path: &["data", "elevation", "attach"],
    contract: 3,
    summary: "Attach Rwanda DEM elevation to a local point source locally.",
    purpose: "Interpolates the governed Rwanda DEM into a new local GeoJSON through the shared native operation and offline ds-network engine. Rwanda holes stay explicit unless --fallback aws_terrarium_30m fills each surface group the DEM misses; the receipt counts both. Small jobs may use exact public byte ranges; above 4,000 parsed points the Desktop component manager installs or verifies the full local DEM once. Bytes never enter DS Cloud Run; no map needs to be open.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        SOURCE_ARG,
        OUT_ARG,
        X_COLUMN_ARG,
        Y_COLUMN_ARG,
        SOURCE_CRS_ARG,
        SEPARATOR_ARG,
        COMMON_COLUMN_ARG,
        ATTACH_FALLBACK_ARG,
    ],
    output: "A native receipt with source/output paths and digest, point, Rwanda and fallback counts, DEM access mode, fallback evidence, and per-layer statistics.",
    examples: &[
        Example {
            command: "ds data elevation attach --source /data/poles.csv --out /data/poles-elevation.geojson --x-column longitude --y-column latitude --source-crs wgs84_lonlat --fallback none",
            note: "Hypothetical longitude/latitude table; works quietly without opening the map.",
            runnable: false,
        },
        Example {
            command: "ds data elevation attach --source /data/alignment-points.tsv --out /data/alignment-elevation.geojson --common-column alignment --fallback none",
            note: "Hypothetical grouped NIX points; each alignment keeps all-or-nothing Rwanda coverage.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::NATIVE_DATA_REFUSED,
        crate::NATIVE_DATA_FAILED,
        ABSOLUTE_PATH_REQUIRED,
        FULL_LOCAL_DEM_REQUIRED,
        MATERIALIZED_FORMAT_BOUND,
        STREAM_GROUP_BOUND,
    ],
    reference: Some("docs/reference/data.md"),
    search: &[],
    requires: Requires::Server,
    availability: crate::available,
};

fn arguments(inputs: &Inputs) -> Result<Map<String, Value>, Failure> {
    let source = inputs.require("source")?;
    let out = inputs.require("out")?;
    let mut arguments = Map::new();
    arguments.insert("source".into(), json!(source));
    arguments.insert("out".into(), json!(out));
    arguments.insert("x_column".into(), json!(inputs.require("x-column")?));
    arguments.insert("y_column".into(), json!(inputs.require("y-column")?));
    arguments.insert("source_crs".into(), json!(inputs.require("source-crs")?));
    arguments.insert("fallback".into(), json!(inputs.require("fallback")?));
    if let Some(value) = inputs.value("separator") {
        arguments.insert("separator".into(), json!(value));
    }
    if let Some(value) = inputs.value("common-column") {
        arguments.insert("common_column".into(), json!(value));
    }
    Ok(arguments)
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let arguments = arguments(inputs)?;
    let context = RequestContext::local_files(
        ds_native_data::ExecutionHost::Headless,
        ds_native_data::online,
    );
    bridge::attach(&context, Value::Object(arguments))
        .and_then(bridge::CommandOutcome::into_receipt)
        .map_err(crate::native_data_failure)
}

pub fn render(data: &Value) -> String {
    format!(
        "attached native elevation to {}\n  {} points · Rwanda {} · fallback {} · sha256 {}\n",
        data["output_path"].as_str().unwrap_or("?"),
        data["point_count"].as_u64().unwrap_or(0),
        data["rwanda_coverage"].as_str().unwrap_or("?"),
        data["fallback_count"].as_u64().unwrap_or(0),
        data["output_sha256"].as_str().unwrap_or("?"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_explicit_none_dispatch_one_rwanda_provider_and_reject_terrarium() {
        let tokens = [
            "--source",
            "/data/poles.csv",
            "--out",
            "/data/poles.geojson",
        ]
        .map(str::to_string)
        .to_vec();
        let defaults = ds_cli_contract::args::parse(&COMMAND, &tokens).unwrap();
        let request = arguments(&defaults).unwrap();
        assert_eq!(request["fallback"], "none");
        assert_eq!(request["source"], "/data/poles.csv");
        let mut explicit = tokens.clone();
        explicit.extend(["--fallback", "none"].map(str::to_string));
        assert_eq!(
            arguments(&ds_cli_contract::args::parse(&COMMAND, &explicit).unwrap()).unwrap(),
            request
        );
        let mut mixed = tokens;
        mixed.extend(["--fallback", "terrarium"].map(str::to_string));
        assert_eq!(
            ds_cli_contract::args::parse(&COMMAND, &mixed)
                .unwrap_err()
                .code(),
            "invalid_choice"
        );
    }

    #[test]
    fn bridge_contract_is_closed_and_mapless() {
        assert_eq!(OPERATION.operation, "data.elevation.attach");
        assert_eq!(OPERATION.arguments.len(), 8);
        assert_eq!(COMMAND.authority, Authority::None);
        assert_eq!(COMMAND.effect, Effect::LocalFileWrite);
    }

    #[test]
    fn native_refusals_are_declared_verbatim() {
        for code in [
            "full_local_dem_required",
            "materialized_format_bound",
            "stream_group_bound",
        ] {
            assert!(COMMAND.refusals.iter().any(|refusal| refusal.code == code));
        }
    }
}

#[cfg(test)]
#[path = "elevation_fallback_tests.rs"]
mod fallback_tests;
