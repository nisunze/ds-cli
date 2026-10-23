//! The report and printing contract shared by the Kernel and the Reporter.
//!
//! Print layouts, style-resolution bindings, report formats and messages, the
//! Core-to-Reporter export receipt and the pure print policies both sides
//! apply to them (DECOUPLING-014: engines depend only on contracts). Nothing
//! here reads a store, a session, a file or the network, and nothing here is a
//! Kernel operation: `ds-command-kernel` re-exports these modules under their
//! original paths and keeps its own operations over them.

pub mod area;
pub mod canonical;
pub mod catalogue_aliases;
pub mod print_scaling;
pub mod printing;
pub mod project_datasets;
pub mod report_export;
pub mod report_formats;
pub mod report_messages;
pub mod style_document;
pub mod style_resolution;
pub mod style_validation;
