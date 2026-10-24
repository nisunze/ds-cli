//! Module release catalogues: the native records (`ds-module-contracts`, at
//! their public Kernel path) and the Kernel's own browser and UI unit
//! admission, which reuse the same catalogue rules.

pub use ds_module_contracts::module_release::*;

pub mod browser;
pub mod ui;
pub mod ui_build;
pub mod ui_page;

// The browser and UI admissions (`use super::*`) reuse the catalogue rules
// re-exported above and these imports.
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::{format_description::well_known::Rfc3339, OffsetDateTime};
use url::Url;
