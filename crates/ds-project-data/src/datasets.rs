//! The registry the kernel echoes: which canonical `Dataset` each print-context
//! source is held as.
//!
//! The kernel's room address is a digest of the dataset document, so these
//! documents are byte-for-byte the desktop's (`project-data/buildings.ts`,
//! `project-data/global-reference.ts`): the same account on the same machine
//! reads and writes the same rooms whichever host is running.

use ds_command_kernel::project_dataset_cache::{
    Dataset, Quality, BUILDINGS_DATASET_ID, CONTOURS_DATASET_ID, REFERENCE_CACHE_PROVIDER,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::catalog::ReferenceResource;

/// Legacy room identity, retained so existing holdings keep their address.
/// New derived acquisitions require explicitly pinned local source inputs.
pub const BRAIN_PRINT_CONTEXT_PROVIDER: &str = "ds-brain:query_print_context";

/// Authored contour intervals, as the layout's context source carries them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ContourSettings {
    pub minor_interval_m: u16,
    pub index_interval_m: u16,
    pub sample_spacing_m: f64,
}

fn dataset(id: &str, provider: &str, parameters: BTreeMap<String, Value>) -> Dataset {
    Dataset {
        id: id.to_owned(),
        provider: provider.to_owned(),
        quality: Quality::Source,
        parameters,
        source_version: String::new(),
        crs: "EPSG:4326".to_owned(),
    }
}

/// Building footprints through ds-brain's `query_print_context`. Only the
/// closed semantic kind travels; provider thresholds stay server-owned and
/// arrive inside the source receipt.
pub fn buildings_dataset() -> Dataset {
    dataset(
        BUILDINGS_DATASET_ID,
        BRAIN_PRINT_CONTEXT_PROVIDER,
        BTreeMap::from([("context_kind".to_owned(), json!(BUILDINGS_DATASET_ID))]),
    )
}

/// Elevation contours through the same door. The authored intervals are part
/// of the room address: two templates asking for different intervals hold two
/// rooms rather than one room answering both with the wrong lines.
pub fn contours_dataset(settings: ContourSettings) -> Dataset {
    dataset(
        CONTOURS_DATASET_ID,
        BRAIN_PRINT_CONTEXT_PROVIDER,
        BTreeMap::from([
            ("context_kind".to_owned(), json!(CONTOURS_DATASET_ID)),
            (
                "minor_interval_m".to_owned(),
                json!(settings.minor_interval_m),
            ),
            (
                "index_interval_m".to_owned(),
                json!(settings.index_interval_m),
            ),
            (
                "sample_spacing_m".to_owned(),
                json!(settings.sample_spacing_m),
            ),
        ]),
    )
}

/// A project subset of one complete national reference resource, extracted
/// from the machine's installed bundle. The dataset id is the resource's own
/// digest, as on the desktop.
pub fn catalog_dataset(resource: &ReferenceResource) -> Dataset {
    dataset(
        &resource.id,
        REFERENCE_CACHE_PROVIDER,
        BTreeMap::from([
            ("resource_id".to_owned(), json!(resource.id)),
            ("layer".to_owned(), json!(resource.layer)),
            ("country".to_owned(), json!(resource.country)),
        ]),
    )
}

/// The same dataset, addressed at a known source version — what the room
/// records when an acquisition completes.
pub fn at_version(dataset: &Dataset, source_version: &str) -> Dataset {
    Dataset {
        source_version: source_version.to_owned(),
        ..dataset.clone()
    }
}

/// A host must not turn a missing local computed context into a Brain compute
/// call. Keep the selected source and authored contour parameters explicit.
pub fn local_resource_need(dataset: &Dataset) -> Option<crate::Failure> {
    match dataset.id.as_str() {
        BUILDINGS_DATASET_ID => Some(crate::Failure::LocalResourceRequired(format!(
            "Google Open Buildings needs a pinned local Google publisher input or analytical source for version {:?}; explicitly supply it, or select Microsoft as a separate source", dataset.source_version))),
        CONTOURS_DATASET_ID => Some(crate::Failure::LocalResourceRequired(format!(
            "elevation contours need an explicitly selected local Rwanda DEM or Terrarium producer and exact surface revision/datum; authored parameters {} and version {:?} are preserved; local contour production is not yet admitted", serde_json::to_string(&dataset.parameters).unwrap_or_default(),dataset.source_version))),
        _ => None,
    }
}
