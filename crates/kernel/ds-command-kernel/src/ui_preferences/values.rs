//! Per-key value contracts of the UI preference store: each admits a stored or
//! offered value into its canonical form, or refuses it with `None`. A key is
//! one row of the table in `values/catalog.rs` (name, home, bound, contract);
//! its contract lives here or in its domain module under `values/`.
use serde_json::Value;

mod catalog;
pub(super) mod design;
pub(super) mod map;
pub(super) mod project_view;
pub(super) mod shape;
pub(super) mod window;
pub(super) mod workbench;

pub(super) use catalog::{admit, max_bytes, name, project_scoped};

/// A project id the Kernel admits; stored as given.
pub(super) fn active_project(value: &Value) -> Option<Value> {
    value
        .as_str()
        .filter(|project| crate::execution_context::valid_project(project))
        .map(|project| Value::String(project.to_owned()))
}
