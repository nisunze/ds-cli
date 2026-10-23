//! Moved to the shared report contract (`ds-report-contracts`), re-exported
//! under its original Kernel path.
pub use ds_report_contracts::report_formats::*;

#[cfg(test)]
#[path = "report_formats_save_tests.rs"]
mod save_tests;
