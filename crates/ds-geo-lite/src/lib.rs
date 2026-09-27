//! ds-geo-lite — the lightweight geometry a field device must carry.
//!
//! Contract: `docs/contracts/ds-portable-geometry.md`; ruling:
//! `docs/contracts/ds-kernel-network-boundary.md`.
//!
//! Everything here answers **yes** to the ruling's litmus test — "would we
//! expect this capability to work on an Android or iOS field device without
//! shipping the heavy network engine?" Great-circle distance, GeoJSON extent,
//! exact area predicates, and revision comparison are all things a surveyor
//! standing in a field, offline, needs.
//!
//! **This crate owns no source.** Every module below is a byte-for-byte
//! snapshot of a file that is still maintained in `ds-network`, mounted from
//! `vendor/ds-geo-lite/` by `#[path]` so the bytes on disk are exactly the
//! bytes `sha256`-pinned in `vendor/ds-geo-lite/MANIFEST.json`. The one
//! exception, `haversine`, is an item-level cut and says so in its own header.
//! `tests/geo_lite_drift.rs` re-checks both digests on every `cargo test`, and
//! additionally diffs against the live `../ds-network` when it is on disk.
//! `scripts/sync-geo-lite.sh` is the only sanctioned way to refresh them.
//!
//! There is **no Cargo edge** to `ds-network` in either direction. That is the
//! point: this crate is what makes `cargo check --features wasm` succeed with
//! `ds-network` absent from the filesystem.

#[path = "../../../vendor/ds-geo-lite/geo/admission.rs"]
pub mod admission;
#[path = "../../../vendor/ds-geo-lite/geo/comparison.rs"]
pub mod comparison;
#[path = "../../../vendor/ds-geo-lite/geo/haversine.rs"]
pub mod haversine;
#[path = "../../../vendor/ds-geo-lite/geo/identity.rs"]
pub mod identity;
#[path = "../../../vendor/ds-geo-lite/geo/spatial_predicate.rs"]
pub mod spatial_predicate;
#[path = "../../../vendor/ds-geo-lite/geo/viewport.rs"]
pub mod viewport;

pub use haversine::{EARTH_RADIUS_M, haversine_distance_m, haversine_m};
