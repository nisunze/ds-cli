//! `ds data admin-bounds attach` — native Rwanda hierarchy attachment.

use std::time::Duration;

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal,
};
use ds_cli_contract::{Context, Inputs};
use ds_cli_desktop::ops::{
    AMBIGUOUS, BridgeOp, DESCRIPTOR_ARG, NOT_PAIRED, PAIRING_REJECTED, REFUSED, SIGNED_OUT,
    UNREACHABLE, UNREADABLE as DESKTOP_UNREADABLE, UNSUPPORTED as DESKTOP_UNSUPPORTED,
    classify_signed_out, invoke, paired, paired_availability,
};
use serde_json::{Map, Value, json};

pub const OPERATION: BridgeOp = BridgeOp {
    operation: "data.admin_bounds.attach",
    arguments: &["source", "out", "longitude_column", "latitude_column"],
};

const OUT_ARG: Arg = Arg {
    name: "out",
    kind: ArgKind::Value,
    value: "<path>",
    required: true,
    default: None,
    choices: &[],
    summary: "New same-format output path. Existing files are never overwritten.",
};
const SOURCE_ARG: Arg = Arg {
    name: "source",
    kind: ArgKind::Value,
    value: "<path>",
    required: true,
    default: None,
    choices: &[],
    summary: "Local CSV, TSV, GeoJSON, or JSON point file carrying elevation data.",
};
const LONGITUDE_ARG: Arg = Arg::value(
    "longitude-column",
    "<name>",
    "Longitude column for CSV/TSV input. Omit for GeoJSON geometry.",
);
const LATITUDE_ARG: Arg = Arg::value(
    "latitude-column",
    "<name>",
    "Latitude column for CSV/TSV input. Omit for GeoJSON geometry.",
);
const ADMIN_UNSUPPORTED: Refusal = Refusal {
    code: "source_unsupported",
    when: "the source is not CSV, TSV, or GeoJSON, or table coordinate columns were omitted",
    remedy: "use GeoJSON geometry, or pass both coordinate columns for CSV/TSV",
};
const NO_OVERWRITE: Refusal = Refusal {
    code: "output_refused",
    when: "the output already exists, is the source, or cannot be created safely",
    remedy: "choose a new same-format --out path; this command never overwrites",
};

pub static COMMAND: Command = Command {
    id: "data.admin-bounds.attach",
    path: &["data", "admin-bounds", "attach"],
    contract: 1,
    summary: "Attach Rwanda province-to-village fields to local elevation points.",
    purpose: "Uses the active project's digest-pinned Rwanda boundary resource and the bundled native reporter to write a new CSV, TSV, or GeoJSON file. The source geometry and elevation fields are unchanged; existing operator-supplied admin values win. This needs the paired desktop for its governed installed resource, but it does not need the map to be open.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::Project,
    execution: Execution::Sync,
    args: &[
        SOURCE_ARG,
        OUT_ARG,
        LONGITUDE_ARG,
        LATITUDE_ARG,
        DESCRIPTOR_ARG,
    ],
    output: "The project, output path, matched/outside counts, output digest, reference digest, and attached columns.",
    examples: &[
        Example {
            command: "ds data admin-bounds attach --source ./elevation.csv --out ./elevation-admin.csv --longitude-column longitude --latitude-column latitude",
            note: "Streams table rows locally; no map is opened.",
            runnable: false,
        },
        Example {
            command: "ds data admin-bounds attach --source ./elevation.geojson --out ./elevation-admin.geojson",
            note: "GeoJSON reads coordinates from feature geometry.",
            runnable: false,
        },
    ],
    refusals: &[
        crate::UNREADABLE,
        ADMIN_UNSUPPORTED,
        NO_OVERWRITE,
        NOT_PAIRED,
        AMBIGUOUS,
        UNREACHABLE,
        PAIRING_REJECTED,
        REFUSED,
        SIGNED_OUT,
        DESKTOP_UNREADABLE,
        DESKTOP_UNSUPPORTED,
    ],
    reference: Some("docs/reference/data.md"),
    availability: paired_availability,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let source = std::fs::canonicalize(inputs.require("source")?).map_err(|error| {
        Failure::invalid(
            "source_unreadable",
            format!("Could not resolve the source: {error}"),
        )
        .remedy("Check the path exists and is readable.")
    })?;
    let raw_out = std::path::PathBuf::from(inputs.require("out")?);
    let out = if raw_out.is_absolute() {
        raw_out
    } else {
        std::env::current_dir()
            .map_err(|error| Failure::internal("output_refused", error.to_string()))?
            .join(raw_out)
    };
    if out.exists() {
        return Err(
            Failure::conflict("output_refused", "The output file already exists.")
                .remedy("Choose another --out path; admin attachment never overwrites."),
        );
    }
    let longitude = inputs.value("longitude-column");
    let latitude = inputs.value("latitude-column");
    if longitude.is_some() != latitude.is_some() {
        return Err(Failure::invalid(
            "source_unsupported",
            "--longitude-column and --latitude-column must be supplied together.",
        ));
    }
    let extension = source
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "csv" | "tsv") && longitude.is_none() {
        return Err(Failure::invalid(
            "source_unsupported",
            "CSV/TSV admin attachment requires explicit coordinate columns.",
        )
        .remedy("Pass --longitude-column and --latitude-column from `ds data inspect`."));
    }
    if !matches!(extension.as_str(), "csv" | "tsv" | "geojson" | "json") {
        return Err(Failure::invalid(
            "source_unsupported",
            "Admin attachment supports CSV, TSV, and GeoJSON point files.",
        ));
    }
    let mut arguments = Map::new();
    arguments.insert("source".into(), json!(source.to_string_lossy()));
    arguments.insert("out".into(), json!(out.to_string_lossy()));
    if let Some(value) = longitude {
        arguments.insert("longitude_column".into(), json!(value));
    }
    if let Some(value) = latitude {
        arguments.insert("latitude_column".into(), json!(value));
    }
    let descriptor = paired(inputs.value("desktop-descriptor"))?;
    invoke(
        &descriptor,
        &OPERATION,
        Value::Object(arguments),
        Duration::from_secs(20 * 60),
    )
    .map_err(classify_signed_out)
}

pub fn render(data: &Value) -> String {
    format!(
        "attached Rwanda admin bounds to {}\n  {} of {} points matched · sha256 {}\n",
        data["out"].as_str().unwrap_or("?"),
        data["features_matched"].as_u64().unwrap_or(0),
        data["features_read"].as_u64().unwrap_or(0),
        data["output_sha256"].as_str().unwrap_or("?"),
    )
}
