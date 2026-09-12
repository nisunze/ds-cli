//! `ds data elevation attach` — native Desktop Rwanda DEM interpolation.

use std::path::Path;
use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops::{
    AMBIGUOUS, BridgeOp, DESCRIPTOR_ARG, NOT_PAIRED, PAIRING_REJECTED, REFUSED, UNREACHABLE,
    UNREADABLE as DESKTOP_UNREADABLE, UNSUPPORTED as DESKTOP_UNSUPPORTED, invoke, paired,
    paired_availability,
};
use serde_json::{Map, Value, json};

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
const FALLBACK_ARG: Arg = Arg {
    name: "fallback",
    kind: ArgKind::Value,
    value: "<source>",
    required: false,
    default: Some("terrarium"),
    choices: &["terrarium", "none"],
    summary: "Use explicit AWS Terrarium fallback outside Rwanda DEM coverage, or none.",
};

const ABSOLUTE_PATH_REQUIRED: Refusal = Refusal {
    code: "absolute_path_required",
    when: "--source or --out is not an absolute local path",
    remedy: "resolve both paths on the Desktop machine before invoking the command",
};
const FULL_LOCAL_DEM_REQUIRED: Refusal = Refusal {
    code: "full_local_dem_required",
    when: "more than 4,000 parsed points are requested without the verified full Rwanda DEM component",
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
    contract: 1,
    summary: "Attach Rwanda DEM elevation to a local point source on Desktop.",
    purpose: "Asks the paired Desktop's native ds-network engine to interpolate the governed Rwanda DEM into a new local GeoJSON. Small jobs may use exact public byte ranges; jobs above 4,000 parsed points make the Desktop component manager install or verify the full local DEM once before retrying. AWS Terrarium is an explicit fallback, source/result bytes never enter DS Cloud Run, and no map needs to be open.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::DesktopPairing,
    execution: Execution::Sync,
    args: &[
        SOURCE_ARG,
        OUT_ARG,
        X_COLUMN_ARG,
        Y_COLUMN_ARG,
        SOURCE_CRS_ARG,
        SEPARATOR_ARG,
        COMMON_COLUMN_ARG,
        FALLBACK_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "A native receipt with source/output paths and digest, point and coverage counts, DEM access mode, fallback evidence, and per-layer statistics.",
    examples: &[
        Example {
            command: "ds data elevation attach --source /data/poles.csv --out /data/poles-elevation.geojson --x-column longitude --y-column latitude --source-crs wgs84_lonlat --fallback terrarium",
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
        ABSOLUTE_PATH_REQUIRED,
        FULL_LOCAL_DEM_REQUIRED,
        MATERIALIZED_FORMAT_BOUND,
        STREAM_GROUP_BOUND,
        NOT_PAIRED,
        AMBIGUOUS,
        UNREACHABLE,
        PAIRING_REJECTED,
        REFUSED,
        DESKTOP_UNREADABLE,
        DESKTOP_UNSUPPORTED,
    ],
    reference: Some("docs/reference/data.md"),
    availability: paired_availability,
};

fn absolute_path(inputs: &Inputs, name: &str) -> Result<String, Failure> {
    let raw = inputs.require(name)?;
    if !Path::new(raw).is_absolute() {
        return Err(Failure::invalid(
            "absolute_path_required",
            format!("--{name} must be an absolute path on the paired Desktop machine."),
        )
        .remedy(ABSOLUTE_PATH_REQUIRED.remedy));
    }
    Ok(raw.to_string())
}

fn refusal(result: &Value) -> Result<Value, Failure> {
    let refusal = &result["refusal"];
    let code = refusal["code"].as_str().unwrap_or("desktop_refused");
    let message = refusal["message"]
        .as_str()
        .unwrap_or("Desktop refused native elevation without a valid explanation.");
    let remedy = refusal["remedy"]
        .as_str()
        .unwrap_or("Update DS GridDesign and retry with the documented command contract.");
    let detail = json!({
        "point_count": refusal.get("point_count").cloned().unwrap_or(Value::Null),
        "point_limit": refusal.get("point_limit").cloned().unwrap_or(Value::Null),
        "source_bytes": refusal.get("source_bytes").cloned().unwrap_or(Value::Null),
        "source_byte_limit": refusal.get("source_byte_limit").cloned().unwrap_or(Value::Null),
    });
    let failure = match code {
        "full_local_dem_required" => Failure::unavailable("full_local_dem_required", message),
        "materialized_format_bound" => Failure::invalid("materialized_format_bound", message),
        "stream_group_bound" => Failure::invalid("stream_group_bound", message),
        _ => Failure::failed("desktop_refused", message),
    };
    Err(failure.remedy(remedy).detail(detail))
}

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let source = absolute_path(inputs, "source")?;
    let out = absolute_path(inputs, "out")?;
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

    let descriptor = paired(inputs.value("desktop-descriptor"))?;
    let result = invoke(
        &descriptor,
        &OPERATION,
        Value::Object(arguments),
        Duration::from_secs(10 * 60 * 60),
    )?;
    match result["status"].as_str() {
        Some("completed") if result["receipt"].is_object() => Ok(result["receipt"].clone()),
        Some("refused") => refusal(&result),
        _ => Err(Failure::failed(
            "desktop_refused",
            "Desktop returned an invalid native elevation outcome.",
        )
        .remedy("Update DS GridDesign and retry with the same explicit source and output.")),
    }
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
    fn bridge_contract_is_closed_and_mapless() {
        assert_eq!(OPERATION.operation, "data.elevation.attach");
        assert_eq!(OPERATION.arguments.len(), 8);
        assert_eq!(COMMAND.authority, Authority::DesktopPairing);
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
