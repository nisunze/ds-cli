//! The `ds data` domain — local data preparation.
//!
//! Conversion is an explicit, named step that happens *before* analysis, never
//! silently inside it. A caller inspects a source, decides how to read it, and
//! converts; analysis then reads the converted artifact.
//!
//! Inspection and conversion need no project or paired desktop. Admin-bound
//! attachment pairs only to resolve the active project's governed installed
//! reference asset; the file computation stays native and needs no map open.
//!
//! The conversion itself lives in `ds-columnar` in `ds-network`. This crate is
//! a surface over it, not a second implementation of it.

use ds_cli_contract::spec::{Arg, ArgKind, Domain, Refusal};

pub mod admin_bounds;
pub mod conversion_matrix;
pub mod convert;
pub mod elevation;
pub mod inspect;

pub static DOMAIN: Domain = Domain {
    id: "data",
    summary: "Local data: inspect, convert, and attach governed reference fields.",
    commands: &[
        &inspect::COMMAND,
        &convert::COMMAND,
        &conversion_matrix::COMMAND,
        &elevation::COMMAND,
        &admin_bounds::COMMAND,
    ],
};

/// The paired operations in this otherwise local domain. Both are file work
/// and deliberately own no map state.
pub const BRIDGE_OPS: &[&ds_cli_desktop::ops::BridgeOp] =
    &[&elevation::OPERATION, &admin_bounds::OPERATION];

pub const SOURCE_ARG: Arg = Arg {
    name: "source",
    kind: ArgKind::Value,
    value: "<path>",
    required: true,
    default: None,
    choices: &[],
    summary: "Path to the local file to read. CSV, TSV, XLSX, GeoJSON, KML/KMZ, or a zipped Shapefile.",
};

pub const SEPARATOR_ARG: Arg = Arg::value(
    "separator",
    "<char>",
    "Field separator override for a delimited file. Detected when omitted.",
);

pub const SHEET_ARG: Arg = Arg::value(
    "sheet",
    "<key>",
    "Which sheet to convert, from `ds data inspect`. Defaults to the first.",
);

/// Refusals shared by the domain. A code is what a script branches on, so each
/// one names a distinct, actionable condition and carries the fix.
pub const UNREADABLE: Refusal = Refusal {
    code: "source_unreadable",
    when: "The path does not exist, or this process cannot read it.",
    remedy: "Check the path and its permissions.",
};
pub const UNSUPPORTED: Refusal = Refusal {
    code: "source_unsupported",
    when: "The file is not a format this reader recognises, or its delimiter could not be detected.",
    remedy: "Pass --separator for a delimited file, or convert the source to CSV/GeoJSON first.",
};
pub const NO_SHEET: Refusal = Refusal {
    code: "sheet_not_found",
    when: "The named --sheet is not in the file.",
    remedy: "Run `ds data inspect` to list the sheets this file holds.",
};
pub const NOT_CONVERTIBLE: Refusal = Refusal {
    code: "source_not_convertible",
    when: "The sheet yielded no layer — usually the coordinate columns were not found.",
    remedy: "Run `ds data inspect` and pass the coordinate columns it reports.",
};
pub const OUTPUT_REFUSED: Refusal = Refusal {
    code: "output_refused",
    when: "The output path already exists, or could not be written.",
    remedy: "Choose another --out path, or pass --overwrite to replace it.",
};

pub fn available() -> ds_cli_contract::spec::Availability {
    ds_cli_contract::spec::Availability::Available
}

/// The `--source` path, or a refusal naming what is missing.
pub fn required_source(
    inputs: &ds_cli_contract::Inputs,
) -> Result<String, ds_cli_contract::outcome::Failure> {
    inputs.value("source").map(str::to_string).ok_or_else(|| {
        ds_cli_contract::outcome::Failure::invalid("source_unreadable", "--source is required.")
            .remedy("Pass the path to a local file, e.g. --source ./poles.csv.")
    })
}

/// Read a local source, refusing with the caller's own path and nothing more.
pub fn read_source(path: &str) -> Result<Vec<u8>, ds_cli_contract::outcome::Failure> {
    std::fs::read(path).map_err(|error| {
        ds_cli_contract::outcome::Failure::invalid(
            "source_unreadable",
            format!("Could not read the source: {error}"),
        )
        .remedy("Check the path exists and is readable.")
    })
}

/// The file name the format dispatcher keys on. `ds-io` selects a reader by
/// extension, so the name matters even though the bytes are already in hand.
pub fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string())
}
