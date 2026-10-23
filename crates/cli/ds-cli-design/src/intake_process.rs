//! The processing step of `ds design intake upload`, run on this machine.
//!
//! ds-brain only receives the uploaded file; it never processes it. Each
//! uploaded design file is processed here through the owners `ds design lv
//! project-run` composes: the Network engine's readers turn the local file into
//! the LV layers it names, the Network contracts build one
//! `ds.lv.process.request/v1` job with the project's configuration and the
//! mode's preset, the installed Network engine processes it by request and
//! response, and the result is saved over the transformer through the fenced
//! governed save. A file whose transformer is not registered in the project
//! is refused by name: registering a transformer is the Desktop's create step.

use std::collections::BTreeMap;
use std::path::Path;

use ds_cli_contract::Failure;
use ds_command_kernel::report_export::jcs::layers_content_digest;
use ds_network_contracts::lv_processing::encode_native_lv_request_with_config;
use ds_network_contracts::source_io::SourceReaderV1;
use serde_json::{Map, Value, json};

/// The LV layers a processed transformer is made of; any other layer in an
/// uploaded file is reported as ignored, never guessed into one of these.
pub const LV_LAYERS: [&str; 6] = [
    "customers",
    "lv_lines",
    "lv_poles",
    "service_cables",
    "spans",
    "tr",
];
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;
const MAX_ROWS: usize = 100_000;

pub(crate) fn invalid(message: impl Into<String>) -> Failure {
    Failure::invalid("status_upload_invalid", message)
        .remedy("Pass bounded regular files and a JSON object of scalar process settings")
}

fn unrecognized(message: impl Into<String>) -> Failure {
    Failure::invalid("status_upload_layers_unrecognized", message).remedy(
        "name the file's layers customers, lv_lines, lv_poles, service_cables, spans and tr, as a DS LV export does",
    )
}

fn unregistered(transformer: &str, project: &str) -> Failure {
    Failure::conflict(
        "status_upload_transformer_unregistered",
        format!("transformer {transformer} (from the file name) is not registered in {project}; nothing was saved"),
    )
    .remedy("create the transformer in DS GridDesign (Design > New transformer), or name the file after an existing transformer")
}

/// The LV layer a source layer name means, if any: its last path segment,
/// without extension, in lower case.
fn lv_layer(name: &str) -> Option<&'static str> {
    let segment = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let stem = segment
        .split('.')
        .next()
        .unwrap_or(segment)
        .to_ascii_lowercase();
    LV_LAYERS.into_iter().find(|layer| *layer == stem)
}

/// The LV layers of one uploaded file, read by the shared readers; unknown
/// layers are returned by name so the receipt can report them.
pub(crate) fn read_layers(path: &Path) -> Result<(BTreeMap<String, Value>, Vec<String>), Failure> {
    let metadata = std::fs::metadata(path).map_err(|error| invalid(error.to_string()))?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_FILE_BYTES {
        return Err(invalid(
            "A design file must be a regular file of at most 64 MiB.",
        ));
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| invalid("The design file name is not UTF-8."))?;
    let lower = name.to_ascii_lowercase();
    let unreadable = |error: String| {
        Failure::invalid(
            "status_upload_layers_unreadable",
            format!("{name}: {error}"),
        )
        .remedy("upload a readable .gpkg, shapefile .zip or .geojson design file")
    };
    let mut raw: Vec<(String, Value)> = Vec::new();
    // The Network engine reads the file; this host admits it (DECOUPLING-013).
    if lower.ends_with(".gpkg") {
        let requested: Vec<String> = LV_LAYERS.iter().map(|layer| (*layer).to_owned()).collect();
        let read = crate::source_read::geopackage_tables(
            path,
            &requested,
            MAX_ROWS,
            MAX_FILE_BYTES as usize,
        )
        .map_err(|error| error.into_failure(unreadable))?;
        raw.extend(read.layers);
    } else {
        let bytes = std::fs::read(path).map_err(|error| invalid(error.to_string()))?;
        let reader = if lower.ends_with(".zip") {
            SourceReaderV1::Archive
        } else if lower.ends_with(".geojson") || lower.ends_with(".json") {
            if std::str::from_utf8(&bytes).is_err() {
                return Err(invalid(format!("{name} is not UTF-8 GeoJSON")));
            }
            SourceReaderV1::Geojson
        } else {
            return Err(invalid(format!(
                "{name} is not a .gpkg, .zip or .geojson design file"
            )));
        };
        let envelope = crate::source_read::layers(name, bytes, reader)
            .map_err(|error| error.into_failure(unreadable))?;
        for layer in envelope.layers {
            raw.push((
                layer.name,
                json!({"type": "FeatureCollection", "features": layer.features}),
            ));
        }
    }
    let (mut layers, mut ignored) = (BTreeMap::new(), Vec::new());
    for (source, collection) in raw {
        match lv_layer(&source) {
            Some(layer) if !layers.contains_key(layer) => {
                layers.insert(layer.to_owned(), collection);
            }
            Some(layer) => {
                return Err(unrecognized(format!("{name} holds two {layer} layers")));
            }
            None => ignored.push(source),
        }
    }
    if layers.is_empty() {
        return Err(unrecognized(format!(
            "{name} holds no LV layer ({})",
            ignored.join(", ")
        )));
    }
    Ok((layers, ignored))
}

