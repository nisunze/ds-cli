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
            "--rows reads converted GeoParquet; run data convert first",
        ));
    }
    let (name, bytes) = read_geometry_source(&source)?;
    // A source that carries its own geometry has layers, not sheets, and needs
    // no coordinate columns named. Reporting sheets for it would invite the
    // caller to supply columns that mean nothing.
    if ds_columnar::convert::carries_geometry(&name) {
        let layers =
            ds_columnar::convert::inspect_geometry_source(&name, &bytes).map_err(|error| {
                Failure::invalid("source_unsupported", error)
                    .remedy("Check the file is a readable GeoJSON, KML/KMZ, or Shapefile (a .shp with its .dbf beside it, or zipped).")
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

/// The name a reader dispatches on and the bytes it reads. A bare `.shp`
/// brings the `.shx`, `.dbf`, `.prj` and `.cpg` beside it as the zipped
/// shapefile every reader takes (feedback 46dec14e: a 10,748-point `.shp` was
/// refused, so its five files had to be zipped by hand first).
pub(crate) fn read_geometry_source(source: &str) -> Result<(String, Vec<u8>), Failure> {
    let name = crate::file_name(source);
    if name.to_ascii_lowercase().ends_with(".shp") {
        let bytes = ds_io::shapefile_with_sidecars_to_zip(std::path::Path::new(source)).map_err(
            |error| {
                Failure::invalid(
                    "source_unreadable",
                    format!("Could not read the shapefile: {error}"),
                )
                .remedy("Check the .shp and its .dbf are readable in the same folder.")
            },
        )?;
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
/// layer could not be read past its first 5,000 rows).
fn inspect_parquet(
    path: &std::path::Path,
    offset: usize,
    limit: usize,
    geometry: bool,
) -> Result<Value, Failure> {
    let mut reader = ds_columnar::ArtifactReader::open(path, 5000)
        .map_err(|e| Failure::invalid("source_unsupported", e))?;
    let summary = reader.summary().clone();
    let mut rows = Vec::new();
    let mut skip = offset;
    while rows.len() < limit {
        let Some(chunk) = reader
            .next_chunk()
            .map_err(|e| Failure::invalid("source_unsupported", e))?
        else {
            break;
        };
        if skip >= chunk.rows {
            skip -= chunk.rows;
            continue;
        }
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
        let wanted = limit - rows.len();
        rows.extend(
            values
                .into_iter()
                .skip(std::mem::take(&mut skip))
                .take(wanted),
        );
    }
    let total = summary.feature_count;
    let end = offset.saturating_add(rows.len());
    let next_offset = (end < total).then_some(end);
    let result = json!({"source": path.file_name().unwrap_or_default().to_string_lossy(), "format":"geoparquet", "summary":summary, "rows":rows, "returned":rows.len(), "total":total, "offset":offset, "next_offset":next_offset, "truncated":next_offset.is_some()});
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
        let meta = inspect_parquet(&path, 0, 0, false).unwrap();
        assert_eq!(meta["total"], 2);
        assert_eq!(meta["returned"], 0);
        assert_eq!(
            meta["summary"]["source_digest"],
            converted.receipt.source_digest
        );
        let bounded = inspect_parquet(&path, 0, 1, true).unwrap();
        assert_eq!(bounded["truncated"], true);
        assert_eq!(bounded["rows"][0]["properties"]["phase"], "II");
        assert_eq!(
            bounded["rows"][0]["geometry"]["coordinates"],
            json!([[15., 8.], [15.1, 8.2]])
        );
        let full = inspect_parquet(&path, 0, 3, true).unwrap();
        assert_eq!(full["truncated"], false);
        assert!(full["rows"][1]["geometry"].is_null());
        std::fs::remove_file(path).unwrap();
    }

    // Feedback 46dec14e: a 10,748-point layer returned its first 5,000 rows
    // and nothing could read the rest, so the GeoJSON had to be assembled
    // outside ds. --offset pages through every row exactly once.
    #[test]
    fn converted_rows_page_through_a_layer_larger_than_one_request() {
        let features: Vec<String> = (0..12_000)
            .map(|index| {
                format!(
                    r#"{{"type":"Feature","properties":{{"n":{index}}},"geometry":{{"type":"Point","coordinates":[{},{}]}}}}"#,
                    29.0 + index as f64 * 1e-5,
                    -2.0
                )
            })
            .collect();
        let source = format!(
            r#"{{"type":"FeatureCollection","features":[{}]}}"#,
            features.join(",")
        );
        let converted =
            ds_columnar::source_to_geoparquet("customers.geojson", source.as_bytes(), None, None)
                .unwrap();
        let path = std::env::temp_dir().join(format!(
            "ds-inspect-pages-{}-{}.parquet",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, &converted.parquet).unwrap();
        let mut offset = 0usize;
        let mut seen = Vec::new();
        let mut pages = Vec::new();
        loop {
            let page = inspect_parquet(&path, offset, 5000, true).unwrap();
            assert_eq!(page["total"], 12_000);
            assert_eq!(page["offset"], offset);
            for row in page["rows"].as_array().unwrap() {
                seen.push(row["properties"]["n"].as_u64().unwrap());
                assert_eq!(row["geometry"]["type"], "Point");
            }
            pages.push(page["returned"].as_u64().unwrap());
            match page["next_offset"].as_u64() {
                Some(next) => {
                    assert_eq!(page["truncated"], true);
                    offset = next as usize;
                }
                None => {
                    assert_eq!(page["truncated"], false);
                    break;
                }
            }
        }
        assert_eq!(pages, [5000, 5000, 2000]);
        assert_eq!(seen, (0..12_000).collect::<Vec<u64>>());
        let past_end = inspect_parquet(&path, 20_000, 10, false).unwrap();
        assert_eq!(past_end["returned"], 0);
        assert!(past_end["next_offset"].is_null());
        std::fs::remove_file(path).unwrap();
    }

    /// One WGS84 point with one text attribute, as the three files a GIS
    /// tool writes: `.shp`, `.shx` and `.dbf`.
    fn one_point_shapefile(x: f64, y: f64, name: &str) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let header = |words: i32| {
            let mut bytes = Vec::with_capacity(100);
            bytes.extend(9994i32.to_be_bytes());
            bytes.extend([0u8; 20]);
            bytes.extend(words.to_be_bytes());
            bytes.extend(1000i32.to_le_bytes());
            bytes.extend(1i32.to_le_bytes());
            for value in [x, y, x, y, 0.0, 0.0, 0.0, 0.0] {
                bytes.extend(value.to_le_bytes());
            }
            bytes
        };
        let mut shp = header(64);
        shp.extend(1i32.to_be_bytes());
        shp.extend(10i32.to_be_bytes());
        shp.extend(1i32.to_le_bytes());
        shp.extend(x.to_le_bytes());
        shp.extend(y.to_le_bytes());
        let mut shx = header(54);
        shx.extend(50i32.to_be_bytes());
        shx.extend(10i32.to_be_bytes());
        let mut dbf = vec![0x03, 126, 10, 9];
        dbf.extend(1u32.to_le_bytes());
        dbf.extend(65u16.to_le_bytes());
        dbf.extend(11u16.to_le_bytes());
        dbf.extend([0u8; 20]);
        let mut field = [0u8; 32];
        field[..4].copy_from_slice(b"NAME");
        field[11] = b'C';
        field[16] = 10;
        dbf.extend(field);
        dbf.push(0x0D);
        dbf.push(b' ');
        dbf.extend(format!("{name:<10}").bytes());
        dbf.push(0x1A);
        (shp, shx, dbf)
    }

    // Feedback 46dec14e / ad44f3a0: `ds data inspect` and `ds data convert`
    // refused a bare .shp whose sidecars sat beside it.
    #[test]
    fn a_bare_shapefile_is_read_with_the_sidecars_beside_it() {
        let folder = std::env::temp_dir().join(format!(
            "ds-inspect-shp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let (shp, shx, dbf) = one_point_shapefile(29.55, -2.47, "Bushengo");
        std::fs::write(folder.join("customers.shp"), shp).unwrap();
        std::fs::write(folder.join("customers.shx"), shx).unwrap();
        std::fs::write(folder.join("Customers.DBF"), dbf).unwrap();
        let source = folder.join("customers.shp");
        let (name, bytes) = read_geometry_source(source.to_str().unwrap()).unwrap();
        assert_eq!(name, "customers.shp.zip");
        let layers = ds_columnar::convert::inspect_geometry_source(&name, &bytes).unwrap();
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].name, "customers");
        assert_eq!(layers[0].feature_count, 1);
        let converted = ds_columnar::source_to_geoparquet(&name, &bytes, None, None).unwrap();
        assert_eq!(converted.receipt.feature_count, 1);

        let missing = read_geometry_source(folder.join("absent.shp").to_str().unwrap())
            .err()
            .expect("an absent .shp is refused");
        assert_eq!(missing.code(), "source_unreadable");
        std::fs::remove_dir_all(folder).unwrap();
    }
}
