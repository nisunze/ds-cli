//! Per-key value contracts of the UI preference store: each admits a stored or
//! offered value into its canonical form, or refuses it with `None`. Keys add
//! one function here and one delegation in `Key::admit`.
use serde_json::Value;

/// A project id the Kernel admits; stored as given.
pub(super) fn active_project(value: &Value) -> Option<Value> {
    value
        .as_str()
        .filter(|project| crate::execution_context::valid_project(project))
        .map(|project| Value::String(project.to_owned()))
}
