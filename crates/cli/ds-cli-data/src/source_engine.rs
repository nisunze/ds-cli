//! The host half of reading a local GIS source (DECOUPLING-013).
//!
//! `ds data inspect` and `ds data convert` admit the file here — its path, its
//! bytes, a bare shapefile's sidecars — and the Network engine reads it behind
//! the closed `gis_source_io_v1` calls. A reader's refusal keeps its own words;
//! an engine that is absent or cannot serve the call is a typed refusal.
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::Refusal;
use ds_network_client::{CapturedNetworkClient, NetworkFailure};
use ds_network_contracts::source_io::SourceReaderV1;
use std::io::{Cursor, Write};

pub const ENGINE_MISSING: Refusal = Refusal {
    code: "network_engine_missing",
    when: "No Network engine is installed for this lane; local sources are read there.",
    remedy: "Install Network, or set DS_NETWORK_ENGINE_BIN to a compatible engine.",
};
pub const ENGINE_INCOMPATIBLE: Refusal = Refusal {
    code: "network_engine_incompatible",
    when: "The Network engine does not serve gis_source_io_v1.",
    remedy: "Update Network to an engine that reads local GIS sources.",
};
pub const ENGINE_ORDER: Refusal = Refusal {
    code: "network_serialization_incompatible",
    when: "The Network engine sorts JSON members; a source read keeps its reader's order.",
    remedy: "Select the insertion-order Network engine the release ships.",
};

/// The lane's Network engine, captured once for this command.
pub(crate) fn capture() -> Result<CapturedNetworkClient, Failure> {
    let lane = ds_cli_auth::reliability::release_lane();
    match lane {
        "local" => CapturedNetworkClient::capture(1, &|| false),
        _ => CapturedNetworkClient::capture_for_lane(lane, 1, &|| false),
    }
    .map_err(engine_failure)
}

/// Split a call's failure: the reader's own refusal (its words, for the
/// command to state in its own code), or a typed engine refusal.
pub(crate) fn split(error: NetworkFailure) -> Result<String, Failure> {
    if error.is_source_refusal() {
        Ok(error.message)
    } else {
        Err(engine_failure(error))
    }
}

pub(crate) fn engine_failure(error: NetworkFailure) -> Failure {
    let code = error.code();
    let message = error.message.clone();
    match code {
        "network_engine_missing" | "module_removed" => {
            Failure::unavailable(ENGINE_MISSING.code, message).remedy(ENGINE_MISSING.remedy)
        }
        "network_engine_incompatible" => Failure::unavailable(ENGINE_INCOMPATIBLE.code, message)
            .remedy(ENGINE_INCOMPATIBLE.remedy),
        "network_serialization_incompatible" => {
            Failure::unavailable(ENGINE_ORDER.code, message).remedy(ENGINE_ORDER.remedy)
        }
        _ => Failure::failed(code, message),
    }
}

/// The reader a source that carries its own geometry is read with, by name.
pub(crate) fn geometry_reader(name: &str) -> SourceReaderV1 {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".kmz") {
        SourceReaderV1::Kmz
    } else if lower.ends_with(".zip") {
        SourceReaderV1::Archive
    } else if lower.ends_with(".kml") {
        SourceReaderV1::Kml
    } else {
        SourceReaderV1::Geojson
    }
}

/// The sidecars a bare `.shp` brings from its own folder.
const SHAPEFILE_SIDECARS: [&str; 4] = ["shx", "dbf", "prj", "cpg"];

/// A bare `.shp` path and the sidecars beside it (same stem in any case,
/// regular files only), packed as the zipped shapefile every DS reader takes:
/// deflated, members in folder order, every time stamp at the DOS epoch, so
/// the same files are the same bytes. Feedback 46dec14e / ad44f3a0:
/// `ds data inspect` and `ds data convert` refused a 10,748-point `.shp` whose
/// `.dbf`, `.shx`, `.prj` and `.cpg` sat in the same folder.
pub(crate) fn shapefile_with_sidecars_to_zip(
    shp_path: &std::path::Path,
) -> Result<Vec<u8>, String> {
    let stem = shp_path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .ok_or("the shapefile path has no file name")?;
    let folder = shp_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(std::path::Path::new("."));
    let mut members = vec![(format!("{stem}.shp"), shp_path.to_path_buf())];
    let mut taken = std::collections::HashSet::new();
    let mut entries: Vec<_> = std::fs::read_dir(folder)
        .map_err(|error| format!("could not list {}: {error}", folder.display()))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        if !entry.file_type().is_ok_and(|kind| kind.is_file()) {
            continue;
        }
        let path = entry.path();
        let (Some(entry_stem), Some(extension)) = (
            path.file_stem().and_then(|value| value.to_str()),
            path.extension().and_then(|value| value.to_str()),
        ) else {
            continue;
        };
        let extension = extension.to_ascii_lowercase();
        if entry_stem.to_lowercase() == stem.to_lowercase()
            && SHAPEFILE_SIDECARS.contains(&extension.as_str())
            && taken.insert(extension.clone())
        {
            members.push((format!("{stem}.{extension}"), path));
        }
    }
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, path) in members {
        let bytes = std::fs::read(&path)
            .map_err(|error| format!("could not read {}: {error}", path.display()))?;
        zip.start_file(name, options)
            .map_err(|error| error.to_string())?;
        zip.write_all(&bytes).map_err(|error| error.to_string())?;
    }
    Ok(zip
        .finish()
        .map_err(|error| error.to_string())?
        .into_inner())
}

/// A GeoParquet fixture from a GeoJSON FeatureCollection's own features, for
/// tests of what reads an artifact (no reader involved).
#[cfg(test)]
pub(crate) fn fixture_geoparquet(name: &str, source: &[u8]) -> ds_columnar::Conversion {
    let document: serde_json::Value = serde_json::from_slice(source).unwrap();
    let layer = ds_columnar::SourceLayer {
        name: name.split('.').next().unwrap_or(name).to_string(),
        features: document["features"].as_array().unwrap().clone(),
        style_document: None,
    };
    ds_columnar::layers_to_geoparquet(name, source, None, vec![layer], None, 0).unwrap()
}

#[cfg(test)]
#[path = "source_engine_tests.rs"]
mod tests;
