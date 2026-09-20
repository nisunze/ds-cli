//! A project's geographic context, held and read in Rust.
//!
//! The desktop assembled this from TypeScript around three Rust decision
//! kernels (`ds_command_kernel::project_dataset_cache`, `printing::context`,
//! `report_export`) and one Rust store (`ds_layer_store`). This crate is that
//! orchestration, once, for every host: the headless `ds` and, later, the
//! desktop shell call the same functions and hold the same rooms.
//!
//! What it owns, and what it does not:
//!
//! * The registry the kernel echoes — which `Dataset` a print-context source
//!   maps to ([`datasets`]). The documents are byte-identical to the desktop's
//!   so `dataset_key`, and therefore the rooms, are shared with a desktop
//!   signed in as the same account on the same machine.
//! * Catalogue validation and facts ([`catalog`]), the datasets a project
//!   declares ([`declared`]), design extents ([`extents`]).
//! * The acquisition loop ([`ensure`]): plan → open or read → assess → acquire
//!   only the gaps → commit or fail → assess again. A read never acquires; a
//!   failed cell never reads as ready; a second unchanged seed acquires nothing.
//! * National bundle installation ([`bundle`]) with the byte transfer injected
//!   by the host — this crate never opens a socket.
//! * The print-context document for one transformer ([`print_context`]),
//!   through the kernel's one recipe.
//!
//! It never decides geometry, policy or document shape — those stay in the
//! kernel — and it never fetches from a provider: the host hands it a
//! [`Provider`] for the governed ds-brain door and a fetch for bundle bytes.

pub mod bundle;
pub mod catalog;
pub mod city_vectors;
pub mod datasets;
pub mod declared;
pub mod ensure;
pub mod extents;
pub mod held;
pub mod print_context;

pub use bundle::{install_bundle, BundleReceipt};
pub use catalog::{facts, named, validate_resources, Named, ReferenceResource};
pub use datasets::{buildings_dataset, catalog_dataset, contours_dataset, ContourSettings};
pub use declared::{declared, Declared, DeclaredSource};
pub use ensure::{ensure, Acquisition, EnsureOutcome, Hosts, Mode, Provider};
pub use extents::design_extents;
pub use print_context::{read_print_context, Omission, PrintContext};

use std::fmt;

/// Why this crate could not do what it was asked. Each variant carries a
/// stable code a command declares as a refusal, and one bounded sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// A read (`Mode::Read`) found coverage the project does not hold.
    NotHeld(String),
    /// A provider or the room refused one acquisition; held data is intact.
    AcquisitionFailed(String),
    /// The governed provider door could not be reached or answered a fault.
    ProviderUnavailable(String),
    /// The catalogue row has no verified bundle, or the bundle could not be
    /// fetched, verified or installed.
    BundleUnavailable(String),
    /// A context source no headless host can supply (survey, local layer).
    Unsupported(String),
    /// The assembled document exceeds the engine's bounds.
    TooLarge(String),
    /// A catalogue row is not a valid published resource.
    CatalogInvalid(String),
    /// The local store or the kernel refused; a defect, not a state.
    Store(String),
    /// A seed or install named a cloud-resident dataset: BigQuery is its only
    /// holding and `ds data …` queries it (foundation-datasets contract R2).
    CloudOnly(String),
    /// A request named a retired layer with no single authority; the message
    /// names the detailed alternatives.
    Retired(String),
}

impl Failure {
    /// The refusal code a command declares for this failure.
    pub const fn code(&self) -> &'static str {
        match self {
            Failure::NotHeld(_) => "project_dataset_not_held",
            Failure::AcquisitionFailed(_) => "project_dataset_acquisition_failed",
            Failure::ProviderUnavailable(_) => "dataset_provider_unavailable",
            Failure::BundleUnavailable(_) => "reference_bundle_unavailable",
            Failure::Unsupported(_) => "print_context_unsupported",
            Failure::TooLarge(_) => "print_context_too_large",
            Failure::CatalogInvalid(_) => "catalog_invalid",
            Failure::Store(_) => "project_dataset_store_failed",
            Failure::CloudOnly(_) => "dataset_cloud_only",
            Failure::Retired(_) => "dataset_retired",
        }
    }

    pub fn message(&self) -> &str {
        match self {
            Failure::NotHeld(m)
            | Failure::AcquisitionFailed(m)
            | Failure::ProviderUnavailable(m)
            | Failure::BundleUnavailable(m)
            | Failure::Unsupported(m)
            | Failure::TooLarge(m)
            | Failure::CatalogInvalid(m)
            | Failure::Store(m)
            | Failure::CloudOnly(m)
            | Failure::Retired(m) => m,
        }
    }
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for Failure {}

fn store(error: impl fmt::Display) -> Failure {
    Failure::Store(error.to_string())
}

/// Read-only MV context projection using the model’s declared CRS.
pub mod mv_projection;
