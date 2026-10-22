//! CLI compatibility import of the owning protected Windows host adapter.
pub use ds_native_host::windows_host::*;

#[cfg(test)]
#[path = "windows_host_tests.rs"]
mod ui_store_tests;
