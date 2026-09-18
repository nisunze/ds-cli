//! `ds data inspect` — what a local source contains, before converting it.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, Authority, Chapter, Command, Effect, Example, Execution, Requires,
};
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
    args: &[
        crate::SOURCE_ARG,
        crate::SEPARATOR_ARG,
        Arg::value(
            "rows",
            "<0..5000>",
            "For a converted GeoParquet source, return this many attribute rows (default 0); geometry is omitted by default.",
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
source that already carries geometry. Converted GeoParquet returns its exact footer summary and optional bounded attribute rows, with total and truncated counts.",
    examples: &[Example {
        command: "ds data inspect --source ./poles.csv --output json",
        note: "`.data.sheets[0].key` is what `ds data convert --sheet` takes.",
        runnable: false,
    }],
    refusals: &[crate::UNREADABLE, crate::UNSUPPORTED],
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
    if name.to_ascii_lowercase().ends_with(".parquet") {
        let limit = inputs
            .value("rows")
            .unwrap_or("0")
            .parse::<usize>()
            .ok()
            .filter(|n| *n <= 5000)
            .ok_or_else(|| Failure::invalid("source_unsupported", "--rows must be 0..5000"))?;
        return inspect_parquet(
            std::path::Path::new(&source),
            limit,
            inputs.switch("geometry"),
        );
    }
    if inputs.value("rows").is_some() || inputs.switch("geometry") {
        return Err(Failure::invalid(
            "source_unsupported",
            "--rows reads converted GeoParquet; run data convert first",
        ));
    }
    let bytes = crate::read_source(&source)?;
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

fn inspect_parquet(path: &std::path::Path, limit: usize, geometry: bool) -> Result<Value, Failure> {
    let mut reader = ds_columnar::ArtifactReader::open(path, limit.clamp(1, 5000))
        .map_err(|e| Failure::invalid("source_unsupported", e))?;
    let summary = reader.summary().clone();
    let mut rows = Vec::new();
    while rows.len() < limit {
        let Some(chunk) = reader
            .next_chunk()
            .map_err(|e| Failure::invalid("source_unsupported", e))?
        else {
            break;
        };
        let mut values = ds_columnar::ipc_to_rows(&chunk.ipc)
            .map_err(|e| Failure::invalid("source_unsupported", e))?;
        if geometry {
            let shapes = ds_columnar::ipc_to_geometry_wkb(&chunk.ipc)
                .map_err(|e| Failure::invalid("source_unsupported", e))?;
            for (row, shape) in values.iter_mut().zip(shapes) {
                let shape = shape
                    .map(|wkb| ds_io::gpkg_geometry_to_geojson(&wkb))
                    .transpose()
                    .map_err(|e| Failure::invalid("source_unsupported", e))?
                    .and_then(|s| s.geometry)
                    .unwrap_or(Value::Null);
                let properties = std::mem::take(row);
                *row = serde_json::Map::from_iter([
                    ("type".into(), json!("Feature")),
                    ("properties".into(), json!(properties)),
                    ("geometry".into(), shape),
                ]);
            }
        }
        rows.extend(values.into_iter().take(limit - rows.len()));
    }
    let result = json!({"source": path.file_name().unwrap_or_default().to_string_lossy(), "format":"geoparquet", "summary":summary, "rows":rows, "returned":rows.len(), "total":summary.feature_count, "truncated":rows.len() < summary.feature_count});
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
mod tests {
    use super::*;
    #[test]
    fn converted_rows_are_bounded_and_geometry_keeps_properties() {
        let source = br#"{"type":"FeatureCollection","features":[{"type":"Feature","properties":{"city":"A","phase":"II"},"geometry":{"type":"LineString","coordinates":[[15,8],[15.1,8.2]]}},{"type":"Feature","properties":{"city":"B","phase":"I"},"geometry":null}]}"#;
        let converted =
            ds_columnar::source_to_geoparquet("routes.geojson", source, None, None).unwrap();
        let path = std::env::temp_dir().join(format!(
            "ds-inspect-{}-{}.parquet",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, &converted.parquet).unwrap();
        let meta = inspect_parquet(&path, 0, false).unwrap();
        assert_eq!(meta["total"], 2);
        assert_eq!(meta["returned"], 0);
        assert_eq!(
            meta["summary"]["source_digest"],
            converted.receipt.source_digest
        );
        let bounded = inspect_parquet(&path, 1, true).unwrap();
        assert_eq!(bounded["truncated"], true);
        assert_eq!(bounded["rows"][0]["properties"]["phase"], "II");
        assert_eq!(
            bounded["rows"][0]["geometry"]["coordinates"],
            json!([[15., 8.], [15.1, 8.2]])
        );
        let full = inspect_parquet(&path, 3, true).unwrap();
        assert_eq!(full["truncated"], false);
        assert!(full["rows"][1]["geometry"].is_null());
        std::fs::remove_file(path).unwrap();
    }
}
