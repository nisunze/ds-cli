//! One transformer's print context, read from the project's rooms and
//! assembled through the kernel's one document recipe — the port of the
//! desktop's `reports/transformer-print-context.ts` and
//! `project-data/print-context.ts`.
//!
//! For each selected context layer, by its source kind:
//!
//! * `catalog` — the project subset of the installed national bundle;
//! * `google_open_buildings` / `elevation_contours` — the project rooms fed
//!   through ds-brain; contours split back into the two print layers;
//! * `project_dsgrid_mv` — omitted and named: the model cache is a desktop
//!   holding for now;
//! * `survey` / `local_layer` — refused: no headless source exists.
//!
//! As on the desktop, a catalogue or derived layer that cannot be supplied is
//! a WARNING and an omission, never the end of the print; `Mode::Read` never
//! acquires, so an unheld layer is omitted with the reason and the seed
//! command that supplies it.

use ds_command_kernel::printing::context::expanded_layer_ids;
use ds_command_kernel::printing::{PrintContextLayer, PrintContextSource};
use ds_command_kernel::project_dataset_cache::{
    buffer_policy, Dataset, Scope, CONTOURS_INDEX_LAYER, CONTOURS_INTERMEDIATE_LAYER,
};
use ds_command_kernel::report_export::{print_context_document, sha256_hex};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::Path;

use crate::catalog::{find, ReferenceResource};
use crate::datasets::{buildings_dataset, catalog_dataset, contours_dataset, ContourSettings};
use crate::declared::DeclaredSource;
use crate::ensure::{ensure, read_features, Mode};
use crate::extents::extent_of;
use crate::Failure;

/// A selected layer the document does not carry, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Omission {
    pub layer: String,
    pub reason: String,
}

/// The failure one `print_context_document` refusal is. The engine's bounds
/// — layers, sources, features, bytes — are a state a host can act on
/// (`print_context_too_large`); any other refusal is a defect in what was
/// assembled (`project_dataset_store_failed`).
pub(crate) fn document_failure(error: String) -> Failure {
    if error.contains("over the") || error.contains("more than") {
        Failure::TooLarge(error)
    } else {
        Failure::Store(error)
    }
}

#[derive(Debug, Clone)]
pub struct PrintContext {
    /// The exact `ds.print-context/v1` bytes, or `None` when no layer could be
    /// supplied (the engine prints without external context).
    pub document: Option<Vec<u8>>,
    pub sha256: Option<String>,
    /// Layer ids the document carries, in document order.
    pub layers: Vec<String>,
    pub omitted: Vec<Omission>,
    pub warnings: Vec<String>,
    /// True when every supplied layer was read from held coverage and no
    /// provider was contacted.
    pub reused: bool,
}

/// The buffered rectangle one context source is read for, through the kernel's
/// own metric buffering (no metre-to-degree arithmetic lives in a host).
fn buffered_area(transformer: &str, layers: &Value, buffer_m: f64) -> Result<Value, Failure> {
    let request = json!({
        "schema": ds_command_kernel::project_design_extent::SCHEMA,
        "action": "extent",
        "name": transformer,
        "layers": layers,
        "buffer_m": buffer_m,
    });
    let bytes = serde_json::to_vec(&request).map_err(crate::store)?;
    let answer: Value = serde_json::from_str(
        &ds_command_kernel::project_design_extent::evaluate(&bytes).map_err(crate::store)?,
    )
    .map_err(crate::store)?;
    let bounds = answer["buffered"]
        .as_array()
        .filter(|b| b.len() == 4)
        .map(|b| {
            [
                b[0].as_f64().unwrap_or(f64::NAN),
                b[1].as_f64().unwrap_or(f64::NAN),
                b[2].as_f64().unwrap_or(f64::NAN),
                b[3].as_f64().unwrap_or(f64::NAN),
            ]
        })
        .ok_or_else(|| Failure::Unsupported("the design has no geographic extent".into()))?;
    let [w, s, e, n] = bounds;
    Ok(json!({"type": "Polygon", "coordinates": [[[w, s], [e, s], [e, n], [w, n], [w, s]]]}))
}

fn buffer_of(source: &PrintContextSource) -> f64 {
    match source {
        PrintContextSource::GoogleOpenBuildings { buffer_m, .. }
        | PrintContextSource::ElevationContours { buffer_m, .. }
        | PrintContextSource::ProjectDsgridMv { buffer_m, .. } => *buffer_m,
        _ => ds_command_kernel::printing::context::DERIVED_BUFFER_M,
    }
}

