//! `ds data inspect` — what a local source contains, before converting it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
use ds_cli_contract::{Context, Inputs};
use ds_network_contracts::source_io::SourceInspectV1;
use serde_json::{Value, json};

pub static COMMAND: Command = Command {
    id: "data.inspect",
    path: &["data", "inspect"],
    contract: 1,
    summary: "What a local source contains: sheets, columns, detected coordinates.",
    purpose: "\
Start here. Reports every sheet in the file, the cleaned column names, which \
columns look like coordinates, and how many rows survived cleaning. Run this \
first: `ds data convert` consumes what this reports rather than guessing, so \
converting first never means guessing first.",
    chapter: Chapter::Data,
    effect: Effect::ReadOnly,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        crate::SOURCE_ARG,
        crate::SEPARATOR_ARG,
        Arg::value(
            "rows",
            "<0..5000>",
            "For a converted GeoParquet source, return this many attribute rows (default 0); geometry is omitted by default.",
        ),
        Arg::value(
            "offset",
            "<n>",
            "For a converted GeoParquet source, skip this many rows before the requested --rows (default 0); page with the returned next_offset.",
        ),
        Arg::switch(
            "geometry",
            "Include CRS84 GeoJSON geometry alongside the requested converted rows.",
        ),
    ],
    output: "\
`source`, `carries_geometry`, and either `sheets` (each with `key`, `name`, \
`columns`, `row_count`, `dropped_count`, detected `geo` columns) for a table, \
or `layers` (each with `name`, `feature_count`, `geometry_type`, `crs`) for a \
source that already carries geometry. Converted GeoParquet returns its exact footer summary and optional bounded attribute rows, with total, offset, next_offset (null on the last page) and truncated counts.",
    examples: &[Example {
        command: "ds data inspect --source ./poles.csv --output json",
        note: "`.data.sheets[0].key` is what `ds data convert --sheet` takes.",
        runnable: false,
    }],
    refusals: &crate::source_engine::with_refusals::<{ 3 + crate::source_engine::REFUSALS.len() }>(
        &[
            crate::UNREADABLE,
            crate::UNSUPPORTED,
            crate::source_engine::ENGINE_UPDATE,
        ],
    ),
    reference: Some("docs/reference/data.md"),
    search: &[
        "crs",
        "schema",
        "fields",
        "shapefile",
        "kml",
        "kmz",
        "preview",
        "profile",
    ],
    requires: Requires::Server,
    availability: crate::available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let source = crate::required_source(inputs)?;
    let name = crate::file_name(&source);
    if is_converted_geoparquet(std::path::Path::new(&source), &name) {
        let limit = inputs
            .value("rows")
            .unwrap_or("0")
            .parse::<usize>()
            .ok()
            .filter(|n| *n <= 5000)
            .ok_or_else(|| Failure::invalid("source_unsupported", "--rows must be 0..5000"))?;
        let offset = inputs
            .value("offset")
            .unwrap_or("0")
            .parse::<usize>()
            .map_err(|_| {
                Failure::invalid(
                    "source_unsupported",
                    "--offset must be a whole number of rows",
                )
            })?;
        return inspect_parquet(
            std::path::Path::new(&source),
            offset,
            limit,
            inputs.switch("geometry"),
        );
    }
    if inputs.value("rows").is_some()
        || inputs.value("offset").is_some()
        || inputs.switch("geometry")
    {
        return Err(Failure::invalid(
            "source_unsupported",
            format!("--rows, --offset and --geometry read converted GeoParquet, and {name} is not GeoParquet; run data convert first"),
        )
        .remedy("Run `ds data convert --source <file> --out <file.parquet>`, then inspect that output with --rows; its content, not its suffix, identifies it."));
    }
    let (name, bytes) = read_geometry_source(&source)?;
    let engine = crate::source_engine::capture()?;
    // A source that carries its own geometry has layers, not sheets, and needs
    // no coordinate columns named. Reporting sheets for it would invite the
    // caller to supply columns that mean nothing.
    if ds_columnar::carries_geometry(&name) {
        let reader = crate::source_engine::geometry_reader(&name);
        let layers = engine
            .source_inspect(&name, bytes, SourceInspectV1::Layers(reader), &|| false)
            .map_err(|error| match crate::source_engine::split(error) {
                Ok(error) => Failure::invalid("source_unsupported", error)
                    .remedy("Check the file is a readable GeoJSON, KML/KMZ, or Shapefile (a .shp with its .dbf beside it, or zipped)."),
                Err(failure) => failure,
            })?
            .layers;
        return Ok(json!({
            "source": name,
            "carries_geometry": true,
            "layers": layers.iter().map(|layer| json!({
                "name": layer.name,
                "feature_count": layer.feature_count,
                "geometry_type": layer.geometry_type,
                "crs": layer.crs,
            })).collect::<Vec<_>>(),
        }));
    }

    let separator = inputs.value("separator").map(str::to_string);
    let inspection = engine
        .source_inspect(&name, bytes, SourceInspectV1::Table { separator }, &|| {
            false
        })
        .map_err(|error| match crate::source_engine::split(error) {
            Ok(error) => Failure::invalid("source_unsupported", error)
                .remedy("Check the file is one of the supported formats, or pass --separator."),
            Err(failure) => failure,
        })?;

    let sheets: Vec<Value> = inspection
        .sheets
        .iter()
        .map(|sheet| {
            json!({
                "key": sheet.key,
                "name": sheet.name,
                "row_count": sheet.row_count,
                "dropped_count": sheet.dropped_count,
                "columns": sheet.columns.iter().map(|column| &column.name).collect::<Vec<_>>(),
                "geo": {
                    "x_column": sheet.geo.x_column,
                    "y_column": sheet.geo.y_column,
                },
            })
        })
        .collect();

    Ok(json!({ "source": name, "carries_geometry": false, "sheets": sheets }))
}

