//! The datasets a project DECLARES — the kernel's answer, mapped to the
//! datasets that can hold them.
//!
//! Available is not declared. Everything published for a country is available
//! to a project; what the project's workflow declares is the narrower set its
//! print templates start with, decided once in `printing::context`: the
//! catalogue's `default_on` rows plus the derived sources. Nothing here
//! consults what is held — a machine that holds nothing must still seed what
//! the project declares, which is the first-use case.

use ds_command_kernel::printing::context::{catalog, derived_layers, CatalogResource};
use ds_command_kernel::printing::PrintContextSource;
use ds_command_kernel::project_dataset_cache::{Dataset, SeedCandidate};

use crate::catalog::ReferenceResource;
use crate::datasets::{buildings_dataset, catalog_dataset, contours_dataset, ContourSettings};
use crate::Failure;

/// Where a declared dataset's rows come from.
#[derive(Debug, Clone, PartialEq)]
pub enum DeclaredSource {
    /// A project subset of one installed national bundle.
    Catalog(Box<ReferenceResource>),
    /// Building footprints through ds-brain.
    Buildings,
    /// Elevation contours through ds-brain, at the template's intervals.
    Contours(ContourSettings),
    /// A cloud-resident dataset (foundation-datasets contract): never a
    /// national bundle, seeded per project cell by cell through the bounded
    /// read — the parcels a project's extents cover, its existing customers —
    /// and held like building footprints, so the project reuses them.
    Cloud(Box<ReferenceResource>),
}

#[derive(Debug, Clone)]
pub struct Declared {
    pub candidate: SeedCandidate,
    pub dataset: Dataset,
    pub source: DeclaredSource,
}

/// Declaration facts for the kernel: installed-ness must not narrow what a
/// project declares, so `ready` is reported false for every row here.
fn declaration_facts(resources: &[ReferenceResource]) -> Vec<CatalogResource> {
    resources
        .iter()
        .map(|resource| CatalogResource {
            layer: resource.layer.clone(),
            label: resource.label.clone(),
            country: resource.country.clone(),
            ready: false,
            downloadable: resource.bundle().is_some(),
            unavailable: resource.is_unavailable(),
        })
        .collect()
}

/// The datasets the project's workflow declares, unordered on purpose: the
/// order a seed acquires in belongs to `project_dataset_cache::seed_targets`.
pub fn declared(resources: &[ReferenceResource]) -> Result<Vec<Declared>, Failure> {
    let mut out: Vec<Declared> = Vec::new();
    for layer in derived_layers() {
        let (dataset, source) = match layer.source {
            PrintContextSource::GoogleOpenBuildings { .. } => {
                (buildings_dataset(), DeclaredSource::Buildings)
            }
            PrintContextSource::ElevationContours {
                minor_interval_m,
                index_interval_m,
                sample_spacing_m,
                ..
            } => {
                let settings = ContourSettings {
                    minor_interval_m,
                    index_interval_m,
                    sample_spacing_m,
                };
                (
                    contours_dataset(settings),
                    DeclaredSource::Contours(settings),
                )
            }
            // The project's MV models have no room; they are projected per
            // print by a host that holds the model cache.
            _ => continue,
        };
        out.push(Declared {
            candidate: SeedCandidate {
                id: dataset.id.clone(),
                label: layer.label.clone(),
                seeded: false,
            },
            dataset,
            source,
        });
    }
    let rows = catalog(&declaration_facts(resources)).map_err(Failure::CatalogInvalid)?;
    for row in rows {
        if !row.default_on {
            continue;
        }
        let Some(resource) = resources
            .iter()
            .find(|candidate| candidate.layer == row.layer.id)
        else {
            continue;
        };
        if out.iter().any(|held| held.dataset.id == resource.id) {
            continue;
        }
        out.push(Declared {
            candidate: SeedCandidate {
                id: resource.id.clone(),
                label: resource.label.clone(),
                seeded: false,
            },
            dataset: catalog_dataset(resource),
            source: DeclaredSource::Catalog(Box::new(resource.clone())),
        });
    }
    Ok(out)
}

/// Resolve one dataset id — declared or explicitly named — to its source.
///
/// A name may be an exact id, a layer name, or a retired alias; the row it
/// answers as is the authority (`docs/contracts/foundation-datasets.md` R4).
/// A cloud-resident row is never a seed target and refuses by name
/// (`dataset_cloud_only`); a retired broad layer with no single authority
/// refuses with its detailed alternatives (`dataset_retired`).
pub fn resolve(resources: &[ReferenceResource], id: &str) -> Result<Option<Declared>, Failure> {
    if let Some(found) = declared(resources)?
        .into_iter()
        .find(|entry| entry.dataset.id == id)
    {
        return Ok(Some(found));
    }
    let Some(found) = crate::catalog::named(resources, id) else {
        if let Some(alternatives) = ds_command_kernel::printing::context::retired_alternatives(id) {
            return Err(Failure::Retired(format!(
                "{id} is retired; its detailed alternatives are {}",
                alternatives.join(", ")
            )));
        }
        return Ok(None);
    };
    let resource = found.resource;
    if resource.is_unavailable() {
        return Ok(None);
    }
    if resource.is_cloud_resident() {
        // Seedable per project (the owner's generalization, 2026-09-20): the
        // project's extents are read from the cloud cell by cell and held.
        // The national install is what a cloud row refuses.
        return Ok(Some(Declared {
            candidate: SeedCandidate {
                id: resource.id.clone(),
                label: resource.label.clone(),
                seeded: false,
            },
            dataset: cloud_dataset(resource),
            source: DeclaredSource::Cloud(Box::new(resource.clone())),
        }));
    }
    Ok(Some(Declared {
        candidate: SeedCandidate {
            id: resource.id.clone(),
            label: resource.label.clone(),
            seeded: false,
        },
        dataset: catalog_dataset(resource),
        source: DeclaredSource::Catalog(Box::new(resource.clone())),
    }))
}

/// The room of a cloud-resident dataset: the bounded-read door, addressed
/// by the catalogue row's layer so the host knows which read answers it.
pub fn cloud_dataset(resource: &ReferenceResource) -> Dataset {
    let mut dataset = catalog_dataset(resource);
    dataset.provider = CLOUD_READ_PROVIDER.to_owned();
    dataset.parameters.insert(
        "layer".to_owned(),
        serde_json::Value::String(resource.layer.clone()),
    );
    dataset
}

/// The governed door a cloud-resident dataset is seeded through.
/// The kernel records the area rule against this provider name
/// (`project_dataset_cache::acquisition_selection`); one constant, re-exported.
pub use ds_command_kernel::project_dataset_cache::CLOUD_READ_PROVIDER;
