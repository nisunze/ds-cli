//! Independent public terrain samples at identical coordinates; the native
//! elevation owner performs acquisition, interpolation and file authoring.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_elevation_host::{ComparisonError, ComparisonOptions, SourceCrs, compare_file};
use serde_json::Value;
use std::path::Path;

pub static COMMAND: Command = Command {
    id: "data.elevation.compare",
    path: &["data", "elevation", "compare"],
    contract: 1,
    summary: "Compare Rwanda TIFF and Terrarium at identical points headlessly.",
    purpose: "Independently samples the governed Rwanda 10 m TIFF and AWS Terrarium at identical coordinates from a local CSV, preserving its fields in a new comparison CSV. Rwanda uses native bilinear interpolation; Terrarium reports its existing nearest pixel and, by default, a separate bilinear result. Reads verified immutable Rwanda index metadata and exact bounded public TIFF ranges, plus bounded Terrarium tiles; no full national download, project, sign-in or Desktop. At most 4,000 points and 32 MiB of CSV. Missing source readings remain gaps, never substituted from the other source. Source methods, byte evidence and unknown vertical datum are explicit; the signed differences are observations, not a datum correction or authority to adjust model heights.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg {
            name: "source",
            kind: ArgKind::Value,
            value: "<file.csv>",
            required: true,
            default: None,
            choices: &[],
            summary: "Local CSV of points; source rows and fields are retained.",
        },
        Arg {
            name: "out",
            kind: ArgKind::Value,
            value: "<new.csv>",
            required: true,
            default: None,
            choices: &[],
            summary: "Fresh CSV destination. Existing files are refused before acquisition.",
        },
        Arg {
            name: "x-column",
            kind: ArgKind::Value,
            value: "<field>",
            required: false,
            default: Some("x"),
            choices: &[],
            summary: "Longitude/easting field.",
        },
        Arg {
            name: "y-column",
            kind: ArgKind::Value,
            value: "<field>",
            required: false,
            default: Some("y"),
            choices: &[],
            summary: "Latitude/northing field.",
        },
        Arg {
            name: "source-crs",
            kind: ArgKind::Value,
            value: "<crs>",
            required: false,
            default: Some("wgs84_lonlat"),
            choices: &[],
            summary: "wgs84_lonlat, nix_itrf2005, or a characterized model CRS such as EPSG:32735. Coordinates are transformed by the native owner.",
        },
        Arg::switch(
            "nearest-only",
            "Omit the extra bilinear Terrarium comparison; retain the existing nearest-pixel method.",
        ),
    ],
    output: "Source/output file identities and SHA-256, point and independent coverage counts, provider identities and methods, unknown-datum limitations and bounded public acquisition evidence. The CSV retains all input fields and adds the actual sampling longitude/latitude, exact per-point readings and signed Terrarium-minus-Rwanda differences; missing readings are blank.",
    examples: &[Example {
        command: "ds data elevation compare --source ./route-points.csv --source-crs EPSG:32735 --out ./terrain-comparison.csv --output json",
        note: "Compare two sources at the same projected route coordinates without changing a model.",
        runnable: false,
    }],
    refusals: &[
        Refusal {
            code: "elevation_invalid_input",
            when: "CSV coordinates, field selection or CRS are invalid, empty or unsupported",
            remedy: "Supply a non-empty CSV with finite coordinates, distinct x/y fields and a characterized CRS.",
        },
        Refusal {
            code: "elevation_source_too_large",
            when: "The source CSV exceeds the 32 MiB bound",
            remedy: "Split the source into bounded CSV files.",
        },
        Refusal {
            code: "elevation_point_limit",
            when: "The source exceeds 4,000 points",
            remedy: "Increase route sampling spacing or split into batches of at most 4,000 points.",
        },
        Refusal {
            code: "elevation_output_exists",
            when: "The output already exists",
            remedy: "Choose a fresh output CSV path.",
        },
        Refusal {
            code: "elevation_output_column_collision",
            when: "An input field already has a reserved comparison-column name",
            remedy: "Rename the existing comparison fields before a new comparison.",
        },
        Refusal {
            code: "elevation_io",
            when: "The source cannot be read or the fresh output cannot be created",
            remedy: "Check the input path and output directory permissions.",
        },
        Refusal {
            code: "elevation_provider_failed",
            when: "A bounded public source transport or tile decoder fails",
            remedy: "Check connectivity to the documented public providers; retry the unchanged request after a transient failure.",
        },
        Refusal {
            code: "elevation_integrity_failed",
            when: "Published metadata or TIFF byte-range identity fails verification",
            remedy: "Keep the refusal evidence and verify the published Rwanda resource before retrying; never substitute an unverified source.",
        },
    ],
    reference: Some("docs/reference/data.md"),
    search: &[
        "dem",
        "height",
        "interpolation",
        "terrain",
        "difference",
        "raster",
    ],
    requires: Requires::Server,
    availability: crate::available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let source = inputs
        .value("source")
        .ok_or_else(|| Failure::invalid("elevation_invalid_input", "--source is required"))?;
    let out = inputs
        .value("out")
        .ok_or_else(|| Failure::invalid("elevation_invalid_input", "--out is required"))?;
    let source_crs =
        SourceCrs::parse(inputs.value("source-crs").unwrap_or("wgs84_lonlat")).map_err(failure)?;
    let options = ComparisonOptions {
        source_crs,
        x_column: inputs.value("x-column").unwrap_or("x").to_string(),
        y_column: inputs.value("y-column").unwrap_or("y").to_string(),
        terrarium_bilinear: !inputs.switch("nearest-only"),
    };
    let receipt = compare_file(Path::new(source), Path::new(out), &options).map_err(failure)?;
    serde_json::to_value(receipt)
        .map_err(|error| Failure::internal("elevation_io", error.to_string()))
}

fn failure(error: ComparisonError) -> Failure {
    let failure = match &error {
        ComparisonError::InvalidInput(_) => {
            Failure::invalid("elevation_invalid_input", "Invalid comparison input")
        }
        ComparisonError::SourceByteLimit => Failure::invalid(
            "elevation_source_too_large",
            "Comparison input exceeds its byte bound",
        ),
        ComparisonError::PointLimit | ComparisonError::RwandaBatchPointLimit => Failure::invalid(
            "elevation_point_limit",
            "Comparison input exceeds its point bound",
        ),
        ComparisonError::OutputExists => Failure::conflict(
            "elevation_output_exists",
            "Comparison output already exists",
        ),
        ComparisonError::OutputColumnCollision(_) => Failure::invalid(
            "elevation_output_column_collision",
            "Comparison fields collide with input fields",
        ),
        ComparisonError::Io(_) => Failure::failed("elevation_io", "Comparison file I/O failed"),
        ComparisonError::Provider(_) => Failure::unavailable(
            "elevation_provider_failed",
            "A public elevation provider failed",
        ),
        ComparisonError::Integrity(_) => Failure::failed(
            "elevation_integrity_failed",
            "Public elevation evidence failed verification",
        ),
    }
    .with_message(error.to_string());
    match COMMAND
        .refusals
        .iter()
        .find(|entry| entry.code == failure.code())
    {
        Some(entry) => failure.remedy(entry.remedy),
        None => failure,
    }
}

pub fn render(data: &Value) -> String {
    format!(
        "Terrain comparison written.\n{}\n",
        serde_json::to_string_pretty(data).unwrap_or_default()
    )
}
