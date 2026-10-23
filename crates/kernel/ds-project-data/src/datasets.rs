//! The registry the kernel echoes: which canonical `Dataset` each print-context
//! source is held as.
//!
//! The kernel's room address is a digest of the dataset document, so these
//! documents stay byte-for-byte what earlier hosts wrote: the same account on
//! the same machine keeps reading and writing the same rooms whichever host is
//! running, and holdings seeded before the shared revisions existed keep
//! their address.

use ds_command_kernel::project_dataset_cache::{
    Dataset, Quality, BUILDINGS_DATASET_ID, CONTOURS_DATASET_ID, REFERENCE_CACHE_PROVIDER,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;

use crate::catalog::ReferenceResource;

/// The room address token building footprints and contours have been held
/// under since they were first seeded. It names no door and grants nothing:
/// the shared project revisions ([`crate::revisions`]) and Core's own
/// publisher or surface acquisition fill these rooms. Brain computes neither.
pub const PRINT_CONTEXT_ROOM_PROVIDER: &str = "ds-brain:query_print_context";

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

/// Building footprints, seeded once per project: published revisions first,
/// then Google Open Buildings tiles for what nobody has published.
pub fn buildings_dataset() -> Dataset {
    dataset(
        BUILDINGS_DATASET_ID,
        PRINT_CONTEXT_ROOM_PROVIDER,
        BTreeMap::from([("context_kind".to_owned(), json!(BUILDINGS_DATASET_ID))]),
    )
}

/// Elevation contours, seeded the same way from the project's surface. The
/// authored intervals are part of the room address and of the shared head:
/// two templates asking for different intervals hold two chains rather than
/// one answering both with the wrong lines.
pub fn contours_dataset(settings: ContourSettings) -> Dataset {
    dataset(
        CONTOURS_DATASET_ID,
        PRINT_CONTEXT_ROOM_PROVIDER,
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

/// The contour settings a contours dataset document carries.
pub fn contour_settings(dataset: &Dataset) -> Option<ContourSettings> {
    if dataset.id != CONTOURS_DATASET_ID {
        return None;
    }
    let number = |key: &str| dataset.parameters.get(key).and_then(Value::as_f64);
    let interval = |key: &str| {
        number(key)
            .filter(|value| value.fract() == 0.0 && (1.0..=1000.0).contains(value))
            .map(|value| value as u16)
    };
    Some(ContourSettings {
        minor_interval_m: interval("minor_interval_m")?,
        index_interval_m: interval("index_interval_m")?,
        sample_spacing_m: number("sample_spacing_m")?,
    })
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
