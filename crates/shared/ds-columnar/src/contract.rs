//! The format facts Core needs without encoding or decoding a column: the
//! GeoParquet version an artifact identity folds in, the canonical CRS84
//! document an admission compares against, the provenance metadata keys and
//! which sources carry their own geometry. Pure values: no Arrow, no Parquet.
//! The encoders and decoders behind `artifacts` run in the Network engine and
//! page unit, which Core reaches through typed `columnar_io_v1` requests.

use serde_json::{Value, json};

/// GeoParquet specification version written into the `geo` metadata. Declared,
/// never left for a reader to infer.
pub const GEOPARQUET_VERSION: &str = "1.1.0";

/// Metadata key holding the layer an artifact holds.
pub const LAYER_NAME_KEY: &str = "ds:layer_name";
/// Metadata key holding the rows rejected for unusable coordinates.
pub const SKIPPED_ROWS_KEY: &str = "ds:skipped_coordinate_rows";

/// Geometry is stored in longitude/latitude WGS 84 — what `ds-io` produces for
/// every import, and GeoParquet's own default. Written explicitly all the same:
/// an unstated CRS is the same class of failure as an unstated vertical datum.
pub fn crs84_projjson() -> Value {
    json!({
        "$schema": "https://proj.org/schemas/v0.5/projjson.schema.json",
        "type": "GeographicCRS",
        "name": "WGS 84 (CRS84)",
        "datum_ensemble": {
            "name": "World Geodetic System 1984 ensemble",
            // Every realization in the ensemble, not just the newest. PROJ
            // rejects a one-member ensemble outright ("ensemble should have at
            // least 2 datums"), so a single entry made every artifact this
            // writer has ever produced unreadable by geopandas and anything
            // else built on PROJ — the failure surfaced only in the reader,
            // long after the conversion reported success.
            "members": [
                {"name": "World Geodetic System 1984 (Transit)"},
                {"name": "World Geodetic System 1984 (G730)"},
                {"name": "World Geodetic System 1984 (G873)"},
                {"name": "World Geodetic System 1984 (G1150)"},
                {"name": "World Geodetic System 1984 (G1674)"},
                {"name": "World Geodetic System 1984 (G1762)"},
                {"name": "World Geodetic System 1984 (G2139)"},
                {"name": "World Geodetic System 1984 (G2296)"}
            ],
            "ellipsoid": {
                "name": "WGS 84",
                "semi_major_axis": 6378137,
                "inverse_flattening": 298.257223563
            },
            "accuracy": "2.0"
        },
        "coordinate_system": {
            "subtype": "ellipsoidal",
            "axis": [
                {"name": "Geodetic longitude", "abbreviation": "Lon",
                 "direction": "east", "unit": "degree"},
                {"name": "Geodetic latitude", "abbreviation": "Lat",
                 "direction": "north", "unit": "degree"}
            ]
        },
        "id": {"authority": "OGC", "code": "CRS84"}
    })
}

/// Whether a source carries geometry of its own, or is a table whose
/// coordinates live in named columns.
///
/// A shapefile already knows its geometry; a CSV does not, and someone has to
/// say which columns hold it. Conflating the two would mean guessing for one of
/// them, which is precisely what the contract forbids.
pub fn carries_geometry(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".geojson", ".json", ".kml", ".kmz", ".zip", ".shp"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}