/// Whether a source is a Parquet file, by its content: the `PAR1` magic that
/// opens and closes every Parquet file. `ds data convert` writes to whatever
/// `--out` names, so its output is recognised by format rather than by a
/// `.parquet` suffix (feedback 8ed4a478: a `.geoparquet` output was refused).
/// A `.parquet` name still selects this reader, so a damaged artifact keeps
/// the reader's own refusal instead of falling through to the table readers.
pub(crate) fn is_converted_geoparquet(path: &std::path::Path, name: &str) -> bool {
    if name.to_ascii_lowercase().ends_with(".parquet") {
        return true;
    }
    use std::io::{Read, Seek, SeekFrom};
    const MAGIC: &[u8; 4] = b"PAR1";
    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let mut head = [0u8; 4];
    let mut tail = [0u8; 4];
    file.read_exact(&mut head).is_ok()
        && &head == MAGIC
        && file.seek(SeekFrom::End(-4)).is_ok()
        && file.read_exact(&mut tail).is_ok()
        && &tail == MAGIC
}

/// The name a reader dispatches on and the bytes it reads. A bare `.shp`
/// brings the `.shx`, `.dbf`, `.prj` and `.cpg` beside it as the zipped
/// shapefile every reader takes (feedback 46dec14e: a 10,748-point `.shp` was
/// refused, so its five files had to be zipped by hand first).
pub(crate) fn read_geometry_source(source: &str) -> Result<(String, Vec<u8>), Failure> {
    let name = crate::file_name(source);
    if name.to_ascii_lowercase().ends_with(".shp") {
        let bytes =
            crate::source_engine::shapefile_with_sidecars_to_zip(std::path::Path::new(source))
                .map_err(|error| {
                    Failure::invalid(
                        "source_unreadable",
                        format!("Could not read the shapefile: {error}"),
                    )
                    .remedy("Check the .shp and its .dbf are readable in the same folder.")
                })?;
        return Ok((format!("{name}.zip"), bytes));
    }
    Ok((name, crate::read_source(source)?))
}

pub fn render(data: &Value) -> String {
    let mut out = format!("source {}\n", data["source"].as_str().unwrap_or("?"));
    for layer in data["layers"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<28} {} features  {}  {}\n",
            layer["name"].as_str().unwrap_or("?"),
            layer["feature_count"].as_u64().unwrap_or(0),
            layer["geometry_type"].as_str().unwrap_or("mixed"),
            layer["crs"].as_str().unwrap_or("crs not declared"),
        ));
    }
    for sheet in data["sheets"].as_array().into_iter().flatten() {
        let columns = sheet["columns"].as_array().map_or(0, Vec::len);
        out.push_str(&format!(
            "  {:<20} {} rows, {} columns",
            sheet["key"].as_str().unwrap_or("?"),
            sheet["row_count"].as_u64().unwrap_or(0),
            columns,
        ));
        match (
            sheet["geo"]["x_column"].as_str(),
            sheet["geo"]["y_column"].as_str(),
        ) {
            (Some(x), Some(y)) => out.push_str(&format!("  coordinates {x}/{y}\n")),
            _ => out.push_str("  no coordinate columns detected\n"),
        }
    }
    out
}

/// One bounded page of a converted GeoParquet source: `offset` rows are
/// skipped, then at most `limit` are returned (feedback 46dec14e: a 10,748-point
/// layer could not be read past its first 5,000 rows). The Network engine
/// decodes the artifact; Core reads its bytes and bounds the page.
fn inspect_parquet(
    path: &std::path::Path,
    offset: usize,
    limit: usize,
    geometry: bool,
) -> Result<Value, Failure> {
    let bytes = std::fs::read(path).map_err(|error| {
        Failure::invalid(
            "source_unsupported",
            format!(
                "could not open the converted artifact {}: {error}",
                path.display()
            ),
        )
    })?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let engine = crate::source_engine::capture()?;
    let page = engine
        .geoparquet_rows(&name, &bytes, offset, limit, geometry, &|| false)
        .map_err(|error| match crate::source_engine::split(error) {
            Ok(error) => Failure::invalid("source_unsupported", error),
            Err(failure) => failure,
        })?;
    let held = page.summary;
    // The footer summary exactly as the converter reports it.
    let summary = json!({
        "layer_name": held.layer_name,
        "feature_count": held.feature_count,
        "columns": held.columns,
        "byte_count": held.byte_count,
        "skipped_coordinate_rows": held.skipped_coordinate_rows,
        "geometry_type": held.geometry_type,
        "source_digest": held.source_digest,
        "conversion": held.conversion,
    });
    let rows = page.rows;
    let total = held.feature_count;
    let end = offset.saturating_add(rows.len());
    let next_offset = (end < total).then_some(end);
    let result = json!({"source": name, "format":"geoparquet", "summary":summary, "rows":rows, "returned":rows.len(), "total":total, "offset":offset, "next_offset":next_offset, "truncated":next_offset.is_some()});
    if serde_json::to_vec(&result)
        .map_err(|e| Failure::invalid("source_unsupported", e.to_string()))?
        .len()
        > 8 * 1024 * 1024
    {
        return Err(Failure::invalid(
            "source_unsupported",
            "attribute rows exceed 8 MiB; request fewer --rows",
        ));
    }
    Ok(result)
}

#[cfg(test)]
#[path = "inspect_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "inspect_format_tests.rs"]
mod format_tests;