/// Which dataset one context source is held as, and where its rows come from.
fn dataset_for(
    source: &PrintContextSource,
    catalog: &[ReferenceResource],
) -> Result<Option<(Dataset, DeclaredSource)>, String> {
    Ok(match source {
        PrintContextSource::Catalog { country, layer } => {
            let resource = find(catalog, country, layer)
                .ok_or_else(|| "this layer is absent from the geographic catalog".to_string())?;
            if let Some(reason) = resource.unavailable.as_deref().filter(|r| !r.is_empty()) {
                return Err(reason.to_owned());
            }
            Some((
                catalog_dataset(resource),
                DeclaredSource::Catalog(Box::new(resource.clone())),
            ))
        }
        PrintContextSource::GoogleOpenBuildings { .. } => {
            Some((buildings_dataset(), DeclaredSource::Buildings))
        }
        PrintContextSource::ElevationContours {
            minor_interval_m,
            index_interval_m,
            sample_spacing_m,
            ..
        } => {
            let settings = ContourSettings {
                minor_interval_m: *minor_interval_m,
                index_interval_m: *index_interval_m,
                sample_spacing_m: *sample_spacing_m,
            };
            Some((
                contours_dataset(settings),
                DeclaredSource::Contours(settings),
            ))
        }
        _ => None,
    })
}

fn collection(features: Vec<Value>) -> Value {
    json!({"type": "FeatureCollection", "features": features})
}

/// Read one room-held source for this transformer: ensure (or read) the
/// project's coverage around the design, then read the features inside the
/// design's own buffered rectangle.
#[allow(clippy::too_many_arguments)]
fn room_layers(
    root: &Path,
    scope: &Scope,
    transformer: &str,
    layers: &Value,
    context: &PrintContextLayer,
    dataset: &Dataset,
    source: &DeclaredSource,
    mode: &mut Mode<'_, '_>,
    warnings: &mut Vec<String>,
    reused: &mut bool,
) -> Result<Vec<(String, Vec<Value>, String)>, Failure> {
    let policy = buffer_policy(&[buffer_of(&context.source)]).map_err(crate::store)?;
    let extent = extent_of(transformer, layers)?;
    if extent.bounds.is_none() {
        return Err(Failure::Unsupported(
            "the design has no geographic extent".into(),
        ));
    }
    let focus = vec![transformer.to_owned()];
    let read = ensure(
        root,
        scope,
        dataset,
        source,
        &policy,
        std::slice::from_ref(&extent),
        &focus,
        Mode::Read,
    );
    let outcome = match read {
        Ok(outcome) if outcome.covered => outcome,
        Ok(_) | Err(Failure::NotHeld(_)) => match mode {
            Mode::Read => {
                return Err(Failure::NotHeld(format!(
                    "{} is not held for this design's extent; run `ds data project-cache seed --yes`, or export with --seed",
                    context.label
                )))
            }
            Mode::Acquire(hosts) => {
                *reused = false;
                let outcome = ensure(
                    root,
                    scope,
                    dataset,
                    source,
                    &policy,
                    std::slice::from_ref(&extent),
                    &focus,
                    Mode::Acquire(hosts),
                )?;
                if !outcome.covered {
                    return Err(Failure::AcquisitionFailed(format!(
                        "{} was updated but this design's extent is still not covered",
                        context.label
                    )));
                }
                outcome
            }
        },
        Err(other) => return Err(other),
    };
    warnings.extend(outcome.warnings.iter().cloned());
    let area = outcome
        .coverage_geometry
        .clone()
        .ok_or_else(|| Failure::Unsupported("this design produced no query area".into()))?;
    let (features, version) = read_features(root, scope, dataset, &area)?;
    Ok(match source {
        DeclaredSource::Contours(_) => {
            let (mut index, mut intermediate) = (Vec::new(), Vec::new());
            for feature in features {
                match feature["properties"]["contour_kind"].as_str() {
                    Some("index") => index.push(feature),
                    _ => intermediate.push(feature),
                }
            }
            vec![
                (
                    CONTOURS_INTERMEDIATE_LAYER.to_owned(),
                    intermediate,
                    version.clone(),
                ),
                (CONTOURS_INDEX_LAYER.to_owned(), index, version),
            ]
        }
        _ => vec![(context.id.clone(), features, version)],
    })
}