/// The preset a processing mode resolves to, as the LV process dialog does.
pub(crate) fn preset(mode: &str) -> &'static str {
    match mode {
        "sketch-lv" => "sketch",
        _ => "drafting",
    }
}

/// Process one uploaded file here and save the result over its transformer.
pub(crate) fn process_file(
    lane: &str,
    project: &str,
    path: &Path,
    mode: &str,
    settings: &Map<String, Value>,
    blob_path: Option<&str>,
) -> Result<Value, Failure> {
    let (layers, ignored) = read_layers(path)?;
    let stem = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_default();
    let transformer =
        ds_command_kernel::report::transformers::canonical_transformer_reference(stem);
    if transformer.is_empty()
        || ds_command_kernel::report::is_reserved_report_identity(&transformer)
    {
        return Err(invalid(format!("{stem:?} does not name a transformer")));
    }
    let context =
        ds_cli_auth::processing_transformer_context_for_project(lane, project, &transformer)
            .map_err(|failure| {
                if failure.code() == "transformer_not_found" {
                    unregistered(&transformer, project)
                } else {
                    failure
                }
            })?;
    let base_version = context.snapshot().metadata().version().ok_or_else(|| {
        Failure::conflict(
            "transformer_context_unfenced",
            "The transformer context has no server version fence.",
        )
        .remedy("Refresh or migrate the transformer until the context call returns both fences.")
    })?;
    let config = ds_cli_auth::settings_configuration_receipt(
        lane,
        project,
        ds_client_core::ProjectConfigurationChange::ReadSettings,
    )?;
    if config.identity() != context.identity() {
        return Err(Failure::conflict(
            "project_context_stale",
            "The account changed while preparing configuration.",
        )
        .remedy("Run the upload again."));
    }
    let sheets: BTreeMap<String, Value> = config.result().document["sheets"]
        .as_object()
        .ok_or_else(|| {
            Failure::failed(
                "auth_response_unreadable",
                "Project configuration has no sheets.",
            )
            .remedy("Read project configuration and report the invalid response.")
        })?
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let request = encode_native_lv_request_with_config(&transformer, &layers, &sheets)
        .map_err(|error| invalid(format!("{transformer}: {error:?}")))?;
    let operator = (!settings.is_empty()).then(|| Value::Object(settings.clone()));
    let (request, _) = crate::lv::project_export::apply_process_preset(
        &request,
        preset(mode),
        operator.as_ref(),
        Some(&sheets),
        // The packaged engine processes this upload below.
        None,
    )?;

    // The Network engine, offline, by request and response.
    let stage =
        ds_network_client::private_disk_stage().map_err(crate::lv::process::map_network_error)?;
    let outcome = ds_network_client::process_lv_with_order(
        &request,
        1,
        stage.path(),
        &|| false,
        ds_network_contracts::engine::JsonObjectOrder::Insertion,
    )
    .map_err(crate::lv::process::map_network_error)?;
    let job = outcome
        .result
        .jobs
        .first()
        .filter(|job| job.transformer_name == transformer)
        .ok_or_else(|| invalid("The Network engine answered for another transformer."))?;
    if !job.ok {
        return Err(Failure::failed(
            "fast_lv_run_process_failed",
            format!(
                "{transformer}: {} ({})",
                job.error.as_deref().unwrap_or("processing failed"),
                job.error_code.as_deref().unwrap_or("unknown"),
            ),
        )
        .remedy("correct the uploaded design file and upload it again"));
    }
    let engine = ds_network_client::CapturedNetworkClient::capture(1, &|| false)
        .map_err(crate::lv::process::map_network_error)?;
    let projection = engine
        .project_lv(
            &request,
            &outcome.bytes,
            &transformer,
            stage.path(),
            &|| false,
        )
        .map_err(crate::lv::process::map_network_error)?
        .result;
    let source_content_digest = layers_content_digest(&projection.source_layers)
        .map_err(|error| invalid(error.to_string()))?;
    let digest = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(&request));
    let operation_id = format!("intake-{}", &digest[..40]);
    let saved = ds_cli_auth::save_transformers(
        lane,
        &ds_client_core::TransformerSaveBatch {
            project_id: project.to_owned(),
            items: vec![ds_client_core::TransformerSaveItem {
                transformer_name: transformer.clone(),
                base_version,
                source_content_digest,
                source_layers: projection.source_layers,
                gdfs: projection.gdfs,
                config_dfs: projection.config_dfs,
                process_metadata: projection.process_metadata,
                process_input: projection.process_input,
                voltage_drop: projection.voltage_drop,
                operation_id,
            }],
        },
    )?;
    // The upload state machine reconciles one result row per file name.
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    Ok(json!({"results": [{
        "file_name": file_name,
        "submitted": 1,
        "computed_on": "edge",
        "transformer": transformer,
        "mode": mode,
        "preset": preset(mode),
        "base_version": base_version,
        "layers": layers.keys().collect::<Vec<_>>(),
        "ignored_layers": ignored,
        "uploaded": blob_path,
        "engine_core_version": outcome.result.engine_core_version,
        "saved": saved.result(),
    }]}))
}

#[cfg(test)]
#[path = "intake_process_tests.rs"]
mod tests;
