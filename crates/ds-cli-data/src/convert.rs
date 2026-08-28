//! `ds data convert` — a local source to the analytical format.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{
    Arg, ArgKind, Authority, Chapter, Command, Effect, Example, Execution,
};
use ds_cli_contract::{Context, Inputs};
use serde_json::{Value, json};

pub const OUT_ARG: Arg = Arg {
    name: "out",
    kind: ArgKind::Value,
    value: "<path>",
    required: true,
    default: None,
    choices: &[],
    summary: "Where to write the GeoParquet file. Refused if it already exists unless --overwrite.",
};

pub static COMMAND: Command = Command {
    id: "data.convert",
    path: &["data", "convert"],
    contract: 1,
    summary: "Convert a local source to GeoParquet, before anything analyses it.",
    purpose: "\
Writes one GeoParquet file to the columnar format contract: WKB geometry, \
CRS84 declared explicitly, statistics on every column. Conversion consumes \
what `ds data inspect` reports — it does not re-derive the delimiter, header \
row or coordinate columns. The receipt carries a source digest and a \
conversion id, so re-converting an unchanged source with unchanged options is \
detectable and the artifact can be reclaimed later by identity.",
    chapter: Chapter::Data,
    effect: Effect::LocalFileWrite,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        crate::SOURCE_ARG,
        OUT_ARG,
        crate::SHEET_ARG,
        crate::SEPARATOR_ARG,
        Arg::value(
            "x-column",
            "<name>",
            "Column holding the easting/longitude.",
        ),
        Arg::value(
            "y-column",
            "<name>",
            "Column holding the northing/latitude.",
        ),
        Arg {
            name: "crs",
            kind: ArgKind::Value,
            value: "<crs>",
            required: false,
            default: Some("wgs84_lonlat"),
            choices: &["wgs84_lonlat", "nix_itrf2005", "utm", "none"],
            summary: "How to read the coordinate columns. Output is always CRS84.",
        },
        Arg::switch(
            "attributes-only",
            "Convert the table without geometry, even when coordinate columns exist.",
        ),
        Arg::value(
            "layer",
            "<name>",
            "Which layer to convert, for a source that holds more than one.",
        ),
        Arg::switch("overwrite", "Replace an existing output file."),
    ],
    output: "\
`out`, `layer_name`, `feature_count`, `column_count`, `byte_count`, \
`source_digest`, `conversion_id`, `skipped_coordinate_rows`.",
    examples: &[Example {
        command: "ds data convert --source ./poles.csv --out ./poles.parquet --x-column lon --y-column lat",
        note: "Run `ds data inspect` first to learn the sheet key and column names.",
        runnable: false,
    }],
    refusals: &[
        crate::UNREADABLE,
        crate::UNSUPPORTED,
        crate::NO_SHEET,
        crate::NOT_CONVERTIBLE,
        crate::OUTPUT_REFUSED,
    ],
    reference: Some("docs/reference/data.md"),
    availability: crate::available,
};

pub fn run(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let source = crate::required_source(inputs)?;
    let out = inputs
        .value("out")
        .ok_or_else(|| Failure::invalid("output_refused", "--out is required."))?;
    let out_path = std::path::PathBuf::from(out);
    if out_path.exists() && !inputs.switch("overwrite") {
        return Err(
            Failure::conflict("output_refused", "The output file already exists.")
                .remedy("Choose another --out path, or pass --overwrite to replace it."),
        );
    }

    let bytes = crate::read_source(&source)?;
    let name = crate::file_name(&source);
    let separator = inputs.value("separator").map(str::to_string);

    // A source that already carries geometry needs no coordinate columns; a
    // table does, and inspection is what supplies the sheet key rather than a
    // guess.
    let input = if ds_columnar::convert::carries_geometry(&name) {
        None
    } else {
        let inspection = ds_columnar::convert::inspect(&name, &bytes, separator.clone())
            .map_err(|error| Failure::invalid("source_unsupported", error))?;
        let sheet_key = match inputs.value("sheet") {
            Some(wanted) => inspection
                .sheets
                .iter()
                .find(|sheet| sheet.key == wanted)
                .map(|sheet| sheet.key.clone())
                .ok_or_else(|| {
                    Failure::invalid("sheet_not_found", format!("No sheet named `{wanted}`."))
                        .remedy("Run `ds data inspect` to list the sheets this file holds.")
                })?,
            None => inspection
                .sheets
                .first()
                .map(|sheet| sheet.key.clone())
                .ok_or_else(|| {
                    Failure::invalid(
                        "source_not_convertible",
                        "The source holds no sheet to convert.",
                    )
                })?,
        };
        Some(
            serde_json::from_value(json!({
                "sheetKey": sheet_key,
                "separator": separator,
                "xColumn": inputs.value("x-column"),
                "yColumn": inputs.value("y-column"),
                "crs": inputs.value("crs").unwrap_or("wgs84_lonlat"),
                "attributesOnly": inputs.switch("attributes-only"),
            }))
            .map_err(|error| Failure::internal("source_not_convertible", error.to_string()))?,
        )
    };

    let conversion = ds_columnar::source_to_geoparquet(&name, &bytes, input, inputs.value("layer"))
        .map_err(|error| {
            Failure::invalid("source_not_convertible", error).remedy(
                "Run `ds data inspect` and pass the layer or coordinate columns it reports.",
            )
        })?;

    std::fs::write(&out_path, &conversion.parquet).map_err(|error| {
        Failure::failed(
            "output_refused",
            format!("Could not write the output file: {error}"),
        )
    })?;

    let receipt = &conversion.receipt;
    Ok(json!({
        "out": out,
        "layer_name": receipt.layer_name,
        "feature_count": receipt.feature_count,
        "column_count": receipt.column_count,
        "byte_count": receipt.byte_count,
        "source_digest": receipt.source_digest,
        "conversion_id": receipt.conversion_id,
        "skipped_coordinate_rows": receipt.skipped_coordinate_rows,
    }))
}

pub fn render(data: &Value) -> String {
    let mut out = format!(
        "wrote {}\n  {} features, {} columns, {} bytes\n",
        data["out"].as_str().unwrap_or("?"),
        data["feature_count"].as_u64().unwrap_or(0),
        data["column_count"].as_u64().unwrap_or(0),
        data["byte_count"].as_u64().unwrap_or(0),
    );
    let skipped = data["skipped_coordinate_rows"].as_u64().unwrap_or(0);
    if skipped > 0 {
        out.push_str(&format!(
            "  {skipped} rows had no usable coordinate and carry no geometry\n"
        ));
    }
    out.push_str(&format!(
        "  conversion {}\n",
        data["conversion_id"].as_str().unwrap_or("?")
    ));
    out
}
