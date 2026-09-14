//! Design extents in the shape a coverage plan consumes.
//!
//! The extent walk is the kernel's, so the rectangle a print reads and the
//! rectangle the project's coverage was planned from come from one
//! implementation. A coverage plan always leaves the point padding at zero.

use ds_command_kernel::project_dataset_cache::Extent;
use ds_command_kernel::project_design_extent::design_extent;
use serde_json::Value;

use crate::Failure;

/// One transformer's extent from its design layers (`{layer: FeatureCollection}`).
pub fn extent_of(name: &str, layers: &Value) -> Result<Extent, Failure> {
    design_extent(name, layers, 0.0).map_err(crate::store)
}

/// Every transformer's extent from a caller-supplied roster.
pub fn design_extents<'a>(
    roster: impl IntoIterator<Item = (&'a str, &'a Value)>,
) -> Result<Vec<Extent>, Failure> {
    roster
        .into_iter()
        .map(|(name, layers)| extent_of(name, layers))
        .collect()
}
