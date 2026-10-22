//! Kernel-owned design working state, selection and mutation history.
//!
//! Hosts mount admitted feature snapshots and render projections of this state.
//! This crate performs no persistence, authorization, engineering or host IO.
//! Neutral geometry retains its existing owner and numeric implementation.

pub use ds_geo_lite::{identity, projection, region};
#[cfg(feature = "enable_dataframes")]
pub mod dataframe;
pub mod domains;
pub mod mutation;
pub mod selection;
pub mod session;
pub mod traits;
