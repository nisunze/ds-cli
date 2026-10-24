//! The columnar format contract, implemented once.
//!
//! See `ds-network/docs/contracts/columnar-format-contract.md`. This Core
//! shared crate is the single centralized converter required by
//! `NATIVE_DUCKDB_ANALYTICS_IMPLEMENTATION_PLAN.md` §0.8c: Desktop import,
//! `ds`, and the estate migration all link it, and none of them reimplements
//! the format decisions it encodes. It writes and reads formats only: the Network
//! engine reads a source into the layers it encodes.
//!
//! Two outputs, one batch builder:
//!
//! - **GeoParquet** — the analytical artifact. Converted before analysis, never
//!   during it.
//! - **Arrow IPC** — the browser working set, byte-compatible with the format
//!   `ds-cleaning::arrow_ipc` produced through Polars. Reimplementing it here
//!   is what lets a Tauri shell stop linking a dataframe engine to serialize
//!   bytes.
//!
//! Two ways in, both landing on those same decisions:
//!
//! - [`convert::layers_to_geoparquet`] encodes a source read whole. Right for
//!   a file that fits in memory; that is where its bound comes from, not a
//!   preference.
//! - [`stream`] takes bounded batches and writes the artifact incrementally, so
//!   a source larger than memory converts without ever being held whole — and
//!   [`stream::ArtifactReader`] reads one back in bounded chunks, which is what
//!   lets a converted artifact BE the source of record rather than a by-product
//!   sitting beside a second parse of the original.
//!
//! Not here, deliberately: no query engine, no SQL, no DuckDB. This crate
//! writes formats. Reading and reducing them is a later, separate decision
//! gated on measurement (§0.3).
//!
//! Elevation is not a workload for this crate. A DEM is a surface, not a table,
//! and stays a Cloud-Optimized GeoTIFF (§0.6a).

#[cfg(feature = "artifacts")]
pub mod batch;
pub mod contract;
#[cfg(feature = "artifacts")]
pub mod convert;
#[cfg(feature = "artifacts")]
pub mod features;
#[cfg(feature = "artifacts")]
pub mod geoparquet;
pub mod identity;
#[cfg(feature = "artifacts")]
pub mod ipc;
pub mod schema;
#[cfg(feature = "artifacts")]
pub mod stream;
#[cfg(feature = "artifacts")]
pub mod wkb;
#[cfg(feature = "artifacts")]
pub use batch::{FeatureBatch, Layout};
pub use contract::{GEOPARQUET_VERSION, carries_geometry};
#[cfg(feature = "artifacts")]
pub use convert::{
    Conversion, ConversionReceipt, SourceLayer, layers_to_geoparquet,
    layers_to_geoparquet_with_digest,
};
#[cfg(feature = "artifacts")]
pub use features::batch_to_features;
#[cfg(feature = "artifacts")]
pub use geoparquet::Provenance;
pub use identity::{conversion_identity, digest, digest_file};
#[cfg(feature = "artifacts")]
pub use ipc::{features_to_ipc, ipc_to_batches, ipc_to_geometry_wkb, ipc_to_rows};
pub use schema::{
    FEATURE_ID_COLUMN, IPC_GEOMETRY_COLUMN, PARQUET_GEOMETRY_COLUMN, columns_for_features,
};
#[cfg(feature = "artifacts")]
pub use stream::{
    ArtifactChunk, ArtifactReader, ArtifactSummary, ConversionStream, StreamedConversion,
    schema_digest,
};
#[cfg(feature = "artifacts")]
pub use wkb::point_xy;
