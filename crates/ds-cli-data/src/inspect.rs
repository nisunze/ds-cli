//! `ds data inspect` — what a local source contains, before converting it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution};
use ds_cli_contract::{Context, Inputs};
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
    args: &[crate::SOURCE_ARG, crate::SEPARATOR_ARG],
    output: "\
`source`, `carries_geometry`, and either `sheets` (each with `key`, `name`, \
`columns`, `row_count`, `dropped_count`, detected `geo` columns) for a table, \
or `layers` (each with `name`, `feature_count`, `geometry_type`, `crs`) for a \
source that already carries geometry.",
    examples: &[Example {
        command: "ds data inspect --source ./poles.csv --output json",
        note: "`.data.sheets[0].key` is what `ds data convert --sheet` takes.",
        runnable: false,
    }],
    refusals: &[crate::UNREADABLE, crate::UNSUPPORTED],
    reference: Some("docs/reference/data.md"),
    availability: crate::available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let source = crate::required_source(inputs)?;
    let bytes = crate::read_source(&source)?;
    let name = crate::file_name(&source);
    // A source that carries its own geometry has layers, not sheets, and needs
    // no coordinate columns named. Reporting sheets for it would invite the
    // caller to supply columns that mean nothing.
    if ds_columnar::convert::carries_geometry(&name) {
        let layers =
            ds_columnar::convert::inspect_geometry_source(&name, &bytes).map_err(|error| {
                Failure::invalid("source_unsupported", error)
                    .remedy("Check the file is a readable GeoJSON, KML/KMZ, or zipped Shapefile.")
            })?;
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

    let inspection =
        ds_columnar::convert::inspect(&name, &bytes, inputs.value("separator").map(str::to_string))
            .map_err(|error| {
                Failure::invalid("source_unsupported", error)
                    .remedy("Check the file is one of the supported formats, or pass --separator.")
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