/// Assemble the print context for one transformer.
///
/// `contexts` is the kernel's `ContextPreparation` answer for the selected
/// printing setups; `catalog` the validated `list_datasets` rows; `mode`
/// whether missing coverage may be acquired on this first print.
#[allow(clippy::too_many_arguments)]
pub fn read_print_context(
    root: &Path,
    scope: &Scope,
    transformer: &str,
    layers: &Value,
    contexts: &[PrintContextLayer],
    catalog: &[ReferenceResource],
    mut mode: Mode<'_, '_>,
) -> Result<PrintContext, Failure> {
    let mut document_layers: BTreeMap<String, Value> = BTreeMap::new();
    let mut sources: Vec<Value> = Vec::new();
    let mut omitted: Vec<Omission> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut reused = true;
    let mut widest = 0.0_f64;
    for context in contexts {
        match &context.source {
            PrintContextSource::Survey { .. } | PrintContextSource::LocalLayer { .. } => {
                return Err(Failure::Unsupported(format!(
                    "{} is a desktop-held context source with no headless source",
                    context.id
                )));
            }
            PrintContextSource::ProjectDsgridMv { .. } => {
                for id in expanded_layer_ids(context) {
                    omitted.push(Omission {
                        layer: id,
                        reason: "desktop_only".into(),
                    });
                }
                continue;
            }
            _ => {}
        }
        let resolved = match dataset_for(&context.source, catalog) {
            Ok(Some(resolved)) => resolved,
            Ok(None) => continue,
            Err(reason) => {
                warnings.push(format!("{}: {reason}", context.label));
                for id in expanded_layer_ids(context) {
                    omitted.push(Omission {
                        layer: id,
                        reason: reason.clone(),
                    });
                }
                continue;
            }
        };
        let (dataset, source) = resolved;
        match room_layers(
            root,
            scope,
            transformer,
            layers,
            context,
            &dataset,
            &source,
            &mut mode,
            &mut warnings,
            &mut reused,
        ) {
            Ok(rows) => {
                widest = widest.max(buffer_of(&context.source));
                for (id, features, version) in rows {
                    if features.is_empty() {
                        // The engine drops an empty collection silently; name
                        // it here instead.
                        omitted.push(Omission {
                            layer: id,
                            reason: "no features in the design's extent".into(),
                        });
                        continue;
                    }
                    sources.push(json!({
                        "layer": id,
                        "kind": context.source_kind(),
                        "dataset_id": dataset.id,
                        "source_version": version,
                        "feature_count": features.len(),
                    }));
                    document_layers.insert(id, collection(features));
                }
            }
            Err(Failure::Unsupported(message)) => return Err(Failure::Unsupported(message)),
            Err(error) => {
                warnings.push(format!("{}: {}", context.label, error.message()));
                for id in expanded_layer_ids(context) {
                    omitted.push(Omission {
                        layer: id,
                        reason: error.code().to_owned(),
                    });
                }
            }
        }
    }
    if document_layers.is_empty() {
        return Ok(PrintContext {
            document: None,
            sha256: None,
            layers: Vec::new(),
            omitted,
            warnings,
            reused,
        });
    }
    let coverage = buffered_area(transformer, layers, widest.max(0.0))?;
    let bytes = print_context_document(&coverage, &json!(sources), &document_layers)
        .map_err(document_failure)?;
    let sha256 = sha256_hex(&bytes);
    Ok(PrintContext {
        layers: document_layers.keys().cloned().collect(),
        document: Some(bytes),
        sha256: Some(sha256),
        omitted,
        warnings,
        reused,
    })
}

trait SourceKind {
    fn source_kind(&self) -> &'static str;
}

impl SourceKind for PrintContextLayer {
    fn source_kind(&self) -> &'static str {
        match self.source {
            PrintContextSource::Survey { .. } => "survey",
            PrintContextSource::LocalLayer { .. } => "local_layer",
            PrintContextSource::Catalog { .. } => "catalog",
            PrintContextSource::GoogleOpenBuildings { .. } => "google_open_buildings",
            PrintContextSource::ElevationContours { .. } => "elevation_contours",
            PrintContextSource::ProjectDsgridMv { .. } => "project_dsgrid_mv",
        }
    }
}
