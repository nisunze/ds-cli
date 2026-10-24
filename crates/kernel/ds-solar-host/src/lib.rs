//! The Core half of the Solar addon boundary: verified local storage, files,
//! cancellation and the bounded process transport that sends the separately
//! released `solar` addon its fixed typed tasks and verifies each reply.
//! Every calculation runs in the addon; this crate links no engine and reads
//! only `ds-solar-contracts`, the shared API of both sides.
#[cfg(not(target_arch = "wasm32"))]
pub mod cache;
pub mod cancel;
#[cfg(not(target_arch = "wasm32"))]
pub mod delivery;
#[cfg(not(target_arch = "wasm32"))]
pub mod files;
#[cfg(not(target_arch = "wasm32"))]
pub mod preparation;
#[cfg(not(target_arch = "wasm32"))]
pub mod process;
pub mod progress;
pub mod scheduling;
#[cfg(not(target_arch = "wasm32"))]
pub mod reference {
    pub use ds_solar_contracts::reference_unit::OpenedBundle;
}
pub const MAX_REFERENCE_ARTIFACT_BYTES: u64 = 16 * 1024 * 1024;

pub mod output;

pub use scheduling::{Concurrency, MAX_CONCURRENCY, compute_parallelism};
